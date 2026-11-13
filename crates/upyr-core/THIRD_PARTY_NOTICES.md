# Third-party notices

## Punto Switcher rule data

`assets/punto/ps.txt` and `assets/punto/triggers.txt` are complete UTF-8
transcriptions of the rule tables shipped with Punto Switcher for Windows 4.4.2.334
(Yandex). They are third-party proprietary data, not covered by Upyr's MIT
license. No redistribution license is asserted here. The adjacent manifest
records the archived source, input hashes and decoding procedure. Upyr's Rust
matcher and importer were written separately; no Punto executable code is used.

## Leipzig Corpora Collection

Upyr's generated character n-gram model in `assets/models/language.ngm` is
derived from the following downloadable corpora from the Leipzig Corpora
Collection:

- `eng_news_2023_1M` (English, 1,000,000 news sentences)
- `ukr_news_2023_1M` (Ukrainian, 1,000,000 news sentences)

Source: [Wortschatz Leipzig](https://wortschatz.uni-leipzig.de/en/download)

Provider terms:
[Wortschatz Leipzig Terms of Usage](https://www.wortschatz.uni-leipzig.de/en/usage)

The downloadable text corpora are provided under the
[Creative Commons Attribution 4.0 International licence](https://creativecommons.org/licenses/by/4.0/).
The corpus provider requests this copyright notice:
Copyright 2025 Universitat Leipzig / Sachsische Akademie der Wissenschaften /
InfAI.

Upyr transforms their frequency tables by filtering tokens to the English or
Ukrainian alphabet, extracting character 2-5-grams, aggregating corpus
frequencies, retaining the strongest signals, quantizing them, and assigning a
signed language confidence. The generated artifact contains no corpus
word-frequency table or sentences, only packed 2-5-character n-grams; some
short n-grams naturally coincide with complete short words. These changes are
not endorsed by the corpus authors.

## Physical-key prefix lexicon

`assets/models/prefixes.lxp` is derived from the same CC BY 4.0 English
and Ukrainian Leipzig news corpora credited above, plus FreeBSD's Web2 English
word list at revision `4a65e12dfbaba7193e37fcd2e14fbc20776a78b9`.
The accompanying [`prefixes-web2-NOTICE.txt`](assets/models/prefixes-web2-NOTICE.txt)
retains that snapshot's dictionary README and repository copyright terms.
The adjacent `prefixes.json` records pinned source URLs, input hashes,
normalization, physical-key mappings and the generated artifact hash.

This transformation case-folds and alphabet-filters word forms, maps Ukrainian
forms to physical EN keyboard positions, and minimizes a prefix automaton with
EN/UK continuation masks. Original sentences, frequencies and word-ending
markers are not included. These changes are not endorsed by the source authors.

## Short-word context index

`assets/models/context.bgm` is derived from the same English and Ukrainian
Leipzig news corpora under CC BY 4.0 credited above. It contains normalized
word forms and aggregated adjacent-word counts with a short function word on
the left, not original sentences. One percent of sentence IDs is excluded for
evaluation. `context.json` records source hashes and transformation rules.
These changes are not endorsed by the corpus authors.
