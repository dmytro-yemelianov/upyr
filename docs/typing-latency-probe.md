# Live typing regression and latency probe

`tools/run_typing_latency.py` exercises the installed Upyr binary with synthetic
physical-key events through the real macOS event monitor, correction pipeline,
input-source switch and editor. It uses its own AppKit text view and/or an owned
Paradev terminal tab containing a raw input sink. Enter in that sink stores a
newline; it cannot execute a command. It does not type into an existing editor.

## Run

Preparation compiles the Swift helpers and captures read-only preflight status:

```sh
python3 tools/run_typing_latency.py
```

The opt-in live run requires an unlocked Mac, existing Cua Driver Accessibility
and Screen Recording grants, and existing physical-event permission. It does
not request or work around permissions. Stop typing/clicking during the run;
outside input or a focus/fixture-identity change aborts physical-key delivery.

```sh
# One short native smoke test first.
python3 tools/run_typing_latency.py --run --surface native \
  --case context-space --repetitions 1 --output target/typing-smoke

# Both real surfaces. The workspace may also come from SUPERSET_WORKSPACE_ID.
python3 tools/run_typing_latency.py --run --surface both \
  --workspace WORKSPACE_ID --repetitions 20 --output target/typing-latency-report
```

Use `--window-id` if more than one Paradev main window exists; an explicitly
bound window can also return from another macOS Space. Terminal testing opens
the requested local workspace, then focuses its owned test tab. The probe checks
the tab's nonce marker in a fresh screenshot before sending each input batch;
selecting a tab in a hidden workspace is insufficient. Testing requires the
user's existing `com.superset.desktop` opt-in. The probe
briefly stops the installed background process, starts the installed binary
(or a candidate supplied with `--binary PATH`) with a private copy of the configuration, and restores the normal
process afterward. Only `auto_correct_learning` is disabled in the copy, so
repeated undo does not train exceptions. The original config is hash-checked.
The driver session and created editor/tab are closed; normal completion also
restores the original input source and foreground window, and the previous
pane in the tested workspace. It leaves that workspace selected in Paradev.
If outside activity
interrupts the run, it does not take focus/layout back from the user.

## Cases and measurements

The versioned corpus is `tests/fixtures/typing-latency-cases.json`: 24 scenarios
cover the reported `dbghfdktyyz`, `сщте`, `nj vb`, `Ш have` and mixed-language
phrases, typing intervals of 10/20/40/80 ms, manual/Globe layout switching,
Space, Enter, shortcut undo, Backspace undo, repeated separators and a queued
boundary correction overtaken by another key.

Timing starts immediately before posting the eligible CGEvent key-down (the
fourth prefix key, final Space, Enter, undo key or Backspace). Globe timing
starts at Fn release, after the simulated tap. The helper polls the owned
editor's text and the active TIS source at a nominal 2 ms interval and reports
the first confirmation of both. AppKit state publication also runs at 2 ms;
file access and scheduling add observation overhead. This measures **editor
state confirmation**, not display-photon latency or physical keyboard hardware.

`report.json` contains the installed binary hash, original config hash and delay
settings, every posted key's monotonic timestamp, actual final text/layout,
checkpoints, clipboard preservation results and per-scenario sample count,
nearest-rank p50/p95/min/max. Failed or untimed checkpoints are excluded from
latency summaries. Completed successful cases remain available if a later
stage aborts; the overall run still fails and records the reason. A small
sample count should not be interpreted as a production latency distribution.
Clipboard cleanup occurs outside the editing measurement; its content must be
restored, and terminal edits must also leave the change count unchanged. The
helper allows at least three additional seconds after its 400 ms settling
period for asynchronous restoration and records that wait separately. A fixed
400 ms check alone can fail during cold audio initialization, even when the
editor's correction has already completed.

The `boundary-overtaken` and `repeated-space` races allow only the exact
preserved or exact corrected text. They check for lost, duplicated or mixed
characters without claiming that every ambiguous scheduling race must correct.

## Offline verification

```sh
python3 -m unittest tools.test_run_typing_latency tools.test_superset_input_probe \
  tools.test_run_terminal_edge_tests
```

These tests verify percentile/sample filtering, private config isolation,
fixture coverage, a locked-session gate that never reaches GUI/process changes,
the raw sink's UTF-8/edit/Enter handling and owned-resource cleanup. Swift
helpers are also compiled with warnings treated as errors during preparation.
They do not substitute for a live run.

## Live results, 2026-11-12

Measured on Apple M5, macOS 27.2, installed Upyr 0.5.0 ARM64, background binary
SHA-256 `807fc40bd00279d9eb8587370829ca0abd070f22a6f47d03bcb8d5c8ec3354fc`.
The user's Conservative configuration was retained: automatic correction delay
35 ms, copy 90 ms, paste 40 ms, clipboard restore 250 ms. Learning was disabled
only in the private test config.

All **40 functional surface/scenario checks passed**: 20 in the AppKit editor
and 20 in the Paradev raw terminal. These include both midword directions,
`nj vb → то ми`, `Ш have → I have`, manual and Globe switching, Space/Enter,
shortcut and Backspace undo, abbreviations, the mixed-language sentence, and
four fast-typing cadences. The requested 10/20/40/80 ms gaps produced observed
median key intervals of approximately 12–88 ms after event-posting overhead.
No characters were lost or duplicated in these cases. The two boundary races
preserved the original text in both surfaces; they do not establish correction
of every race. Clipboard content was restored; terminal edits also left its
change count unchanged.

The repeated timing series uses **20 samples per scenario**. These are
end-to-end editor-state measurements with the limits described above; synthetic
Fn events exercise Upyr's Globe observer, not the physical keyboard's complete
macOS switching path. Normal background activity was left running.

| Scenario | AppKit p50 / p95, ms | Terminal p50 / p95, ms |
| --- | ---: | ---: |
| Fourth prefix key: `dbgh → випр` | 90.7 / 107.0 | 24.1 / 68.1 |
| `nj vb → то ми`, final Space | 127.3 / 135.1 | 76.8 / 131.2 |
| `Ш have → I have`, manual switch + final Space | 125.4 / 132.7 | 63.3 / 69.3 |
| `Ш рфму → I have`, Enter | 92.3 / 105.4 | — |
| Globe release to layout change, pending context retained | 7.3 / 23.6 | 11.1 / 19.9 |

All **180/180 repeated cases passed**, with no outlier removal. Enter is tested
functionally in the raw terminal after correction, not timed as a terminal
command-submission path. These distributions describe this machine and these
editors; they do not establish latency in every app.

The GUI path schedules a 30 ms selection wait and a configured 40 ms paste
wait. Valid AX selection can bypass the 90 ms copy fallback. A Space boundary
adds the configured 35 ms correction delay. The terminal path directly applies
validated key/Unicode edits. These scheduled waits explain much of the observed
GUI/terminal difference; further GUI latency work should investigate the edit
pipeline before tuning dictionary lookup again. No production delay settings
were changed for this measurement.

Local evidence (ignored, not committed) is under `.stos/typing-latency-review/`:
`final-report.json` combines the 40 functional checks and 180 repeated cases,
with source-report hashes. Native results come from `live-scenarios-1` and
`live-measurements-1`; those runs completed their native stages, then aborted
before terminal input during workspace/Spaces setup. Terminal results come
from `live-terminal-1` and `live-terminal-measurements-2`. An intervening setup
attempt refused input because the fixture marker was hidden in another
workspace. The runner now explicitly opens the requested workspace and waits
for the visible marker. The original configuration was unchanged in all four
evidence reports.

A subsequent combined smoke test (`final-smoke`) verified the workspace/Spaces
transition but found a separate clipboard regression: native correction and
layout succeeded, while readable clipboard formats were not all restored. The
log showed an unreadable iOS pasteboard type. `snapshot_native` previously
abandoned the entire native snapshot on the first unavailable format, invoking
the lossy text/HTML fallback. It now retains all readable formats and items;
formats whose data macOS cannot supply cannot be restored. Two tests use private
named pasteboards to reproduce the failure and verify rich data, multiple items,
and empty-board round trips without touching the general clipboard. The
unavailable-format regression failed before the fix. The timing table above
records the original binary, before this clipboard fix.

Offline validation: 17 Python tests passed, both Swift helpers compiled with
warnings treated as errors, and privacy and whitespace checks passed.

The clipboard fix passed **370 Rust tests** (6 ignored) and Clippy. The updated
ARM64 app was installed, then `post-clipboard-fix-2` passed **12/12 live checks**
across both surfaces: midword correction, both context phrases, shortcut undo,
Backspace undo and the 10 ms typing case. All clipboard checks passed, workspace
transition and cleanup completed, and the normal app/config were restored.
The preceding post-install attempt stopped on outside input/focus loss; its
first 400 ms clipboard check also ended before a logged 1.8 s cold audio startup
finished. The final run uses the bounded asynchronous cleanup wait described
above. These smoke samples are kept separate from the 20-sample timing table.

Installed background SHA-256 after the clipboard fix:
`75240f2c7adb08207037b5e0f3e2850d0605a7b57650a232a0376da05b41a9fe`.
The previous app is retained locally at
`.stos/install-clipboard-20261112/previous/Upyr.app`.

## Earlier selection readiness

The next optimization changes only the macOS selection-settling phase. For
automatic correction and undo whose keyboard input is held, the pipeline can
check the selected text after 15 ms instead of always waiting 30 ms. An early
result must exactly equal the expected text, including whitespace. Each AX
request in this one early probe has a 5 ms timeout. An unavailable, busy or
partially selected editor falls back to the original 30 ms checkpoint and
normal copy path. Manual selection conversions keep their existing behavior.
The configured copy, paste, correction and clipboard-restoration delays are
unchanged. Focus, input and clipboard ownership are still checked before paste.

Six additional pipeline tests cover early success, unavailable/partial/trim-only
selection, a delayed event loop, focus loss, clipboard changes, undo and manual
fallback. The 5 ms *probe schedule* tried first was usually too early for the
editor to finish processing the selection keys and did not materially improve
the measured medians. It was replaced by the 15 ms schedule; the AX request
timeout remains 5 ms.

On the same M5/macOS machine and unchanged configuration, the sequential live
series measured the following editor-state latencies (nearest-rank percentiles):

| Scenario | Before p50 / p95, ms | After p50 / p95, ms | Median reduction |
| --- | ---: | ---: | ---: |
| `dbgh → випр` | 90.5 / 100.2 | 83.3 / 96.0 | 7.9% |
| `nj vb → то ми`, Space | 128.5 / 138.1 | 119.8 / 131.9 | 6.8% |
| `Ш have → I have`, manual switch + Space | 126.5 / 133.0 | 116.7 / 129.1 | 7.8% |

Each series requested 20 samples per scenario. The first baseline midword
sample corrected the text/layout but failed clipboard preservation, so its
timing is excluded by the existing reporting rule (19 valid baseline midword
samples; 20 in every other cell). This baseline clipboard mismatch remains
unattributed; it is retained in the evidence, not presented as a successful
check. The final timing series passed all 60 cases, including clipboard checks.
The 15 ms probe avoided the fallback in 34/60 corrections; the earlier 5 ms
experiment did so in only 8/60. This is a modest observed improvement in the
owned AppKit editor, not a latency guarantee for other apps or a statistically
established production p95.

Evidence is under `.stos/gui-readiness-review/`: `before-2/report.json` is the
baseline, `after/report.json` is the 5 ms experiment without a meaningful speedup,
and `after-15ms/report.json` is the final timing series. The timing implementation
is unchanged; a clipboard mismatch diagnostic added after the baseline records
only format names and sizes, never clipboard payloads. The baseline background
hash is `75240f2c7adb08207037b5e0f3e2850d0605a7b57650a232a0376da05b41a9fe`;
the installed optimized background hash is
`36f1a5210fb644dbaa2484ede872b9743fe1ed58a158d50a5eb5369f43e3f001`.

Final validation: 376 Rust tests passed (6 ignored), Clippy/fmt/privacy passed,
and all 20 native live regression scenarios passed, including four typing
speeds, both correction directions, Space/Enter, Globe, undo and mixed text.
The 17 Python tests also passed. A PTY marker test exposed a partial-read race
in the test itself; it now inspects only complete rows and passed 20 consecutive
repetitions. The normal installed app was restored after testing, the original
config hash and input source are unchanged, and owned fixtures/sessions were
closed. Full comparison metadata is in
`.stos/gui-readiness-review/comparison.json`. The baseline app remains at
`.stos/install-gui-readiness-20261112/previous/Upyr.app`.

## Confirmed paste readiness

The next stage removes the ordinary pre-paste wait only after an exact AX
selection has been read, keyboard input remains held, and a readback verifies
both Upyr's replacement text and clipboard revision after the synchronous
native write. It applies to automatic correction and undo with a configured
paste delay of 0–40 ms. Longer configured delays remain in force as deliberate
compatibility settings. Copied selections, unheld input, matches that require
trimming whitespace and unavailable clipboard readback retain the configured wait.

Readiness schedules the existing Paste phase with no additional timer delay;
it still yields to the event loop, then checks focus/input and clipboard
ownership again before releasing the held input stream. It does not claim that
the target has consumed the paste, so the existing delayed clipboard restoration
is unchanged. There is no extra polling loop or blanket reduction of defaults.

The baseline reuses the previous final 60-sample report, whose installed binary
and config hashes were checked before editing. The new series uses the same
machine, configuration, three scenarios, timing helper and 20 samples per case:

| Scenario | Before p50 / p95, ms | After p50 / p95, ms | Median reduction |
| --- | ---: | ---: | ---: |
| `dbgh → випр` | 83.3 / 96.0 | 39.8 / 55.8 | 52.2% |
| `nj vb → то ми`, Space | 119.8 / 131.9 | 76.2 / 91.2 | 36.4% |
| `Ш have → I have`, manual switch + Space | 116.7 / 129.1 | 75.1 / 81.8 | 35.6% |

Both baseline and final series passed **60/60 cases**, including clipboard
preservation. The readiness condition allowed the pre-paste wait to be skipped
in all 60 final cases. No samples or outliers were removed. These remain
sequential observations in the owned AppKit editor, not an app-independent
latency guarantee. The cold first midword sample was 117.0 ms; min/max and every
sample remain in the raw report.

Evidence is in `.stos/paste-readiness-review/`: `before.json` is the unchanged
copy of the prior final report, `after/report.json` is the new series, and
`comparison.json` records the comparison and validation. The baseline background
hash is `36f1a5210fb644dbaa2484ede872b9743fe1ed58a158d50a5eb5369f43e3f001`;
the installed optimized background hash is
`9647ac3f43850f03d856d4ddb2614ba81f09c748dcecc14a9d1a82d05b6b8df3`.
The preceding app is retained at
`.stos/install-ready-paste-20261112/previous/Upyr.app`.

Validation: 381 Rust tests passed (6 ignored), Clippy/fmt/privacy checks passed,
and all 20 native GUI regression scenarios passed on the installed build.
They cover both midword directions, context, four typing speeds, mixed language,
Space/Enter, Globe, shortcut undo and Backspace undo. The original config hash
is unchanged; the normal process, original layout and foreground window were
restored, and owned fixtures/sessions were closed.

## Real editor and sustained-input probes

The opt-in runner also accepts `--surface textedit`, `chrome`, `cursor` or
`editors`. TextEdit runs in a new instance with one scratch document and an AX
observer. Chrome uses an isolated profile and a loopback-only textarea fixture.
Cursor uses a temporary development extension to observe its actual document;
`--existing-cursor` opens a dedicated development window in the signed-in profile
and closes only that scratch window. It never stops the shared Cursor process.
The isolated Cursor profile may require interactive sign-in.

Each observer confirms its own editor focus and emits a freshness timestamp.
The physical-input helper checks the window, process, nonce, focus heartbeat and
outside input before each key. It stops on interference and retains partial test
checkpoints in `interrupted.json`; interrupted series are not successful runs.
Private profiles, fixtures and raw reports stay under the chosen output directory.

`--burst` sends 200 alternating corrections (`виправлення continue`, repeated
100 times) into one document, with a nominal 20 ms interval between physical
keys, without clearing it between corrections. Every trigger is timed; the
first cold sample remains included. Text/layout confirmation between words is
part of this scenario, so it is not an uninterrupted 200-word paste.
Reports expose nearest-rank p99 only with at least 100 successful timed
checkpoints, retaining N/min/max. This is a local scenario distribution,
not a production tail-latency estimate.

`--continuous-pairs 100 --case continuous-alternating-10ms` instead sends
100 word pairs without waiting for individual corrections. The default corpus
uses eight pairs at each speed. These stress cases verify the complete text
and final layout; they do not produce individual correction latency samples.

`--silent` changes only `[sounds].enabled` in the private test config, in addition
to the normal learning override, for a matched sound-on/off experiment. The
original configuration and installed binary are not modified.

~~~sh
python3 tools/run_typing_latency.py --run --surface textedit --burst --output target/textedit-series
python3 tools/run_typing_latency.py --run --surface chrome --burst --output target/chrome-series
python3 tools/run_typing_latency.py --run --surface cursor --existing-cursor --burst --output target/cursor-series
python3 tools/run_typing_latency.py --run --surface textedit --burst --silent --output target/textedit-silent
~~~

Measurements include observer transport: TextEdit AX polling is nominally 5 ms,
Chrome reports DOM input changes over loopback HTTP, and Cursor reports document
events through its local extension host. Browser/extension focus heartbeats run
at 10 ms. These observations must not be compared as pure Upyr execution time.

The completed stepped series passed 200 corrections per editor, including
exact clipboard preservation. Cold samples and outliers are included:

| Editor | p50 / p95 / p99, ms | Min / max, ms | Fast path / copy fallback |
| --- | ---: | ---: | ---: |
| TextEdit 1.21 | 73.22 / 98.18 / 111.71 | 39.35 / 124.55 | 200 / 0 |
| Chrome 157.0.8103.51 | 48.50 / 82.60 / 187.92 | 30.29 / 270.33 | 196 / 4 |
| Cursor 3.27.12 | 53.81 / 83.43 / 92.88 | 31.74 / 213.18 | 199 / 1 |

All four Chrome fallbacks were its first four corrections. Cursor's 213.18 ms
outlier was correction 67 and used the copy fallback. Its first correction was
109.05 ms and used the fast path. Thus these observed tail events are explained
by fallback selection handling, not simply discarded as cold-start noise.
These timings are specific to the stepped scenario and different observers.

With sound disabled only in TextEdit's private config, all 200 corrections
passed: p50 74.27 ms, p95 95.98 ms, p99 100.24 ms, min/max 35.62/116.69 ms.
The median did not improve in this single sequential comparison. This does not
prove that sound cannot stall the main thread: NSSound setup/playback is invoked
synchronously there. The earlier 1.8-second cold delay was not reproduced.

Evidence is under `.stos/real-editors-review/`: `textedit-burst`,
`cursor-burst`, `chrome-burst-2` and `textedit-silent` each contain the raw
report and logs. The interrupted first Chrome series remains recorded as aborted.
The installed binary/config hashes match the previous section.

### Continuous alternating input exposes a defect

The added `continuous-alternating-10ms`, `20ms` and `40ms` cases send eight
repetitions of the physical keys `dbghfdktyyz continue ` without intermediate
confirmation. All nine editor/speed combinations failed exact text validation:
some words remained in the previous layout, some became mixed (for example
`contшnue`), and some trials lost parts of words. All nine retained the original
clipboard. The neighboring context-Space, manually switched context and undo
checks passed in every editor (9/9). Evidence: `context-continuous/report.json`.

The 40 ms TextEdit case also failed with sound off (`continuous-silent`), so sound
is not the sole cause. The unchanged physical text passed at 10/20/40 ms with
autocorrection disabled only in the private config (`continuous-control-2`).
The first control run also disabled the sound's auto-correct cue due to an
overbroad test-config rewrite; it is retained separately and superseded by that
corrected control. An isolation test now protects same-named keys in TOML sections.

These failing live regression cases remain in the corpus. At this investigation
stage the production input pipeline had not been changed. The repair below
addresses layout changes, edit delivery and held-key replay, including corrections
queued behind clipboard restoration.

The earlier isolated clipboard mismatch remains unattributed. A new test on a
private native pasteboard runs 100 full-format round trips with plain text,
HTML, RTF, binary and empty data plus an unavailable iOS format. Every readable
format and byte matches, including any native format aliases; the general
clipboard is untouched. This verifies controlled preservation, not the cause of
the older failure.

### Continuous-input repair

The GUI path starts holding incoming keys before cross-process editor queries,
selects inside the target process, verifies the exact selected prefix, and
replaces it with one ordered batch of Unicode events. It then releases the held
keys in order with the destination layout's characters. A bounded caret query
can include extra prefix letters that reached the editor before the hold.
If the editor delivers a queued letter between that query and the selection,
the selected suffix and a bounded range read can extend the selection to the
whole longer prefix. This re-evaluates the normal language rules and exceptions,
then verifies the exact expanded selection before replacing it.
Unsupported selection readers retain the copy fallback; clipboard restoration
does not block an already verified direct replacement.

Posting selection Shift through HID was a separate race: an owned-fixture trace
captured uppercase `F` in the CGEvent Unicode cache despite unshifted event flags,
which became `f` after `випр`. The target-process selection avoids changing global
modifier state. Manual input-source notifications clear stale translation maps.
While input is held, Electron's transient AX-element replacement does not cancel
an edit with a stable app/window and an exact verified selection.

Already-dispatched application events and monitor-buffered physical keys keep
their order across consecutive corrections. A terminal correction cancelled
before editing solely because another key arrived retains its tracked prefix;
the next key re-evaluates the longer word. Pointer, modifier and focus changes
still invalidate that retry. Terminal deletion additionally requires the last
key delivered to the target process to have the exact physical-event timestamp
of the recognized prefix. Counter snapshots alone are insufficient when the
event loop recognizes key four after key five has already arrived. Event identity
survives both replay queues; rejected batches release their held keys in order.
Sound playback runs on a bounded worker queue,
keeping NSSound setup off the input thread.

The final candidate's background SHA-256 is
`d84be25692bed640ae22810429f6749a20222d68c58311c32496a4441999d1e4`.
Compared with the previous `9647ac3f…` build on the same Mac/configuration,
with sound enabled and the same private learning override:

| Editor | p50 before → after, ms | p95 before → after, ms | p99 before → after, ms |
| --- | ---: | ---: | ---: |
| TextEdit | 73.22 → 41.84 | 98.18 → 51.99 | 111.71 → 59.12 |
| Chrome | 48.50 → 39.18 | 82.60 → 51.97 | 187.92 → 141.95 |
| Cursor | 53.81 → 42.48 | 83.43 → 60.32 | 92.88 → 72.68 |

Each before/after series contains 200 successful timed corrections, including
its first sample and all outliers. All 600 final corrections preserved exact
text/layout and clipboard contents. Final min/max: TextEdit 30.82/82.55 ms,
Chrome 28.83/191.02 ms, Cursor 31.67/122.18 ms. These are sequential local
measurements including observer transport, not a guarantee for every application
or a comparison of the editors' own speed.

The same candidate passed all 46 native/terminal corpus checks and all 18 real
editor context, undo and continuous-input checks. Offline validation passed
396 Rust tests (6 ignored), 19 Python tests, Clippy, formatting and the privacy
check. Raw evidence is in `.stos/continuous-fix/anchored-regressions`,
`anchored-editors`, `anchored-benchmarks`, `anchored-chrome-benchmark` and
`anchored-cursor-benchmark`. The combined benchmark run completed TextEdit before
Chrome's owned window failed to start; that raw run remains marked aborted.
Only its completed TextEdit series is used. Chrome and Cursor have separate
complete runs; no failed checkpoint was discarded from a reported series.

The long final stress runs also passed: 100 uninterrupted word pairs in Cursor
at a nominal 40 ms key interval, and 100 pairs in the owned terminal at 10 ms
(`anchored-cursor-stress` and `anchored-terminal-stress`). Each produced the exact
2,100-character text and final layout, with unchanged clipboard contents. These
400 continuous switches are separate from the 600 stepped latency samples.

The validated ARM64 preview was installed in `/Applications/Upyr.app` on
2026-11-12. The original configuration hash remained
`c15c242c429ad016500634d705c39b385563d17e4cd0932f9dfa94aadd620444`.
A baseline bundle backup and the installation record are kept in
`.stos/install-continuous-20261112/`; `.stos/continuous-fix/comparison.json`
contains the full before/after summary and links the retained experimental runs.
The installed-path smoke test then passed all six checks (both mid-word
directions, contextual Space/manual switch, undo and continuous 10 ms typing).
The normal Upyr process was restored, no owned test windows remained, and the
current user clipboard was left untouched during final cleanup.
