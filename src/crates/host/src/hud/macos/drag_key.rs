//! The keys a HUD drag takes: macOS draws the grabbing cursor only for the key
//! window, so the panel becomes key while held without activating the app. A
//! drag is not a text gesture, so it does not hand the web view first
//! responder the way the explicit text lease does.

use super::on_main;
use objc2::runtime::NSObjectProtocol;
use objc2_app_kit::NSWindow;
use tauri::Manager;

/// A drag takes keys so macOS draws the grabbing cursor, but it is not a text
/// gesture: the page has nothing to type into and the drag is polled natively.
/// Handing the web view first responder would make it the key window's input
/// client, so AppKit would attach a Text Services session to a panel that gives
/// the keyboard straight back. Only the explicit text lease does that.
pub(in crate::hud) fn begin_drag_key(window: &tauri::WebviewWindow) -> Result<(), String> {
    let retained_window = window.clone();
    on_main(window.app_handle(), move |_| {
        let pointer = retained_window
            .ns_window()
            .map_err(|error| error.to_string())?;
        if pointer.is_null() {
            return Err("The HUD native window is unavailable".into());
        }
        let native = unsafe { &*pointer.cast::<NSWindow>() };
        if native.isKindOfClass(objc2::class!(NSPanel)) {
            native.makeKeyWindow();
            if !native.isKeyWindow() {
                return Err("The HUD panel could not obtain keyboard focus".into());
            }
            return Ok(());
        }
        native.makeKeyAndOrderFront(None);
        Ok(())
    })
}

pub(in crate::hud) fn resign_drag_key(window: &tauri::WebviewWindow) -> Result<(), String> {
    let retained_window = window.clone();
    on_main(window.app_handle(), move |_| {
        let pointer = retained_window
            .ns_window()
            .map_err(|error| error.to_string())?;
        if !pointer.is_null() {
            let native = unsafe { &*pointer.cast::<NSWindow>() };
            if native.isKeyWindow() {
                native.makeFirstResponder(None);
                native.resignKeyWindow();
            }
        }
        Ok(())
    })
}
