//! Learning from the user's own fixes. A correction the user reverts twice
//! with the undo shortcut becomes an exception for the word as typed; a word
//! the user converts by hand twice becomes a `correct` rule. Counts last for
//! the session only.
//!
//! A Backspace revert teaches nothing: that Backspace was often meant for
//! the space after the word. A revert followed by a manual conversion of the
//! same word cancels out: together they say the revert was a slip, not that
//! the correction was wrong.

use std::collections::HashMap;

/// How many times the same fix must repeat before Upyr remembers it.
const REPEATS: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Lesson {
    /// Never correct this word as typed.
    Exception(String),
    /// Always correct this word as typed: the rule `word correct`.
    Rule(String),
}

/// How the user reverted a correction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Revert {
    /// The undo shortcut: a deliberate choice.
    Shortcut,
    /// Backspace right after the correction.
    Backspace,
}

#[derive(Debug, Default)]
pub(crate) struct Learner {
    reverted: HashMap<String, u8>,
    converted: HashMap<String, u8>,
}

impl Learner {
    /// A correction was reverted; `original` is the text the revert restored.
    pub fn reverted(&mut self, original: &str, how: Revert) -> Option<Lesson> {
        if how == Revert::Backspace {
            return None;
        }
        let word = learnable_word(original)?;
        repeated(&mut self.reverted, word).map(Lesson::Exception)
    }

    /// The user converted `source` by hand.
    pub fn converted(&mut self, source: &str) -> Option<Lesson> {
        let word = learnable_word(source)?;
        if self.reverted.remove(&word).is_some() {
            // Upyr already corrects this word; the revert was the mistake.
            return None;
        }
        repeated(&mut self.converted, word).map(Lesson::Rule)
    }
}

/// Counts one more occurrence and returns the word once it has repeated
/// enough, starting its count over.
fn repeated(counts: &mut HashMap<String, u8>, word: String) -> Option<String> {
    let count = counts.entry(word.clone()).or_default();
    *count += 1;
    if *count < REPEATS {
        return None;
    }
    counts.remove(&word);
    Some(word)
}

/// One word as typed, lower case: letters, apostrophes and the US keys that
/// are Ukrainian letters (`[];',.` and the backtick). Anything else, such
/// as spaces, digits or rule syntax, is not something to learn.
fn learnable_word(text: &str) -> Option<String> {
    let word = text.trim();
    let letters = word.chars().filter(|c| c.is_alphabetic()).count();
    let typed_word = word.chars().all(|c| {
        c.is_alphabetic() || matches!(c, '\'' | '’' | 'ʼ' | '[' | ']' | ';' | ',' | '.' | '`')
    });
    (typed_word && letters >= 1 && (2..=32).contains(&word.chars().count()))
        .then(|| word.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_reverts_of_the_same_word_teach_an_exception() {
        let mut learner = Learner::default();
        assert_eq!(learner.reverted("api ", Revert::Shortcut), None);
        assert_eq!(
            learner.reverted("API ", Revert::Shortcut),
            Some(Lesson::Exception("api".to_owned()))
        );
        // Learned once; the count starts over.
        assert_eq!(learner.reverted("api ", Revert::Shortcut), None);
    }

    #[test]
    fn reverts_with_backspace_teach_nothing() {
        // That Backspace may have been meant for the space after the word.
        let mut learner = Learner::default();
        for _ in 0..3 {
            assert_eq!(learner.reverted("ghbdsn ", Revert::Backspace), None);
        }
        assert_eq!(learner.reverted("ghbdsn ", Revert::Shortcut), None);
    }

    #[test]
    fn two_manual_conversions_teach_a_correct_rule() {
        let mut learner = Learner::default();
        assert_eq!(learner.converted("jr"), None);
        assert_eq!(
            learner.converted("jr "),
            Some(Lesson::Rule("jr".to_owned()))
        );
        assert_eq!(learner.converted("щл"), None);
        assert_eq!(learner.converted("щл"), Some(Lesson::Rule("щл".to_owned())));
        // Keys that are Ukrainian letters belong to the word (`;jdnbq` is `жовтий`).
        assert_eq!(learner.converted(";jdnbq"), None);
        assert_eq!(
            learner.converted(";jdnbq"),
            Some(Lesson::Rule(";jdnbq".to_owned()))
        );
    }

    #[test]
    fn a_manual_conversion_after_a_revert_cancels_it() {
        let mut learner = Learner::default();
        assert_eq!(learner.reverted("ghbdsn ", Revert::Shortcut), None);
        // The revert was a slip: the user converted the word back by hand.
        assert_eq!(learner.converted("ghbdsn"), None);
        assert_eq!(learner.reverted("ghbdsn ", Revert::Shortcut), None);
        assert_eq!(
            learner.reverted("ghbdsn ", Revert::Shortcut),
            Some(Lesson::Exception("ghbdsn".to_owned()))
        );
    }

    #[test]
    fn only_single_words_are_learned() {
        let mut learner = Learner::default();
        for text in ["", "a", "two words", "*x*", "#tag", "123", "hi!", "x2"] {
            assert_eq!(learner.reverted(text, Revert::Shortcut), None, "{text:?}");
            assert_eq!(learner.reverted(text, Revert::Shortcut), None, "{text:?}");
            assert_eq!(learner.converted(text), None, "{text:?}");
            assert_eq!(learner.converted(text), None, "{text:?}");
        }
    }
}
