//! Deterministic, high-precision correction rules consulted before the
//! statistical scorer.
//!
//! This mirrors Punto Switcher's `triggers.dat`: a small table of physical key
//! sequences whose intended layout is unambiguous, so a decision can be made
//! without — or against — the n-gram model. Triggers give short and
//! domain-specific words a deterministic path that the coverage model, tuned to
//! abstain when uncertain, would otherwise miss.

use serde::{Deserialize, Serialize};

const BUILTIN_TRIGGERS: &str = include_str!("../assets/triggers.txt");

/// What a matching trigger forces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TriggerAction {
    /// Force a layout correction (reinterpret the keys in the other layout).
    Correct,
    /// Never correct this sequence; end the current language segment.
    Keep,
}

/// A single deterministic rule: an exact physical key sequence and the action it
/// forces. `physical` is stored normalized (trimmed and lowercased) so it
/// matches the tracker's physical word regardless of case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub physical: String,
    pub action: TriggerAction,
}

impl Trigger {
    pub fn new(physical: impl AsRef<str>, action: TriggerAction) -> Self {
        Self {
            physical: normalize_physical(physical.as_ref()),
            action,
        }
    }
}

/// Parses a trigger table. Each non-empty, non-comment line is
/// `physical <whitespace> action`, where action is `correct` or `keep`. A
/// trailing `# comment` is ignored. Malformed lines are skipped.
pub fn parse_triggers(text: &str) -> Vec<Trigger> {
    text.lines()
        .filter_map(parse_line)
        .collect()
}

fn parse_line(line: &str) -> Option<Trigger> {
    let line = line.split('#').next().unwrap_or(line).trim();
    if line.is_empty() {
        return None;
    }
    let (physical, action) = line.split_once(char::is_whitespace)?;
    let action = match action.trim() {
        "correct" => TriggerAction::Correct,
        "keep" => TriggerAction::Keep,
        _ => return None,
    };
    let trigger = Trigger::new(physical, action);
    (!trigger.physical.is_empty()).then_some(trigger)
}

/// The curated, embedded trigger table shipped with the engine.
pub fn builtin_triggers() -> Vec<Trigger> {
    parse_triggers(BUILTIN_TRIGGERS)
}

/// Normalizes a physical key sequence for matching: trims surrounding
/// whitespace and lowercases, while preserving every key character (including
/// the `[];'\,./` positions that are letters on the Ukrainian layout).
pub(crate) fn normalize_physical(word: &str) -> String {
    word.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_actions_comments_and_blank_lines() {
        let table = parse_triggers(
            "# heading\n\nghbdsn   correct   # привіт\nnpn keep\nbroken-line\nbad   sideways\n",
        );
        assert_eq!(
            table,
            vec![
                Trigger::new("ghbdsn", TriggerAction::Correct),
                Trigger::new("npn", TriggerAction::Keep),
            ]
        );
    }

    #[test]
    fn normalizes_case_on_construction() {
        assert_eq!(Trigger::new("GhBdSn", TriggerAction::Correct).physical, "ghbdsn");
    }

    #[test]
    fn builtin_table_is_nonempty_and_wellformed() {
        let table = builtin_triggers();
        assert!(table.len() >= 10, "expected a seeded built-in table");
        assert!(table.iter().all(|trigger| !trigger.physical.is_empty()));
    }
}
