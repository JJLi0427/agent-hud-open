//! Where the pointer is and what its buttons hold: state queries only. NSEvent's
//! class properties answer outside a non-key window, so a HUD that passes events
//! through still knows a drag is happening, and the main run loop is not woken
//! to ask. This is not an event tap and not a global keyboard listener.
use super::super::{Point, PointerKeys};
use objc2_foundation::NSPoint;

// The display whose height is the plane NSEvent reports in: the main one.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayPixelsHigh(display: u32) -> usize;
}

pub(in crate::hud) fn left_mouse_down() -> Result<bool, String> {
    // NSEvent's public class property reports button state outside our non-key window as
    // well. This is a state query, not an event tap or global keyboard listener; AX is unused.
    let buttons: usize = unsafe { objc2::msg_send![objc2::class!(NSEvent), pressedMouseButtons] };
    Ok(buttons & 1 != 0)
}

pub(in crate::hud) fn pointer_keys() -> PointerKeys {
    // Public class properties also work when this non-key window passes events through.
    // NSEvent and DOM use the same left/right/middle bits for the first five buttons.
    let buttons: usize = unsafe { objc2::msg_send![objc2::class!(NSEvent), pressedMouseButtons] };
    let modifiers: usize = unsafe { objc2::msg_send![objc2::class!(NSEvent), modifierFlags] };
    PointerKeys {
        buttons: Some((buttons & 0x1f) as u32),
        alt: Some(modifiers & (1 << 19) != 0),
        ctrl: Some(modifiers & (1 << 18) != 0),
        meta: Some(modifiers & (1 << 20) != 0),
        shift: Some(modifiers & (1 << 17) != 0),
    }
}

pub(in crate::hud) fn global_pointer(app: &tauri::AppHandle) -> Result<Point, String> {
    let _ = app;
    // NSEvent.mouseLocation and CoreGraphics display pixel height are thread-safe global
    // state queries, just like pressedMouseButtons and modifierFlags above. Reading them
    // directly avoids waking the main run loop every 32 ms while the pointer is at rest.
    let point: NSPoint = unsafe { objc2::msg_send![objc2::class!(NSEvent), mouseLocation] };
    let primary_height = unsafe { CGDisplayPixelsHigh(CGMainDisplayID()) } as f64;
    Ok(Point {
        x: point.x,
        y: primary_height - point.y,
    })
}
