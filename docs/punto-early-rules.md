# Punto early-word rules imported into Upyr

On 2026-11-11 the eight hand-selected draft prefix rules were replaced with the
complete early-word rule path recovered from **Punto Switcher for Windows 4.4.2.334**.
This is a versioned archival artifact, not a claim of parity with current Punto
or a byte-for-byte port of its native executable.

## Data and reproduction

`crates/upyr-core/assets/punto/` preserves all 4,511 `triggers.dat` entries and
all 37,612 nonempty `ps.dat` rules, including their order, duplicates, case,
punctuation, and original tag spelling. The file retains its one empty data row
as well (37,613 data rows in total). It contains 9,383 E-tagged exceptions.
The header is `PSVersion=20170301`. The JSON manifest pins source and decoded hashes.

The package is a [Wayback capture of the official Windows installer](https://web.archive.org/web/20180101000000id_/https://download.cdn.yandex.net/punto/PuntoSwitcherSetup.exe),
resolved to the capture dated 2018-04-10. It was extracted without installing or
executing Punto or its helpers. Installer SHA-256:
`cd337ea4252118aec42decab6720ced96c324eaa006be21d1bf7bf632d357a76`.

Given the extracted Windows resource directory containing `ps.dat` and `triggers.dat`:

```sh
python3 tools/import_punto.py --source-dir /path/to/dictionaries
python3 tools/import_punto.py --source-dir /path/to/dictionaries --check
python3 -m unittest discover -s tools -p test_import_punto.py
```

The importer checks both pinned SHA-256 hashes before writing any output.
`ps.dat` is XOR-decoded with `0xaa`, except TAB/CR/LF bytes, then decoded as
Windows-1251. `triggers.dat` is plain Windows-1251. UTF-8 conversion preserves
all records; it does not curate a subset or silently repair tags.

## Native evidence and the recovered early path

Ghidra 12.1.3 analyzed `punto.exe`, SHA-256
`2010cd64f1797c6348217236dafbd81cec626744b50c94ec73bf2327a8251162`.
This is static evidence; the original application was not run. The earlier Mac
archive was also analyzed, but Windows semantics below come from this PE itself.

| Method / address | Observed behavior | Evidence ID |
| --- | --- | --- |
| `0x41e5b0` | Loads ps.dat, skips header and empty lines | `ev_4a05aafb1f37c12f86533b8aa7948fe6fa7f7cc69ae5386daca71037dd40f197` |
| `0x41f050` | Splits tagged rules at the first ASCII space; preserves the rest exactly | `ev_642a1d7c1478475f8c83850d7fa5fcc239e759300cd58c96e347000ed7f6bb68` |
| `0x41ea60` | Orders A rules first, E rules second, others last; encodes B/P/C/E/A/D flags | `ev_2a42877e7eefc688e72b718d7c544e719881e2c6bef73fd7ae6a27ef531d0854` |
| `0x41f170` | First-match linear scan: substring, equality or prefix; returns E/A/D flags | `ev_0be414989d46857651b61c78b0635520d322c6a26609ec6964a19600daf75af2` |
| `0x476bd0` | Case-insensitive prefix lookup in triggers.dat | `ev_6029cf15b7fc6700e4b1487cc2187cc1c46e74ce321e680fdb9c6d7d12549618` |
| `0x41b610` | Calls trigger lookup on the rules object's trigger vector, then calls the full checker with boundary=0 | `ev_a6f15e508e0ec1a5fc08371ddc4b0ad92e4c07a08584f7b762c35eb04841f794` |
| `0x41c1b0` | Checks source and opposite-layout candidate; exceptions and A priority decide conversion | `ev_0a63b0ebf427fb57fbae96f83adaa86377279eb50e0a4d04e147d91cc0042b3d` |

Windows first requires a trigger prefix, then invokes the same complete rule
checker used at word boundaries. The recovered rule grammar is:

| Tag | Meaning |
| --- | --- |
| none | Substring match |
| B | Prefix match |
| P | Exact match; takes precedence over B |
| C | Compare original input; otherwise compare lowercased input |
| E | Exception / keep |
| A | Highest-priority rule group; a positive source match bypasses target checking |
| D | Return a suppression flag used after the preceding dash key |

The first match in A → E → ordinary order wins; source order within a group is
preserved. A source exception or absent source match prevents correction.
A positive A source match permits it. Otherwise the target must have no positive
rule match (no match or an exception). All rules and flags are compiled into
Upyr's tries with explicit ordering ranks. The original Windows implementation
uses arrays and a linear scan; the trie is Upyr's acceleration of those semantics.

The rule key remains literal even without C: Windows lowercases the input, not
the stored key. The port preserves that quirk and literal trailing spaces rather
than silently repairing the data. Early triggers use case-insensitive lookup.

Upyr additionally protects potential E-exception continuations, following the
Mac early check, and conservatively protects future user keep rules. Its regular
word-boundary scorer remains unchanged; the complete imported matcher is used
for the new early path. No executable Punto code is embedded.

## Persistent source rules without a trigger

Upyr extends the imported path when no `triggers.dat` prefix exists: the first
matching source rule may also justify early correction if it is a substring or
beginning rule. Exact source rules wait for a boundary; source-rule ordering,
E exceptions, A priority, and the opposite-layout check remain intact. Future
exception continuations and user keep rules are still protected.

This fixes `dbghfdktyyz` → `виправлення`: at `dbgh`, the full table already
matches `bgh`, but there is no separate trigger prefix. The generated EN/UK
lexicon and conservative open-prefix evidence additionally confirm `випр`.
This extension is Upyr policy, not a claim about the archived native engine.

## EN/UK adaptation

The original data describes EN/RU. Lookup maps Ukrainian key positions into
the corresponding Russian source characters (`і`→`ы`, `ї`→`ъ`, `є`→`э`), while
the actual replacement always uses the installed EN/UK mapping. Changed custom
key mappings disable imported matches. All imported entries remain in the trie.

Upyr adds its own application guards: at least four letters (or a larger user
minimum), at most 64, alphabetic source and target, no manual mid-word layout
switch, known source-word prefix protection, exceptions and keep rules, and an
EN/UK language-evidence veto. The veto scores the open prefix without an invented
end-of-word marker and uses the existing conservative thresholds. It cannot
cause a correction without an imported trigger, persistent source rule, or
explicit user prefix rule. The [physical-key lexicon](prefix-lexicon.md) also
requires a target-language continuation and vetoes known source continuations. This protects
Ukrainian sequences such as `дівк…`, which the Russian table alone misclassifies.

## Validation and corrected historical findings

The trigger and continuation tries pass 96,506 comparisons with every trigger
and exception, their prefixes, uppercase forms, and extended forms. The complete
rule matcher passes 3,051 deterministic samples against an independent ordered
linear reference, plus focused flag/priority checks. Native EN/UK recall and replay
fixtures exercise another 14,287 prefixes; a continuation replay completes 971
early-corrected words and checks their following boundary. Application tests cover
layout switching, terminal replacement, undo, cancellation and manual changes.
The initial imported-table workspace run passed 348 tests (4 ignored), plus 3 importer tests;
Clippy with warnings denied and formatting checks pass.

The earlier [reverse-engineering article](articles/reverse-engineering-punto-switcher.md)
was incorrect: E is an **exception**, not an end-position marker, and A is live
priority logic in both examined versions. The claimed Windows trie addresses
were not connected to the actual layout checker. `0x51e801`, previously mistaken
for trie insertion, is a byte-array setter (evidence
`ev_996c9171b20ec10cb9f105a384a776ced565517d212740c8d6fd16eef14acd7a`).
The fresh Mac and Windows evidence bundles and snapshots are retained locally under
`.stos/punto-review/`, outside version control.

## Compact matcher storage and measurements

The full matcher and early guards share a compact immutable trie. `build.rs`
compiles all five trees from the original text assets into Rust arrays in
`OUT_DIR`. The production matcher borrows those read-only arrays; no parser,
builder, lazy initialization, or heap allocation is needed to initialize them.
Generated Rust values are target-independent and also work in the WASM build.
The source tables remain in the package for provenance and reproducibility.

Nodes contain four 16-bit offsets/counts (8 bytes), edges contain a 16-bit
symbol and node ID (4 bytes), and terminal rules pack their rank, mode and
flags into 4 bytes. Rank occupies the high bits, preserving native rule order.
The compiler fails explicitly if future imports exceed the format's limits;
indices cannot silently wrap. A direct root index covers ASCII and Cyrillic.
BMP symbols are stored directly; supplementary Unicode characters are assigned
codes in the otherwise unused surrogate range with a sorted side alphabet.
Already-lowercase inputs are borrowed instead of copied, while Unicode and
contextual lowercasing retain the standard behavior.

Tests compare every generated node, edge, root entry, alphabet and rule output
against compilation of the text assets. Independent string-oracle and typing
regressions additionally verify matcher semantics, case and native-word guards.

The standalone benchmark can be rerun without dependencies:

```sh
tools/benchmark_punto.sh
# Optionally retain the executable for alternating comparisons:
tools/benchmark_punto.sh /tmp/benchmark-punto
```

The first local ARM64 comparison on 2026-11-11 measured the original
BTreeMap-based full matcher against the version that built compact arrays at runtime. There are 612 fixed probes and
1,224,000 lookups per run. Each result is the median of seven repetitions; the
comparison alternated before/after three times and took their medians.

| Metric | Original tree | Runtime compact tree |
| --- | ---: | ---: |
| Live matcher allocations | 9,586,688 bytes | 1,360,132 bytes |
| Peak during construction | 10,635,264 bytes | 5,051,360 bytes |
| Construction time | 2,780 µs | 2,848 µs |
| Construction allocation calls | 58,668  | 23,414  |
| Lookup time | 141 ns | 85 ns |
| Allocation calls per 1,224,000 lookups | 1,224,000  | 202,000  |

Memory values are requested heap bytes tracked by a counting allocator, not
process RSS or allocator overhead. Lookup times are a microbenchmark, not an
end-to-end typing-latency guarantee. Both versions returned the same 992,000
matching results per run; the independent rule/guard and typing tests also pass.
Local raw runs, source hashes, and the original benchmark binary are retained
in `.stos/punto-benchmark/`.

### Build-time tables and packed fields

A second staged comparison uses the same machine, 612 probes, seven repeats
per executable, and three alternating runs. No compiler or test processes ran
during these timing samples.

| Metric (full rule matcher only) | Runtime arrays | Build-time arrays | Build-time packed arrays |
| --- | ---: | ---: | ---: |
| Table storage | 1,360,132 bytes | 1,360,132 bytes | 785,684 bytes |
| Heap retained for tables | 1,360,132 bytes | 0 | 0 |
| Peak heap during initialization | 5,051,360 bytes | 0 | 0 |
| Initialization time | 2,875 µs | <1 µs | <1 µs |
| Initialization allocation calls | 23,414 | 0 | 0 |
| Lookup time | 87 ns | 87 ns | 87 ns |
| Allocation calls per 1,224,000 lookups | 202,000 | 202,000 | 202,000 |

Packing saves 42.2% of the full matcher's table storage. All five generated
Punto tables together occupy 1,099,364 bytes. Zero table heap does **not** mean
zero memory usage: the read-only arrays occupy mapped executable pages, whose
first access can still fault into memory. These numbers exclude the rest of
the app, input/case-conversion buffers, and OS replacement work. Lookup speed
is effectively unchanged in this measurement; this step improves storage and
initialization, not end-to-end typing latency.

All variants returned 992,000 matching results per run. The final change passed
353 workspace tests (4 intentionally ignored), strict workspace Clippy, and
an all-target WASM compilation check. Raw samples, intermediate executables,
and source/executable hashes are in
`.stos/punto-benchmark/static-stages/final-comparison.json`.
