#![windows_subsystem = "windows"]

mod fetcher;
mod format;
mod gdi;
mod geometry;
mod session;
mod settings;
mod source;
mod usage;
mod view;
mod widget;

use std::process::ExitCode;
use widget::StartMode;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows::Win32::System::Threading::CreateMutexW;

fn main() -> ExitCode {
    // Single instance: a second launch exits silently.
    let mutex = unsafe { CreateMutexW(None, true, w!(r"Local\ClaudeUsageWidget.SingleInstance")) };
    let Ok(mutex) = mutex else { return ExitCode::FAILURE };
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        unsafe {
            let _ = CloseHandle(mutex);
        }
        return ExitCode::SUCCESS;
    }

    let dir = settings::app_dir();
    let settings = settings::Settings::load(&dir);
    let start = start_mode(&dir);
    let code = widget::run(settings, start, sign_in);
    unsafe {
        let _ = CloseHandle(mutex);
    }
    code
}

fn start_mode(dir: &std::path::Path) -> StartMode {
    #[cfg(debug_assertions)]
    if std::env::args().any(|a| a == "--demo") {
        return StartMode::Demo;
    }
    match session::load_session(dir) {
        Some(s) => StartMode::Session(s),
        None => StartMode::NeedSignIn,
    }
}

/// Placeholder until Task 7 adds the WebView2 sign-in window.
fn sign_in(_owner: HWND) -> Option<session::Session> {
    None
}

/// Placeholder until Task 7: removes the WebView2 profile on sign-out.
pub fn signin_cleanup() {}
