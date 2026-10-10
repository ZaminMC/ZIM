//! ZIM desktop host (ADR-0003): a bridge, nothing more.
//!
//! The webview owns the protocol client — handshake, correlation,
//! subscriptions, reconnect, cursors. This host owns exactly three things:
//! the daemon connection's lifetime, frame forwarding in both directions,
//! and the ~50 ms coalescing of daemon → webview frames (which lives in
//! `zamin-bridge`, tested in the workspace). No business logic belongs
//! here, so none is here.
//!
//! The Phase 7 additions stay inside that boundary: `daemon_ensure` makes
//! "double-clicking ZIM must never show a daemon error"
//! (ARCH-REVIEW §1.2) a host responsibility, and the autostart commands
//! are the OS-integration seam the webview cannot reach (§12.1).
//!
//! Phase 8 widens the wire, not the boundary (ADR-0011): the same three
//! duties now cover the remote case — the host may open the TLS relay to
//! a zaminagent (fingerprint pinned by the webview's connection profile)
//! instead of the local socket. Frame handling is identical; the agent is
//! just another transport. A remote profile's daemon is never spawned
//! here — the webview only asks `daemon_ensure` for local wires.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;
use zamin_ipc::Endpoint;

mod autostart;
mod daemon_ensure;
mod shell;

use shell::host::ShellState;

/// One live daemon connection and the tasks moving its frames.
struct Connected {
    /// Webview → daemon frames queue; the writer task drains it.
    outgoing: mpsc::Sender<String>,
    read_pump: tokio::task::JoinHandle<()>,
    batch_forwarder: tauri::async_runtime::JoinHandle<()>,
    write_pump: tauri::async_runtime::JoinHandle<()>,
}

impl Connected {
    fn shutdown(self) {
        // Dropping `outgoing` ends the writer loop; aborting the pumps ends
        // the readers. A deliberate close never fires the down signal.
        self.read_pump.abort();
        self.batch_forwarder.abort();
        self.write_pump.abort();
    }
}

// One connection slot PER WEBVIEW, keyed by the webview's label — the
// Chromium law this used to violate: every renderer owns its channel to
// the browser process for its whole life; a sibling connecting never
// tears an existing one down. The single-slot shape made every tab's
// `daemon_connect` REPLACE the previous webview's wire: the loser's
// down-channel fired, its client dropped into reconnect, the two tabs
// stole the slot from each other in a loop, and the operator read
// "ZIM is not answering" on whichever tab had lost the latest round.
// The daemon is a session server (one task per accepted connection),
// so N tab wires cost N cheap sessions — the honest topology.
#[derive(Default)]
struct HostState(Mutex<HashMap<String, Connected>>);

impl HostState {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, Connected>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Drop one webview's wire — the webview is gone (tab closed, orphan
/// swept), so its connection and pumps must not outlive it.
pub fn drop_wire(app: &AppHandle, label: &str) {
    let host = app.state::<HostState>();
    let mut guard = host.lock();
    if let Some(connected) = guard.remove(label) {
        connected.shutdown();
    }
}

/// Drop every wire belonging to a closed window's children
/// (`tab-{window}-{id}` labels). The window event loop has no
/// per-webview destroyed hook, so the close path sweeps.
pub fn drop_wire_window(app: &AppHandle, window_label: &str) {
    let prefix = format!("tab-{window_label}-");
    let dead: Vec<String> = {
        let host = app.state::<HostState>();
        let guard = host.lock();
        guard
            .keys()
            .filter(|label| label.starts_with(&prefix))
            .cloned()
            .collect()
    };
    for label in dead {
        drop_wire(app, &label);
    }
}

#[tauri::command]
async fn daemon_connect(
    window: tauri::Webview,
    endpoint: Option<String>,
    remote_addr: Option<String>,
    remote_token: Option<String>,
    remote_fingerprint: Option<String>,
    frames: Channel<String>,
    down: Channel<()>,
    state: State<'_, HostState>,
) -> Result<String, String> {
    // The active connection profile decides the wire (ADR-0011): with an
    // agent address the host relays over TLS to the remote box, pinning
    // its certificate fingerprint; without one it is the per-user local
    // socket. The resolver's errors are the honest, pre-network ones.
    let connection = match zamin_bridge::remote::resolve_remote(
        remote_addr,
        remote_token,
        remote_fingerprint,
    )? {
        Some(cfg) => {
            if cfg.trust == zamin_agent::client::Trust::InsecureSkipVerify {
                tracing::warn!(
                    "connecting to {} WITHOUT a pinned fingerprint: the agent's \
                     certificate is not verified, so this connection proves no \
                     server identity",
                    cfg.addr
                );
            }
            zamin_agent::client::connect(&cfg)
                .await
                .map_err(|error| format!("could not reach the agent at {}: {error}", cfg.addr))?
        }
        None => {
            let resolved = match endpoint {
                Some(value) => Endpoint::from_daemon_arg(&value),
                None => Endpoint::default_endpoint(),
            };
            zamin_ipc::connect(resolved)
                .await
                .map_err(|error| format!("could not connect to the daemon: {error}"))?
        }
    };
    let (mut write_half, read_half) = connection.split();

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<String>(256);
    let (batch_tx, mut batch_rx) = mpsc::channel::<Vec<String>>(256);

    // Daemon → webview: frames coalesce in zamin-bridge, then ride the
    // channel as one JSON array per batch (never one message per line —
    // PERFORMANCE-BUDGETS). Wire death surfaces through `down`, and the
    // webview's reconnect logic takes over from there.
    let down_channel = down.clone();
    let read_pump = zamin_bridge::spawn_read(read_half, batch_tx, move || {
        let _ = down_channel.send(());
    });

    let frames_channel = frames.clone();
    let batch_forwarder = tauri::async_runtime::spawn(async move {
        while let Some(batch) = batch_rx.recv().await {
            let Ok(payload) = serde_json::to_string(&batch) else {
                break;
            };
            if frames_channel.send(payload).is_err() {
                break; // webview is gone; nothing left to serve
            }
        }
    });

    // Webview → daemon: one frame per message, forwarded verbatim.
    let write_pump = tauri::async_runtime::spawn(async move {
        while let Some(frame) = outgoing_rx.recv().await {
            if zamin_bridge::send_frame(&mut write_half, &frame)
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // This webview's OWN slot — a sibling's connect never touches it
    // (the keyed law above). A re-connect from this same webview (its
    // client's retry) replaces only its own wire.
    let label = window.label().to_owned();
    let mut guard = state.lock();
    if let Some(previous) = guard.insert(
        label,
        Connected {
            outgoing: outgoing_tx,
            read_pump,
            batch_forwarder,
            write_pump,
        },
    ) {
        previous.shutdown();
    }
    Ok("connected".into())
}

#[tauri::command]
async fn daemon_send(
    window: tauri::Webview,
    frame: String,
    state: State<'_, HostState>,
) -> Result<(), String> {
    let outgoing = state
        .lock()
        .get(window.label())
        .map(|connected| connected.outgoing.clone())
        .ok_or_else(|| "not connected to the daemon".to_owned())?;
    outgoing
        .send(frame)
        .await
        .map_err(|error| format!("the daemon connection is gone: {error}"))
}

#[tauri::command]
async fn daemon_close(window: tauri::Webview, state: State<'_, HostState>) -> Result<(), String> {
    if let Some(connected) = state.lock().remove(window.label()) {
        connected.shutdown();
    }
    Ok(())
}

/// Probe the per-user daemon endpoint; spawn the sibling daemon when it is
/// down; wait for it to bind. The webview calls this when a transport start
/// fails, and its normal retry loop does the rest. Errors are plain
/// strings — the webview surfaces them through its own connection state.
#[tauri::command]
async fn daemon_ensure() -> Result<String, String> {
    daemon_ensure::ensure_daemon().await
}

/// Ask the daemon to end itself: one one-shot connection, one
/// `daemon.shutdown` request, a two-second reply budget. The daemon's
/// reply lands BEFORE its stop ladder finishes (it owns the wait); this
/// only proves the ask arrived. Fire-and-forget by design: every
/// failure mode (endpoint gone, daemon already dead) means the same
/// thing — nothing to stop — and the caller exits regardless.
async fn shutdown_daemon() -> Result<(), String> {
    use zamin_protocol::envelope::{Request, RequestId};
    use zamin_protocol::methods;
    let endpoint = Endpoint::default_endpoint();
    let connection = zamin_ipc::connect(endpoint)
        .await
        .map_err(|error| format!("daemon not reachable: {error}"))?;
    let (mut write_half, mut read_half) = connection.split();
    let request = Request::new(RequestId::Number(1), methods::DAEMON_SHUTDOWN, None);
    let frame = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    zamin_bridge::send_frame(&mut write_half, &frame)
        .await
        .map_err(|error| format!("shutdown ask failed: {error}"))?;
    // The reply is courtesy, not a barrier: read it with a small budget
    // so a wedged daemon cannot hold the Quit hostage.
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), read_half.recv()).await;
    Ok(())
}

/// `Some(enabled)` — autostart state; `None` — cannot be determined and
/// the webview shows "unavailable".
#[tauri::command]
async fn autostart_get() -> Result<Option<bool>, String> {
    Ok(autostart::get())
}

/// Turn login autostart on or off (XDG autostart entry / HKCU Run key).
#[tauri::command]
async fn autostart_set(enabled: bool) -> Result<(), String> {
    autostart::set(enabled)
}

/// The one window that parks in the tray: closing it hides it, and
/// quitting happens only through the tray menu's explicit Quit. Runtime
/// windows (tab tear-offs, popups) keep their normal close semantics.
const TRAY_WINDOW: &str = "main";

/// Show/raise the tray window. Left-clicking the tray icon and the menu's
/// Open item both land here.
fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window(TRAY_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        // The update lane (ADR-0024): the webview drives check/install/relaunch
        // through these plugins; the host adds no policy of its own.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // Feedback's send routes (ADR-0028): the webview composes the issue
        // and picks the route; these perform the OS calls — the browser
        // open, and the clipboard handoff for a pasted screenshot.
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(HostState::default())
        .manage(ShellState::new())
        // The shell (ADR-0033): restore the session before any webview
        // asks; the restored primary tab's webview is created lazily by
        // sync once the frame reports its size.
        .setup(|app| {
            let handle = app.handle().clone();
            app.state::<ShellState>().restore(&handle);

            // The tray (§12.1 seam): closing the window parks ZIM here
            // instead of leaving a taskbar ghost, and the menu's Quit is
            // the one action that ends the process. The daemon and its
            // servers are deliberately detached (daemon_ensure.rs) —
            // quitting the panel does not stop servers; that is the
            // product's own topology, not an accident.
            use tauri::menu::{Menu, MenuItem};
            let open = MenuItem::with_id(app, "tray-open", "Open ZIM", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "tray-quit", "Quit ZIM", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = tauri::tray::TrayIconBuilder::with_id("zim-tray")
                .menu(&menu)
                // Left click opens the window; the menu belongs to the
                // right click, like every Windows tray icon.
                .show_menu_on_left_click(false)
                .tooltip("ZIM — quit stops the servers and the daemon")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "tray-open" => show_main_window(app),
                    // The one full shutdown: the daemon is asked to stop
                    // (it runs its graceful stop ladder for every running
                    // server, then exits), and the panel ends with it —
                    // Quit means nothing lingers: no tray icon, no
                    // zamind.exe, no headless servers. The ask is
                    // best-effort with a short budget; the panel always
                    // exits (a daemon that already died must not wedge
                    // the Quit).
                    "tray-quit" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = shutdown_daemon().await;
                            handle.exit(0);
                        });
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        // Geometry is model-visible: every window resize re-runs the
        // layout law so the frame band and tab webviews stay exact. The
        // relayout rides the async runtime — the event loop's own callback
        // must never birth a webview (the re-entrancy law, shell/host.rs:
        // on Windows it deadlocks the boot IPC and whites the window).
        .on_window_event(|window, event| {
            match event {
                tauri::WindowEvent::Resized(_) => {
                    let app = window.app_handle().clone();
                    let label = window.label().to_owned();
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<ShellState>();
                        shell::host::relayout_window(&app, &state, &label);
                    });
                }
                // Closing the primary window parks ZIM in the tray: the
                // close is prevented, the window hides, the session (and
                // every tab's webview) stays alive. Quitting happens only
                // through the tray menu. Tear-offs and popups close for
                // real — the tray is the primary window's parking spot,
                // not every window's.
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    if window.label() == TRAY_WINDOW {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                // A runtime window's death drops its strip (shell
                // hygiene); the primary window's strip IS the session
                // and is never dropped (drop_strip's own law).
                tauri::WindowEvent::Destroyed => {
                    let app = window.app_handle().clone();
                    let label = window.label().to_owned();
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<ShellState>();
                        state.drop_strip(&label);
                    });
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            daemon_connect,
            daemon_send,
            daemon_close,
            daemon_ensure,
            autostart_get,
            autostart_set,
            shell::host::shell_boot,
            shell::host::shell_snapshot,
            shell::host::shell_tab_hello,
            shell::host::shell_tab_navigate,
            shell::host::shell_tab_action,
            shell::host::shell_command,
            shell::host::shell_omnibox_classify,
            shell::host::shell_omnibox_commit,
            shell::host::shell_bookmarks,
            shell::host::shell_bookmark_remove,
            shell::host::shell_bookmark_add,
            shell::host::shell_drag,
            shell::host::shell_window_resized,
            shell::host::shell_popup,
            shell::host::shell_popup_boot,
            shell::host::shell_popup_close,
            shell::host::shell_popup_update,
            shell::host::shell_popup_fade,
            shell::host::shell_popup_dismiss
        ])
        // The run loop owns two more tray laws: with the window hidden the
        // OS never sees a "last window closed" moment, but a hidden window
        // plus a stray runtime close would still ask to exit — with no
        // exit code (i.e. not an explicit `app.exit`) the tray keeps the
        // process alive. Only the tray menu's Quit (exit code Some) ends
        // it. The restart exit code is honored by the runtime itself.
        // Whatever ends the process, the session lands first: the save
        // pump may still sit inside its debounce window when the exit is
        // requested, so the loop flushes synchronously here.
        .build(tauri::generate_context!())
        .expect("error while building the ZIM host")
        .run(|app, event| {
            match event {
                tauri::RunEvent::ExitRequested { code, api, .. } => {
                    app.state::<ShellState>().flush();
                    if code.is_none() {
                        api.prevent_exit();
                    }
                }
                // A tear-off (or any non-tray window) closed for real:
                // its tabs' wires must not linger as dead sessions on
                // the daemon.
                tauri::RunEvent::WindowEvent { event, label, .. } => {
                    if matches!(event, tauri::WindowEvent::Destroyed) && label != "main" {
                        drop_wire_window(app, &label);
                    }
                }
                _ => {}
            }
        });
}
