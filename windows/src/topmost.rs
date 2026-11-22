// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeping the widget in Windows' always-on-top layer.
//!
//! Creating the window with `WS_EX_TOPMOST` is not always enough: other programs can push
//! a topmost window back among ordinary windows (seen with a maximized terminal covering
//! the widget even though its topmost flag was still set). The widget then stays hidden
//! until it is re-asserted, so `widget.rs` calls `ensure_topmost` on a short timer.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::*;

fn is_topmost(hwnd: HWND) -> bool {
    unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0 }
}

/// True if `hwnd` has left the topmost layer: its topmost flag is gone, or an ordinary
/// (non-topmost) window sits above it in the z-order. In a healthy state only other
/// topmost windows, such as menus and tooltips, can be above it.
pub fn lost_topmost(hwnd: HWND) -> bool {
    if !is_topmost(hwnd) {
        return true;
    }
    let mut above = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
    while let Ok(h) = above {
        if h.is_invalid() {
            break;
        }
        if !is_topmost(h) {
            return true;
        }
        above = unsafe { GetWindow(h, GW_HWNDPREV) };
    }
    false
}

/// Put `hwnd` back on top of the topmost layer if it has left it, without moving,
/// resizing or activating it. Returns true if a repair was needed. Doing nothing in the
/// healthy case means the widget never jumps above its own menu or other topmost windows.
pub fn ensure_topmost(hwnd: HWND) -> bool {
    if !lost_topmost(hwnd) {
        return false;
    }
    unsafe {
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;

    /// A hidden, off-screen test window (z-order applies to hidden windows too, and
    /// nothing flashes on the user's screen).
    fn hidden_window(ex: WINDOW_EX_STYLE) -> HWND {
        unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            CreateWindowExW(ex, w!("STATIC"), w!("topmost-test"), WS_POPUP, -5000, -5000, 10, 10, None, None, Some(inst.into()), None)
                .unwrap()
        }
    }

    #[test]
    fn detects_and_repairs_a_widget_pushed_out_of_the_topmost_layer() {
        let widget = hidden_window(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE);
        let other = hidden_window(WS_EX_TOOLWINDOW); // an ordinary window, like a maximized app
        unsafe {
            assert!(!lost_topmost(widget), "a fresh topmost window is in the topmost layer");
            assert!(!ensure_topmost(widget), "nothing to repair");

            // Push it out of the topmost layer: it now sits among ordinary windows,
            // possibly underneath them (what the user saw: Warp covering the widget).
            SetWindowPos(widget, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).unwrap();
            SetWindowPos(other, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE).unwrap();
            assert!(lost_topmost(widget));

            assert!(ensure_topmost(widget), "repair reported");
            assert!(!lost_topmost(widget), "back in the topmost layer");
            let _ = DestroyWindow(other);
            let _ = DestroyWindow(widget);
        }
    }
}
