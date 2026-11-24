//! Per-application input-source preferences. A rule applies on keyboard focus,
//! never on ordinary typing or on an input-source change.
use serde::{Deserialize, Serialize};
use upyr_core::InputLayout;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationLayoutRule {
    /// macOS bundle identifier (preferred) or exact application display name.
    pub application: String,
    pub layout: InputLayout,
}

impl ApplicationLayoutRule {
    pub fn raycast() -> Self {
        Self {
            application: "com.raycast.macos".into(),
            layout: InputLayout::English,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn preferred_layout(rules: &[ApplicationLayoutRule], id: &str, name: &str) -> Option<InputLayout> {
    // A stable bundle-ID rule wins over a display-name rule.
    rules
        .iter()
        .find(|r| r.application.eq_ignore_ascii_case(id))
        .or_else(|| {
            rules
                .iter()
                .find(|r| r.application.eq_ignore_ascii_case(name))
        })
        .map(|r| r.layout)
}

/// Duplicate AX/window notifications must not undo a manual layout choice.
/// An unruled app still ends the previous app's focus session.
#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct FocusSession(Option<i32>);

#[cfg(any(target_os = "macos", test))]
impl FocusSession {
    fn changed(&mut self, pid: Option<i32>) -> bool {
        // A timeout or inaccessible field does not prove that focus left.
        // Preserving the session avoids undoing a manual layout on recovery.
        if pid.is_none() {
            return false;
        }
        let changed = self.0 != pid;
        self.0 = pid;
        changed
    }
}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub(crate) use macos::Monitor;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_reopening_applies_again_but_internal_focus_preserves_manual_layout() {
        let mut focus = FocusSession::default();
        assert!(focus.changed(Some(10))); // underlying, unruled app
        assert!(focus.changed(Some(20))); // nonactivating launcher
        assert!(!focus.changed(Some(20))); // field/window change or duplicate event
        assert!(focus.changed(Some(10))); // launcher dismissed, workspace unchanged
        assert!(focus.changed(Some(20))); // same launcher process opens again
        assert!(!focus.changed(None)); // timeout must not erase a manual override
        assert!(!focus.changed(None));
        assert!(!focus.changed(Some(20)));
        assert!(focus.changed(Some(30))); // another unruled app ends the session
        assert!(focus.changed(Some(20)));
    }

    #[test]
    fn rules_round_trip_and_survive_unrelated_concurrent_settings_saves() {
        use crate::config::Config;
        let base = Config::default();
        assert!(
            !toml::to_string(&base)
                .unwrap()
                .contains("application_layout_rules")
        );
        let mut edited = base.clone();
        edited
            .application_layout_rules
            .push(ApplicationLayoutRule::raycast());
        let mut current = base.clone();
        current.auto_correct = true;
        let merged = Config::merge_edits(&base, &edited, &current).unwrap();
        merged.validate().unwrap();
        assert!(merged.auto_correct);
        let decoded: Config = toml::from_str(&toml::to_string(&merged).unwrap()).unwrap();
        assert_eq!(decoded, merged);
        let unchanged_rules = Config::merge_edits(&base, &current, &edited).unwrap();
        assert_eq!(
            unchanged_rules.application_layout_rules,
            edited.application_layout_rules
        );
        let removed = Config::merge_edits(&edited, &base, &edited).unwrap();
        assert!(removed.application_layout_rules.is_empty());
    }

    #[test]
    fn rejects_ambiguous_or_invalid_application_rules() {
        use crate::config::Config;
        for application in ["", " Raycast", "Raycast\n", "\t", "Raycast\0"] {
            let config = Config {
                application_layout_rules: vec![ApplicationLayoutRule {
                    application: application.into(),
                    layout: InputLayout::English,
                }],
                ..Config::default()
            };
            assert!(config.validate().is_err(), "{application:?}");
        }
        let mut config = Config {
            application_layout_rules: vec![ApplicationLayoutRule::raycast()],
            ..Config::default()
        };
        config.application_layout_rules.push(ApplicationLayoutRule {
            application: "COM.RAYCAST.MACOS".into(),
            layout: InputLayout::Ukrainian,
        });
        assert!(config.validate().is_err());
    }

    #[test]
    fn matches_only_the_requested_app_and_prefers_its_stable_id() {
        let rules = vec![
            ApplicationLayoutRule {
                application: "Raycast".into(),
                layout: InputLayout::Ukrainian,
            },
            ApplicationLayoutRule::raycast(),
        ];
        assert_eq!(
            preferred_layout(&rules, "com.raycast.macos", "Raycast"),
            Some(InputLayout::English)
        );
        assert_eq!(
            preferred_layout(&rules, "COM.RAYCAST.MACOS", "Renamed"),
            Some(InputLayout::English)
        );
        assert_eq!(
            preferred_layout(&rules, "other", "Raycast"),
            Some(InputLayout::Ukrainian)
        );
        assert_eq!(
            preferred_layout(&rules, "com.raycast.macos.helper", "Raycast Helper"),
            None
        );
        assert_eq!(preferred_layout(&[], "com.raycast.macos", "Raycast"), None);
    }
}
