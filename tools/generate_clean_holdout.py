#!/usr/bin/env python3
"""Build a deterministic clean-boundary holdout outside the repository.

The holdout samples lowercase words from English and Ukrainian Wikipedia
corpora, while Upyr's model is trained on news corpora. The generated TSV is a
development artifact under `.cache`: it is not embedded in Upyr or committed.
"""

from __future__ import annotations

import argparse
import hashlib
import tarfile
import urllib.request
from pathlib import Path


SEED = b"upyr-clean-wikipedia-v1\0"
ENGLISH_LOWER = "qwertyuiop[]asdfghjkl;'zxcvbnm,./`\\"
UKRAINIAN_LOWER = "йцукенгшщзхїфівапролджєячсмитьбю.'ґ"
SOURCES = {
    "english": {
        "url": "https://downloads.wortschatz-leipzig.de/corpora/eng_wikipedia_2016_100K.tar.gz",
        "sha256": "04aa301072a612e0368f1a0abe5f6b011ab03df84961c29b80bd126883a5a6f0",
        "archive": "eng_wikipedia_2016_100K.tar.gz",
        "alphabet": frozenset("abcdefghijklmnopqrstuvwxyz'"),
    },
    "ukrainian": {
        "url": "https://downloads.wortschatz-leipzig.de/corpora/ukr_wikipedia_2021_100K.tar.gz",
        "sha256": "2de40d49fa110d645529d94b8e5deb5a430eb78b29db57ac8b21cc237f5fa548",
        "archive": "ukr_wikipedia_2021_100K.tar.gz",
        "alphabet": frozenset("абвгґдеєжзиіїйклмнопрстуфхцчшщьюя'"),
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
        "--per-language",
        type=int,
        default=10_000,
        help="sampled boundaries per language (default: 10000)",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(".cache/upyr-benchmarks/clean-wikipedia-v1.tsv"),
        help="generated TSV outside the repository",
    )
    args = parser.parse_args()
    if args.per_language < 1:
        parser.error("--per-language must be positive")
    return args


def file_sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def corpus_archive(language: str, cache_dir: Path) -> Path:
    source = SOURCES[language]
    path = cache_dir / str(source["archive"])
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + ".part")
        print(f"downloading {source['url']}")
        urllib.request.urlretrieve(str(source["url"]), temporary)
        temporary.replace(path)
    actual = file_sha256(path)
    if actual != source["sha256"]:
        raise SystemExit(
            f"unexpected SHA-256 for {path}: {actual} "
            f"(expected {source['sha256']})"
        )
    return path


def frequency_rows(archive: Path):
    with tarfile.open(archive, "r:gz") as bundle:
        members = [
            member for member in bundle.getmembers() if member.name.endswith("-words.txt")
        ]
        if len(members) != 1:
            raise SystemExit(
                f"expected one *-words.txt in {archive}, found {len(members)}"
            )
        stream = bundle.extractfile(members[0])
        if stream is None:
            raise SystemExit(f"cannot read {members[0].name} from {archive}")
        for raw_line in stream:
            try:
                _identifier, token, count = (
                    raw_line.decode("utf-8").rstrip("\n").split("\t")
                )
                yield token, int(count)
            except (UnicodeDecodeError, ValueError) as error:
                raise SystemExit(
                    f"invalid frequency row in {archive}: {raw_line!r}"
                ) from error


def normalized_token(raw_token: str, alphabet: frozenset[str]) -> str | None:
    token = raw_token.replace("’", "'")
    # Preserve lexical shape: case-folding names and acronyms would manufacture
    # lowercase inputs that did not occur in the held-out corpus.
    if token != token.casefold():
        return None
    if not 4 <= len(token) <= 32:
        return None
    if token[0] == "'" or token[-1] == "'" or "''" in token:
        return None
    return token if all(character in alphabet for character in token) else None


def sample_words(language: str, archive: Path, count: int) -> list[str]:
    alphabet = SOURCES[language]["alphabet"]
    candidates: dict[str, bytes] = {}
    for raw_token, frequency in frequency_rows(archive):
        if frequency < 2:
            continue
        token = normalized_token(raw_token, alphabet)
        if token is None:
            continue
        key = hashlib.sha256(SEED + language.encode() + b"\0" + token.encode()).digest()
        candidates[token] = key
    if len(candidates) < count:
        raise SystemExit(
            f"{archive} yielded only {len(candidates)} eligible {language} words"
        )
    return [
        token
        for token, _key in sorted(candidates.items(), key=lambda item: (item[1], item[0]))[
            :count
        ]
    ]


def ukrainian_to_physical(token: str) -> str:
    positions = {character: index for index, character in enumerate(UKRAINIAN_LOWER)}
    return "".join(
        ENGLISH_LOWER[positions[character]] if character in positions else character
        for character in token
    )


def write_holdout(path: Path, samples: dict[str, list[str]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="\n") as output:
        output.write("# upyr-clean-wikipedia-v1\n")
        output.write("# id\tsource_layout\tphysical_word\tobserved_word\n")
        for language in ("english", "ukrainian"):
            for index, word in enumerate(samples[language], start=1):
                physical = word if language == "english" else ukrainian_to_physical(word)
                output.write(
                    f"{language}-{index:05d}\t{language}\t{physical}\t{word}\n"
                )


def main() -> None:
    args = parse_args()
    samples = {
        language: sample_words(
            language,
            corpus_archive(language, args.cache_dir),
            args.per_language,
        )
        for language in ("english", "ukrainian")
    }
    write_holdout(args.output, samples)
    print(
        f"wrote {sum(map(len, samples.values())):,} clean boundaries to "
        f"{args.output} (SHA-256 {file_sha256(args.output)})"
    )


if __name__ == "__main__":
    main()
