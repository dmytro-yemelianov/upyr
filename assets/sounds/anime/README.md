# Upyr anime event cues

These six event cues were generated with ElevenLabs Sound Effects (`eleven_text_to_sound_v2`) on 2026-07-17 and edited for frequent, unobtrusive desktop feedback. They contain no speech and are bundled with the application; Upyr never calls ElevenLabs at runtime.

Each request used `prompt_influence = 0.65` and asked for a short, dry, non-looping one-shot with no voice, music bed, ambience, hiss, or reverb:

- `auto-correct.wav` (0.5s prompt): Bright ascending twinkling chime, kawaii video-game confirmation, cute and bouncy, no voice, no music bed, dry one-shot
- `manual-conversion.wav` (0.5s prompt): Playful upward marimba pop with a sparkly shimmer tail, anime UI confirmation, no voice, dry one-shot
- `layout-switch.wav` (0.5s prompt): Quick bouncy synth blip with a cute flutter, anime menu-switch sound, no voice, dry one-shot
- `pause.wav` (0.5s prompt): Soft descending kawaii xylophone dip, gentle and cute, anime pause cue, no voice, dry one-shot
- `resume.wav` (0.5s prompt): Bright ascending kawaii xylophone bounce, cheerful anime resume cue, no voice, dry one-shot
- `error.wav` (0.5s prompt): Cute low descending boop-womp synth tone, gentle comedic anime error cue, no voice, dry one-shot

The selected MP3 generations were converted with FFmpeg to mono 44.1 kHz, 16-bit PCM WAV: high-passed at 70 Hz, low-passed at 14 kHz, trimmed to at most 320 ms, given a 5 ms attack and 45 ms release fade, and loudness-normalized. Raw generated candidates are intentionally not kept in the repository.
