#!/usr/bin/env python3
"""Build a deterministic EN/UK physical-key prefix automaton from pinned lexicons.

Only subtree language masks are stored: no sentences, frequencies or terminal
word markers. Wikipedia/recall fixtures are evaluation input only, never read.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path

if __package__:
    from .generate_ngram_model import LANGUAGES, SOURCES, corpus_archive, frequency_rows, normalized_tokens
    from .generate_clean_holdout import ukrainian_to_physical, ENGLISH_LOWER, UKRAINIAN_LOWER
else:
    from generate_ngram_model import LANGUAGES, SOURCES, corpus_archive, frequency_rows, normalized_tokens
    from generate_clean_holdout import ukrainian_to_physical, ENGLISH_LOWER, UKRAINIAN_LOWER

MAGIC = b"UPYRLP1\0"
HEADER = struct.Struct("<8sIII")
NODE = struct.Struct("<I")
EDGE = struct.Struct("<I")
WEB2 = {
    "url": "https://raw.githubusercontent.com/freebsd/freebsd-src/4a65e12dfbaba7193e37fcd2e14fbc20776a78b9/share/dict/web2",
    "sha256": "92165d95d72a67377e80052b1b9c3d63a4ac402ccd71690c89d949e809738238",
    "archive": "freebsd-web2",
}


def vocabulary(language: str, archive: Path) -> set[str]:
    alphabet = LANGUAGES[language]["alphabet"]
    return {word for raw, count in frequency_rows(archive) if count > 0
            for word in normalized_tokens(raw, alphabet)}


def build_model(words: dict[str, set[str]]) -> tuple[bytes, dict]:
    # Ordinary trie first, then merge states with identical language masks and
    # transitions. Node IDs point backward in the minimized acyclic automaton.
    children: list[dict[int, int]] = [{}]
    masks = [0]
    for language, mask in [("english", 1), ("ukrainian", 2)]:
        for word in sorted(words[language]):
            physical = word if mask == 1 else ukrainian_to_physical(word)
            if not physical.isascii():
                raise ValueError(f"unmapped character in {word!r}")
            node = 0
            masks[node] |= mask
            for key in physical.encode("ascii"):
                child = children[node].get(key)
                if child is None:
                    child = len(children)
                    children[node][key] = child
                    children.append({})
                    masks.append(0)
                node = child
                masks[node] |= mask
    intern: dict[tuple, int] = {}
    states: list[tuple] = []
    mapped = [0] * len(children)
    for node in range(len(children) - 1, -1, -1):
        transitions = tuple((key, mapped[child]) for key, child in sorted(children[node].items()))
        signature = (masks[node], transitions)
        state = intern.get(signature)
        if state is None:
            state = len(states)
            intern[signature] = state
            states.append(signature)
        mapped[node] = state
    if len(states) >= 1 << 24:
        raise ValueError("prefix model exceeds 24-bit state IDs")
    edges = bytearray()
    nodes = bytearray()
    for mask, transitions in states:
        if len(transitions) > 63:
            raise ValueError("prefix model exceeds 6-bit branch count")
        if len(edges) // EDGE.size >= 1 << 24:
            raise ValueError("prefix model exceeds 24-bit edge offsets")
        nodes += NODE.pack(((len(edges) // EDGE.size) << 8) | (len(transitions) << 2) | mask)
        for key, child in transitions:
            edges += EDGE.pack((child << 8) | key)
    blob = HEADER.pack(MAGIC, len(states), len(edges) // EDGE.size, mapped[0]) + nodes + edges
    return blob, {"input_words": {lang: len(values) for lang, values in words.items()},
                  "trie_nodes": len(children), "states": len(states),
                  "edges": len(edges) // EDGE.size, "bytes": len(blob)}


def lookup(blob: bytes, physical: str) -> int:
    _, count, _, node = HEADER.unpack_from(blob)
    edges_start = HEADER.size + count * NODE.size
    for key in physical.encode("ascii"):
        record, = NODE.unpack_from(blob, HEADER.size + node * NODE.size)
        first, length = record >> 8, (record >> 2) & 63
        for index in range(first, first + length):
            edge, = EDGE.unpack_from(blob, edges_start + index * EDGE.size)
            if edge & 255 == key:
                node = edge >> 8
                break
        else:
            return 0
    return NODE.unpack_from(blob, HEADER.size + node * NODE.size)[0] & 3


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache-dir", type=Path, default=Path(".cache/upyr-corpora"))
    parser.add_argument("--output", type=Path, default=Path("crates/upyr-core/assets/models/prefixes.lxp"))
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    sources = SOURCES["1M"]
    words = {lang: vocabulary(lang, corpus_archive(lang, src, None, args.cache_dir))
             for lang, src in sources.items()}
    web2 = corpus_archive("English Web2", WEB2, None, args.cache_dir)
    words["english"].update(word for raw in web2.read_text().splitlines()
                            for word in normalized_tokens(raw, LANGUAGES["english"]["alphabet"]))
    blob, stats = build_model(words)
    manifest = {"format": "UPYRLP1", "sources": {**sources, "english_dictionary": WEB2},
                "normalization": "casefold; normalize apostrophes; split hyphens; alphabet-filter; length 2..48; all positive frequencies",
                "physical_keys": {"english": ENGLISH_LOWER, "ukrainian": UKRAINIAN_LOWER},
                "evaluation_inputs_in_training": False, **stats, "sha256": hashlib.sha256(blob).hexdigest()}
    manifest_bytes = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode()
    metadata = args.output.with_suffix(".json")
    if args.check:
        if args.output.read_bytes() != blob or metadata.read_bytes() != manifest_bytes:
            raise SystemExit("generated prefix model differs from checked-in artifacts")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes(blob)
        metadata.write_bytes(manifest_bytes)
    print(json.dumps(stats, indent=2))


if __name__ == "__main__":
    main()
