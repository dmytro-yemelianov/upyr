//! Prior evaluation routing retained as a differential oracle.
use super::*;

#[test]
fn streamed_visible_context_matches_original_for_unicode_and_mapping_edits() {
    let texts = [
        "",
        "abc XYZ",
        "ghbdsn # @ &",
        "і ї є ґ",
        "a’b σΣ ß",
        "🚀 界",
        "  ",
    ];
    let mut custom = vec![('a', '🦀'), ('a', 'x'), (' ', '_'), ('і', 'i'), ('#', '№')];
    let mut cases = vec![vec![]];
    for first in texts {
        for second in texts {
            for a in [SystemLayout::English, SystemLayout::Ukrainian] {
                for b in [SystemLayout::English, SystemLayout::Ukrainian] {
                    cases.push(vec![
                        LayoutRun {
                            layout: a,
                            physical: first.into(),
                        },
                        LayoutRun {
                            layout: b,
                            physical: second.into(),
                        },
                    ]);
                }
            }
        }
    }
    cases.push(
        (0..256)
            .map(|i| LayoutRun {
                layout: if i % 2 == 0 {
                    SystemLayout::English
                } else {
                    SystemLayout::Ukrainian
                },
                physical: "a # 🚀".into(),
            })
            .collect(),
    );
    let installed = upyr_core::default_physical_mapping();
    for _ in 0..2 {
        for runs in &cases {
            for mapping in [
                None,
                Some(&[][..]),
                Some(installed.as_slice()),
                Some(custom.as_slice()),
            ] {
                assert_eq!(
                    render_visible_context(runs, mapping),
                    reference_render_visible_context(runs, mapping)
                );
                for run in runs {
                    let single = std::slice::from_ref(run);
                    assert_eq!(
                        render_visible_context(single, mapping),
                        reference_render_visible_context(single, mapping)
                    );
                }
            }
        }
        // Same allocation, changed contents: rendering must not cache by pointer.
        custom[0] = ('a', 'і');
        custom[2] = (' ', '🦀');
    }
}

#[test]
fn english_visible_context_borrows_snapshot_text() {
    let runs = [LayoutRun {
        layout: SystemLayout::English,
        physical: "hello 🚀".into(),
    }];
    let visible = render_visible_context(&runs, None);
    assert!(matches!(visible, Cow::Borrowed(_)));
    assert_eq!(visible.as_ptr(), runs[0].physical.as_ptr());
    assert!(matches!(
        render_visible_context(&[], None),
        Cow::Borrowed("")
    ));
}

fn reference_render_visible_context(
    runs: &[LayoutRun],
    mapping: Option<&[(char, char)]>,
) -> String {
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

fn reference_evaluate_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    let policy = correction_policy(config);
    let visible = reference_render_visible_context(&sample.visible_runs, mapping);
    if let Some(correction) = upyr_core::evaluate_context(&visible, &policy, mapping) {
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

fn reference_evaluate_caps_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    caps_lock: bool,
    terminal: bool,
) -> AutoDecision {
    let evaluate = |sample: &WordSample| {
        if terminal {
            reference_evaluate_terminal_with_mapping(sample, config, mapping)
        } else {
            reference_evaluate_with_mapping(sample, config, mapping)
        }
    };
    if !caps_lock {
        return evaluate(sample);
    }
    let visible = reference_render_visible_context(&sample.visible_runs, mapping);
    let boundary = if visible.ends_with(' ') { " " } else { "" };
    let visible_word = visible
        .strip_suffix(' ')
        .unwrap_or(&visible)
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

fn reference_evaluate_terminal_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
) -> AutoDecision {
    if let Some(correction) = upyr_core::evaluate_context(
        &reference_render_visible_context(&sample.visible_runs, mapping),
        &correction_policy(config),
        mapping,
    ) {
        return AutoDecision::Correct(correction);
    }
    reference_evaluate_single_word(sample, config, mapping, " ")
}

fn reference_evaluate_line_break_with_mapping(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    caps_lock: bool,
) -> AutoDecision {
    match reference_evaluate_caps_with_mapping(sample, config, mapping, caps_lock, false) {
        AutoDecision::Correct(correction) => AutoDecision::Correct(correction),
        decision => match reference_evaluate_single_word(sample, config, mapping, "") {
            AutoDecision::Correct(correction) => AutoDecision::Correct(correction),
            _ => decision,
        },
    }
}

fn reference_evaluate_single_word(
    sample: &WordSample,
    config: &Config,
    mapping: Option<&[(char, char)]>,
    boundary: &str,
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
            let visible = reference_render_visible_context(&sample.visible_runs, mapping);
            let word = visible
                .strip_suffix(' ')
                .unwrap_or(&visible)
                .rsplit(' ')
                .next()
                .unwrap_or("");
            correction.expected_source = format!("{word}{boundary}");
            AutoDecision::Correct(correction)
        }
        decision => decision,
    }
}

#[test]
fn shared_rendering_preserves_all_boundary_routes_and_renders_once() {
    let mappings = [
        None,
        Some(vec![]),
        Some(vec![
            ('g', 'п'),
            ('H', 'Р'),
            ('B', 'И'),
            ('D', 'В'),
            ('S', 'І'),
            ('N', 'Т'),
            ('I', 'Ш'),
            (' ', '🦀'),
        ]),
    ];
    for mapping in &mappings {
        for minimum in [2, 4, 5] {
            let config = Config {
                auto_correct_min_word_length: minimum,
                auto_correct_exceptions: vec!["vb".into()],
                ..Config::default()
            };
            for source in [SystemLayout::English, SystemLayout::Ukrainian] {
                for previous in [SystemLayout::English, SystemLayout::Ukrainian] {
                    for (prefix, word) in [
                        ("", "gHBDSN"),
                        ("ok ", "ghbdsn"),
                        ("I ", "have"),
                        ("nj ", "vb"),
                        ("", "nfr"),
                        ("", "hELLO"),
                        ("", "i’m"),
                        ("", "ßAA"),
                        ("", "σΣΑ"),
                        ("", "Ghbdtn,"),
                    ] {
                        for boundary in ["", " "] {
                            let context = format!("{prefix}{word}{boundary}");
                            let mut runs = Vec::new();
                            if !prefix.is_empty() {
                                runs.push(LayoutRun {
                                    layout: previous,
                                    physical: prefix.into(),
                                });
                            }
                            runs.push(LayoutRun {
                                layout: source,
                                physical: format!("{word}{boundary}"),
                            });
                            let sample = WordSample {
                                inner: upyr_core::WordSample::new(word, context, source),
                                visible_runs: runs,
                            };
                            for caps in [false, true] {
                                for terminal in [false, true] {
                                    let expected = reference_evaluate_caps_with_mapping(
                                        &sample,
                                        &config,
                                        mapping.as_deref(),
                                        caps,
                                        terminal,
                                    );
                                    RENDER_COUNT.with(|count| count.set(0));
                                    let actual = evaluate_caps_with_mapping(
                                        &sample,
                                        &config,
                                        mapping.as_deref(),
                                        caps,
                                        terminal,
                                    );
                                    assert_eq!(
                                        actual, expected,
                                        "{prefix:?}/{word:?}/{boundary:?}, {source:?}/{previous:?}, caps={caps}, terminal={terminal}"
                                    );
                                    assert_eq!(RENDER_COUNT.with(|count| count.get()), 1);
                                }
                                let expected = reference_evaluate_line_break_with_mapping(
                                    &sample,
                                    &config,
                                    mapping.as_deref(),
                                    caps,
                                );
                                RENDER_COUNT.with(|count| count.set(0));
                                let actual = evaluate_line_break_with_mapping(
                                    &sample,
                                    &config,
                                    mapping.as_deref(),
                                    caps,
                                );
                                assert_eq!(actual, expected);
                                assert_eq!(RENDER_COUNT.with(|count| count.get()), 1);
                            }
                        }
                    }
                }
            }
        }
    }
}
