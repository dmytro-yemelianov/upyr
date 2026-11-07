#![deny(unsafe_code)]

#[cfg(target_os = "macos")]
pub mod accessibility;
pub mod app;
pub mod audio_output;
mod auto_correct;
mod auto_correct_monitor;
pub mod automation;
pub mod autostart;
pub mod caret;
mod child_processes;
mod clipboard_guard;
pub mod config;
mod config_watcher;
mod feedback;
#[cfg(target_os = "macos")]
mod function_keys;
pub mod globe_switch;
mod hotkey_bindings;
mod input_context;
// Only the macOS app has undo and the guarded pipeline that learning uses.
pub mod layout;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod learning;
mod modifier_gesture;
pub mod settings;
pub mod shell_init;
#[cfg(any(target_os = "macos", test))]
mod slash_commands;
pub mod system_layout;
pub mod tray;

pub use layout::{Conversion, Direction, convert, convert_with_mapping, resolve_direction};
