//! Indexed reproduction of the Windows `ps.dat` rule matcher.
//!
//! The source engine scans its rules in A, E, then ordinary order.  This
//! module preserves that observable order while using tries only to avoid a
//! linear scan on each typed character.

use super::punto_model::Rule;
use super::punto_trie::{Trie, lowercase};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Match {
    pub(super) exception: bool,
    pub(super) priority: bool,
    pub(super) after_dash: bool,
    pub(super) exact: bool,
}

impl Trie<Rule> {
    fn first_match(&self, text: &str) -> Option<Rule> {
        let mut best = None;
        for (start, _) in text.char_indices() {
            let mut node = 0;
            for (end, character) in text[start..].char_indices() {
                let Some(next) = self.next(node, character) else {
                    break;
                };
                node = next;
                let end = start + end + character.len_utf8();
                for rule in self.values(node) {
                    if rule.anchored() && start != 0 {
                        continue;
                    }
                    if rule.exact() && end != text.len() {
                        continue;
                    }
                    if best.is_none_or(|current: Rule| rule.order_key() < current.order_key()) {
                        best = Some(*rule);
                    }
                }
            }
        }
        best
    }
}

/// Complete Windows rule set, split on the native C-tag case input choice.
pub(super) struct RuleSet {
    original: Trie<Rule>,
    lowercased: Trie<Rule>,
    original_failure: Cow<'static, [u16]>,
    original_anywhere: Cow<'static, [u32]>,
    lowercased_failure: Cow<'static, [u16]>,
    lowercased_anywhere: Cow<'static, [u32]>,
}

#[derive(Clone, Copy, Debug)]
struct MatchCursor {
    node: u16,
    anchored: u16,
    persistent: u32,
}

impl Default for MatchCursor {
    fn default() -> Self {
        Self {
            node: 0,
            anchored: 0,
            persistent: u32::MAX,
        }
    }
}

impl MatchCursor {
    fn advance(&mut self, c: char, trie: &Trie<Rule>, failure: &[u16], anywhere: &[u32]) {
        loop {
            if let Some(next) = trie.next(self.node, c) {
                self.node = next;
                break;
            }
            if self.node == 0 {
                break;
            }
            self.node = failure[self.node as usize];
        }
        self.persistent = self.persistent.min(anywhere[self.node as usize]);
        if self.anchored != u16::MAX {
            self.anchored = trie.next(self.anchored, c).unwrap_or(u16::MAX);
            if self.anchored != u16::MAX {
                for rule in trie
                    .values(self.anchored)
                    .iter()
                    .filter(|r| r.anchored() && !r.exact())
                {
                    self.persistent = self.persistent.min(rule.order_key());
                }
            }
        }
    }

    fn best(&self, trie: &Trie<Rule>) -> u32 {
        // Exact matches expire on the next key; do not store them in the
        // persistent minimum, even if they outrank all substring rules.
        if self.anchored != u16::MAX && self.anchored != 0 {
            trie.values(self.anchored)
                .iter()
                .filter(|r| r.exact())
                .map(|r| r.0)
                .min()
                .unwrap_or(u32::MAX)
                .min(self.persistent)
        } else {
            self.persistent
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RuleCursor {
    original: MatchCursor,
    lowercased: MatchCursor,
}

impl RuleCursor {
    // Used only with ASCII / canonical Cyrillic, whose lowercase is scalar-
    // local. General Unicode input below lowercases the whole string first.
    pub(super) fn advance(&mut self, c: char, rules: &RuleSet) {
        self.original.advance(
            c,
            &rules.original,
            &rules.original_failure,
            &rules.original_anywhere,
        );
        for lower in c.to_lowercase() {
            self.lowercased.advance(
                lower,
                &rules.lowercased,
                &rules.lowercased_failure,
                &rules.lowercased_anywhere,
            );
        }
    }

    pub(super) fn first_match(&self, rules: &RuleSet) -> Option<Match> {
        output(
            self.original
                .best(&rules.original)
                .min(self.lowercased.best(&rules.lowercased)),
        )
    }
}

fn output(value: u32) -> Option<Match> {
    (value != u32::MAX).then(|| {
        let rule = Rule(value);
        Match {
            exception: rule.exception(),
            priority: rule.priority(),
            after_dash: rule.after_dash(),
            exact: rule.exact(),
        }
    })
}

impl RuleSet {
    pub(super) const fn builtin() -> Self {
        Self {
            original: super::punto_tables::ORIGINAL,
            lowercased: super::punto_tables::LOWERCASED,
            original_failure: Cow::Borrowed(super::punto_tables::ORIGINAL_FAILURE),
            original_anywhere: Cow::Borrowed(super::punto_tables::ORIGINAL_ANYWHERE),
            lowercased_failure: Cow::Borrowed(super::punto_tables::LOWERCASED_FAILURE),
            lowercased_anywhere: Cow::Borrowed(super::punto_tables::LOWERCASED_ANYWHERE),
        }
    }

    #[cfg(test)]
    fn parse(table: &str) -> Result<Self, &'static str> {
        let (original, lowercased) = super::punto_compile::compile_rules(table)?;
        let (original_failure, original_anywhere) =
            super::punto_compile::compile_failure(&original);
        let (lowercased_failure, lowercased_anywhere) =
            super::punto_compile::compile_failure(&lowercased);
        Ok(Self {
            original,
            lowercased,
            original_failure: Cow::Owned(original_failure),
            original_anywhere: Cow::Owned(original_anywhere),
            lowercased_failure: Cow::Owned(lowercased_failure),
            lowercased_anywhere: Cow::Owned(lowercased_anywhere),
        })
    }

    #[allow(dead_code)] // Used by the standalone storage/lookup benchmark.
    pub(super) fn storage_bytes(&self) -> usize {
        self.original.storage_bytes()
            + self.lowercased.storage_bytes()
            + (self.original_failure.len() + self.lowercased_failure.len()) * 2
            + (self.original_anywhere.len() + self.lowercased_anywhere.len()) * 4
    }

    pub(super) fn first_match(&self, source: &str) -> Option<Match> {
        // Short standalone probes benchmark faster with the indexed trie.
        // Live prefixes use RuleCursor and compiled failure links instead.
        let lower = lowercase(source);
        let best = [
            self.original.first_match(source),
            self.lowercased.first_match(&lower),
        ]
        .into_iter()
        .flatten()
        .min_by_key(|rule| rule.order_key());
        output(best.map_or(u32::MAX, |rule| rule.0))
    }

    #[cfg(test)]
    fn first_match_streaming(&self, source: &str) -> Option<Match> {
        let lower = lowercase(source);
        let mut original = MatchCursor::default();
        let mut lowercased = MatchCursor::default();
        for c in source.chars() {
            original.advance(
                c,
                &self.original,
                &self.original_failure,
                &self.original_anywhere,
            );
        }
        for c in lower.chars() {
            lowercased.advance(
                c,
                &self.lowercased,
                &self.lowercased_failure,
                &self.lowercased_anywhere,
            );
        }
        output(
            original
                .best(&self.original)
                .min(lowercased.best(&self.lowercased)),
        )
    }

    /// The Windows whole-word gate after its two layout candidates have been
    /// mapped into the Russian physical-key alphabet.
    pub(super) fn should_switch(&self, source: &str, target: &str, after_dash: bool) -> bool {
        let Some(source_match) = self.first_match(source) else {
            return false;
        };
        self.switch_from_match(source_match, target, after_dash)
    }

    /// Upyr's early fallback: a substring or beginning rule remains matched
    /// when more letters arrive. An exact source rule needs a word boundary
    /// (or the separate imported early-trigger path).
    pub(super) fn should_switch_partial(&self, source: &str, target: &str) -> bool {
        let Some(source_match) = self.first_match(source).filter(|found| !found.exact) else {
            return false;
        };
        self.switch_from_match(source_match, target, false)
    }

    fn switch_from_match(&self, source_match: Match, target: &str, after_dash: bool) -> bool {
        if source_match.exception || (source_match.after_dash && after_dash) {
            return false;
        }
        if source_match.priority {
            return true;
        }
        self.first_match(target)
            .is_none_or(|target_match| target_match.exception)
    }
}

#[cfg(test)]
mod tests {
    use super::super::punto_compile::RULES;
    use super::*;

    #[test]
    fn generated_tables_equal_all_compiled_nodes_edges_and_outputs() {
        let generated = RuleSet::builtin();
        let compiled = RuleSet::parse(RULES).unwrap();
        assert_eq!(generated.original, compiled.original);
        assert_eq!(generated.lowercased, compiled.lowercased);
    }

    fn reference(table: &str, source: &str) -> Option<Match> {
        let entries = table
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let lower = source.to_lowercase();
        entries
            .iter()
            .enumerate()
            .filter_map(|(line_index, line)| {
                let (tags, key) = match line.split_once(' ') {
                    Some((tags, key)) if tags.starts_with('_') => (tags, key),
                    _ => ("", *line),
                };
                let input = if tags.contains('C') { source } else { &lower };
                let matched = if tags.contains('P') {
                    input == key
                } else if tags.contains('B') {
                    input.starts_with(key)
                } else {
                    input.contains(key)
                };
                matched.then_some((
                    (if tags.contains('A') {
                        0
                    } else if tags.contains('E') {
                        1
                    } else {
                        2
                    }) * entries.len()
                        + line_index,
                    Match {
                        exception: tags.contains('E'),
                        priority: tags.contains('A'),
                        after_dash: tags.contains('D'),
                        exact: tags.contains('P'),
                    },
                ))
            })
            .min_by_key(|(rank, _)| *rank)
            .map(|(_, found)| found)
    }

    #[test]
    fn failure_outputs_equal_nested_reference_for_complete_tables() {
        let rules = RuleSet::builtin();
        for trie in [&rules.original, &rules.lowercased] {
            let (failure, anywhere) = super::super::punto_compile::compile_failure(trie);
            let (stored_failure, stored_anywhere) = if std::ptr::eq(trie, &rules.original) {
                (&rules.original_failure, &rules.original_anywhere)
            } else {
                (&rules.lowercased_failure, &rules.lowercased_anywhere)
            };
            assert_eq!(stored_failure.as_ref(), failure);
            assert_eq!(stored_anywhere.as_ref(), anywhere);
        }
        for line in RULES.lines().skip(1).filter(|s| !s.is_empty()) {
            let key = line
                .split_once(' ')
                .filter(|(tags, _)| tags.starts_with('_'))
                .map_or(line, |(_, key)| key);
            for text in [key.to_owned(), format!("x{key}xyz"), key.to_uppercase()] {
                let lower = lowercase(&text);
                let expected = [
                    rules.original.first_match(&text),
                    rules.lowercased.first_match(&lower),
                ]
                .into_iter()
                .flatten()
                .min_by_key(|rule| rule.order_key());
                assert_eq!(
                    rules.first_match_streaming(&text),
                    output(expected.map_or(u32::MAX, |r| r.0)),
                    "{text}"
                );
            }
        }
    }

    #[test]
    fn preserves_modes_case_raw_key_and_group_precedence() {
        let table = "PSVersion=test\n_BC Ab\n_B ab\n_P exact\n_E needle \n_AD dash\n_A priority\n";
        let rules = RuleSet::parse(table).unwrap();
        assert_eq!(rules.first_match("Abcd"), reference(table, "Abcd"));
        assert_eq!(rules.first_match("abcd"), reference(table, "abcd"));
        assert_eq!(rules.first_match("xexact"), None);
        assert_eq!(rules.first_match("exact"), reference(table, "exact"));
        assert_eq!(rules.first_match("needle"), None);
        assert_eq!(rules.first_match("needle "), reference(table, "needle "));
        assert_eq!(rules.first_match("dash"), reference(table, "dash"));
        assert_eq!(rules.first_match("priority"), reference(table, "priority"));
    }

    #[test]
    fn whole_word_gate_keeps_exceptions_and_after_dash_behavior() {
        let rules = RuleSet::parse("PSVersion=test\n_B src\n_E target\n_A fast\n_PD z\n").unwrap();
        assert!(rules.should_switch("src", "target", false));
        assert!(!rules.should_switch("target", "src", false));
        assert!(rules.should_switch("fast", "src", false));
        assert!(rules.should_switch("z", "none", false));
        assert!(!rules.should_switch("z", "none", true));
    }

    #[test]
    fn early_fallback_requires_a_persistent_rule_and_preserves_precedence() {
        let rules = RuleSet::parse(
            "PSVersion=test\nbgh\n_B start\n_P exact\n_AP abgh\n_E exemptbgh\n_E target\n_A fast\nblocked\n",
        ).unwrap();
        assert!(rules.should_switch_partial("dbgh", "випр"));
        assert!(rules.should_switch_partial("dbghfdk", "виправл"));
        assert!(rules.should_switch_partial("starting", "target"));
        assert!(!rules.should_switch_partial("xstarting", "target"));
        assert!(rules.should_switch("exact", "target", false));
        assert!(!rules.should_switch_partial("exact", "target"));
        // Do not skip the earlier exact rule to reach a later substring rule.
        assert!(!rules.should_switch_partial("abgh", "target"));
        assert!(!rules.should_switch_partial("exemptbgh", "target"));
        assert!(!rules.should_switch_partial("dbgh", "blocked"));
        assert!(rules.should_switch_partial("fast", "blocked"));
    }

    #[test]
    fn differential_samples_complete_embedded_table() {
        let rules = RuleSet::builtin();
        let entries = RULES
            .lines()
            .skip(1)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        for (index, line) in entries.iter().enumerate().step_by(37) {
            let key = line
                .split_once(' ')
                .filter(|(tag, _)| tag.starts_with('_'))
                .map_or(*line, |(_, key)| key);
            for source in [key.to_owned(), format!("x{key}y"), key.to_uppercase()] {
                assert_eq!(
                    rules.first_match(&source),
                    reference(RULES, &source),
                    "{index}: {source:?}"
                );
            }
        }
    }
}
