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
use std::sync::{
    mpsc, Arc, Mutex, MutexGuard, PoisonError,
};

use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, State, WebviewUrl, Window,
};

use crate::shell::bookmarks::Bookmarks;
use crate::shell::commands as cmd;
use crate::shell::layout::{self, Slot};
use crate::shell::tabs::{Destination, GroupId, Strip, TabId};

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
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

pub struct ShellState {
    inner: Arc<Mutex<ShellInner>>,
}

impl ShellState {
    /// A runtime window went away (tear-off closed): its strip dies with
    /// it. The primary window's strip IS the session — never dropped
    /// here, or a quit would wipe the restore set.
    pub fn drop_strip(&self, window_label: &str) {
        if window_label == "main" {
            return;
        }
        let mut inner = self.lock();
        inner.strips.remove(window_label);
        inner.strip_widths.remove(window_label);
        inner.save();
    }

    fn lock(&self) -> MutexGuard<'_, ShellInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Take the persistence snapshot now (the exit path's synchronous
    /// flush; the pump owns the interactive path).
    pub fn flush(&self) {
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

    fn spawn_pump(inner: Arc<Mutex<ShellInner>>, rx: mpsc::Receiver<()>) {
        let _ = std::thread::Builder::new()
            .name("shell-save".into())
            .spawn(move || {
                // Each poke opens a debounce window; the pokes that land
                // inside it collapse into one write when it closes.
                while rx.recv().is_ok() {
                    std::thread::sleep(SAVE_DEBOUNCE);
                    while rx.try_recv().is_ok() {}
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
        Self::spawn_pump(Arc::clone(&inner), save_rx);
        ShellState { inner }
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
    let slots = layout::compute_layout(strip_width, &tab_tuples, active, &group_tuples);
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
    let address = strip
        .tabs
        .iter()
        .find(|t| t.id == active)
        .map(|t| match t.destination() {
            Destination::New => String::new(),
            other => other.url(),
        })
        .unwrap_or_default();
    Snapshot {
        window: window.to_owned(),
        strip_width,
        header_height: layout::header_height(inner.bookmarks.bar_visible),
        bookmarks_bar_visible: inner.bookmarks.bar_visible,
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
/// tab webview, push snapshots, persist the session. Every mutation
/// funnels here — the model changes, then this applies it.
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
        inner
            .strip_widths
            .insert(window_label.to_owned(), size.width as f32);
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
        inner.save();
        (snap, header, create_tab)
    };

    let content_bounds = Rect {
        position: LogicalPosition::new(0.0, header as f64).into(),
        // `size` is already LogicalSize<f64> — the unit was named when
        // the annotation landed; no cast to restate it.
        size: LogicalSize::new(size.width, (size.height - header as f64).max(0.0)).into(),
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
    // orphan of this window.
    let live: std::collections::HashSet<String> = snap
        .tabs
        .iter()
        .map(|t| tab_label(window_label, t.id))
        .collect();
    for webview in host_window.webviews() {
        let label = webview.label();
        if label.starts_with(&format!("tab-{window_label}-")) && !live.contains(label) {
            let _ = webview.close();
        }
    }

    // The active tab's webview is created lazily (WebContents born on
    // first focus — a restored session's inactive tabs stay cold).
    if let Some((id, destination, reload)) = create_tab {
        let label = tab_label(window_label, id);
        host_window
            .add_child(
                tauri::webview::WebviewBuilder::new(&label, WebviewUrl::App("index.html".into())),
                LogicalPosition::new(0.0, header as f64),
                LogicalSize::new(size.width, (size.height - header as f64).max(0.0)),
            )
            .map_err(|e| format!("could not create the tab webview: {e}"))?;
        let payload = serde_json::json!({
            "tab_id": id, "destination": destination, "reload": reload,
            "can_back": false, "can_forward": false,
        });
        let _ = app.emit_to(&label, "shell://tab", &payload);
    }

    // The frame webview: the header band. Content webviews are created
    // later and stack ABOVE the primary, so the frame shrinks itself to
    // the header and the content owns the rest of the window.
    if let Some(frame) = app.get_webview(&frame_label(window_label)) {
        let _ = frame.set_bounds(Rect {
            position: LogicalPosition::new(0.0, 0.0).into(),
            size: LogicalSize::new(size.width, header as f64).into(),
        });
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
    }))
}

/// A content webview navigates (ServerView → Fleet, §61 singleton
/// identity: an existing resting tab of the same identity is focused).
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
        let identity = destination_identity(&destination);
        if let Some(existing) = strip
            .tabs
            .iter()
            .find(|t| destination_identity(t.destination()) == identity)
            .map(|t| t.id)
        {
            strip.select(existing);
        } else {
            strip.navigate(tab_id, destination);
        }
    }
    emit_tab_state(&app, &state, &window_name);
    sync(&app, &state, &window_name)
}

fn destination_identity(d: &Destination) -> String {
    match d {
        Destination::New => "new".into(),
        Destination::DevTools => "devtools".into(),
        Destination::Servers => "servers".into(),
        Destination::Server { server_id } => format!("server:{server_id}"),
        Destination::Console { server_id } => format!("console:{server_id}"),
        Destination::Join { host, port } => {
            format!("join:{}:{port}", host.clone().unwrap_or_default())
        }
        Destination::Settings => "settings".into(),
        Destination::Jobs => "jobs".into(),
        Destination::Audit => "audit".into(),
        Destination::About => "about".into(),
        Destination::Feedback => "feedback".into(),
        Destination::Extensions => "extensions".into(),
        Destination::Downloads => "downloads".into(),
        Destination::Missing { url } => format!("missing:{url}"),
    }
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
    let arg_destination = arg.as_ref().and_then(|a| a.get("destination")).cloned();

    let mut window_verb: Option<u32> = None;
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
                }
            }
            cmd::SHOW_BOOKMARK_BAR => {
                inner.bookmarks.toggle_bar();
            }
            // Window verbs touch the OS, not the model — deferred below.
            // NEW_WINDOW tears off a fresh window from the model too.
            // DEV_TOOLS and OPEN_LOGS are OS-facing too (devtools pane,
            // the log folder in the platform file manager).
            cmd::WINDOW_MINIMIZE
            | cmd::WINDOW_TOGGLE_MAXIMIZE
            | cmd::WINDOW_CLOSE
            | cmd::FOCUS_LOCATION
            | cmd::TOGGLE_PALETTE
            | cmd::NEW_WINDOW
            | cmd::DEV_TOOLS
            | cmd::OPEN_LOGS => {
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
                            strip.close(target);
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
                            strip.group_create(&[target], &label);
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
                        // §61 singleton identity through the active tab.
                        let identity = destination_identity(&destination);
                        if let Some(existing) = strip
                            .tabs
                            .iter()
                            .find(|t| destination_identity(t.destination()) == identity)
                            .map(|t| t.id)
                        {
                            strip.select(existing);
                        } else if let Some(active) = strip.active {
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
            }
        }
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

/// The omnibox commit: classify, then drive the model. Async per the
/// re-entrancy law (a query can create a tab webview inside `sync`).
#[tauri::command]
pub async fn shell_omnibox_commit(
    window: tauri::Webview,
    text: String,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<serde_json::Value, String> {
    use crate::shell::omnibox::AddressRequest;
    let window_name = window_label_of(&window);
    // An omnibox commit is an interaction: any open popup yields.
    dismiss_popup(&app, &state, &window_name);
    let request = crate::shell::omnibox::classify(&text);
    let outcome = {
        let mut inner = state.lock();
        let strip = inner.strip(&window_name);
        let Some(active) = strip.active else {
            return Err("no active tab".into());
        };
        match request {
            AddressRequest::Internal(destination) => {
                // Singleton identity (§61): a resting tab of the same
                // identity is focused, else the active tab navigates.
                let identity = destination_identity(&destination);
                if let Some(existing) = strip
                    .tabs
                    .iter()
                    .find(|t| destination_identity(t.destination()) == identity)
                    .map(|t| t.id)
                {
                    strip.select(existing);
                } else {
                    strip.navigate(active, destination);
                }
                serde_json::json!({ "kind": "navigated" })
            }
            AddressRequest::Join { host, port } => {
                // §7's honest join, completed: the address lands on its
                // own destination (singleton identity — the same address
                // refocuses its resting tab), and the Join page consults
                // the daemon (registry, server-list ping) for the
                // verdict. No webview navigates to a raw address.
                let destination = Destination::Join { host, port };
                let identity = destination_identity(&destination);
                if let Some(existing) = strip
                    .tabs
                    .iter()
                    .find(|t| destination_identity(t.destination()) == identity)
                    .map(|t| t.id)
                {
                    strip.select(existing);
                } else {
                    strip.navigate(active, destination);
                }
                serde_json::json!({ "kind": "navigated" })
            }
            AddressRequest::Query(text) => {
                // A discovery query lands on the new tab (§22): focused
                // if it already rests, created if it does not.
                let resting = strip
                    .tabs
                    .iter()
                    .find(|t| t.destination() == &Destination::New)
                    .map(|t| t.id);
                match resting {
                    Some(id) => {
                        strip.select(id);
                    }
                    None => {
                        strip.append(Destination::New, true);
                    }
                }
                serde_json::json!({ "kind": "query", "text": text })
            }
        }
    };
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
    state.lock().bookmarks.remove(&id);
    sync(&app, &state, &window_name)
}

/// Open a popup overlay for a window: the application-owned menu or
/// form that replaced the NATIVE gray popup. The overlay is a
/// transparent child webview (the newest child, so it floats above the
/// content), focused on arrival, carrying its kind and tab context in
/// the URL for [`shell_popup_boot`] to read. Any previously open popup
/// of the window dies first — one popup at a time, as upstream's widget
/// stack allows exactly one.
#[tauri::command]
pub async fn shell_popup(
    window: tauri::Webview,
    kind: String,
    tab_id: Option<u32>,
    x: f64,
    y: f64,
    state: State<'_, ShellState>,
    app: AppHandle,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    let host_window = window_for(&app, &window_name)?;
    let size: LogicalSize<f64> = host_window
        .inner_size()
        .map_err(|e| e.to_string())?
        .to_logical(host_window.scale_factor().unwrap_or(1.0));
    let (position, popup_size) = popup_rect((size.width, size.height), &kind, x, y);
    dismiss_popup(&app, &state, &window_name);
    let label = popup_label(&window_name);
    host_window
        .add_child(
            tauri::webview::WebviewBuilder::new(&label, WebviewUrl::App("popup.html".into()))
                .transparent(true),
            position,
            popup_size,
        )
        .map_err(|e| format!("could not open the popup: {e}"))?;
    let _ = app.emit_to(
        &label,
        "shell://popup-boot",
        serde_json::json!({ "kind": kind, "tab_id": tab_id }),
    );
    state
        .lock()
        .popups
        .insert(window_name, PopupState { kind, tab: tab_id });
    // Focus follows the popup: Escape and the arrow keys work from the
    // first keystroke, exactly like a freshly opened OS menu.
    if let Some(popup) = app.get_webview(&label) {
        let _ = popup.set_focus();
    }
    Ok(())
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
    let (kind, tab_id) = {
        let inner = state.lock();
        match inner.popups.get(&window_name) {
            Some(popup) => (popup.kind.clone(), popup.tab),
            None => ("unknown".into(), None),
        }
    };
    let mut context = serde_json::json!({ "kind": kind, "tab_id": tab_id });
    if let Some(tab_id) = tab_id {
        let inner = state.lock();
        if let Some(strip) = inner.strips.get(&window_name) {
            if let Some(tab) = strip.tabs.iter().find(|t| t.id == tab_id) {
                context["pinned"] = serde_json::json!(tab.pinned);
                context["muted"] = serde_json::json!(tab.muted);
                context["zoom"] = serde_json::json!(tab.zoom);
            }
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
                if let Some(y) = y {
                    let strip_h = layout::TAB_HEIGHT + layout::STRIP_PADDING;
                    if y > strip_h + VERTICAL_DETACH_MAGNETISM || y < -VERTICAL_DETACH_MAGNETISM {
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
                // bands — a drop over one of them MOVES the tab there
                // (drag between windows); only a drop on empty screen
                // tears off into a new window at the pointer.
                if let Some((target, local_x)) = window_strip_at(&app, &state, &window_name, sx, sy)
                {
                    return move_tab_between_windows(
                        &app,
                        &state,
                        &window_name,
                        &target,
                        id,
                        local_x,
                    );
                }
                let tab = {
                    let mut inner = state.lock();
                    let strip = inner.strip(&window_name);
                    strip.detach(id).ok_or("tab vanished")?.0
                };
                spawn_tearoff(&app, &state, tab.destination().clone(), sx, sy)
            } else {
                {
                    let mut inner = state.lock();
                    // The width reads before the strip's mutable borrow:
                    // the guard can't serve both at once (E0502's law).
                    let strip_width = inner
                        .strip_widths
                        .get(&window_name)
                        .copied()
                        .unwrap_or(1024.0);
                    let slots = {
                        let strip = inner.strip(&window_name);
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
                        layout::compute_layout(strip_width, &tab_tuples, active, &group_tuples)
                    };
                    if let Some(x) = x {
                        // The lift-out rule: the insertion index runs over
                        // the REMAINING tabs — the dragged tab's own slot
                        // never participates in its own drop verdict.
                        let others: Vec<layout::Slot> = slots
                            .iter()
                            .filter(|s| !s.header && s.id != id)
                            .cloned()
                            .collect();
                        let index = layout::drop_index(&others, x);
                        inner.strip(&window_name).reorder_drop(id, index);
                    }
                }
                emit_tab_state(&app, &state, &window_name);
                sync(&app, &state, &window_name)
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

/// Which OTHER window's strip band does this screen point land on, and
/// where within it? (The drag-between-windows drop: Chromium tears off
/// into a NEW window only when the drop lands on no existing strip.)
/// Returns the target window's label and the pointer's x in that
/// window's logical coordinates.
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
    let band = layout::header_height(state.lock().bookmarks.bar_visible);
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
        let x = outer.x as f32 / scale as f32;
        let y = outer.y as f32 / scale as f32;
        let w = size.width as f32 / scale as f32;
        if screen_x >= x && screen_x <= x + w && screen_y >= y && screen_y <= y + band {
            return Some((name, screen_x - x));
        }
    }
    None
}

/// The between-windows move: the tab leaves the source strip as a
/// DetachedTab and re-inserts at the drop point of the target strip
/// (the pinned block law rides insert/set_pinned; the history travels
/// intact). No second server process is possible — destinations are
/// views; the daemon owns the processes.
fn move_tab_between_windows(
    app: &AppHandle,
    state: &ShellState,
    source: &str,
    target: &str,
    id: TabId,
    local_x: f32,
) -> Result<(), String> {
    let tab = {
        let mut inner = state.lock();
        let strip = inner
            .strips
            .get_mut(source)
            .ok_or("source strip vanished")?;
        strip.detach(id).ok_or("tab vanished")?.0
    };
    {
        let mut inner = state.lock();
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
            let slots = layout::compute_layout(strip_width, &tab_tuples, active, &group_tuples);
            layout::drop_index(&slots, local_x)
        };
        let strip = inner.strip(target);
        let at = drop_index.min(strip.tabs.len());
        let new_id = strip.insert(at, tab.destination().clone(), true);
        // The insert minted a fresh tab id (the model's own business);
        // carry the DetachedTab's history, posture, and pinned-ness.
        if let Some(inserted) = strip.tabs.iter_mut().find(|t| t.id == new_id) {
            inserted.history = tab.history;
            inserted.history_index = tab.history_index;
            inserted.muted = tab.muted;
            inserted.zoom = tab.zoom;
        }
        if tab.pinned {
            strip.set_pinned(new_id, true);
        }
    }
    emit_tab_state(app, state, source);
    emit_tab_state(app, state, target);
    sync(app, state, source)?;
    sync(app, state, target)
}

/// A tear-off: a new window with its own frame and the detached tab.
fn spawn_tearoff(
    app: &AppHandle,
    state: &ShellState,
    destination: Destination,
    screen_x: f32,
    screen_y: f32,
) -> Result<(), String> {
    let label = {
        let mut inner = state.lock();
        let n = inner.next_window;
        inner.next_window += 1;
        let label = format!("win-{n}");
        let mut strip = Strip::new();
        strip.tabs[0].history = vec![destination];
        inner.strips.insert(label.clone(), strip);
        label
    };
    let window = tauri::window::WindowBuilder::new(app, &label)
        .decorations(false)
        .inner_size(1100.0, 720.0)
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
            ),
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1100.0, 83.0),
        )
        .map_err(|e| format!("tear-off frame failed: {e}"))?;
    // The content webview is created by sync() once the tear-off's frame
    // reports its size (shell_boot).
    sync(app, state, &label)
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
