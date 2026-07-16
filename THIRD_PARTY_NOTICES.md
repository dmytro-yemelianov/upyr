# Third-party notices

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

Upyr uses [rdev](https://github.com/Narsil/rdev) for native global key-down
events on macOS, Windows, and Linux/X11.

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

## Leipzig Corpora Collection

Upyr's generated character n-gram model in `src/models/language.ngm` is derived
from the following downloadable corpora from the Leipzig Corpora Collection:

- `eng_news_2023_100K` (English, 100,000 news sentences)
- `ukr_news_2023_100K` (Ukrainian, 100,000 news sentences)

Source: [Wortschatz Leipzig](https://wortschatz.uni-leipzig.de/en/download)

The downloadable text corpora are provided under the
[Creative Commons Attribution 4.0 International licence](https://creativecommons.org/licenses/by/4.0/).
Upyr transforms their frequency tables by filtering tokens to the English or
Ukrainian alphabet, extracting character 2–5-grams, aggregating corpus
frequencies, retaining the strongest signals, quantizing them, and assigning a
signed language confidence. The generated artifact contains no complete corpus
words or sentences. These changes are not endorsed by the corpus authors.
