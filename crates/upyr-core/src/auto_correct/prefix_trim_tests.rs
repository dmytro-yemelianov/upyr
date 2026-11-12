//! Original character-vector walk retained as a differential oracle.
use super::*;

fn reference_drop_source_language_prefix<S: CandidateScorer + ?Sized>(
    correction: AutoCorrection,
    candidates: &Candidates,
    policy: &AutoCorrectPolicy,
    scorer: &S,
    mapping: Option<&[(char, char)]>,
) -> AutoCorrection {
    let source: Vec<char> = correction.expected_source.chars().collect();
    let target: Vec<char> = correction.replacement.chars().collect();
    // Conversion is character-for-character, so word boundaries line up.
    if source.len() != target.len() {
        return correction;
    }
    let body_end = source.len() - source.iter().rev().take_while(|c| **c == ' ').count();
    let current_start = source[..body_end]
        .iter()
        .rposition(|c| *c == ' ')
        .map_or(0, |index| index + 1);

    let (minimum_coverage, minimum_advantage, _) = model_thresholds(policy.sensitivity);
    let text = |characters: &[char]| characters.iter().collect::<String>();
    let mut start = current_start;
    while start > 0 {
        let previous_end = start - 1;
        let previous_start = source[..previous_end]
            .iter()
            .rposition(|c| *c == ' ')
            .map_or(0, |index| index + 1);
        let word = text(&source[previous_start..previous_end]);
        if !word.is_empty() {
            let source_word = normalize_word(&word);
            let target_text = text(&target[previous_start..previous_end]);
            let target_word = normalize_word(&target_text);
            let (physical, source_layout) = match candidates.source_language {
                Language::English => (Cow::Borrowed(source_word.as_ref()), InputLayout::English),
                Language::Ukrainian => (
                    Cow::Owned(match mapping {
                        Some(mapping) => {
                            convert_with_mapping(
                                &source_word,
                                Direction::UkrainianToEnglish,
                                mapping,
                            )
                            .text
                        }
                        None => convert(&source_word, Direction::UkrainianToEnglish).text,
                    }),
                    InputLayout::Ukrainian,
                ),
            };
            let deliberate_source = is_user_exception(policy, &source_word)
                || policy.triggers.iter().any(|rule| {
                    rule.action == TriggerAction::Keep && rule.matches(&physical, source_layout)
                })
                || known(candidates.source_language, &source_word)
                || (matches!(candidates.source_language, Language::English)
                    && (looks_like_deliberate_latin_identifier(&word)
                        || looks_like_deliberate_latin_technical_token(&word)));
            let evidence = scorer.compare(candidates.text_pair(&source_word, &target_word));
            let reads_as_source = evidence.source.coverage >= minimum_coverage
                && evidence.source.coverage - evidence.target.coverage >= minimum_advantage;
            if deliberate_source || reads_as_source {
                break;
            }
        }
        start = previous_start;
    }

    if start == 0 {
        return correction;
    }
    AutoCorrection {
        expected_source: text(&source[start..]),
        replacement: text(&target[start..]),
        direction: correction.direction,
    }
}

#[derive(Default)]
struct RecordingScorer(RefCell<Vec<(String, String)>>);
impl CandidateScorer for RecordingScorer {
    fn compare(&self, pair: CandidatePair<'_>) -> PairEvidence {
        self.0
            .borrow_mut()
            .push((pair.source.text.into(), pair.target.text.into()));
        PairEvidence {
            source: ModelEvidence {
                coverage: if pair.source.text.starts_with('z') {
                    0.9
                } else {
                    0.0
                },
                grams: 6,
            },
            target: ModelEvidence {
                coverage: 0.0,
                grams: 6,
            },
        }
    }
}

#[test]
fn byte_walk_matches_original_character_walk_and_scorer_calls() {
    let vocabulary = [
        "ok",
        "zzzz",
        "α",
        "é",
        "🙂",
        "привіт",
        "nfr",
        "a’b",
        "",
        "ghbdsn",
    ];
    let custom = [
        ('o', '界'),
        ('k', '🙂'),
        (' ', '_'),
        ('g', 'і'),
        ('а', 'a'),
        ('α', 'Σ'),
    ];
    let mut seed = 17_u64;
    for iteration in 0..250 {
        let mut words = Vec::new();
        for _ in 0..iteration % 8 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            words.push(vocabulary[(seed >> 32) as usize % vocabulary.len()]);
        }
        let text = words.join(" ") + &" ".repeat(iteration % 3);
        for mapping in [None, Some(custom.as_slice())] {
            for layout in [InputLayout::English, InputLayout::Ukrainian] {
                let sample = WordSample::new("ghbdsn", &text, layout);
                let candidates = Candidates::new(&sample, mapping);
                let policy = AutoCorrectPolicy {
                    exceptions: vec!["nfr".into(), "a'b".into()],
                    ..Default::default()
                };
                for mismatch in [false, true] {
                    let correction = AutoCorrection {
                        expected_source: candidates.source_context().to_owned(),
                        replacement: candidates.target_context().to_owned()
                            + if mismatch { "x" } else { "" },
                        direction: candidates.direction,
                    };
                    let original = RecordingScorer::default();
                    let actual = RecordingScorer::default();
                    let expected = reference_drop_source_language_prefix(
                        correction.clone(),
                        &candidates,
                        &policy,
                        &original,
                        mapping,
                    );
                    let got = drop_source_language_prefix(
                        correction,
                        &candidates,
                        &policy,
                        &actual,
                        mapping,
                    );
                    assert_eq!(got, expected, "{text:?} {layout:?}");
                    assert_eq!(*actual.0.borrow(), *original.0.borrow(), "{text:?}");
                }
            }
        }
    }
}

#[test]
fn trimming_a_large_public_sample_does_not_retain_its_prefix_buffer() {
    let mut text = "界".repeat(4096) + "слово";
    remove_prefix(&mut text, 4096 * '界'.len_utf8());
    assert_eq!(text, "слово");
    assert!(text.capacity() <= MAX_CONTEXT_CHARACTERS * 4);
    let mut small = "ок привіт".to_owned();
    let capacity = small.capacity();
    remove_prefix(&mut small, "ок ".len());
    assert_eq!(small, "привіт");
    assert_eq!(small.capacity(), capacity);
}
