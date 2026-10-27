//! Headless regression corpus: detector -> terminal edit plan -> undo.
use super::*;

#[derive(serde::Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}

#[derive(serde::Deserialize)]
struct Case {
    name: String,
    physical: String,
    layout: String,
    expected: Option<String>,
    visible: Option<String>,
    mapping: Option<String>,
    #[serde(default)]
    minimum: Option<usize>,
    #[serde(default)]
    exceptions: Vec<String>,
}

fn sample(case: &Case, prefix: &str) -> WordSample {
    let layout = match case.layout.as_str() {
        "english" => SystemLayout::English,
        "ukrainian" => SystemLayout::Ukrainian,
        _ => panic!("unknown fixture layout"),
    };
    let context = format!("{prefix}{} ", case.physical);
    WordSample {
        inner: upyr_core::WordSample::new(&case.physical, &context, layout),
        physical_word: case.physical.clone(),
        source_layout: layout,
        visible_runs: vec![LayoutRun {
            layout,
            physical: context,
        }],
    }
}

#[test]
fn curated_edge_corpus_reaches_a_reversible_terminal_plan() {
    #[derive(serde::Deserialize)]
    struct Mapping {
        pairs: Vec<[char; 2]>,
    }
    let native: Mapping = toml::from_str(include_str!(
        "../tests/fixtures/layouts/macos-abc-ukrainian-pc.toml"
    ))
    .unwrap();
    let native: Vec<_> = native.pairs.into_iter().map(|[a, b]| (a, b)).collect();
    let corpus: Corpus =
        toml::from_str(include_str!("../tests/fixtures/terminal-edge-cases.toml")).unwrap();
    let mut failures = Vec::new();
    let mut evaluated = 0;
    for case in &corpus.cases {
        for (profile, mapping) in [("portable", None), ("macos", Some(native.as_slice()))] {
            if case
                .mapping
                .as_deref()
                .is_some_and(|wanted| wanted != profile)
            {
                continue;
            }
            for prefix in ["", "ambiguous earlier words "] {
                evaluated += 1;
                let config = Config {
                    auto_correct_min_word_length: case.minimum.unwrap_or(4),
                    auto_correct_exceptions: case.exceptions.clone(),
                    ..Config::default()
                };
                let decision =
                    evaluate_terminal_with_mapping(&sample(case, prefix), &config, mapping);
                match (&case.expected, decision) {
                    (None, AutoDecision::Continue | AutoDecision::Reset) => {}
                    (Some(expected), AutoDecision::Correct(c)) => {
                        if let Some(visible) = &case.visible {
                            assert_eq!(
                                c.expected_source,
                                format!("{visible} "),
                                "{} ({profile})",
                                case.name
                            );
                        }
                        if c.replacement != format!("{expected} ") {
                            failures.push(format!(
                                "{}: unexpected replacement {:?}",
                                case.name, c.replacement
                            ));
                            continue;
                        }
                        let counts = crate::automation::terminal_plan_counts_for_test(
                            &c.expected_source,
                            &c.replacement,
                            mapping,
                        );
                        if counts
                            != Some((
                                c.expected_source.chars().count(),
                                c.replacement.chars().count(),
                            ))
                        {
                            failures.push(format!(
                                "{}: accepted word did not reach a reversible terminal edit",
                                case.name
                            ));
                        }
                        // Simulate precisely the planned deletion without selecting another word.
                        let mut text = format!("untouched prefix {}", c.expected_source);
                        for _ in 0..c.expected_source.chars().count() {
                            text.pop();
                        }
                        text.push_str(&c.replacement);
                        assert_eq!(text, format!("untouched prefix {expected} "));
                        for _ in 0..c.replacement.chars().count() {
                            text.pop();
                        }
                        text.push_str(&c.expected_source);
                        assert_eq!(text, format!("untouched prefix {}", c.expected_source));
                    }
                    (expected, decision) => failures.push(format!(
                        "{}: expected {expected:?}, got {decision:?}",
                        case.name
                    )),
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    println!(
        "{evaluated} curated evaluations ({} cases, portable/macOS maps, 2 contexts)",
        corpus.cases.len()
    );
}
