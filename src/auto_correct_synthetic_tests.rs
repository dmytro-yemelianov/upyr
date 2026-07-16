use super::*;

struct SyntheticTyping {
    tracker: AutoWordTracker,
    config: Config,
    active_layout: SystemLayout,
    buffer: String,
    corrections: Vec<AutoCorrection>,
    layout_switches: usize,
}

impl SyntheticTyping {
    fn new() -> Self {
        Self {
            tracker: AutoWordTracker::default(),
            config: Config {
                auto_correct: true,
                ..Config::default()
            },
            active_layout: SystemLayout::English,
            buffer: String::new(),
            corrections: Vec::new(),
            layout_switches: 0,
        }
    }

    fn switch_layout(&mut self, layout: SystemLayout) {
        if self.active_layout != layout {
            self.active_layout = layout;
            self.layout_switches += 1;
        }
    }

    fn type_intended(
        &mut self,
        text: &str,
        intended_layout: SystemLayout,
        active_layout: SystemLayout,
    ) {
        self.switch_layout(active_layout);
        let physical = match intended_layout {
            SystemLayout::English => text.to_owned(),
            SystemLayout::Ukrainian => convert(text, Direction::UkrainianToEnglish).text,
        };
        self.type_physical(&physical);
    }

    fn type_physical(&mut self, physical: &str) {
        for character in physical.chars() {
            let event = physical_event(character);
            if self.tracker.needs_layout_check() && AutoWordTracker::can_begin(event.key) {
                self.tracker.set_source_layout(Some(self.active_layout));
            }

            self.buffer
                .push_str(&visible_character(character, self.active_layout));
            let Some(sample) = self.tracker.observe(event) else {
                continue;
            };

            match evaluate(&sample, &self.config) {
                AutoDecision::Correct(correction) => {
                    assert!(
                        self.buffer.ends_with(&correction.expected_source),
                        "virtual buffer {:?} does not end with expected source {:?}",
                        self.buffer,
                        correction.expected_source
                    );
                    let start = self.buffer.len() - correction.expected_source.len();
                    self.buffer.replace_range(start.., &correction.replacement);
                    if self.config.switch_layout {
                        let target_layout = match correction.direction {
                            Direction::EnglishToUkrainian => SystemLayout::Ukrainian,
                            Direction::UkrainianToEnglish => SystemLayout::English,
                            Direction::Smart => unreachable!("evaluated correction is directional"),
                        };
                        self.switch_layout(target_layout);
                    }
                    self.corrections.push(correction);
                    // App::apply_auto_correction clears the tracker after a
                    // successful guarded replacement.
                    self.tracker.clear();
                }
                AutoDecision::Continue => {}
                AutoDecision::Reset => self.tracker.clear(),
            }
        }
    }
}

fn visible_character(character: char, layout: SystemLayout) -> String {
    match layout {
        SystemLayout::English => character.to_string(),
        SystemLayout::Ukrainian => to_ukrainian(&character.to_string()),
    }
}

fn physical_event(character: char) -> AutoKeyEvent {
    let (key, shifted) = match character {
        'a'..='z' => (
            character
                .to_ascii_uppercase()
                .to_string()
                .parse()
                .expect("ASCII letter has a keycode"),
            false,
        ),
        'A'..='Z' => (
            character
                .to_string()
                .parse()
                .expect("ASCII letter has a keycode"),
            true,
        ),
        ' ' => (Keycode::Space, false),
        '`' => (Keycode::Grave, false),
        '~' => (Keycode::Grave, true),
        '[' => (Keycode::LeftBracket, false),
        '{' => (Keycode::LeftBracket, true),
        ']' => (Keycode::RightBracket, false),
        '}' => (Keycode::RightBracket, true),
        '\\' => (Keycode::BackSlash, false),
        '|' => (Keycode::BackSlash, true),
        ';' => (Keycode::Semicolon, false),
        ':' => (Keycode::Semicolon, true),
        '\'' => (Keycode::Apostrophe, false),
        '"' => (Keycode::Apostrophe, true),
        ',' => (Keycode::Comma, false),
        '<' => (Keycode::Comma, true),
        '.' => (Keycode::Dot, false),
        '>' => (Keycode::Dot, true),
        '/' => (Keycode::Slash, false),
        '?' => (Keycode::Slash, true),
        unsupported => panic!("unsupported synthetic physical character: {unsupported:?}"),
    };
    let event = AutoKeyEvent { key, shifted };
    if character != ' ' {
        assert_eq!(
            physical_english_character(event.key, event.shifted),
            Some(character),
            "synthetic event must round-trip to its physical key"
        );
    }
    event
}

#[test]
fn synthetic_native_text_preserves_languages_product_names_and_punctuation() {
    const ENGLISH: &str = "FAANG companies build SaaS platforms; NASDAQ tracks Apple, iPhone, ServiceNow, Microsoft, Google, Amazon, Meta, Netflix. ";
    const UKRAINIAN: &str =
        "Українська клавіатура швидко перемикає розкладку, пунктуацію та великі літери. ";

    let mut typing = SyntheticTyping::new();
    typing.type_intended(ENGLISH, SystemLayout::English, SystemLayout::English);
    typing.type_intended(UKRAINIAN, SystemLayout::Ukrainian, SystemLayout::Ukrainian);

    assert_eq!(typing.buffer, format!("{ENGLISH}{UKRAINIAN}"));
    assert!(typing.corrections.is_empty());
    assert_eq!(typing.layout_switches, 1);
}

#[test]
fn synthetic_wrong_layout_text_corrects_both_directions_with_punctuation() {
    const UKRAINIAN: &str = "Ольга перевіряє українську клавіатуру, налаштування та пунктуацію. ";
    const ENGLISH: &str =
        "FAANG companies prefer SaaS platforms; NASDAQ compares iPhone with ServiceNow. ";

    let mut typing = SyntheticTyping::new();
    typing.type_intended(UKRAINIAN, SystemLayout::Ukrainian, SystemLayout::English);
    typing.type_intended(ENGLISH, SystemLayout::English, SystemLayout::Ukrainian);

    assert_eq!(typing.buffer, format!("{UKRAINIAN}{ENGLISH}"));
    assert!(typing.corrections.len() >= 2);
    assert_eq!(typing.layout_switches, 2);
}

#[test]
fn synthetic_mid_sentence_layout_switches_do_not_cross_contaminate_context() {
    let segments = [
        ("ServiceNow ", SystemLayout::English),
        ("перевіряє ", SystemLayout::Ukrainian),
        ("NASDAQ, ", SystemLayout::English),
        ("клавіатуру. ", SystemLayout::Ukrainian),
        ("SaaS ", SystemLayout::English),
        ("налаштування, ", SystemLayout::Ukrainian),
        ("iPhone. ", SystemLayout::English),
    ];
    let expected = segments.iter().map(|(text, _)| *text).collect::<String>();

    let mut typing = SyntheticTyping::new();
    for (text, layout) in segments {
        typing.type_intended(text, layout, layout);
    }

    assert_eq!(typing.buffer, expected);
    assert!(typing.corrections.is_empty());
    assert_eq!(typing.layout_switches, 6);
}

#[test]
fn synthetic_edge_identifiers_convert_only_when_wrong_layout_context_confirms_it() {
    for text in [
        "FAANG companies. ",
        "SaaS platform. ",
        "NASDAQ market. ",
        "iPhone device. ",
        "ServiceNow platform. ",
    ] {
        let mut typing = SyntheticTyping::new();
        typing.type_intended(text, SystemLayout::English, SystemLayout::Ukrainian);
        assert_eq!(typing.buffer, text, "wrong-layout context failed: {text}");
        assert!(
            !typing.corrections.is_empty(),
            "wrong-layout context produced no correction: {text}"
        );
    }
}

#[test]
fn synthetic_technical_prefixes_are_not_swept_into_later_ukrainian_corrections() {
    for technical in ["github.com ", "src/main.rs ", "https://example.com "] {
        let mut typing = SyntheticTyping::new();
        typing.type_intended(technical, SystemLayout::English, SystemLayout::English);
        typing.type_intended(
            "перевіримо. ",
            SystemLayout::Ukrainian,
            SystemLayout::English,
        );
        assert_eq!(typing.buffer, format!("{technical}перевіримо. "));
    }
}

#[test]
fn synthetic_physical_punctuation_uses_the_active_os_mapping() {
    const COMMON_PHYSICAL: &str = "[];',./{}:\"<>? ";
    const COMMON_UKRAINIAN: &str = "хїжєбю.ХЇЖЄБЮ, ";
    const ALL_PHYSICAL_PUNCTUATION: &str = "`~[]{}\\|;:'\",<.>/? ";

    let mut common = SyntheticTyping::new();
    common.switch_layout(SystemLayout::Ukrainian);
    common.type_physical(COMMON_PHYSICAL);
    assert_eq!(common.buffer, COMMON_UKRAINIAN);

    let expected = to_ukrainian(ALL_PHYSICAL_PUNCTUATION);
    let mut all = SyntheticTyping::new();
    all.switch_layout(SystemLayout::Ukrainian);
    all.type_physical(ALL_PHYSICAL_PUNCTUATION);
    assert_eq!(all.buffer, expected);
}

#[test]
fn deterministic_random_mix_exercises_layouts_languages_and_edge_contexts() {
    const CASES: &[(&str, SystemLayout, SystemLayout)] = &[
        ("FAANG, ", SystemLayout::English, SystemLayout::English),
        ("SaaS. ", SystemLayout::English, SystemLayout::English),
        ("NASDAQ: ", SystemLayout::English, SystemLayout::English),
        ("iPhone; ", SystemLayout::English, SystemLayout::English),
        ("ServiceNow? ", SystemLayout::English, SystemLayout::English),
        (
            "українська, ",
            SystemLayout::Ukrainian,
            SystemLayout::Ukrainian,
        ),
        (
            "клавіатура. ",
            SystemLayout::Ukrainian,
            SystemLayout::Ukrainian,
        ),
        (
            "перевіримо, ",
            SystemLayout::Ukrainian,
            SystemLayout::English,
        ),
        (
            "налаштування. ",
            SystemLayout::Ukrainian,
            SystemLayout::English,
        ),
        ("keyboard, ", SystemLayout::English, SystemLayout::Ukrainian),
        (
            "configuration. ",
            SystemLayout::English,
            SystemLayout::Ukrainian,
        ),
        (
            "ServiceNow, ",
            SystemLayout::English,
            SystemLayout::Ukrainian,
        ),
    ];

    let mut seed = 0x5eed_f00du32;
    let mut expected = String::new();
    let mut typing = SyntheticTyping::new();
    for iteration in 0..96 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let (text, intended, active) = CASES[(seed as usize) % CASES.len()];
        expected.push_str(text);
        typing.type_intended(text, intended, active);
        assert_eq!(
            typing.buffer, expected,
            "synthetic case {iteration} failed for {text:?} ({intended:?} text on {active:?} layout)"
        );
    }

    assert_eq!(typing.buffer, expected);
    assert!(typing.layout_switches >= 30);
    assert!(typing.corrections.len() >= 20);
}
