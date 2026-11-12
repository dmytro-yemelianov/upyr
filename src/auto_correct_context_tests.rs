use super::*;

#[test]
fn extending_selection_requires_an_exact_suffix_of_the_rechecked_prefix() {
    let mut config = Config {
        direction: upyr_core::Direction::UkrainianToEnglish,
        ..Config::default()
    };
    assert_eq!(
        catch_up_selected_prefix("сщте", "cont", "щтеш", "раніше сщтеш", &config)
            .unwrap()
            .replacement,
        "conti"
    );
    for selected in ["сщте", "теш", " щте", "щтешт"] {
        assert!(catch_up_selected_prefix("сщте", "cont", selected, "сщтеш", &config).is_none());
    }
    config.auto_correct_exceptions = vec!["сщтеш".into()];
    assert!(catch_up_selected_prefix("сщте", "cont", "щтеш", "сщтеш", &config).is_none());
}

#[test]
fn delivered_prefix_catchup_requires_the_same_word_and_rechecks_exceptions() {
    let mut config = Config {
        direction: upyr_core::Direction::EnglishToUkrainian,
        ..Config::default()
    };
    assert_eq!(
        catch_up_prefix("dbgh", "випр", "earlier dbghfd", &config)
            .unwrap()
            .replacement,
        "виправ"
    );
    for text in [
        "otherword",
        "dbgh",
        "xdbghfd",
        "dbgh fd",
        "dbghfd ",
        "dbghfd1",
        "dbghfd_foo",
    ] {
        assert!(
            catch_up_prefix("dbgh", "випр", text, &config).is_none(),
            "{text}"
        );
    }
    config.auto_correct_exceptions = vec!["dbghfd".into()];
    assert!(catch_up_prefix("dbgh", "випр", "dbghfd", &config).is_none());
    config.auto_correct_exceptions.clear();
    config.direction = upyr_core::Direction::UkrainianToEnglish;
    assert_eq!(
        catch_up_prefix("сщте", "cont", "раніше сщтеш", &config)
            .unwrap()
            .replacement,
        "conti"
    );
    assert!(catch_up_prefix("сщте", "else", "сщтеш", &config).is_none());
}

fn key(c: char) -> AutoKeyEvent {
    let letters = [
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
    AutoKeyEvent {
        timestamp: None,
        key: match c.to_ascii_lowercase() {
            'a'..='z' => letters[(c.to_ascii_lowercase() as u8 - b'a') as usize],
            ' ' => Keycode::Space,
            ',' => Keycode::Comma,
            '.' => Keycode::Dot,
            '[' => Keycode::LeftBracket,
            ']' => Keycode::RightBracket,
            ';' => Keycode::Semicolon,
            '\'' => Keycode::Apostrophe,
            '/' => Keycode::Slash,
            '-' => Keycode::Minus,
            _ => panic!("unsupported {c:?}"),
        },
        shifted: c.is_ascii_uppercase(),
    }
}

fn replay(
    label: &str,
    initial: SystemLayout,
    segments: &[(Option<SystemLayout>, &str)],
    terminal: bool,
) -> String {
    let mut tracker = AutoWordTracker::default();
    let mut layout = initial;
    let mut text = String::new();
    let mut edits = Vec::new();
    for &(manual, physical) in segments {
        if let Some(changed) = manual {
            layout = changed;
        }
        for c in physical.chars() {
            let event = key(c);
            if AutoWordTracker::can_begin(event.key) {
                tracker.set_source_layout(Some(layout));
            }
            text.push_str(&match layout {
                SystemLayout::English => c.to_string(),
                SystemLayout::Ukrainian => {
                    upyr_core::convert(&c.to_string(), upyr_core::Direction::EnglishToUkrainian)
                        .text
                }
            });
            let decision = if let Some(sample) = tracker.observe(event) {
                evaluate_caps_with_mapping(&sample, &Config::default(), None, false, terminal)
            } else if let Some(sample) = tracker.partial_word_sample() {
                evaluate_partial_with_mapping(
                    &sample,
                    &Config::default(),
                    None,
                    &mut tracker.partial_state,
                )
            } else {
                AutoDecision::Continue
            };
            match decision {
                AutoDecision::Correct(fix) => {
                    assert!(
                        text.ends_with(&fix.expected_source),
                        "{label}: {text:?} {fix:?}"
                    );
                    if terminal {
                        assert!(
                            crate::automation::terminal_plan_counts_for_test(
                                &fix.expected_source,
                                &fix.replacement,
                                None
                            )
                            .is_some()
                        );
                    }
                    text.truncate(text.len() - fix.expected_source.len());
                    text.push_str(&fix.replacement);
                    layout = match fix.direction {
                        upyr_core::Direction::EnglishToUkrainian => SystemLayout::Ukrainian,
                        upyr_core::Direction::UkrainianToEnglish => SystemLayout::English,
                        _ => layout,
                    };
                    edits.push(fix);
                    tracker.clear();
                }
                AutoDecision::Reset => tracker.clear(),
                AutoDecision::Continue => {}
            }
        }
    }
    println!("{label}, terminal={terminal}: {text:?}, edits={edits:?}");
    text
}

#[test]
fn context_short_words_survive_following_word_and_manual_switch() {
    for terminal in [false, true] {
        for manual in [false, true] {
            assert_eq!(
                replay(
                    "nj vb",
                    SystemLayout::English,
                    &[
                        (None, "nj vb "),
                        (
                            manual.then_some(SystemLayout::Ukrainian),
                            "ntgth yjhvfkmyj ,eltvj "
                        ),
                    ],
                    terminal
                ),
                "то ми тепер нормально будемо "
            );
            assert_eq!(
                replay(
                    "I have",
                    SystemLayout::Ukrainian,
                    &[
                        (None, "I "),
                        (manual.then_some(SystemLayout::English), "have a question "),
                    ],
                    terminal
                ),
                "I have a question "
            );
        }
        let uk = |s: &str| upyr_core::convert(s, upyr_core::Direction::UkrainianToEnglish).text;
        assert_eq!(
            replay(
                "native code switching",
                SystemLayout::Ukrainian,
                &[
                    (None, &uk("тепер нормально будемо ")),
                    (Some(SystemLayout::English), "fastest as ever proof of "),
                    (
                        Some(SystemLayout::Ukrainian),
                        &uk("працювати і нас ніщо не зупинить ")
                    ),
                ],
                terminal
            ),
            "тепер нормально будемо fastest as ever proof of працювати і нас ніщо не зупинить "
        );
        assert_eq!(
            replay(
                "native abbreviations",
                SystemLayout::English,
                &[(None, "NJ VB vs I have a question ")],
                terminal
            ),
            "NJ VB vs I have a question "
        );
    }
}

#[test]
fn pending_context_is_abandoned_on_navigation_and_recovers_after_backspace() {
    let mut tracker = AutoWordTracker::default();
    tracker.set_source_layout(Some(SystemLayout::Ukrainian));
    for c in "I ".chars() {
        tracker.observe(key(c));
    }
    tracker.manual_layout_switch(Some(SystemLayout::English));
    for c in "havx".chars() {
        tracker.observe(key(c));
    }
    tracker.observe(AutoKeyEvent {
        timestamp: None,
        key: Keycode::Backspace,
        shifted: false,
    });
    tracker.observe(key('e'));
    let sample = tracker.line_break_sample().unwrap();
    let expected = AutoCorrection {
        expected_source: "Ш have".into(),
        replacement: "I have".into(),
        direction: upyr_core::Direction::UkrainianToEnglish,
    };
    assert_eq!(
        evaluate_line_break_with_mapping(&sample, &Config::default(), None, false),
        AutoDecision::Correct(expected)
    );
    tracker.observe(AutoKeyEvent {
        timestamp: None,
        key: Keycode::Left,
        shifted: false,
    });
    assert!(tracker.line_break_sample().is_none());
    tracker.set_source_layout(Some(SystemLayout::English));
    for c in "have".chars() {
        tracker.observe(key(c));
    }
    let sample = tracker.line_break_sample().unwrap();
    assert!(!matches!(
        evaluate_with_mapping(&sample, &Config::default(), None),
        AutoDecision::Correct(_)
    ));
}

#[test]
fn pending_context_defers_early_edit_but_not_an_explicitly_protected_word() {
    let mut tracker = AutoWordTracker::default();
    tracker.set_source_layout(Some(SystemLayout::Ukrainian));
    for c in "I have".chars() {
        tracker.observe(key(c));
    }
    let sample = tracker.partial_word_sample().unwrap();
    assert_eq!(
        evaluate_partial_with_mapping(
            &sample,
            &Config::default(),
            None,
            &mut tracker.partial_state
        ),
        AutoDecision::Continue
    );
    let config = Config {
        auto_correct_exceptions: vec!["Ш".into()],
        ..Config::default()
    };
    // The exception protects Ш, so have may still be fixed on its own.
    let AutoDecision::Correct(edit) =
        evaluate_partial_with_mapping(&sample, &config, None, &mut tracker.partial_state)
    else {
        panic!("have should be corrected independently")
    };
    assert_eq!(edit.expected_source, "рфму");
    assert_eq!(edit.replacement, "have");
    // If the early edit was overtaken by typing, the word-boundary fallback
    // must still honor the exception/Keep rule on the previous token.
    let finished = tracker.observe(key(' ')).unwrap();
    for config in [
        config,
        Config {
            auto_correct_rules: vec!["Ш keep".into()],
            ..Config::default()
        },
    ] {
        for terminal in [false, true] {
            let AutoDecision::Correct(edit) =
                evaluate_caps_with_mapping(&finished, &config, None, false, terminal)
            else {
                panic!("anchor should be corrected")
            };
            assert_eq!(edit.expected_source, "рфму ");
            assert_eq!(edit.replacement, "have ");
        }
    }
}

#[test]
fn borrowed_partial_context_matches_uncached_path_across_edits_and_policy_changes() {
    let mut tracker = AutoWordTracker::default();
    let mut reference = upyr_core::PartialWordState::default();
    let mut config = Config::default();
    let mut mapping = upyr_core::default_physical_mapping();
    let mut changed = mapping.clone();
    changed.insert(0, ('c', ' '));
    for initial in [SystemLayout::English, SystemLayout::Ukrainian] {
        for text in [
            "we can continue",
            "nj vb dbghfdktyyz",
            "I have context",
            "vs continue",
            "nj  dbghf",
            "nj dbghf",
            "c cont",
            "nj vb cont",
        ] {
            tracker.clear();
            tracker.set_source_layout(Some(initial));
            for (index, c) in text.chars().enumerate() {
                tracker.observe(key(c));
                for step in 0..5 {
                    config.auto_correct_min_word_length = if step == 1 { 8 } else { 4 };
                    config.auto_correct_exceptions = if step == 2 {
                        vec!["nj".into(), "сщте".into()]
                    } else {
                        vec![]
                    };
                    config.auto_correct_rules = if step == 3 {
                        vec!["cont* keep ukrainian".into()]
                    } else {
                        vec![]
                    };
                    let map = if step == 4 {
                        Some(changed.as_slice())
                    } else {
                        Some(mapping.as_slice())
                    };
                    let expected =
                        tracker
                            .partial_word_sample()
                            .map_or(AutoDecision::Continue, |sample| {
                                evaluate_partial_with_mapping(&sample, &config, map, &mut reference)
                            });
                    assert_eq!(
                        tracker.evaluate_current_partial_with_mapping(&config, map.map(Arc::from)),
                        expected,
                        "{text} / {index} / {step}"
                    );
                }
                if index % 5 == 4 && c != ' ' {
                    tracker.observe(AutoKeyEvent {
                        key: Keycode::Backspace,
                        shifted: false,
                        timestamp: None,
                    });
                    let expected =
                        tracker
                            .partial_word_sample()
                            .map_or(AutoDecision::Continue, |sample| {
                                evaluate_partial_with_mapping(
                                    &sample,
                                    &config,
                                    None,
                                    &mut reference,
                                )
                            });
                    assert_eq!(
                        tracker.evaluate_current_partial_with_mapping(&config, None),
                        expected
                    );
                    tracker.observe(key(c));
                }
            }
            // Editing a mapping in place must not reuse a cached pending verdict.
            mapping[0].1 = if mapping[0].1 == 'й' { 'x' } else { 'й' };
            tracker.manual_layout_switch(Some(if initial == SystemLayout::English {
                SystemLayout::Ukrainian
            } else {
                SystemLayout::English
            }));
            assert_eq!(
                tracker.evaluate_current_partial_with_mapping(
                    &config,
                    Some(Arc::from(mapping.as_slice()))
                ),
                AutoDecision::Continue
            );
        }
    }
}
