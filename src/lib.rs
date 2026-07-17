#![deny(unsafe_code)]

#[cfg(target_os = "macos")]
pub mod accessibility;
pub mod app;
mod auto_correct;
mod auto_correct_monitor;
pub mod automation;
pub mod autostart;
mod clipboard_guard;
pub mod config;
mod config_watcher;
mod feedback;
pub mod layout;
mod modifier_gesture;
pub mod settings;
pub mod system_layout;
pub mod tray;

pub use layout::{Conversion, Direction, convert, convert_with_mapping, resolve_direction};
