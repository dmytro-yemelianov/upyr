//! Minimal dependency-free PCM16 WAV encoding.

pub(crate) fn encode_pcm16(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_size = u32::try_from(samples.len().saturating_mul(2))
        .expect("procedural cues are too short to exceed WAV's 32-bit data size");
    let mut output = Vec::with_capacity(44 + data_size as usize);

    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&(36 + data_size).to_le_bytes());
    output.extend_from_slice(b"WAVE");
    output.extend_from_slice(b"fmt ");
    output.extend_from_slice(&16_u32.to_le_bytes());
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.extend_from_slice(&sample_rate.to_le_bytes());
    output.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    output.extend_from_slice(&2_u16.to_le_bytes());
    output.extend_from_slice(&16_u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&data_size.to_le_bytes());

    for &sample in samples {
        let finite = if sample.is_finite() { sample } else { 0.0 };
        let encoded = (finite.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
        output.extend_from_slice(&encoded.to_le_bytes());
    }
    output
}
