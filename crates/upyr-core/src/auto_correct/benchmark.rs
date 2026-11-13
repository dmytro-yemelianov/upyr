//! Opt-in offline benchmark access. Excluded from normal application builds.
use super::*;

pub fn reset_caches() {
    TOKEN_CONTRIBUTION.with(|cache| *cache.borrow_mut() = Default::default());
    CONTEXT_SCORES.with(|cache| *cache.borrow_mut() = Default::default());
    contextual::reset_cache();
}

pub fn cache_len() -> usize {
    TOKEN_CONTRIBUTION.with(|cache| cache.borrow().len())
}

pub fn token(word: &str) -> (i64, i64, usize) {
    let contribution = SIGNED_NGRAM_V1.token_contribution(word);
    (contribution.raw, contribution.maximum, contribution.grams)
}

pub fn score(text: &str, ukrainian: bool) -> (u32, usize) {
    let language = if ukrainian {
        Language::Ukrainian
    } else {
        Language::English
    };
    let score = SIGNED_NGRAM_V1.score(language, text);
    (score.coverage.to_bits(), score.grams)
}
