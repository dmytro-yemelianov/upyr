use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

use crate::{autostart, config::Config};

pub const TRAY_FLIP_FRAME_COUNT: u8 = 6;
pub const TRAY_FLIP_FRAME_DELAY_MS: u64 = 40;

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

        let icon = Icon::from_rgba(icon_rgba(0.0), ICON_SIZE, ICON_SIZE)
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

    pub fn set_flip_frame(&self, frame: u8) -> Result<()> {
        let rotation_degrees = if frame >= TRAY_FLIP_FRAME_COUNT {
            0.0
        } else {
            f32::from(frame) * 360.0 / f32::from(TRAY_FLIP_FRAME_COUNT)
        };
        let icon = Icon::from_rgba(icon_rgba(rotation_degrees), ICON_SIZE, ICON_SIZE)
            .context("failed to create tray animation frame")?;
        self._icon
            .set_icon(Some(icon))
            .context("failed to update tray animation frame")
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
const U_SEGMENTS: [(f32, f32, f32, f32); 6] = [
    (6.0, 4.0, 6.0, 12.0),
    (6.0, 12.0, 8.0, 16.0),
    (8.0, 16.0, 11.0, 18.0),
    (11.0, 18.0, 14.0, 16.0),
    (14.0, 16.0, 16.0, 12.0),
    (16.0, 12.0, 16.0, 4.0),
];
const LEFT_WING: [(f32, f32); 8] = [
    (5.2, 7.0),
    (3.2, 5.7),
    (1.0, 5.8),
    (2.4, 8.0),
    (0.9, 10.4),
    (3.6, 9.7),
    (4.0, 12.5),
    (6.0, 10.8),
];
const RIGHT_WING: [(f32, f32); 8] = [
    (16.8, 7.0),
    (18.8, 5.7),
    (21.0, 5.8),
    (19.6, 8.0),
    (21.1, 10.4),
    (18.4, 9.7),
    (18.0, 12.5),
    (16.0, 10.8),
];
const LEFT_EAR: [(f32, f32); 3] = [(4.5, 5.0), (6.0, 1.7), (7.6, 5.0)];
const RIGHT_EAR: [(f32, f32); 3] = [(17.5, 5.0), (16.0, 1.7), (14.4, 5.0)];
const LEFT_FANG: [(f32, f32); 3] = [(6.8, 5.2), (8.5, 5.8), (7.2, 7.6)];
const RIGHT_FANG: [(f32, f32); 3] = [(15.2, 5.2), (13.5, 5.8), (14.8, 7.6)];

fn icon_rgba(rotation_degrees: f32) -> Vec<u8> {
    let mut rgba = vec![0; (ICON_SIZE * ICON_SIZE * 4) as usize];
    let radians = rotation_degrees.to_radians();
    let (sin, cos) = radians.sin_cos();
    let center = ICON_SIZE as f32 / 2.0;
    for y in 0..ICON_SIZE as i32 {
        for x in 0..ICON_SIZE as i32 {
            let centered_x = x as f32 - center;
            let centered_y = y as f32 - center;
            let source_x = center + centered_x * cos + centered_y * sin;
            let source_y = center - centered_x * sin + centered_y * cos;
            if mascot_pixel(source_x, source_y) {
                let offset = ((y as u32 * ICON_SIZE + x as u32) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    rgba
}

fn mascot_pixel(x: f32, y: f32) -> bool {
    let u_stroke = U_SEGMENTS
        .iter()
        .any(|&(x1, y1, x2, y2)| distance_to_segment(x, y, x1, y1, x2, y2) <= 1.35);
    let details = point_in_polygon(x, y, &LEFT_WING)
        || point_in_polygon(x, y, &RIGHT_WING)
        || point_in_polygon(x, y, &LEFT_EAR)
        || point_in_polygon(x, y, &RIGHT_EAR)
        || point_in_polygon(x, y, &LEFT_FANG)
        || point_in_polygon(x, y, &RIGHT_FANG);
    u_stroke || details
}

#[allow(clippy::too_many_arguments)]
fn distance_to_segment(
    point_x: f32,
    point_y: f32,
    start_x: f32,
    start_y: f32,
    end_x: f32,
    end_y: f32,
) -> f32 {
    let delta_x = end_x - start_x;
    let delta_y = end_y - start_y;
    let length_squared = delta_x * delta_x + delta_y * delta_y;
    let projection = (((point_x - start_x) * delta_x + (point_y - start_y) * delta_y)
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest_x = start_x + projection * delta_x;
    let nearest_y = start_y + projection * delta_y;
    ((point_x - nearest_x).powi(2) + (point_y - nearest_y).powi(2)).sqrt()
}

fn point_in_polygon<const N: usize>(x: f32, y: f32, points: &[(f32, f32); N]) -> bool {
    let mut inside = false;
    let mut previous = points[N - 1];
    for &current in points {
        let (x1, y1) = previous;
        let (x2, y2) = current;
        if (y1 > y) != (y2 > y) {
            let crossing_x = (x2 - x1) * (y - y1) / (y2 - y1) + x1;
            if x < crossing_x {
                inside = !inside;
            }
        }
        previous = current;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mascot_icon_has_expected_size_and_visible_pixels() {
        let rgba = icon_rgba(0.0);

        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn mascot_icon_keeps_its_brand_details() {
        assert!(mascot_pixel(6.0, 2.5), "left ear should be visible");
        assert!(mascot_pixel(2.5, 8.0), "left wing should be visible");
        assert!(mascot_pixel(7.5, 6.2), "left fang should be visible");
        assert!(mascot_pixel(11.0, 18.0), "U bowl should be visible");
        assert!(!mascot_pixel(11.0, 8.0), "U counter should stay open");
    }

    #[test]
    fn flip_frames_are_distinct_and_finish_at_idle() {
        let idle = icon_rgba(0.0);
        let frames = (1..=TRAY_FLIP_FRAME_COUNT)
            .map(|frame| {
                let rotation = if frame == TRAY_FLIP_FRAME_COUNT {
                    0.0
                } else {
                    f32::from(frame) * 360.0 / f32::from(TRAY_FLIP_FRAME_COUNT)
                };
                icon_rgba(rotation)
            })
            .collect::<Vec<_>>();

        assert!(
            frames[..frames.len() - 1]
                .iter()
                .all(|frame| frame != &idle)
        );
        assert_eq!(frames.last(), Some(&idle));
    }

    #[test]
    fn status_reflects_pause_state() {
        let config = Config::default();

        assert!(status_text(&config, false).contains(&config.hotkey));
        assert_eq!(status_text(&config, true), "Status: paused");
    }
}
