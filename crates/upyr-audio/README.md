# upyr-audio

Dependency-free procedural sound packs for Upyr.

This crate contains deterministic local audio synthesis used by the Upyr
desktop app for optional layout-switch and key-feedback sounds. Layout-switch
tunes come from a small note sequencer over the same pfxr voice, so each
layout in a user's cycle has a distinct melody. It does not
ship recorded samples or use a network service.
