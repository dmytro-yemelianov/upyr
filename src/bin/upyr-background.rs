#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::Result;
use tracing_subscriber::EnvFilter;
use upyr::{app, config::Config};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("upyr=info")),
        )
        .with_target(false)
        .init();
    // An unreadable configuration must not leave the user with no running app
    // and no way to fix it: start with defaults and say so.
    let (config, problem) = Config::load_or_default();
    if let Some(problem) = &problem {
        tracing::error!(%problem, "configuration could not be read; running with defaults");
        #[cfg(target_os = "macos")]
        if upyr::accessibility::alert_invalid_config(problem) {
            if let Err(error) = upyr::settings::spawn() {
                tracing::error!(%error, "could not open Upyr Settings");
            }
        }
    }
    app::run(config, None)
}
