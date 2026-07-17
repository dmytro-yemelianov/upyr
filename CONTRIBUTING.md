# Contributing to Upyr

Upyr welcomes focused bug fixes, regression tests, platform improvements, and
language-model work. Privacy, physical-key correctness, and low-latency local
inference are architectural requirements.

## Before making a change

Search existing issues. For bugs, open the structured bug or model-report form
with a synthetic reproduction. For a substantial feature or new permission,
network capability, dependency, language, or model format, discuss the design in
an issue before implementation.

Never put passwords, private messages, customer data, proprietary corpora, signing
material, or real captured typing into an issue, test, fixture, commit, or log.

## Development setup

Install the Rust toolchain declared by `rust-version` in `Cargo.toml` or newer.
Linux builds also require the GTK 3, X11, xkbcommon, and Ayatana
AppIndicator development packages listed in the README.

Run the desktop app during development with:

```sh
cargo run --locked --bin upyr-background
```

The CLI can exercise layout conversion without granting global input access:

```sh
cargo run --locked -- convert ghbdsn
```

## Required checks

Run the checks relevant to your platform before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo audit
python3 tools/check_privacy.py
python3 tools/check_site_content.py
python3 -m unittest tools.test_check_site_content tools.test_check_privacy
python3 tools/check_version_sync.py
python3 tools/check_codebase_graph.py
```

For WASM changes, also run the build, Node smoke test, and size check documented in
`crates/upyr-wasm/README.md`. Platform-specific changes should be tested on the
affected operating system. Include the exact commands and manual scenarios in the
pull request.

## Correction and n-gram changes

The correction engine in `crates/upyr-core` must remain deterministic and
platform-neutral. Add regression cases for the physical key sequence, source
layout, expected conversion, capitalization, and punctuation. Include mixed
English/Ukrainian context and product names when they explain an edge case.

The packed model is derived from character n-grams, not shipped whole-word user
history. Model contributions must:

1. use public, license-compatible corpora that may be redistributed or used to
   derive the model;
2. document the source URL, version, license, and checksum;
3. avoid personal, scraped-private, or proprietary text;
4. regenerate with `tools/generate_ngram_model.py` from pinned inputs;
5. include holdout/replay results, model-size impact, and false-positive analysis;
6. keep corpus archives and caches out of Git.

See `docs/benchmarks/signed-ngram-v1.md` for the current evaluation contract.

## Pull requests

Keep changes reviewable and avoid unrelated formatting or generated artifacts.
Use an imperative title, link the issue, explain privacy and failure modes, and
update user-facing documentation when behavior changes. The project follows
Semantic Versioning; normal feature branches do not bump versions unless they are
part of a release change.

By submitting a contribution, you agree that it is licensed under the repository's
MIT License and that you have the right to provide it.
