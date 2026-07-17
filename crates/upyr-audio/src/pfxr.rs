//! Compact pfxr/sfxr-style oscillator, filters, envelope, and cue presets.
//!
//! The sample-loop DSP is adapted from `dyco-audio` commit
//! `31917ec17637aa661676c297a903fff84c849dfd` (MIT), which ports the WebAudio
//! graph used by pfxr.

use std::f64::consts::PI;

use crate::prng::Prng;
use crate::{Cue, EventCue, KeyCue, SAMPLE_RATE, SoundPack};

#[derive(Debug, Clone, Copy)]
enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

#[derive(Debug, Clone, Copy)]
struct Params {
    waveform: Waveform,
    volume: f64,
    attack: f64,
    sustain: f64,
    punch: f64,
    decay: f64,
    frequency: f64,
    pitch_delta: f64,
    vibrato_rate: f64,
    vibrato_depth: f64,
    high_pass: f64,
    low_pass: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            waveform: Waveform::Sine,
            volume: 0.7,
            attack: 0.001,
            sustain: 0.012,
            punch: 0.08,
            decay: 0.035,
            frequency: 800.0,
            pitch_delta: 0.0,
            vibrato_rate: 0.0,
            vibrato_depth: 0.0,
            high_pass: 55.0,
            low_pass: 5_000.0,
        }
    }
}

struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}

impl Biquad {
    fn new(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

pub(crate) fn render(pack: SoundPack, cue: Cue, seed: u32) -> Vec<f32> {
    let params = params_for(pack, cue, seed);
    let samples = synthesize(params);
    let peak = match pack {
        SoundPack::Original => 0.32,
        SoundPack::Arcade => 0.38,
        SoundPack::Anime => 0.30,
    };
    normalize(samples, peak)
}

fn params_for(pack: SoundPack, cue: Cue, seed: u32) -> Params {
    let mut random = Prng::new(seed);
    let mut params = match cue {
        Cue::Event(event) => event_params(event),
        Cue::Key(key) => key_params(key),
    };

    apply_pack(&mut params, pack, cue, &mut random);
    params.frequency = (params.frequency * random.range(0.94, 1.06)).clamp(90.0, 3_800.0);
    params.pitch_delta *= random.range(0.92, 1.08);
    params.sustain = (params.sustain * random.range(0.94, 1.06)).max(0.003);
    params.decay = (params.decay * random.range(0.94, 1.06)).max(0.008);
    params
}

fn event_params(event: EventCue) -> Params {
    let (frequency, pitch_delta, sustain, decay, waveform) = match event {
        EventCue::AutoCorrect => (780.0, 420.0, 0.040, 0.105, Waveform::Triangle),
        EventCue::ManualConversion => (680.0, 330.0, 0.045, 0.110, Waveform::Sine),
        EventCue::LayoutSwitch => (570.0, 210.0, 0.035, 0.090, Waveform::Triangle),
        EventCue::Pause => (650.0, -250.0, 0.035, 0.105, Waveform::Sine),
        EventCue::Resume => (480.0, 310.0, 0.035, 0.105, Waveform::Sine),
        EventCue::Error => (420.0, -170.0, 0.025, 0.125, Waveform::Saw),
    };
    Params {
        waveform,
        frequency,
        pitch_delta,
        sustain,
        decay,
        punch: 0.20,
        low_pass: if event == EventCue::Error {
            2_800.0
        } else {
            5_500.0
        },
        ..Params::default()
    }
}

fn key_params(key: KeyCue) -> Params {
    let (frequency, pitch_delta, sustain, decay) = match key {
        KeyCue::Character => (940.0, -90.0, 0.006, 0.026),
        KeyCue::Space => (620.0, -45.0, 0.010, 0.034),
        KeyCue::Enter => (520.0, 250.0, 0.020, 0.060),
        KeyCue::Escape => (790.0, -260.0, 0.012, 0.052),
        KeyCue::Tab => (690.0, 130.0, 0.010, 0.040),
        KeyCue::Option => (590.0, 80.0, 0.012, 0.042),
        KeyCue::Command => (480.0, 210.0, 0.015, 0.050),
        KeyCue::Control => (540.0, -90.0, 0.012, 0.045),
        KeyCue::Shift => (760.0, 160.0, 0.008, 0.034),
        KeyCue::CapsLock => (510.0, 310.0, 0.018, 0.060),
        KeyCue::Delete => (650.0, -280.0, 0.010, 0.050),
        KeyCue::Backspace => (720.0, -320.0, 0.008, 0.042),
        KeyCue::Navigation => (820.0, 60.0, 0.007, 0.032),
        KeyCue::Function => (560.0, 180.0, 0.014, 0.048),
        KeyCue::Other => (610.0, 0.0, 0.010, 0.040),
    };
    Params {
        waveform: Waveform::Triangle,
        volume: 0.62,
        frequency,
        pitch_delta,
        sustain,
        decay,
        low_pass: 4_600.0,
        ..Params::default()
    }
}

fn apply_pack(params: &mut Params, pack: SoundPack, cue: Cue, random: &mut Prng) {
    match pack {
        SoundPack::Original => {
            params.waveform = if random.index(4) == 0 {
                Waveform::Sine
            } else {
                params.waveform
            };
            params.volume *= 0.86;
            params.low_pass = params.low_pass.min(4_800.0);
        }
        SoundPack::Arcade => {
            params.waveform = if random.index(3) == 0 {
                Waveform::Square
            } else {
                Waveform::Triangle
            };
            params.frequency *= 1.25;
            params.pitch_delta *= 1.35;
            params.decay *= 0.82;
            params.punch = (params.punch + 0.18).min(0.65);
            params.low_pass = 6_800.0;
            params.high_pass = 90.0;
        }
        SoundPack::Anime => {
            // This branch handles event cues and character clicks only. All
            // non-character keys are routed to the formant synthesizer.
            params.waveform = if matches!(cue, Cue::Key(KeyCue::Character)) {
                Waveform::Sine
            } else {
                Waveform::Triangle
            };
            params.frequency *= 1.10;
            params.decay *= 0.90;
            params.vibrato_rate = if matches!(cue, Cue::Event(_)) {
                8.0
            } else {
                0.0
            };
            params.vibrato_depth = if matches!(cue, Cue::Event(_)) {
                7.0
            } else {
                0.0
            };
            params.low_pass = 5_800.0;
        }
    }
}

fn synthesize(params: Params) -> Vec<f32> {
    let sample_rate = f64::from(SAMPLE_RATE);
    let duration = (params.attack + params.sustain + params.decay).min(0.30);
    let sample_count = (duration * sample_rate).ceil() as usize;
    let mut low_pass =
        (params.low_pass < sample_rate / 2.0).then(|| lowpass(params.low_pass, 0.0, sample_rate));
    let mut high_pass =
        (params.high_pass > 0.0).then(|| highpass(params.high_pass, 0.0, sample_rate));
    let mut samples = Vec::with_capacity(sample_count);
    let mut phase = 0.0;

    for index in 0..sample_count {
        let time = index as f64 / sample_rate;
        let progress = (time / duration).clamp(0.0, 1.0);
        let base_frequency = params.frequency + params.pitch_delta * progress;
        let vibrato = params.vibrato_depth * (2.0 * PI * params.vibrato_rate * time).sin();
        let frequency = (base_frequency + vibrato).clamp(20.0, 4_000.0);
        phase += frequency / sample_rate;

        let mut sample = oscillator(params.waveform, phase) / 3.0;
        if let Some(filter) = low_pass.as_mut() {
            sample = filter.process(sample);
        }
        if let Some(filter) = high_pass.as_mut() {
            sample = filter.process(sample);
        }
        sample *= envelope(time, params) * params.volume;
        samples.push(sample as f32);
    }
    samples
}

fn oscillator(waveform: Waveform, phase: f64) -> f64 {
    let phase = phase.rem_euclid(1.0);
    match waveform {
        Waveform::Sine => (2.0 * PI * phase).sin(),
        Waveform::Saw => 2.0 * phase - 1.0,
        Waveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
    }
}

fn envelope(time: f64, params: Params) -> f64 {
    if time < params.attack {
        return if params.attack > 0.0 {
            (1.0 - params.punch) * time / params.attack
        } else {
            1.0
        };
    }
    if time < params.attack + params.sustain {
        return if params.sustain > 0.0 {
            1.0 - params.punch * (time - params.attack) / params.sustain
        } else {
            1.0 - params.punch
        };
    }
    if time <= params.attack + params.sustain + params.decay {
        return if params.decay > 0.0 {
            (1.0 - params.punch) * (1.0 - (time - params.attack - params.sustain) / params.decay)
        } else {
            0.0
        };
    }
    0.0
}

fn lowpass(frequency: f64, resonance_db: f64, sample_rate: f64) -> Biquad {
    let angular = 2.0 * PI * frequency / sample_rate;
    let (sin, cos) = (angular.sin(), angular.cos());
    let alpha = sin / (2.0 * 10_f64.powf(resonance_db / 20.0));
    Biquad::new(
        (1.0 - cos) / 2.0,
        1.0 - cos,
        (1.0 - cos) / 2.0,
        1.0 + alpha,
        -2.0 * cos,
        1.0 - alpha,
    )
}

fn highpass(frequency: f64, resonance_db: f64, sample_rate: f64) -> Biquad {
    let angular = 2.0 * PI * frequency / sample_rate;
    let (sin, cos) = (angular.sin(), angular.cos());
    let alpha = sin / (2.0 * 10_f64.powf(resonance_db / 20.0));
    Biquad::new(
        (1.0 + cos) / 2.0,
        -(1.0 + cos),
        (1.0 + cos) / 2.0,
        1.0 + alpha,
        -2.0 * cos,
        1.0 - alpha,
    )
}

fn normalize(mut samples: Vec<f32>, target_peak: f32) -> Vec<f32> {
    let peak = samples
        .iter()
        .fold(0.0_f32, |current, sample| current.max(sample.abs()));
    if peak > 0.0 {
        let gain = (target_peak / peak).min(1.8);
        for sample in &mut samples {
            *sample = (*sample * gain).clamp(-target_peak, target_peak);
        }
    }
    samples
}
