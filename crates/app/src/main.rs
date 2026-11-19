//! The Swarmed desktop app: a thin Tauri shell over `swarmed-core`.
#![allow(
    clippy::multiple_crate_versions,
    reason = "Tauri's dependency tree carries the duplicates; nothing of ours adds one"
)]
// Without this a release build on Windows opens a console window next to the app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use std::sync::Arc;

use swarmed_core::agent::FakeBackend;
use swarmed_core::{Core, Paths, SessionEvent};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast::Receiver;
use tokio::sync::broadcast::error::RecvError;

/// The event every session event is emitted as.
const SESSION_EVENT: &str = "session-event";
/// Emitted when the UI fell too far behind to be sent every event and must reload history.
const SESSION_EVENTS_LAGGED: &str = "session-events-lagged";

/// `SWARMED_FAKE_AGENT` swaps Claude for the scripted agent, to work on the UI without tokens.
fn open_core() -> swarmed_core::Result<Core> {
    let paths = Paths::from_env()?;
    if std::env::var_os("SWARMED_FAKE_AGENT").is_some() {
        return Core::with_backend(paths, Arc::new(FakeBackend::default()));
    }
    Core::open(paths)
}

/// Passes every session event on to the UI until the core goes away.
async fn forward_events(mut bus: Receiver<SessionEvent>, app: AppHandle) {
    loop {
        // An emit only fails when there is no window to receive it, which is not an error here.
        match bus.recv().await {
            Ok(event) => app.emit(SESSION_EVENT, event).ok(),
            Err(RecvError::Lagged(_)) => app.emit(SESSION_EVENTS_LAGGED, ()).ok(),
            Err(RecvError::Closed) => return,
        };
    }
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let core = Arc::new(open_core()?);
            drop(tauri::async_runtime::spawn(forward_events(
                core.subscribe(),
                app.handle().clone(),
            )));
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::overview,
            commands::project_add,
            commands::task_new,
            commands::task_remove,
            commands::session_new,
            commands::session_events,
            commands::session_send,
            commands::session_allow,
            commands::session_deny,
            commands::session_interrupt,
            commands::session_stop,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| panic!("could not start Swarmed: {e}"));
}
