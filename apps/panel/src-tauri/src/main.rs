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

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Mutex, MutexGuard, PoisonError};

use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;
use zamin_ipc::Endpoint;

mod autostart;
mod daemon_ensure;

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
    frames: Channel<String>,
    down: Channel<()>,
    state: State<'_, HostState>,
) -> Result<String, String> {
    let resolved = match endpoint {
        Some(value) => Endpoint::from_daemon_arg(&value),
        None => Endpoint::default_endpoint(),
    };
    let connection = zamin_ipc::connect(resolved)
        .await
        .map_err(|error| format!("could not connect to the daemon: {error}"))?;
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
        .manage(HostState::default())
        .invoke_handler(tauri::generate_handler![
            daemon_connect,
            daemon_send,
            daemon_close,
            daemon_ensure,
            autostart_get,
            autostart_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running the ZaminPanel host");
}
