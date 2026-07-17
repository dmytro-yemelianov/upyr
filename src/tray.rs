use anyhow::{Context, Result};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

use crate::{autostart, config::Config};

pub const TRAY_FLIP_FRAME_COUNT: u8 = 17;
pub const TRAY_FLIP_FRAME_DELAY_MS: u64 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    ConvertPreviousWord,
    ConvertSelection,
    TogglePaused,
    OpenSettings,
    OpenAbout,
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
    about: MenuItem,
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
        let about = MenuItem::new("About Upyr…", true, None);
        let reload_config = MenuItem::new("Reload configuration", true, None);
        let autostart_status = autostart::status()?;
        let (autostart_label, autostart_checked) = autostart_presentation(autostart_status.state);
        let autostart = CheckMenuItem::new(autostart_label, true, autostart_checked, None);
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
            &about,
            &reload_config,
            &autostart,
            &third_separator,
            &quit,
        ])
        .context("failed to create tray menu")?;

        let icon = Icon::from_rgba(icon_rgba(0), ICON_SIZE, ICON_SIZE)
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
            about,
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
        } else if id == self.about.id() {
            Some(TrayAction::OpenAbout)
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
        let autostart_status = autostart::status()?;
        let (autostart_label, autostart_checked) = autostart_presentation(autostart_status.state);
        self.autostart.set_text(autostart_label);
        self.autostart.set_checked(autostart_checked);
        self.status.set_text(status_text(config, paused));
        self._icon
            .set_tooltip(Some(tooltip(config, paused)))
            .context("failed to update tray tooltip")
    }

    pub fn set_flip_frame(&self, frame: u8) -> Result<()> {
        let icon = Icon::from_rgba(icon_rgba(frame), ICON_SIZE, ICON_SIZE)
            .context("failed to create tray animation frame")?;
        self._icon
            .set_icon(Some(icon))
            .context("failed to update tray animation frame")
    }
}

fn autostart_presentation(state: autostart::AutostartState) -> (&'static str, bool) {
    match state {
        autostart::AutostartState::Disabled => ("Launch at login", false),
        autostart::AutostartState::Enabled => ("Launch at login", true),
        autostart::AutostartState::Stale => ("Repair launch at login…", false),
        autostart::AutostartState::Broken => ("Launch at login needs attention…", false),
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

const ICON_SIZE: u32 = 44;
const LOGICAL_SIZE: f32 = 22.0;
const LOGICAL_CENTER: f32 = LOGICAL_SIZE / 2.0;
const TRAY_FLIP_MOTION_FRAMES: u8 = 16;
const BOUNCE_HEIGHT: f32 = 2.0;
const LEFT_WING: [(f32, f32); 11] = [
    (2.2, 3.5),
    (6.3, 3.5),
    (7.0, 5.7),
    (7.7, 6.5),
    (9.1, 6.5),
    (9.1, 11.0),
    (5.7, 11.0),
    (5.0, 8.7),
    (3.1, 7.6),
    (3.1, 6.0),
    (2.7, 4.7),
];
const RIGHT_WING: [(f32, f32); 11] = [
    (19.8, 3.5),
    (15.7, 3.5),
    (15.0, 5.7),
    (14.3, 6.5),
    (12.9, 6.5),
    (12.9, 11.0),
    (16.3, 11.0),
    (17.0, 8.7),
    (18.9, 7.6),
    (18.9, 6.0),
    (19.3, 4.7),
];
const LEFT_EAR: [(f32, f32); 3] = [(5.4, 5.0), (6.5, 1.0), (7.6, 5.8)];
const RIGHT_EAR: [(f32, f32); 3] = [(16.6, 5.0), (15.5, 1.0), (14.4, 5.8)];
const LEFT_EYE: [(f32, f32); 4] = [(7.8, 13.1), (9.9, 13.8), (9.5, 15.0), (8.3, 14.6)];
const RIGHT_EYE: [(f32, f32); 4] = [(14.2, 13.1), (12.1, 13.8), (12.5, 15.0), (13.7, 14.6)];
const LEFT_FANG: [(f32, f32); 3] = [(8.6, 18.8), (10.0, 18.8), (9.3, 21.0)];
const RIGHT_FANG: [(f32, f32); 3] = [(13.4, 18.8), (12.0, 18.8), (12.7, 21.0)];

#[derive(Debug, Clone, Copy)]
struct AnimationTransform {
    angle: f32,
    center_y: f32,
    scale_x: f32,
    scale_y: f32,
}

fn animation_transform(frame: u8) -> AnimationTransform {
    if frame >= TRAY_FLIP_FRAME_COUNT {
        return AnimationTransform {
            angle: 0.0,
            center_y: LOGICAL_CENTER,
            scale_x: 1.0,
            scale_y: 1.0,
        };
    }

    let progress = f32::from(frame) / f32::from(TRAY_FLIP_MOTION_FRAMES);
    let angle = progress * std::f32::consts::TAU;
    let arc = 4.0 * progress * (1.0 - progress);
    let center_y = LOGICAL_CENTER - BOUNCE_HEIGHT * arc;
    let fit = 1.0 - (progress * std::f32::consts::PI).sin() * 0.08;
    let (scale_x, scale_y) = if frame == 1 || frame == TRAY_FLIP_MOTION_FRAMES - 1 {
        (fit * 0.92, fit * 1.08)
    } else if frame == TRAY_FLIP_MOTION_FRAMES {
        (1.14, 0.82)
    } else {
        (fit, fit)
    };

    AnimationTransform {
        angle,
        center_y,
        scale_x,
        scale_y,
    }
}

fn icon_rgba(frame: u8) -> Vec<u8> {
    let transform = animation_transform(frame);
    let (sin, cos) = transform.angle.sin_cos();
    let source_scale = ICON_SIZE as f32 / LOGICAL_SIZE;
    let mut rgba = vec![0; (ICON_SIZE * ICON_SIZE * 4) as usize];

    for y in 0..ICON_SIZE {
        for x in 0..ICON_SIZE {
            let destination_x = x as f32 / source_scale;
            let destination_y = y as f32 / source_scale;
            let centered_x = destination_x - LOGICAL_CENTER;
            let centered_y = destination_y - transform.center_y;
            let rotated_x = centered_x * cos + centered_y * sin;
            let rotated_y = -centered_x * sin + centered_y * cos;
            let source_x = LOGICAL_CENTER + rotated_x / transform.scale_x;
            let source_y = LOGICAL_CENTER + rotated_y / transform.scale_y;
            if mascot_pixel(source_x, source_y) {
                let offset = ((y * ICON_SIZE + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    rgba
}

fn mascot_pixel(x: f32, y: f32) -> bool {
    let body = (6.5..=9.3).contains(&x) && (6.0..=13.7).contains(&y)
        || (12.7..=15.5).contains(&x) && (6.0..=13.7).contains(&y)
        || inside_ellipse(x, y, 11.0, 13.2, 4.7, 5.2);
    let wings = point_in_polygon(x, y, &LEFT_WING) || point_in_polygon(x, y, &RIGHT_WING);
    let ears = point_in_polygon(x, y, &LEFT_EAR) || point_in_polygon(x, y, &RIGHT_EAR);
    let fangs = point_in_polygon(x, y, &LEFT_FANG) || point_in_polygon(x, y, &RIGHT_FANG);
    let inner_u =
        (9.2..=12.8).contains(&x) && y <= 7.6 || inside_ellipse(x, y, 11.0, 7.6, 1.8, 3.0);
    let eyes = point_in_polygon(x, y, &LEFT_EYE) || point_in_polygon(x, y, &RIGHT_EYE);
    (body || wings || ears || fangs) && !inner_u && !eyes
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

fn inside_ellipse(x: f32, y: f32, center_x: f32, center_y: f32, rx: f32, ry: f32) -> bool {
    ((x - center_x) / rx).powi(2) + ((y - center_y) / ry).powi(2) <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mascot_icon_has_expected_size_and_visible_pixels() {
        let rgba = icon_rgba(0);

        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 255));
        assert!(rgba.chunks_exact(4).any(|pixel| pixel[3] == 0));
    }

    #[test]
    fn mascot_icon_keeps_its_brand_details() {
        assert!(mascot_pixel(3.0, 4.0), "left wing should be visible");
        assert!(mascot_pixel(6.5, 1.5), "pointed left ear should be visible");
        assert!(mascot_pixel(7.0, 9.0), "left U arm should be visible");
        assert!(mascot_pixel(11.0, 18.0), "rounded U bowl should be visible");
        assert!(!mascot_pixel(11.0, 7.0), "U counter should stay open");
        assert!(!mascot_pixel(8.7, 14.0), "left eye should stay cut out");
        assert!(
            mascot_pixel(9.3, 20.0),
            "left fang should hang below the body"
        );
        assert!(!mascot_pixel(9.3, 18.6), "left fang should stay detached");
    }

    #[test]
    fn flip_frames_are_distinct_and_finish_at_idle() {
        let idle = icon_rgba(0);
        let frames = (1..=TRAY_FLIP_FRAME_COUNT)
            .map(icon_rgba)
            .collect::<Vec<_>>();

        assert!(
            frames[..frames.len() - 1]
                .iter()
                .all(|frame| frame != &idle)
        );
        assert_eq!(frames.last(), Some(&idle));
    }

    #[test]
    fn flip_follows_a_parabolic_bounce_and_squashes_on_impact() {
        let launch = animation_transform(1);
        let apex = animation_transform(TRAY_FLIP_MOTION_FRAMES / 2);
        let landing = animation_transform(TRAY_FLIP_MOTION_FRAMES);
        let settled = animation_transform(TRAY_FLIP_FRAME_COUNT);

        assert!(apex.center_y < launch.center_y);
        assert!((landing.center_y - LOGICAL_CENTER).abs() < f32::EPSILON);
        assert!(landing.scale_x > 1.0);
        assert!(landing.scale_y < 1.0);
        assert!((settled.center_y - LOGICAL_CENTER).abs() < f32::EPSILON);
        assert_eq!((settled.scale_x, settled.scale_y), (1.0, 1.0));
    }

    #[test]
    fn status_reflects_pause_state() {
        let config = Config::default();

        assert!(status_text(&config, false).contains(&config.hotkey));
        assert_eq!(status_text(&config, true), "Status: paused");
    }

    #[test]
    fn exceptional_autostart_states_are_actionable_in_the_menu() {
        assert_eq!(
            autostart_presentation(autostart::AutostartState::Stale),
            ("Repair launch at login…", false)
        );
        assert_eq!(
            autostart_presentation(autostart::AutostartState::Broken),
            ("Launch at login needs attention…", false)
        );
    }
}
