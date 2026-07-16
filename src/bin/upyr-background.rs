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
    app::run(Config::load()?)
}
