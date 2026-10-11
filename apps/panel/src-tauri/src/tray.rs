//! The system tray (§12.1): a live SERVER STACK, not just a parking spot.
//!
//! The tray owns its own thin daemon wire (a dedicated session — never a
//! webview's): it speaks `daemon.hello`, snapshots `server.list`, rides
//! the `events` stream, and rebuilds its menu whenever a server's state
//! actually changes. The menu reads as a stack —
//!
//! ```text
//!   Open ZIM
//!   ────────────────
//!   Daemon: online
//!   ●  Alpha — running
//!   ○  Beta — not-running
//!   ✖  Gamma — crashed
//!   ────────────────
//!   Quit ZIM (stops servers & daemon)
//! ```
//!
//! Clicking a server row toggles it: running → graceful stop, stopped →
//! start. The state glyphs are the "dynamic indicators": each row's mark
//! moves with the daemon's own state machine (ADR-0005), driven by
//! `serverStateChanged` events — never by a poll alone (a 30 s
//! `server.list` re-snapshot rides along as the missed-event safety net).
//! The registry's own verbs speak the same stream: `registered`
//! transitions re-snapshot immediately (the event carries no display
//! name — the registry does, and a late row beats a guessed one),
//! `removed` transitions drop the row on the spot (a "?" zombie would
//! read as a server in trouble), and `serverRenamed` events move the
//! row's name. The tooltip carries the summary ("ZIM — 2 of 5 servers
//! running").
//!
//! Quit is the ONE full shutdown: the graceful `daemon.shutdown` ask (the
//! daemon runs its stop ladder for every running server), a bounded wait
//! for the endpoint to die, and — if the daemon is wedged and still
//! alive — a force kill by image name, so "no zamind.exe left behind" is
//! a guarantee, not a hope. The panel then exits with it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Manager, Runtime};
use tokio::sync::{mpsc, oneshot};
use zamin_ipc::Endpoint;

/// The tray's protocol identity — its own client, honestly named so the
/// daemon's audit log says who toggled what.
const TRAY_CLIENT_NAME: &str = "zim-tray";

/// Menu item ids. A server row's id is `srv:<server_id>`; the current
/// state decides whether the click starts or stops (the menu text says
/// which, and the model is re-read at click time — never cached in the
/// menu item).
const MENU_OPEN: &str = "tray-open";
const MENU_STATUS: &str = "tray-status";
const MENU_QUIT: &str = "tray-quit";
const MENU_SRV_PREFIX: &str = "srv:";

/// How often the safety-net re-snapshot runs. Events drive the updates;
/// this only catches what a dropped notification hid.
const RESNAPSHOT_INTERVAL: Duration = Duration::from_secs(30);

/// Reconnect backoff when the daemon wire is down. The tray never spawns
/// a daemon (that is the webview's `daemon_ensure` job); it waits for one
/// to appear and shows "offline" until then.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(5);

/// The state glyph for one row — the "dynamic indicator". Every font on
/// the supported platforms carries these marks.
fn state_glyph(state: &str) -> &'static str {
    match state {
        "running" => "●",
        "starting" => "◐",
        "stopping" => "◑",
        "adopting" => "◌",
        "crashed" => "✖",
        "failed-preflight" => "⚠",
        "unknown" => "?",
        _ => "○", // not-running, stopped: the honest resting mark
    }
}

/// The tray's whole view of the world, behind one lock.
#[derive(Default)]
struct TrayModel {
    daemon_online: bool,
    /// Ordered rows: (server id, display name, state string).
    servers: Vec<(String, String, String)>,
}

impl TrayModel {
    fn running_count(&self) -> usize {
        self.servers.iter().filter(|(_, _, s)| s == "running").count()
    }

    /// The tooltip: "ZIM — 2 of 5 servers running".
    fn summary(&self) -> String {
        if !self.daemon_online {
            return "ZIM — daemon offline".to_owned();
        }
        let total = self.servers.len();
        if total == 0 {
            return "ZIM — no servers registered".to_owned();
        }
        format!("ZIM — {} of {} servers running", self.running_count(), total)
    }
}

/// Shared handle: the menu-event handler (main thread) reads the model to
/// decide start/stop; the wire task (async runtime) writes it. Managed as
/// `Arc<TrayState>` so the menu callback and the wire task share ONE.
#[derive(Default)]
pub struct TrayState {
    model: Mutex<TrayModel>,
    /// The command lane into the wire task: menu clicks turn into calls.
    commands: Mutex<Option<mpsc::Sender<TrayCommand>>>,
}

impl TrayState {
    fn lock_model(&self) -> std::sync::MutexGuard<'_, TrayModel> {
        self.model.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Connect the wire task's command lane (called once at wire startup;
    /// the channel outlives every reconnect).
    fn attach(&self, sender: mpsc::Sender<TrayCommand>) {
        *self.commands.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    }

    /// One protocol call from a menu click. Fire-and-forget by design:
    /// the tray shows the outcome through the events stream (a start's
    /// transition arrives as `starting` → `running`), not a dialog.
    fn dispatch(&self, method: &str, params: Value) {
        let guard = self.commands.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sender) = guard.as_ref() {
            let _ = sender.try_send(TrayCommand::Call {
                method: method.to_owned(),
                params,
            });
        }
    }

    fn set_online(&self, online: bool) -> bool {
        let mut model = self.lock_model();
        if model.daemon_online == online {
            return false;
        }
        model.daemon_online = online;
        true
    }

    /// Apply a full `server.list` snapshot. Returns whether the menu
    /// should be rebuilt.
    fn apply_snapshot(&self, servers: Vec<(String, String, String)>) -> bool {
        let mut model = self.lock_model();
        if model.servers == servers {
            return false;
        }
        model.servers = servers;
        true
    }

    /// Apply one `serverStateChanged` transition. The registry's own
    /// verbs ride the same stream with a `reason` (the engine's law, the
    /// panel's own handler follows it too): `removed` drops the row NOW
    /// — a "?" zombie would read as a server in trouble — and anything
    /// else that moves a known row updates it.
    fn apply_transition(&self, server_id: &str, to: &str, reason: &str) -> Transition {
        let mut model = self.lock_model();
        if reason == "removed" {
            let before = model.servers.len();
            model.servers.retain(|(sid, _, _)| sid != server_id);
            return if model.servers.len() == before {
                Transition::Noop
            } else {
                Transition::Gone
            };
        }
        for entry in model.servers.iter_mut() {
            if entry.0 == server_id {
                if entry.2 == to {
                    return Transition::Noop;
                }
                entry.2 = to.to_owned();
                return Transition::Applied;
            }
        }
        // A server the tray has never heard of (registered since the last
        // snapshot, or a dropped notification hid it): the display name
        // lives in the registry, not the event — a re-snapshot is the
        // honest path, and guessing a name is worse than a late row.
        Transition::Unknown
    }

    /// Apply one `serverRenamed` event: the row keeps its state, its name
    /// moves. Returns whether the menu should be rebuilt.
    fn apply_rename(&self, server_id: &str, display_name: &str) -> bool {
        let mut model = self.lock_model();
        for entry in model.servers.iter_mut() {
            if entry.0 == server_id {
                if entry.1 == display_name {
                    return false;
                }
                entry.1 = display_name.to_owned();
                return true;
            }
        }
        // Never heard of it: the next re-snapshot carries both the row
        // and the new name.
        false
    }
}

/// What one inbound transition means for the tray's model — the caller
/// decides between a menu rebuild and a re-snapshot.
enum Transition {
    /// A known row moved: rebuild the menu.
    Applied,
    /// The registry dropped the server: the row is gone, rebuild.
    Gone,
    /// The tray has never heard of this server: a `server.list`
    /// re-snapshot is mandatory.
    Unknown,
    /// Nothing moved.
    Noop,
}

/// The wire task's command vocabulary.
enum TrayCommand {
    /// One protocol request; the outcome rides the events stream.
    Call { method: String, params: Value },
    /// Re-snapshot `server.list` (the safety net).
    Refresh,
}

/// Build the tray's menu from the current model. Runs on the MAIN thread
/// (Tauri's menu law) via `run_on_main_thread`.
fn rebuild_menu<R: Runtime>(app: &AppHandle<R>, state: &TrayState) {
    let model = state.lock_model();
    let mut menu = match Menu::new(app) {
        Ok(menu) => menu,
        Err(_) => return,
    };

    // The window verb first: the stack reads top-down, and the
    // destructive Quit lands last where a slip cannot reach it.
    if let Ok(open) = MenuItem::with_id(app, MENU_OPEN, "Open ZIM", true, None::<&str>) {
        let _ = menu.append(&open);
    }
    if let Ok(sep) = PredefinedMenuItem::separator(app) {
        let _ = menu.append(&sep);
    }

    // The status row (disabled — it is a readout, not a verb). Exactly
    // one id, exactly one row, whatever the daemon's condition.
    let status_text = if !model.daemon_online {
        "Daemon: offline — waiting…"
    } else if model.servers.is_empty() {
        "Daemon: online — no servers registered"
    } else {
        "Daemon: online"
    };
    if let Ok(status) = MenuItem::with_id(app, MENU_STATUS, status_text, false, None::<&str>) {
        let _ = menu.append(&status);
    }

    // The server stack: one row per server, its glyph moving with the
    // daemon's own state machine. The id carries the server id; the
    // CURRENT state is re-read at click time.
    for (id, name, state) in &model.servers {
        let text = format!("{}  {} — {}", state_glyph(state), name, state);
        let row =
            MenuItem::with_id(app, format!("{MENU_SRV_PREFIX}{id}"), text, true, None::<&str>);
        if let Ok(row) = row {
            let _ = menu.append(&row);
        }
    }

    let summary = model.summary();
    drop(model);

    if let Ok(sep) = PredefinedMenuItem::separator(app) {
        let _ = menu.append(&sep);
    }
    if let Ok(quit) = MenuItem::with_id(
        app,
        MENU_QUIT,
        "Quit ZIM (stops servers & daemon)",
        true,
        None::<&str>,
    ) {
        let _ = menu.append(&quit);
    }

    if let Some(tray) = app.tray_by_id("zim-tray") {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(summary));
    }
}

/// Ask the main thread to rebuild the tray menu (state may have changed).
fn refresh_menu(app: &AppHandle<tauri::Wry>, state: &Arc<TrayState>) {
    let state = state.clone();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || rebuild_menu(&handle, &state));
}

/// Parse a `server.list` result (or the subscribe snapshot's `servers`
/// array — the same shape) into tray rows.
fn parse_servers(result: &Value) -> Vec<(String, String, String)> {
    result
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    (
                        entry
                            .get("serverId")
                            .and_then(Value::as_str)
                            .unwrap_or("?")
                            .to_owned(),
                        entry
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or("(unnamed)")
                            .to_owned(),
                        entry
                            .get("state")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// One pending protocol reply, completed by id.
type Pending = oneshot::Sender<Result<Value, String>>;

/// The tray's dedicated daemon session: a framed connection, a reader
/// integrated into the task's select loop, and a small id→pending map so
/// menu clicks and refreshes can overlap safely.
struct TrayWire {
    writer: zamin_ipc::ConnectionWriteHalf,
    reader: zamin_ipc::ConnectionReadHalf,
    next_id: u64,
    pending: HashMap<u64, Pending>,
}

impl TrayWire {
    /// Send one request; the reply (or error) lands on `tx` when it
    /// arrives. The wire never blocks on a reply — the select loop keeps
    /// reading notifications while requests are in flight.
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        reply: Option<Pending>,
    ) -> Result<(), String> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if !params.is_null() {
            request["params"] = params;
        }
        let frame =
            serde_json::to_string(&request).map_err(|e| format!("tray frame encode: {e}"))?;
        if let Some(tx) = reply {
            self.pending.insert(id, tx);
        }
        zamin_bridge::send_frame(&mut self.writer, &frame)
            .await
            .map_err(|error| format!("tray write failed: {error}"))
    }

    /// Handle one inbound frame: a reply completes its pending call; a
    /// `streams.notification` updates the model and refreshes the menu.
    /// Returns true when the caller should re-snapshot (unknown server or
    /// missed-marker seen).
    fn on_frame(
        &mut self,
        frame: &str,
        app: &AppHandle<tauri::Wry>,
        state: &Arc<TrayState>,
    ) -> bool {
        let mut needs_snapshot = false;
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return false;
        };
        // A reply to something we asked.
        if let Some(id) = value.get("id").and_then(Value::as_u64) {
            if let Some(tx) = self.pending.remove(&id) {
                if let Some(error) = value.get("error") {
                    let _ = tx.send(Err(error.to_string()));
                } else {
                    let _ = tx.send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
                }
            }
            return false;
        }
        // The events stream's notifications. The payload is a tagged
        // StreamPayload (`kind` — the same envelope the panel's wire
        // unwraps): "event" carries a CoreEvent under `event`, "missed"
        // says the bounded queue dropped notifications.
        if value.get("method").and_then(Value::as_str) == Some("streams.notification") {
            let payload = &value["params"]["payload"];
            match payload["kind"].as_str() {
                Some("event") => {
                    let event = &payload["event"];
                    match event["type"].as_str() {
                        Some("serverStateChanged") => {
                            let server_id = event["serverId"].as_str().unwrap_or_default();
                            let to = event["to"].as_str().unwrap_or("unknown");
                            let reason = event["reason"].as_str().unwrap_or_default();
                            if !server_id.is_empty() {
                                match state.apply_transition(server_id, to, reason) {
                                    Transition::Applied | Transition::Gone => {
                                        refresh_menu(app, state);
                                    }
                                    // Registered since the last snapshot (or a
                                    // dropped notification): the registry holds the
                                    // name; the re-snapshot fetches the row now,
                                    // not at the next 30 s tick.
                                    Transition::Unknown => needs_snapshot = true,
                                    Transition::Noop => {}
                                }
                            }
                        }
                        Some("serverRenamed") => {
                            let server_id = event["serverId"].as_str().unwrap_or_default();
                            let display_name =
                                event["displayName"].as_str().unwrap_or_default();
                            if !server_id.is_empty()
                                && !display_name.is_empty()
                                && state.apply_rename(server_id, display_name)
                            {
                                refresh_menu(app, state);
                            }
                        }
                        _ => {}
                    }
                }
                // The bounded queue dropped notifications: the next
                // re-snapshot is now mandatory, not just a safety net.
                Some("missed") => needs_snapshot = true,
                _ => {}
            }
        }
        needs_snapshot
    }
}

/// The tray wire's whole lifecycle: connect → hello → subscribe (its
/// snapshot seeds the stack) → serve. One task, restarted by its own
/// supervisor arm on wire death — the tray outlives daemon restarts by
/// design. The command channel is created ONCE and survives reconnects,
/// so a click during a blip queues instead of dying.
async fn run_tray_wire(app: AppHandle<tauri::Wry>, state: Arc<TrayState>) {
    let endpoint = Endpoint::default_endpoint();
    let (command_tx, mut command_rx) = mpsc::channel::<TrayCommand>(64);
    state.attach(command_tx);

    loop {
        // Connect (down daemon → offline row, backoff, retry).
        let connection = loop {
            match zamin_ipc::connect(endpoint.clone()).await {
                Ok(connection) => break connection,
                Err(_) => {
                    if state.set_online(false) {
                        refresh_menu(&app, &state);
                    }
                    tokio::time::sleep(RECONNECT_INTERVAL).await;
                }
            }
        };
        let (writer, reader) = connection.split();
        let mut wire = TrayWire {
            writer,
            reader,
            next_id: 0,
            pending: HashMap::new(),
        };

        // The handshake is MANDATORY (protocol spec §2): everything else
        // is refused until it completes. A reply budget keeps a half-dead
        // daemon from wedging the tray's one task.
        let (hello_tx, hello_rx) = oneshot::channel();
        if let Err(error) = wire
            .request(
                "daemon.hello",
                json!({
                    "protocol": zamin_protocol::PROTOCOL_VERSION,
                    "client": {
                        "name": TRAY_CLIENT_NAME,
                        "version": app.package_info().version.to_string(),
                    },
                }),
                Some(hello_tx),
            )
            .await
        {
            tracing::warn!("tray wire: {error}");
            tokio::time::sleep(RECONNECT_INTERVAL).await;
            continue;
        }
        match tokio::time::timeout(Duration::from_secs(2), hello_rx).await {
            Ok(Ok(Ok(_))) => {}
            _ => {
                // Connected but not speaking: offline, backoff, reconnect.
                let _ = state.set_online(false);
                refresh_menu(&app, &state);
                tokio::time::sleep(RECONNECT_INTERVAL).await;
                continue;
            }
        }

        // Live on the events stream BEFORE the first snapshot lands: the
        // subscribe result itself carries the snapshot (the engine's
        // anchor-then-register ordering), so nothing between hello and
        // subscribe can be lost.
        let (sub_tx, sub_rx) = oneshot::channel();
        if wire
            .request("streams.subscribe", json!({ "stream": "events" }), Some(sub_tx))
            .await
            .is_err()
        {
            continue;
        }
        let mut needs_snapshot = false;
        match tokio::time::timeout(Duration::from_secs(2), sub_rx).await {
            Ok(Ok(Ok(result))) => {
                if state.set_online(true) {
                    refresh_menu(&app, &state);
                }
                // The subscribe result's snapshot IS the first
                // server.list (same shape: every registered server with
                // its state).
                if let Some(servers) = result.get("snapshot").and_then(|s| s.get("servers")) {
                    if state.apply_snapshot(parse_servers(servers)) {
                        refresh_menu(&app, &state);
                    }
                }
            }
            _ => {
                let _ = state.set_online(false);
                refresh_menu(&app, &state);
                tokio::time::sleep(RECONNECT_INTERVAL).await;
                continue;
            }
        }

        let mut resnapshot = tokio::time::interval(RESNAPSHOT_INTERVAL);
        resnapshot.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        // Serve: reader frames, menu commands, the safety-net tick. The
        // reader failing (daemon death) ends the pass and loops back to
        // reconnect; a failed command write means the same.
        loop {
            tokio::select! {
                frame = wire.reader.recv() => {
                    match frame {
                        Ok(Some(frame)) => {
                            let text = String::from_utf8_lossy(&frame).into_owned();
                            if wire.on_frame(&text, &app, &state) {
                                needs_snapshot = true;
                            }
                        }
                        Ok(None) => break,       // daemon closed: reconnect
                        Err(_) => break,         // wire died: reconnect
                    }
                }
                command = command_rx.recv() => {
                    match command {
                        Some(TrayCommand::Call { method, params }) => {
                            if wire.request(&method, params, None).await.is_err() {
                                break; // write failed: the wire is dead
                            }
                        }
                        Some(TrayCommand::Refresh) => {
                            needs_snapshot = true;
                        }
                        None => return,          // the tray went away (app exit)
                    }
                }
                _ = resnapshot.tick() => {
                    needs_snapshot = true;
                }
            }
            if needs_snapshot {
                needs_snapshot = false;
                let (list_tx, list_rx) = oneshot::channel();
                if wire.request("server.list", Value::Null, Some(list_tx)).await.is_ok() {
                    if let Ok(Ok(Ok(result))) =
                        tokio::time::timeout(Duration::from_secs(3), list_rx).await
                    {
                        if state.apply_snapshot(parse_servers(&result)) {
                            refresh_menu(&app, &state);
                        }
                    }
                }
            }
        }

        // Wire death: show offline until the reconnect lands.
        if state.set_online(false) {
            refresh_menu(&app, &state);
        }
        tokio::time::sleep(RECONNECT_INTERVAL).await;
    }
}

/// Start the tray wire task. Called once from the host's setup.
pub fn spawn(app: AppHandle<tauri::Wry>, state: Arc<TrayState>) {
    tauri::async_runtime::spawn(run_tray_wire(app, state));
}

/// The Quit verb's daemon half: graceful ask → bounded wait for death →
/// force kill by image name. "No zamind.exe left behind" as a guarantee.
///
/// The graceful ask rides its own one-shot connection (`daemon.shutdown`
/// ends every session, including the tray's own wire); the wait watches
/// the per-user endpoint disappear; the force kill reuses the heal's
/// image-name kill (a wedged daemon answers nothing — including this
/// ask — and only the kill speaks to it).
pub async fn quit_shutdown_daemon() {
    use zamin_protocol::envelope::{Request, RequestId};
    let endpoint = Endpoint::default_endpoint();
    let graceful = async {
        let connection = zamin_ipc::connect(endpoint.clone()).await.ok()?;
        let (mut write, mut read) = connection.split();
        let request = Request::new(
            RequestId::Number(1),
            zamin_protocol::methods::DAEMON_SHUTDOWN,
            None,
        );
        let frame = serde_json::to_string(&request).ok()?;
        zamin_bridge::send_frame(&mut write, &frame).await.ok()?;
        // The reply is courtesy; the daemon's own ladder does the work.
        let _ = tokio::time::timeout(Duration::from_secs(2), read.recv()).await;
        Some(())
    };
    let _ = graceful.await;

    // Wait for the endpoint to actually die (the stop ladder can take
    // stop_timeout per server; 15 s covers a default fleet's graceful
    // path without making Quit feel like a hang).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    while tokio::time::Instant::now() < deadline {
        if zamin_ipc::connect(endpoint.clone()).await.is_err() {
            return; // gone: the graceful path won
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // Still listening: either wedged (answers nothing) or a stubborn
    // server. The operator said QUIT — the force kill ends both the
    // daemon and every server tree it owns (the installer's own verb).
    crate::daemon_ensure::kill_daemons_by_image();
    // One final settle so the exit and the kill never race in the OS's
    // eyes (the pipe lingers one scheduling tick after the kill).
    let settle = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < settle {
        if zamin_ipc::connect(endpoint.clone()).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// Route a tray menu event. Runs in the host's menu callback; the async
/// verbs spawn onto the runtime.
pub fn on_menu_event<R: Runtime>(app: &AppHandle<R>, state: &TrayState, id: &str) {
    if let Some(server_id) = id.strip_prefix(MENU_SRV_PREFIX) {
        // The toggle: the CURRENT state decides the verb, read at click
        // time — the events stream may have moved it since the menu was
        // rendered. Live-ish states stop; resting states start; anything
        // ambiguous does nothing (the row's glyph already says why).
        let action = {
            let model = state.lock_model();
            let current = model
                .servers
                .iter()
                .find(|(sid, _, _)| sid == server_id)
                .map(|(_, _, s)| s.as_str())
                .unwrap_or("unknown");
            match current {
                "running" | "starting" | "adopting" => Some("server.stop"),
                "not-running" | "stopped" | "crashed" | "failed-preflight" => Some("server.start"),
                _ => None,
            }
        };
        if let Some(method) = action {
            state.dispatch(method, json!({ "serverId": server_id }));
        }
        return;
    }
    match id {
        MENU_OPEN => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }
        MENU_QUIT => {
            let handle = app.clone();
            tauri::async_runtime::spawn(async move {
                quit_shutdown_daemon().await;
                handle.exit(0);
            });
        }
        _ => {}
    }
}
