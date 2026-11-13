//! Compile the immutable Punto tables once on the build host.
#![allow(dead_code)]

#[path = "src/auto_correct/punto_compile.rs"]
mod punto_compile;
#[path = "src/auto_correct/punto_model.rs"]
mod punto_model;
#[path = "src/auto_correct/punto_trie.rs"]
mod punto_trie;

use std::fmt::Debug;
use std::io::{BufWriter, Write};

fn write_trie<T: Copy + Debug>(
    out: &mut impl Write,
    name: &str,
    kind: &str,
    trie: punto_trie::Trie<T>,
) {
    writeln!(
        out,
        "pub(super) const {name}: Trie<{kind}> = Trie::from_static("
    )
    .unwrap();
    writeln!(out, "&{:?},", trie.nodes).unwrap();
    writeln!(out, "&{:?},", trie.edges).unwrap();
    writeln!(out, "&{:?},", trie.values).unwrap();
    writeln!(out, "&{:?},", trie.root).unwrap();
    writeln!(out, "&{:?});", trie.alphabet).unwrap();
}

fn main() {
    for path in [
        "build.rs",
        "assets/punto/ps.txt",
        "assets/punto/triggers.txt",
        "src/auto_correct/punto_compile.rs",
        "src/auto_correct/punto_model.rs",
        "src/auto_correct/punto_trie.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
    }
    let dest = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let mut out = BufWriter::new(std::fs::File::create(dest.join("punto_tables.rs")).unwrap());
    let (original, lowercased) = punto_compile::compile_rules(punto_compile::RULES).unwrap();
    for (name, trie) in [("ORIGINAL", &original), ("LOWERCASED", &lowercased)] {
        let (failure, anywhere) = punto_compile::compile_failure(trie);
        writeln!(
            out,
            "pub(super) const {name}_FAILURE: &[u16] = &{failure:?};"
        )
        .unwrap();
        writeln!(
            out,
            "pub(super) const {name}_ANYWHERE: &[u32] = &{anywhere:?};"
        )
        .unwrap();
    }
    write_trie(&mut out, "ORIGINAL", "Rule", original);
    write_trie(&mut out, "LOWERCASED", "Rule", lowercased);
    let (triggers, exceptions, case_exceptions) = punto_compile::compile_early();
    write_trie(&mut out, "TRIGGERS", "()", triggers);
    write_trie(&mut out, "EXCEPTIONS", "()", exceptions);
    write_trie(&mut out, "CASE_EXCEPTIONS", "()", case_exceptions);
    out.flush().unwrap();
}
