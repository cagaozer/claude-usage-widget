//! Platform-neutral core of Claude Usage Widget.
//!
//! Everything here is plain Rust with no OS APIs, so it is shared by the Windows and
//! macOS front ends and tested on both: parsing claude.ai's usage response, polling it on
//! a background thread, turning state into what the widget draws, and settings.

pub mod fetcher;
pub mod format;
pub mod geometry;
pub mod session;
pub mod settings;
pub mod source;
pub mod usage;
pub mod view;
pub mod controller;
pub mod layout;
pub mod menu;
pub mod instance;
#[cfg(debug_assertions)]
pub mod demo;
