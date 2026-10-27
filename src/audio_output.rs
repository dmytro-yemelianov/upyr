//! Output-aware app gain. This never reads a microphone or changes OS volume.
use crate::config::SoundSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Speakers,
    Headphones,
    Unknown,
}

impl OutputKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Speakers => "speakers",
            Self::Headphones => "headphones",
            Self::Unknown => "unrecognized output (quiet limit)",
        }
    }
    pub fn volume(self, settings: &SoundSettings) -> u8 {
        if self == Self::Speakers {
            settings.volume_percent
        } else {
            settings
                .volume_percent
                .min(settings.headphone_volume_percent)
                .min(30)
        }
    }
}

#[derive(Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct OutputRoute {
    pub kind: OutputKind,
    pub name: String,
    pub(crate) uid: Option<String>,
    pub(crate) device: u32,
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos;

#[cfg(target_os = "macos")]
pub fn current() -> OutputRoute {
    macos::current()
}
#[cfg(not(target_os = "macos"))]
pub fn current() -> OutputRoute {
    OutputRoute {
        kind: OutputKind::Speakers,
        name: "System output".to_owned(),
        uid: None,
        device: 0,
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos::{generation, watch};

#[cfg(any(target_os = "macos", test))]
fn classify(terminals: &[u32], data_source: Option<u32>, name: &str) -> OutputKind {
    let code = |s: &[u8; 4]| u32::from_be_bytes(*s);
    let name = name.to_lowercase();
    if terminals
        .iter()
        .any(|t| [code(b"hdph"), code(b"rspk")].contains(t))
        || data_source.is_some_and(|source| [code(b"hdpn"), code(b"hdph")].contains(&source))
        || ["headphone", "headset", "airpod", "earpod", "навушник"]
            .iter()
            .any(|part| name.contains(part))
    {
        OutputKind::Headphones
    } else if terminals
        .iter()
        .any(|t| [code(b"spkr"), code(b"hdmi"), code(b"dprt"), code(b"lfes")].contains(t))
        // Built-in Mac outputs can report a USB terminal number rather than
        // Core Audio's FourCC. The selected data source still identifies them.
        || data_source.is_some_and(|source| [code(b"ispk"), code(b"espk")].contains(&source))
    {
        OutputKind::Speakers
    } else {
        OutputKind::Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headphone_data_source_overrides_a_shared_builtin_speaker_stream() {
        assert_eq!(
            classify(
                &[u32::from_be_bytes(*b"spkr")],
                Some(u32::from_be_bytes(*b"hdpn")),
                "Built-in Output"
            ),
            OutputKind::Headphones
        );
        assert_eq!(
            classify(&[u32::from_be_bytes(*b"hdph")], None, "MAJOR IV"),
            OutputKind::Headphones
        );
        assert_eq!(
            classify(&[u32::from_be_bytes(*b"hdmi")], None, "Display"),
            OutputKind::Speakers
        );
        assert_eq!(classify(&[], None, "USB Audio"), OutputKind::Unknown);
        assert_eq!(
            classify(
                &[0x0301],
                Some(u32::from_be_bytes(*b"ispk")),
                "MacBook Pro Speakers"
            ),
            OutputKind::Speakers
        );
    }
    #[test]
    fn quiet_outputs_never_raise_master_gain_or_change_speaker_volume() {
        let mut settings = SoundSettings {
            volume_percent: 65,
            headphone_volume_percent: 12,
            ..SoundSettings::default()
        };
        assert_eq!(OutputKind::Speakers.volume(&settings), 65);
        assert_eq!(OutputKind::Headphones.volume(&settings), 12);
        assert_eq!(OutputKind::Unknown.volume(&settings), 12);
        settings.volume_percent = 4;
        assert_eq!(OutputKind::Headphones.volume(&settings), 4);
        settings.headphone_volume_percent = 0;
        assert_eq!(OutputKind::Headphones.volume(&settings), 0);
        assert_eq!(OutputKind::Speakers.volume(&settings), 4);
    }
}
