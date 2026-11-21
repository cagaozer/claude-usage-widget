// SPDX-License-Identifier: GPL-3.0-or-later
//! Claude Usage Widget for Windows: entry point.
//!
//! Every module is Windows-only. On other targets this crate builds an empty program so
//! that `cargo build --workspace` works on any OS.

#![windows_subsystem = "windows"]

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod dpapi_store;
#[cfg(windows)]
mod gdi;
#[cfg(windows)]
mod paths;
#[cfg(windows)]
mod signin;
#[cfg(windows)]
mod widget;

#[cfg(windows)]
fn main() -> std::process::ExitCode {
    app::main()
}

#[cfg(not(windows))]
fn main() {}
