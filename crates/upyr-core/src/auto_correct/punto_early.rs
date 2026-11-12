//! Complete Punto Switcher Windows 4.4.2.334 trigger and exception tables.
//!
//! Windows checks a trigger prefix then both layouts against the full ps.dat
//! matcher. Upyr additionally accepts persistent prefix/substring rules when
//! no early trigger exists, and protects future E-exception continuations as
//! observed in the Mac early path. Parsing preserves literal spaces.
//! See docs/punto-early-rules.md for artifact identity and native evidence.

use super::punto_trie::{Trie, lowercase};

use super::{InputLayout, to_ukrainian};

impl Trie<()> {
    fn has_prefix_of(&self, text: &str) -> bool {
        let mut node = 0;
        for c in text.chars() {
            let Some(next) = self.next(node, c) else {
                return false;
            };
            node = next;
            if !self.values(node).is_empty() {
                return true;
            }
        }
        false
    }

    fn has_continuation_of(&self, text: &str) -> bool {
        let mut node = 0;
        for c in text.chars() {
            let Some(next) = self.next(node, c) else {
                return false;
            };
            node = next;
        }
        !self.values(node).is_empty() || self.has_children(node)
    }
}

struct EarlyRules {
    triggers: Trie<()>,
    exceptions: Trie<()>,
    case_sensitive_exceptions: Trie<()>,
}

static EARLY: EarlyRules = EarlyRules::builtin();
static FULL: super::punto_rules::RuleSet = super::punto_rules::RuleSet::builtin();

#[derive(Debug, Clone, Copy, Default)]
struct LanguageCursor {
    rules: super::punto_rules::RuleCursor,
    trigger: u16,
    exception: u16,
    case_exception: u16,
    triggered: bool,
}

impl LanguageCursor {
    fn advance(&mut self, c: char) {
        self.rules.advance(c, &FULL);
        fn advance(node: &mut u16, trie: &Trie<()>, c: char) {
            if *node != u16::MAX {
                *node = trie.next(*node, c).unwrap_or(u16::MAX);
            }
        }
        advance(
            &mut self.case_exception,
            &EARLY.case_sensitive_exceptions,
            c,
        );
        for lower in c.to_lowercase() {
            advance(&mut self.trigger, &EARLY.triggers, lower);
            advance(&mut self.exception, &EARLY.exceptions, lower);
            self.triggered |=
                self.trigger != u16::MAX && !EARLY.triggers.values(self.trigger).is_empty();
        }
    }

    fn protected(&self) -> bool {
        fn continuation(node: u16, trie: &Trie<()>) -> bool {
            node != u16::MAX && (!trie.values(node).is_empty() || trie.has_children(node))
        }
        continuation(self.exception, &EARLY.exceptions)
            || continuation(self.case_exception, &EARLY.case_sensitive_exceptions)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Snapshot {
    english: LanguageCursor,
    russian: LanguageCursor,
}

/// Bounded snapshots cover append, Backspace and edits without rescanning all
/// earlier substrings. These states depend only on physical keys, not policy.
#[derive(Debug, Clone)]
pub(super) struct EarlyState {
    physical: [u8; 64],
    snapshots: [Snapshot; 65],
    len: usize,
}

impl Default for EarlyState {
    fn default() -> Self {
        Self {
            physical: [0; 64],
            snapshots: [Snapshot::default(); 65],
            len: 0,
        }
    }
}

impl EarlyState {
    pub(super) fn reset(&mut self) {
        self.len = 0;
    }

    pub(super) fn matches(&mut self, english: &str, russian: &str, layout: InputLayout) -> bool {
        debug_assert!(english.is_ascii() && english.len() <= 64);
        let common = english
            .bytes()
            .zip(self.physical[..self.len].iter().copied())
            .take_while(|(a, b)| a == b)
            .count();
        for (index, (en, ru)) in english
            .bytes()
            .zip(russian.chars())
            .enumerate()
            .skip(common)
        {
            let mut next = self.snapshots[index];
            next.english.advance(en as char);
            next.russian.advance(ru);
            self.physical[index] = en;
            self.snapshots[index + 1] = next;
        }
        self.len = english.len();
        let state = self.snapshots[self.len];
        let (source, target) = match layout {
            InputLayout::English => (state.english, state.russian),
            InputLayout::Ukrainian => (state.russian, state.english),
        };
        if source.protected() {
            return false;
        }
        let Some(found) = source.rules.first_match(&FULL) else {
            return false;
        };
        if found.exception || (found.exact && !source.triggered) {
            return false;
        }
        found.priority
            || target
                .rules
                .first_match(&FULL)
                .is_none_or(|found| found.exception)
    }
}

impl EarlyRules {
    const fn builtin() -> Self {
        Self {
            triggers: super::punto_tables::TRIGGERS,
            exceptions: super::punto_tables::EXCEPTIONS,
            case_sensitive_exceptions: super::punto_tables::CASE_EXCEPTIONS,
        }
    }

    fn is_protected(&self, source: &str) -> bool {
        let lower = lowercase(source);
        self.exceptions.has_continuation_of(&lower)
            || self.case_sensitive_exceptions.has_continuation_of(source)
    }

    #[cfg(test)]
    fn matches(&self, source: &str) -> bool {
        self.triggers.has_prefix_of(&lowercase(source)) && !self.is_protected(source)
    }
}

pub(super) fn matches(physical: &str, layout: InputLayout) -> bool {
    let russian = canonical_source(physical, InputLayout::Ukrainian);
    matches_preconverted(physical, &russian, layout)
}

pub(super) fn matches_preconverted(english: &str, russian: &str, layout: InputLayout) -> bool {
    let (source, target) = match layout {
        InputLayout::English => (english, russian),
        InputLayout::Ukrainian => (russian, english),
    };
    if EARLY.is_protected(source) {
        return false;
    }
    // D rules in this table are exact one-letter rules; the caller's four-
    // letter minimum means the native after-dash special case cannot fire.
    if EARLY.triggers.has_prefix_of(&lowercase(source)) {
        FULL.should_switch(source, target, false)
    } else {
        FULL.should_switch_partial(source, target)
    }
}

// Punto's tables describe Russian key positions. Normalize the corresponding
// Ukrainian source keys into that alphabet solely for rule lookup; conversion
// and all native-word protection still use the actual EN/UK layout pair.
fn canonical_source(physical: &str, layout: InputLayout) -> String {
    if layout == InputLayout::English {
        return physical.to_owned();
    }
    to_ukrainian(physical, None)
        .chars()
        .map(|c| match c {
            'і' => 'ы',
            'І' => 'Ы',
            'ї' => 'ъ',
            'Ї' => 'Ъ',
            'є' => 'э',
            'Є' => 'Э',
            '\'' => 'ё',
            '₴' => 'Ё',
            'ґ' => '\\',
            'Ґ' => '|',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::punto_compile::{Builder, TRIGGERS, compile_early, exception_rules};
    use super::*;

    #[test]
    fn streaming_rules_match_reference_across_prefixes_backspace_and_case_edits() {
        let mut state = EarlyState::default();
        let words = [
            "dbghfdktyyz",
            "continue",
            "CONTINUE",
            "conteXt",
            "conn",
            "fooBar",
            "gggghgh",
            "rehn",
            "[ks,",
            "Abcd",
            "lkm ",
            "\\\"@#$^&",
            "vbh",
            "a",
            "",
        ];
        for word in words {
            for len in (0..=word.len()).chain((0..word.len()).rev()) {
                let physical = &word[..len];
                let russian = canonical_source(physical, InputLayout::Ukrainian);
                for layout in [InputLayout::English, InputLayout::Ukrainian] {
                    assert_eq!(
                        state.matches(physical, &russian, layout),
                        matches(physical, layout),
                        "{physical} {layout:?}"
                    );
                }
            }
        }
        // Branch edits and an exact rule expiring must not retain old outputs.
        for physical in [
            "cont", "conx", "cont", "dbgh", "DBGH", "dbghf", "db", "cont",
        ] {
            let russian = canonical_source(physical, InputLayout::Ukrainian);
            for layout in [InputLayout::English, InputLayout::Ukrainian] {
                assert_eq!(
                    state.matches(physical, &russian, layout),
                    matches(physical, layout)
                );
            }
        }
        state.reset();
        assert_eq!(
            state.matches(
                "cont",
                &canonical_source("cont", InputLayout::Ukrainian),
                InputLayout::Ukrainian
            ),
            matches("cont", InputLayout::Ukrainian)
        );
    }

    #[test]
    fn generated_early_tables_equal_all_compiled_nodes_edges_and_outputs() {
        let generated = EarlyRules::builtin();
        let (triggers, exceptions, case_exceptions) = compile_early();
        assert_eq!(generated.triggers, triggers);
        assert_eq!(generated.exceptions, exceptions);
        assert_eq!(generated.case_sensitive_exceptions, case_exceptions);
    }

    // Windows 0x476bd0 supplies the trigger prefix predicate; the additional
    // exception-continuation guard follows Mac 0x3f58. This is a static oracle,
    // not execution of either original binary.
    fn reference(source: &str, triggers: &[String], exceptions: &[(bool, String)]) -> bool {
        let lower = source.to_lowercase();
        triggers.iter().any(|s| lower.starts_with(s))
            && !exceptions.iter().any(|(case_sensitive, text)| {
                if *case_sensitive {
                    text.starts_with(source)
                } else {
                    text.starts_with(&lower)
                }
            })
    }

    #[test]
    fn complete_trigger_and_exception_tries_match_string_reference() {
        let rules = EarlyRules::builtin();
        let triggers = TRIGGERS.lines().map(str::to_lowercase).collect::<Vec<_>>();
        let exceptions = exception_rules()
            .map(|(tags, text)| {
                let sensitive = tags.contains('C');
                (
                    sensitive,
                    if sensitive {
                        text.to_owned()
                    } else {
                        text.to_lowercase()
                    },
                )
            })
            .collect::<Vec<_>>();
        let mut checked = 0;
        for text in TRIGGERS.lines().chain(exception_rules().map(|(_, s)| s)) {
            // Complete entries, longer words, case changes and every prefix
            // cover terminal nodes, shared branches and exception descendants.
            for variant in [text.to_owned(), format!("{text}xyz"), text.to_uppercase()] {
                assert_eq!(
                    rules.matches(&variant),
                    reference(&variant, &triggers, &exceptions),
                    "{variant}"
                );
                checked += 1;
            }
            for (i, c) in text.char_indices() {
                let prefix = &text[..i + c.len_utf8()];
                assert_eq!(
                    rules.matches(prefix),
                    reference(prefix, &triggers, &exceptions),
                    "{prefix}"
                );
                checked += 1;
            }
        }
        assert!(checked > 30_000);
        println!("{checked} complete-table differential checks");
    }

    #[test]
    fn shared_prefixes_are_anchored_and_exceptions_keep_case() {
        assert!(exception_rules().any(|(tags, text)| tags == "_E" && text == "lkm "));
        let mut trie = Builder::new();
        trie.insert("cont", ());
        trie.insert("conn", ());
        let trie = trie.finish();
        assert!(!trie.has_prefix_of("con"));
        assert!(trie.has_prefix_of("continue"));
        assert!(trie.has_prefix_of("connection"));
        assert!(!trie.has_prefix_of("xcontinue"));
        assert!(trie.has_continuation_of("con"));
        assert!(!trie.has_continuation_of("continued"));
        let rules = EarlyRules {
            triggers: trie,
            exceptions: Builder::new().finish(),
            case_sensitive_exceptions: {
                let mut trie = Builder::new();
                trie.insert("CONTOSO", ());
                trie.finish()
            },
        };
        assert!(rules.matches("cont"));
        assert!(!rules.matches("CONT"));
    }

    #[test]
    fn lookup_uses_russian_positions_without_changing_ukrainian_output() {
        assert_eq!(canonical_source("s]';", InputLayout::Ukrainian), "ыъэж");
        assert!(matches("cont", InputLayout::Ukrainian));
        assert!(matches("ghbd", InputLayout::English));
        assert!(!matches("cont", InputLayout::English));
    }
}
