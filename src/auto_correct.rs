use device_query::Keycode;
use std::{borrow::Cow, cell::RefCell, sync::Arc};

use crate::{
    config::{AutoCorrectSensitivity, Config},
    system_layout::{self, SystemLayout},
};

pub use upyr_core::{AutoCorrection, AutoDecision};

const MAX_TRACKED_CONTEXT_CHARACTERS: usize = 256;

#[cfg(any(target_os = "macos", test))]
pub(crate) fn catch_up_selected_prefix(
    expected: &str,
    replacement: &str,
    selected: &str,
    through_selection: &str,
    config: &Config,
) -> Option<AutoCorrection> {
    if selected.chars().count() != expected.chars().count()
        || !selected.chars().all(char::is_alphabetic)
    {
        return None;
    }
    let correction = catch_up_prefix(expected, replacement, through_selection, config)?;
    correction
        .expected_source
        .ends_with(selected)
        .then_some(correction)
}

/// Re-evaluate a longer prefix already delivered before the input hold began.
/// Every extra letter must be in the same word; all normal rules and user
/// exceptions still apply to the observed text.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn catch_up_prefix(
    expected: &str,
    replacement: &str,
    before_caret: &str,
    config: &Config,
) -> Option<AutoCorrection> {
    if !expected.chars().all(char::is_alphabetic) || expected.chars().count() < 4 {
        return None;
    }
    let word = before_caret.rsplit(|c: char| !c.is_alphabetic()).next()?;
    if word.len() <= expected.len() || !word.starts_with(expected) || word.chars().count() > 64 {
        return None;
    }
    let mapping = system_layout::shared_installed_mapping().ok().flatten();
    let (physical, source) = match config.direction {
        upyr_core::Direction::EnglishToUkrainian => (word.to_owned(), SystemLayout::English),
        upyr_core::Direction::UkrainianToEnglish => (
            match &mapping {
                Some(mapping) => {
                    upyr_core::convert_with_mapping(word, config.direction, mapping).text
                }
                None => upyr_core::convert(word, config.direction).text,
            },
            SystemLayout::Ukrainian,
        ),
        _ => return None,
    };
    let sample = upyr_core::WordSample::new(&physical, &physical, source);
    match upyr_core::evaluate_partial(&sample, &correction_policy(config), mapping.as_deref()) {
        AutoDecision::Correct(correction)
            if correction.expected_source == word
                && correction.replacement.starts_with(replacement) =>
        {
            Some(correction)
        }
        _ => None,
    }
}

/// Initialize immutable layout/model caches before subscribing to live keys.
/// A cold first lookup must not leave a four-key correction several keys late.
pub(crate) fn prepare() {
    let mapping = system_layout::shared_installed_mapping().ok().flatten();
    let policy = upyr_core::AutoCorrectPolicy::default();
    for (physical, layout) in [
        ("dbgh", SystemLayout::English),
        ("cont", SystemLayout::Ukrainian),
    ] {
        let sample = upyr_core::WordSample::new(physical, physical, layout);
        let _ = upyr_core::evaluate_partial(&sample, &policy, mapping.as_deref());
    }
}

#[cfg(test)]
#[path = "auto_correct_edge_tests.rs"]
mod edge_tests;

#[cfg(test)]
#[path = "auto_correct_context_tests.rs"]
mod context_tests;
#[cfg(test)]
#[path = "auto_correct_visible_tests.rs"]
mod visible_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoKeyEvent {
    /// Identity of the physical Quartz key, retained through both replay queues.
    pub timestamp: Option<u64>,
    pub key: Keycode,
    pub shifted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutRun {
    layout: SystemLayout,
    physical: String,
}

struct PendingContext {
    policy: Arc<upyr_core::AutoCorrectPolicy>,
    mapping: Option<system_layout::SharedMapping>,
    pending: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSample {
    pub(crate) inner: upyr_core::WordSample,
    visible_runs: Vec<LayoutRun>,
}

#[derive(Default)]
pub struct AutoWordTracker {
    partial_state: upyr_core::PartialWordState,
    pending_context: Option<PendingContext>,
    physical_context: String,
    visible_runs: Vec<LayoutRun>,
    current_word_start: Option<usize>,
    current_word_layout: Option<SystemLayout>,
    current_word_mixed: bool,
    active_layout: Option<SystemLayout>,
}

impl AutoWordTracker {
    pub fn can_begin(key: Keycode) -> bool {
        physical_key(key).is_some_and(upyr_core::AutoWordTracker::can_begin)
    }

    #[cfg(test)]
    pub fn needs_layout_check(&self) -> bool {
        self.current_word_start.is_none()
    }

    pub fn set_source_layout(&mut self, layout: Option<SystemLayout>) {
        if self.active_layout != layout {
            self.pending_context = None;
        }
        if self.active_layout != layout && self.current_word_start.is_none() {
            // Keep only an unresolved short tail for one-word look-ahead.
            // The contextual evaluator uses actual visible scripts; the old
            // whole-run evaluator must not reinterpret a mixed-language run.
            let pending = layout.is_some()
                && upyr_core::has_pending_context(
                    &render_visible_context(&self.visible_runs, None),
                    &upyr_core::AutoCorrectPolicy::default(),
                    None,
                );
            if !pending {
                self.clear_context();
            }
        }
        self.active_layout = layout;
    }

    #[cfg(any(target_os = "macos", test))]
    pub fn manual_layout_switch(&mut self, layout: Option<SystemLayout>) {
        if self.current_word_start.is_some() {
            self.clear();
        }
        self.set_source_layout(layout);
    }

    pub fn clear(&mut self) {
        self.clear_context();
        self.active_layout = None;
    }

    /// The current word as if a line break ended it now, without changing the
    /// tracker. No boundary is appended: Enter is held back until the
    /// correction is in place, so nothing follows the word on screen yet.
    pub fn line_break_sample(&self) -> Option<WordSample> {
        let word_start = self.current_word_start?;
        let source_layout = self.current_word_layout?;
        let final_layout = self.active_layout?;
        if self.current_word_mixed && source_layout == final_layout {
            return None;
        }
        let physical_word = self.physical_context[word_start..].to_owned();
        if physical_word.is_empty() {
            return None;
        }
        Some(WordSample {
            inner: upyr_core::WordSample::new(
                physical_word,
                self.physical_context.clone(),
                source_layout,
            ),
            visible_runs: self.visible_runs.clone(),
        })
    }

    /// An unfinished word is eligible only while all of its keys were typed
    /// in one layout. A manual switch must never be second-guessed mid-word.
    #[allow(dead_code)] // Reference replay API; production borrows the current word.
    pub fn partial_word_sample(&self) -> Option<WordSample> {
        if self.current_word_mixed {
            return None;
        }
        self.line_break_sample()
    }

    pub fn observe(&mut self, event: AutoKeyEvent) -> Option<WordSample> {
        let Some(key) = physical_key(event.key) else {
            self.clear();
            return None;
        };
        match key {
            upyr_core::PhysicalKey::Space => {
                if self.current_word_start.is_none() {
                    // A repeated space is not represented in the tracked
                    // context; keeping the older words would select text that
                    // no longer matches the screen.
                    self.clear_context();
                    return None;
                }
                return self.finish_word();
            }
            upyr_core::PhysicalKey::Backspace => {
                self.backspace();
                return None;
            }
            upyr_core::PhysicalKey::Shift => return None,
            upyr_core::PhysicalKey::CapsLock
            | upyr_core::PhysicalKey::Control
            | upyr_core::PhysicalKey::Alt
            | upyr_core::PhysicalKey::Meta
            | upyr_core::PhysicalKey::Unsupported => {
                self.clear();
                return None;
            }
            _ => {}
        }

        let layout = self.active_layout?;
        let physical_event = upyr_core::PhysicalKeyEvent {
            key,
            shifted: event.shifted,
        };
        let Some(character) = upyr_core::physical_character(physical_event) else {
            self.clear();
            return None;
        };

        if self.physical_context.chars().count() >= MAX_TRACKED_CONTEXT_CHARACTERS {
            self.clear();
            return None;
        }
        if self.current_word_start.is_none() {
            self.current_word_start = Some(self.physical_context.len());
            self.current_word_layout = Some(layout);
            self.current_word_mixed = false;
        } else if self.current_word_layout != Some(layout) {
            self.current_word_mixed = true;
        }
        self.physical_context.push(character);
        self.append_visible(layout, character);
        None
    }

    fn finish_word(&mut self) -> Option<WordSample> {
        self.pending_context = None;
        self.partial_state.reset();
        let word_start = self.current_word_start.take()?;
        let source_layout = self.current_word_layout.take()?;
        let final_layout = self.active_layout?;
        if self.current_word_mixed && source_layout == final_layout {
            // More than one layout transition inside one word is too
            // ambiguous to rewrite safely.
            self.clear_context();
            return None;
        }

        let physical_word = self.physical_context[word_start..].to_owned();
        if physical_word.is_empty() {
            return None;
        }
        self.physical_context.push(' ');
        self.append_visible(final_layout, ' ');
        self.current_word_mixed = false;

        Some(WordSample {
            inner: upyr_core::WordSample::new(
                physical_word,
                self.physical_context.clone(),
                source_layout,
            ),
            visible_runs: self.visible_runs.clone(),
        })
    }

    fn backspace(&mut self) {
        self.pending_context = None;
        self.physical_context.pop();
        if let Some(run) = self.visible_runs.last_mut() {
            run.physical.pop();
            if run.physical.is_empty() {
                self.visible_runs.pop();
            }
        }
        self.recompute_current_word();
    }

    fn append_visible(&mut self, layout: SystemLayout, character: char) {
        if let Some(run) = self.visible_runs.last_mut() {
            if run.layout == layout {
                run.physical.push(character);
                return;
            }
        }
        self.visible_runs.push(LayoutRun {
            layout,
            physical: character.to_string(),
        });
    }

    fn recompute_current_word(&mut self) {
        if self.physical_context.is_empty() || self.physical_context.ends_with(' ') {
            self.current_word_start = None;
            self.current_word_layout = None;
            self.current_word_mixed = false;
            return;
        }

        let word_start = self
            .physical_context
            .rfind(' ')
            .map_or(0, |index| index + 1);
        self.current_word_start = Some(word_start);

        let mut offset = 0;
        let mut first_layout = None;
        let mut mixed = false;
        for run in &self.visible_runs {
            let end = offset + run.physical.len();
            if end > word_start {
                match first_layout {
                    None => first_layout = Some(run.layout),
                    Some(layout) if layout != run.layout => mixed = true,
                    Some(_) => {}
                }
            }
            offset = end;
        }
        self.current_word_layout = first_layout;
        self.current_word_mixed = mixed;
    }

    fn clear_context(&mut self) {
        self.pending_context = None;
        self.partial_state.reset();
        self.physical_context.clear();
        self.visible_runs.clear();
        self.current_word_start = None;
        self.current_word_layout = None;
        self.current_word_mixed = false;
    }
}

/// macOS evaluates through [`evaluate_with_caps_lock`], which also handles
/// terminals and an accidental Caps Lock.
#[cfg(not(target_os = "macos"))]
pub fn evaluate(sample: &WordSample, config: &Config) -> AutoDecision {
    let mapping = system_layout::shared_installed_mapping().ok().flatten();
    evaluate_with_mapping(sample, config, mapping.as_deref())
}

impl AutoWordTracker {
    pub fn evaluate_current_partial(&mut self, config: &Config) -> AutoDecision {
        if !config.switch_layout
            || self.current_word_mixed
            || self.current_word_start.is_none_or(|start| {
                self.physical_context.len() - start < config.auto_correct_min_word_length.max(4)
            })
        {
            return AutoDecision::Continue;
        }
        let mapping = system_layout::shared_installed_mapping().ok().flatten();
        self.evaluate_current_partial_with_mapping(config, mapping)
    }

    fn evaluate_current_partial_with_mapping(
        &mut self,
        config: &Config,
        shared_mapping: Option<system_layout::SharedMapping>,
    ) -> AutoDecision {
        let mapping = shared_mapping.as_deref();
        if !config.switch_layout || self.current_word_mixed {
            return AutoDecision::Continue;
        }
        let (Some(start), Some(layout), Some(_)) = (
            self.current_word_start,
            self.current_word_layout,
            self.active_layout,
        ) else {
            return AutoDecision::Continue;
        };
        let physical = &self.physical_context[start..];
        if physical.len() < config.auto_correct_min_word_length.max(4) {
            return AutoDecision::Continue;
        }
        let policy = correction_policy(config);
        if self.physical_context.contains(' ') {
            // A custom map can introduce a visible space inside the live word.
            // Then its last separator is not stable: retain the uncached path.
            let stable = !mapping
                .is_some_and(|pairs| pairs.iter().any(|(from, to)| *from != ' ' && *to == ' '));
            let cached = self.pending_context.as_ref().filter(|cached| {
                stable
                    && Arc::ptr_eq(&cached.policy, &policy)
                    && cached.mapping.as_deref() == mapping
            });
            let pending = if let Some(cached) = cached {
                cached.pending
            } else {
                let visible = render_visible_context(&self.visible_runs, mapping);
                let pending = visible.rsplit_once(' ').is_some_and(|(previous, _)| {
                    upyr_core::has_pending_context(&format!("{previous} "), &policy, mapping)
                });
                self.pending_context = stable.then(|| PendingContext {
                    policy: Arc::clone(&policy),
                    mapping: shared_mapping.clone(),
                    pending,
                });
                pending
            };
            if pending {
                return AutoDecision::Continue;
            }
        }
        let decision = self
            .partial_state
            .evaluate_physical(physical, layout, &policy, mapping);
        if let AutoDecision::Correct(correction) = &decision {
            if !render_visible_context(&self.visible_runs, mapping)
                .ends_with(&correction.expected_source)
            {
                return AutoDecision::Continue;
            }
        }
        decision
    }

    #[allow(dead_code)] // Owned-snapshot reference used by replay tests/benchmarks.
    pub fn evaluate_partial(&mut self, sample: &WordSample, config: &Config) -> AutoDecision {
        let mapping = system_layout::shared_installed_mapping().ok().flatten();
        evaluate_partial_with_mapping(sample, config, mapping.as_deref(), &mut self.partial_state)
    }
}

fn evaluate_partial_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    state: &mut upyr_core::PartialWordState,
) -> AutoDecision {
    if !config.switch_layout {
        return AutoDecision::Continue;
    }
    let policy = correction_policy(config);
    if sample.inner.physical_word().chars().count() < policy.min_word_length.max(4) {
        return AutoDecision::Continue;
    }
    let visible = sample
        .inner
        .physical_context()
        .contains(' ')
        .then(|| render_visible_context(&sample.visible_runs, mapping));
    if let Some((previous, _)) = visible.as_deref().and_then(|text| text.rsplit_once(' ')) {
        if upyr_core::has_pending_context(&format!("{previous} "), &policy, mapping) {
            // A partial anchor is not a word bigram. Wait for Space/Enter so
            // correcting it cannot discard the unresolved short word.
            return AutoDecision::Continue;
        }
    }
    let decision = state.evaluate(&sample.inner, &policy, mapping);
    if let AutoDecision::Correct(correction) = &decision {
        // Verify the prefix corresponds to the actual layout runs on screen.
        if !visible
            .unwrap_or_else(|| render_visible_context(&sample.visible_runs, mapping))
            .ends_with(&correction.expected_source)
        {
            return AutoDecision::Continue;
        }
    }
    decision
}

#[cfg(any(not(target_os = "macos"), test))]
fn evaluate_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    let visible = render_visible_context(&sample.visible_runs, mapping);
    evaluate_with_visible(sample, config, mapping, &visible)
}

fn evaluate_with_visible(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    visible: &str,
) -> AutoDecision {
    let policy = correction_policy(config);
    if let Some(correction) = upyr_core::evaluate_context(visible, &policy, mapping) {
        return AutoDecision::Correct(correction);
    }
    // A manual switch can retain a short unresolved word. If context evidence
    // abstains, judge only the current word instead of converting older words
    // through its layout. Mid-word mixed-layout repair remains unchanged.
    let context = sample.inner.physical_context();
    let boundary = if context.ends_with(' ') { " " } else { "" };
    let preceding = context.len() - boundary.len() - sample.inner.physical_word().len();
    let mut offset = 0;
    let previous_layout_differs = sample.visible_runs.iter().any(|run| {
        let before = offset < preceding;
        offset += run.physical.len();
        before && run.layout != sample.inner.source_layout()
    });
    let single;
    let inner = if previous_layout_differs {
        single = upyr_core::WordSample::new(
            sample.inner.physical_word(),
            format!("{}{boundary}", sample.inner.physical_word()),
            sample.inner.source_layout(),
        );
        &single
    } else {
        &sample.inner
    };
    let decision = upyr_core::evaluate(inner, &policy, mapping);
    match decision {
        AutoDecision::Correct(mut correction) => {
            // The engine may rewrite only the tail of the tracked context.
            // Physical and visible text align character for character, so the
            // selection is exactly that many trailing visible characters.
            let corrected = correction.expected_source.chars().count();
            let skip = visible.chars().count().saturating_sub(corrected);
            correction.expected_source = visible.chars().skip(skip).collect();
            AutoDecision::Correct(correction)
        }
        decision => decision,
    }
}

/// `пРИВІТ`, `hELLO`: typed with Caps Lock on while Shift was held for the
/// first letter. Returns the intended case (`Привіт`, `Hello`).
#[cfg(any(target_os = "macos", test))]
pub(crate) fn accidental_caps_fix(word: &str) -> Option<String> {
    let cased = |c: &char| c.is_lowercase() || c.is_uppercase();
    if !word
        .chars()
        .all(|c| cased(&c) || matches!(c, '\'' | '’' | 'ʼ'))
    {
        return None;
    }
    let mut letters = word.chars().filter(cased);
    if !letters.next()?.is_lowercase() {
        return None;
    }
    let rest: Vec<char> = letters.collect();
    if rest.len() < 2 || !rest.iter().all(|c| c.is_uppercase()) {
        return None;
    }
    let mut first = true;
    Some(
        word.chars()
            .flat_map(|c| -> Vec<char> {
                if !cased(&c) {
                    vec![c]
                } else if std::mem::take(&mut first) {
                    c.to_uppercase().collect()
                } else {
                    c.to_lowercase().collect()
                }
            })
            .collect(),
    )
}

/// Evaluates a finished word, fixing an accidental Caps Lock first when it is
/// on: `gHBDSN` becomes `Привіт` (layout and case), `пРИВІТ` becomes `Привіт`
/// (case only, `Direction::Smart`, which must not switch layouts).
#[cfg(target_os = "macos")]
pub fn evaluate_with_caps_lock(
    sample: &WordSample,
    config: &Config,
    caps_lock: bool,
    terminal: bool,
) -> AutoDecision {
    let mapping = system_layout::shared_installed_mapping().ok().flatten();
    evaluate_caps_with_mapping(sample, config, mapping.as_deref(), caps_lock, terminal)
}

#[cfg(any(target_os = "macos", test))]
fn evaluate_caps_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    caps_lock: bool,
    terminal: bool,
) -> AutoDecision {
    let visible = render_visible_context(&sample.visible_runs, mapping);
    evaluate_caps_with_visible(sample, config, mapping, caps_lock, terminal, &visible)
}

#[cfg(any(target_os = "macos", test))]
fn evaluate_caps_with_visible(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    caps_lock: bool,
    terminal: bool,
    visible: &str,
) -> AutoDecision {
    let evaluate = |sample: &WordSample| {
        if terminal {
            evaluate_terminal_with_visible(sample, config, mapping, visible)
        } else {
            evaluate_with_visible(sample, config, mapping, visible)
        }
    };
    if !caps_lock {
        return evaluate(sample);
    }
    let boundary = if visible.ends_with(' ') { " " } else { "" };
    let visible_word = visible
        .strip_suffix(' ')
        .unwrap_or(visible)
        .rsplit(' ')
        .next()
        .unwrap_or("");
    let fixes =
        accidental_caps_fix(visible_word).zip(accidental_caps_fix(sample.inner.physical_word()));
    let context = sample.inner.physical_context();
    let prefix = context
        .strip_suffix(' ')
        .unwrap_or(context)
        .strip_suffix(sample.inner.physical_word());
    let (Some((fixed_visible, fixed_physical)), Some(prefix)) = (fixes, prefix) else {
        return evaluate(sample);
    };
    // Judge the word as it was meant to be typed. Selection still targets the
    // text on screen, which every path renders from the original key runs.
    let intended_context = format!("{prefix}{fixed_physical}{boundary}");
    let intended = WordSample {
        inner: upyr_core::WordSample::new(
            fixed_physical,
            intended_context,
            sample.inner.source_layout(),
        ),
        visible_runs: sample.visible_runs.clone(),
    };
    match evaluate(&intended) {
        AutoDecision::Correct(correction) => AutoDecision::Correct(correction),
        _ => AutoDecision::Correct(AutoCorrection {
            expected_source: format!("{visible_word}{boundary}"),
            replacement: format!("{fixed_visible}{boundary}"),
            direction: upyr_core::Direction::Smart,
        }),
    }
}

#[cfg(test)]
fn evaluate_terminal_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    let visible = render_visible_context(&sample.visible_runs, mapping);
    evaluate_terminal_with_visible(sample, config, mapping, &visible)
}

#[cfg(any(target_os = "macos", test))]
fn evaluate_terminal_with_visible(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    visible: &str,
) -> AutoDecision {
    if let Some(correction) =
        upyr_core::evaluate_context(visible, &correction_policy(config), mapping)
    {
        return AutoDecision::Correct(correction);
    }
    evaluate_single_word(sample, config, mapping, " ", visible)
}

/// The word before Enter on macOS: the ordinary decision first, then the
/// short-word policy, because no later word can resolve `nfr` → `так` there.
#[cfg(target_os = "macos")]
pub fn evaluate_line_break(sample: &WordSample, config: &Config, caps_lock: bool) -> AutoDecision {
    let mapping = system_layout::shared_installed_mapping().ok().flatten();
    evaluate_line_break_with_mapping(sample, config, mapping.as_deref(), caps_lock)
}

#[cfg(any(target_os = "macos", test))]
fn evaluate_line_break_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    caps_lock: bool,
) -> AutoDecision {
    let visible = render_visible_context(&sample.visible_runs, mapping);
    match evaluate_caps_with_visible(sample, config, mapping, caps_lock, false, &visible) {
        AutoDecision::Correct(correction) => AutoDecision::Correct(correction),
        decision => match evaluate_single_word(sample, config, mapping, "", &visible) {
            AutoDecision::Correct(correction) => AutoDecision::Correct(correction),
            _ => decision,
        },
    }
}

/// Judges only the current word, with the short-word policy, as terminals do.
/// `boundary` is what follows the word on screen: a space, or nothing yet
/// when Enter is held behind the correction.
#[cfg(any(target_os = "macos", test))]
fn evaluate_single_word(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    boundary: &str,
    visible: &str,
) -> AutoDecision {
    let prepared = correction_policy(config);
    let mut minimum = prepared.min_word_length;
    let word = sample.inner.physical_word().to_ascii_lowercase();
    let intended_word = match sample.inner.source_layout() {
        SystemLayout::English => match mapping {
            Some(mapping) => upyr_core::convert_with_mapping(
                &word,
                upyr_core::Direction::EnglishToUkrainian,
                mapping,
            ),
            None => upyr_core::convert(&word, upyr_core::Direction::EnglishToUkrainian),
        }
        .text
        .to_lowercase(),
        SystemLayout::Ukrainian => word,
    };
    // A leading physical punctuation key can be a Ukrainian initial letter.
    // Keep the normal lexical/model decision, but do not discard a three-letter
    // word such as `;er` -> `жук` before that evidence can be considered.
    if minimum <= 4
        && sample.inner.source_layout() == SystemLayout::English
        && sample
            .inner
            .physical_word()
            .chars()
            .next()
            .zip(intended_word.chars().next())
            .is_some_and(|(source, target)| {
                source.is_ascii_punctuation()
                    && target.is_alphabetic()
                    && !matches!(target, '\'' | '’' | 'ʼ')
            })
        && intended_word.chars().count() >= 3
        && intended_word
            .chars()
            .all(|c| c.is_alphabetic() || matches!(c, '\'' | '’'))
    {
        minimum = minimum.min(3);
    }
    // Common short words can establish either layout at a sentence boundary.
    // Keep the user's threshold for other tokens and explicit stricter settings.
    if minimum <= 4
        && matches!(
            intended_word.as_str(),
            "am" | "an"
                | "ok"
                | "and"
                | "are"
                | "as"
                | "at"
                | "be"
                | "but"
                | "by"
                | "can"
                | "do"
                | "for"
                | "go"
                | "has"
                | "had"
                | "he"
                | "her"
                | "him"
                | "his"
                | "how"
                | "if"
                | "in"
                | "is"
                | "it"
                | "its"
                | "me"
                | "my"
                | "no"
                | "not"
                | "of"
                | "on"
                | "or"
                | "our"
                | "out"
                | "she"
                | "so"
                | "the"
                | "to"
                | "up"
                | "us"
                | "was"
                | "we"
                | "who"
                | "why"
                | "you"
                | "i'm"
                | "i'd"
                | "аби"
                | "або"
                | "але"
                | "без"
                | "був"
                | "вам"
                | "вас"
                | "ваш"
                | "вже"
                | "все"
                | "всі"
                | "від"
                | "він"
                | "де"
                | "для"
                | "до"
                | "між"
                | "на"
                | "нам"
                | "нас"
                | "наш"
                | "не"
                | "ні"
                | "ок"
                | "про"
                | "раз"
                | "так"
                | "ти"
                | "тут"
                | "хай"
                | "хто"
                | "це"
                | "цей"
                | "час"
                | "ще"
                | "що"
                | "щоб"
                | "як"
        )
    {
        minimum = minimum.min(2);
    }
    let mut adjusted;
    let policy = if minimum == prepared.min_word_length {
        prepared.as_ref()
    } else {
        adjusted = (*prepared).clone();
        adjusted.min_word_length = minimum;
        &adjusted
    };
    let single_word = upyr_core::WordSample::new(
        sample.inner.physical_word(),
        format!("{}{boundary}", sample.inner.physical_word()),
        sample.inner.source_layout(),
    );
    match upyr_core::evaluate(&single_word, policy, mapping) {
        AutoDecision::Correct(mut correction) => {
            let word = visible
                .strip_suffix(' ')
                .unwrap_or(visible)
                .rsplit(' ')
                .next()
                .unwrap_or("");
            correction.expected_source = format!("{word}{boundary}");
            AutoDecision::Correct(correction)
        }
        decision => decision,
    }
}

#[cfg(test)]
thread_local! { static RENDER_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn render_visible_context<'a>(
    runs: &'a [LayoutRun],
    mapping: Option<&[(char, char)]>,
) -> Cow<'a, str> {
    #[cfg(test)]
    RENDER_COUNT.with(|count| count.set(count.get() + 1));
    match runs {
        [] => return Cow::Borrowed(""),
        [run] if run.layout == SystemLayout::English => return Cow::Borrowed(&run.physical),
        _ => {}
    }
    let mut visible = String::with_capacity(runs.iter().map(|run| run.physical.len()).sum());
    for run in runs {
        match run.layout {
            SystemLayout::English => visible.push_str(&run.physical),
            SystemLayout::Ukrainian => {
                upyr_core::layout::append_converted(
                    &mut visible,
                    &run.physical,
                    upyr_core::Direction::EnglishToUkrainian,
                    mapping,
                );
            }
        }
    }
    Cow::Owned(visible)
}

/// Offline allocation/timing hook; excluded from ordinary builds.
#[cfg(feature = "engine-bench")]
#[allow(dead_code)]
pub(crate) struct VisibleContextProbe(Vec<LayoutRun>);

#[cfg(feature = "engine-bench")]
#[allow(dead_code)]
impl VisibleContextProbe {
    pub(crate) fn new(runs: &[(SystemLayout, &str)]) -> Self {
        Self(
            runs.iter()
                .map(|(layout, physical)| LayoutRun {
                    layout: *layout,
                    physical: (*physical).to_owned(),
                })
                .collect(),
        )
    }

    pub(crate) fn render(&self, mapping: Option<&[(char, char)]>) -> impl AsRef<str> + '_ {
        render_visible_context(&self.0, mapping)
    }
}

#[derive(Clone)]
struct PolicyCache {
    sensitivity: AutoCorrectSensitivity,
    minimum: usize,
    exceptions: Vec<String>,
    rules: Vec<String>,
    policy: Arc<upyr_core::AutoCorrectPolicy>,
}

fn correction_policy(config: &Config) -> Arc<upyr_core::AutoCorrectPolicy> {
    // Each caller thread owns its cache. Value comparisons notice in-place
    // settings edits and learned rules too; pointer identity alone is unsafe.
    thread_local! {
        static CACHE: RefCell<Option<PolicyCache>> = const { RefCell::new(None) };
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(cached) = cache.as_ref() {
            if cached.sensitivity == config.auto_correct_sensitivity
                && cached.minimum == config.auto_correct_min_word_length
                && cached.exceptions == config.auto_correct_exceptions
                && cached.rules == config.auto_correct_rules
            {
                return Arc::clone(&cached.policy);
            }
        }
        let policy = Arc::new(build_correction_policy(config));
        *cache = Some(PolicyCache {
            sensitivity: config.auto_correct_sensitivity,
            minimum: config.auto_correct_min_word_length,
            exceptions: config.auto_correct_exceptions.clone(),
            rules: config.auto_correct_rules.clone(),
            policy: Arc::clone(&policy),
        });
        policy
    })
}

fn build_correction_policy(config: &Config) -> upyr_core::AutoCorrectPolicy {
    upyr_core::AutoCorrectPolicy {
        sensitivity: match config.auto_correct_sensitivity {
            AutoCorrectSensitivity::Conservative => upyr_core::Sensitivity::Conservative,
            AutoCorrectSensitivity::Balanced => upyr_core::Sensitivity::Balanced,
            AutoCorrectSensitivity::Aggressive => upyr_core::Sensitivity::Aggressive,
        },
        min_word_length: config.auto_correct_min_word_length,
        exceptions: config.auto_correct_exceptions.clone(),
        triggers: config
            .auto_correct_rules
            .iter()
            .filter_map(|rule| upyr_core::parse_trigger(rule).ok().flatten())
            .chain(upyr_core::builtin_triggers())
            .collect(),
    }
}

pub(crate) fn physical_key(key: Keycode) -> Option<upyr_core::PhysicalKey> {
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
        Keycode::Key1 => PhysicalKey::Digit1,
        Keycode::Key2 => PhysicalKey::Digit2,
        Keycode::Key3 => PhysicalKey::Digit3,
        Keycode::Key4 => PhysicalKey::Digit4,
        Keycode::Key5 => PhysicalKey::Digit5,
        Keycode::Key6 => PhysicalKey::Digit6,
        Keycode::Key7 => PhysicalKey::Digit7,
        Keycode::Key8 => PhysicalKey::Digit8,
        Keycode::Key9 => PhysicalKey::Digit9,
        Keycode::Key0 => PhysicalKey::Digit0,
        Keycode::Minus => PhysicalKey::Minus,
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

    fn terminal_sample(word: &str, context: &str, layout: SystemLayout) -> WordSample {
        WordSample {
            inner: upyr_core::WordSample::new(word, context, layout),
            visible_runs: vec![LayoutRun {
                layout,
                physical: context.to_owned(),
            }],
        }
    }

    #[test]
    fn retained_boundary_snapshot_survives_following_input_and_reset() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(SystemLayout::English));
        for key in [
            Keycode::G,
            Keycode::H,
            Keycode::B,
            Keycode::D,
            Keycode::S,
            Keycode::N,
        ] {
            tracker.observe(AutoKeyEvent {
                key,
                shifted: false,
                timestamp: None,
            });
        }
        let sample = tracker
            .observe(AutoKeyEvent {
                key: Keycode::Space,
                shifted: false,
                timestamp: None,
            })
            .unwrap();
        for key in [Keycode::A, Keycode::B, Keycode::Backspace] {
            tracker.observe(AutoKeyEvent {
                key,
                shifted: false,
                timestamp: None,
            });
        }
        tracker.set_source_layout(Some(SystemLayout::Ukrainian));
        tracker.clear();
        assert_eq!(sample.inner.physical_word(), "ghbdsn");
        assert_eq!(sample.inner.physical_context(), "ghbdsn ");
        assert_eq!(
            render_visible_context(&sample.visible_runs, None),
            "ghbdsn "
        );
        let AutoDecision::Correct(correction) =
            evaluate_with_mapping(&sample, &Config::default(), None)
        else {
            panic!("retained sample must remain correctable");
        };
        assert_eq!(correction.expected_source, "ghbdsn ");
        assert_eq!(correction.replacement, "привіт ");
    }

    #[test]
    fn partial_word_correction_preserves_context_and_finishes_typing() {
        let alphabet = [
            Keycode::A,
            Keycode::B,
            Keycode::C,
            Keycode::D,
            Keycode::E,
            Keycode::F,
            Keycode::G,
            Keycode::H,
            Keycode::I,
            Keycode::J,
            Keycode::K,
            Keycode::L,
            Keycode::M,
            Keycode::N,
            Keycode::O,
            Keycode::P,
            Keycode::Q,
            Keycode::R,
            Keycode::S,
            Keycode::T,
            Keycode::U,
            Keycode::V,
            Keycode::W,
            Keycode::X,
            Keycode::Y,
            Keycode::Z,
        ];
        for (physical, initial, expected) in [
            ("continue ", SystemLayout::Ukrainian, "continue "),
            ("connection ", SystemLayout::Ukrainian, "connection "),
            ("control ", SystemLayout::Ukrainian, "control "),
            ("ghbdsn ", SystemLayout::English, "привіт "),
            ("dbghfdktyyz ", SystemLayout::English, "виправлення "),
        ] {
            for terminal in [false, true] {
                let config = Config::default();
                let mut tracker = AutoWordTracker::default();
                let mut layout = initial;
                let mut buffer = String::from("earlier text ");
                let mut early_edits = 0;
                for character in physical.chars() {
                    let key = if character == ' ' {
                        Keycode::Space
                    } else {
                        alphabet[(character as u8 - b'a') as usize]
                    };
                    tracker.set_source_layout(Some(layout));
                    buffer.push_str(&match layout {
                        SystemLayout::English => character.to_string(),
                        SystemLayout::Ukrainian => {
                            upyr_core::convert(
                                &character.to_string(),
                                upyr_core::Direction::EnglishToUkrainian,
                            )
                            .text
                        }
                    });
                    let completed = tracker.observe(AutoKeyEvent {
                        timestamp: None,
                        key,
                        shifted: false,
                    });
                    let decision = if let Some(sample) = completed {
                        if terminal {
                            evaluate_terminal_with_mapping(&sample, &config, None)
                        } else {
                            evaluate_with_mapping(&sample, &config, None)
                        }
                    } else if let Some(sample) = tracker.partial_word_sample() {
                        evaluate_partial_with_mapping(
                            &sample,
                            &config,
                            None,
                            &mut tracker.partial_state,
                        )
                    } else {
                        AutoDecision::Continue
                    };
                    match decision {
                        AutoDecision::Correct(correction) => {
                            assert!(buffer.ends_with(&correction.expected_source));
                            if character != ' ' {
                                early_edits += 1;
                                assert_eq!(correction.replacement.chars().count(), 4);
                            }
                            if terminal {
                                assert!(
                                    crate::automation::terminal_plan_counts_for_test(
                                        &correction.expected_source,
                                        &correction.replacement,
                                        None
                                    )
                                    .is_some()
                                );
                            }
                            buffer.truncate(buffer.len() - correction.expected_source.len());
                            buffer.push_str(&correction.replacement);
                            layout = match correction.direction {
                                upyr_core::Direction::EnglishToUkrainian => SystemLayout::Ukrainian,
                                upyr_core::Direction::UkrainianToEnglish => SystemLayout::English,
                                _ => unreachable!(),
                            };
                            tracker.clear();
                        }
                        AutoDecision::Reset => tracker.clear(),
                        AutoDecision::Continue => {}
                    }
                }
                assert_eq!(early_edits, 1, "{physical}");
                assert_eq!(buffer, format!("earlier text {expected}"));
            }
        }
    }

    #[test]
    fn partial_word_peek_survives_cancelled_edits_backspace_and_layout_changes() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(SystemLayout::Ukrainian));
        for key in [Keycode::C, Keycode::O, Keycode::N, Keycode::T] {
            tracker.observe(AutoKeyEvent {
                timestamp: None,
                key,
                shifted: false,
            });
        }
        let config = Config::default();
        let first = tracker.partial_word_sample().unwrap();
        assert!(matches!(
            evaluate_partial_with_mapping(&first, &config, None, &mut tracker.partial_state),
            AutoDecision::Correct(_)
        ));
        // If the queued edit is overtaken by typing, peeking has consumed no
        // context: the next candidate must include the newly typed key.
        tracker.observe(AutoKeyEvent {
            timestamp: None,
            key: Keycode::I,
            shifted: false,
        });
        let next = tracker.partial_word_sample().unwrap();
        assert_eq!(next.inner.physical_word(), "conti");
        assert_eq!(
            evaluate_partial_with_mapping(&next, &config, None, &mut tracker.partial_state),
            upyr_core::evaluate_partial(&next.inner, &correction_policy(&config), None)
        );
        tracker.observe(AutoKeyEvent {
            timestamp: None,
            key: Keycode::Backspace,
            shifted: false,
        });
        assert_eq!(tracker.partial_word_sample().unwrap(), first);
        assert_eq!(
            evaluate_partial_with_mapping(&first, &config, None, &mut tracker.partial_state),
            upyr_core::evaluate_partial(&first.inner, &correction_policy(&config), None)
        );
        let no_switch = Config {
            switch_layout: false,
            ..config
        };
        assert_eq!(
            evaluate_partial_with_mapping(&first, &no_switch, None, &mut tracker.partial_state),
            AutoDecision::Continue
        );
        tracker.set_source_layout(Some(SystemLayout::English));
        tracker.observe(AutoKeyEvent {
            timestamp: None,
            key: Keycode::I,
            shifted: false,
        });
        assert!(tracker.partial_word_sample().is_none());
        tracker.observe(AutoKeyEvent {
            timestamp: None,
            key: Keycode::Left,
            shifted: false,
        });
        assert!(tracker.partial_word_sample().is_none());
    }

    #[test]
    fn terminal_corrects_the_reported_english_words_without_a_four_letter_delay() {
        for word in ["it's", "not", "so", "good", "for", "the", "i'm", "don't"] {
            let sample = terminal_sample(word, &format!("{word} "), SystemLayout::Ukrainian);
            let decision = evaluate_terminal_with_mapping(&sample, &Config::default(), None);
            let AutoDecision::Correct(correction) = decision else {
                panic!("{word}: {decision:?}")
            };
            assert_eq!(correction.replacement, format!("{word} "));
            assert_eq!(
                correction.expected_source,
                format!(
                    "{} ",
                    upyr_core::convert(word, crate::layout::Direction::EnglishToUkrainian).text
                )
            );
        }
    }

    #[test]
    fn terminal_rewrites_only_a_supported_bounded_tail_after_an_ambiguous_prefix() {
        let sample = terminal_sample("good", "it's not so good ", SystemLayout::Ukrainian);
        let AutoDecision::Correct(correction) =
            evaluate_terminal_with_mapping(&sample, &Config::default(), None)
        else {
            panic!("expected correction")
        };
        assert_eq!(correction.expected_source, "тще іщ пщщв ");
        assert_eq!(correction.replacement, "not so good ");
    }

    #[test]
    fn terminal_corrects_the_reported_switch_back_to_ukrainian() {
        let mut failures = Vec::new();
        for (physical, expected) in [
            ("nfr", "так "),
            ("lj,ht", "добре "),
            ("zr", "як "),
            ("[jnskjcz", "хотілося "),
            ("Nfr", "Так "),
            ("{nj", "Хто "),
            ("wtq", "цей "),
            ("hj,bnm", "робить "),
            ("nht,f", "треба "),
            ("hjpevs.", "розумію "),
            ("dsl,edf'nmcz", "відбувається "),
        ] {
            let sample = terminal_sample(physical, &format!("{physical} "), SystemLayout::English);
            let decision = evaluate_terminal_with_mapping(&sample, &Config::default(), None);
            let AutoDecision::Correct(correction) = decision else {
                failures.push(format!("{physical}: {decision:?}"));
                continue;
            };
            assert_eq!(correction.expected_source, format!("{physical} "));
            assert_eq!(correction.replacement, expected);
        }
        assert!(failures.is_empty(), "{}", failures.join("; "));
    }

    #[test]
    fn terminal_corrects_words_starting_on_punctuation_keys() {
        let mut failures = Vec::new();
        for (physical, replacement) in [
            ("[ez", "хуя"),
            ("[eq", "хуй"),
            (";er", "жук"),
            ("];f", "їжа"),
            (";jgf", "жопа"),
            ("[b;f", "хижа"),
            ("[jxe", "хочу"),
            ("'yjn", "єнот"),
            (".yfr", "юнак"),
            ("\\fyjr", "ґанок"),
            ("[eqyz", "хуйня"),
        ] {
            let sample = terminal_sample(physical, &format!("{physical} "), SystemLayout::English);
            let decision = evaluate_terminal_with_mapping(&sample, &Config::default(), None);
            match decision {
                AutoDecision::Correct(correction) => {
                    assert_eq!(correction.expected_source, format!("{physical} "));
                    assert_eq!(correction.replacement, format!("{replacement} "));
                }
                _ => failures.push(format!("{physical}: {decision:?}")),
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("; "));
    }

    #[test]
    fn leading_punctuation_keeps_technical_tokens_exceptions_and_strict_limits() {
        for physical in [
            ".env", ".git", "[id", "[raw", "[foo", "[bar", ";ls", "[x]", "/tmp", "./a", "\\n",
        ] {
            let sample = terminal_sample(physical, &format!("{physical} "), SystemLayout::English);
            assert!(
                !matches!(
                    evaluate_terminal_with_mapping(&sample, &Config::default(), None),
                    AutoDecision::Correct(_)
                ),
                "{physical}"
            );
        }
        let sample = terminal_sample("[ez", "[ez ", SystemLayout::English);
        for config in [
            Config {
                auto_correct_min_word_length: 5,
                ..Config::default()
            },
            Config {
                auto_correct_exceptions: vec!["[ez".to_owned()],
                ..Config::default()
            },
        ] {
            assert!(!matches!(
                evaluate_terminal_with_mapping(&sample, &config, None),
                AutoDecision::Correct(_)
            ));
        }
    }

    #[test]
    fn terminal_short_word_policy_keeps_real_english_and_exceptions() {
        for word in [
            "it's", "not", "so", "for", "git", "ls", "cd", "rm", "ps", "su", "pwd", "npm", "npx",
            "pip", "uv", "go", "sh", "zsh", "rg", "fd", "cp", "mv", "ln", "du", "df", "jq", "awk",
            "sed", "ssh", "scp", "tar", "zip", "curl", "env", "vs",
        ] {
            let sample = terminal_sample(word, &format!("{word} "), SystemLayout::English);
            assert!(!matches!(
                evaluate_terminal_with_mapping(&sample, &Config::default(), None),
                AutoDecision::Correct(_)
            ));
        }
        let sample = terminal_sample("not", "not ", SystemLayout::Ukrainian);
        let config = Config {
            auto_correct_exceptions: vec!["тще".to_owned()],
            ..Config::default()
        };
        assert!(!matches!(
            evaluate_terminal_with_mapping(&sample, &config, None),
            AutoDecision::Correct(_)
        ));
        for (physical, layout) in [
            ("not", SystemLayout::Ukrainian),
            ("nfr", SystemLayout::English),
            ("zr", SystemLayout::English),
        ] {
            let strict = Config {
                auto_correct_min_word_length: 5,
                ..Config::default()
            };
            let sample = terminal_sample(physical, &format!("{physical} "), layout);
            assert!(!matches!(
                evaluate_terminal_with_mapping(&sample, &strict, None),
                AutoDecision::Correct(_)
            ));
        }
    }

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
    fn accidental_caps_lock_words_are_recognized_by_their_inverted_case() {
        for (typed, fixed) in [
            ("пРИВІТ", "Привіт"),
            ("hELLO", "Hello"),
            ("gHBDSN", "Ghbdsn"),
        ] {
            assert_eq!(
                accidental_caps_fix(typed).as_deref(),
                Some(fixed),
                "{typed}"
            );
        }
        for typed in ["ПРИВІТ", "привіт", "Привіт", "пРИвіт", "hE", "a1B"] {
            assert_eq!(accidental_caps_fix(typed), None, "{typed}");
        }
    }

    #[test]
    fn caps_lock_accidents_are_fixed_with_or_without_a_layout_change() {
        let config = Config::default();
        for (word, layout, expected, replacement, direction) in [
            // Wrong layout as well: convert and fix the case.
            (
                "gHBDSN",
                SystemLayout::English,
                "gHBDSN ",
                "Привіт ",
                upyr_core::Direction::EnglishToUkrainian,
            ),
            // Right layout: only the case changes, and no layout switch.
            (
                "gHBDSN",
                SystemLayout::Ukrainian,
                "пРИВІТ ",
                "Привіт ",
                upyr_core::Direction::Smart,
            ),
            (
                "hELLO",
                SystemLayout::English,
                "hELLO ",
                "Hello ",
                upyr_core::Direction::Smart,
            ),
        ] {
            let sample = terminal_sample(word, &format!("{word} "), layout);
            for terminal in [false, true] {
                let decision = evaluate_caps_with_mapping(&sample, &config, None, true, terminal);
                let AutoDecision::Correct(correction) = decision else {
                    panic!("{expected:?} (terminal: {terminal}) was not fixed: {decision:?}");
                };
                assert_eq!(
                    (
                        correction.expected_source.as_str(),
                        correction.replacement.as_str(),
                        correction.direction
                    ),
                    (expected, replacement, direction),
                    "terminal: {terminal}"
                );
            }
        }
        // Without Caps Lock the same shape is left alone (`iPHONE`-like text).
        let sample = terminal_sample("hELLO", "hELLO ", SystemLayout::English);
        assert!(!matches!(
            evaluate_caps_with_mapping(&sample, &config, None, false, false),
            AutoDecision::Correct(correction) if correction.replacement == "Hello "
        ));
    }

    #[test]
    fn user_rules_reach_both_correction_paths() {
        let config = Config {
            auto_correct_rules: vec!["zzzz correct".to_owned(), "ghbdsn keep".to_owned()],
            ..Config::default()
        };
        for (path, decide) in [
            (
                "desktop",
                evaluate_with_mapping as fn(&WordSample, &Config, Option<&[(char, char)]>) -> _,
            ),
            ("terminal", evaluate_terminal_with_mapping),
        ] {
            let forced = decide(
                &terminal_sample("zzzz", "zzzz ", SystemLayout::English),
                &config,
                None,
            );
            let AutoDecision::Correct(correction) = forced else {
                panic!("{path}: a user `correct` rule did not apply: {forced:?}");
            };
            assert_eq!(correction.replacement, "яяяя ", "{path}");
            // A user `keep` beats the built-in `ghbdsn correct` trigger.
            assert!(
                !matches!(
                    decide(
                        &terminal_sample("ghbdsn", "ghbdsn ", SystemLayout::English),
                        &config,
                        None
                    ),
                    AutoDecision::Correct(_)
                ),
                "{path}: a user `keep` rule did not apply"
            );
        }
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

    #[test]
    fn conservative_policy_recognizes_reported_ukrainian_word() {
        let mut tracker = AutoWordTracker::default();
        tracker.set_source_layout(Some(SystemLayout::English));
        let mut sample = None;
        for key in [
            Keycode::G,
            Keycode::H,
            Keycode::F,
            Keycode::W,
            Keycode::Dot,
            Keycode::Apostrophe,
            Keycode::I,
            Keycode::Space,
        ] {
            sample = tracker.observe(AutoKeyEvent {
                timestamp: None,
                key,
                shifted: false,
            });
        }

        let decision = upyr_core::evaluate(
            &sample.expect("word sample after Space").inner,
            &correction_policy(&Config::default()),
            None,
        );
        let AutoDecision::Correct(correction) = decision else {
            panic!("expected correction, got {decision:?}");
        };
        assert_eq!(correction.expected_source, "ghfw.'i ");
        assert_eq!(correction.replacement, "працюєш ");
    }

    #[test]
    fn tracker_keeps_the_capital_first_key_in_reported_ukrainian_words() {
        fn event(key: Keycode, shifted: bool) -> AutoKeyEvent {
            AutoKeyEvent {
                timestamp: None,
                key,
                shifted,
            }
        }

        for (events, expected_source, expected_replacement) in [
            (
                vec![
                    event(Keycode::LShift, false),
                    event(Keycode::G, true),
                    event(Keycode::J, false),
                    event(Keycode::L, false),
                    event(Keycode::S, false),
                    event(Keycode::Z, false),
                    event(Keycode::Space, false),
                ],
                "Gjlsz ",
                "Подія ",
            ),
            (
                vec![
                    event(Keycode::LShift, false),
                    event(Keycode::H, true),
                    event(Keycode::J, false),
                    event(Keycode::Comma, false),
                    event(Keycode::J, false),
                    event(Keycode::N, false),
                    event(Keycode::F, false),
                    event(Keycode::Space, false),
                ],
                "Hj,jnf ",
                "Робота ",
            ),
        ] {
            let mut tracker = AutoWordTracker::default();
            let mut decision = AutoDecision::Continue;
            for event in events {
                if tracker.needs_layout_check() && AutoWordTracker::can_begin(event.key) {
                    tracker.set_source_layout(Some(SystemLayout::English));
                }
                if let Some(sample) = tracker.observe(event) {
                    decision = upyr_core::evaluate(
                        &sample.inner,
                        &correction_policy(&Config::default()),
                        None,
                    );
                }
            }

            let AutoDecision::Correct(correction) = decision else {
                panic!("expected title-case correction, got {decision:?}");
            };
            assert_eq!(correction.expected_source, expected_source);
            assert_eq!(correction.replacement, expected_replacement);
        }
    }

    #[test]
    fn typed_number_row_punctuation_and_hyphens_are_corrected_in_both_paths() {
        fn event(key: Keycode, shifted: bool) -> AutoKeyEvent {
            AutoKeyEvent {
                timestamp: None,
                key,
                shifted,
            }
        }
        let letters = |word: &str| -> Vec<AutoKeyEvent> {
            word.chars()
                .map(|character| {
                    let key = match character {
                        'b' => Keycode::B,
                        'd' => Keycode::D,
                        'e' => Keycode::E,
                        'g' => Keycode::G,
                        'h' => Keycode::H,
                        'l' => Keycode::L,
                        'm' => Keycode::M,
                        'n' => Keycode::N,
                        'o' => Keycode::O,
                        'q' => Keycode::Q,
                        'r' => Keycode::R,
                        's' => Keycode::S,
                        'z' => Keycode::Z,
                        _ => unreachable!("test words use these keys"),
                    };
                    event(key, false)
                })
                .collect()
        };
        let space = event(Keycode::Space, false);
        for (layout, events, replacement) in [
            (
                SystemLayout::English,
                [letters("ghbdsn"), vec![event(Keycode::Key1, true), space]].concat(),
                "привіт! ",
            ),
            (
                SystemLayout::English,
                [letters("ghbdsn"), vec![event(Keycode::Key7, true), space]].concat(),
                "привіт? ",
            ),
            (
                SystemLayout::English,
                [
                    vec![event(Keycode::Comma, false)],
                    letters("elm"),
                    vec![event(Keycode::Minus, false)],
                    letters("zrbq"),
                    vec![space],
                ]
                .concat(),
                "будь-який ",
            ),
            (
                SystemLayout::Ukrainian,
                [letters("hello"), vec![event(Keycode::Key1, true), space]].concat(),
                "hello! ",
            ),
        ] {
            let mut tracker = AutoWordTracker::default();
            let mut sample = None;
            for event in events {
                if tracker.needs_layout_check() && AutoWordTracker::can_begin(event.key) {
                    tracker.set_source_layout(Some(layout));
                }
                if let Some(finished) = tracker.observe(event) {
                    sample = Some(finished);
                }
            }
            let sample = sample.unwrap_or_else(|| panic!("{replacement:?} was never tracked"));
            for (path, decision) in [
                (
                    "desktop",
                    evaluate_with_mapping(&sample, &Config::default(), None),
                ),
                (
                    "terminal",
                    evaluate_terminal_with_mapping(&sample, &Config::default(), None),
                ),
            ] {
                let AutoDecision::Correct(correction) = decision else {
                    panic!("{path}: {replacement:?} was not corrected: {decision:?}");
                };
                assert_eq!(correction.replacement, replacement, "{path}");
            }
        }
    }

    #[test]
    fn ok_is_a_short_word_in_both_layouts() {
        let config = Config::default();
        // `jr` is `ок` typed on English; `щл` is `ok` typed on Ukrainian.
        for (physical, layout, replacement) in [
            ("jr", SystemLayout::English, "ок"),
            ("ok", SystemLayout::Ukrainian, "ok"),
        ] {
            let terminal = evaluate_terminal_with_mapping(
                &terminal_sample(physical, &format!("{physical} "), layout),
                &config,
                None,
            );
            let AutoDecision::Correct(correction) = terminal else {
                panic!("terminal: {physical:?} on {layout:?} was not corrected: {terminal:?}");
            };
            assert_eq!(correction.replacement, format!("{replacement} "));

            let sample = WordSample {
                inner: upyr_core::WordSample::new(physical, physical, layout),
                visible_runs: vec![LayoutRun {
                    layout,
                    physical: physical.to_owned(),
                }],
            };
            let desktop = evaluate_with_mapping(
                &terminal_sample(physical, &format!("{physical} "), layout),
                &config,
                None,
            );
            let AutoDecision::Correct(correction) = desktop else {
                panic!("desktop: {physical:?} on {layout:?} was not corrected: {desktop:?}");
            };
            assert_eq!(correction.replacement, format!("{replacement} "));

            let line_break = evaluate_line_break_with_mapping(&sample, &config, None, false);
            let AutoDecision::Correct(correction) = line_break else {
                panic!(
                    "before Enter: {physical:?} on {layout:?} was not corrected: {line_break:?}"
                );
            };
            assert_eq!(correction.replacement, replacement);
        }
        // Typed on the right layout, both stay as they are.
        for (physical, layout) in [
            ("ok", SystemLayout::English),
            ("jr", SystemLayout::Ukrainian),
        ] {
            assert!(!matches!(
                evaluate_terminal_with_mapping(
                    &terminal_sample(physical, &format!("{physical} "), layout),
                    &config,
                    None
                ),
                AutoDecision::Correct(_)
            ));
        }
    }

    #[test]
    fn short_words_before_enter_use_the_short_word_policy() {
        // Nothing follows the last word of a message to resolve it, so the
        // common short words are corrected there as in terminals.
        for (physical, layout, expected, replacement) in [
            ("nfr", SystemLayout::English, "nfr", "так"),
            ("zr", SystemLayout::English, "zr", "як"),
            ("ghbdsn", SystemLayout::English, "ghbdsn", "привіт"),
        ] {
            let sample = WordSample {
                inner: upyr_core::WordSample::new(physical, physical, layout),
                visible_runs: vec![LayoutRun {
                    layout,
                    physical: physical.to_owned(),
                }],
            };
            let decision =
                evaluate_line_break_with_mapping(&sample, &Config::default(), None, false);
            let AutoDecision::Correct(correction) = decision else {
                panic!("{physical:?} before Enter was not corrected: {decision:?}");
            };
            assert_eq!(
                (
                    correction.expected_source.as_str(),
                    correction.replacement.as_str()
                ),
                (expected, replacement)
            );
        }
        // Real English stays.
        let sample = WordSample {
            inner: upyr_core::WordSample::new("is", "is", SystemLayout::English),
            visible_runs: vec![LayoutRun {
                layout: SystemLayout::English,
                physical: "is".to_owned(),
            }],
        };
        assert!(!matches!(
            evaluate_line_break_with_mapping(&sample, &Config::default(), None, false),
            AutoDecision::Correct(_)
        ));
    }

    #[test]
    fn a_line_break_sample_ends_the_word_without_a_boundary_or_side_effects() {
        let mut tracker = AutoWordTracker::default();
        for key in [
            Keycode::O,
            Keycode::K,
            Keycode::Space,
            Keycode::G,
            Keycode::H,
            Keycode::B,
            Keycode::D,
            Keycode::S,
            Keycode::N,
        ] {
            if tracker.needs_layout_check() && AutoWordTracker::can_begin(key) {
                tracker.set_source_layout(Some(SystemLayout::English));
            }
            tracker.observe(AutoKeyEvent {
                timestamp: None,
                key,
                shifted: false,
            });
        }

        let sample = tracker.line_break_sample().expect("a word is being typed");
        let AutoDecision::Correct(correction) =
            evaluate_with_mapping(&sample, &Config::default(), None)
        else {
            panic!("the word before Enter should be corrected");
        };
        // Enter is held back, so nothing follows the word on screen yet.
        assert_eq!(correction.expected_source, "ghbdsn");
        assert_eq!(correction.replacement, "привіт");

        // Peeking does not finish the word: Space still completes it.
        let finished = tracker
            .observe(AutoKeyEvent {
                timestamp: None,
                key: Keycode::Space,
                shifted: false,
            })
            .expect("the same word finishes at Space");
        assert_eq!(finished.inner.physical_word(), "ghbdsn");
        assert!(AutoWordTracker::default().line_break_sample().is_none());
    }

    #[test]
    fn a_repeated_space_keeps_the_selection_aligned_with_the_screen() {
        // Screen after typing: "nfr  ,elt " (two spaces). The selection the
        // app makes must be a suffix of exactly that text.
        let screen = "nfr  ,elt ";
        let mut tracker = AutoWordTracker::default();
        let mut sample = None;
        for character in screen.chars() {
            let key = match character {
                ' ' => Keycode::Space,
                ',' => Keycode::Comma,
                'e' => Keycode::E,
                'f' => Keycode::F,
                'l' => Keycode::L,
                'n' => Keycode::N,
                'r' => Keycode::R,
                't' => Keycode::T,
                _ => unreachable!(),
            };
            if tracker.needs_layout_check() && AutoWordTracker::can_begin(key) {
                tracker.set_source_layout(Some(SystemLayout::English));
            }
            if let Some(finished) = tracker.observe(AutoKeyEvent {
                timestamp: None,
                key,
                shifted: false,
            }) {
                sample = Some(finished);
            }
        }
        let sample = sample.expect("the last word is tracked");
        if let AutoDecision::Correct(correction) =
            evaluate_with_mapping(&sample, &Config::default(), None)
        {
            assert!(
                screen.ends_with(&correction.expected_source),
                "{:?} is not the end of {screen:?}",
                correction.expected_source
            );
        }
    }

    #[test]
    fn desktop_selects_only_the_visible_suffix_the_engine_corrects() {
        // Real source-language words typed before a wrong-layout word stay
        // untouched: only the final word may be selected and replaced.
        for (word, context, layout, expected_source, replacement) in [
            (
                "ghbdsn",
                "ok ghbdsn ",
                SystemLayout::English,
                "ghbdsn ",
                "привіт ",
            ),
            (
                "hello",
                "s hello ",
                SystemLayout::Ukrainian,
                "руддщ ",
                "hello ",
            ),
        ] {
            let sample = terminal_sample(word, context, layout);
            let decision = evaluate_with_mapping(&sample, &Config::default(), None);
            let AutoDecision::Correct(correction) = decision else {
                panic!("expected correction for {context:?}, got {decision:?}");
            };
            assert_eq!(correction.expected_source, expected_source);
            assert_eq!(correction.replacement, replacement);
        }
    }

    #[test]
    fn mixed_layout_word_uses_exact_visible_multiword_context() {
        fn type_keys(
            tracker: &mut AutoWordTracker,
            layout: SystemLayout,
            keys: &[Keycode],
        ) -> Option<WordSample> {
            let mut sample = None;
            for &key in keys {
                if AutoWordTracker::can_begin(key) {
                    tracker.set_source_layout(Some(layout));
                }
                sample = tracker.observe(AutoKeyEvent {
                    timestamp: None,
                    key,
                    shifted: false,
                });
            }
            sample
        }

        let mut tracker = AutoWordTracker::default();

        // A valid Ukrainian word before the layout change must not be included
        // in the later English-to-Ukrainian replacement.
        type_keys(
            &mut tracker,
            SystemLayout::Ukrainian,
            &[
                Keycode::G,
                Keycode::H,
                Keycode::B,
                Keycode::D,
                Keycode::S,
                Keycode::N,
                Keycode::Space,
            ],
        );

        let first = type_keys(
            &mut tracker,
            SystemLayout::English,
            &[Keycode::N, Keycode::B, Keycode::Space],
        )
        .expect("ambiguous first word sample");
        assert_eq!(
            upyr_core::evaluate(&first.inner, &correction_policy(&Config::default()), None,),
            AutoDecision::Continue
        );

        type_keys(
            &mut tracker,
            SystemLayout::English,
            &[Keycode::D, Keycode::V, Keycode::S],
        );
        let mixed = type_keys(
            &mut tracker,
            SystemLayout::Ukrainian,
            &[Keycode::N, Keycode::B, Keycode::Space],
        )
        .expect("mixed-layout word sample");

        let decision = evaluate_with_mapping(&mixed, &Config::default(), None);
        let AutoDecision::Correct(correction) = decision else {
            panic!("expected correction, got {decision:?}");
        };

        assert_eq!(correction.expected_source, "nb dvsти ");
        assert_eq!(correction.replacement, "ти вміти ");
        assert_eq!(
            correction.direction,
            upyr_core::Direction::EnglishToUkrainian
        );
    }
}

#[cfg(test)]
mod prepared_policy_tests {
    use super::*;

    #[test]
    fn prepared_policy_tracks_all_decision_settings_and_in_place_edits() {
        let mut config = Config::default();
        let first = correction_policy(&config);
        assert!(Arc::ptr_eq(&first, &correction_policy(&config)));
        config.switch_layout = !config.switch_layout;
        assert!(Arc::ptr_eq(&first, &correction_policy(&config)));
        config.auto_correct_exceptions.push("Cont".into());
        let exceptions = correction_policy(&config);
        assert!(!Arc::ptr_eq(&first, &exceptions));
        config
            .auto_correct_rules
            .push("cont* keep ukrainian".into());
        let rules = correction_policy(&config);
        assert!(!Arc::ptr_eq(&exceptions, &rules));
        config.auto_correct_rules[0] = "cont* correct ukrainian".into();
        assert_ne!(*rules, *correction_policy(&config));
        config.auto_correct_min_word_length += 1;
        assert_eq!(
            *correction_policy(&config),
            build_correction_policy(&config)
        );
        config.auto_correct_sensitivity = AutoCorrectSensitivity::Aggressive;
        assert_eq!(
            *correction_policy(&config),
            build_correction_policy(&config)
        );
    }
}
