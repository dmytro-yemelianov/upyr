use device_query::Keycode;

use crate::{
    config::{AutoCorrectSensitivity, Config},
    system_layout::{self, SystemLayout},
};

pub use upyr_core::{AutoCorrection, AutoDecision};

const MAX_TRACKED_CONTEXT_CHARACTERS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoKeyEvent {
    pub key: Keycode,
    pub shifted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutRun {
    layout: SystemLayout,
    physical: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordSample {
    pub(crate) inner: upyr_core::WordSample,
    visible_runs: Vec<LayoutRun>,
    #[cfg(any(target_os = "macos", test))]
    physical_word: String,
    #[cfg(any(target_os = "macos", test))]
    source_layout: SystemLayout,
}

#[derive(Default)]
pub struct AutoWordTracker {
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
        if self.active_layout != layout && self.current_word_start.is_none() {
            // A layout change between words begins a new language context. Do
            // not let an earlier ambiguous word sweep into the next phrase.
            self.clear_context();
        }
        self.active_layout = layout;
    }

    pub fn clear(&mut self) {
        self.clear_context();
        self.active_layout = None;
    }

    pub fn observe(&mut self, event: AutoKeyEvent) -> Option<WordSample> {
        let Some(key) = physical_key(event.key) else {
            self.clear();
            return None;
        };
        match key {
            upyr_core::PhysicalKey::Space => return self.finish_word(),
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
            #[cfg(any(target_os = "macos", test))]
            physical_word: physical_word.clone(),
            #[cfg(any(target_os = "macos", test))]
            source_layout,
            inner: upyr_core::WordSample::new(
                physical_word,
                self.physical_context.clone(),
                source_layout,
            ),
            visible_runs: self.visible_runs.clone(),
        })
    }

    fn backspace(&mut self) {
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
        self.physical_context.clear();
        self.visible_runs.clear();
        self.current_word_start = None;
        self.current_word_layout = None;
        self.current_word_mixed = false;
    }
}

pub fn evaluate(sample: &WordSample, config: &Config) -> AutoDecision {
    let policy = correction_policy(config);
    let mapping = system_layout::installed_mapping().ok().flatten();
    let decision = upyr_core::evaluate(&sample.inner, &policy, mapping.as_deref());
    match decision {
        AutoDecision::Correct(mut correction) => {
            correction.expected_source =
                render_visible_context(&sample.visible_runs, mapping.as_deref());
            AutoDecision::Correct(correction)
        }
        decision => decision,
    }
}

/// Terminal edits are bounded to the current word, even when the language
/// engine has accumulated an ambiguous phrase before it.
#[cfg(target_os = "macos")]
pub fn evaluate_terminal(sample: &WordSample, config: &Config) -> AutoDecision {
    let mapping = system_layout::installed_mapping().ok().flatten();
    evaluate_terminal_with_mapping(sample, config, mapping.as_deref())
}

#[cfg(any(target_os = "macos", test))]
fn evaluate_terminal_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    let mut policy = correction_policy(config);
    let word = sample.physical_word.to_ascii_lowercase();
    let intended_word = match sample.source_layout {
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
    // Common short words can establish either layout at a sentence boundary.
    // Keep the user's threshold for other tokens and explicit stricter settings.
    if policy.min_word_length <= 4
        && matches!(
            intended_word.as_str(),
            "am" | "an"
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
        policy.min_word_length = policy.min_word_length.min(2);
    }
    let single_word = upyr_core::WordSample::new(
        sample.physical_word.clone(),
        format!("{} ", sample.physical_word),
        sample.source_layout,
    );
    match upyr_core::evaluate(&single_word, &policy, mapping) {
        AutoDecision::Correct(mut correction) => {
            let visible = render_visible_context(&sample.visible_runs, mapping);
            let word = visible
                .strip_suffix(' ')
                .unwrap_or(&visible)
                .rsplit(' ')
                .next()
                .unwrap_or("");
            correction.expected_source = format!("{word} ");
            AutoDecision::Correct(correction)
        }
        decision => decision,
    }
}

fn render_visible_context(runs: &[LayoutRun], mapping: Option<&[(char, char)]>) -> String {
    let mut visible = String::new();
    for run in runs {
        match run.layout {
            SystemLayout::English => visible.push_str(&run.physical),
            SystemLayout::Ukrainian => {
                let converted = if let Some(mapping) = mapping {
                    upyr_core::convert_with_mapping(
                        &run.physical,
                        upyr_core::Direction::EnglishToUkrainian,
                        mapping,
                    )
                } else {
                    upyr_core::convert(&run.physical, upyr_core::Direction::EnglishToUkrainian)
                };
                visible.push_str(&converted.text);
            }
        }
    }
    visible
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
        triggers: upyr_core::builtin_triggers(),
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
            physical_word: word.to_owned(),
            source_layout: layout,
            visible_runs: vec![LayoutRun {
                layout,
                physical: context.to_owned(),
            }],
        }
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
    fn terminal_rewrites_only_the_current_word_after_an_ambiguous_prefix() {
        let sample = terminal_sample("good", "it's not so good ", SystemLayout::Ukrainian);
        let AutoDecision::Correct(correction) =
            evaluate_terminal_with_mapping(&sample, &Config::default(), None)
        else {
            panic!("expected correction")
        };
        assert_eq!(correction.expected_source, "пщщв ");
        assert_eq!(correction.replacement, "good ");
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
            AutoKeyEvent { key, shifted }
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

        let core_decision =
            upyr_core::evaluate(&mixed.inner, &correction_policy(&Config::default()), None);
        let AutoDecision::Correct(mut correction) = core_decision else {
            panic!("expected correction, got {core_decision:?}");
        };
        correction.expected_source = render_visible_context(&mixed.visible_runs, None);

        assert_eq!(correction.expected_source, "nb dvsти ");
        assert_eq!(correction.replacement, "ти вміти ");
        assert_eq!(
            correction.direction,
            upyr_core::Direction::EnglishToUkrainian
        );
    }
}
