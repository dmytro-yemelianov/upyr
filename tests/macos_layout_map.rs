//! Read-only native integration check. TIS must run on the process main thread.
//! No event posting, app activation, clipboard access or preference writes.

#[cfg(target_os = "macos")]
fn main() {
    use std::collections::HashSet;
    use upyr::{Direction, convert_with_mapping, system_layout};

    let before = system_layout::current().expect("read current input source");
    let Some(pairs) = system_layout::installed_mapping().expect("read installed layout mapping")
    else {
        println!("macOS layout check skipped: both English and Ukrainian layouts are required");
        return;
    };
    let mut english = HashSet::new();
    let mut ukrainian = HashSet::new();
    for &(en, uk) in &pairs {
        assert!(
            english.insert(en) && ukrainian.insert(uk),
            "mapping must be reversible"
        );
        assert_eq!(
            convert_with_mapping(&en.to_string(), Direction::EnglishToUkrainian, &pairs).text,
            uk.to_string()
        );
        assert_eq!(
            convert_with_mapping(&uk.to_string(), Direction::UkrainianToEnglish, &pairs).text,
            en.to_string()
        );
    }
    assert_eq!(
        before,
        system_layout::current().unwrap(),
        "read-only check changed input source"
    );
    if std::env::args().any(|arg| arg == "--dump") {
        #[derive(serde::Serialize)]
        struct Mapping {
            pairs: Vec<[String; 2]>,
        }
        let mapping = Mapping {
            pairs: pairs
                .iter()
                .map(|(en, uk)| [en.to_string(), uk.to_string()])
                .collect(),
        };
        print!("{}", toml::to_string(&mapping).unwrap());
    } else {
        println!(
            "macOS layout mapping: {} reversible pairs; input source unchanged",
            pairs.len()
        );
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS native layout check skipped on this platform");
}
