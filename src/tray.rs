use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

use crate::{autostart, config::Config};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ConvertPreviousWord,
    ConvertSelection,
    TogglePaused,
    OpenSettings,
    ReloadConfiguration,
    ToggleAutostart,
    Quit,
}

pub struct Tray {
    _icon: TrayIcon,
    status: MenuItem,
    previous_word: MenuItem,
    convert: MenuItem,
    pause: CheckMenuItem,
    edit_config: MenuItem,
    reload_config: MenuItem,
    autostart: CheckMenuItem,
    quit: MenuItem,
}

impl Tray {
    pub fn new(config: &Config) -> Result<Self> {
        let status = MenuItem::new(status_text(config, false), false, None);
        let previous_word = MenuItem::new("Fix previous word", true, None);
        let convert = MenuItem::new("Convert selected text", true, None);
        let pause = CheckMenuItem::new("Pause shortcut", true, false, None);
        let edit_config = MenuItem::new("Settings…", true, None);
        let reload_config = MenuItem::new("Reload configuration", true, None);
        let autostart_enabled = autostart::status()?.enabled;
        let autostart = CheckMenuItem::new("Launch at login", true, autostart_enabled, None);
        let quit = MenuItem::new("Quit Upyr", true, None);
        let first_separator = PredefinedMenuItem::separator();
        let second_separator = PredefinedMenuItem::separator();
        let third_separator = PredefinedMenuItem::separator();
        let menu = Menu::with_items(&[
            &status,
            &first_separator,
            &previous_word,
            &convert,
            &pause,
            &second_separator,
            &edit_config,
            &reload_config,
            &autostart,
            &third_separator,
            &quit,
        ])
        .context("failed to create tray menu")?;

        let icon = Icon::from_rgba(icon_rgba(), ICON_SIZE, ICON_SIZE)
            .context("failed to create tray icon pixels")?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip(config, false))
            .with_icon(icon)
            .with_icon_as_template(cfg!(target_os = "macos"))
            .build()
            .context("failed to create system tray icon")?;

        Ok(Self {
            _icon: icon,
            status,
            previous_word,
            convert,
            pause,
            edit_config,
            reload_config,
            autostart,
            quit,
        })
    }

    pub fn action(&self, event: &MenuEvent) -> Option<TrayAction> {
        let id = &event.id;
        if id == self.previous_word.id() {
            Some(TrayAction::ConvertPreviousWord)
        } else if id == self.convert.id() {
            Some(TrayAction::ConvertSelection)
        } else if id == self.pause.id() {
            Some(TrayAction::TogglePaused)
        } else if id == self.edit_config.id() {
            Some(TrayAction::OpenSettings)
        } else if id == self.reload_config.id() {
            Some(TrayAction::ReloadConfiguration)
        } else if id == self.autostart.id() {
            Some(TrayAction::ToggleAutostart)
        } else if id == self.quit.id() {
            Some(TrayAction::Quit)
        } else {
            None
        }
    }

    pub fn update(&self, config: &Config, paused: bool) -> Result<()> {
        self.pause.set_checked(paused);
        self.autostart.set_checked(autostart::status()?.enabled);
        self.status.set_text(status_text(config, paused));
        self._icon
            .set_tooltip(Some(tooltip(config, paused)))
            .context("failed to update tray tooltip")
    }
}

fn status_text(config: &Config, paused: bool) -> String {
    if paused {
        "Status: paused".to_owned()
    } else {
        format!("Shortcut: {}", config.hotkey)
    }
}

fn tooltip(config: &Config, paused: bool) -> String {
    if paused {
        "Upyr — paused".to_owned()
    } else {
        format!(
            "Upyr — selection: {}; previous word: {}",
            config.hotkey, config.last_word_hotkey
        )
    }
}

const ICON_SIZE: u32 = 22;

fn icon_rgba() -> Vec<u8> {
    let mut rgba = vec![0; (ICON_SIZE * ICON_SIZE * 4) as usize];
    for y in 0..ICON_SIZE as i32 {
        for x in 0..ICON_SIZE as i32 {
            let on_stroke = distance_to_segment(x, y, 4, 3, 4, 13) <= 1.5
                || distance_to_segment(x, y, 4, 13, 7, 17) <= 1.5
                || distance_to_segment(x, y, 7, 17, 11, 19) <= 1.5
                || distance_to_segment(x, y, 11, 19, 15, 17) <= 1.5
                || distance_to_segment(x, y, 15, 17, 18, 13) <= 1.5
                || distance_to_segment(x, y, 18, 13, 18, 3) <= 1.5;
            if on_stroke {
                let offset = ((y as u32 * ICON_SIZE + x as u32) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    rgba
}

#[allow(clippy::too_many_arguments)]
fn distance_to_segment(
    point_x: i32,
    point_y: i32,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
) -> f32 {
    let delta_x = (end_x - start_x) as f32;
    let delta_y = (end_y - start_y) as f32;
    let point_x = point_x as f32;
    let point_y = point_y as f32;
    let start_x = start_x as f32;
    let start_y = start_y as f32;
    let length_squared = delta_x * delta_x + delta_y * delta_y;
    let projection = (((point_x - start_x) * delta_x + (point_y - start_y) * delta_y)
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest_x = start_x + projection * delta_x;
    let nearest_y = start_y + projection * delta_y;
    ((point_x - nearest_x).powi(2) + (point_y - nearest_y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_has_expected_size_and_visible_pixels() {
        let rgba = icon_rgba();

        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn status_reflects_pause_state() {
        let config = Config::default();

        assert!(status_text(&config, false).contains(&config.hotkey));
        assert_eq!(status_text(&config, true), "Status: paused");
    }
}
