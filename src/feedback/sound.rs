use anyhow::{Result, bail};

use crate::config::SoundEvent;

#[derive(Clone, Copy)]
pub(super) struct SoundAsset {
    pub event: SoundEvent,
    pub slug: &'static str,
    pub bytes: &'static [u8],
}

pub(super) const fn asset(event: SoundEvent) -> SoundAsset {
    let (slug, bytes): (&str, &[u8]) = match event {
        SoundEvent::AutoCorrect => (
            "auto-correct",
            include_bytes!("../../assets/sounds/auto-correct.wav"),
        ),
        SoundEvent::ManualConversion => (
            "manual-conversion",
            include_bytes!("../../assets/sounds/manual-conversion.wav"),
        ),
        SoundEvent::LayoutSwitch => (
            "layout-switch",
            include_bytes!("../../assets/sounds/layout-switch.wav"),
        ),
        SoundEvent::Pause => ("pause", include_bytes!("../../assets/sounds/pause.wav")),
        SoundEvent::Resume => ("resume", include_bytes!("../../assets/sounds/resume.wav")),
        SoundEvent::Error => ("error", include_bytes!("../../assets/sounds/error.wav")),
    };
    SoundAsset { event, slug, bytes }
}

pub(super) fn play(event: SoundEvent, volume_percent: u8) -> Result<()> {
    if !(1..=100).contains(&volume_percent) {
        bail!("sound volume must be between 1 and 100 percent");
    }
    platform::play(asset(event), volume_percent)
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn scale_pcm16_wav(input: &[u8], volume_percent: u8) -> Result<Vec<u8>> {
    if volume_percent > 100 {
        bail!("sound volume must not exceed 100 percent");
    }
    let data = pcm16_data_range(input)?;
    let mut output = input.to_vec();
    if volume_percent == 100 {
        return Ok(output);
    }

    for sample in output[data].chunks_exact_mut(2) {
        let value = i16::from_le_bytes([sample[0], sample[1]]);
        let scaled = i32::from(value) * i32::from(volume_percent);
        let scaled = if scaled >= 0 {
            (scaled + 50) / 100
        } else {
            (scaled - 50) / 100
        };
        sample.copy_from_slice(&(scaled as i16).to_le_bytes());
    }
    Ok(output)
}

#[cfg(any(target_os = "linux", target_os = "windows", test))]
fn pcm16_data_range(input: &[u8]) -> Result<std::ops::Range<usize>> {
    if input.len() < 12 || &input[..4] != b"RIFF" || &input[8..12] != b"WAVE" {
        bail!("embedded sound is not a RIFF/WAVE file");
    }

    let declared_size = usize::try_from(u32::from_le_bytes(input[4..8].try_into()?))?
        .checked_add(8)
        .ok_or_else(|| anyhow::anyhow!("embedded sound has an invalid RIFF size"))?;
    if declared_size > input.len() {
        bail!("embedded sound is truncated");
    }

    let mut cursor = 12usize;
    let mut supported_format = false;
    let mut data = None;
    while cursor
        .checked_add(8)
        .is_some_and(|end| end <= declared_size)
    {
        let chunk_id = &input[cursor..cursor + 4];
        let chunk_size = usize::try_from(u32::from_le_bytes(
            input[cursor + 4..cursor + 8].try_into()?,
        ))?;
        let chunk_start = cursor + 8;
        let chunk_end = chunk_start
            .checked_add(chunk_size)
            .ok_or_else(|| anyhow::anyhow!("embedded sound chunk size overflowed"))?;
        if chunk_end > declared_size {
            bail!("embedded sound contains a truncated chunk");
        }

        match chunk_id {
            b"fmt " => {
                if chunk_size < 16 {
                    bail!("embedded sound has a short format chunk");
                }
                let audio_format =
                    u16::from_le_bytes(input[chunk_start..chunk_start + 2].try_into()?);
                let channels =
                    u16::from_le_bytes(input[chunk_start + 2..chunk_start + 4].try_into()?);
                let sample_rate =
                    u32::from_le_bytes(input[chunk_start + 4..chunk_start + 8].try_into()?);
                let bits_per_sample =
                    u16::from_le_bytes(input[chunk_start + 14..chunk_start + 16].try_into()?);
                supported_format = audio_format == 1
                    && channels == 1
                    && sample_rate == 44_100
                    && bits_per_sample == 16;
            }
            b"data" => data = Some(chunk_start..chunk_end),
            _ => {}
        }

        cursor = chunk_end
            .checked_add(chunk_size & 1)
            .ok_or_else(|| anyhow::anyhow!("embedded sound chunk padding overflowed"))?;
    }

    if !supported_format {
        bail!("embedded sound must use mono 44.1 kHz PCM16 samples");
    }
    let data = data.ok_or_else(|| anyhow::anyhow!("embedded sound has no data chunk"))?;
    if data.len() % 2 != 0 {
        bail!("embedded PCM16 sound has an odd data length");
    }
    Ok(data)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::cell::RefCell;

    use anyhow::{Result, bail};
    use objc2::{AnyThread, rc::Retained};
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSData;

    use super::SoundAsset;
    use crate::config::SoundEvent;

    struct LoadedSound {
        event: SoundEvent,
        sound: Retained<NSSound>,
    }

    thread_local! {
        static LOADED_SOUNDS: RefCell<Vec<LoadedSound>> = const { RefCell::new(Vec::new()) };
    }

    pub fn play(asset: SoundAsset, volume_percent: u8) -> Result<()> {
        LOADED_SOUNDS.with_borrow_mut(|loaded| {
            for item in loaded.iter() {
                if item.sound.isPlaying() {
                    item.sound.stop();
                }
            }

            let index = if let Some(index) =
                loaded.iter().position(|item| item.event == asset.event)
            {
                index
            } else {
                let data = NSData::with_bytes(asset.bytes);
                let sound = NSSound::initWithData(NSSound::alloc(), &data).ok_or_else(|| {
                    anyhow::anyhow!("AppKit rejected the embedded {} WAV sound", asset.slug)
                })?;
                loaded.push(LoadedSound {
                    event: asset.event,
                    sound,
                });
                loaded.len() - 1
            };

            let sound = &loaded[index].sound;
            sound.setCurrentTime(0.0);
            sound.setVolume(f32::from(volume_percent) / 100.0);
            if !sound.play() {
                bail!("AppKit could not start {} sound playback", asset.slug);
            }
            Ok(())
        })
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{fs, path::PathBuf, process::Command, process::Stdio};

    use anyhow::{Context, Result};
    use directories::ProjectDirs;

    use super::{SoundAsset, scale_pcm16_wav};

    pub fn play(asset: SoundAsset, volume_percent: u8) -> Result<()> {
        let path = cached_sound(asset, volume_percent)?;
        Command::new("canberra-gtk-play")
            .arg("--file")
            .arg(&path)
            .arg("--description")
            .arg(format!("Upyr {}", asset.event.label()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("could not play embedded sound {}", path.display()))?;
        Ok(())
    }

    fn cached_sound(asset: SoundAsset, volume_percent: u8) -> Result<PathBuf> {
        let directories = ProjectDirs::from("dev", "Upyr", "Upyr")
            .context("the operating system did not provide a cache directory")?;
        let directory = directories.cache_dir().join("sounds-v1");
        fs::create_dir_all(&directory)
            .with_context(|| format!("could not create sound cache at {}", directory.display()))?;
        let path = directory.join(format!("{}-{volume_percent}.wav", asset.slug));
        let scaled = scale_pcm16_wav(asset.bytes, volume_percent)?;
        let current = fs::read(&path).ok();
        if current.as_deref() != Some(scaled.as_slice()) {
            fs::write(&path, scaled)
                .with_context(|| format!("could not cache embedded sound at {}", path.display()))?;
        }
        Ok(path)
    }
}

#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod platform {
    use std::{ptr, thread};

    use anyhow::{Context, Result};
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT};

    use super::{SoundAsset, scale_pcm16_wav};

    pub fn play(asset: SoundAsset, volume_percent: u8) -> Result<()> {
        let scaled = scale_pcm16_wav(asset.bytes, volume_percent)?;
        thread::Builder::new()
            .name(format!("upyr-sound-{}-{:?}", asset.slug, asset.event))
            .spawn(move || unsafe {
                // PlaySound is synchronous here so the owned WAV buffer stays
                // alive until winmm has finished reading its memory image.
                PlaySoundW(
                    scaled.as_ptr().cast::<u16>(),
                    ptr::null_mut(),
                    SND_MEMORY | SND_NODEFAULT,
                );
            })
            .context("could not start the Windows sound playback thread")?;
        Ok(())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    use anyhow::{Result, bail};

    use super::SoundAsset;

    pub fn play(_asset: SoundAsset, _volume_percent: u8) -> Result<()> {
        bail!("embedded sound playback is unavailable on this platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_events_have_distinct_embedded_pcm16_assets() {
        let expected = [
            (SoundEvent::AutoCorrect, "auto-correct"),
            (SoundEvent::ManualConversion, "manual-conversion"),
            (SoundEvent::LayoutSwitch, "layout-switch"),
            (SoundEvent::Pause, "pause"),
            (SoundEvent::Resume, "resume"),
            (SoundEvent::Error, "error"),
        ];

        for (event, slug) in expected {
            let asset = asset(event);
            assert_eq!(asset.event, event);
            assert_eq!(asset.slug, slug);
            assert!(asset.bytes.len() > 44);
            let samples = pcm16_data_range(asset.bytes).unwrap();
            assert!(!samples.is_empty());
            assert!(samples.len() <= 44_100 * 2 * 350 / 1_000);
        }
    }

    #[test]
    fn pcm16_scaling_changes_only_sample_data() {
        let input = asset(SoundEvent::Error).bytes;
        let data = pcm16_data_range(input).unwrap();
        let output = scale_pcm16_wav(input, 50).unwrap();

        assert_eq!(&output[..data.start], &input[..data.start]);
        assert_eq!(&output[data.end..], &input[data.end..]);
        for (source, scaled) in input[data.clone()]
            .chunks_exact(2)
            .zip(output[data].chunks_exact(2))
        {
            let source = i16::from_le_bytes(source.try_into().unwrap());
            let scaled = i16::from_le_bytes(scaled.try_into().unwrap());
            let expected = i32::from(source) * 50;
            let expected = if expected >= 0 {
                (expected + 50) / 100
            } else {
                (expected - 50) / 100
            };
            assert_eq!(scaled, expected as i16);
        }
    }

    #[test]
    fn full_volume_preserves_the_embedded_wav_exactly() {
        let input = asset(SoundEvent::Resume).bytes;
        assert_eq!(scale_pcm16_wav(input, 100).unwrap(), input);
    }

    #[test]
    fn rejects_invalid_wav_and_out_of_range_volume() {
        assert!(pcm16_data_range(b"not a wav").is_err());
        assert!(scale_pcm16_wav(asset(SoundEvent::Pause).bytes, 101).is_err());
    }
}
