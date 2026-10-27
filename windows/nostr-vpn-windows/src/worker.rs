// Background thread that owns the FfiApp. The UI thread sends actions and
// coalesced refreshes over a channel; results come back with full state.
// Mirrors linux/src/main/core_worker.rs.

use std::sync::mpsc::{Receiver, Sender};

use nostr_vpn_app_core::{FfiApp, NativeAppAction, NativeAppState};

pub struct CoreResult {
    pub is_action: bool,
    pub state: NativeAppState,
}

pub type RequestSender = Sender<Option<NativeAppAction>>;
pub type ResultReceiver = Receiver<CoreResult>;

pub fn spawn(data_dir: String, version: String) -> (RequestSender, ResultReceiver) {
    let (requests, incoming) = std::sync::mpsc::channel::<Option<NativeAppAction>>();
    let (outgoing, results) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        // Construction also reads daemon/service status and may wait on the CLI.
        let core = FfiApp::new(data_dir, version);
        if outgoing
            .send(CoreResult {
                is_action: false,
                state: core.state(),
            })
            .is_err()
        {
            return;
        }
        // Preserve command order without blocking the UI thread on daemon IPC.
        // Refreshes are coalesced by the UI; only explicit actions can queue.
        while let Ok(action) = incoming.recv() {
            let is_action = action.is_some();
            let state = match action {
                Some(action) => core.dispatch(action),
                None => core.refresh(),
            };
            if outgoing.send(CoreResult { is_action, state }).is_err() {
                break;
            }
        }
    });
    (requests, results)
}
