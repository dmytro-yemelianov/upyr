//! GlobeSwitch's clean-tap layout switcher, hosted inside Upyr.
//!
//! This path never inspects characters or changes keyboard events. Correction,
//! clipboard access, sounds and the indicator remain owned by Upyr.

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{GlobeMonitor, diagnostics, restore_system_setting};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CycleOrder {
    #[default]
    List,
    Recent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GlobeSettings {
    /// macOS only. Off until the user opts into replacing the system handler.
    pub enabled: bool,
    pub tap_window_ms: u64,
    pub cycle_order: CycleOrder,
    pub skipped_layouts: Vec<String>,
    /// ISO region codes or custom text, keyed by macOS input-source ID.
    pub layout_flags: std::collections::BTreeMap<String, String>,
}

impl Default for GlobeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            tap_window_ms: 600,
            cycle_order: CycleOrder::List,
            skipped_layouts: Vec::new(),
            layout_flags: std::collections::BTreeMap::new(),
        }
    }
}

/// The next enabled source in OS order, or the most recently used alternative.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn next_layout<'a>(
    current: Option<&str>,
    enabled: &'a [String],
    recent: &[String],
    settings: &GlobeSettings,
) -> Option<&'a str> {
    let eligible =
        |id: &str| Some(id) != current && !settings.skipped_layouts.iter().any(|skip| skip == id);
    if settings.cycle_order == CycleOrder::Recent {
        for id in recent {
            if eligible(id) {
                if let Some(id) = enabled.iter().find(|candidate| *candidate == id) {
                    return Some(id);
                }
            }
        }
    }
    let start = current
        .and_then(|id| enabled.iter().position(|candidate| candidate == id))
        .map_or(0, |index| index + 1);
    (0..enabled.len())
        .map(|offset| &enabled[(start + offset) % enabled.len()])
        .find(|id| eligible(id))
        .map(String::as_str)
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug, Default)]
pub(crate) struct TapDetector {
    down_at: Option<u64>,
    chorded: bool,
}

#[cfg(any(target_os = "macos", test))]
impl TapDetector {
    pub fn down(&mut self, at: u64, other_modifiers: bool) {
        // Repeated flagsChanged events must not restart a hold or erase a chord.
        if self.down_at.is_none() {
            self.down_at = Some(at);
            self.chorded = other_modifiers;
        }
    }

    pub fn other(&mut self) {
        self.chorded = true;
    }

    pub fn up(&mut self, at: u64, window_ns: u64) -> bool {
        let start = self.down_at.take();
        let clean = !self.chorded;
        self.chorded = false;
        clean && start.is_some_and(|start| at >= start && at - start <= window_ns)
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taps_holds_chords_and_lost_events() {
        let mut detector = TapDetector::default();
        assert!(!detector.up(5, 600));
        detector.down(10, false);
        assert!(detector.up(610, 600));
        detector.down(10, false);
        assert!(!detector.up(611, 600));
        detector.down(10, false);
        detector.other();
        detector.down(11, false);
        assert!(!detector.up(20, 600));
        detector.down(10, true); // Shift was already held when fn went down.
        assert!(!detector.up(20, 600));
        detector.down(10, false);
        detector.reset();
        assert!(!detector.up(20, 600));
        detector.down(10, false);
        assert!(!detector.up(9, 600));
        detector.down(30, false);
        assert!(detector.up(40, 600));
    }

    #[test]
    fn cycles_all_layouts_including_from_a_skipped_or_unknown_source() {
        let ids = ["ABC", "Russian", "Ukrainian", "German"].map(str::to_owned);
        let settings = GlobeSettings {
            skipped_layouts: vec!["Russian".to_owned()],
            ..GlobeSettings::default()
        };
        for (current, expected) in [
            ("ABC", "Ukrainian"),
            ("Russian", "Ukrainian"),
            ("Ukrainian", "German"),
            ("German", "ABC"),
            ("Unknown", "ABC"),
        ] {
            assert_eq!(
                next_layout(Some(current), &ids, &[], &settings),
                Some(expected)
            );
        }
        assert_eq!(next_layout(None, &[], &[], &settings), None);
        assert_eq!(next_layout(Some("ABC"), &ids[..1], &[], &settings), None);
    }

    #[test]
    fn recent_order_ignores_disabled_and_skipped_sources() {
        let ids = ["ABC", "Russian", "Ukrainian", "German"].map(str::to_owned);
        let recent = ["German", "Missing", "Russian", "ABC"].map(str::to_owned);
        let settings = GlobeSettings {
            cycle_order: CycleOrder::Recent,
            skipped_layouts: vec!["Russian".to_owned()],
            ..GlobeSettings::default()
        };
        assert_eq!(
            next_layout(Some("German"), &ids, &recent, &settings),
            Some("ABC")
        );
        assert_eq!(
            next_layout(Some("ABC"), &ids, &[], &settings),
            Some("Ukrainian")
        );
        let settings = GlobeSettings {
            skipped_layouts: ids.to_vec(),
            ..settings
        };
        assert_eq!(next_layout(Some("ABC"), &ids, &recent, &settings), None);
    }
}
