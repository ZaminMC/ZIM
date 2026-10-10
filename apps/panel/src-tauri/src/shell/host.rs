// The shell host — where the model meets the window system.
//
// Architecture (ADR-0033): the frame webview (the window's primary
// webview, frame.html) is the VIEW of the shell; every tab's content is
// its OWN webview positioned below the frame band — Chromium's
// WebContents-per-tab model, adapted: the Tauri multi-webview host stands
// in for the Views window, and the tab webview stands in for WebContents.
// The model (tabs.rs) is authoritative; both layers render from snapshots.
//
// Every command is a browser command in the ported ID space (commands.rs);
// the drag session ports TabDragController's detach magnetism (15 DIP
// vertical, touch 50) with a drop-based tear-off adaptation documented in
// the porting spec §3.
//
// THE RE-ENTRANCY LAW (this bit is not style, it is correctness): a
// webview must never be born synchronously on the main/UI thread while
// that thread is inside an IPC callback or a window-event callback — on
// Windows WebView2 the controller creation cannot complete inside its own
// event loop turn, `shell_boot` never answers, and the frame dies as a
// blank white window. Every command that can reach [`sync`] (and so may
// create a tab webview) is therefore `async fn`: it runs on the async
// runtime, and `add_child` reaches a FREE event loop through the proxy.
// The window-event relayout is deferred the same way (main.rs).

use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};

use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, State, WebviewUrl, Window,
};

use crate::shell::bookmarks::Bookmarks;
use crate::shell::commands as cmd;
use crate::shell::layout::{self, Slot};
use crate::shell::tabs::{Destination, GroupId, Strip, Tab, TabId};

// ---------------------------------------------------------------------------
// Drag session (TabDragController, ported subset)
// ---------------------------------------------------------------------------

/// kVerticalDetachMagnetism — tab_drag_controller.cc: 15 DIP (touch 50).
const VERTICAL_DETACH_MAGNETISM: f32 = 15.0;

#[derive(Default)]
struct DragSession {
    tab: Option<TabId>,
    /// True once the pointer left the strip band beyond the magnetism —
    /// the drop becomes a tear-off (DetachIntoNewBrowserAndRunMoveLoop's
    /// drop-based adaptation, porting spec §3).
    beyond_strip: bool,
    screen_x: f32,
    screen_y: f32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct GroupView {
    id: GroupId,
    label: String,
    color: u8,
    collapsed: bool,
}

#[derive(Serialize, Clone)]
pub struct TabView {
    id: TabId,
    title: String,
    url: String,
    pinned: bool,
    muted: bool,
    group: Option<GroupId>,
    active: bool,
    /// Chromium's multi-selection paint: the tab wears the selected fill
    /// (kDefaultSelectedTabOpacity's consumer — the constant was ported
    /// with the strip and now has its rule).
    selected: bool,
    can_back: bool,
    can_forward: bool,
    /// The tab's contents zoom (Chromium's per-contents zoom factor).
    zoom: f32,
}

#[derive(Serialize, Clone)]
pub struct Snapshot {
    window: String,
    strip_width: f32,
    header_height: f32,
    bookmarks_bar_visible: bool,
    /// §54 (ADR-0026): the strip's presentation axis — the frame renders
    /// a left rail when true, the horizontal band when false.
    vertical: bool,
    slots: Vec<Slot>,
    tabs: Vec<TabView>,
    groups: Vec<GroupView>,
    active: Option<TabId>,
    address: String,
    can_reopen_closed: bool,
    bookmarks: Vec<crate::shell::bookmarks::Bookmark>,
}

#[derive(Serialize, Clone, serde::Deserialize, Default)]
struct Session {
    primary: Option<Strip>,
    #[serde(default)]
    bar_visible: bool,
}

/// The persistence snapshot: the exact bytes the pump writes, built
/// under a SHORT lock (serialization only — the disk never sees the
/// mutex).
struct Persist {
    session: Option<Vec<u8>>,
    bookmarks: Option<Vec<u8>>,
    session_path: Option<PathBuf>,
    bookmarks_path: Option<PathBuf>,
}

/// The session's persistence pump.
///
/// `save` used to run two synchronous `fs::write`s (session + bookmarks)
/// inside the state mutex on EVERY mutation — every click, every
/// reorder, every drag move — and on Windows (Defender scans each new
/// file) that read as a dead UI: every command queues behind one slow
/// disk while the write holds the lock the whole shell needs. Now
/// `ShellInner::save` only marks dirty and pokes a channel; ONE
/// background thread owns the disk, coalescing the pokes into at most
/// one write per tick. Writes are atomic (temp file + rename) so a hard
/// kill can never leave a half-written session.
const SAVE_DEBOUNCE: std::time::Duration = std::time::Duration::from_millis(400);

fn write_atomic(path: &std::path::Path, bytes: &[u8]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // The temp name carries this process's id: two ZIM processes (a
    // misbehaving launch, a stale copy) must never share one .tmp — the
    // shared name let a rename fail ("already moved") or, worse, interleave
    // two writers' bytes into one rename. With per-PID temps each
    // process's rename lands its OWN whole file; the final rename is
    // last-writer-wins on the json, which the single-instance plugin in
    // main.rs makes a two-actors-at-most scenario instead of a protocol.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub struct ShellState {
    inner: Arc<Mutex<ShellInner>>,
    /// The ONE-writer-at-a-time lock: the save pump's coalesced write and
    /// the exit path's synchronous flush both hold it across their whole
    /// persist→write span, so their temp files and renames can never
    /// interleave (the audit's P0: flush() and the pump both used the same
    /// .tmp with no barrier — a shutdown could lose or corrupt the
    /// session). The inner mutex stays the model's; this one is the disk's.
    write_lock: Arc<Mutex<()>>,
}

impl ShellState {
    /// A runtime window went away (tear-off closed): its strip dies with
    /// it. The primary window's strip IS the session — never dropped
    /// here, or a quit would wipe the restore set. Tear-off strips are
    /// not persisted (only "main" is), so this is a pure in-memory
    /// removal — no dirty mark, no disk write.
    pub fn drop_strip(&self, window_label: &str) {
        if window_label == "main" {
            return;
        }
        let mut inner = self.lock();
        inner.strips.remove(window_label);
        inner.strip_widths.remove(window_label);
    }

    fn lock(&self) -> MutexGuard<'_, ShellInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Take the persistence snapshot now (the exit path's synchronous
    /// flush; the pump owns the interactive path). The write lock makes
    /// this and the pump serial: no shared .tmp, no rename that finds
    /// the other side already moved it.
    pub fn flush(&self) {
        let _writer = self
            .write_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let persist = {
            let mut inner = self.lock();
            let persist = inner.persist();
            inner.dirty = false;
            persist
        };
        Self::write(persist);
    }

    fn write(persist: Persist) {
        if let (Some(path), Some(bytes)) = (persist.session_path, persist.session) {
            write_atomic(&path, &bytes);
        }
        if let (Some(path), Some(bytes)) = (persist.bookmarks_path, persist.bookmarks) {
            write_atomic(&path, &bytes);
        }
    }

    fn spawn_pump(
        inner: Arc<Mutex<ShellInner>>,
        write_lock: Arc<Mutex<()>>,
        rx: mpsc::Receiver<()>,
    ) {
        let _ = std::thread::Builder::new()
            .name("shell-save".into())
            .spawn(move || {
                // Each poke opens a debounce window; the pokes that land
                // inside it collapse into one write when it closes. The
                // write lock is taken before the dirty check and held
                // through the write: a flush() racing this window cannot
                // slip between the snapshot and the rename.
                while rx.recv().is_ok() {
                    std::thread::sleep(SAVE_DEBOUNCE);
                    while rx.try_recv().is_ok() {}
                    let _writer = write_lock.lock().unwrap_or_else(PoisonError::into_inner);
                    let persist = {
                        let mut guard = inner.lock().unwrap_or_else(PoisonError::into_inner);
                        if !guard.dirty {
                            continue;
                        }
                        guard.dirty = false;
                        guard.persist()
                    };
                    Self::write(persist);
                }
            });
    }
}
struct ShellInner {
    /// One strip per window; the primary window's strip is the one the
    /// session restores.
    strips: HashMap<String, Strip>,
    bookmarks: Bookmarks,
    session_path: Option<PathBuf>,
    bookmarks_path: Option<PathBuf>,
    drag: DragSession,
    next_window: u32,
    /// Set by every mutation (save()); the pump thread clears it when
    /// the coalesced write lands.
    dirty: bool,
    /// The pump's poke channel — save() sends, never blocks (the
    /// channel is unbounded; a mutation only asks for a flush).
    save_tx: mpsc::Sender<()>,
    /// The frame's viewport width per window (reported on boot/resize;
    /// each window's layout law runs on its own width — a single scalar
    /// went stale the moment a second window synced).
    strip_widths: HashMap<String, f32>,
    /// One popup overlay per window at most (menus, the group form).
    popups: HashMap<String, PopupState>,
}

impl ShellState {
    pub fn new() -> Self {
        let (save_tx, save_rx) = mpsc::channel::<()>();
        let inner = Arc::new(Mutex::new(ShellInner {
            strips: HashMap::new(),
            bookmarks: Bookmarks::default(),
            session_path: None,
            bookmarks_path: None,
            drag: DragSession::default(),
            next_window: 1,
            dirty: false,
            save_tx,
            strip_widths: HashMap::new(),
            popups: HashMap::new(),
        }));
        Self::spawn_pump(Arc::clone(&inner), Arc::new(Mutex::new(())), save_rx);
        ShellState {
            inner,
            write_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Load the session (if any) and seed the bookmarks + the primary
    /// strip. Runs before any webview asks for state.
    pub fn restore(&self, app: &AppHandle) {
        let dir = app.path().app_data_dir().expect("app data dir");
        let session_path = dir.join("shell-session.json");
        let bookmarks_path = dir.join("bookmarks.json");
        let session: Session = std::fs::read(&session_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let mut inner = self.lock();
        inner.session_path = Some(session_path);
        inner.bookmarks = Bookmarks::load(&bookmarks_path);
        inner.bookmarks_path = Some(bookmarks_path);
        inner.bookmarks.bar_visible = session.bar_visible;
        // v1 restore: the primary window's strip only (divergence
        // documented — tear-off windows keep their strips for their
        // lifetime but are not resurrected).
        inner
            .strips
            .insert("main".into(), session.primary.unwrap_or_default());
    }
}

impl ShellInner {
    /// A mutation asks for a flush: mark dirty and poke the pump. No
    /// disk here — the pump coalesces the pokes and owns every write.
    fn save(&mut self) {
        self.dirty = true;
        let _ = self.save_tx.send(());
    }

    /// Build the persistence snapshot (serialization only — the caller
    /// owns the disk). Returns None paths when restore() never ran
    /// (tests, early boot) — nothing to write then.
    fn persist(&self) -> Persist {
        let session = serde_json::to_vec(&Session {
            primary: self.strips.get("main").cloned(),
            bar_visible: self.bookmarks.bar_visible,
        })
        .ok();
        let bookmarks = serde_json::to_vec_pretty(&self.bookmarks).ok();
        Persist {
            session,
            bookmarks,
            session_path: self.session_path.clone(),
            bookmarks_path: self.bookmarks_path.clone(),
        }
    }

    fn strip(&mut self, window: &str) -> &mut Strip {
        self.strips.entry(window.to_owned()).or_default()
    }
}

// ---------------------------------------------------------------------------
// View construction
// ---------------------------------------------------------------------------

fn snapshot(inner: &ShellInner, window: &str) -> Snapshot {
    let empty = Strip::new();
    let strip = inner.strips.get(window).unwrap_or(&empty);
    let strip_width = inner.strip_widths.get(window).copied().unwrap_or(1024.0);
    let tab_tuples: Vec<(u32, bool, bool, Option<u32>)> = strip
        .tabs
        .iter()
        .map(|t| (t.id, t.pinned, false, t.group))
        .collect();
    let group_tuples: Vec<(u32, bool, &str)> = strip
        .groups
        .values()
        .map(|g| (g.id, g.collapsed, g.label.as_str()))
        .collect();
    let active = strip.active.unwrap_or(0);
    // The presentation axis picks the layout law: the horizontal band
    // runs the Chromium width rules; the rail runs full-width rows.
    let slots = if strip.vertical {
        layout::compute_layout_vertical(layout::RAIL_WIDTH, &tab_tuples, &group_tuples)
    } else {
        layout::compute_layout(strip_width, &tab_tuples, active, &group_tuples)
    };
    let tabs = strip
        .tabs
        .iter()
        .map(|t| TabView {
            id: t.id,
            title: t.destination().label(),
            url: t.destination().url(),
            pinned: t.pinned,
            muted: t.muted,
            group: t.group,
            active: strip.active == Some(t.id),
            selected: strip.selection.contains(&t.id),
            can_back: t.history_index > 0,
            can_forward: t.history_index + 1 < t.history.len(),
            zoom: t.zoom,
        })
        .collect();
    let groups = strip
        .groups
        .values()
        .map(|g| GroupView {
            id: g.id,
            label: g.label.clone(),
            color: g.color,
            collapsed: g.collapsed,
        })
        .collect();
    // The omnibox's resting text. A new tab shows NOTHING — Chrome's
    // NTP law: the address bar carries the page's address, and the new
    // tab has none to show (its placeholder speaks instead). Every other
    // destination — internal pages included — is an address worth
    // showing, because the user can have arrived here by navigation.
    // A JOIN page shows the BARE address the operator speaks —
    // `localhost:25565`, never `zim://join/localhost:25565` (the
    // founder's own words). The scheme is the shell's plumbing; the
    // operator's address is the host and the port, and the omnibox's
    // classifier re-lands that bare text on the same join anyway.
    let address = strip
        .tabs
        .iter()
        .find(|t| t.id == active)
        .map(|t| match t.destination() {
            Destination::New => String::new(),
            Destination::Join { host, port } => {
                format!("{}:{port}", host.clone().unwrap_or_default())
            }
            other => other.url(),
        })
        .unwrap_or_default();
    Snapshot {
        window: window.to_owned(),
        strip_width,
        header_height: layout::header_height(inner.bookmarks.bar_visible),
        bookmarks_bar_visible: inner.bookmarks.bar_visible,
        vertical: strip.vertical,
        slots,
        tabs,
        groups,
        active: strip.active,
        address,
        can_reopen_closed: !strip.closed.is_empty(),
        bookmarks: inner.bookmarks.items.clone(),
    }
}

/// The frame webview's label for a window: the configured main window's
/// primary webview shares the window's label; runtime tear-off windows
/// name theirs "<window>-frame".
fn frame_label(window: &str) -> String {
    if window == "main" {
        "main".into()
    } else {
        format!("{window}-frame")
    }
}

fn tab_label(window: &str, id: TabId) -> String {
    format!("tab-{window}-{id}")
}

fn parse_tab_label(label: &str) -> Option<(String, TabId)> {
    let rest = label.strip_prefix("tab-")?;
    let (window, id) = rest.rsplit_once('-')?;
    Some((window.to_owned(), id.parse().ok()?))
}

/// Which strip does this webview belong to?
fn window_label_of(webview: &tauri::Webview) -> String {
    let label = webview.label();
    if let Some((window, _)) = parse_tab_label(label) {
        return window.to_owned();
    }
    if let Some(window) = label.strip_suffix("-frame") {
        return window.to_owned();
    }
    // The popup overlay (menus, the group form) belongs to its window:
    // its verbs land on that window's strip.
    if let Some(window) = label.strip_prefix("popup-") {
        return window.to_owned();
    }
    label.to_owned()
}

/// True when this webview IS a popup overlay.
fn is_popup(webview: &tauri::Webview) -> bool {
    webview.label().starts_with("popup-")
}

fn popup_label(window: &str) -> String {
    format!("popup-{window}")
}

// ---------------------------------------------------------------------------
// The popup overlay — application-owned menus and forms
// ---------------------------------------------------------------------------

/// One open popup overlay per window. The overlay is a transparent child
/// webview floating above the content, the DOM stand-in for Chromium's
/// popup widgets (menu anchoring, focus capture, Escape-to-close all live
/// in it). The NATIVE menu is gone: an OS-drawn gray popup can neither
/// match the shell's theme nor carry its keyboard contract.
#[derive(Clone, Debug)]
struct PopupState {
    kind: String,
    tab: Option<TabId>,
    /// The group editor addresses a group, not a tab.
    group: Option<GroupId>,
    /// The hover card's dressed display payload (hoverCard.ts): the card
    /// content is the frame's law-dressed snapshot, re-dressed on every
    /// slide/update. Menus carry none.
    card: Option<serde_json::Value>,
    /// The opener's own context for the menu's live state — the frame
    /// passes the update lane's phase so the ⋮ menu's item speaks the
    /// truth of the moment it opened (Chrome's menu button badge +
    /// "Update Chromium" item ride the same shape).
    meta: Option<serde_json::Value>,
}

/// The overlay's rect for a kind anchored at (x, y): the menu's drop
/// direction flips when it would cross the window's bottom, and the x
/// clamps inside the window's width. The command layer supplies the
/// window's true logical size.
fn popup_rect(
    window_size: (f64, f64),
    kind: &str,
    x: f64,
    y: f64,
) -> (LogicalPosition<f64>, LogicalSize<f64>) {
    let (w, h) = match kind {
        "group" => (300.0, 180.0),
        // kDialogWidth 240 (tab_group_editor_bubble_view.cc): the title
        // field, the nine-color grid, and the three menu rows.
        "group-editor" => (240.0, 224.0),
        _ => (280.0, 430.0),
    };
    let (win_w, win_h) = window_size;
    let px = x.max(8.0).min((win_w - w - 8.0).max(8.0));
    let py = if y + h > win_h && y >= h { y - h } else { y };
    (
        LogicalPosition::new(px, py.max(0.0)),
        LogicalSize::new(w, h),
    )
}

// ---------------------------------------------------------------------------
// Synchronization — the model's decisions, applied to windows
// ---------------------------------------------------------------------------

/// The one sync: layout the frame band, position/visibility for every
/// tab webview, push snapshots. Every mutation funnels here — the model
/// changes, then this applies it.
///
/// THE PERSISTENCE LAW (the audit's own): a MODEL MUTATION saves, a sync
/// does not. sync() runs on every resize, every boot, every relayout —
/// geometry churn is not model churn, and the old unconditional save()
/// here serialized and re-wrote the session for every pixel dragged.
/// The verbs that mutate the model call `save()` themselves, right
/// where they mutate.
pub fn sync(app: &AppHandle, state: &ShellState, window_label: &str) -> Result<(), String> {
    let Some(host_window) = app.get_window(window_label) else {
        return Ok(()); // window gone mid-sync; nothing to lay out
    };
    let size: LogicalSize<f64> = host_window
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical(host_window.scale_factor().unwrap_or(1.0));

    let (snap, header, create_tab) = {
        let mut inner = state.lock();
        let vertical = inner.strip(window_label).vertical;
        inner.strip_widths.insert(
            window_label.to_owned(),
            // The rail's layout width IS the rail constant — the frame's
            // reported size agrees (the rail renders at RAIL_WIDTH), and
            // reorder's horizontal branch never reads it in vertical mode.
            if vertical {
                layout::RAIL_WIDTH
            } else {
                size.width as f32
            },
        );
        let header = layout::header_height(inner.bookmarks.bar_visible);
        let create_tab = {
            let strip = inner.strip(window_label);
            let active_id = strip.active;
            active_id
                .and_then(|id| strip.tabs.iter().find(|t| t.id == id))
                .filter(|_| {
                    app.get_webview(&tab_label(window_label, active_id.unwrap_or(0)))
                        .is_none()
                })
                .map(|t| (t.id, t.destination().clone(), t.reload))
        };
        let snap = snapshot(&inner, window_label);
        (snap, header, create_tab)
    };

    // The seam law (layout::snap_split): the band's bottom edge and the
    // content's top edge are ONE device row — the arithmetic and its
    // tests live with the layout's constants.
    let scale = host_window.scale_factor().unwrap_or(1.0);
    let win_w = layout::snap_to_device(size.width, scale);
    let win_h = layout::snap_to_device(size.height, scale);
    let (content_pos, content_bounds, frame_bounds) = if snap.vertical {
        let (rail, content_w) = layout::snap_split(layout::RAIL_WIDTH as f64, size.width, scale);
        (
            LogicalPosition::new(rail, 0.0),
            Rect {
                position: LogicalPosition::new(rail, 0.0).into(),
                size: LogicalSize::new(content_w, win_h).into(),
            },
            Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: LogicalSize::new(rail, win_h).into(),
            },
        )
    } else {
        let (header_log, content_h) = layout::snap_split(header as f64, size.height, scale);
        (
            LogicalPosition::new(0.0, header_log),
            Rect {
                position: LogicalPosition::new(0.0, header_log).into(),
                size: LogicalSize::new(win_w, content_h).into(),
            },
            Rect {
                position: LogicalPosition::new(0.0, 0.0).into(),
                size: LogicalSize::new(win_w, header_log).into(),
            },
        )
    };

    // Show the active tab's webview at its slot; hide the rest. Every
    // tab's own zoom rides its webview (Chromium zooms per-contents).
    for tab in &snap.tabs {
        let Some(webview) = app.get_webview(&tab_label(window_label, tab.id)) else {
            continue;
        };
        if tab.active {
            let _ = webview.set_bounds(content_bounds);
            let _ = webview.show();
            let _ = webview.set_zoom(tab.zoom as f64);
        } else {
            let _ = webview.hide();
        }
    }

    // Webview hygiene: a tab the model no longer knows (closed,
    // detached, moved between windows) must not survive as a hidden
    // child — its renderer would keep running forever. Remove every
    // orphan of this window — and its daemon wire with it (a closed
    // tab's session must not linger on the daemon).
    let live: std::collections::HashSet<String> = snap
        .tabs
        .iter()
        .map(|t| tab_label(window_label, t.id))
        .collect();
    for webview in host_window.webviews() {
        let label = webview.label();
        if label.starts_with(&format!("tab-{window_label}-")) && !live.contains(label) {
            let _ = webview.close();
            crate::drop_wire(app, label);
        }
    }

    // The active tab's webview is created lazily (WebContents born on
    // first focus — a restored session's inactive tabs stay cold).
    // The webview's own background is WHITE (Chromium's blank paint —
    // the first composited frame of a new tab): a booting page flashes
    // paper, never the window's raw surface.
    if let Some((id, destination, reload)) = create_tab {
        let label = tab_label(window_label, id);
        host_window
            .add_child(
                tauri::webview::WebviewBuilder::new(&label, WebviewUrl::App("index.html".into()))
                    .background_color(tauri::utils::config::Color(255, 255, 255, 255)),
                content_pos,
                content_bounds.size,
            )
            .map_err(|e| format!("could not create the tab webview: {e}"))?;
        let payload = serde_json::json!({
            "tab_id": id, "destination": destination, "reload": reload,
            "can_back": false, "can_forward": false,
        });
        let _ = app.emit_to(&label, "shell://tab", &payload);
    }

    // The frame webview: the header band (horizontal) or the rail
    // column (vertical). Content webviews are created later and stack
    // ABOVE the primary, so the frame shrinks itself to its pane and
    // the content owns the rest of the window.
    if let Some(frame) = app.get_webview(&frame_label(window_label)) {
        let _ = frame.set_bounds(frame_bounds);
        let _ = app.emit_to(frame_label(window_label), "shell://snapshot", &snap);
    }
    Ok(())
}

/// Push per-tab state to every tab webview of a strip (navigation,
/// history flags, reload tokens).
fn emit_tab_state(app: &AppHandle, state: &ShellState, window_label: &str) {
    let payloads: Vec<(String, serde_json::Value)> = {
        let inner = state.lock();
        let Some(strip) = inner.strips.get(window_label) else {
            return;
        };
        strip
            .tabs
            .iter()
            .map(|tab| {
                let payload = serde_json::json!({
                    "tab_id": tab.id,
                    "destination": tab.destination(),
                    "can_back": tab.history_index > 0,
                    "can_forward": tab.history_index + 1 < tab.history.len(),
                    "reload": tab.reload,
                });
                (tab_label(window_label, tab.id), payload)
            })
            .collect()
    };
    for (label, payload) in payloads {
        let _ = app.emit_to(label, "shell://tab", &payload);
    }
}

/// Window resized — geometry is model-visible state (the layout law runs
/// in the model, so the frame must be told). Deferred onto the async
/// runtime by main.rs's event hook — see the re-entrancy law above. Any
/// open popup dies with the geometry change (Chromium closes menus when
/// the window moves under them).
pub fn relayout_window(app: &AppHandle, state: &ShellState, window_label: &str) {
    dismiss_popup(app, state, window_label);
    if let Err(e) = sync(app, state, window_label) {
        tracing::debug!("relayout of {window_label} failed: {e}");
    }
}

/// Close a window's popup overlay, if one is open. Every other
/// interaction of the window routes through here first: a click, a drag,
/// an omnibox commit — the menu yields to whatever the operator did next
/// (the dismiss-then-act order also makes the anchor button's own
/// pointerdown toggle the menu, as upstream's does).
pub fn dismiss_popup(app: &AppHandle, state: &ShellState, window_label: &str) {
    let Some(_popup) = state.lock().popups.remove(window_label) else {
        return;
    };
    // The webview closes itself — tauri 2.12's child-webview removal is
    // Webview::close (the manager unregisters it with the window).
    if let Some(webview) = app.get_webview(&popup_label(window_label)) {
        let _ = webview.close();
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — the browser command surface
// ---------------------------------------------------------------------------

fn window_for(app: &AppHandle, label: &str) -> Result<Window, String> {
    app.get_window(label)
        .ok_or_else(|| format!("window {label} vanished"))
}

/// Boot: the frame asks for the world. Async per the re-entrancy law —
/// the first tab webview is born inside this call's `sync`.
#[tauri::command]
pub async fn shell_boot(
    window: tauri::Webview,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<Snapshot, String> {
    let window_name = window_label_of(&window);
    sync(&app, &state, &window_name)?;
    let inner = state.lock();
    Ok(snapshot(&inner, &window_name))
}

#[tauri::command]
pub fn shell_snapshot(
    window: tauri::Webview,
    state: State<'_, ShellState>,
) -> Result<Snapshot, String> {
    let inner = state.lock();
    Ok(snapshot(&inner, &window_label_of(&window)))
}

/// A content webview's hello: which tab am I, and what do I show?
#[tauri::command]
pub fn shell_tab_hello(
    window: tauri::Webview,
    state: State<'_, ShellState>,
) -> Result<serde_json::Value, String> {
    let label = window.label().to_owned();
    let Some((window_name, tab_id)) = parse_tab_label(&label) else {
        // Browser dev-bridge fallback: no host model — the page picks its
        // destination from ?d= (dev tooling only, never the shipped path).
        return Ok(serde_json::json!({ "tab_id": 0, "fallback": true }));
    };
    let inner = state.lock();
    let Some(strip) = inner.strips.get(&window_name) else {
        return Err("strip vanished".into());
    };
    let tab = strip
        .tabs
        .iter()
        .find(|t| t.id == tab_id)
        .ok_or("tab vanished")?;
    Ok(serde_json::json!({
        "tab_id": tab.id,
        "fallback": false,
        "destination": tab.destination(),
        "can_back": tab.history_index > 0,
        "can_forward": tab.history_index + 1 < tab.history.len(),
        "reload": tab.reload,
        // The omnibox's waiting search dialect (the query may have landed
        // before this webview existed — Alt-Enter's fresh tab): the page
        // takes it once at boot, the same text the live event carries.
        "query": tab.pending_query,
    }))
}

/// A content webview navigates (ServerView → Fleet, a Join verdict's
/// "go to the server"): the calling tab navigates — Chromium's law, the
/// same one the omnibox and every UI verb obey (the anchor tab is the
/// one under the operator's hand; no resting sibling may hijack the
/// ride).
#[tauri::command]
pub async fn shell_tab_navigate(
    window: tauri::Webview,
    destination: Destination,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    let Some((_win, tab_id)) = parse_tab_label(window.label()) else {
        return Err("no shell model in this context (dev bridge?)".into());
    };
    {
        let mut inner = state.lock();
        let strip = inner.strip(&window_name);
        strip.navigate(tab_id, destination);
        // The mutation saves — not the sync that follows it.
        inner.save();
    }
    emit_tab_state(&app, &state, &window_name);
    sync(&app, &state, &window_name)
}

/// Tab-local verbs the content layer drives: back / forward / reload.
#[tauri::command]
pub async fn shell_tab_action(
    window: tauri::Webview,
    action: String,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_owned();
    let Some((window_name, tab_id)) = parse_tab_label(&label) else {
        return Err("no shell model in this context".into());
    };
    {
        let mut inner = state.lock();
        let strip = inner.strip(&window_name);
        match action.as_str() {
            "back" => {
                strip.back(tab_id);
            }
            "forward" => {
                strip.forward(tab_id);
            }
            "reload" => {
                if let Some(t) = strip.tabs.iter_mut().find(|t| t.id == tab_id) {
                    t.reload += 1;
                }
            }
            _ => return Err(format!("unknown tab action {action}")),
        }
        inner.save();
    }
    emit_tab_state(&app, &state, &window_name);
    sync(&app, &state, &window_name)
}

/// The frame's command dispatch — one entry point, the ported ID space.
/// Async per the re-entrancy law: NEW_TAB (and restore/duplicate) can
/// create a tab webview inside `sync`.
#[tauri::command]
pub async fn shell_command(
    window: tauri::Webview,
    id: u32,
    arg: Option<serde_json::Value>,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    // A command from any webview OTHER than the popup itself dismisses
    // the popup first — the menu yields to whatever the operator did
    // next (the popup's own verbs must not kill their own overlay).
    if !is_popup(&window) {
        dismiss_popup(&app, &state, &window_name);
    }
    let arg_id = arg
        .as_ref()
        .and_then(|a| a.get("tab_id"))
        .and_then(|v| v.as_u64())
        .map(|v| v as TabId);
    let arg_group = arg
        .as_ref()
        .and_then(|a| a.get("group_id"))
        .and_then(|v| v.as_u64())
        .map(|v| v as GroupId);
    let arg_label = arg
        .as_ref()
        .and_then(|a| a.get("label"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let arg_color = arg
        .as_ref()
        .and_then(|a| a.get("color"))
        .and_then(|v| v.as_u64())
        .map(|v| v as u8);
    let arg_destination = arg.as_ref().and_then(|a| a.get("destination")).cloned();

    let mut window_verb: Option<u32> = None;
    let mut mutated = false;
    {
        let mut inner = state.lock();
        match id {
            // Bookmark arms scope the strip borrow — the bookmarks live in
            // a different field of the same state.
            cmd::BOOKMARK_THIS_TAB => {
                let info = {
                    let strip = inner.strip(&window_name);
                    strip.active.and_then(|id| {
                        strip
                            .tabs
                            .iter()
                            .find(|t| t.id == id)
                            .map(|t| (t.destination().clone(), t.destination().label()))
                    })
                };
                if let Some((destination, title)) = info {
                    inner.bookmarks.toggle(destination, title);
                    mutated = true;
                }
            }
            cmd::SHOW_BOOKMARK_BAR => {
                inner.bookmarks.toggle_bar();
                mutated = true;
            }
            // Window verbs touch the OS, not the model — deferred below.
            // NEW_WINDOW tears off a fresh window from the model too.
            // DEV_TOOLS and OPEN_LOGS are OS-facing too (devtools pane,
            // the log folder in the platform file manager). The update
            // verbs ring the frame webview — the lane's owner.
            cmd::WINDOW_MINIMIZE
            | cmd::WINDOW_TOGGLE_MAXIMIZE
            | cmd::WINDOW_CLOSE
            | cmd::FOCUS_LOCATION
            | cmd::TOGGLE_PALETTE
            | cmd::NEW_WINDOW
            | cmd::DEV_TOOLS
            | cmd::OPEN_LOGS
            | cmd::UPDATE_INSTALL
            | cmd::UPDATE_RESTART => {
                window_verb = Some(id);
            }
            _ => {
                let strip = inner.strip(&window_name);
                match id {
                    cmd::NEW_TAB => {
                        strip.append(Destination::New, true);
                    }
                    cmd::CLOSE_TAB => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            // The close BOX's law: ONE tab — the one under
                            // the press (TabStrip::CloseTab → delegate
                            // CloseTab(tab); upstream's box never touches
                            // the selection). The MENU's close commands
                            // the whole selection via CLOSE_SELECTED_TABS.
                            strip.close(target);
                        }
                    }
                    cmd::CLOSE_SELECTED_TABS => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            // GetIndicesForCommand's scope law: a SELECTED
                            // context tab commands the whole selection —
                            // the menu's "Close N tabs" closes every
                            // selected tab (tab_strip_model.cc
                            // CommandCloseTab → ExecuteCloseTabsCommand).
                            for victim in strip.indices_for_command(target) {
                                strip.close(victim);
                            }
                        }
                    }
                    cmd::SELECT_NEXT_TAB => strip.select_next(),
                    cmd::SELECT_PREVIOUS_TAB => strip.select_previous(),
                    cmd::SELECT_TAB_0..=34025 => {
                        strip.select_index((id - cmd::SELECT_TAB_0) as usize);
                    }
                    cmd::SELECT_LAST_TAB => {
                        let len = strip.tabs.len();
                        if len > 0 {
                            strip.select_index(len - 1);
                        }
                    }
                    cmd::DUPLICATE_TAB => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.duplicate(target);
                        }
                    }
                    cmd::RESTORE_TAB => {
                        strip.reopen_closed();
                    }
                    cmd::MOVE_TAB_NEXT => {
                        if let Some(a) = strip.active {
                            strip.move_relative(a, 1);
                        }
                    }
                    cmd::MOVE_TAB_PREVIOUS => {
                        if let Some(a) = strip.active {
                            strip.move_relative(a, -1);
                        }
                    }
                    cmd::ADD_NEW_TAB_TO_GROUP => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            let label = arg_label.unwrap_or_else(|| "group".into());
                            let group = strip.group_create(&[target], &label);
                            // The naming form's picked color rides along;
                            // no pick → the enum cycles on the group id.
                            if let (Some(group), Some(color)) = (group, arg_color) {
                                strip.group_set_color(group, color);
                            }
                        }
                    }
                    cmd::CLOSE_TAB_GROUP => {
                        if let Some(group) = arg_group {
                            strip.group_close(group);
                        }
                    }
                    cmd::CLOSE_OTHER_TABS => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.close_others(target);
                        }
                    }
                    cmd::CLOSE_TABS_TO_THE_RIGHT => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.close_to_right(target);
                        }
                    }
                    cmd::TOGGLE_GROUP_COLLAPSE => {
                        if let Some(group) = arg_group {
                            strip.group_toggle_collapsed(group);
                        }
                    }
                    // The group editor's verbs (tab_group_editor_bubble_view.cc
                    // writes the visual data straight from its controls; this
                    // build routes the same writes through the command lane).
                    cmd::RENAME_GROUP => {
                        if let (Some(group), Some(label)) = (arg_group, arg_label.as_deref()) {
                            strip.group_rename(group, label);
                        }
                    }
                    cmd::SET_GROUP_COLOR => {
                        if let (Some(group), Some(color)) = (arg_group, arg_color) {
                            strip.group_set_color(group, color);
                        }
                    }
                    cmd::REMOVE_TAB_FROM_GROUP => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.group_remove(target);
                        }
                    }
                    cmd::UNGROUP_GROUP => {
                        if let Some(group) = arg_group {
                            strip.group_ungroup(group);
                        }
                    }
                    cmd::NEW_TAB_IN_GROUP => {
                        if let Some(group) = arg_group {
                            strip.group_new_tab(group);
                        }
                    }
                    cmd::ADD_TAB_TO_EXISTING_GROUP => {
                        let target = arg_id.or(strip.active);
                        if let (Some(target), Some(group)) = (target, arg_group) {
                            strip.group_add(group, target);
                        }
                    }
                    // The multi-selection gestures (tab.cc:726-764's
                    // modifier branches; the model laws live in tabs.rs).
                    cmd::TOGGLE_TAB_SELECTION => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.toggle_selection(target);
                        }
                    }
                    cmd::EXTEND_TAB_SELECTION => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.extend_selection(target);
                        }
                    }
                    cmd::ADD_SELECTION_FROM_ANCHOR_TO => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            strip.add_selection_from_anchor_to(target);
                        }
                    }
                    cmd::TOGGLE_VERTICAL_STRIP => {
                        // §54 (ADR-0026): the presentation axis flips; the
                        // model, the identity rules, and the tabs themselves
                        // do not. The sync reflows the window (the frame
                        // becomes the rail, the content slides over).
                        strip.vertical = !strip.vertical;
                    }
                    cmd::NAV_BACK => {
                        if let Some(a) = strip.active {
                            strip.back(a);
                        }
                    }
                    cmd::NAV_FORWARD => {
                        if let Some(a) = strip.active {
                            strip.forward(a);
                        }
                    }
                    cmd::TOGGLE_PINNED => {
                        let target = arg_id.or(strip.active);
                        if let Some(target) = target {
                            let pinned = strip
                                .tabs
                                .iter()
                                .find(|t| t.id == target)
                                .map(|t| !t.pinned)
                                .unwrap_or(false);
                            strip.set_pinned(target, pinned);
                        }
                    }
                    cmd::TOGGLE_MUTE => {
                        let target = arg_id.or(strip.active);
                        if let Some(t) =
                            target.and_then(|id| strip.tabs.iter_mut().find(|t| t.id == id))
                        {
                            t.muted = !t.muted;
                        }
                    }
                    cmd::ZOOM_IN | cmd::ZOOM_OUT | cmd::ZOOM_RESET => {
                        // Contents zoom on the addressed tab (default:
                        // the active one), one rung along the ladder.
                        let direction = match id {
                            cmd::ZOOM_IN => 1,
                            cmd::ZOOM_OUT => -1,
                            _ => 0,
                        };
                        let target = arg_id.or(strip.active);
                        if let Some(t) =
                            target.and_then(|id| strip.tabs.iter_mut().find(|t| t.id == id))
                        {
                            t.zoom = crate::shell::tabs::zoom_step(t.zoom, direction);
                        }
                    }
                    cmd::NAVIGATE_ACTIVE => {
                        let Some(destination) = arg_destination else {
                            return Err("NAVIGATE_ACTIVE needs a destination".into());
                        };
                        let Ok(destination) = serde_json::from_value::<Destination>(destination)
                        else {
                            return Err("NAVIGATE_ACTIVE: bad destination".into());
                        };
                        // Chromium's law (the omnibox and every UI verb
                        // share it): the address a UI surface carries
                        // lands on the tab that is ALIVE UNDER THE USER'S
                        // HAND — never a teleport to some resting sibling
                        // that happens to hold the same page. The
                        // singleton-focus shape sent a bookmark click or
                        // a ⋮ menu verb flying to ANOTHER tab while the
                        // operator's own tab stayed where it was ("the
                        // app does not care about the url").
                        if let Some(active) = strip.active {
                            strip.navigate(active, destination);
                        }
                    }
                    cmd::RELOAD | cmd::RELOAD_BYPASSING_CACHE => {
                        let target = arg_id.or(strip.active);
                        if let Some(t) =
                            target.and_then(|id| strip.tabs.iter_mut().find(|t| t.id == id))
                        {
                            t.reload += 1;
                        }
                    }
                    other => return Err(format!("unknown command {other}")),
                }
                // Every arm above mutates the model (or honestly refused
                // with an Err before reaching this line) — this is where
                // the command surface's persistence lives.
                mutated = true;
            }
        }
    }
    if mutated {
        state.lock().save();
    }
    match window_verb {
        Some(cmd::WINDOW_MINIMIZE) => {
            window_for(&app, &window_name)?
                .minimize()
                .map_err(|e| e.to_string())?;
        }
        Some(cmd::WINDOW_TOGGLE_MAXIMIZE) => {
            let window = window_for(&app, &window_name)?;
            if window.is_maximized().unwrap_or(false) {
                window.unmaximize().map_err(|e| e.to_string())?;
            } else {
                window.maximize().map_err(|e| e.to_string())?;
            }
        }
        Some(cmd::WINDOW_CLOSE) => {
            window_for(&app, &window_name)?
                .close()
                .map_err(|e| e.to_string())?;
        }
        Some(cmd::FOCUS_LOCATION) => {
            let _ = app.emit_to(frame_label(&window_name), "shell://focus-address", ());
            return Ok(()); // frame-local; no model change, no sync
        }
        Some(cmd::TOGGLE_PALETTE) => {
            // The palette renders in the active tab's webview; the frame
            // only asks. Frame-local truth, no model change, no sync.
            let active = {
                let mut inner = state.lock();
                inner.strip(&window_name).active
            };
            if let Some(tab) = active {
                let _ = app.emit_to(tab_label(&window_name, tab), "shell://toggle-palette", ());
            }
            return Ok(());
        }
        Some(cmd::NEW_WINDOW) => {
            // A fresh top-level browser window with one New tab, opened
            // in a cascade so consecutive windows don't stack dead-on.
            let cascade = state.lock().next_window as f32;
            let (sx, sy) = (140.0 + 26.0 * cascade, 90.0 + 26.0 * cascade);
            spawn_tearoff(&app, &state, Destination::New, sx, sy)?;
            return Ok(());
        }
        Some(cmd::DEV_TOOLS) => {
            // The active tab's developer tools — the shell's own page,
            // the same pane Chromium opens for the inspected contents.
            let active = state.lock().strip(&window_name).active;
            if let Some(tab) = active {
                if let Some(webview) = app.get_webview(&tab_label(&window_name, tab)) {
                    webview.open_devtools();
                }
            }
            return Ok(());
        }
        Some(cmd::OPEN_LOGS) => {
            // The application's log folder: the daemon's audit.log and
            // runtime state. The opener plugin performs the OS call; the
            // folder is the shared platform data dir both sides agree on.
            use tauri_plugin_opener::OpenerExt;
            let dir = zamin_core::platform::paths::data_dir();
            let _ = std::fs::create_dir_all(&dir);
            app.opener()
                .open_path(dir.to_string_lossy().to_string(), None::<&str>)
                .map_err(|e| format!("could not open the log folder: {e}"))?;
            return Ok(());
        }
        // The update lane lives in the frame webview (the one React root
        // that survives the whole session — hidden-to-tray included);
        // the ⋮ menu's update verbs ring the frame's bell. Frame-local
        // truth, no model change, no sync.
        Some(cmd::UPDATE_INSTALL) => {
            let _ = app.emit_to(frame_label(&window_name), "shell://update-install", ());
            return Ok(());
        }
        Some(cmd::UPDATE_RESTART) => {
            let _ = app.emit_to(frame_label(&window_name), "shell://update-restart", ());
            return Ok(());
        }
        _ => {}
    }
    emit_tab_state(&app, &state, &window_name);
    sync(&app, &state, &window_name)
}

/// The omnibox classification — the view calls this on every keystroke.
#[tauri::command]
pub fn shell_omnibox_classify(text: String) -> crate::shell::omnibox::AddressRequest {
    crate::shell::omnibox::classify(&text)
}

/// The omnibox popup's rows (OmniboxPopupViewViews' matches): the typed
/// text against the frame's own fleet projection. The frame passes the
/// names the registry holds; the host classifies and orders — one
/// authority, the same classifier a commit speaks.
#[tauri::command]
pub fn shell_omnibox_suggest(
    text: String,
    servers: Option<Vec<crate::shell::omnibox::FleetServer>>,
) -> Vec<crate::shell::omnibox::Suggestion> {
    crate::shell::omnibox::suggest(&text, &servers.unwrap_or_default())
}

/// The omnibox commit: classify, then land through the model's OpenURL
/// law. Async per the re-entrancy law (a query can create a tab webview
/// inside `sync`). `new_tab` — Alt-Enter's NEW_FOREGROUND_TAB: the
/// classified request takes a fresh foreground tab instead of the hand's
/// tab (paste-and-go rides the same door with the plain disposition).
#[tauri::command]
pub async fn shell_omnibox_commit(
    window: tauri::Webview,
    text: String,
    new_tab: Option<bool>,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    use crate::shell::omnibox::{land, Landing};
    let window_name = window_label_of(&window);
    // An omnibox commit is an interaction: any open popup yields.
    dismiss_popup(&app, &state, &window_name);
    let request = crate::shell::omnibox::classify(&text);
    let (outcome, query) = {
        let mut inner = state.lock();
        let strip = inner.strip(&window_name);
        let Some(landing) = land(strip, request, new_tab.unwrap_or(false)) else {
            return Err("no active tab".into());
        };
        let outcome = match &landing {
            // Chromium's omnibox law (AutocompleteController's commit
            // path): whatever the operator typed lands on the tab THEY
            // are in — typed chrome://settings navigates THIS tab even
            // when a sibling already rests on settings. The old
            // singleton-focus shape hijacked the commit to another tab
            // ("paste the settings url and it takes you to a different
            // tab that has it enabled"); it is gone.
            Landing::Destination { .. } => serde_json::json!({ "kind": "navigated" }),
            Landing::Query { text, .. } => {
                serde_json::json!({ "kind": "query", "text": text })
            }
        };
        // The query's delivery is the MODEL's, not the timing's: the
        // text waits on the tab (`pending_query`), so a fresh Alt-Enter
        // tab — whose webview does not exist until the sync below —
        // still receives it through hello. The live emit below is the
        // fast path for a page that is already listening.
        let query = match &landing {
            Landing::Query { tab, text } => Some((tab_label(&window_name, *tab), text.clone())),
            _ => None,
        };
        // The commit mutated the active tab's history — the mutation
        // saves, here, not in the sync below.
        inner.save();
        (outcome, query)
    };
    // A live page hears the query immediately; a booting one takes the
    // same text from hello. (An emit to a webview that does not exist
    // is a no-op — the pending copy carries the delivery.)
    if let Some((label, text)) = query {
        let _ = app.emit_to(&label, "shell://discover-query", &text);
    }
    emit_tab_state(&app, &state, &window_name);
    sync(&app, &state, &window_name)?;
    Ok(outcome)
}

/// Bookmarks for the frame layer.
#[tauri::command]
pub fn shell_bookmarks(state: State<'_, ShellState>) -> Result<Bookmarks, String> {
    let inner = state.lock();
    Ok(inner.bookmarks.clone())
}

#[tauri::command]
pub async fn shell_bookmark_remove(
    window: tauri::Webview,
    id: String,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    let removed = state.lock().bookmarks.remove(&id);
    if removed {
        state.lock().save();
    }
    sync(&app, &state, &window_name)
}

/// The bar's DROP verb (bookmark_utils.cc's drop path): a URL dragged
/// onto the bookmarks bar becomes a node. The address is classified by
/// the SAME law the omnibox speaks (one dialect): an internal page or a
/// join address lands as its destination; bare search text has no
/// address to keep and is honestly refused (false). A drop never
/// removes — `add` is the law, not the star's toggle.
#[tauri::command]
pub async fn shell_bookmark_add(
    window: tauri::Webview,
    url: String,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<bool, String> {
    use crate::shell::omnibox::AddressRequest;
    let window_name = window_label_of(&window);
    let added = {
        let mut inner = state.lock();
        match crate::shell::omnibox::classify(&url) {
            AddressRequest::Internal(destination) => {
                let title = destination.label();
                inner.bookmarks.add(destination, title)
            }
            AddressRequest::Join { host, port } => {
                let destination = Destination::Join { host, port };
                let title = destination.label();
                inner.bookmarks.add(destination, title)
            }
            AddressRequest::Query(_) => false,
        }
    };
    if added {
        // The bar's model changed — the mutation saves.
        state.lock().save();
    }
    sync(&app, &state, &window_name)?;
    Ok(added)
}

/// Open a popup overlay for a window: the application-owned menu,
/// form, or hover card that replaced the NATIVE gray popup. The overlay
/// is a transparent child webview (the newest child, so it floats above
/// the content), carrying its kind and tab context in the URL for
/// [`shell_popup_boot`] to read. Any previously open popup of the window
/// dies first — one popup at a time, as upstream's widget stack allows
/// exactly one — EXCEPT the hover card: while any popup holds the lane
/// the card is refused (ScopedHideHoverCardLock's law, enforced at the
/// one-popup registry), and a menu opening kills a showing card by the
/// same dismiss-first order as always. The argument list is the popup
/// protocol's own shape (kind + anchor + context + state) — the count
/// is the surface's, not a design smell.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn shell_popup(
    window: tauri::Webview,
    kind: String,
    tab_id: Option<u32>,
    group_id: Option<u32>,
    x: f64,
    y: f64,
    card: Option<serde_json::Value>,
    meta: Option<serde_json::Value>,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<bool, String> {
    let window_name = window_label_of(&window);
    if kind == "hover-card" && state.lock().popups.contains_key(&window_name) {
        // The lock law: a popup is open — no card shows.
        return Ok(false);
    }
    let host_window = window_for(&app, &window_name)?;
    let size: LogicalSize<f64> = host_window
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical(host_window.scale_factor().unwrap_or(1.0));
    // The hover card's carrier covers exactly the payload's SLIDE BAND —
    // the window rectangle the card may roam (the frame's law), clamped
    // into the window. Slides stay inside the band; everything outside
    // it keeps its own pointer. Menus keep popup_rect's law.
    let (position, popup_size) = if kind == "hover-card" {
        let band = card
            .as_ref()
            .and_then(|c| c.get("band"))
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let field = |key: &str| {
            band.get(key)
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                .max(0.0)
        };
        let (bx, by, bw, bh) = (field("x"), field("y"), field("w"), field("h"));
        (
            LogicalPosition::new(bx, by),
            LogicalSize::new(
                bw.max(1.0).min((size.width - bx).max(1.0)),
                bh.max(1.0).min((size.height - by).max(1.0)),
            ),
        )
    } else {
        popup_rect((size.width, size.height), &kind, x, y)
    };
    dismiss_popup(&app, &state, &window_name);
    let label = popup_label(&window_name);
    // THE BOOT ORDER LAW (the audit's P0): the state lands FIRST, the
    // webview second. `shell_popup_boot` reads `popups[window]` from the
    // popup's own load path — a webview created before its PopupState
    // existed could boot into kind="unknown" whenever its page loaded
    // fast enough to win the race against the insert. With the state
    // established before `add_child`, no boot can arrive early; if the
    // webview itself fails to open, the state is rolled back so the
    // registry never holds a popup with no widget behind it.
    let is_card = kind == "hover-card";
    state.lock().popups.insert(
        window_name.clone(),
        PopupState {
            kind: kind.clone(),
            tab: tab_id,
            group: group_id.map(|g| g as GroupId),
            card: if is_card { card.clone() } else { None },
            meta: meta.clone(),
        },
    );
    if let Err(e) = host_window.add_child(
        tauri::webview::WebviewBuilder::new(&label, WebviewUrl::App("popup.html".into()))
            .transparent(true),
        position,
        popup_size,
    ) {
        state.lock().popups.remove(&window_name);
        return Err(format!("could not open the popup: {e}"));
    }
    let _ = app.emit_to(
        &label,
        "shell://popup-boot",
        serde_json::json!({ "kind": kind, "tab_id": tab_id, "group_id": group_id, "meta": meta }),
    );
    // Focus follows the popup: Escape and the arrow keys work from the
    // first keystroke, exactly like a freshly opened OS menu. The hover
    // card breaks the rule on purpose — SetCanActivate(false) +
    // set_accept_events(false) upstream: the card never takes the
    // focus, it only paints (webview hosting has no click-through, so
    // the card also yields any press on its own band — PopupApp's law).
    if let Some(popup) = app.get_webview(&label) {
        if !is_card {
            let _ = popup.set_focus();
        }
    }
    Ok(true)
}

/// The overlay asks for its context: kind, the addressed tab, and the
/// menu-relevant posture of that tab (pinned/muted/zoom) plus the
/// window's bookmarks-bar state for the app menu's checkmark.
#[tauri::command]
pub fn shell_popup_boot(
    window: tauri::Webview,
    state: State<'_, ShellState>,
) -> Result<serde_json::Value, String> {
    let window_name = window_label_of(&window);
    let (kind, tab_id, group_id, meta) = {
        let inner = state.lock();
        match inner.popups.get(&window_name) {
            Some(popup) => (
                popup.kind.clone(),
                popup.tab,
                popup.group,
                popup.meta.clone(),
            ),
            None => ("unknown".into(), None, None, None),
        }
    };
    let mut context = serde_json::json!({ "kind": kind, "tab_id": tab_id, "group_id": group_id });
    // The opener's live state (the update lane's phase, app-menu only)
    // rides verbatim — the host never re-dresses it.
    if let Some(meta) = meta {
        context["meta"] = meta;
    }
    if kind == "hover-card" {
        // The card's content is the frame's dressed payload, relayed
        // verbatim — the host never re-dresses it (one law home).
        let inner = state.lock();
        if let Some(popup) = inner.popups.get(&window_name) {
            context["card"] = popup.card.clone().unwrap_or(serde_json::Value::Null);
        }
        return Ok(context);
    }
    if kind == "group-editor" {
        if let Some(group_id) = group_id {
            let inner = state.lock();
            if let Some(strip) = inner.strips.get(&window_name) {
                if let Some(group) = strip.groups.get(&group_id) {
                    context["label"] = serde_json::json!(group.label);
                    context["color"] = serde_json::json!(group.color);
                    context["collapsed"] = serde_json::json!(group.collapsed);
                }
            }
        }
    } else if let Some(tab_id) = tab_id {
        let inner = state.lock();
        if let Some(strip) = inner.strips.get(&window_name) {
            // The tab menu's presentation verb labels itself from the
            // strip's current axis (§54).
            context["vertical"] = serde_json::json!(strip.vertical);
            if let Some(tab) = strip.tabs.iter().find(|t| t.id == tab_id) {
                context["pinned"] = serde_json::json!(tab.pinned);
                context["muted"] = serde_json::json!(tab.muted);
                context["zoom"] = serde_json::json!(tab.zoom);
                context["grouped"] = serde_json::json!(tab.group);
                // The close item's plural law: the CONTEXT tab's posture
                // in the selection decides whether the menu says "Close
                // tab" or "Close N tabs" (tab_menu_model.cc's
                // IDS_TAB_CXMENU_CLOSETAB plural — GetIndicesForCommand's
                // scope is what the command will act through).
                context["tab_selected"] = serde_json::json!(strip.selection.contains(&tab_id));
                context["selection_size"] = serde_json::json!(strip.selection.len());
            }
            // "Add to existing group" (tab_menu_model.cc's submenu): the
            // menu carries the strip's groups — id, name, and the color
            // index the menu's dot renders from.
            let mut groups: Vec<serde_json::Value> = strip
                .groups
                .values()
                .map(|g| serde_json::json!({ "id": g.id, "label": g.label, "color": g.color }))
                .collect();
            groups.sort_by_key(|g| g["id"].as_u64());
            context["groups"] = serde_json::json!(groups);
        }
        context["bar_visible"] = serde_json::json!(state.lock().bookmarks.bar_visible);
    } else {
        let inner = state.lock();
        context["bar_visible"] = serde_json::json!(inner.bookmarks.bar_visible);
        // The app menu's zoom row reads the ACTIVE tab's factor.
        if let Some(strip) = inner.strips.get(&window_name) {
            if let Some(active) = strip
                .active
                .and_then(|id| strip.tabs.iter().find(|t| t.id == id))
            {
                context["zoom"] = serde_json::json!(active.zoom);
            }
        }
    }
    Ok(context)
}

/// The overlay closes itself (verb dispatched, Escape, click in its own
/// transparent gutter) — or any other webview of the window asks for
/// dismissal through [`shell_popup_dismiss`].
#[tauri::command]
pub fn shell_popup_close(
    window: tauri::Webview,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    dismiss_popup(&app, &state, &window_name);
    Ok(())
}

/// Slide or refresh the LIVE hover card: the frame's new dressed payload
/// replaces the stored one and rides to the overlay. False means no
/// card is alive (the fade won the race) — the frame's next show
/// recreates the widget. The payload's BAND is the carrier's own rect
/// (hoverCardBand's corridor law) — when it moved, the overlay re-bounds
/// before the content lands, so the slide's anchors stay covered and the
/// rest of the window keeps its own pointer.
#[tauri::command]
pub fn shell_popup_update(
    window: tauri::Webview,
    card: serde_json::Value,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<bool, String> {
    let window_name = window_label_of(&window);
    {
        let mut inner = state.lock();
        match inner.popups.get_mut(&window_name) {
            Some(popup) if popup.kind == "hover-card" => popup.card = Some(card.clone()),
            _ => return Ok(false),
        }
    }
    // The band moved → the carrier follows it (the popup re-bases its
    // render against the band's origin, PopupApp's coordinate law).
    let band = card.get("band").cloned().unwrap_or(serde_json::Value::Null);
    let field = |key: &str| {
        band.get(key)
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
            .max(0.0)
    };
    if let Some(popup) = app.get_webview(&popup_label(&window_name)) {
        let window = window_for(&app, &window_name)?;
        let size: LogicalSize<f64> = window
            .inner_size()
            .map_err(|e| e.to_string())?
            .to_logical(window.scale_factor().unwrap_or(1.0));
        let (bx, by, bw, bh) = (field("x"), field("y"), field("w"), field("h"));
        let _ = popup.set_bounds(Rect {
            position: LogicalPosition::new(bx, by).into(),
            size: LogicalSize::new(
                bw.max(1.0).min((size.width - bx).max(1.0)),
                bh.max(1.0).min((size.height - by).max(1.0)),
            )
            .into(),
        });
    }
    let _ = app.emit_to(popup_label(&window_name), "shell://popup-update", card);
    Ok(true)
}

/// Order the live hover card to fade out (the HideHoverCard law: 200ms
/// out, THEN the widget closes — the overlay plays the fade and asks
/// [`shell_popup_close`] when it lands). False = nothing alive to fade.
#[tauri::command]
pub fn shell_popup_fade(
    window: tauri::Webview,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<bool, String> {
    let window_name = window_label_of(&window);
    let is_card = {
        let inner = state.lock();
        inner
            .popups
            .get(&window_name)
            .map(|p| p.kind == "hover-card")
            .unwrap_or(false)
    };
    if !is_card {
        return Ok(false);
    }
    let _ = app.emit_to(
        popup_label(&window_name),
        "shell://popup-fade",
        serde_json::json!({}),
    );
    Ok(true)
}

/// Dismissal from the FRAME or a CONTENT webview ("I was clicked — the
/// menu must yield"). A no-op when nothing is open, so it rides every
/// pointerdown cheaply.
#[tauri::command]
pub fn shell_popup_dismiss(
    window: tauri::Webview,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    dismiss_popup(&app, &state, &window_name);
    Ok(())
}

/// The drag session — frame pointer events in, model decisions out.
/// Async per the re-entrancy law: a detached drop spawns a whole WINDOW
/// (plus its frame webview) from the tear-off path.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // the command surface is contractual
pub async fn shell_drag(
    window: tauri::Webview,
    phase: String,
    tab_id: Option<TabId>,
    x: Option<f32>,
    y: Option<f32>,
    screen_x: Option<f32>,
    screen_y: Option<f32>,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    // A drag session is an interaction: any open popup yields.
    dismiss_popup(&app, &state, &window_name);
    match phase.as_str() {
        "start" => {
            let mut inner = state.lock();
            inner.drag.tab = tab_id;
            inner.drag.beyond_strip = false;
            inner.drag.screen_x = screen_x.unwrap_or(0.0);
            inner.drag.screen_y = screen_y.unwrap_or(0.0);
            Ok(())
        }
        "move" => {
            let mut inner = state.lock();
            if inner.drag.tab == tab_id {
                // The detach magnetism runs on the strip's OWN axis: the
                // horizontal band watches the pointer's Y leave the 41px
                // band; the rail watches the pointer's X leave the rail
                // column. Either way past ±15 DIP is a tear-off posture.
                let vertical = inner.strip(&window_name).vertical;
                if let (Some(x), Some(y)) = (x, y) {
                    let beyond = if vertical {
                        let rail = layout::RAIL_WIDTH;
                        x > rail + VERTICAL_DETACH_MAGNETISM || x < -VERTICAL_DETACH_MAGNETISM
                    } else {
                        let strip_h = layout::TAB_HEIGHT + layout::STRIP_PADDING;
                        y > strip_h + VERTICAL_DETACH_MAGNETISM || y < -VERTICAL_DETACH_MAGNETISM
                    };
                    if beyond {
                        inner.drag.beyond_strip = true;
                    }
                }
                inner.drag.screen_x = screen_x.unwrap_or(0.0);
                inner.drag.screen_y = screen_y.unwrap_or(0.0);
            }
            Ok(())
        }
        "drop" => {
            let decision = {
                let mut inner = state.lock();
                let tab = inner.drag.tab.take();
                let detached = inner.drag.beyond_strip;
                inner.drag.beyond_strip = false;
                (
                    detached,
                    tab.or(tab_id),
                    x,
                    screen_x.unwrap_or(inner.drag.screen_x),
                    screen_y.unwrap_or(inner.drag.screen_y),
                )
            };
            let (detached, id, x, sx, sy) = decision;
            let Some(id) = id else { return Ok(()) };
            if detached {
                // DetachIntoNewBrowserAndRunMoveLoop, drop-based
                // adaptation: first hit-test the OTHER windows' strip
                // bands — a drop over one of them MOVES the drag's block
                // there (drag between windows).
                if let Some((target, local_x)) = window_strip_at(&app, &state, &window_name, sx, sy)
                {
                    return move_block_between_windows(
                        &app,
                        &state,
                        &window_name,
                        &target,
                        id,
                        local_x,
                    );
                }
                // A drop inside the SOURCE window — its content area
                // included — never tears off: the tab returns to the
                // source strip. Chromium tears off only when the drop
                // lands OFF the browser window entirely; a drag that
                // dipped past the strip band and released back inside
                // the window is a dock-back, not a new window (the
                // "my tab vanished into a spawned window" disease).
                if window_contains(&app, &window_name, sx, sy) {
                    return reorder_in_strip(&app, &state, &window_name, id, x);
                }
                // The tear-off carries the WHOLE BLOCK the drag carried
                // (drag_block's law): the selected tabs leave together —
                // never the one-tab-under-the-hand of the audit's own
                // "selected B,C, arrived B" — and the hand's tab
                // activates in the new window.
                let block = {
                    let mut inner = state.lock();
                    let strip = inner.strip(&window_name);
                    let ids = strip.drag_block(id);
                    let block = strip.detach_block(&ids);
                    // The source strip just lost real tabs — the
                    // mutation saves (the tear-off's own strip is not
                    // persisted; nothing saves for it).
                    inner.save();
                    block
                };
                if block.is_empty() {
                    return Err("tab vanished".into());
                }
                spawn_tearoff_block(&app, &state, block, id, sx, sy)
            } else {
                reorder_in_strip(&app, &state, &window_name, id, x)
            }
        }
        "cancel" => {
            let mut inner = state.lock();
            inner.drag.tab = None;
            inner.drag.beyond_strip = false;
            Ok(())
        }
        _ => Err(format!("unknown drag phase {phase}")),
    }
}

/// The in-strip reorder — the drop's shared tail (the plain drop and
/// the dock-back both land here). The model axis runs the axis-matched
/// drop_index over the slots MINUS the dragged tab (the lift-out rule),
/// the model reorders, and the sync re-lays the window out. The frame
/// sends `axis` as the coordinate along the strip's own presentation
/// axis (pointer X horizontal, pointer Y in the rail), the lane's
/// scroll shift included.
fn reorder_in_strip(
    app: &AppHandle,
    state: &ShellState,
    window_name: &str,
    id: TabId,
    axis: Option<f32>,
) -> Result<(), String> {
    {
        let mut inner = state.lock();
        // The width reads before the strip's mutable borrow:
        // the guard can't serve both at once (E0502's law).
        let vertical = inner.strip(window_name).vertical;
        let strip_width = inner
            .strip_widths
            .get(window_name)
            .copied()
            .unwrap_or(1024.0);
        let slots = {
            let strip = inner.strip(window_name);
            let tab_tuples: Vec<(u32, bool, bool, Option<u32>)> = strip
                .tabs
                .iter()
                .map(|t| (t.id, t.pinned, false, t.group))
                .collect();
            let group_tuples: Vec<(u32, bool, &str)> = strip
                .groups
                .values()
                .map(|g| (g.id, g.collapsed, g.label.as_str()))
                .collect();
            let active = strip.active.unwrap_or(0);
            if vertical {
                layout::compute_layout_vertical(layout::RAIL_WIDTH, &tab_tuples, &group_tuples)
            } else {
                layout::compute_layout(strip_width, &tab_tuples, active, &group_tuples)
            }
        };
        if let Some(axis) = axis {
            // The lift-out rule: the insertion index runs over the
            // REMAINING tabs — the dragged tab's own slot never
            // participates in its own drop verdict. A source standing
            // in a multi-selection lifts the WHOLE selection out
            // (MaybeStartDrag dragged them together — the drop verdict
            // runs over the strip minus the block; reorder_drop makes
            // the same block call on the model side).
            let block: Vec<u32> = {
                let strip = inner.strip(window_name);
                if strip.selection.contains(&id) && strip.selection.len() > 1 {
                    strip.selection.iter().copied().collect()
                } else {
                    vec![id]
                }
            };
            let others: Vec<layout::Slot> = slots
                .iter()
                .filter(|s| !s.header && !block.contains(&s.id))
                .cloned()
                .collect();
            let index = if vertical {
                layout::drop_index_vertical(&others, axis)
            } else {
                layout::drop_index(&others, axis)
            };
            inner.strip(window_name).reorder_drop(id, index);
        }
    }
    // The drop re-ordered the model — the mutation saves.
    state.lock().save();
    emit_tab_state(app, state, window_name);
    sync(app, state, window_name)
}

/// Does this screen point land inside the window's outer rect? Logical
/// units throughout — the frame's screenX/screenY are CSS pixels, the
/// window's physical rect divides by its own scale factor.
fn window_contains(app: &AppHandle, label: &str, screen_x: f32, screen_y: f32) -> bool {
    let Some(window) = app.get_window(label) else {
        return false;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let Ok(outer) = window.outer_position() else {
        return false;
    };
    let Ok(size) = window.outer_size() else {
        return false;
    };
    let x = outer.x as f32 / scale as f32;
    let y = outer.y as f32 / scale as f32;
    let w = size.width as f32 / scale as f32;
    let h = size.height as f32 / scale as f32;
    screen_x >= x && screen_x <= x + w && screen_y >= y && screen_y <= y + h
}

/// Which OTHER window's strip band does this screen point land on, and
/// where within it? (The drag-between-windows drop: Chromium tears off
/// into a NEW window only when the drop lands on no existing strip.)
/// Returns the target window's label and the pointer's coordinate along
/// that strip's OWN presentation axis, in logical units.
fn window_strip_at(
    app: &AppHandle,
    state: &ShellState,
    source: &str,
    screen_x: f32,
    screen_y: f32,
) -> Option<(String, f32)> {
    let names: Vec<String> = {
        let inner = state.lock();
        inner
            .strips
            .keys()
            .filter(|k| k.as_str() != source)
            .cloned()
            .collect()
    };
    for name in names {
        let Some(window) = app.get_window(&name) else {
            continue;
        };
        let scale = window.scale_factor().unwrap_or(1.0);
        let Ok(outer) = window.outer_position() else {
            continue;
        };
        let Ok(size) = window.outer_size() else {
            continue;
        };
        let (vertical, band) = {
            let inner = state.lock();
            let vertical = inner.strips.get(&name).map(|s| s.vertical).unwrap_or(false);
            (vertical, layout::header_height(inner.bookmarks.bar_visible))
        };
        let x = outer.x as f32 / scale as f32;
        let y = outer.y as f32 / scale as f32;
        let w = size.width as f32 / scale as f32;
        let h = size.height as f32 / scale as f32;
        // The drop band is the strip's OWN pane: the horizontal band
        // spans the window's width for the header's height; the rail
        // spans the window's height for the rail's width.
        let over = if vertical {
            screen_x >= x
                && screen_x <= x + layout::RAIL_WIDTH
                && screen_y >= y
                && screen_y <= y + h
        } else {
            screen_x >= x && screen_x <= x + w && screen_y >= y && screen_y <= y + band
        };
        if over {
            // The axis coordinate the TARGET's drop law consumes.
            let axis = if vertical { screen_y - y } else { screen_x - x };
            return Some((name, axis));
        }
    }
    None
}

/// The between-windows move — of the WHOLE DRAG BLOCK (the audit's P0:
/// the old path detached one tab out of a selected block, so dragging
/// B+C landed only B). The block leaves the source in strip order
/// (detach_block's single-pass removal) and re-inserts as a block at
/// the drop point of the target strip; the pinned members re-pin at the
/// target's pinned edge (insert never pins — set_pinned owns the
/// relocation), and the landing selection is the block with the hand's
/// tab active and anchoring (select_block). Group membership does not
/// survive a window change — detach_block already ungrouped, the same
/// documented divergence the single detach speaks. History, muted,
/// zoom ride intact; the ids mint fresh from the TARGET's own counter
/// (window id spaces are independent). No second server process is
/// possible — destinations are views; the daemon owns the processes.
fn move_block_between_windows(
    app: &AppHandle,
    state: &ShellState,
    source: &str,
    target: &str,
    dragged: TabId,
    local_axis: f32,
) -> Result<(), String> {
    let block = {
        let mut inner = state.lock();
        let strip = inner
            .strips
            .get_mut(source)
            .ok_or("source strip vanished")?;
        let ids = strip.drag_block(dragged);
        let block = strip.detach_block(&ids);
        inner.save();
        block
    };
    if block.is_empty() {
        return Err("tab vanished".into());
    }
    {
        let mut inner = state.lock();
        let vertical = inner
            .strips
            .get(target)
            .map(|s| s.vertical)
            .unwrap_or(false);
        let strip_width = inner.strip_widths.get(target).copied().unwrap_or(1024.0);
        let drop_index = {
            let strip = inner.strip(target);
            let tab_tuples: Vec<(u32, bool, bool, Option<u32>)> = strip
                .tabs
                .iter()
                .map(|t| (t.id, t.pinned, false, t.group))
                .collect();
            let group_tuples: Vec<(u32, bool, &str)> = strip
                .groups
                .values()
                .map(|g| (g.id, g.collapsed, g.label.as_str()))
                .collect();
            let active = strip.active.unwrap_or(0);
            let slots = if vertical {
                layout::compute_layout_vertical(layout::RAIL_WIDTH, &tab_tuples, &group_tuples)
            } else {
                layout::compute_layout(strip_width, &tab_tuples, active, &group_tuples)
            };
            if vertical {
                layout::drop_index_vertical(&slots, local_axis)
            } else {
                layout::drop_index(&slots, local_axis)
            }
        };
        let strip = inner.strip(target);
        let at = drop_index.min(strip.tabs.len());
        let mut new_ids: Vec<TabId> = Vec::with_capacity(block.len());
        let mut pinned_ids: Vec<TabId> = Vec::new();
        for (offset, tab) in block.iter().enumerate() {
            let new_id = strip.insert(at + offset, tab.destination().clone(), false);
            // The insert minted a fresh tab id (the model's own business);
            // carry the DetachedTab's history, posture, and zoom.
            if let Some(inserted) = strip.tabs.iter_mut().find(|t| t.id == new_id) {
                inserted.history = tab.history.clone();
                inserted.history_index = tab.history_index;
                inserted.muted = tab.muted;
                inserted.zoom = tab.zoom;
            }
            if tab.pinned {
                pinned_ids.push(new_id);
            }
            new_ids.push(new_id);
        }
        for new_id in pinned_ids {
            strip.set_pinned(new_id, true);
        }
        // The hand's tab anchors the landing selection: its position in
        // the strip-order block maps onto the fresh ids one for one.
        let dragged_new = block
            .iter()
            .position(|t| t.id == dragged)
            .and_then(|i| new_ids.get(i).copied())
            .unwrap_or_else(|| *new_ids.last().expect("non-empty block above"));
        strip.select_block(dragged_new, &new_ids);
        // Both strips changed — the mutations save.
        inner.save();
    }
    emit_tab_state(app, state, source);
    emit_tab_state(app, state, target);
    sync(app, state, source)?;
    sync(app, state, target)
}

/// A tear-off: a new window with its own frame and the detached BLOCK.
/// The tabs land with their identities intact (ids preserved — the
/// window label is fresh, so the `tab-{window}-{id}` space cannot
/// collide); the hand's tab activates, the rest of the block stays
/// selected. `dragged` is the tab the hand carried at release.
fn spawn_tearoff_block(
    app: &AppHandle,
    state: &ShellState,
    block: Vec<Tab>,
    dragged: TabId,
    screen_x: f32,
    screen_y: f32,
) -> Result<(), String> {
    let label = {
        let mut inner = state.lock();
        let n = inner.next_window;
        inner.next_window += 1;
        let label = format!("win-{n}");
        let mut strip = Strip::new();
        // The seed New tab steps aside; the block replaces it whole.
        strip.tabs.clear();
        strip.selection.clear();
        strip.anchor = None;
        strip.active = None;
        let mut max_id = 0;
        for tab in block {
            max_id = max_id.max(tab.id);
            strip.tabs.push(tab);
        }
        strip.next_tab = max_id + 1;
        strip.select_block(dragged, &[]);
        inner.strips.insert(label.clone(), strip);
        label
    };
    let window = tauri::window::WindowBuilder::new(app, &label)
        .decorations(false)
        .inner_size(1100.0, 720.0)
        // The tear-off's raw surface is the strip gray, not black —
        // booting webviews flash paper and chrome, never the void.
        .background_color(tauri::utils::config::Color(222, 225, 230, 255))
        .build()
        .map_err(|e| format!("tear-off window failed: {e}"))?;
    let _ = window.set_position(LogicalPosition::new(
        ((screen_x - 120.0).max(0.0)) as f64,
        ((screen_y - 20.0).max(0.0)) as f64,
    ));
    window
        .add_child(
            tauri::webview::WebviewBuilder::new(
                frame_label(&label),
                WebviewUrl::App("frame.html".into()),
            )
            .background_color(tauri::utils::config::Color(222, 225, 230, 255)),
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1100.0, 83.0),
        )
        .map_err(|e| format!("tear-off frame failed: {e}"))?;
    // The content webviews are created by sync() once the tear-off's
    // frame reports its size (shell_boot) — the active tab first, the
    // rest of the block stays cold until selected, exactly the
    // restore-session law.
    sync(app, state, &label)
}

/// A NEW_WINDOW tear-off: one fresh tab, minted by the model's own
/// Strip::new, then the same block path the drag speaks.
fn spawn_tearoff(
    app: &AppHandle,
    state: &ShellState,
    destination: Destination,
    screen_x: f32,
    screen_y: f32,
) -> Result<(), String> {
    let mut strip = Strip::new();
    strip.tabs[0].history = vec![destination];
    let dragged = strip.tabs[0].id;
    spawn_tearoff_block(app, state, strip.tabs, dragged, screen_x, screen_y)
}

/// The frame layer reports its window size on resize (belt and braces
/// with the native Resized event, which misses webview-level relayouts
/// during live drags on some platforms). Async per the re-entrancy law.
#[tauri::command]
pub async fn shell_window_resized(
    window: Window,
    width: f64,
    height: f64,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let label = window.label().to_owned();
    {
        let mut inner = state.lock();
        inner.strip_widths.insert(label.clone(), width as f32);
        let _ = height;
    }
    sync(&app, &state, &label)
}
