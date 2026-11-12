//! Reuse normalized tokens and exact prefix sums for bounded completed contexts.
use super::{Language, NgramModel, TokenContribution, normalize_language_token};

const MAX_BYTES: usize = 1024;
const MAX_TOKENS: usize = 128;

#[derive(Default)]
struct Token {
    // Byte range in ScoreState::text; that text stays intact until all tokens are compared.
    raw: std::ops::Range<usize>,
    value: TokenContribution,
    prefix: TokenContribution,
}
#[derive(Default)]
struct ScoreState {
    text: String,
    tokens: Vec<Token>,
    total: TokenContribution,
}

#[derive(Default)]
pub(super) struct ContextScores {
    states: [ScoreState; 4],
}

impl ContextScores {
    pub(super) fn contribution(
        &mut self,
        model: &NgramModel<'_>,
        language: Language,
        text: &str,
    ) -> TokenContribution {
        if text.len() > MAX_BYTES {
            return model.text_contribution(text);
        }
        let first = usize::from(language == Language::Ukrainian) * 2;
        if let Some(state) = self.states[first..first + 2]
            .iter()
            .find(|state| state.text == text)
        {
            return state.total;
        }
        let count = text.split_whitespace().count();
        if count > MAX_TOKENS {
            return model.text_contribution(text);
        }
        // Word-only/name checks must not evict the source/target context state.
        let index = first + usize::from(count > 1);
        self.states[index].contribution(model, text, count)
    }
}
impl ScoreState {
    fn contribution(
        &mut self,
        model: &NgramModel<'_>,
        text: &str,
        count: usize,
    ) -> TokenContribution {
        let mut total = TokenContribution::default();
        let mut prefix_unchanged = true;
        for (index, raw) in text.split_whitespace().enumerate() {
            if index == self.tokens.len() {
                self.tokens.push(Token::default());
            }
            let token = &mut self.tokens[index];
            if self.text.get(token.raw.clone()) != Some(raw) {
                prefix_unchanged = false;
                let normalized = normalize_language_token(raw);
                token.value = if normalized.is_empty() {
                    TokenContribution::default()
                } else {
                    model.token_contribution(&normalized)
                };
            }
            // split_whitespace returns subslices of text, so this byte offset
            // is within its allocation and on UTF-8 character boundaries.
            let start = raw.as_ptr() as usize - text.as_ptr() as usize;
            token.raw = start..start + raw.len();
            if prefix_unchanged {
                total = token.prefix;
            } else {
                total.raw += token.value.raw;
                total.maximum += token.value.maximum;
                total.grams += token.value.grams;
                token.prefix = total;
            }
        }
        self.tokens.truncate(count);
        self.text.clear();
        self.text.push_str(text);
        self.total = total;
        total
    }
}

#[cfg(test)]
mod tests {
    use super::super::{LANGUAGE_MODEL, SIGNED_NGRAM_V1};
    use super::*;
    #[test]
    fn prefix_totals_match_stateless_normalization_through_edits() {
        let mut cache = ContextScores::default();
        for text in [
            "",
            "we",
            "we can",
            "we can continue ",
            "we can",
            "we can work",
            "we CAN work!",
            "we can work",
            "work",
            "we can work today",
            "we  can\twork\n!!!",
            "ІЇЄҐ п’ять пʼять",
            "ΟΣ ΟΣΑ Σ-Σ",
            "İ ǅ 123",
            "we can work today",
            "",
        ] {
            for language in [Language::English, Language::Ukrainian] {
                assert_eq!(
                    cache.contribution(&SIGNED_NGRAM_V1, language, text),
                    SIGNED_NGRAM_V1.text_contribution(text),
                    "{text}"
                );
                let sign = if language == Language::English { -1 } else { 1 };
                let reference = SIGNED_NGRAM_V1.text_contribution(text);
                let expected = if reference.maximum == 0 {
                    0.0
                } else {
                    (reference.raw * sign) as f32 / reference.maximum as f32
                };
                assert_eq!(
                    SIGNED_NGRAM_V1.score(language, text).coverage.to_bits(),
                    expected.to_bits()
                );
            }
        }
        for text in ["a ".repeat(129), "x".repeat(1025), "слово ".repeat(100)] {
            assert_eq!(
                cache.contribution(&SIGNED_NGRAM_V1, Language::English, &text),
                SIGNED_NGRAM_V1.text_contribution(&text)
            );
        }
    }
    #[test]
    fn arbitrary_token_edits_preserve_both_language_totals() {
        let vocabulary = [
            "we",
            "ІЇЄҐ",
            "ΟΣ",
            "Σ-Σ",
            "İ",
            "123",
            "!!!",
            "a’b",
            "пʼять",
            "a",
            "continue",
        ];
        let mut cache = ContextScores::default();
        let mut tokens = Vec::new();
        let mut seed = 37_u64;
        for step in 0..2000 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let index = (seed >> 32) as usize;
            match index % 4 {
                0 if !tokens.is_empty() => {
                    tokens.remove(index % tokens.len());
                }
                1 if !tokens.is_empty() => {
                    let len = tokens.len();
                    tokens[index % len] = vocabulary[index % vocabulary.len()];
                }
                2 => tokens.reverse(),
                _ => {
                    if tokens.len() < 20 {
                        tokens.push(vocabulary[index % vocabulary.len()]);
                    }
                }
            }
            let text = tokens.join(if step % 2 == 0 { " " } else { "\t  " });
            for language in [Language::English, Language::Ukrainian] {
                let expected = SIGNED_NGRAM_V1.text_contribution(&text);
                assert_eq!(
                    cache.contribution(&SIGNED_NGRAM_V1, language, &text),
                    expected,
                    "step {step}: {text}"
                );
                assert_eq!(
                    cache.contribution(&SIGNED_NGRAM_V1, language, &text),
                    expected
                );
            }
        }
    }
    #[test]
    fn alternate_models_bypass_both_production_caches() {
        let mut bytes = LANGUAGE_MODEL.to_vec();
        for entry in bytes[12..].chunks_exact_mut(17) {
            entry[16] = (-(entry[16] as i8)) as u8;
        }
        let alternate = NgramModel { bytes: &bytes };
        let text = "we can continue працювати";
        let original = SIGNED_NGRAM_V1.score(Language::English, text);
        let changed = alternate.score(Language::English, text);
        assert_eq!(original.coverage, -changed.coverage);
        assert_eq!(original.grams, changed.grams);
        assert_eq!(
            original.coverage.to_bits(),
            SIGNED_NGRAM_V1
                .score(Language::English, text)
                .coverage
                .to_bits()
        );
    }
}
