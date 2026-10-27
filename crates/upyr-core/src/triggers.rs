//! Deterministic, high-precision correction rules consulted before the
//! statistical scorer.
//!
//! This mirrors the useful part of Punto Switcher's deterministic data files: a
//! small table of physical key sequences whose intended layout is unambiguous,
//! so a decision can be made without — or against — the n-gram model. Triggers
//! give short and domain-specific words a deterministic path that the coverage
//! model, tuned to abstain when uncertain, would otherwise miss. Patterns can
//! use a leading and/or trailing `*`; `*text*` is the clean-room equivalent of
//! Punto for Windows' live `A` tag, which reverse-engineering showed to mean
//! "match at any word position."

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::auto_correct::InputLayout;

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

/// A single deterministic rule: a physical key sequence pattern and the action
/// it forces. `physical` is stored normalized (trimmed and lowercased) so it
/// matches the tracker's physical word regardless of case.
///
/// Pattern syntax is deliberately tiny:
/// - `text` matches the whole physical word exactly;
/// - `text*` matches a prefix;
/// - `*text` matches a suffix;
/// - `*text*` matches anywhere in the physical word.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trigger {
    pub physical: String,
    pub action: TriggerAction,
    /// The active source layout this rule applies to. A trigger only fires when
    /// the current source layout matches, so a `Correct` rule for a Ukrainian
    /// word typed on the English layout never fires against the already-correct
    /// Ukrainian text those same physical keys produce on the Ukrainian layout.
    pub source_layout: InputLayout,
}

impl Trigger {
    /// Builds a trigger for the English source layout — the built-in set is
    /// Ukrainian words typed on a US-QWERTY layout.
    pub fn new(physical: impl AsRef<str>, action: TriggerAction) -> Self {
        Self::for_source_layout(physical, action, InputLayout::English)
    }

    /// Builds a trigger scoped to a specific source layout.
    pub fn for_source_layout(
        physical: impl AsRef<str>,
        action: TriggerAction,
        source_layout: InputLayout,
    ) -> Self {
        Self {
            physical: normalize_physical(physical.as_ref()),
            action,
            source_layout,
        }
    }

    pub(crate) fn matches(&self, physical: &str, source_layout: InputLayout) -> bool {
        self.source_layout == source_layout && pattern_matches(&self.physical, physical)
    }
}

/// Parses a trigger table. Each non-empty, non-comment line is
/// `physical-pattern <whitespace> action`, where action is `correct` or `keep`.
/// A trailing `# comment` is ignored. Malformed lines are skipped.
pub fn parse_triggers(text: &str) -> Vec<Trigger> {
    text.lines().filter_map(parse_line).collect()
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

/// The curated, embedded trigger table shipped with the engine. The asset is
/// parsed once and cached; callers get a cheap clone rather than re-parsing on
/// every keystroke.
pub fn builtin_triggers() -> Vec<Trigger> {
    static BUILTIN: OnceLock<Vec<Trigger>> = OnceLock::new();
    BUILTIN
        .get_or_init(|| parse_triggers(BUILTIN_TRIGGERS))
        .clone()
}

/// Normalizes a physical key sequence for matching: trims surrounding
/// whitespace and lowercases, while preserving every key character (including
/// the `[];'\,./` positions that are letters on the Ukrainian layout).
pub(crate) fn normalize_physical(word: &str) -> String {
    word.trim().to_lowercase()
}

fn pattern_matches(pattern: &str, physical: &str) -> bool {
    let leading_wildcard = pattern.starts_with('*');
    let trailing_wildcard = pattern.ends_with('*');
    let needle = pattern.trim_matches('*');
    if needle.is_empty() {
        return false;
    }

    match (leading_wildcard, trailing_wildcard) {
        (true, true) => physical.contains(needle),
        (true, false) => physical.ends_with(needle),
        (false, true) => physical.starts_with(needle),
        (false, false) => physical == needle,
    }
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
        assert_eq!(
            Trigger::new("GhBdSn", TriggerAction::Correct).physical,
            "ghbdsn"
        );
    }

    #[test]
    fn builtin_table_is_nonempty_and_wellformed() {
        let table = builtin_triggers();
        assert!(table.len() >= 10, "expected a seeded built-in table");
        assert!(table.iter().all(|trigger| !trigger.physical.is_empty()));
    }

    #[test]
    fn wildcard_patterns_match_by_word_position() {
        assert!(pattern_matches("abc", "abc"));
        assert!(!pattern_matches("abc", "xabc"));
        assert!(pattern_matches("abc*", "abcdef"));
        assert!(pattern_matches("*abc", "xxabc"));
        assert!(pattern_matches("*abc*", "xxabcxx"));
        assert!(!pattern_matches("*", "anything"));
    }

    #[test]
    fn wildcard_triggers_remain_source_layout_scoped() {
        let trigger =
            Trigger::for_source_layout("*abc*", TriggerAction::Correct, InputLayout::English);

        assert!(trigger.matches("xxabcxx", InputLayout::English));
        assert!(!trigger.matches("xxabcxx", InputLayout::Ukrainian));
    }
}
