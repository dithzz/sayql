//! The macOS menu bar. Tauri's default menu gives ⌘W to Close Window, so the shortcut never
//! reaches the page and closes SayQL instead of the tab. This menu is the default one with
//! ⌘W as Close Tab (Close Window moves to ⇧⌘W), Settings… on ⌘, and a Help menu.
//!
//! Items other than Close Window are handled by the page: their id is sent as a
//! `menu://action` event (see src/app/Workbench.tsx). Windows and Linux have no menu bar, so
//! there the page sees the keys itself.

use tauri::menu::{
    AboutMetadata, HELP_SUBMENU_ID, Menu, MenuItem, PredefinedMenuItem, Submenu, WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// The event the page listens to; the payload is one of the ids below.
pub const EVENT: &str = "menu://action";
pub const CLOSE_TAB: &str = "close-tab";
pub const SETTINGS: &str = "settings";
pub const SHORTCUTS: &str = "shortcuts";
pub const WHATS_NEW: &str = "whats-new";
pub const DOCS: &str = "docs";
const CLOSE_WINDOW: &str = "close-window";

/// The menu bar, built when the app starts.
pub fn build<R: Runtime>(h: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let pkg = h.package_info();
    let about = AboutMetadata {
        name: Some(pkg.name.clone()),
        version: Some(pkg.version.to_string()),
        copyright: h.config().bundle.copyright.clone(),
        authors: h.config().bundle.publisher.clone().map(|p| vec![p]),
        ..Default::default()
    };
    let item =
        |id: &str, text: &str, accel: Option<&str>| MenuItem::with_id(h, id, text, true, accel);
    let sep = || PredefinedMenuItem::separator(h);

    let app_menu = Submenu::with_items(
        h,
        pkg.name.clone(),
        true,
        &[
            &PredefinedMenuItem::about(h, None, Some(about))?,
            &sep()?,
            &item(SETTINGS, "Settings…", Some("CmdOrCtrl+,"))?,
            &sep()?,
            &PredefinedMenuItem::services(h, None)?,
            &sep()?,
            &PredefinedMenuItem::hide(h, None)?,
            &PredefinedMenuItem::hide_others(h, None)?,
            &sep()?,
            &PredefinedMenuItem::quit(h, None)?,
        ],
    )?;
    let file = Submenu::with_items(
        h,
        "File",
        true,
        &[
            &item(CLOSE_TAB, "Close Tab", Some("CmdOrCtrl+W"))?,
            &item(CLOSE_WINDOW, "Close Window", Some("CmdOrCtrl+Shift+W"))?,
        ],
    )?;
    // The Edit items are what make copy and paste work in the page on macOS.
    let edit = Submenu::with_items(
        h,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(h, None)?,
            &PredefinedMenuItem::redo(h, None)?,
            &sep()?,
            &PredefinedMenuItem::cut(h, None)?,
            &PredefinedMenuItem::copy(h, None)?,
            &PredefinedMenuItem::paste(h, None)?,
            &PredefinedMenuItem::select_all(h, None)?,
        ],
    )?;
    let view = Submenu::with_items(
        h,
        "View",
        true,
        &[&PredefinedMenuItem::fullscreen(h, None)?],
    )?;
    let window = Submenu::with_id_and_items(
        h,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(h, None)?,
            &PredefinedMenuItem::maximize(h, None)?,
        ],
    )?;
    let help = Submenu::with_id_and_items(
        h,
        HELP_SUBMENU_ID,
        "Help",
        true,
        &[
            &item(SHORTCUTS, "Keyboard Shortcuts", None)?,
            &item(WHATS_NEW, "What's New", None)?,
            &item(DOCS, "SayQL Docs", None)?,
        ],
    )?;
    Menu::with_items(h, &[&app_menu, &file, &edit, &view, &window, &help])
}

/// Closes the focused window for Close Window; hands every other item to the page.
pub fn on_event<R: Runtime>(h: &AppHandle<R>, id: &str) {
    if id == CLOSE_WINDOW {
        if let Some(w) = h
            .webview_windows()
            .values()
            .find(|w| w.is_focused().unwrap_or(false))
        {
            let _ = w.close();
        }
        return;
    }
    if [CLOSE_TAB, SETTINGS, SHORTCUTS, WHATS_NEW, DOCS].contains(&id) {
        let _ = h.emit(EVENT, id);
    }
}
