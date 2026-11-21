//! Windows start-up: single-instance guard, COM init, settings/session loading, and the
//! sign-in hook the widget calls.

use std::process::ExitCode;
use crate::dpapi_store::DpapiStore;
use crate::paths;
use crate::signin;
use crate::widget::{self, StartMode};
use usage_core::session::{Session, SessionStore};
use usage_core::settings;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, PostMessageW, MB_ICONWARNING, MB_OK, WM_CLOSE};
use windows::Win32::System::Threading::CreateMutexW;

pub fn main() -> ExitCode {
    // Single instance: a second launch exits silently.
    let mutex = unsafe { CreateMutexW(None, true, w!(r"Local\ClaudeUsageWidget.SingleInstance")) };
    let Ok(mutex) = mutex else { return ExitCode::FAILURE };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return ExitCode::SUCCESS;
    }

    unsafe {
        // WebView2 requires a single-threaded apartment on the UI thread.
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    let dir = paths::app_dir();
    let settings = settings::Settings::load(&dir);
    let start = start_mode(&dir);
    let code = widget::run(settings, start, sign_in);
    unsafe {
        windows::Win32::System::Com::CoUninitialize();
        let _ = CloseHandle(mutex);
    }
    code
}

fn start_mode(dir: &std::path::Path) -> StartMode {
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--demo") {
        return StartMode::Demo;
    }
    match (DpapiStore { dir: dir.to_path_buf() }).load() {
        Some(s) => StartMode::Session(s),
        None => StartMode::NeedSignIn,
    }
}

/// Sign-in hook for the widget: shows the WebView2 window. The controller stores the
/// returned session.
fn sign_in(owner: HWND) -> Option<Session> {
    match signin::sign_in(owner) {
        Ok(s) => Some(s),
        Err(signin::SignInError::Cancelled) => None,
        Err(signin::SignInError::WebView2Missing) => {
            message_box(
                owner,
                concat!(
                    "Claude Usage Widget needs the Microsoft Edge WebView2 Runtime, which isn't installed.",
                    "\n\nDownload it from:\nhttps://go.microsoft.com/fwlink/p/?LinkId=2124703"
                ),
            );
            unsafe {
                let _ = PostMessageW(Some(owner), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            None
        }
        Err(signin::SignInError::Failed(e)) => {
            message_box(owner, &format!("Sign-in failed: {e}"));
            None
        }
    }
}

fn message_box(owner: HWND, text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain([0]).collect();
    unsafe {
        MessageBoxW(Some(owner), PCWSTR(text.as_ptr()), w!("Claude Usage Widget"), MB_OK | MB_ICONWARNING);
    }
}

/// Removes the WebView2 profile folder (used on sign-out).
pub fn signin_cleanup() {
    signin::cleanup_profile();
}
