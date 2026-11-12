#!/usr/bin/env python3
"""Compile short-word bigrams from pinned Leipzig sentences (no runtime training)."""
from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import struct
import tarfile

if __package__:
    from .generate_ngram_model import LANGUAGES, SOURCES, corpus_archive
else:
    from generate_ngram_model import LANGUAGES, SOURCES, corpus_archive

MAGIC = b"UPYRBG1\0"
HEADER = struct.Struct("<8sIII")
PAIR = struct.Struct("<QI")
MIN_COUNT = 8
SHORT_WORDS = Path(__file__).resolve().parents[1] / "crates/upyr-core/assets/context-short-words.txt"
TOKEN = re.compile(r"[^\W\d_]+(?:['’ʼ][^\W\d_]+)*", re.UNICODE)


def short_words(path=SHORT_WORDS):
    return {parts[0]: set(parts[1:]) for line in path.read_text().splitlines()
            if (parts := line.split()) and not line.startswith("#")}


def pairs(sentence, language):
    alphabet = LANGUAGES[language]["alphabet"]
    previous = None
    for match in TOKEN.finditer(sentence):
        word = match.group().lower().replace("’", "'").replace("ʼ", "'")
        valid = 1 <= len(word) <= 48 and all(c in alphabet for c in word)
        if valid and previous is not None:
            before, end, raw = previous
            gap = sentence[end:match.start()]
            if gap and gap.isspace():
                yield before, word, raw, match.group()
        previous = (word, match.end(), match.group()) if valid else None


def sentences(archive):
    with tarfile.open(archive, "r:gz") as bundle:
        members = [m for m in bundle.getmembers() if m.name.endswith("-sentences.txt")]
        if len(members) != 1:
            raise ValueError(f"expected one sentence file in {archive}")
        stream = bundle.extractfile(members[0])
        for line in stream:
            identifier, sentence = line.decode("utf-8").rstrip("\n").split("\t", 1)
            yield int(identifier), sentence


def encode(counts):
    retained = {pair: n for pair, n in counts.items() if n >= MIN_COUNT}
    words = sorted({w for pair in retained for w in pair})
    ids = {word: i for i, word in enumerate(words)}
    text = bytearray()
    offsets = bytearray()
    for word in words:
        offsets += struct.pack("<I", len(text))
        text += word.encode()
    offsets += struct.pack("<I", len(text))
    records = bytearray()
    for (left, right), count in sorted(retained.items()):
        records += PAIR.pack((ids[left] << 32) | ids[right], min(count, 0xffffffff))
    blob = HEADER.pack(MAGIC, len(words), len(retained), len(text)) + offsets + text + records
    return blob, {"words": len(words), "pairs": len(retained), "bytes": len(blob)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache-dir", type=Path, default=Path(".cache/upyr-corpora"))
    parser.add_argument("--output", type=Path, default=Path("crates/upyr-core/assets/models/context.bgm"))
    parser.add_argument("--holdout", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    allowed = short_words()
    counts = Counter()
    stats = {}
    held = []
    for language, source in SOURCES["1M"].items():
        archive = corpus_archive(language, source, None, args.cache_dir)
        trained = 0
        for identifier, sentence in sentences(archive):
            # Entire sentences are excluded, before tokenization/counting.
            held_out = identifier % 100 == 0
            for left, right, raw_left, raw_right in pairs(sentence, language):
                if held_out:
                    if args.holdout:
                        held.append(f"{language}\t{raw_left} {raw_right}\n")
                elif left in allowed[language]:
                    counts[left, right] += 1
                    trained += 1
        stats[language] = trained
    blob, sizes = encode(counts)
    manifest = {"format": "UPYRBG1", "sources": SOURCES["1M"],
                "normalization": "lowercase; normalize apostrophes; alphabet-filter; whitespace-adjacent pairs only; length 1..48",
                "short_words_sha256": hashlib.sha256(SHORT_WORDS.read_bytes()).hexdigest(),
                "minimum_pair_count": MIN_COUNT, "excluded_sentence_ids": "id % 100 == 0",
                "training_pair_occurrences": stats, **sizes, "sha256": hashlib.sha256(blob).hexdigest()}
    metadata = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode()
    if args.check:
        if args.output.read_bytes() != blob or args.output.with_suffix(".json").read_bytes() != metadata:
            raise SystemExit("context artifacts differ from deterministic regeneration")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(blob)
        args.output.with_suffix(".json").write_bytes(metadata)
    if args.holdout:
        args.holdout.parent.mkdir(parents=True, exist_ok=True)
        args.holdout.write_text("".join(held))
    print(json.dumps({**sizes, "heldout_pairs": len(held)}, indent=2))


if __name__ == "__main__":
    main()
