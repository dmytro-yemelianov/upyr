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
    // a parent terminal for all invocations (including failed GUI startup)
    // without opening a console
    // for ordinary Explorer/menu launches. Redirected handles remain usable.
    #[cfg(target_os = "windows")]
    {
        // SAFETY: this API takes a process-id sentinel and no pointers.
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn,upyr=info")),
        )
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    let result = settings::run_with(Cli::parse().options);
    #[cfg(target_os = "windows")]
    if let Err(error) = &result {
        eprintln!("Upyr Settings: {error:#}");
        show_error(error);
    }
    result
}

/// Explorer/tray launches have no terminal. Keep startup failures visible even
/// when the graphics device could not be created; this dialog uses native GDI.
#[cfg(target_os = "windows")]
fn show_error(error: &anyhow::Error) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    let message: Vec<u16> = format!(
        "Не вдалося відкрити налаштування Upyr.\n\n{error:#}\n\n\
         Термінальний редактор: upyr.exe settings --tui\n\
         Діагностика: upyr.exe doctor"
    )
    .encode_utf16()
    .chain(Some(0))
    .collect();
    let title: Vec<u16> = "Upyr Settings".encode_utf16().chain(Some(0)).collect();
    // SAFETY: both UTF-16 strings are NUL-terminated and live until the dialog
    // returns; a null owner is valid before the settings window exists.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            message.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
