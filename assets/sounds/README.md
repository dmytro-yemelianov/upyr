# Upyr sound theme

These six local UI cues were generated with ElevenLabs Sound Effects
(`eleven_text_to_sound_v2`) on 2026-07-17 and edited for frequent, unobtrusive
desktop feedback. They contain no speech and are bundled with the application;
Upyr never calls ElevenLabs at runtime.

The source requests used a 0.5 second, non-looping generation with
`prompt_influence = 0.65`. Each prompt described a single dry desktop one-shot
with no voice, music, ambience, hiss, or reverb:

- `auto-correct.wav`: warm two-note upward wood/glass confirmation
- `manual-conversion.wav`: tactile paper flip, rounded click, soft sparkle
- `layout-switch.wav`: short lateral whoosh ending in a muted glass tick
- `pause.wav`: two gently descending muted notes
- `resume.wav`: two gently ascending muted notes
- `error.wav`: soft low wooden knock with a muted downward tail

The selected MP3 generations were converted with FFmpeg to mono 44.1 kHz,
16-bit PCM WAV. They were high-passed at 70 Hz, low-passed at 14 kHz, trimmed
to 220-320 ms, given 5 ms attack and 40-50 ms release fades, and gain-matched
with a -12 dBFS peak ceiling. Raw generated candidates are intentionally not
kept in the repository.

To preserve reproducibility, validate committed assets with:

```sh
ffprobe -v error \
  -show_entries stream=codec_name,sample_rate,channels,bits_per_sample:format=duration \
  -of default=noprint_wrappers=1 assets/sounds/auto-correct.wav
```
