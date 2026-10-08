//! ZaminPanel desktop host (ADR-0003): a bridge, nothing more.
//!
//! The webview owns the protocol client — handshake, correlation,
//! subscriptions, reconnect, cursors. This host owns exactly three things:
//! the daemon connection's lifetime, frame forwarding in both directions,
//! and the ~50 ms coalescing of daemon → webview frames (which lives in
//! `zamin-bridge`, tested in the workspace). No business logic belongs
//! here, so none is here.
//!
//! The Phase 7 additions stay inside that boundary: `daemon_ensure` makes
//! "double-clicking ZaminPanel must never show a daemon error"
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

use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::ipc::Channel;
use tauri::{Manager, State};
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

#[derive(Default)]
struct HostState(Mutex<Option<Connected>>);

impl HostState {
    fn lock(&self) -> MutexGuard<'_, Option<Connected>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[tauri::command]
async fn daemon_connect(
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
            if zamin_bridge::send_frame(&mut write_half, &frame).await.is_err() {
                break;
            }
        }
    });

    let mut guard = state.lock();
    if let Some(previous) = guard.take() {
        previous.shutdown();
    }
    *guard = Some(Connected {
        outgoing: outgoing_tx,
        read_pump,
        batch_forwarder,
        write_pump,
    });
    Ok("connected".into())
}

#[tauri::command]
async fn daemon_send(frame: String, state: State<'_, HostState>) -> Result<(), String> {
    let outgoing = state
        .lock()
        .as_ref()
        .map(|connected| connected.outgoing.clone())
        .ok_or_else(|| "not connected to the daemon".to_owned())?;
    outgoing
        .send(frame)
        .await
        .map_err(|error| format!("the daemon connection is gone: {error}"))
}

#[tauri::command]
async fn daemon_close(state: State<'_, HostState>) -> Result<(), String> {
    if let Some(connected) = state.lock().take() {
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
            Ok(())
        })
        // Geometry is model-visible: every window resize re-runs the
        // layout law so the frame band and tab webviews stay exact. The
        // relayout rides the async runtime — the event loop's own callback
        // must never birth a webview (the re-entrancy law, shell/host.rs:
        // on Windows it deadlocks the boot IPC and whites the window).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(_) = event {
                let app = window.app_handle().clone();
                let label = window.label().to_owned();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<ShellState>();
                    shell::host::relayout_window(&app, &state, &label);
                });
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
            shell::host::shell_drag,
            shell::host::shell_window_resized
        ])
        .run(tauri::generate_context!())
        .expect("error while running the ZaminPanel host");
}
