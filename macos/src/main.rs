//! Claude Usage Widget for macOS: entry point.
//!
//! Every module is macOS-only. On other targets this crate builds an empty program so
//! that `cargo build --workspace` works on any OS.

#[cfg(target_os = "macos")]
mod app;
#[cfg(target_os = "macos")]
mod draw;
#[cfg(target_os = "macos")]
mod keychain;
#[cfg(target_os = "macos")]
mod menu;
#[cfg(target_os = "macos")]
mod panel;
#[cfg(target_os = "macos")]
mod paths;
#[cfg(target_os = "macos")]
mod signin;

#[cfg(target_os = "macos")]
fn main() {
    use usage_core::session::SessionStore;

    // Single instance: a second launch exits silently. The lock is held until exit.
    let Some(_lock) = usage_core::instance::lock_single_instance(&paths::app_dir()) else {
        return;
    };

    let args: Vec<String> = std::env::args().collect();
    #[cfg(debug_assertions)]
    let quit_after = args
        .iter()
        .position(|a| a == "--quit-after")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<f64>().ok());
    #[cfg(not(debug_assertions))]
    let quit_after = None;

    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--demo") {
        app::run(app::StartMode::Demo, quit_after);
        return;
    }
    // CI check: open the sign-in window without touching the stored session.
    #[cfg(debug_assertions)]
    if args.iter().any(|a| a == "--signin-smoke") {
        app::run(app::StartMode::NeedSignIn, quit_after);
        return;
    }
    let _ = &args;

    let start = match keychain::KeychainStore::new().load() {
        Some(s) => app::StartMode::Session(s),
        None => app::StartMode::NeedSignIn,
    };
    app::run(start, quit_after);
}

#[cfg(not(target_os = "macos"))]
fn main() {}
