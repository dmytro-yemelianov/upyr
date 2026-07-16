#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use anyhow::Result;
use upyr::settings;

fn main() -> Result<()> {
    settings::run()
}
