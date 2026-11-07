//! A small note sequencer on top of the pfxr voice, used for per-layout
//! switch tunes.
//!
//! pfxr renders one note whose pitch can only slide in a straight line. A
//! layout tune needs separate notes, so each note is rendered as its own pfxr
//! voice and mixed in at its onset. Tunes differ by shape (rising, falling,
//! turning) and length rather than by absolute pitch, which most listeners
//! cannot name without a reference. Every note comes from the C-major
//! pentatonic scale so any tune sounds consonant and none resembles an error.

use crate::SoundPack;
use crate::pfxr::{self, Params, Waveform};

const C5: f64 = 523.25;
const D5: f64 = 587.33;
const E5: f64 = 659.25;
const G5: f64 = 783.99;
const A5: f64 = 880.00;

/// Tunes in the order layouts receive them. The first two are the most
/// distinct pair (up versus down), because most people cycle two layouts.
pub(crate) const TUNES: [&[f64]; 6] = [
    &[C5, G5],     // up
    &[E5, C5],     // down
    &[C5, E5, G5], // up, up
    &[E5, A5, E5], // up, down
    &[G5, E5, C5], // down, down
    &[G5, D5, G5], // down, up
];

/// Renders the tune for the layout at `slot`, wrapping after the last tune.
pub(crate) fn render_layout_tune(pack: SoundPack, slot: usize) -> Vec<f32> {
    let notes = TUNES[slot % TUNES.len()];
    // Three notes sit a little closer so the tune stays inside the cue limit.
    let spacing = if notes.len() > 2 { 0.09 } else { 0.10 };
    let voice = voice(pack);
    let sample_rate = f64::from(crate::SAMPLE_RATE);
    let mut mix: Vec<f32> = Vec::new();

    for (index, &frequency) in notes.iter().enumerate() {
        let rendered = pfxr::synthesize(Params { frequency, ..voice });
        let offset = (index as f64 * spacing * sample_rate).round() as usize;
        if mix.len() < offset + rendered.len() {
            mix.resize(offset + rendered.len(), 0.0);
        }
        for (target, sample) in mix[offset..].iter_mut().zip(rendered) {
            *target += sample;
        }
    }
    pfxr::normalize(mix, pfxr::peak_for(pack))
}

/// The pack changes only the voice; the tune itself stays the same, so a
/// layout keeps its tune when the user changes packs.
fn voice(pack: SoundPack) -> Params {
    let soft = Params {
        waveform: Waveform::Triangle,
        volume: 0.7,
        attack: 0.004,
        sustain: 0.015,
        punch: 0.30,
        decay: 0.11,
        frequency: 0.0,
        pitch_delta: 0.0,
        vibrato_rate: 0.0,
        vibrato_depth: 0.0,
        high_pass: 120.0,
        low_pass: 2_600.0,
    };
    match pack {
        SoundPack::Original => soft,
        SoundPack::Arcade => Params {
            waveform: Waveform::Square,
            decay: 0.085,
            punch: 0.40,
            high_pass: 150.0,
            low_pass: 5_000.0,
            ..soft
        },
        SoundPack::Anime => Params {
            waveform: Waveform::Sine,
            decay: 0.12,
            vibrato_rate: 9.0,
            vibrato_depth: 8.0,
            low_pass: 6_000.0,
            ..soft
        },
    }
}
