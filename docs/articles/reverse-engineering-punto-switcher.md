# What Punto Switcher actually does with its rule tags

*A reverse-engineering note on Punto Switcher's layout-detection engine, the
Mac/Windows divergence hiding behind the same data files, and the one part Upyr
adopted without importing Punto's code or rule data.*

---

## The short version

Upyr started as a clean-room rebuild of the useful Punto Switcher behavior:
recognize when a user typed the right physical keys on the wrong keyboard layout,
then rewrite the text in the intended layout. The first implementation inferred
the broad shape from behavior: deterministic rules for high-certainty cases, then
a cautious statistical scorer for everything else.

The later binary teardown answered a narrower question that behavior alone could
not settle: what do Punto's rule tags actually mean?

The decisive result is that Punto's macOS and Windows builds do **not** agree on
one tag:

| Tag | macOS build | Windows build |
| --- | --- | --- |
| `A` | parsed into a bit, then never tested | live wildcard: match at any word position |
| `B` | begin-position condition | begin-position condition |
| `C` | case/condition bit in the rule checker | node-position condition |
| `D` | negation/exception bit in the rule checker | node-position condition |
| `E` | end-position condition | end-position condition |

Same `ps.dat`, different engines. On macOS, `A` is vestigial. On Windows,
`A` is consumed by the live matcher.

Upyr incorporated the useful part conservatively: deterministic triggers now
support leading/trailing `*` patterns. The form `*text*` is Upyr's clean-room
way to express the Windows `A` finding: "this physical sequence may match
anywhere in the current word." Upyr still ships its own tiny trigger table and
does not include Punto's dictionaries or rule files.

## What was found in the data files

The Windows and macOS builds use the same family of data files:

| File | Role established by the teardown |
| --- | --- |
| `ps.dat` | main layout-detection rule/dictionary data |
| `triggers.dat` | deterministic switch-trigger data |
| `translit-en.dat`, `translit-ru.dat` | transliteration tables |
| `replace.dat` | user autoreplace/abbreviation data |
| `prog_ex.dat`, `folders.dat`, `titles.dat` | program/path/title exceptions |
| `diary.dat` | typed-text diary storage |

The Windows binary also carries embedded SQLite and schema strings for the diary,
clipboard history, and cookie-like state. That matches the long-standing privacy
concern around Punto's Diary feature: it is not a layout heuristic; it is typed
text persistence. Upyr deliberately has no equivalent feature, no telemetry, and
no runtime network client.

The important implementation detail for `ps.dat`: the payload is decoded with an
XOR `0xaa` transform. In the Windows build, Ghidra identifies the loader around
`FUN_0041ea80`; the decompiled body contains the `x ^ 0xaaaaaaaa` pattern and the
Russian error path for failing to open `ps.dat`. The loader decodes and stores
lines; the actual tag interpretation happens later in the matcher.

## The macOS engine

The macOS build uses a straightforward string-rule engine. A function named like
`parseRule:` turns a rule suffix into a bitmask, then `checkStringWithRule:` tests
the candidate with operations equivalent to prefix, suffix, substring, case, and
negation checks.

The useful part was easy to prove there:

- `A` is parsed and assigned bit `0x20`.
- No live checker reads that bit.
- The rest of the rule bits are used by the rule evaluator.

That makes `A` dead on macOS. It exists in the file format and in the parser, but
it has no behavioral effect in that build.

## The Windows engine

The Windows binary resisted the quick methods because it is not shaped like the
Mac engine. There is no useful sequence of direct character comparisons against
`A`, `B`, `C`, `D`, or `E` in the first obvious parser path. The engine is a
compiled automaton/trie walker over decoded rule data, and the tag handling is
buried behind a predicate called by the walker.

Ghidra made the difference. The live path is:

| Function | Finding |
| --- | --- |
| `FUN_0041ea80` | opens and XOR-decodes `ps.dat` |
| `FUN_004dfec0`, `FUN_004e02f0` | walk the compiled rule automaton |
| `FUN_004b16a0` | checks whether a node's position/type satisfies a tag |

The decisive predicate is `FUN_004b16a0(rule_state, tag_char)`. Its first branch
is the whole answer:

```c
if (tag_char == 'A') {
    return true;
}
```

For `B`, `C`, `D`, and `E`, the function switches on a node/type field and checks
whether the current automaton position satisfies the requested condition. For `A`,
there is no condition. It is an unconditional match.

That means the Windows interpretation of `A` is not "unused." It means "this rule
can match anywhere."

## Why the divergence matters

The surprising part is not that the two builds use different implementation
techniques. That is normal for mature cross-platform desktop software. The
surprising part is that the same data file semantics are not preserved exactly.

On macOS:

```text
read tag A -> set bit 0x20 -> never test bit 0x20
```

On Windows:

```text
read tag A -> call tag predicate -> return true
```

So a rule marked with `A` can affect correction behavior on Windows and do
nothing on macOS. The small set of `_A` entries in `ps.dat` is therefore a real
cross-platform behavioral divergence, not just an unused historical artifact.

This also explains why the early byte-hunting pass was inconclusive. Looking for
`cmp al, 'A'` or wide-character comparisons in the obvious parser code was the
wrong shape for the Windows binary. The tag letter eventually appears in a small
predicate called from the automaton walker, not in a Mac-style bitmask checker.

## What Upyr adopted

Upyr should not copy Punto's data. It does not need to. The useful engineering
lesson is the rule shape:

- exact deterministic rules are good for high-certainty short words;
- position-aware deterministic rules are useful when a sequence is meaningful as
  a prefix, suffix, or substring;
- wildcard rules must be opt-in because they are more powerful and easier to
  over-broaden.

Upyr's trigger table now supports a deliberately tiny wildcard syntax:

```text
word      exact match
word*     prefix match
*word     suffix match
*word*    any-position match
```

The built-in table still uses exact rules. That is intentional. The production
policy remains precision-first, and every broadening rule should be justified by
a reproducible test or a domain-specific user need.

The implementation lives in `crates/upyr-core/src/triggers.rs`; the evaluator in
`auto_correct.rs` still consults triggers before the statistical scorer. This
keeps the original two-layer design intact:

1. deterministic triggers can force `correct` or `keep`;
2. the signed n-gram model handles everything else;
3. uncertainty still resolves to no correction.

## What Upyr did not adopt

The teardown also confirmed several things Upyr should continue to avoid:

- no typed-text diary;
- no clipboard-history database;
- no cookies or network-linked runtime state;
- no opaque multi-megabyte proprietary dictionary;
- no hidden cross-platform rule semantics users cannot inspect.

The point of studying Punto was to recover the useful decision architecture, not
to inherit the product's privacy model or proprietary assets.

## The result

The final reverse-engineering result is sharper than the original black-box
story:

- `ps.dat` is XOR-decoded, not just an unknowable blob.
- Punto's Windows engine compiles and walks a rule automaton.
- Punto's macOS engine uses a separate bitmask/string-rule evaluator.
- The `A` tag is dead on macOS but live on Windows as an any-position wildcard.
- Upyr now has a clean-room way to express that wildcard rule shape while keeping
  its own data, benchmarks, and privacy boundary.

That is the right kind of incorporation: not copying the old engine, but letting
the old engine's proven edge case improve the new one.

---

*Upyr is an independent Rust project; it is not affiliated with Yandex or Punto
Switcher and shares no code or rule data with Punto Switcher. The reverse-
engineering described here was used to understand file formats and rule
semantics, then translated into a small, separately implemented trigger feature.*
