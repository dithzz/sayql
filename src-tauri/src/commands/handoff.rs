//! Receives `sayql://` handoffs from AI apps (see src/handoff.rs) and team invite links, and
//! passes the valid ones to the window, which shows them for the person to act on.

use crate::handoff::{self, Handoff};
use crate::state::AppState;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};

/// Handoffs the window hasn't picked up yet: a link can arrive before the UI has loaded.
#[derive(Default)]
pub struct PendingHandoffs(Mutex<Vec<Handoff>>);

/// Checks each link and queues the valid ones; links without a fresh token are dropped.
pub fn receive(app: &AppHandle, links: impl IntoIterator<Item = String>) {
    let state = app.state::<AppState>();
    let mut any = false;
    for link in links {
        if let Some(join) = handoff::parse_team_join(&link) {
            match join {
                Ok(h) => {
                    crate::state::lock(&app.state::<PendingHandoffs>().0).push(h);
                    any = true;
                }
                Err(_) => eprintln!("sayql: ignored an invalid team invite link"),
            }
            continue;
        }
        match handoff::parse_url(&link) {
            Ok((h, token)) if handoff::redeem_token(state.store.dir(), &token) => {
                crate::state::lock(&app.state::<PendingHandoffs>().0).push(h);
                any = true;
            }
            // The link's contents aren't logged: it's untrusted and could hold anything.
            Ok(_) => eprintln!("sayql: ignored a sayql:// link that didn't come from sayql-mcp"),
            Err(_) => eprintln!("sayql: ignored an invalid sayql:// link"),
        }
    }
    if any {
        let _ = app.emit("handoff://new", ());
        focus_main(app);
    }
}

/// Brings the main window to the front.
pub fn focus_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The handoffs waiting for the window, oldest first.
#[tauri::command]
pub fn take_handoffs(pending: State<'_, PendingHandoffs>) -> Vec<Handoff> {
    std::mem::take(&mut *crate::state::lock(&pending.0))
}
