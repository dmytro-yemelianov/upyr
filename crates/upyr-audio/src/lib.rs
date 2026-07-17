//! Dependency-free, deterministic procedural sound packs for Upyr.
//!
//! Every cue is synthesized locally. No recordings, downloaded assets, or
//! network services are used. [`SoundPack::Original`] and
//! [`SoundPack::Arcade`] use short pfxr-style effects. In the Anime pack,
//! character keys retain a light synthesized click while non-character keys
//! use a non-explicit, age-neutral vowel/formant reaction.
//!
//! The pfxr/sfxr DSP foundation is adapted from
//! [`dyco-audio`](https://github.com/dmytro-yemelianov/dyco/tree/31917ec17637aa661676c297a903fff84c849dfd/crates/dyco-audio),
//! itself a Rust port of
//! [`pfxr`](https://github.com/achtaitaipai/pfxr). pfxr is Copyright (c) 2025
//! Charles Cailleteau and is used under the MIT License. The complete notices
//! are in `THIRD_PARTY_NOTICES.md` next to this crate.

#![deny(unsafe_code)]

mod pfxr;
mod prng;
mod voice;
mod wav;

/// Sample rate of every rendered cue.
pub const SAMPLE_RATE: u32 = 44_100;

/// A built-in sound palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundPack {
    /// Soft, restrained synthesized feedback.
    Original,
    /// Brighter retro-game feedback.
    Arcade,
    /// Light clicks for characters and stylized procedural vowel reactions for
    /// non-character keys.
    Anime,
}

/// A semantic application event that can produce feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventCue {
    /// An automatic layout correction completed.
    AutoCorrect,
    /// A user-requested conversion completed.
    ManualConversion,
    /// The active keyboard layout changed.
    LayoutSwitch,
    /// Automatic processing was paused.
    Pause,
    /// Automatic processing resumed.
    Resume,
    /// An operation failed.
    Error,
}

/// A privacy-preserving key category.
///
/// Callers classify a key before rendering; the crate never receives or stores
/// typed characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCue {
    /// Any printable character key.
    Character,
    /// Space bar.
    Space,
    /// Enter or return.
    Enter,
    /// Escape.
    Escape,
    /// Tab.
    Tab,
    /// Option or Alt.
    Option,
    /// Command, Super, or Windows key.
    Command,
    /// Control.
    Control,
    /// Shift.
    Shift,
    /// Caps Lock.
    CapsLock,
    /// Forward delete.
    Delete,
    /// Backspace.
    Backspace,
    /// Arrow, Home, End, Page Up, or Page Down.
    Navigation,
    /// Function key.
    Function,
    /// Any other non-character key.
    Other,
}

/// A sound-worthy application or keyboard cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Application feedback.
    Event(EventCue),
    /// Keyboard feedback.
    Key(KeyCue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Synthesis {
    Pfxr,
    Voice,
}

/// Render a cue to deterministic mono floating-point samples at
/// [`SAMPLE_RATE`].
///
/// Samples are finite and peak-limited to `[-0.42, 0.42]`. The same
/// `(pack, cue, seed)` always produces identical output, which makes it cheap
/// for an application to pre-render and cache a cue bank.
#[must_use]
pub fn render_samples(pack: SoundPack, cue: Cue, seed: u32) -> Vec<f32> {
    let mixed_seed = mix_seed(seed, pack, cue);
    match synthesis_for(pack, cue) {
        Synthesis::Pfxr => pfxr::render(pack, cue, mixed_seed),
        Synthesis::Voice => {
            let Cue::Key(key) = cue else {
                unreachable!("only key cues are routed to voice synthesis")
            };
            voice::render(key, mixed_seed)
        }
    }
}

/// Render a cue as a complete mono 44.1 kHz PCM16 WAV file.
///
/// This delegates to [`render_samples`], so cached floating-point and WAV
/// renderings have the same duration and waveform.
#[must_use]
pub fn render_wav(pack: SoundPack, cue: Cue, seed: u32) -> Vec<u8> {
    wav::encode_pcm16(&render_samples(pack, cue, seed), SAMPLE_RATE)
}

fn synthesis_for(pack: SoundPack, cue: Cue) -> Synthesis {
    match (pack, cue) {
        (SoundPack::Anime, Cue::Key(key)) if key != KeyCue::Character => Synthesis::Voice,
        _ => Synthesis::Pfxr,
    }
}

fn mix_seed(seed: u32, pack: SoundPack, cue: Cue) -> u32 {
    let pack_tag = match pack {
        SoundPack::Original => 0x9e37_79b9,
        SoundPack::Arcade => 0x243f_6a88,
        SoundPack::Anime => 0xb7e1_5163,
    };
    let cue_tag = match cue {
        Cue::Event(event) => 0x100 + event_index(event),
        Cue::Key(key) => 0x200 + key_index(key),
    };

    let mut value = seed ^ pack_tag ^ cue_tag.wrapping_mul(0x85eb_ca6b);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb_352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846c_a68b);
    value ^ (value >> 16)
}

const fn event_index(event: EventCue) -> u32 {
    match event {
        EventCue::AutoCorrect => 0,
        EventCue::ManualConversion => 1,
        EventCue::LayoutSwitch => 2,
        EventCue::Pause => 3,
        EventCue::Resume => 4,
        EventCue::Error => 5,
    }
}

const fn key_index(key: KeyCue) -> u32 {
    match key {
        KeyCue::Character => 0,
        KeyCue::Space => 1,
        KeyCue::Enter => 2,
        KeyCue::Escape => 3,
        KeyCue::Tab => 4,
        KeyCue::Option => 5,
        KeyCue::Command => 6,
        KeyCue::Control => 7,
        KeyCue::Shift => 8,
        KeyCue::CapsLock => 9,
        KeyCue::Delete => 10,
        KeyCue::Backspace => 11,
        KeyCue::Navigation => 12,
        KeyCue::Function => 13,
        KeyCue::Other => 14,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKS: [SoundPack; 3] = [SoundPack::Original, SoundPack::Arcade, SoundPack::Anime];
    const EVENTS: [EventCue; 6] = [
        EventCue::AutoCorrect,
        EventCue::ManualConversion,
        EventCue::LayoutSwitch,
        EventCue::Pause,
        EventCue::Resume,
        EventCue::Error,
    ];
    const KEYS: [KeyCue; 15] = [
        KeyCue::Character,
        KeyCue::Space,
        KeyCue::Enter,
        KeyCue::Escape,
        KeyCue::Tab,
        KeyCue::Option,
        KeyCue::Command,
        KeyCue::Control,
        KeyCue::Shift,
        KeyCue::CapsLock,
        KeyCue::Delete,
        KeyCue::Backspace,
        KeyCue::Navigation,
        KeyCue::Function,
        KeyCue::Other,
    ];

    fn cues() -> impl Iterator<Item = Cue> {
        EVENTS
            .into_iter()
            .map(Cue::Event)
            .chain(KEYS.into_iter().map(Cue::Key))
    }

    #[test]
    fn renders_are_deterministic() {
        for pack in PACKS {
            for cue in cues() {
                assert_eq!(
                    render_samples(pack, cue, 0xdecafbad),
                    render_samples(pack, cue, 0xdecafbad)
                );
                assert_eq!(
                    render_wav(pack, cue, 0xdecafbad),
                    render_wav(pack, cue, 0xdecafbad)
                );
            }
        }
    }

    #[test]
    fn wav_is_mono_pcm16_at_the_documented_rate() {
        for pack in PACKS {
            for cue in cues() {
                let wav = render_wav(pack, cue, 42);
                assert_eq!(&wav[0..4], b"RIFF");
                assert_eq!(&wav[8..12], b"WAVE");
                assert_eq!(&wav[12..16], b"fmt ");
                assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1);
                assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
                assert_eq!(
                    u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
                    SAMPLE_RATE
                );
                assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
                assert_eq!(&wav[36..40], b"data");
                let data_len = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]) as usize;
                assert_eq!(data_len + 44, wav.len());
                assert_eq!(data_len, render_samples(pack, cue, 42).len() * 2);
            }
        }
    }

    #[test]
    fn every_render_is_finite_bounded_and_short() {
        let longest = (SAMPLE_RATE as f32 * 0.32) as usize;
        for pack in PACKS {
            for cue in cues() {
                let samples = render_samples(pack, cue, 7);
                assert!(!samples.is_empty(), "empty {pack:?} {cue:?}");
                assert!(
                    samples.len() <= longest,
                    "too long {pack:?} {cue:?}: {} samples",
                    samples.len()
                );
                assert!(
                    samples
                        .iter()
                        .all(|sample| sample.is_finite() && sample.abs() <= 0.420_001),
                    "invalid sample in {pack:?} {cue:?}"
                );
                let peak = samples
                    .iter()
                    .fold(0.0_f32, |current, sample| current.max(sample.abs()));
                assert!(peak >= 0.02, "inaudibly quiet {pack:?} {cue:?}: {peak}");
            }
        }
    }

    #[test]
    fn pack_cue_and_seed_change_the_waveform() {
        let cue = Cue::Key(KeyCue::Character);
        assert_ne!(
            render_samples(SoundPack::Original, cue, 1),
            render_samples(SoundPack::Original, cue, 2)
        );
        assert_ne!(
            render_samples(SoundPack::Original, cue, 1),
            render_samples(SoundPack::Arcade, cue, 1)
        );
        assert_ne!(
            render_samples(SoundPack::Arcade, cue, 1),
            render_samples(SoundPack::Arcade, Cue::Event(EventCue::AutoCorrect), 1)
        );
    }

    #[test]
    fn voice_synthesis_is_exclusive_to_anime_non_character_keys() {
        for pack in PACKS {
            for event in EVENTS {
                assert_eq!(synthesis_for(pack, Cue::Event(event)), Synthesis::Pfxr);
            }
            for key in KEYS {
                let expected = if pack == SoundPack::Anime && key != KeyCue::Character {
                    Synthesis::Voice
                } else {
                    Synthesis::Pfxr
                };
                assert_eq!(synthesis_for(pack, Cue::Key(key)), expected);
            }
        }
    }
}
