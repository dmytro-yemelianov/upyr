use std::{
    env,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use global_hotkey::hotkey::HotKey;
use serde::{Deserialize, Serialize};

use crate::layout::Direction;

pub const CURRENT_CONFIG_VERSION: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AutoCorrectSensitivity {
    Conservative,
    Balanced,
    Aggressive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModifierGesture {
    Disabled,
    DoubleControl,
    DoubleShift,
    DoubleControlShift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GestureAction {
    PreviousWord,
    Selection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    AutoCorrect,
    ManualConversion,
    LayoutSwitch,
    Pause,
    Resume,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SoundPack {
    Original,
    Arcade,
    Anime,
}

impl SoundPack {
    pub const ALL: [Self; 3] = [Self::Original, Self::Arcade, Self::Anime];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Upyr Original",
            Self::Arcade => "Pocket Arcade",
            Self::Anime => "Anime Reactions",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Original => "Soft local clicks with Upyr's original event cues.",
            Self::Arcade => "Bright pfxr-style clicks and action sounds.",
            Self::Anime => {
                "Clicks for text keys, synthesized vocal reactions for control keys, and \
                 expressive cues for Upyr's own events."
            }
        }
    }
}

/// What the floating layout indicator shows next to the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IndicatorStyle {
    /// Just the language letters, for example `EN`.
    Letters,
    /// Just the flag emoji.
    Flag,
    /// Letters and flag together, for example `EN  🇬🇧`.
    Both,
}

impl IndicatorStyle {
    pub const ALL: [Self; 3] = [Self::Letters, Self::Flag, Self::Both];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Letters => "Letters",
            Self::Flag => "Flag",
            Self::Both => "Letters and flag",
        }
    }
}

impl SoundEvent {
    pub const ALL: [Self; 6] = [
        Self::AutoCorrect,
        Self::ManualConversion,
        Self::LayoutSwitch,
        Self::Pause,
        Self::Resume,
        Self::Error,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::AutoCorrect => "Automatic correction",
            Self::ManualConversion => "Manual conversion",
            Self::LayoutSwitch => "Layout switch",
            Self::Pause => "Pause",
            Self::Resume => "Resume",
            Self::Error => "Error",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SoundSettings {
    /// Master switch for all event sounds.
    pub enabled: bool,
    /// Playback volume as a user-facing percentage.
    pub volume_percent: u8,
    /// Local procedural sound design selected for event and keyboard feedback.
    pub pack: SoundPack,
    /// Play low-latency, locally generated feedback for physical key presses.
    pub key_clicks: bool,
    pub auto_correct: bool,
    pub manual_conversion: bool,
    pub layout_switch: bool,
    pub pause: bool,
    pub resume: bool,
    pub error: bool,
}

impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            volume_percent: 65,
            pack: SoundPack::Original,
            key_clicks: false,
            auto_correct: true,
            manual_conversion: true,
            layout_switch: true,
            pause: true,
            resume: true,
            error: true,
        }
    }
}

impl SoundSettings {
    pub const fn event_selected(&self, event: SoundEvent) -> bool {
        match event {
            SoundEvent::AutoCorrect => self.auto_correct,
            SoundEvent::ManualConversion => self.manual_conversion,
            SoundEvent::LayoutSwitch => self.layout_switch,
            SoundEvent::Pause => self.pause,
            SoundEvent::Resume => self.resume,
            SoundEvent::Error => self.error,
        }
    }

    pub fn set_event_selected(&mut self, event: SoundEvent, selected: bool) {
        match event {
            SoundEvent::AutoCorrect => self.auto_correct = selected,
            SoundEvent::ManualConversion => self.manual_conversion = selected,
            SoundEvent::LayoutSwitch => self.layout_switch = selected,
            SoundEvent::Pause => self.pause = selected,
            SoundEvent::Resume => self.resume = selected,
            SoundEvent::Error => self.error = selected,
        }
    }

    pub const fn event_enabled(&self, event: SoundEvent) -> bool {
        self.enabled && self.volume_percent > 0 && self.event_selected(event)
    }

    pub const fn key_clicks_enabled(&self) -> bool {
        self.enabled && self.volume_percent > 0 && self.key_clicks
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Schema version used for forward-compatible configuration migrations.
    pub config_version: u32,
    /// Global shortcut syntax follows global-hotkey, for example CmdOrCtrl+Alt+Space.
    pub hotkey: String,
    /// Converts the word immediately before the caret without a manual selection.
    pub last_word_hotkey: String,
    pub direction: Direction,
    pub copy_delay_ms: u64,
    pub paste_delay_ms: u64,
    /// Follow a successful conversion by selecting the target OS input source.
    pub switch_layout: bool,
    /// Briefly show the target language next to the pointer after Upyr changes
    /// the active OS input source.
    pub show_layout_indicator: bool,
    pub layout_indicator_duration_ms: u64,
    /// What the layout indicator displays: letters, a flag, or both.
    pub indicator_style: IndicatorStyle,
    /// Event-specific local sound feedback.
    pub sounds: SoundSettings,
    /// Correct confidently recognized wrong-layout text after Space.
    /// Disabled means no ordinary typing is monitored.
    pub auto_correct: bool,
    pub auto_correct_sensitivity: AutoCorrectSensitivity,
    pub auto_correct_min_word_length: usize,
    pub auto_correct_delay_ms: u64,
    /// Words which should never trigger automatic correction.
    pub auto_correct_exceptions: Vec<String>,
    /// Optional modifier-only gesture. Disabled means no keyboard-state polling occurs.
    pub modifier_gesture: ModifierGesture,
    pub modifier_gesture_action: GestureAction,
    pub modifier_gesture_timeout_ms: u64,
    pub restore_clipboard: bool,
    pub restore_delay_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_version: CURRENT_CONFIG_VERSION,
            hotkey: "CmdOrCtrl+Alt+Space".to_owned(),
            last_word_hotkey: "CmdOrCtrl+Alt+Backspace".to_owned(),
            direction: Direction::Smart,
            copy_delay_ms: 90,
            paste_delay_ms: 40,
            switch_layout: true,
            show_layout_indicator: false,
            layout_indicator_duration_ms: 900,
            indicator_style: IndicatorStyle::Both,
            sounds: SoundSettings::default(),
            auto_correct: false,
            auto_correct_sensitivity: AutoCorrectSensitivity::Conservative,
            auto_correct_min_word_length: 4,
            auto_correct_delay_ms: 35,
            auto_correct_exceptions: Vec::new(),
            modifier_gesture: ModifierGesture::Disabled,
            modifier_gesture_action: GestureAction::PreviousWord,
            modifier_gesture_timeout_ms: 500,
            restore_clipboard: true,
            restore_delay_ms: 250,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }

        let source = fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let config = Self::decode(&source)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        Ok(config)
    }

    pub fn write(&self, overwrite: bool) -> Result<PathBuf> {
        self.validate()?;
        let path = config_path()?;
        if path.exists() && !overwrite {
            bail!(
                "config already exists at {}; pass --force to replace it",
                path.display()
            );
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory {}", parent.display())
            })?;
        }
        let source = toml::to_string_pretty(self).context("failed to serialize config")?;
        write_config_file(&path, source.as_bytes(), overwrite)?;
        Ok(path)
    }

    pub fn validate(&self) -> Result<()> {
        if self.config_version != CURRENT_CONFIG_VERSION {
            bail!(
                "unsupported config_version {}; this Upyr build supports version {}",
                self.config_version,
                CURRENT_CONFIG_VERSION
            );
        }
        if self.hotkey.trim().is_empty() {
            bail!("hotkey must not be empty");
        }
        if self.last_word_hotkey.trim().is_empty() {
            bail!("last_word_hotkey must not be empty");
        }
        if self.hotkey.eq_ignore_ascii_case(&self.last_word_hotkey) {
            bail!("hotkey and last_word_hotkey must be different");
        }
        let hotkey: HotKey = self
            .hotkey
            .parse()
            .with_context(|| format!("invalid hotkey {:?}", self.hotkey))?;
        let last_word_hotkey: HotKey = self
            .last_word_hotkey
            .parse()
            .with_context(|| format!("invalid last_word_hotkey {:?}", self.last_word_hotkey))?;
        if hotkey == last_word_hotkey {
            bail!("hotkey and last_word_hotkey resolve to the same shortcut");
        }
        if !(10..=2_000).contains(&self.copy_delay_ms) {
            bail!("copy_delay_ms must be between 10 and 2000");
        }
        if self.paste_delay_ms > 2_000 {
            bail!("paste_delay_ms must be at most 2000");
        }
        if self.restore_delay_ms > 5_000 {
            bail!("restore_delay_ms must be at most 5000");
        }
        if !(250..=3_000).contains(&self.layout_indicator_duration_ms) {
            bail!("layout_indicator_duration_ms must be between 250 and 3000");
        }
        if self.sounds.volume_percent > 100 {
            bail!("sounds.volume_percent must be between 0 and 100");
        }
        if !(150..=2_000).contains(&self.modifier_gesture_timeout_ms) {
            bail!("modifier_gesture_timeout_ms must be between 150 and 2000");
        }
        if !(2..=32).contains(&self.auto_correct_min_word_length) {
            bail!("auto_correct_min_word_length must be between 2 and 32");
        }
        if !(10..=250).contains(&self.auto_correct_delay_ms) {
            bail!("auto_correct_delay_ms must be between 10 and 250");
        }
        if self
            .auto_correct_exceptions
            .iter()
            .any(|word| word.trim().is_empty() || word.chars().any(char::is_whitespace))
        {
            bail!("auto_correct_exceptions must contain non-empty single words");
        }
        Ok(())
    }

    fn decode(source: &str) -> Result<Self> {
        let mut value: toml::Value = toml::from_str(source)?;
        migrate(&mut value)?;
        let config: Self = value.try_into()?;
        config.validate()?;
        Ok(config)
    }
}

fn write_config_file(path: &Path, source: &[u8], overwrite: bool) -> Result<()> {
    let parent = path
        .parent()
        .context("the configuration path has no parent directory")?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("the configuration filename is not valid UTF-8")?;

    let mut temporary = None;
    let mut file = None;
    for attempt in 0..128_u16 {
        let candidate = parent.join(format!(".{name}.{}.{}.tmp", process::id(), attempt));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&candidate) {
            Ok(created) => {
                temporary = Some(candidate);
                file = Some(created);
                break;
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "failed to create a private config file in {}",
                        parent.display()
                    )
                });
            }
        }
    }

    let temporary = temporary.context("could not reserve a temporary configuration file")?;
    let result = (|| -> Result<()> {
        let mut file = file.context("temporary configuration file was not opened")?;
        file.write_all(source)
            .context("failed to write the temporary configuration file")?;
        file.sync_all()
            .context("failed to flush the temporary configuration file")?;
        drop(file);

        if overwrite {
            replace_config_file(&temporary, path)?;
        } else {
            fs::hard_link(&temporary, path).with_context(|| {
                format!(
                    "config already exists at {}; pass --force to replace it",
                    path.display()
                )
            })?;
            fs::remove_file(&temporary)
                .context("failed to remove the temporary configuration link")?;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o600)).with_context(|| {
                format!("failed to protect config permissions at {}", path.display())
            })?;
        }
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.with_context(|| format!("failed to write config at {}", path.display()))
}

#[cfg(unix)]
fn replace_config_file(temporary: &Path, path: &Path) -> Result<()> {
    fs::rename(temporary, path).context("failed to atomically replace the configuration file")
}

#[cfg(windows)]
#[allow(unsafe_code)]
fn replace_config_file(temporary: &Path, path: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    fn wide_path(path: &Path) -> Result<Vec<u16>> {
        let mut encoded: Vec<u16> = path.as_os_str().encode_wide().collect();
        if encoded.contains(&0) {
            bail!("Windows configuration path contains an embedded NUL");
        }
        encoded.push(0);
        Ok(encoded)
    }

    let temporary = wide_path(temporary)?;
    let path = wide_path(path)?;
    // SAFETY: both buffers are NUL-terminated and remain alive for the call.
    // The temporary file is created beside the destination, so this is a
    // same-volume rename. MOVEFILE_REPLACE_EXISTING lets Windows perform the
    // replacement as one operation; unlike delete-then-rename, a failed call
    // leaves the destination name intact.
    let replaced = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            path.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if replaced == 0 {
        return Err(std::io::Error::last_os_error())
            .context("failed to atomically replace the Windows configuration file");
    }
    Ok(())
}

#[cfg(all(not(unix), not(windows)))]
fn replace_config_file(temporary: &Path, path: &Path) -> Result<()> {
    fs::rename(temporary, path).context(
        "failed to replace the configuration file without deleting the existing file first",
    )
}

fn migrate(value: &mut toml::Value) -> Result<()> {
    let table = value
        .as_table_mut()
        .context("configuration root must be a TOML table")?;
    let mut version = match table.get("config_version") {
        Some(toml::Value::Integer(version)) => u32::try_from(*version)
            .context("config_version must be a non-negative 32-bit integer")?,
        Some(_) => bail!("config_version must be an integer"),
        None => 0,
    };

    if version > CURRENT_CONFIG_VERSION {
        bail!(
            "config_version {version} was written by a newer Upyr; this build supports version {CURRENT_CONFIG_VERSION}"
        );
    }

    // Version 0 is the pre-versioned 0.1 configuration. Its field names were
    // retained in v1, so the first migration only records the schema.
    if version == 0 {
        version = 1;
        table.insert("config_version".to_owned(), toml::Value::Integer(1));
    }

    // Version 2 adds an explicitly disabled modifier-only gesture. Writing
    // the defaults into the in-memory value makes the migration deterministic
    // without rewriting the user's file behind their back.
    if version == 1 {
        table
            .entry("modifier_gesture".to_owned())
            .or_insert_with(|| toml::Value::String("disabled".to_owned()));
        table
            .entry("modifier_gesture_action".to_owned())
            .or_insert_with(|| toml::Value::String("previous-word".to_owned()));
        table
            .entry("modifier_gesture_timeout_ms".to_owned())
            .or_insert(toml::Value::Integer(500));
        table.insert("config_version".to_owned(), toml::Value::Integer(2));
        version = 2;
    }

    // Version 3 adds opt-in automatic correction. It remains disabled during
    // migration so updating Upyr never starts ordinary-key monitoring
    // without an explicit user choice.
    if version == 2 {
        table
            .entry("auto_correct".to_owned())
            .or_insert(toml::Value::Boolean(false));
        table
            .entry("auto_correct_sensitivity".to_owned())
            .or_insert_with(|| toml::Value::String("conservative".to_owned()));
        table
            .entry("auto_correct_min_word_length".to_owned())
            .or_insert(toml::Value::Integer(4));
        table
            .entry("auto_correct_delay_ms".to_owned())
            .or_insert(toml::Value::Integer(35));
        table
            .entry("auto_correct_exceptions".to_owned())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        table.insert("config_version".to_owned(), toml::Value::Integer(3));
        version = 3;
    }

    // Version 4 adds optional local layout-change feedback. Both channels stay
    // disabled during migration so upgrading never introduces new UI or sound.
    if version == 3 {
        table
            .entry("show_layout_indicator".to_owned())
            .or_insert(toml::Value::Boolean(false));
        table
            .entry("layout_indicator_duration_ms".to_owned())
            .or_insert(toml::Value::Integer(900));
        table
            .entry("play_switch_sound".to_owned())
            .or_insert(toml::Value::Boolean(false));
        table.insert("config_version".to_owned(), toml::Value::Integer(4));
        version = 4;
    }

    // Version 5 replaces the single layout-switch flag with a master volume
    // and per-event controls. A previously enabled sound remains enabled only
    // for layout switches so upgrading never introduces additional sounds.
    if version == 4 {
        let legacy_enabled = match table.remove("play_switch_sound") {
            Some(toml::Value::Boolean(enabled)) => enabled,
            Some(_) => bail!("play_switch_sound must be a boolean"),
            None => false,
        };
        table.entry("sounds".to_owned()).or_insert_with(|| {
            let enable_new_events = !legacy_enabled;
            toml::Value::Table(toml::Table::from_iter([
                ("enabled".to_owned(), toml::Value::Boolean(legacy_enabled)),
                ("volume_percent".to_owned(), toml::Value::Integer(65)),
                (
                    "auto_correct".to_owned(),
                    toml::Value::Boolean(enable_new_events),
                ),
                (
                    "manual_conversion".to_owned(),
                    toml::Value::Boolean(enable_new_events),
                ),
                ("layout_switch".to_owned(), toml::Value::Boolean(true)),
                ("pause".to_owned(), toml::Value::Boolean(enable_new_events)),
                ("resume".to_owned(), toml::Value::Boolean(enable_new_events)),
                ("error".to_owned(), toml::Value::Boolean(enable_new_events)),
            ]))
        });
        table.insert("config_version".to_owned(), toml::Value::Integer(5));
        version = 5;
    }

    // Version 6 adds opt-in procedural keyboard feedback and a sound-pack
    // selector. Existing installations keep the original event sounds and do
    // not begin monitoring ordinary key presses unless the user enables it.
    if version == 5 {
        if !table.contains_key("sounds") {
            table.insert(
                "sounds".to_owned(),
                toml::Value::try_from(SoundSettings::default())
                    .context("could not build default sound settings")?,
            );
        }
        let sounds = table
            .get_mut("sounds")
            .context("sounds was not available after migration")?
            .as_table_mut()
            .context("sounds must be a TOML table")?;
        sounds
            .entry("pack".to_owned())
            .or_insert_with(|| toml::Value::String("original".to_owned()));
        sounds
            .entry("key_clicks".to_owned())
            .or_insert(toml::Value::Boolean(false));
        table.insert("config_version".to_owned(), toml::Value::Integer(6));
        version = 6;
    }

    // Version 7 lets the layout indicator show letters, a flag, or both.
    // Existing installations keep today's combined presentation.
    if version == 6 {
        table
            .entry("indicator_style".to_owned())
            .or_insert_with(|| toml::Value::String("both".to_owned()));
        table.insert(
            "config_version".to_owned(),
            toml::Value::Integer(i64::from(CURRENT_CONFIG_VERSION)),
        );
    }
    Ok(())
}

pub fn config_path() -> Result<PathBuf> {
    if let Some(path) = env::var_os("UPYR_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    ProjectDirs::from("dev", "Upyr", "Upyr")
        .map(|directories| directories.config_dir().join("config.toml"))
        .context("the operating system did not provide a config directory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid_and_serializable() {
        let config = Config::default();

        config.validate().unwrap();
        let encoded = toml::to_string(&config).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();

        assert_eq!(decoded.hotkey, config.hotkey);
        assert_eq!(decoded.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(decoded.last_word_hotkey, config.last_word_hotkey);
        assert_eq!(decoded.direction, Direction::Smart);
        assert!(decoded.switch_layout);
        assert!(!decoded.show_layout_indicator);
        assert_eq!(decoded.indicator_style, IndicatorStyle::Both);
        assert!(!decoded.sounds.enabled);
        assert_eq!(decoded.sounds.volume_percent, 65);
        assert_eq!(decoded.sounds.pack, SoundPack::Original);
        assert!(!decoded.sounds.key_clicks);
        assert!(
            SoundEvent::ALL
                .into_iter()
                .all(|event| !decoded.sounds.event_enabled(event))
        );
        assert!(!encoded.contains("play_switch_sound"));
        assert!(!decoded.auto_correct);
        assert_eq!(decoded.modifier_gesture, ModifierGesture::Disabled);
    }

    #[test]
    fn config_writes_are_private_and_refuse_accidental_overwrite() {
        let directory = env::temp_dir().join(format!("upyr-config-write-test-{}", process::id()));
        let path = directory.join("config.toml");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();

        write_config_file(&path, b"first", false).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let error = format!(
            "{:#}",
            write_config_file(&path, b"second", false).unwrap_err()
        );
        assert!(error.contains("pass --force"));
        assert_eq!(fs::read(&path).unwrap(), b"first");

        write_config_file(&path, b"second", true).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn failed_windows_replace_preserves_the_existing_config() {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

        let directory =
            env::temp_dir().join(format!("upyr-windows-replace-test-{}", process::id()));
        let path = directory.join("config.toml");
        let temporary = directory.join("config.toml.tmp");
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(&path, b"valid config").unwrap();
        fs::write(&temporary, b"new config").unwrap();

        // Denying FILE_SHARE_DELETE makes the rename fail deterministically.
        // The failed atomic replacement must leave both files untouched.
        let existing = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&path)
            .unwrap();
        assert!(replace_config_file(&temporary, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"valid config");
        assert_eq!(fs::read(&temporary).unwrap(), b"new config");

        drop(existing);
        replace_config_file(&temporary, &path).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new config");
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn rejects_unreasonable_delays() {
        let config = Config {
            copy_delay_ms: 9,
            ..Config::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_sound_volume_over_one_hundred_percent() {
        let config = Config {
            sounds: SoundSettings {
                volume_percent: 101,
                ..SoundSettings::default()
            },
            ..Config::default()
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_shortcut_aliases_that_resolve_to_the_same_keys() {
        let config = Config {
            hotkey: "Ctrl+Alt+Space".to_owned(),
            last_word_hotkey: "Control+Option+Space".to_owned(),
            ..Config::default()
        };

        let error = config.validate().unwrap_err().to_string();

        assert!(error.contains("resolve to the same shortcut"));
    }

    #[test]
    fn migrates_pre_versioned_configuration_in_memory() {
        let source = r#"
hotkey = "CmdOrCtrl+Alt+Space"
last_word_hotkey = "CmdOrCtrl+Alt+Backspace"
direction = "smart"
copy_delay_ms = 100
paste_delay_ms = 50
restore_clipboard = true
restore_delay_ms = 250
"#;

        let config = Config::decode(source).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(config.switch_layout);
        assert_eq!(config.modifier_gesture, ModifierGesture::Disabled);
    }

    #[test]
    fn rejects_configuration_from_a_newer_schema() {
        let source = toml::to_string(&Config {
            config_version: CURRENT_CONFIG_VERSION + 1,
            ..Config::default()
        })
        .unwrap();

        let error = Config::decode(&source).unwrap_err().to_string();

        assert!(error.contains("newer Upyr"));
    }

    #[test]
    fn migrates_v1_configuration_with_disabled_gesture() {
        let source = r#"
config_version = 1
hotkey = "CmdOrCtrl+Alt+Space"
last_word_hotkey = "CmdOrCtrl+Alt+Backspace"
direction = "smart"
copy_delay_ms = 100
paste_delay_ms = 50
switch_layout = true
restore_clipboard = true
restore_delay_ms = 250
"#;

        let config = Config::decode(source).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.modifier_gesture, ModifierGesture::Disabled);
        assert_eq!(config.modifier_gesture_action, GestureAction::PreviousWord);
        assert_eq!(config.modifier_gesture_timeout_ms, 500);
        assert!(!config.auto_correct);
    }

    #[test]
    fn migrates_v2_configuration_with_automatic_correction_disabled() {
        let source = r#"
config_version = 2
hotkey = "CmdOrCtrl+Alt+Space"
last_word_hotkey = "CmdOrCtrl+Alt+Backspace"
direction = "smart"
copy_delay_ms = 100
paste_delay_ms = 50
switch_layout = true
modifier_gesture = "disabled"
modifier_gesture_action = "previous-word"
modifier_gesture_timeout_ms = 500
restore_clipboard = true
restore_delay_ms = 250
"#;

        let config = Config::decode(source).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(!config.auto_correct);
        assert_eq!(
            config.auto_correct_sensitivity,
            AutoCorrectSensitivity::Conservative
        );
        assert_eq!(config.auto_correct_min_word_length, 4);
        assert_eq!(config.auto_correct_delay_ms, 35);
        assert!(config.auto_correct_exceptions.is_empty());
        assert!(!config.show_layout_indicator);
        assert_eq!(config.layout_indicator_duration_ms, 900);
        assert!(!config.sounds.enabled);
        assert!(config.sounds.layout_switch);
    }

    #[test]
    fn migrates_v3_configuration_with_feedback_disabled() {
        let source = r#"
config_version = 3
hotkey = "CmdOrCtrl+Alt+Space"
last_word_hotkey = "CmdOrCtrl+Alt+Backspace"
direction = "smart"
copy_delay_ms = 90
paste_delay_ms = 40
switch_layout = true
auto_correct = false
auto_correct_sensitivity = "conservative"
auto_correct_min_word_length = 4
auto_correct_delay_ms = 35
auto_correct_exceptions = []
modifier_gesture = "disabled"
modifier_gesture_action = "previous-word"
modifier_gesture_timeout_ms = 500
restore_clipboard = true
restore_delay_ms = 250
"#;

        let config = Config::decode(source).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(!config.show_layout_indicator);
        assert_eq!(config.layout_indicator_duration_ms, 900);
        assert!(!config.sounds.enabled);
        assert!(config.sounds.auto_correct);
        assert!(config.sounds.manual_conversion);
        assert!(config.sounds.layout_switch);
    }

    #[test]
    fn migrates_v4_layout_sound_without_enabling_new_event_sounds() {
        let source = r#"
config_version = 4
hotkey = "CmdOrCtrl+Alt+Space"
last_word_hotkey = "CmdOrCtrl+Alt+Backspace"
direction = "smart"
copy_delay_ms = 90
paste_delay_ms = 40
switch_layout = true
show_layout_indicator = false
layout_indicator_duration_ms = 900
play_switch_sound = true
auto_correct = false
auto_correct_sensitivity = "conservative"
auto_correct_min_word_length = 4
auto_correct_delay_ms = 35
auto_correct_exceptions = []
modifier_gesture = "disabled"
modifier_gesture_action = "previous-word"
modifier_gesture_timeout_ms = 500
restore_clipboard = true
restore_delay_ms = 250
"#;

        let config = Config::decode(source).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(config.sounds.enabled);
        assert_eq!(config.sounds.volume_percent, 65);
        assert!(config.sounds.layout_switch);
        assert!(!config.sounds.auto_correct);
        assert!(!config.sounds.manual_conversion);
        assert!(!config.sounds.pause);
        assert!(!config.sounds.resume);
        assert!(!config.sounds.error);
        assert!(config.sounds.event_enabled(SoundEvent::LayoutSwitch));
        assert_eq!(config.sounds.pack, SoundPack::Original);
        assert!(!config.sounds.key_clicks);
    }

    #[test]
    fn migrates_v5_sounds_without_enabling_keyboard_monitoring() {
        let mut value = toml::Value::try_from(Config::default()).unwrap();
        let table = value.as_table_mut().unwrap();
        table.insert("config_version".to_owned(), toml::Value::Integer(5));
        let sounds = table
            .get_mut("sounds")
            .and_then(toml::Value::as_table_mut)
            .unwrap();
        sounds.remove("pack");
        sounds.remove("key_clicks");

        let config = Config::decode(&toml::to_string(&value).unwrap()).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.sounds.pack, SoundPack::Original);
        assert!(!config.sounds.key_clicks);
        assert!(!config.sounds.key_clicks_enabled());
    }

    #[test]
    fn migrates_v6_configuration_with_the_combined_indicator_style() {
        let mut value = toml::Value::try_from(Config::default()).unwrap();
        let table = value.as_table_mut().unwrap();
        table.insert("config_version".to_owned(), toml::Value::Integer(6));
        table.remove("indicator_style");

        let config = Config::decode(&toml::to_string(&value).unwrap()).unwrap();

        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.indicator_style, IndicatorStyle::Both);
    }
}
