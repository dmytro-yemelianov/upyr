#!/usr/bin/env python3
"""Build Upyr's compact EN/UK character n-gram language index.

The downloaded corpus word-frequency tables are training input only. The output
contains no words or sentences: each record is a packed 2-5 character n-gram
key plus a signed confidence byte (negative for English, positive for
Ukrainian).
"""

from __future__ import annotations

import argparse
import hashlib
import math
import struct
import tarfile
import urllib.request
from collections import defaultdict
from pathlib import Path


MAGIC = b"UPYRLM1\0"
HEADER = struct.Struct("<8sI")
ENTRY_SIZE = 17
MIN_NGRAM = 2
MAX_NGRAM = 5
MIN_STRENGTH = 24
MAX_STRENGTH = 127
GRAM_BUDGETS = {2: 2_048, 3: 8_192, 4: 16_384, 5: 24_576}

SOURCES = {
    "english": {
        "url": "https://downloads.wortschatz-leipzig.de/corpora/eng_news_2023_100K.tar.gz",
        "sha256": "8e65ed5b9c96687d293374335c14dfb9db4c150877bcc208a21bcb2f86b43484",
        "archive": "eng_news_2023_100K.tar.gz",
        "alphabet": frozenset("abcdefghijklmnopqrstuvwxyz'"),
        "sign": -1,
    },
    "ukrainian": {
        "url": "https://downloads.wortschatz-leipzig.de/corpora/ukr_news_2023_100K.tar.gz",
        "sha256": "c66f1245ab624885354b5f19bc66aaab71977322136fe0d2835befca5688d7e4",
        "archive": "ukr_news_2023_100K.tar.gz",
        "alphabet": frozenset("абвгґдеєжзиіїйклмнопрстуфхцчшщьюя'"),
        "sign": 1,
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=Path(".cache/upyr-corpora"),
        help="download cache (default: .cache/upyr-corpora)",
    )
    parser.add_argument(
        "--english-archive",
        type=Path,
        help="use an existing English Leipzig archive",
    )
    parser.add_argument(
        "--ukrainian-archive",
        type=Path,
        help="use an existing Ukrainian Leipzig archive",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("src/models/language.ngm"),
        help="generated packed model (default: src/models/language.ngm)",
    )
    return parser.parse_args()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def corpus_archive(language: str, supplied: Path | None, cache_dir: Path) -> Path:
    source = SOURCES[language]
    path = supplied or cache_dir / str(source["archive"])
    if not path.exists():
        if supplied:
            raise SystemExit(f"missing {language} archive: {path}")
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + ".part")
        print(f"downloading {source['url']}")
        urllib.request.urlretrieve(str(source["url"]), temporary)
        temporary.replace(path)

    actual = sha256(path)
    if actual != source["sha256"]:
        raise SystemExit(
            f"unexpected SHA-256 for {path}: {actual} (expected {source['sha256']})"
        )
    return path


def frequency_rows(archive: Path):
    with tarfile.open(archive, "r:gz") as bundle:
        members = [member for member in bundle.getmembers() if member.name.endswith("-words.txt")]
        if len(members) != 1:
            raise SystemExit(f"expected one *-words.txt in {archive}, found {len(members)}")
        stream = bundle.extractfile(members[0])
        if stream is None:
            raise SystemExit(f"cannot read {members[0].name} from {archive}")
        for raw_line in stream:
            try:
                _identifier, token, count = raw_line.decode("utf-8").rstrip("\n").split("\t")
                yield token, int(count)
            except (UnicodeDecodeError, ValueError) as error:
                raise SystemExit(f"invalid frequency row in {archive}: {raw_line!r}") from error


def normalized_tokens(raw_token: str, alphabet: frozenset[str]):
    normalized = raw_token.casefold().replace("’", "'").replace("‐", "-").replace("–", "-")
    for token in normalized.split("-"):
        if not 2 <= len(token) <= 48:
            continue
        if token[0] == "'" or token[-1] == "'" or "''" in token:
            continue
        if all(character in alphabet for character in token):
            yield token


def ngram_key(characters: str) -> int:
    key = len(characters)
    for character in characters:
        key = (key << 21) | ord(character)
    return key


def token_ngrams(token: str):
    bordered = f"^{token}$"
    for size in range(MIN_NGRAM, MAX_NGRAM + 1):
        for start in range(len(bordered) - size + 1):
            yield size, ngram_key(bordered[start : start + size])


def count_ngrams(archive: Path, alphabet: frozenset[str]):
    counts: dict[int, dict[int, int]] = {
        size: defaultdict(int) for size in range(MIN_NGRAM, MAX_NGRAM + 1)
    }
    accepted_types = 0
    accepted_tokens = 0
    for raw_token, frequency in frequency_rows(archive):
        for token in normalized_tokens(raw_token, alphabet):
            accepted_types += 1
            accepted_tokens += frequency
            for size, key in token_ngrams(token):
                counts[size][key] += frequency
    return counts, accepted_types, accepted_tokens


def quantized_scores(counts: dict[int, dict[int, int]]) -> dict[int, int]:
    scores: dict[int, int] = {}
    for size in range(MIN_NGRAM, MAX_NGRAM + 1):
        selected = sorted(
            counts[size].items(), key=lambda item: (-item[1], item[0])
        )[: GRAM_BUDGETS[size]]
        if not selected:
            continue
        minimum = math.log1p(selected[-1][1])
        maximum = math.log1p(selected[0][1])
        span = maximum - minimum
        for key, count in selected:
            position = 1.0 if span == 0.0 else (math.log1p(count) - minimum) / span
            scores[key] = round(MIN_STRENGTH + position * (MAX_STRENGTH - MIN_STRENGTH))
    return scores


def build_model(language_scores: dict[str, dict[int, int]]) -> list[tuple[int, int]]:
    signed: dict[int, int] = defaultdict(int)
    for language, scores in language_scores.items():
        sign = int(SOURCES[language]["sign"])
        for key, strength in scores.items():
            signed[key] += sign * strength
    return sorted(
        (key, max(-MAX_STRENGTH, min(MAX_STRENGTH, score)))
        for key, score in signed.items()
        if score != 0
    )


def write_model(path: Path, entries: list[tuple[int, int]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("wb") as output:
        output.write(HEADER.pack(MAGIC, len(entries)))
        for key, score in entries:
            output.write(key.to_bytes(16, "little"))
            output.write(struct.pack("b", score))
    expected = HEADER.size + len(entries) * ENTRY_SIZE
    if path.stat().st_size != expected:
        raise SystemExit(f"generated model has unexpected size: {path}")


def main() -> None:
    args = parse_args()
    supplied = {
        "english": args.english_archive,
        "ukrainian": args.ukrainian_archive,
    }
    scores: dict[str, dict[int, int]] = {}
    for language in ("english", "ukrainian"):
        source = SOURCES[language]
        archive = corpus_archive(language, supplied[language], args.cache_dir)
        counts, word_types, word_tokens = count_ngrams(archive, source["alphabet"])
        scores[language] = quantized_scores(counts)
        print(
            f"{language}: {word_types:,} accepted word types, "
            f"{word_tokens:,} tokens, {len(scores[language]):,} retained n-grams"
        )

    entries = build_model(scores)
    write_model(args.output, entries)
    print(f"wrote {len(entries):,} language-tagged n-grams to {args.output}")


if __name__ == "__main__":
    main()
