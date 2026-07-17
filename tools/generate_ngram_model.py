#!/usr/bin/env python3
"""Build Upyr's compact EN/UK character n-gram language index.

The downloaded corpus word-frequency tables are training input only. The output
contains no word-frequency table or sentences: each record is a packed 2-5
character n-gram key plus a signed confidence byte (negative for English,
positive for Ukrainian). Short n-grams can coincide with complete short words.
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
MAX_STRENGTH = 127
GRAM_BUDGETS = {2: 2_048, 3: 8_192, 4: 16_384, 5: 24_576}
DEFAULT_CORPUS_SIZE = "1M"
DEFAULT_BUDGET_SCALE = 1.75
DEFAULT_MIN_STRENGTH = 32

LANGUAGES = {
    "english": {
        "alphabet": frozenset("abcdefghijklmnopqrstuvwxyz'"),
        "sign": -1,
    },
    "ukrainian": {
        "alphabet": frozenset("абвгґдеєжзиіїйклмнопрстуфхцчшщьюя'"),
        "sign": 1,
    },
}

SOURCES = {
    "100K": {
        "english": {
            "url": "https://downloads.wortschatz-leipzig.de/corpora/eng_news_2023_100K.tar.gz",
            "sha256": "8e65ed5b9c96687d293374335c14dfb9db4c150877bcc208a21bcb2f86b43484",
            "archive": "eng_news_2023_100K.tar.gz",
        },
        "ukrainian": {
            "url": "https://downloads.wortschatz-leipzig.de/corpora/ukr_news_2023_100K.tar.gz",
            "sha256": "c66f1245ab624885354b5f19bc66aaab71977322136fe0d2835befca5688d7e4",
            "archive": "ukr_news_2023_100K.tar.gz",
        },
    },
    "1M": {
        "english": {
            "url": "https://downloads.wortschatz-leipzig.de/corpora/eng_news_2023_1M.tar.gz",
            "sha256": "c8a5a5e72897aa5e367b0319c1884831c02aaf29bf81342de31ca1b1cc8f3e4c",
            "archive": "eng_news_2023_1M.tar.gz",
        },
        "ukrainian": {
            "url": "https://downloads.wortschatz-leipzig.de/corpora/ukr_news_2023_1M.tar.gz",
            "sha256": "0901bff8b3fdb3a8c657137754b4214b8ea6f241572d3ff9b2ae718487412383",
            "archive": "ukr_news_2023_1M.tar.gz",
        },
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
        "--corpus-size",
        choices=tuple(SOURCES),
        default=DEFAULT_CORPUS_SIZE,
        help=f"pinned Leipzig corpus size (default: {DEFAULT_CORPUS_SIZE})",
    )
    parser.add_argument(
        "--budget-scale",
        type=float,
        default=DEFAULT_BUDGET_SCALE,
        help=(
            "multiply each retained n-gram budget "
            f"(default: {DEFAULT_BUDGET_SCALE})"
        ),
    )
    parser.add_argument(
        "--min-strength",
        type=int,
        default=DEFAULT_MIN_STRENGTH,
        help=(
            "minimum retained n-gram strength "
            f"(default: {DEFAULT_MIN_STRENGTH})"
        ),
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
        default=Path("crates/upyr-core/assets/models/language.ngm"),
        help="generated packed model (default: crates/upyr-core/assets/models/language.ngm)",
    )
    args = parser.parse_args()
    if args.budget_scale <= 0.0:
        parser.error("--budget-scale must be greater than zero")
    if not 1 <= args.min_strength <= MAX_STRENGTH:
        parser.error(f"--min-strength must be between 1 and {MAX_STRENGTH}")
    return args


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def corpus_archive(
    language: str,
    source: dict[str, str],
    supplied: Path | None,
    cache_dir: Path,
) -> Path:
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


def quantized_scores(
    counts: dict[int, dict[int, int]],
    gram_budgets: dict[int, int],
    min_strength: int,
) -> dict[int, int]:
    scores: dict[int, int] = {}
    for size in range(MIN_NGRAM, MAX_NGRAM + 1):
        selected = sorted(
            counts[size].items(), key=lambda item: (-item[1], item[0])
        )[: gram_budgets[size]]
        if not selected:
            continue
        minimum = math.log1p(selected[-1][1])
        maximum = math.log1p(selected[0][1])
        span = maximum - minimum
        for key, count in selected:
            position = 1.0 if span == 0.0 else (math.log1p(count) - minimum) / span
            scores[key] = round(
                min_strength + position * (MAX_STRENGTH - min_strength)
            )
    return scores


def build_model(language_scores: dict[str, dict[int, int]]) -> list[tuple[int, int]]:
    signed: dict[int, int] = defaultdict(int)
    for language, scores in language_scores.items():
        sign = int(LANGUAGES[language]["sign"])
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
    sources = SOURCES[args.corpus_size]
    gram_budgets = {
        size: max(1, round(budget * args.budget_scale))
        for size, budget in GRAM_BUDGETS.items()
    }
    for language in ("english", "ukrainian"):
        source = sources[language]
        archive = corpus_archive(
            language, source, supplied[language], args.cache_dir
        )
        counts, word_types, word_tokens = count_ngrams(
            archive, LANGUAGES[language]["alphabet"]
        )
        scores[language] = quantized_scores(
            counts, gram_budgets, args.min_strength
        )
        print(
            f"{language}: {word_types:,} accepted word types, "
            f"{word_tokens:,} tokens, {len(scores[language]):,} retained n-grams"
        )

    entries = build_model(scores)
    write_model(args.output, entries)
    print(f"wrote {len(entries):,} language-tagged n-grams to {args.output}")


if __name__ == "__main__":
    main()
