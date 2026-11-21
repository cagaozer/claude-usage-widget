//! Claude Usage Widget for macOS: entry point.
//!
//! Every module is macOS-only. On other targets this crate builds an empty program so
//! that `cargo build --workspace` works on any OS.

#[cfg(target_os = "macos")]
mod keychain;
#[cfg(target_os = "macos")]
mod paths;

fn main() {}
