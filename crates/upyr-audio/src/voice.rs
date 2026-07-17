//! Procedural vowel/formant reactions for Anime non-character key cues.
//!
//! This is synthesis, not sampled speech: a deterministic glottal source is
//! passed through three resonant formant filters and a short envelope.

use std::f64::consts::PI;

use crate::prng::Prng;
use crate::{KeyCue, SAMPLE_RATE};

#[derive(Clone, Copy)]
enum Vowel {
    A,
    E,
    I,
    O,
    U,
}

impl Vowel {
    const fn formants(self) -> [(f64, f64, f64); 3] {
        match self {
            // (center frequency, bandwidth, relative level)
            Self::A => [
                (760.0, 105.0, 1.00),
                (1_180.0, 135.0, 0.58),
                (2_750.0, 230.0, 0.23),
            ],
            Self::E => [
                (500.0, 95.0, 1.00),
                (1_720.0, 150.0, 0.52),
                (2_480.0, 220.0, 0.20),
            ],
            Self::I => [
                (330.0, 80.0, 1.00),
                (2_080.0, 170.0, 0.48),
                (2_900.0, 250.0, 0.18),
            ],
            Self::O => [
                (520.0, 95.0, 1.00),
                (900.0, 125.0, 0.62),
                (2_520.0, 230.0, 0.18),
            ],
            Self::U => [
                (360.0, 85.0, 1.00),
                (720.0, 110.0, 0.65),
                (2_350.0, 220.0, 0.17),
            ],
        }
    }
}

struct Profile {
    vowel: Vowel,
    duration: f64,
    pitch: f64,
    glide: f64,
}

struct Resonator {
    b0: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Resonator {
    fn new(frequency: f64, bandwidth: f64, sample_rate: f64) -> Self {
        let angular = 2.0 * PI * frequency / sample_rate;
        let quality = (frequency / bandwidth).max(0.5);
        let alpha = angular.sin() / (2.0 * quality);
        let a0 = 1.0 + alpha;
        Self {
            b0: alpha / a0,
            b2: -alpha / a0,
            a1: -2.0 * angular.cos() / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.b2 * self.x2 - self.a1 * self.y1 - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

pub(crate) fn render(key: KeyCue, seed: u32) -> Vec<f32> {
    debug_assert_ne!(key, KeyCue::Character);
    let mut random = Prng::new(seed);
    let mut profile = profile_for(key);
    profile.duration *= random.range(0.94, 1.06);
    profile.pitch *= random.range(0.96, 1.04);
    profile.glide += random.range(-7.0, 7.0);

    let sample_rate = f64::from(SAMPLE_RATE);
    let duration = profile.duration.min(0.25);
    let sample_count = (duration * sample_rate).ceil() as usize;
    let formants = profile.vowel.formants();
    let mut filters = formants.map(|(frequency, bandwidth, _)| {
        Resonator::new(
            frequency * random.range(0.985, 1.015),
            bandwidth,
            sample_rate,
        )
    });
    let mut samples = Vec::with_capacity(sample_count);
    let mut phase = 0.0_f64;

    for index in 0..sample_count {
        let time = index as f64 / sample_rate;
        let progress = (time / duration).clamp(0.0, 1.0);
        let vibrato_fade = smoothstep((progress - 0.12) / 0.35);
        let vibrato = 3.2 * vibrato_fade * (2.0 * PI * 5.4 * time).sin();
        let expressive_arc = 8.0 * (PI * progress).sin();
        let frequency = (profile.pitch + profile.glide * progress + vibrato + expressive_arc)
            .clamp(130.0, 310.0);
        phase += frequency / sample_rate;

        // A band-limited-enough glottal source for these short, quiet cues.
        let mut source = 0.0;
        for harmonic in 1..=12 {
            let harmonic_frequency = frequency * f64::from(harmonic);
            if harmonic_frequency >= 8_500.0 {
                break;
            }
            source += (2.0 * PI * phase * f64::from(harmonic)).sin() / f64::from(harmonic);
        }
        source *= 0.42;
        source += random.signed_unit() * 0.012;

        let voiced = filters
            .iter_mut()
            .zip(formants)
            .map(|(filter, (_, _, level))| filter.process(source) * level)
            .sum::<f64>();
        let amplitude = reaction_envelope(time, duration);
        samples.push((voiced * amplitude) as f32);
    }

    normalize(samples, 0.34)
}

fn profile_for(key: KeyCue) -> Profile {
    let (vowel, duration, pitch, glide) = match key {
        KeyCue::Character => unreachable!("characters use pfxr clicks"),
        KeyCue::Space => (Vowel::A, 0.105, 205.0, 12.0),
        KeyCue::Enter => (Vowel::O, 0.205, 195.0, 34.0),
        KeyCue::Escape => (Vowel::E, 0.150, 218.0, -38.0),
        KeyCue::Tab => (Vowel::I, 0.115, 220.0, 18.0),
        KeyCue::Option => (Vowel::U, 0.135, 190.0, 15.0),
        KeyCue::Command => (Vowel::A, 0.165, 200.0, 28.0),
        KeyCue::Control => (Vowel::U, 0.130, 188.0, -12.0),
        KeyCue::Shift => (Vowel::I, 0.095, 228.0, 24.0),
        KeyCue::CapsLock => (Vowel::O, 0.190, 194.0, 42.0),
        KeyCue::Delete => (Vowel::E, 0.135, 215.0, -50.0),
        KeyCue::Backspace => (Vowel::A, 0.110, 210.0, -44.0),
        KeyCue::Navigation => (Vowel::I, 0.090, 224.0, 8.0),
        KeyCue::Function => (Vowel::O, 0.145, 198.0, 22.0),
        KeyCue::Other => (Vowel::E, 0.120, 208.0, 0.0),
    };
    Profile {
        vowel,
        duration,
        pitch,
        glide,
    }
}

fn reaction_envelope(time: f64, duration: f64) -> f64 {
    let attack = smoothstep(time / 0.014);
    let release = smoothstep((duration - time) / 0.045);
    let progress = (time / duration).clamp(0.0, 1.0);
    attack * release * (1.0 - 0.12 * progress)
}

fn smoothstep(value: f64) -> f64 {
    let value = value.clamp(0.0, 1.0);
    value * value * (3.0 - 2.0 * value)
}

fn normalize(mut samples: Vec<f32>, target_peak: f32) -> Vec<f32> {
    let peak = samples
        .iter()
        .fold(0.0_f32, |current, sample| current.max(sample.abs()));
    if peak > 0.0 {
        let gain = (target_peak / peak).min(2.4);
        for sample in &mut samples {
            *sample = (*sample * gain).clamp(-target_peak, target_peak);
        }
    }
    samples
}
