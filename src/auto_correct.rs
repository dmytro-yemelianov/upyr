use device_query::Keycode;

use crate::{
    config::{AutoCorrectSensitivity, Config},
    system_layout::{self, SystemLayout},
};

pub use upyr_core::{AutoCorrection, AutoDecision, WordSample};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoKeyEvent {
    pub key: Keycode,
    pub shifted: bool,
}

#[derive(Default)]
pub struct AutoWordTracker {
    inner: upyr_core::AutoWordTracker,
}

impl AutoWordTracker {
    pub fn can_begin(key: Keycode) -> bool {
        physical_key(key).is_some_and(upyr_core::AutoWordTracker::can_begin)
    }

    pub fn needs_layout_check(&self) -> bool {
        self.inner.needs_layout_check()
    }

    pub fn set_source_layout(&mut self, layout: Option<SystemLayout>) {
        self.inner.set_source_layout(layout);
    }

    pub fn clear(&mut self) {
        self.inner.clear();
    }

    pub fn observe(&mut self, event: AutoKeyEvent) -> Option<WordSample> {
        let key = physical_key(event.key).unwrap_or(upyr_core::PhysicalKey::Unsupported);
        self.inner.observe(upyr_core::PhysicalKeyEvent {
            key,
            shifted: event.shifted,
        })
    }
}

pub fn evaluate(sample: &WordSample, config: &Config) -> AutoDecision {
    let policy = correction_policy(config);
    let mapping = system_layout::installed_mapping().ok().flatten();
    upyr_core::evaluate(sample, &policy, mapping.as_deref())
}

fn correction_policy(config: &Config) -> upyr_core::AutoCorrectPolicy {
    upyr_core::AutoCorrectPolicy {
        sensitivity: match config.auto_correct_sensitivity {
            AutoCorrectSensitivity::Conservative => upyr_core::Sensitivity::Conservative,
            AutoCorrectSensitivity::Balanced => upyr_core::Sensitivity::Balanced,
            AutoCorrectSensitivity::Aggressive => upyr_core::Sensitivity::Aggressive,
        },
        min_word_length: config.auto_correct_min_word_length,
        exceptions: config.auto_correct_exceptions.clone(),
    }
}

fn physical_key(key: Keycode) -> Option<upyr_core::PhysicalKey> {
    use upyr_core::PhysicalKey;

    Some(match key {
        Keycode::A => PhysicalKey::KeyA,
        Keycode::B => PhysicalKey::KeyB,
        Keycode::C => PhysicalKey::KeyC,
        Keycode::D => PhysicalKey::KeyD,
        Keycode::E => PhysicalKey::KeyE,
        Keycode::F => PhysicalKey::KeyF,
        Keycode::G => PhysicalKey::KeyG,
        Keycode::H => PhysicalKey::KeyH,
        Keycode::I => PhysicalKey::KeyI,
        Keycode::J => PhysicalKey::KeyJ,
        Keycode::K => PhysicalKey::KeyK,
        Keycode::L => PhysicalKey::KeyL,
        Keycode::M => PhysicalKey::KeyM,
        Keycode::N => PhysicalKey::KeyN,
        Keycode::O => PhysicalKey::KeyO,
        Keycode::P => PhysicalKey::KeyP,
        Keycode::Q => PhysicalKey::KeyQ,
        Keycode::R => PhysicalKey::KeyR,
        Keycode::S => PhysicalKey::KeyS,
        Keycode::T => PhysicalKey::KeyT,
        Keycode::U => PhysicalKey::KeyU,
        Keycode::V => PhysicalKey::KeyV,
        Keycode::W => PhysicalKey::KeyW,
        Keycode::X => PhysicalKey::KeyX,
        Keycode::Y => PhysicalKey::KeyY,
        Keycode::Z => PhysicalKey::KeyZ,
        Keycode::Grave => PhysicalKey::Backquote,
        Keycode::LeftBracket => PhysicalKey::BracketLeft,
        Keycode::RightBracket => PhysicalKey::BracketRight,
        Keycode::BackSlash => PhysicalKey::Backslash,
        Keycode::Semicolon => PhysicalKey::Semicolon,
        Keycode::Apostrophe => PhysicalKey::Quote,
        Keycode::Comma => PhysicalKey::Comma,
        Keycode::Dot => PhysicalKey::Period,
        Keycode::Slash => PhysicalKey::Slash,
        Keycode::Space => PhysicalKey::Space,
        Keycode::Backspace => PhysicalKey::Backspace,
        Keycode::LShift | Keycode::RShift => PhysicalKey::Shift,
        Keycode::CapsLock => PhysicalKey::CapsLock,
        Keycode::LControl | Keycode::RControl => PhysicalKey::Control,
        Keycode::LAlt | Keycode::RAlt | Keycode::LOption | Keycode::ROption => PhysicalKey::Alt,
        Keycode::Command | Keycode::RCommand | Keycode::LMeta | Keycode::RMeta => PhysicalKey::Meta,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_keycodes_map_to_portable_physical_positions() {
        assert_eq!(physical_key(Keycode::A), Some(upyr_core::PhysicalKey::KeyA));
        assert_eq!(
            physical_key(Keycode::LeftBracket),
            Some(upyr_core::PhysicalKey::BracketLeft)
        );
        assert_eq!(
            physical_key(Keycode::Apostrophe),
            Some(upyr_core::PhysicalKey::Quote)
        );
        assert_eq!(physical_key(Keycode::Left), None);
    }

    #[test]
    fn desktop_config_maps_to_portable_correction_policy() {
        for (source, expected) in [
            (
                AutoCorrectSensitivity::Conservative,
                upyr_core::Sensitivity::Conservative,
            ),
            (
                AutoCorrectSensitivity::Balanced,
                upyr_core::Sensitivity::Balanced,
            ),
            (
                AutoCorrectSensitivity::Aggressive,
                upyr_core::Sensitivity::Aggressive,
            ),
        ] {
            let config = Config {
                auto_correct_sensitivity: source,
                auto_correct_min_word_length: 7,
                auto_correct_exceptions: vec!["ServiceNow".to_owned()],
                ..Config::default()
            };
            let policy = correction_policy(&config);

            assert_eq!(policy.sensitivity, expected);
            assert_eq!(policy.min_word_length, 7);
            assert_eq!(policy.exceptions, ["ServiceNow"]);
        }
    }
}
