# Third-party notices

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
