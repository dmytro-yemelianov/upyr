use std::collections::{BTreeMap, BTreeSet};

use super::*;

const REPLAY_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/fixtures/boundary-replay-v1.tsv"
));
const CATEGORIES: &[&str] = &[
    "ambiguous-abstain",
    "contextual",
    "native-english",
    "native-ukrainian",
    "proper-names",
    "punctuation",
    "reported-regressions",
    "technical-native",
    "technical-wrong-layout",
    "wrong-english-to-ukrainian",
    "wrong-ukrainian-to-english",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedAction {
    Correct,
    Keep,
}

#[derive(Debug)]
struct ReplayCase {
    id: String,
    group: String,
    category: String,
    split: String,
    tags: Vec<String>,
    mapping_profile: String,
    source_layout: InputLayout,
    physical_word: String,
    physical_context: String,
    observed_context: String,
    intended_context: String,
    expected_action: ExpectedAction,
    expected_direction: Option<Direction>,
}

#[derive(Debug, Default, Clone)]
struct Metrics {
    total: usize,
    expected_corrections: usize,
    expected_keeps: usize,
    true_corrections: usize,
    missed_corrections: usize,
    false_corrections: usize,
    wrong_corrections: usize,
    kept: usize,
    resets: usize,
    continues: usize,
}

impl Metrics {
    fn record(
        &mut self,
        expected: ExpectedAction,
        decision: &AutoDecision,
        correction_matches: bool,
    ) {
        self.total += 1;
        match expected {
            ExpectedAction::Correct => {
                self.expected_corrections += 1;
                match decision {
                    AutoDecision::Correct(_) if correction_matches => self.true_corrections += 1,
                    AutoDecision::Correct(_) => {
                        self.wrong_corrections += 1;
                        self.missed_corrections += 1;
                    }
                    AutoDecision::Reset => {
                        self.resets += 1;
                        self.missed_corrections += 1;
                    }
                    AutoDecision::Continue => {
                        self.continues += 1;
                        self.missed_corrections += 1;
                    }
                }
            }
            ExpectedAction::Keep => {
                self.expected_keeps += 1;
                match decision {
                    AutoDecision::Correct(_) => self.false_corrections += 1,
                    AutoDecision::Reset => {
                        self.resets += 1;
                        self.kept += 1;
                    }
                    AutoDecision::Continue => {
                        self.continues += 1;
                        self.kept += 1;
                    }
                }
            }
        }
    }

    fn precision(&self) -> f64 {
        ratio(
            self.true_corrections,
            self.true_corrections + self.false_corrections + self.wrong_corrections,
        )
    }

    fn recall(&self) -> f64 {
        ratio(self.true_corrections, self.expected_corrections)
    }

    fn false_corrections_per_10k(&self) -> f64 {
        ratio(self.false_corrections * 10_000, self.expected_keeps)
    }
}

fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn parse_cases() -> Vec<ReplayCase> {
    let mut cases = Vec::new();
    let mut ids = BTreeSet::new();

    for (line_index, line) in REPLAY_V1.lines().enumerate() {
        let line_number = line_index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(
            fields.len(),
            13,
            "replay corpus line {line_number} must have exactly thirteen tab-separated fields"
        );

        let id = fields[0].to_owned();
        assert!(!id.is_empty(), "replay corpus line {line_number} has no id");
        assert!(
            ids.insert(id.clone()),
            "duplicate replay corpus id {id:?} on line {line_number}"
        );
        assert!(
            CATEGORIES.contains(&fields[2]),
            "unknown category {:?} on line {line_number}",
            fields[2]
        );
        assert!(
            matches!(fields[3], "calibration" | "evaluation" | "regression"),
            "unknown split {:?} on line {line_number}",
            fields[3]
        );
        assert!(
            matches!(fields[5], "builtin-uk-v1" | "reported-user-layout-v1"),
            "unknown mapping profile {:?} on line {line_number}",
            fields[5]
        );

        let expected_action = match fields[11] {
            "correct" => ExpectedAction::Correct,
            "keep" => ExpectedAction::Keep,
            other => panic!("unknown expected action {other:?} on line {line_number}"),
        };
        let expected_direction = match fields[12] {
            "english-to-ukrainian" => Some(Direction::EnglishToUkrainian),
            "ukrainian-to-english" => Some(Direction::UkrainianToEnglish),
            "-" => None,
            other => panic!("unknown expected direction {other:?} on line {line_number}"),
        };
        assert_eq!(
            expected_direction.is_some(),
            expected_action == ExpectedAction::Correct,
            "direction/action mismatch for {id:?}"
        );

        let physical_word = expand_markers(fields[7]);
        let physical_context = expand_markers(fields[8]);
        let observed_context = expand_markers(fields[9]);
        let intended_context = expand_markers(fields[10]);
        for (label, context) in [
            ("physical", &physical_context),
            ("observed", &observed_context),
            ("intended", &intended_context),
        ] {
            assert!(
                context.ends_with(' '),
                "{label} context for {id:?} must end at a word boundary"
            );
        }
        assert!(
            physical_context.chars().count() <= MAX_CONTEXT_CHARACTERS,
            "physical context for {id:?} exceeds the tracker limit"
        );
        assert_eq!(
            final_word(&physical_context),
            physical_word,
            "physical word/context mismatch for {id:?}"
        );

        let source_layout = parse_layout(fields[6], line_number);
        if let Some(direction) = expected_direction {
            assert_eq!(
                direction,
                match source_layout {
                    InputLayout::English => Direction::EnglishToUkrainian,
                    InputLayout::Ukrainian => Direction::UkrainianToEnglish,
                },
                "source layout/direction mismatch for {id:?}"
            );
        }

        let group = fields[1].to_owned();
        assert!(!group.is_empty(), "replay case {id:?} has no lexical group");
        let tags = fields[4].split(',').map(str::to_owned).collect::<Vec<_>>();
        assert!(
            !tags.is_empty() && tags.iter().all(|tag| !tag.is_empty()),
            "replay case {id:?} has invalid tags"
        );

        cases.push(ReplayCase {
            id,
            group,
            category: fields[2].to_owned(),
            split: fields[3].to_owned(),
            tags,
            mapping_profile: fields[5].to_owned(),
            source_layout,
            physical_word,
            physical_context,
            observed_context,
            intended_context,
            expected_action,
            expected_direction,
        });
    }

    assert_group_splits_are_stable(&cases);
    cases
}

fn assert_group_splits_are_stable(cases: &[ReplayCase]) {
    let mut splits = BTreeMap::<&str, &str>::new();
    for case in cases {
        if let Some(previous) = splits.insert(&case.group, &case.split) {
            assert_eq!(
                previous, case.split,
                "lexical group {:?} crosses benchmark splits",
                case.group
            );
        }
    }
}

fn parse_layout(value: &str, line_number: usize) -> InputLayout {
    match value {
        "english" => InputLayout::English,
        "ukrainian" => InputLayout::Ukrainian,
        other => panic!("unknown layout {other:?} on line {line_number}"),
    }
}

fn expand_markers(value: &str) -> String {
    value.replace('␠', " ")
}

fn final_word(physical_context: &str) -> String {
    physical_context
        .trim_end_matches(' ')
        .rsplit(' ')
        .next()
        .expect("validated replay context has a final word")
        .to_owned()
}

fn mapping_for(profile: &str) -> Option<Vec<(char, char)>> {
    match profile {
        "builtin-uk-v1" => None,
        "reported-user-layout-v1" => {
            const ENGLISH_LOWER: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`\\";
            const REPORTED_UKRAINIAN_LOWER: &str = "йцукенгшщзхїфівапролджєячсмитьбю.'ʼ";
            const ENGLISH_UPPER: &str = "QWERTYUIOP{}ASDFGHJKL:\"ZXCVBNM<>?~|";
            const REPORTED_UKRAINIAN_UPPER: &str = "ЙЦУКЕНГШЩЗХЇФІВАПРОЛДЖЄЯЧСМИТЬБЮ,₴ʼ";
            Some(
                ENGLISH_LOWER
                    .chars()
                    .zip(REPORTED_UKRAINIAN_LOWER.chars())
                    .chain(ENGLISH_UPPER.chars().zip(REPORTED_UKRAINIAN_UPPER.chars()))
                    .collect(),
            )
        }
        _ => unreachable!("mapping profile was validated while parsing"),
    }
}

fn print_dimension(label: &str, values: &BTreeMap<String, Metrics>) {
    println!("{label}\tcases\ttp/positive\tfp/negative\twrong\tprecision\trecall");
    for (value, metrics) in values {
        println!(
            "{}\t{}\t{}/{}\t{}/{}\t{}\t{:.3}\t{:.3}",
            value,
            metrics.total,
            metrics.true_corrections,
            metrics.expected_corrections,
            metrics.false_corrections,
            metrics.expected_keeps,
            metrics.wrong_corrections,
            metrics.precision(),
            metrics.recall(),
        );
    }
}

struct ReplayReport {
    overall: Metrics,
    by_category: BTreeMap<String, Metrics>,
    by_split: BTreeMap<String, Metrics>,
    by_tag: BTreeMap<String, Metrics>,
    by_direction: BTreeMap<String, Metrics>,
    misses: Vec<String>,
}

impl ReplayReport {
    fn print(&self, model: &str) {
        println!(
            "{model}: total={} correct={}/{} false-corrections={}/{} wrong-corrections={} precision={:.3} recall={:.3} false-corrections/10k={:.1} reset={} continue={}",
            self.overall.total,
            self.overall.true_corrections,
            self.overall.expected_corrections,
            self.overall.false_corrections,
            self.overall.expected_keeps,
            self.overall.wrong_corrections,
            self.overall.precision(),
            self.overall.recall(),
            self.overall.false_corrections_per_10k(),
            self.overall.resets,
            self.overall.continues,
        );
        print_dimension("category", &self.by_category);
        print_dimension("split", &self.by_split);
        print_dimension("direction", &self.by_direction);
        print_dimension("tag", &self.by_tag);
        if !self.misses.is_empty() {
            println!("misses:\n{}", self.misses.join("\n"));
        }
    }

    fn assert_v1_guardrails(&self) {
        // Monotonic guardrails around the documented signed N-gram v1
        // baseline. A challenger may improve recall, but it must not add
        // unsafe replacements.
        assert_eq!(self.overall.total, 191);
        assert_eq!(self.overall.false_corrections, 0);
        assert_eq!(self.overall.wrong_corrections, 0);
        assert!(self.overall.true_corrections >= 90);

        assert!(self.by_direction["english-to-ukrainian"].true_corrections >= 41);
        assert!(self.by_direction["ukrainian-to-english"].true_corrections >= 49);
        assert!(self.by_split["calibration"].true_corrections >= 21);
        assert!(self.by_split["evaluation"].true_corrections >= 52);
        assert!(self.by_split["regression"].true_corrections >= 17);
        assert_eq!(self.by_split["regression"].false_corrections, 0);
        assert!(self.by_category["technical-wrong-layout"].true_corrections >= 14);
    }
}

fn run_replay<S: CandidateScorer + ?Sized>(scorer: &S) -> ReplayReport {
    let cases = parse_cases();
    let mut overall = Metrics::default();
    let mut by_category = BTreeMap::<String, Metrics>::new();
    let mut by_split = BTreeMap::<String, Metrics>::new();
    let mut by_tag = BTreeMap::<String, Metrics>::new();
    let mut by_direction = BTreeMap::<String, Metrics>::new();
    let mut misses = Vec::new();
    let policy = AutoCorrectPolicy::default();

    for case in &cases {
        let mapping = mapping_for(&case.mapping_profile);
        let sample = WordSample::new(
            &case.physical_word,
            &case.physical_context,
            case.source_layout,
        );
        let decision = evaluate_with_scorer(&sample, &policy, mapping.as_deref(), scorer);
        let correction_matches = matches!(
            &decision,
            AutoDecision::Correct(correction)
                if correction.expected_source == case.observed_context
                    && correction.replacement == case.intended_context
                    && Some(correction.direction) == case.expected_direction
        );

        let record = |metrics: &mut Metrics| {
            metrics.record(case.expected_action, &decision, correction_matches);
        };
        record(&mut overall);
        record(by_category.entry(case.category.clone()).or_default());
        record(by_split.entry(case.split.clone()).or_default());
        for tag in &case.tags {
            record(by_tag.entry(tag.clone()).or_default());
        }
        let direction = match case.expected_direction {
            Some(Direction::EnglishToUkrainian) => "english-to-ukrainian",
            Some(Direction::UkrainianToEnglish) => "ukrainian-to-english",
            Some(Direction::Smart) => unreachable!("replay directions are explicit"),
            None => "keep",
        };
        record(by_direction.entry(direction.to_owned()).or_default());

        let met_expectation = match case.expected_action {
            ExpectedAction::Correct => correction_matches,
            ExpectedAction::Keep => !matches!(decision, AutoDecision::Correct(_)),
        };
        if !met_expectation {
            misses.push(format!(
                "{} [{} / {}]: expected {:?} {:?} -> {:?}, got {:?}",
                case.id,
                case.split,
                case.category,
                case.expected_action,
                case.observed_context,
                case.intended_context,
                decision,
            ));
        }
    }

    ReplayReport {
        overall,
        by_category,
        by_split,
        by_tag,
        by_direction,
        misses,
    }
}

#[test]
fn signed_ngram_v1_boundary_replay() {
    let report = run_replay(&SIGNED_NGRAM_V1);
    report.print("signed-ngram-v1 / boundary-replay-v1");
    report.assert_v1_guardrails();
}

#[test]
#[ignore = "set UPYR_CANDIDATE_MODEL to a generated .ngm artifact"]
fn candidate_ngram_boundary_replay() {
    let path = configured_path("UPYR_CANDIDATE_MODEL");
    let bytes = std::fs::read(&path).expect("read candidate model");
    let scorer = NgramModel { bytes: &bytes };
    assert!(
        scorer.is_valid(),
        "invalid candidate model at {}",
        path.display()
    );

    let report = run_replay(&scorer);
    report.print(&format!(
        "candidate {} / boundary-replay-v1",
        path.display()
    ));
    report.assert_v1_guardrails();
}

fn configured_path(variable: &str) -> std::path::PathBuf {
    let configured = std::env::var(variable)
        .unwrap_or_else(|_| panic!("{variable} must point to an evaluation artifact"));
    let path = std::path::PathBuf::from(&configured);
    if path.is_absolute() {
        path
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(path)
    }
}

fn run_external_clean_holdout<S: CandidateScorer + ?Sized>(scorer: &S) -> (usize, usize) {
    let path = configured_path("UPYR_CLEAN_HOLDOUT");
    let contents = std::fs::read_to_string(&path).expect("read clean holdout");
    let mut ids = BTreeSet::new();
    let mut total = 0usize;
    let mut english = 0usize;
    let mut ukrainian = 0usize;
    let mut resets = 0usize;
    let mut continues = 0usize;
    let mut false_corrections = Vec::new();
    let policy = AutoCorrectPolicy::default();

    for (line_index, line) in contents.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line_number = line_index + 1;
        let fields = line.split('\t').collect::<Vec<_>>();
        assert_eq!(
            fields.len(),
            4,
            "clean holdout line {line_number} must have four fields"
        );
        assert!(
            ids.insert(fields[0]),
            "duplicate clean holdout id {:?}",
            fields[0]
        );
        assert!(
            !fields[2].is_empty() && !fields[2].contains(char::is_whitespace),
            "invalid physical word on clean holdout line {line_number}"
        );
        let source_layout = parse_layout(fields[1], line_number);
        match source_layout {
            InputLayout::English => english += 1,
            InputLayout::Ukrainian => ukrainian += 1,
        }
        let context = format!("{} ", fields[2]);
        let sample = WordSample::new(fields[2], context, source_layout);
        let decision = evaluate_with_scorer(&sample, &policy, None, scorer);
        match decision {
            AutoDecision::Correct(correction) => {
                let candidates = Candidates::new(&sample, None);
                let context_evidence = scorer.compare(candidates.context_pair());
                let source_word = normalize_word(&candidates.source_word);
                let target_word = normalize_word(&candidates.target_word);
                let word_evidence =
                    scorer.compare(candidates.text_pair(&source_word, &target_word));
                false_corrections.push(format!(
                    "{}: native {:?} {:?}, correction {:?}, context={:.3}/{:.3} word={:.3}/{:.3} known={}/{}",
                    fields[0],
                    source_layout,
                    fields[3],
                    correction,
                    context_evidence.source.coverage,
                    context_evidence.target.coverage,
                    word_evidence.source.coverage,
                    word_evidence.target.coverage,
                    known(candidates.source_language, &source_word),
                    known(candidates.target_language, &target_word),
                ));
            }
            AutoDecision::Reset => resets += 1,
            AutoDecision::Continue => continues += 1,
        }
        total += 1;
    }

    println!(
        "clean holdout {}: total={total} english={english} ukrainian={ukrainian} false-corrections={} false-corrections/10k={:.1} reset={resets} continue={continues}",
        path.display(),
        false_corrections.len(),
        ratio(false_corrections.len() * 10_000, total),
    );
    if !false_corrections.is_empty() {
        println!(
            "false corrections (first 50):\n{}",
            false_corrections
                .iter()
                .take(50)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    assert!(total > 0, "clean holdout is empty");
    (total, false_corrections.len())
}

#[test]
#[ignore = "set UPYR_CLEAN_HOLDOUT to a generated clean-boundary TSV"]
fn signed_ngram_v1_external_clean_holdout() {
    let (total, false_corrections) = run_external_clean_holdout(&SIGNED_NGRAM_V1);
    assert_eq!(total, 20_000);
    assert_eq!(false_corrections, 0);
}

#[test]
#[ignore = "set UPYR_CLEAN_HOLDOUT and UPYR_CANDIDATE_MODEL"]
fn candidate_ngram_external_clean_holdout() {
    let path = configured_path("UPYR_CANDIDATE_MODEL");
    let bytes = std::fs::read(&path).expect("read candidate model");
    let scorer = NgramModel { bytes: &bytes };
    assert!(
        scorer.is_valid(),
        "invalid candidate model at {}",
        path.display()
    );

    let (total, false_corrections) = run_external_clean_holdout(&scorer);
    assert_eq!(total, 20_000);
    assert_eq!(false_corrections, 0);
}
