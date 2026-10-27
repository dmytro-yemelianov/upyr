//! GlobeSwitch's original pixel flags and layout-to-region rules.
use std::collections::BTreeMap;

const FLAGS: &[u8] = include_bytes!("../../assets/globe-switch/flags.bin");
const LANGUAGE_REGIONS: &str = include_str!("../../assets/globe-switch/language-regions.txt");

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Flag {
    Region(String),
    Text(String),
}

/// Enumerate the complete bundled collection, including territory aliases.
/// The gallery and layout indicator read exactly the same lossless images.
pub(crate) fn images() -> impl Iterator<Item = (&'static str, &'static [u8])> {
    let mut remaining = FLAGS.strip_prefix(b"UPYRFLAGS1").unwrap_or_default();
    std::iter::from_fn(move || {
        let code = std::str::from_utf8(remaining.get(..2)?).ok()?;
        let length = u32::from_le_bytes(remaining.get(2..6)?.try_into().ok()?) as usize;
        let end = 6_usize.checked_add(length)?;
        let image = remaining.get(6..end)?;
        remaining = remaining.get(end..)?;
        Some((code, image))
    })
}

pub(super) fn png(region: &str) -> Option<&'static [u8]> {
    images().find_map(|(code, image)| code.eq_ignore_ascii_case(region).then_some(image))
}

pub(crate) fn flag(
    id: &str,
    languages: &[String],
    overrides: &BTreeMap<String, String>,
) -> Option<Flag> {
    if let Some(choice) = overrides.get(id) {
        return Some(if png(choice).is_some() {
            Flag::Region(choice.to_ascii_uppercase())
        } else {
            Flag::Text(choice.clone())
        });
    }
    let name = id.rsplit('.').next().unwrap_or(id);
    for (prefix, region) in [
        ("British", "GB"),
        ("Australian", "AU"),
        ("Canadian", "CA"),
        ("Irish", "IE"),
        ("Swiss", "CH"),
        ("Austrian", "AT"),
        ("Belgian", "BE"),
        ("Portuguese", "PT"),
    ] {
        if name.starts_with(prefix) {
            return Some(Flag::Region(region.to_owned()));
        }
    }
    let language = languages.first()?.replace('_', "-");
    if let Some(region) = language
        .split('-')
        .skip(1)
        .find(|part| part.len() == 2 && png(part).is_some())
    {
        return Some(Flag::Region(region.to_ascii_uppercase()));
    }
    let region = LANGUAGE_REGIONS
        .lines()
        .find_map(|line| {
            let (key, region) = line.split_once(' ')?;
            (key.eq_ignore_ascii_case(&language)).then_some(region)
        })
        .or_else(|| {
            let primary = language.split('-').next()?;
            LANGUAGE_REGIONS.lines().find_map(|line| {
                let (key, region) = line.split_once(' ')?;
                (key == primary).then_some(region)
            })
        })?;
    png(region).map(|_| Flag::Region(region.to_owned()))
}

pub(super) fn shown(id: &str, skipped: &[String], previous: Option<&str>, active: &str) -> bool {
    id == active || Some(id) == previous || !skipped.iter().any(|skip| skip == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_flag_is_a_complete_22_by_14_png() {
        let mut remaining = &FLAGS[10..];
        let mut count = 0;
        while !remaining.is_empty() {
            let region = std::str::from_utf8(&remaining[..2]).unwrap();
            let length = u32::from_le_bytes(remaining[2..6].try_into().unwrap()) as usize;
            let image = png(region).expect("embedded region is readable");
            assert_eq!(&image[..8], b"\x89PNG\r\n\x1a\n");
            assert_eq!(u32::from_be_bytes(image[16..20].try_into().unwrap()), 22);
            assert_eq!(u32::from_be_bytes(image[20..24].try_into().unwrap()), 14);
            remaining = &remaining[6 + length..];
            count += 1;
        }
        assert_eq!(count, 259);
        assert_eq!(png("ua"), png("UA"));
        assert_eq!(png("BV"), png("NO"));
        assert_eq!(png("UM"), png("US"));
        assert!(png("ZZ").is_none());
    }

    #[test]
    fn overrides_and_country_specific_names_take_precedence_over_language() {
        let overrides = BTreeMap::from([("com.apple.keylayout.ABC".to_owned(), "GB".to_owned())]);
        assert_eq!(
            flag("com.apple.keylayout.ABC", &["en".to_owned()], &overrides),
            Some(Flag::Region("GB".to_owned()))
        );
        assert_eq!(
            flag(
                "com.apple.keylayout.British",
                &["en".to_owned()],
                &BTreeMap::new()
            ),
            Some(Flag::Region("GB".to_owned()))
        );
        assert_eq!(
            flag(
                "com.apple.keylayout.Ukrainian-PC",
                &["uk".to_owned()],
                &overrides
            ),
            Some(Flag::Region("UA".to_owned()))
        );
        assert_eq!(
            flag("ABC", &["en-GB".to_owned()], &BTreeMap::new()),
            Some(Flag::Region("GB".to_owned()))
        );
    }

    #[test]
    fn custom_text_and_unknown_languages_keep_their_fallback() {
        let overrides = BTreeMap::from([("ABC".to_owned(), "EN".to_owned())]);
        assert_eq!(
            flag("ABC", &[], &overrides),
            Some(Flag::Text("EN".to_owned()))
        );
        assert_eq!(flag("unknown", &["zz".to_owned()], &overrides), None);
    }

    #[test]
    fn skipped_sources_stay_hidden_except_the_two_switch_endpoints() {
        let skipped = vec!["old".to_owned(), "active".to_owned(), "other".to_owned()];
        assert!(shown("old", &skipped, Some("old"), "active"));
        assert!(shown("active", &skipped, Some("old"), "active"));
        assert!(!shown("other", &skipped, Some("old"), "active"));
        assert!(shown("enabled", &skipped, Some("old"), "active"));
    }
}
