# Third-party notices

## Punto Switcher rule data

The complete tables in `crates/upyr-core/assets/punto/` come from Punto Switcher
for Mac 1.0 (Yandex). They are third-party proprietary data and are not covered
by Upyr's MIT license. No redistribution license is asserted here. See the
[core notices](crates/upyr-core/THIRD_PARTY_NOTICES.md) and
[import evidence](docs/punto-early-rules.md) for provenance and scope.

## GlobeSwitch

The macOS flag-row indicator and pixel flag corpus are adapted from
[GlobeSwitch](https://github.com/dmytro-yemelianov/globe-switch), commit
`e018daf06e38094f497dd500e87dc72a22212f63`.
The corpus started from public-domain country flags and was finished by hand.
The language-to-region data records Foundation's likely-region expansion.
`tools/build_globe_flags.py` compiles the original grid into lossless PNGs.

MIT License

Copyright (c) 2026 Dmytro Yemelianov

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## issw

Upyr's macOS input-source binding contains code adapted from [issw](https://github.com/0xAndoroid/issw).

MIT License

Copyright (c) 2026 0xAndoroid

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## rdev

Upyr uses [rdev](https://github.com/Narsil/rdev) for physical key types and native
global key-down events on Windows and Linux/X11. macOS uses AppKit observation.

MIT License

Copyright (c) 2020 Nicolas Patry

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## pfxr / dyco-audio

Upyr's dependency-free procedural sound engine in `crates/upyr-audio` adapts
the DSP foundation from [`dyco-audio`](https://github.com/dmytro-yemelianov/dyco)
at commit `31917ec17637aa661676c297a903fff84c849dfd`, which in turn implements
pfxr/sfxr-style synthesis based on [pfxr](https://github.com/achtaitaipai/pfxr).

Copyright (c) 2026 Dmytro Yemelianov and dyco contributors

Copyright (c) 2025 Charles Cailleteau

MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Leipzig Corpora Collection

Upyr's generated character n-gram model in `crates/upyr-core/assets/models/language.ngm` is derived
from the following downloadable corpora from the Leipzig Corpora Collection:

- `eng_news_2023_1M` (English, 1,000,000 news sentences)
- `ukr_news_2023_1M` (Ukrainian, 1,000,000 news sentences)

Source: [Wortschatz Leipzig](https://wortschatz.uni-leipzig.de/en/download)
Provider terms: [Wortschatz Leipzig Terms of Usage](https://www.wortschatz.uni-leipzig.de/en/usage)

The downloadable text corpora are provided under the
[Creative Commons Attribution 4.0 International licence](https://creativecommons.org/licenses/by/4.0/).
The corpus provider requests this copyright notice:
© 2025 Universität Leipzig / Sächsische Akademie der Wissenschaften / InfAI.
Upyr transforms their frequency tables by filtering tokens to the English or
Ukrainian alphabet, extracting character 2–5-grams, aggregating corpus
frequencies, retaining the strongest signals, quantizing them, and assigning a
signed language confidence. The generated artifact contains no corpus
word-frequency table or sentences, only packed 2–5-character n-grams; some
short n-grams naturally coincide with complete short words. These changes are
not endorsed by the corpus authors.

## Physical-key prefix lexicon

`crates/upyr-core/assets/models/prefixes.lxp` is derived from the same CC BY 4.0 English
and Ukrainian Leipzig news corpora credited above, plus FreeBSD's Web2 English
word list at revision `4a65e12dfbaba7193e37fcd2e14fbc20776a78b9`.
The accompanying [`prefixes-web2-NOTICE.txt`](crates/upyr-core/assets/models/prefixes-web2-NOTICE.txt)
retains that snapshot's dictionary README and repository copyright terms.
The adjacent `prefixes.json` records pinned source URLs, input hashes,
normalization, physical-key mappings and the generated artifact hash.

This transformation case-folds and alphabet-filters word forms, maps Ukrainian
forms to physical EN keyboard positions, and minimizes a prefix automaton with
EN/UK continuation masks. Original sentences, frequencies and word-ending
markers are not included. These changes are not endorsed by the source authors.

## Short-word context index

`crates/upyr-core/assets/models/context.bgm` is derived from the same English
and Ukrainian Leipzig news corpora under CC BY 4.0 credited above. It contains
normalized word forms and aggregated adjacent-word counts with a short function
word on the left, not original sentences. One percent of sentence IDs is
excluded for evaluation. `context.json` records source hashes and transformation
rules; `tools/generate_context_model.py` reproduces the artifact. These changes
are not endorsed by the corpus authors.
