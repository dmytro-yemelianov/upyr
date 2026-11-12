#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::Result;
use clap::Parser;
use upyr::settings;

#[derive(Parser)]
#[command(
    version,
    about = "Upyr settings: native, pixel-style, or local terminal editor"
)]
struct Cli {
    #[command(flatten)]
    options: settings::SettingsOptions,
}

fn main() -> Result<()> {
    // The GUI subsystem does not automatically attach standard handles. Reuse
    // a parent terminal for explicit CLI invocations without opening a console
    // for ordinary Explorer/menu launches. Redirected handles remain usable.
    #[cfg(target_os = "windows")]
    if std::env::args_os().skip(1).any(|arg| {
        matches!(
            arg.to_str(),
            Some("--tui" | "--help" | "-h" | "--version" | "-V")
        )
    }) {
        // SAFETY: this API takes a process-id sentinel and no pointers.
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    settings::run_with(Cli::parse().options)
}
