#!/usr/bin/env python3
"""Render a pack's Upyr event cues with the ElevenLabs Sound Effects API.

This is a manual, developer-run tool, exactly like `generate_ngram_model.py`:
it is never invoked by the desktop app or WASM runtime, which stay fully
offline. It requests one short sound effect per application event, then
normalizes the result locally with `ffmpeg` into the mono 44.1 kHz, 16-bit PCM
WAV format the app embeds via `include_bytes!` and plays without any network
access. See `assets/sounds/README.md` for how the Original pack's cues were
produced by the same method.

Usage:
    export ELEVENLABS_API_KEY=...
    python3 tools/generate_event_sound_pack.py anime

Requires `ffmpeg` on PATH and an ElevenLabs API key with Sound Effects access.
Run it yourself in your own shell so the API key never leaves your machine.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
API_URL = "https://api.elevenlabs.io/v1/sound-generation"
MODEL_ID = "eleven_text_to_sound_v2"
OUTPUT_FORMAT = "mp3_44100_128"
PROMPT_INFLUENCE = 0.65
MAX_CUE_SECONDS = 0.32

# pack -> event slug -> (prompt, requested generation duration in seconds)
PACKS: dict[str, dict[str, tuple[str, float]]] = {
    "anime": {
        "auto-correct": (
            "Bright ascending twinkling chime, kawaii video-game confirmation, "
            "cute and bouncy, no voice, no music bed, dry one-shot",
            0.5,
        ),
        "manual-conversion": (
            "Playful upward marimba pop with a sparkly shimmer tail, anime UI "
            "confirmation, no voice, dry one-shot",
            0.5,
        ),
        "layout-switch": (
            "Quick bouncy synth blip with a cute flutter, anime menu-switch "
            "sound, no voice, dry one-shot",
            0.5,
        ),
        "pause": (
            "Soft descending kawaii xylophone dip, gentle and cute, anime "
            "pause cue, no voice, dry one-shot",
            0.5,
        ),
        "resume": (
            "Bright ascending kawaii xylophone bounce, cheerful anime resume "
            "cue, no voice, dry one-shot",
            0.5,
        ),
        "error": (
            "Cute low descending boop-womp synth tone, gentle comedic anime "
            "error cue, no voice, dry one-shot",
            0.5,
        ),
    },
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pack", choices=sorted(PACKS), help="sound pack to render")
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="output directory (default: assets/sounds/<pack>)",
    )
    return parser.parse_args()


def request_sound_effect(api_key: str, text: str, duration_seconds: float) -> bytes:
    payload = json.dumps(
        {
            "text": text,
            "duration_seconds": duration_seconds,
            "prompt_influence": PROMPT_INFLUENCE,
            "model_id": MODEL_ID,
        }
    ).encode("utf-8")
    request = urllib.request.Request(
        f"{API_URL}?output_format={OUTPUT_FORMAT}",
        data=payload,
        method="POST",
        headers={"xi-api-key": api_key, "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        detail = error.read().decode("utf-8", errors="replace")
        raise SystemExit(f"ElevenLabs request failed ({error.code}): {detail}") from error


def normalize_to_wav(source_bytes: bytes, destination: Path) -> None:
    with tempfile.NamedTemporaryFile(suffix=".mp3") as source:
        source.write(source_bytes)
        source.flush()
        try:
            subprocess.run(
                [
                    "ffmpeg",
                    "-y",
                    "-i",
                    source.name,
                    "-af",
                    "highpass=f=70,lowpass=f=14000,"
                    "afade=t=in:st=0:d=0.005,afade=t=out:st=0.18:d=0.045,"
                    "loudnorm=I=-16:TP=-9:LRA=6,volume=-3dB",
                    "-ar",
                    "44100",
                    "-ac",
                    "1",
                    "-sample_fmt",
                    "s16",
                    "-t",
                    str(MAX_CUE_SECONDS),
                    str(destination),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
        except subprocess.CalledProcessError as error:
            detail = (error.stderr or error.stdout or "unknown ffmpeg error").strip()
            raise SystemExit(f"ffmpeg normalization failed:\n{detail}") from error


def write_readme(directory: Path, pack: str, prompts: dict[str, tuple[str, float]]) -> None:
    today = datetime.date.today().isoformat()
    lines = [
        f"# Upyr {pack} event cues",
        "",
        f"These six event cues were generated with ElevenLabs Sound Effects "
        f"(`{MODEL_ID}`) on {today} and edited for frequent, unobtrusive desktop "
        "feedback. They contain no speech and are bundled with the application; "
        "Upyr never calls ElevenLabs at runtime.",
        "",
        f"Each request used `prompt_influence = {PROMPT_INFLUENCE}` and asked for "
        "a short, dry, non-looping one-shot with no voice, music bed, ambience, "
        "hiss, or reverb:",
        "",
    ]
    for slug, (prompt, duration) in prompts.items():
        lines.append(f"- `{slug}.wav` ({duration}s prompt): {prompt}")
    lines += [
        "",
        "The selected MP3 generations were converted with FFmpeg to mono "
        "44.1 kHz, 16-bit PCM WAV: high-passed at 70 Hz, low-passed at 14 kHz, "
        f"trimmed to at most {MAX_CUE_SECONDS * 1000:.0f} ms, given a 5 ms attack "
        "and 45 ms release fade, and loudness-normalized. Raw generated "
        "candidates are intentionally not kept in the repository.",
    ]
    (directory / "README.md").write_text("\n".join(lines) + "\n")


def main() -> int:
    args = parse_args()
    api_key = os.environ.get("ELEVENLABS_API_KEY")
    if not api_key:
        raise SystemExit("set ELEVENLABS_API_KEY before running this tool")

    prompts = PACKS[args.pack]
    directory = args.out or (ROOT / "assets" / "sounds" / args.pack)
    directory.mkdir(parents=True, exist_ok=True)

    for slug, (prompt, duration) in prompts.items():
        print(f"generating {slug}.wav ...", file=sys.stderr)
        audio = request_sound_effect(api_key, prompt, duration)
        normalize_to_wav(audio, directory / f"{slug}.wav")

    write_readme(directory, args.pack, prompts)
    print(f"wrote {len(prompts)} cues to {directory}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
