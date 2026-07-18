use std::{
    cell::RefCell,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use anyhow::{Result, bail};
use upyr_audio::{Cue, EventCue, KeyCue};

use crate::config::{SoundEvent, SoundPack};

const KEY_VARIANTS: u32 = 6;
static KEY_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static PREWARMED_PACKS: AtomicU8 = AtomicU8::new(0);
type SoundCache = Mutex<Vec<(SoundId, Arc<[u8]>)>>;
static GENERATED_SOUNDS: OnceLock<SoundCache> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SoundId {
    Event(SoundPack, SoundEvent),
    Key(SoundPack, KeyCue, u8),
}

#[derive(Clone)]
pub(super) struct SoundAsset {
    id: SoundId,
    slug: String,
    bytes: Arc<[u8]>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    keyboard: bool,
}

fn event_asset(event: SoundEvent, pack: SoundPack) -> SoundAsset {
    let cue = event_cue(event);
    let seed = 0x5550_5952 ^ event_index(event).wrapping_mul(0x9e37_79b9);
    SoundAsset {
        id: SoundId::Event(pack, event),
        slug: format!("{}-{}", pack_slug(pack), event_slug(event)),
        bytes: generated_sound(SoundId::Event(pack, event), pack, Cue::Event(cue), seed),
        keyboard: false,
    }
}

fn key_asset(cue: KeyCue, pack: SoundPack) -> SoundAsset {
    let variant = (KEY_SEQUENCE.fetch_add(1, Ordering::Relaxed) % KEY_VARIANTS) as u8;
    let seed = key_seed(cue, variant);
    SoundAsset {
        id: SoundId::Key(pack, cue, variant),
        slug: format!("{}-key-{}-{variant}", pack_slug(pack), key_cue_slug(cue)),
        bytes: generated_sound(SoundId::Key(pack, cue, variant), pack, Cue::Key(cue), seed),
        keyboard: true,
    }
}

pub(super) fn play_event(event: SoundEvent, pack: SoundPack, volume_percent: u8) -> Result<()> {
    if !(1..=100).contains(&volume_percent) {
        bail!("sound volume must be between 1 and 100 percent");
    }
    platform::play(event_asset(event, pack), volume_percent)
}

pub(super) fn play_key(
    cue: KeyCue,
    pack: SoundPack,
    volume_percent: u8,
    preview: bool,
) -> Result<()> {
    if !(1..=100).contains(&volume_percent) {
        bail!("sound volume must be between 1 and 100 percent");
    }
    if !preview && !KEY_RATE_LIMITER.with_borrow_mut(|limiter| limiter.allow(pack, cue)) {
        return Ok(());
    }
    platform::play(key_asset(cue, pack), volume_percent)
}

pub(super) fn prewarm(pack: SoundPack) {
    let bit = 1_u8 << pack_index(pack);
    if PREWARMED_PACKS.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
        return;
    }
    if thread::Builder::new()
        .name(format!("upyr-sound-prewarm-{}", pack_slug(pack)))
        .spawn(move || {
            for cue in ALL_KEY_CUES {
                for variant in 0..KEY_VARIANTS as u8 {
                    let id = SoundId::Key(pack, cue, variant);
                    generated_sound(id, pack, Cue::Key(cue), key_seed(cue, variant));
                }
            }
            for event in SoundEvent::ALL {
                let id = SoundId::Event(pack, event);
                let seed = 0x5550_5952 ^ event_index(event).wrapping_mul(0x9e37_79b9);
                generated_sound(id, pack, Cue::Event(event_cue(event)), seed);
            }
        })
        .is_err()
    {
        PREWARMED_PACKS.fetch_and(!bit, Ordering::AcqRel);
    }
}

fn generated_sound(id: SoundId, pack: SoundPack, cue: Cue, seed: u32) -> Arc<[u8]> {
    let cache = GENERATED_SOUNDS.get_or_init(|| Mutex::new(Vec::new()));
    {
        let cached = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, bytes)) = cached.iter().find(|(cached_id, _)| *cached_id == id) {
            return Arc::clone(bytes);
        }
    }

    let rendered: Arc<[u8]> = upyr_audio::render_wav(audio_pack(pack), cue, seed).into();
    let mut cached = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, bytes)) = cached.iter().find(|(cached_id, _)| *cached_id == id) {
        return Arc::clone(bytes);
    }
    cached.push((id, Arc::clone(&rendered)));
    rendered
}

const ALL_KEY_CUES: [KeyCue; 15] = [
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

const fn key_seed(cue: KeyCue, variant: u8) -> u32 {
    0x4b45_5953
        ^ key_cue_index(cue).wrapping_mul(0x85eb_ca6b)
        ^ (variant as u32).wrapping_mul(0xc2b2_ae35)
}

struct KeyRateLimiter {
    last: [Option<Instant>; 15],
    last_vocal: Option<Instant>,
}

impl KeyRateLimiter {
    const fn new() -> Self {
        Self {
            last: [None; 15],
            last_vocal: None,
        }
    }

    fn allow(&mut self, pack: SoundPack, cue: KeyCue) -> bool {
        let now = Instant::now();
        let cooldown = match cue {
            KeyCue::Character => Duration::from_millis(7),
            KeyCue::Backspace | KeyCue::Delete => Duration::from_millis(28),
            KeyCue::Space | KeyCue::Enter | KeyCue::Tab | KeyCue::Navigation => {
                Duration::from_millis(35)
            }
            _ => Duration::from_millis(65),
        };
        let slot = &mut self.last[key_cue_index(cue) as usize];
        if slot.is_some_and(|last| now.duration_since(last) < cooldown) {
            return false;
        }
        if pack == SoundPack::Anime && cue != KeyCue::Character {
            let vocal_cooldown = Duration::from_millis(95);
            if self
                .last_vocal
                .is_some_and(|last| now.duration_since(last) < vocal_cooldown)
            {
                return false;
            }
            self.last_vocal = Some(now);
        }
        *slot = Some(now);
        true
    }
}

thread_local! {
    static KEY_RATE_LIMITER: RefCell<KeyRateLimiter> = const { RefCell::new(KeyRateLimiter::new()) };
}

const fn audio_pack(pack: SoundPack) -> upyr_audio::SoundPack {
    match pack {
        SoundPack::Original => upyr_audio::SoundPack::Original,
        SoundPack::Arcade => upyr_audio::SoundPack::Arcade,
        SoundPack::Anime => upyr_audio::SoundPack::Anime,
    }
}

const fn event_cue(event: SoundEvent) -> EventCue {
    match event {
        SoundEvent::AutoCorrect => EventCue::AutoCorrect,
        SoundEvent::ManualConversion => EventCue::ManualConversion,
        SoundEvent::LayoutSwitch => EventCue::LayoutSwitch,
        SoundEvent::Pause => EventCue::Pause,
        SoundEvent::Resume => EventCue::Resume,
        SoundEvent::Error => EventCue::Error,
    }
}

const fn event_index(event: SoundEvent) -> u32 {
    match event {
        SoundEvent::AutoCorrect => 0,
        SoundEvent::ManualConversion => 1,
        SoundEvent::LayoutSwitch => 2,
        SoundEvent::Pause => 3,
        SoundEvent::Resume => 4,
        SoundEvent::Error => 5,
    }
}

const fn event_slug(event: SoundEvent) -> &'static str {
    match event {
        SoundEvent::AutoCorrect => "auto-correct",
        SoundEvent::ManualConversion => "manual-conversion",
        SoundEvent::LayoutSwitch => "layout-switch",
        SoundEvent::Pause => "pause",
        SoundEvent::Resume => "resume",
        SoundEvent::Error => "error",
    }
}

const fn pack_slug(pack: SoundPack) -> &'static str {
    match pack {
        SoundPack::Original => "original",
        SoundPack::Arcade => "arcade",
        SoundPack::Anime => "anime",
    }
}

const fn pack_index(pack: SoundPack) -> u8 {
    match pack {
        SoundPack::Original => 0,
        SoundPack::Arcade => 1,
        SoundPack::Anime => 2,
    }
}

const fn key_cue_index(cue: KeyCue) -> u32 {
    match cue {
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

const fn key_cue_slug(cue: KeyCue) -> &'static str {
    match cue {
        KeyCue::Character => "character",
        KeyCue::Space => "space",
        KeyCue::Enter => "enter",
        KeyCue::Escape => "escape",
        KeyCue::Tab => "tab",
        KeyCue::Option => "option",
        KeyCue::Command => "command",
        KeyCue::Control => "control",
        KeyCue::Shift => "shift",
        KeyCue::CapsLock => "caps-lock",
        KeyCue::Delete => "delete",
        KeyCue::Backspace => "backspace",
        KeyCue::Navigation => "navigation",
        KeyCue::Function => "function",
        KeyCue::Other => "other",
    }
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

    use super::{SoundAsset, SoundId};

    struct LoadedSound {
        id: SoundId,
        sound: Retained<NSSound>,
    }

    thread_local! {
        static LOADED_SOUNDS: RefCell<Vec<LoadedSound>> = const { RefCell::new(Vec::new()) };
    }

    pub fn play(asset: SoundAsset, volume_percent: u8) -> Result<()> {
        LOADED_SOUNDS.with_borrow_mut(|loaded| {
            if !asset.keyboard {
                for item in loaded.iter() {
                    if item.sound.isPlaying() {
                        item.sound.stop();
                    }
                }
            }

            let index = if let Some(index) = loaded.iter().position(|item| item.id == asset.id) {
                index
            } else {
                let data = NSData::with_bytes(asset.bytes.as_ref());
                let sound = NSSound::initWithData(NSSound::alloc(), &data).ok_or_else(|| {
                    anyhow::anyhow!("AppKit rejected the embedded {} WAV sound", asset.slug)
                })?;
                loaded.push(LoadedSound {
                    id: asset.id,
                    sound,
                });
                loaded.len() - 1
            };

            let sound = &loaded[index].sound;
            if sound.isPlaying() {
                sound.stop();
            }
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
        let path = cached_sound(&asset, volume_percent)?;
        Command::new("canberra-gtk-play")
            .arg("--file")
            .arg(&path)
            .arg("--description")
            .arg(format!("Upyr {:?}", asset.id))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("could not play embedded sound {}", path.display()))?;
        Ok(())
    }

    fn cached_sound(asset: &SoundAsset, volume_percent: u8) -> Result<PathBuf> {
        let directories = ProjectDirs::from("dev", "Upyr", "Upyr")
            .context("the operating system did not provide a cache directory")?;
        let directory = directories.cache_dir().join("sounds-v2");
        fs::create_dir_all(&directory)
            .with_context(|| format!("could not create sound cache at {}", directory.display()))?;
        let path = directory.join(format!("{}-{volume_percent}.wav", asset.slug));
        let scaled = scale_pcm16_wav(asset.bytes.as_ref(), volume_percent)?;
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
        let scaled = scale_pcm16_wav(asset.bytes.as_ref(), volume_percent)?;
        thread::Builder::new()
            .name(format!("upyr-sound-{}-{:?}", asset.slug, asset.id))
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
    fn all_packs_generate_distinct_pcm16_event_assets() {
        let packs = [SoundPack::Original, SoundPack::Arcade, SoundPack::Anime];
        let expected = [
            (SoundEvent::AutoCorrect, "auto-correct"),
            (SoundEvent::ManualConversion, "manual-conversion"),
            (SoundEvent::LayoutSwitch, "layout-switch"),
            (SoundEvent::Pause, "pause"),
            (SoundEvent::Resume, "resume"),
            (SoundEvent::Error, "error"),
        ];
        let mut waveforms: Vec<Arc<[u8]>> = Vec::new();

        for pack in packs {
            for (event, slug) in expected {
                let asset = event_asset(event, pack);
                assert_eq!(asset.id, SoundId::Event(pack, event));
                assert_eq!(asset.slug, format!("{}-{slug}", pack_slug(pack)));
                assert!(asset.bytes.len() > 44);
                let samples = pcm16_data_range(asset.bytes.as_ref()).unwrap();
                assert!(!samples.is_empty());
                assert!(samples.len() <= 44_100 * 2 * 500 / 1_000);
                assert!(
                    waveforms
                        .iter()
                        .all(|existing| existing.as_ref() != asset.bytes.as_ref()),
                    "{pack:?} {event:?} unexpectedly reused another event waveform"
                );
                waveforms.push(asset.bytes);
            }
        }

        assert_eq!(waveforms.len(), packs.len() * expected.len());
    }

    #[test]
    fn pcm16_scaling_changes_only_sample_data() {
        let asset = event_asset(SoundEvent::Error, SoundPack::Original);
        let input = asset.bytes.as_ref();
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
    fn full_volume_preserves_the_generated_wav_exactly() {
        let asset = event_asset(SoundEvent::Resume, SoundPack::Original);
        let input = asset.bytes.as_ref();
        assert_eq!(scale_pcm16_wav(input, 100).unwrap().as_slice(), input);
    }

    #[test]
    fn rejects_invalid_wav_and_out_of_range_volume() {
        assert!(pcm16_data_range(b"not a wav").is_err());
        assert!(
            scale_pcm16_wav(
                event_asset(SoundEvent::Pause, SoundPack::Original)
                    .bytes
                    .as_ref(),
                101
            )
            .is_err()
        );
    }

    #[test]
    fn physical_keys_map_to_distinct_control_cues() {
        for cue in ALL_KEY_CUES {
            assert_eq!(
                key_cue_index(cue) as usize,
                ALL_KEY_CUES.iter().position(|item| *item == cue).unwrap()
            );
        }
    }

    #[test]
    fn generated_assets_include_pack_and_variant_in_their_identity() {
        let event = event_asset(SoundEvent::LayoutSwitch, SoundPack::Arcade);
        assert_eq!(
            event.id,
            SoundId::Event(SoundPack::Arcade, SoundEvent::LayoutSwitch)
        );
        assert!(event.slug.starts_with("arcade-"));
        assert!(!pcm16_data_range(event.bytes.as_ref()).unwrap().is_empty());

        let key = key_asset(KeyCue::Space, SoundPack::Anime);
        assert!(matches!(
            key.id,
            SoundId::Key(SoundPack::Anime, KeyCue::Space, _)
        ));
        assert!(key.slug.starts_with("anime-key-space-"));
        assert!(!pcm16_data_range(key.bytes.as_ref()).unwrap().is_empty());
    }

    #[test]
    fn generated_waveforms_are_cached_by_pack_cue_and_variant() {
        let id = SoundId::Key(SoundPack::Anime, KeyCue::Enter, 2);
        let first = generated_sound(
            id,
            SoundPack::Anime,
            Cue::Key(KeyCue::Enter),
            key_seed(KeyCue::Enter, 2),
        );
        let second = generated_sound(
            id,
            SoundPack::Anime,
            Cue::Key(KeyCue::Enter),
            key_seed(KeyCue::Enter, 2),
        );

        assert!(Arc::ptr_eq(&first, &second));
    }
}
