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
use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Rect, State, WebviewUrl, Window};

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

pub struct ShellState {
    inner: Mutex<ShellInner>,
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
    /// The frame's viewport width (reported on boot/resize; it sizes the
    /// layout law).
    strip_width: f32,
}

impl ShellState {
    pub fn new() -> Self {
        ShellState {
            inner: Mutex::new(ShellInner {
                strips: HashMap::new(),
                bookmarks: Bookmarks::default(),
                session_path: None,
                bookmarks_path: None,
                drag: DragSession::default(),
                next_window: 1,
                strip_width: 1024.0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, ShellInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
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
        inner.strips.insert("main".into(), session.primary.unwrap_or_else(Strip::new));
    }
}

impl ShellInner {
    fn save(&self) {
        let session = Session {
            primary: self.strips.get("main").cloned(),
            bar_visible: self.bookmarks.bar_visible,
        };
        if let Some(path) = &self.session_path {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(bytes) = serde_json::to_vec(&session) {
                let _ = std::fs::write(path, bytes);
            }
        }
        if let Some(path) = &self.bookmarks_path {
            self.bookmarks.save(path);
        }
    }

    fn strip(&mut self, window: &str) -> &mut Strip {
        self.strips.entry(window.to_owned()).or_insert_with(Strip::new)
    }
}

// ---------------------------------------------------------------------------
// View construction
// ---------------------------------------------------------------------------

fn snapshot(inner: &ShellInner, window: &str) -> Snapshot {
    let empty = Strip::new();
    let strip = inner.strips.get(window).unwrap_or(&empty);
    let tab_tuples: Vec<(u32, bool, bool)> =
        strip.tabs.iter().map(|t| (t.id, t.pinned, false)).collect();
    let active = strip.active.unwrap_or(0);
    let slots = layout::compute_layout(inner.strip_width, &tab_tuples, active);
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
        strip_width: inner.strip_width,
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
    label.to_owned()
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
        inner.strip_width = size.width as f32;
        let header = layout::header_height(inner.bookmarks.bar_visible);
        let create_tab = {
            let strip = inner.strip(window_label);
            let active_id = strip.active;
            active_id
                .and_then(|id| strip.tabs.iter().find(|t| t.id == id))
                .filter(|_| app.get_webview(&tab_label(window_label, active_id.unwrap_or(0))).is_none())
                .map(|t| (t.id, t.destination().clone(), t.reload))
        };
        let snap = snapshot(&inner, window_label);
        inner.save();
        (snap, header, create_tab)
    };

    let content_bounds = Rect {
        position: LogicalPosition::new(0.0, header as f64).into(),
        size: LogicalSize::new(
            size.width as f64,
            (size.height as f64 - header as f64).max(0.0),
        )
        .into(),
    };

    // Show the active tab's webview at its slot; hide the rest.
    for tab in &snap.tabs {
        let Some(webview) = app.get_webview(&tab_label(window_label, tab.id)) else { continue };
        if tab.active {
            let _ = webview.set_bounds(content_bounds.clone());
            let _ = webview.show();
        } else {
            let _ = webview.hide();
        }
    }

    // The active tab's webview is created lazily (WebContents born on
    // first focus — a restored session's inactive tabs stay cold).
    if let Some((id, destination, reload)) = create_tab {
        let label = tab_label(window_label, id);
        let webview = host_window
            .add_child(
                tauri::webview::WebviewBuilder::new(&label, WebviewUrl::App("index.html".into())),
                LogicalPosition::new(0.0, header as f64),
                LogicalSize::new(
                    size.width as f64,
                    (size.height as f64 - header as f64).max(0.0),
                ),
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
            size: LogicalSize::new(size.width as f64, header as f64).into(),
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
        let Some(strip) = inner.strips.get(window_label) else { return };
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
/// runtime by main.rs's event hook — see the re-entrancy law above.
pub fn relayout_window(app: &AppHandle, state: &ShellState, window_label: &str) {
    if let Err(e) = sync(app, state, window_label) {
        tracing::debug!("relayout of {window_label} failed: {e}");
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — the browser command surface
// ---------------------------------------------------------------------------

fn window_for(app: &AppHandle, label: &str) -> Result<Window, String> {
    app.get_window(label).ok_or_else(|| format!("window {label} vanished"))
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
    let tab = strip.tabs.iter().find(|t| t.id == tab_id).ok_or("tab vanished")?;
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
        Destination::Servers => "servers".into(),
        Destination::Server { server_id } => format!("server:{server_id}"),
        Destination::Console { server_id } => format!("console:{server_id}"),
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
    let arg_id = arg.as_ref().and_then(|a| a.get("tab_id")).and_then(|v| v.as_u64()).map(|v| v as TabId);
    let arg_group = arg.as_ref().and_then(|a| a.get("group_id")).and_then(|v| v.as_u64()).map(|v| v as GroupId);
    let arg_label = arg.as_ref().and_then(|a| a.get("label")).and_then(|v| v.as_str()).map(String::from);
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
                        strip.tabs.iter().find(|t| t.id == id).map(|t| {
                            (t.destination().clone(), t.destination().label())
                        })
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
            cmd::WINDOW_MINIMIZE | cmd::WINDOW_TOGGLE_MAXIMIZE | cmd::WINDOW_CLOSE
            | cmd::FOCUS_LOCATION => {
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
                        if let Some(t) = target.and_then(|id| strip.tabs.iter_mut().find(|t| t.id == id)) {
                            t.muted = !t.muted;
                        }
                    }
                    cmd::NAVIGATE_ACTIVE => {
                        let Some(destination) = arg_destination else {
                            return Err("NAVIGATE_ACTIVE needs a destination".into());
                        };
                        let Ok(destination) = serde_json::from_value::<Destination>(destination) else {
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
                        if let Some(t) = target.and_then(|id| strip.tabs.iter_mut().find(|t| t.id == id)) {
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
            window_for(&app, &window_name)?.minimize().map_err(|e| e.to_string())?;
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
            window_for(&app, &window_name)?.close().map_err(|e| e.to_string())?;
        }
        Some(cmd::FOCUS_LOCATION) => {
            let _ = app.emit_to(frame_label(&window_name), "shell://focus-address", ());
            return Ok(()); // frame-local; no model change, no sync
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
    let request = crate::shell::omnibox::classify(&text);
    let outcome = {
        let mut inner = state.lock();
        let strip = inner.strip(&window_name);
        let Some(active) = strip.active else { return Err("no active tab".into()) };
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
                // The join miss is honest (§7): the shell reports what it
                // heard; resolution rides the daemon through the panel.
                serde_json::json!({ "kind": "join", "host": host, "port": port })
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

/// The tab context menu — a NATIVE popup. The frame webview is an
/// 83px band; a DOM menu drawn inside it is clipped at the band's
/// bottom edge (shipped 0.4.3 hid this — the menu simply lost its tail),
/// so Chromium's Windows answer applies here too: the OS renders the
/// menu and it floats over the whole window. Items dispatch through
/// [`handle_tab_menu_verb`]; menu events arrive on the main thread, so
/// the verb itself runs on the async runtime (the re-entrancy law).
#[tauri::command]
pub async fn shell_tab_menu(
    window: tauri::Webview,
    app: AppHandle,
    state: State<'_, ShellState>,
    tab_id: u32,
) -> Result<(), String> {
    let window_name = window_label_of(&window);
    let (pinned, muted) = {
        let inner = state.lock();
        let strip = inner.strip(&window_name);
        match strip.tabs.iter().find(|t| t.id == tab_id) {
            Some(tab) => (tab.pinned, tab.muted),
            None => return Ok(()), // gone before the menu could show
        }
    };
    let base = format!("zamin-tab-menu|{window_name}|{tab_id}");
    let pin = MenuItem::with_id(&app, format!("{base}|pin"), if pinned { "Unpin tab" } else { "Pin tab" }, true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let mute = MenuItem::with_id(&app, format!("{base}|mute"), if muted { "Unmute tab" } else { "Mute tab" }, true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let duplicate = MenuItem::with_id(&app, format!("{base}|duplicate"), "Duplicate", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let group = MenuItem::with_id(&app, format!("{base}|group"), "Add to new group…", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let new_tab = MenuItem::with_id(&app, format!("{base}|new"), "New tab", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let close = MenuItem::with_id(&app, format!("{base}|close"), "Close tab", true, None::<&str>)
        .map_err(|e| e.to_string())?;
    let before_new = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
    let before_close = PredefinedMenuItem::separator(&app).map_err(|e| e.to_string())?;
    let menu = Menu::with_items(
        &app,
        &[&pin, &mute, &duplicate, &group, &before_new, &new_tab, &before_close, &close],
    )
    .map_err(|e| e.to_string())?;
    let host = window_for(&app, &window_name)?;
    menu.popup(host).map_err(|e| e.to_string())
}

/// The native tab menu's verbs — the model mutations behind the popup
/// items. Called from main.rs's menu-event hook, which spawns this onto
/// the async runtime: the event fires on the main thread, and a verb
/// like "new tab" births a webview inside sync (the re-entrancy law).
pub fn handle_tab_menu_verb(
    app: &AppHandle,
    state: &ShellState,
    window_label: &str,
    tab_id: u32,
    verb: &str,
) {
    {
        let mut inner = state.lock();
        let strip = inner.strip(window_label);
        match verb {
            "pin" => {
                let pinned = strip
                    .tabs
                    .iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.pinned)
                    .unwrap_or(false);
                strip.set_pinned(tab_id, !pinned);
            }
            "mute" => {
                if let Some(tab) = strip.tabs.iter_mut().find(|t| t.id == tab_id) {
                    tab.muted = !tab.muted;
                }
            }
            "duplicate" => strip.duplicate(tab_id),
            "close" => strip.close(tab_id),
            "new" => strip.append(Destination::New, true),
            // The group needs a label only the operator can type — the
            // frame asks, then lands ADD_NEW_TAB_TO_GROUP itself.
            "group" => {
                let _ = app.emit_to(
                    frame_label(window_label),
                    "shell://ask-group-label",
                    serde_json::json!({ "tab_id": tab_id }),
                );
            }
            _ => {}
        }
    }
    emit_tab_state(app, state, window_label);
    if let Err(e) = sync(app, state, window_label) {
        tracing::debug!("tab-menu verb {verb} sync failed: {e}");
    }
}

/// The drag session — frame pointer events in, model decisions out.
/// Async per the re-entrancy law: a detached drop spawns a whole WINDOW
/// (plus its frame webview) from the tear-off path.
#[tauri::command]
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
                    if y > strip_h + VERTICAL_DETACH_MAGNETISM
                        || y < -VERTICAL_DETACH_MAGNETISM
                    {
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
            if detached {
                // DetachIntoNewBrowserAndRunMoveLoop, adapted: the new
                // window opens where the pointer let go.
                let Some(id) = id else { return Err("no drag tab".into()) };
                let tab = {
                    let mut inner = state.lock();
                    let strip = inner.strip(&window_name);
                    strip.detach(id).ok_or("tab vanished")?.0
                };
                spawn_tearoff(&app, &state, tab.destination().clone(), sx, sy)
            } else {
                let Some(id) = id else { return Ok(()) };
                {
                    let mut inner = state.lock();
                    let slots = {
                        let strip = inner.strip(&window_name);
                        let tab_tuples: Vec<(u32, bool, bool)> =
                            strip.tabs.iter().map(|t| (t.id, t.pinned, false)).collect();
                        let active = strip.active.unwrap_or(0);
                        layout::compute_layout(inner.strip_width, &tab_tuples, active)
                    };
                    if let Some(x) = x {
                        let index = layout::drop_index(&slots, x);
                        inner.strip(&window_name).move_to(id, index);
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
        inner.strip_width = width as f32;
        let _ = height;
    }
    sync(&app, &state, &label)
}
