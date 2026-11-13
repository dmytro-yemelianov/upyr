# What Punto Switcher actually does with its rules

*Corrected reverse-engineering findings, 2026-11-11. The earlier version of this
article misidentified the Windows matcher and misinterpreted several tags.*

Upyr's early-switching implementation now uses the complete rule resources from
Punto Switcher for Windows 4.4.2.334: 4,511 triggers and 37,612 nonempty ps.dat
rules. The original data, decoding procedure, hashes, and detailed static
Evidence IDs are documented in [Punto early-word rules](../punto-early-rules.md).
These are archived 2018 resources, not a claim about the current release.

## What the fresh binary analysis established

The Windows loader decodes ps.dat, skips its version header and empty rows, and
parses tagged entries at the first ASCII space. It preserves the rest of the key
literally, including trailing spaces. The source file has one blank data row;
counting every line as a rule would create a spurious empty substring match.

Rules are ordered in three passes: A, then E, then all remaining rules. Within
each group, their source order is preserved. A parallel byte array holds flags.
The actual matcher, `FUN_0041f170`, scans these ordered strings and returns the
flags of the first matching rule.

| Tag | Windows 4.4.2.334 meaning |
| --- | --- |
| no position tag | Match a substring |
| B | Match at the beginning |
| P | Match the whole input; overrides B |
| C | Compare the original input; otherwise compare lowercased input |
| E | Exception: keep the source layout |
| A | Highest-priority group; a source match bypasses target checking |
| D | Suppress conversion after the preceding dash key |

The stored pattern is not lowercased by this Windows matcher. That detail can
make uppercase keys without C ineffective, and the port preserves the observed
behavior rather than silently rewriting the data.

## How switching before the end of a word works

The key handler calls `FUN_00476bd0` on the complete triggers.dat string array.
This checks case-insensitive prefixes. A trigger match invokes the same full
checker used at a word boundary, with a zero boundary argument.

That checker consults ps.dat for the typed source. An absent match or an E
exception prevents conversion. An A source match permits conversion directly.
For other positive source matches, the opposite-layout candidate must have no
positive rule match: an absent target match or a target exception permits the
switch. The D flag adds the preceding-dash condition.

The inspected Mac build differs: its early path checks trigger prefixes and
potential continuations of E exceptions. Upyr retains that future-exception
protection as an additional guard, along with user keep rules and EN/UK checks.

## Corrections to the previous report

The earlier claim that A is dead on macOS was wrong. Although its per-rule
checker does not directly test the A bit, the loader uses it to prioritize rules.
The earlier claim that E means end-position was also wrong: it marks exceptions.

The old Windows trie attribution was not connected to the actual layout checker.
Fresh call tracing found the linear matcher described above. A helper previously
treated as trie insertion, `FUN_0051e801`, is an indexed byte-array setter.
Those older function addresses therefore do not prove that Punto uses the
claimed automaton for layout detection.

## What Upyr implements

Upyr imports all the pinned data, including all 9,383 E-tagged rules, and compiles
it into Rust tries. Explicit ordering ranks preserve the native first-match
behavior across substring, exact, prefix, and case-sensitive branches. The trie
is Upyr's optimization of the recovered semantics, not a copied executable tree.

The EN/RU rules are evaluated through corresponding Ukrainian physical key
positions. Actual replacement uses the installed EN/UK layout map. Ukrainian
word-prefix protection and conservative language evidence prevent Russian rules
from incorrectly switching valid Ukrainian, such as `дівк…`.

The complete matcher participates in early switching. Upyr's existing
word-boundary scorer remains unchanged. The data is identified as third-party
Punto material in the project notices; it is no longer described as Upyr's own
small clean-room rule table.

All native conclusions here come from static analysis. The original Windows and
Mac applications were extracted but not installed or executed. Source data,
Evidence bundles, and snapshots are retained locally for further verification.
