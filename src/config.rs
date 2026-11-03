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

pub const CURRENT_CONFIG_VERSION: u32 = 12;

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

/// The layout-feedback presentation (the flag row on macOS, or pointer labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IndicatorStyle {
    /// Just the language letters, for example `EN`.
    Letters,
    /// The GlobeSwitch flag row on macOS; a single flag on other platforms.
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
    /// Separate quiet ceiling for headphones and unrecognized macOS outputs.
    pub headphone_volume_percent: u8,
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
            headphone_volume_percent: 12,
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
    /// macOS: restore the last automatic edit before further input or navigation.
    pub undo_hotkey: String,
    pub direction: Direction,
    pub copy_delay_ms: u64,
    pub paste_delay_ms: u64,
    /// Follow a successful conversion by selecting the target OS input source.
    pub switch_layout: bool,
    /// The integrated macOS GlobeSwitch observer; independent of correction.
    pub globe_switch: crate::globe_switch::GlobeSettings,
    /// Briefly show the active OS input source using the selected indicator style.
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
    /// Opt-in application IDs for tracked terminal/TUI replacement without clipboard access.
    pub terminal_typing_apps: Vec<String>,
    /// Apps whose input lines accept /commands. Requires terminal_typing_apps and auto_correct.
    pub slash_command_apps: Vec<String>,
    /// Words which should never trigger automatic correction.
    pub auto_correct_exceptions: Vec<String>,
    /// Deterministic rules, one per entry: `pattern correct|keep [layout]`.
    /// The pattern is the keys as typed on a US keyboard (`ghbdsn`) or the text
    /// as seen when typed on the Ukrainian layout (`руку`); `*` at either end
    /// matches a prefix, suffix or any position. They apply before the
    /// built-in rules, and a matching `keep` always wins. Omitted when empty
    /// so older versions keep reading the file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub auto_correct_rules: Vec<String>,
    /// Applications (bundle identifiers on macOS) where automatic correction
    /// stays off, toggled from the menu bar. Omitted when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub auto_correct_disabled_apps: Vec<String>,
    /// macOS: Backspace as the first key after an automatic correction
    /// reverts it. Omitted while on.
    #[serde(skip_serializing_if = "is_true")]
    pub backspace_undo: bool,
    /// Remember the user's own fixes: a correction reverted twice with the
    /// undo shortcut adds an exception, a word converted by hand twice adds a
    /// `correct` rule. Backspace reverts teach nothing.
    /// Omitted while on.
    #[serde(skip_serializing_if = "is_true")]
    pub auto_correct_learning: bool,
    /// Optional shortcut transliterating the selection (Ukrainian → Latin by
    /// the 2010 national system). Empty means off; omitted when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub transliterate_hotkey: String,
    /// Optional shortcut cycling the selection's case (lower → UPPER →
    /// Title). Empty means off; omitted when empty.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub case_hotkey: String,
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
            undo_hotkey: "CmdOrCtrl+Alt+Z".to_owned(),
            direction: Direction::Smart,
            copy_delay_ms: 90,
            paste_delay_ms: 40,
            switch_layout: true,
            globe_switch: crate::globe_switch::GlobeSettings::default(),
            show_layout_indicator: false,
            layout_indicator_duration_ms: 900,
            indicator_style: IndicatorStyle::Both,
            sounds: SoundSettings::default(),
            auto_correct: false,
            auto_correct_sensitivity: AutoCorrectSensitivity::Conservative,
            auto_correct_min_word_length: 4,
            auto_correct_delay_ms: 35,
            terminal_typing_apps: Vec::new(),
            slash_command_apps: Vec::new(),
            auto_correct_exceptions: Vec::new(),
            auto_correct_rules: Vec::new(),
            auto_correct_disabled_apps: Vec::new(),
            backspace_undo: true,
            auto_correct_learning: true,
            transliterate_hotkey: String::new(),
            case_hotkey: String::new(),
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
        Self::load_from(&config_path()?)
    }

    fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let source = fs::read_to_string(path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let config = Self::decode(&source)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        Ok(config)
    }

    /// For starting the app and Settings: an unreadable file must not leave
    /// the user with no running app and no way to fix it from the UI. Returns
    /// the defaults together with a description of the problem.
    pub fn load_or_default() -> (Self, Option<String>) {
        match config_path() {
            Ok(path) => Self::load_or_default_from(&path),
            Err(error) => (Self::default(), Some(format!("{error:#}"))),
        }
    }

    fn load_or_default_from(path: &Path) -> (Self, Option<String>) {
        match Self::load_from(path) {
            Ok(config) => (config, None),
            Err(error) => (Self::default(), Some(format!("{error:#}"))),
        }
    }

    /// Three-way merge for a Settings save. `base` is what the window loaded,
    /// `edited` what it would write, `current` the file as it is now. Fields
    /// the window changed take its value; every other field keeps its current
    /// on-disk value, so changes made while Settings was open (the CLI,
    /// `upyr globe`, a hand edit) survive the save. Lists merge as a whole.
    pub fn merge_edits(base: &Self, edited: &Self, current: &Self) -> Result<Self> {
        fn merge(base: &toml::Value, edited: &toml::Value, current: &toml::Value) -> toml::Value {
            match (base, edited, current) {
                (
                    toml::Value::Table(base),
                    toml::Value::Table(edited),
                    toml::Value::Table(current),
                ) => {
                    let mut merged = current.clone();
                    for (key, edited_value) in edited {
                        match (base.get(key), current.get(key)) {
                            (Some(base_value), Some(current_value)) => {
                                merged.insert(
                                    key.clone(),
                                    merge(base_value, edited_value, current_value),
                                );
                            }
                            (Some(base_value), None) if base_value == edited_value => {}
                            _ => {
                                merged.insert(key.clone(), edited_value.clone());
                            }
                        }
                    }
                    // A key the window dropped (an emptied, unserialized list).
                    for key in base.keys().filter(|key| !edited.contains_key(*key)) {
                        merged.remove(key);
                    }
                    toml::Value::Table(merged)
                }
                _ if edited != base => edited.clone(),
                _ => current.clone(),
            }
        }
        let value = |config: &Self| {
            toml::Value::try_from(config).context("could not represent the configuration")
        };
        merge(&value(base)?, &value(edited)?, &value(current)?)
            .try_into()
            .context("could not merge the configuration")
    }

    /// What a Settings window writes: its edits merged onto the file as it is
    /// now. An unreadable file is replaced, so Settings can still repair it.
    pub fn for_settings_save(base: &Self, edited: Self) -> Self {
        match Self::load() {
            Ok(current) => Self::merge_edits(base, &edited, &current).unwrap_or(edited),
            Err(_) => edited,
        }
    }

    /// The enabled text-transform shortcuts.
    pub fn transform_hotkeys(&self) -> Result<Vec<(upyr_core::TextTransform, HotKey)>> {
        [
            (
                upyr_core::TextTransform::Transliterate,
                "transliterate_hotkey",
                &self.transliterate_hotkey,
            ),
            (
                upyr_core::TextTransform::CycleCase,
                "case_hotkey",
                &self.case_hotkey,
            ),
        ]
        .into_iter()
        .filter(|(_, _, value)| !value.trim().is_empty())
        .map(|(transform, name, value)| {
            let shortcut: HotKey = value
                .parse()
                .with_context(|| format!("invalid {name} {value:?}"))?;
            Ok((transform, shortcut))
        })
        .collect()
    }

    /// Switches automatic correction off or back on for one application.
    /// Returns true when it is now off there.
    pub fn toggle_auto_correct_app(&mut self, app: &str) -> bool {
        let before = self.auto_correct_disabled_apps.len();
        self.auto_correct_disabled_apps
            .retain(|listed| !listed.eq_ignore_ascii_case(app));
        if self.auto_correct_disabled_apps.len() == before {
            self.auto_correct_disabled_apps.push(app.to_owned());
            true
        } else {
            false
        }
    }

    /// Applies what Upyr learned from the user's fixes. Returns false when it
    /// changes nothing. A learned rule never overrides the user's own
    /// exception or rule for the same word.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) fn learn(&mut self, lesson: &crate::learning::Lesson) -> bool {
        use crate::learning::Lesson;
        let same_word =
            |listed: &str, word: &str| listed.trim().to_lowercase() == word.to_lowercase();
        let rule_pattern = |rule: &str| rule.split_whitespace().next().unwrap_or("").to_owned();
        match lesson {
            Lesson::Exception(word) => {
                if self
                    .auto_correct_exceptions
                    .iter()
                    .any(|listed| same_word(listed, word))
                {
                    return false;
                }
                let learned_rule = format!("{word} correct");
                self.auto_correct_rules
                    .retain(|rule| !same_word(rule, &learned_rule));
                self.auto_correct_exceptions.push(word.clone());
                true
            }
            Lesson::Rule(word) => {
                if self
                    .auto_correct_exceptions
                    .iter()
                    .any(|listed| same_word(listed, word))
                    || self
                        .auto_correct_rules
                        .iter()
                        .any(|rule| same_word(&rule_pattern(rule), word))
                {
                    return false;
                }
                self.auto_correct_rules.push(format!("{word} correct"));
                true
            }
        }
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

    /// Applies an `upyr run --hotkey` override to a freshly loaded
    /// configuration. The running app reapplies it after every reload, and the
    /// result passes the same validation and shortcut conflict checks as a
    /// hotkey saved in the configuration file.
    pub fn with_hotkey_override(mut self, hotkey: Option<&str>) -> Result<Self> {
        if let Some(hotkey) = hotkey {
            self.hotkey = hotkey.to_owned();
            self.validate()
                .with_context(|| format!("invalid --hotkey override {hotkey:?}"))?;
        }
        Ok(self)
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
        let undo: HotKey = self.undo_hotkey.parse().context("invalid undo_hotkey")?;
        // The undo shortcut is registered only on macOS, and only the macOS
        // Settings window can record it, so it can conflict only there.
        if cfg!(target_os = "macos") && [hotkey, last_word_hotkey].contains(&undo) {
            bail!("undo_hotkey must differ from the conversion shortcuts");
        }
        let mut taken = vec![hotkey, last_word_hotkey];
        if cfg!(target_os = "macos") {
            taken.push(undo);
        }
        for (_, shortcut) in self.transform_hotkeys()? {
            if taken.contains(&shortcut) {
                bail!("transliterate_hotkey and case_hotkey must differ from every other shortcut");
            }
            taken.push(shortcut);
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
        if self.sounds.headphone_volume_percent > 30 {
            bail!("sounds.headphone_volume_percent must be between 0 and 30");
        }
        if !(100..=2_000).contains(&self.globe_switch.tap_window_ms) {
            bail!("globe_switch.tap_window_ms must be between 100 and 2000");
        }
        if self
            .globe_switch
            .skipped_layouts
            .iter()
            .any(|id| id.trim().is_empty())
        {
            bail!("globe_switch.skipped_layouts must contain non-empty input-source IDs");
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
        for rule in &self.auto_correct_rules {
            if let Err(error) = upyr_core::parse_trigger(rule) {
                bail!("auto_correct_rules entry {rule:?} is invalid: {error}");
            }
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

#[allow(clippy::trivially_copy_pass_by_ref)] // serde passes a reference.
const fn is_true(value: &bool) -> bool {
    *value
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
        table.insert("config_version".to_owned(), toml::Value::Integer(7));
        version = 7;
    }

    // Opt-in: an upgrade must not claim the Globe key or request input access.
    if version == 7 {
        table
            .entry("globe_switch".to_owned())
            .or_insert(toml::Value::try_from(
                crate::globe_switch::GlobeSettings::default(),
            )?);
        table.insert("config_version".to_owned(), toml::Value::Integer(8));
        version = 8;
    }
    if version == 8 {
        if let Some(globe) = table
            .get_mut("globe_switch")
            .and_then(toml::Value::as_table_mut)
        {
            globe
                .entry("layout_flags".to_owned())
                .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        }
        table.insert("config_version".to_owned(), toml::Value::Integer(9));
        version = 9;
    }
    if version == 9 {
        table
            .entry("terminal_typing_apps".to_owned())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        table.insert("config_version".to_owned(), toml::Value::Integer(10));
        version = 10;
    }
    if version == 10 {
        table
            .entry("slash_command_apps".to_owned())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        table.insert("config_version".to_owned(), toml::Value::Integer(11));
        version = 11;
    }
    if version == 11 {
        if !table.contains_key("undo_hotkey") {
            let occupied: Vec<HotKey> = ["hotkey", "last_word_hotkey"]
                .iter()
                .filter_map(|key| table.get(*key)?.as_str()?.parse().ok())
                .collect();
            let choice = [
                "CmdOrCtrl+Alt+Z",
                "CmdOrCtrl+Alt+Shift+Z",
                "CmdOrCtrl+Alt+Shift+Backspace",
            ]
            .into_iter()
            .find(|choice| {
                choice
                    .parse::<HotKey>()
                    .is_ok_and(|key| !occupied.contains(&key))
            })
            .expect("three defaults for two occupied actions");
            table.insert(
                "undo_hotkey".to_owned(),
                toml::Value::String(choice.to_owned()),
            );
        }
        let sounds = table
            .entry("sounds".to_owned())
            .or_insert_with(|| toml::Value::Table(toml::Table::new()));
        sounds
            .as_table_mut()
            .context("sounds must be a table")?
            .entry("headphone_volume_percent".to_owned())
            .or_insert(toml::Value::Integer(12));
        table.insert("config_version".to_owned(), toml::Value::Integer(12));
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
    fn release_upgrade_preserves_old_preferences_through_save_and_reload() {
        fn preserved(before: &toml::Value, after: &toml::Value, path: &str) {
            if let Some(table) = before.as_table() {
                for (name, value) in table {
                    if path.is_empty() && name == "config_version" {
                        continue;
                    }
                    let next = format!("{path}.{name}");
                    preserved(value, after.get(name).expect(&next), &next);
                }
            } else {
                assert_eq!(before, after, "preference changed: {path}");
            }
        }
        let directory = env::temp_dir().join(format!("upyr-upgrade-test-{}", process::id()));
        fs::create_dir_all(&directory).unwrap();
        for (name, source) in [
            (
                "published-v6",
                include_str!("../tests/fixtures/config/published-v6.toml"),
            ),
            (
                "integrated-v11",
                include_str!("../tests/fixtures/config/integrated-v11.toml"),
            ),
            (
                "current-v12",
                include_str!("../tests/fixtures/config/current-v12.toml"),
            ),
        ] {
            let original: toml::Value = toml::from_str(source).unwrap();
            let config = Config::decode(source).unwrap();
            let upgraded = toml::Value::try_from(&config).unwrap();
            preserved(&original, &upgraded, "");
            assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
            if !original.as_table().unwrap().contains_key("globe_switch") {
                assert!(!config.globe_switch.enabled);
                assert!(config.terminal_typing_apps.is_empty());
                assert!(config.slash_command_apps.is_empty());
            }
            if original["sounds"].get("headphone_volume_percent").is_none() {
                assert_eq!(config.sounds.headphone_volume_percent, 12);
            }
            let path = directory.join(format!("{name}.toml"));
            write_config_file(
                &path,
                toml::to_string_pretty(&config).unwrap().as_bytes(),
                false,
            )
            .unwrap();
            let reloaded = Config::decode(&fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(upgraded, toml::Value::try_from(reloaded).unwrap());
        }
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn upgrade_adds_quiet_headphones_without_altering_existing_audio_or_hotkeys() {
        let config = Config::decode("config_version = 11\nhotkey = 'CmdOrCtrl+Alt+Z'\nauto_correct = true\n[sounds]\nenabled = true\nvolume_percent = 65\npack = 'anime'\n").unwrap();
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert_eq!(config.hotkey, "CmdOrCtrl+Alt+Z");
        assert_eq!(config.undo_hotkey, "CmdOrCtrl+Alt+Shift+Z");
        assert!(config.auto_correct && config.sounds.enabled);
        assert_eq!(config.sounds.volume_percent, 65);
        assert_eq!(config.sounds.headphone_volume_percent, 12);
        assert_eq!(config.sounds.pack, SoundPack::Anime);
        let mut invalid = config;
        invalid.sounds.headphone_volume_percent = 31;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn slash_command_upgrade_preserves_terminal_opt_in_without_enabling_commands() {
        let config = Config::decode("config_version = 10\nauto_correct = true\nterminal_typing_apps = [\"com.superset.desktop\"]\n").unwrap();
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(config.auto_correct);
        assert_eq!(config.terminal_typing_apps, ["com.superset.desktop"]);
        assert!(config.slash_command_apps.is_empty());
    }

    #[test]
    fn terminal_typing_upgrade_stays_opt_in_and_preserves_other_features() {
        let mut value = toml::Value::try_from(Config::default()).unwrap();
        let table = value.as_table_mut().unwrap();
        table.insert("config_version".to_owned(), toml::Value::Integer(9));
        table.insert("auto_correct".to_owned(), toml::Value::Boolean(true));
        table.remove("terminal_typing_apps");
        let config = Config::decode(&toml::to_string(&value).unwrap()).unwrap();
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(config.auto_correct);
        assert!(config.terminal_typing_apps.is_empty());
    }

    #[test]
    fn globe_upgrade_preserves_correction_and_does_not_claim_the_key() {
        let mut value = toml::Value::try_from(Config::default()).unwrap();
        let table = value.as_table_mut().unwrap();
        table.insert("config_version".to_owned(), toml::Value::Integer(7));
        table.insert("auto_correct".to_owned(), toml::Value::Boolean(true));
        table.remove("globe_switch");
        let config = Config::decode(&toml::to_string(&value).unwrap()).unwrap();
        assert_eq!(config.config_version, CURRENT_CONFIG_VERSION);
        assert!(config.auto_correct);
        assert_eq!(
            config.globe_switch,
            crate::globe_switch::GlobeSettings::default()
        );
    }

    #[test]
    fn globe_settings_round_trip_and_reject_invalid_tap_windows() {
        let mut config = Config::default();
        config.globe_switch.enabled = true;
        config.globe_switch.cycle_order = crate::globe_switch::CycleOrder::Recent;
        config.globe_switch.skipped_layouts = vec!["com.apple.keylayout.Russian".to_owned()];
        config
            .globe_switch
            .layout_flags
            .insert("com.apple.keylayout.ABC".to_owned(), "GB".to_owned());
        let decoded = Config::decode(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(decoded.globe_switch, config.globe_switch);
        for invalid in [0, 99, 2_001, u64::MAX] {
            config.globe_switch.tap_window_ms = invalid;
            assert!(config.validate().is_err());
        }
    }

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
    fn backspace_undo_and_learning_are_on_and_only_written_when_off() {
        let config = Config::default();
        assert!(config.backspace_undo && config.auto_correct_learning);
        let saved = toml::to_string(&config).unwrap();
        assert!(!saved.contains("backspace_undo") && !saved.contains("auto_correct_learning"));
        let off = Config {
            backspace_undo: false,
            auto_correct_learning: false,
            ..Config::default()
        };
        let saved = toml::to_string(&off).unwrap();
        assert!(saved.contains("backspace_undo = false"));
        let read: Config = toml::from_str(&saved).unwrap();
        assert!(!read.backspace_undo && !read.auto_correct_learning);
    }

    #[test]
    fn a_learned_exception_replaces_the_learned_rule_for_its_word() {
        use crate::learning::Lesson;
        let mut config = Config {
            auto_correct_rules: vec!["jr correct".to_owned(), "*zzz* keep".to_owned()],
            ..Config::default()
        };
        assert!(config.learn(&Lesson::Exception("jr".to_owned())));
        assert_eq!(config.auto_correct_exceptions, ["jr"]);
        assert_eq!(config.auto_correct_rules, ["*zzz* keep"]);
        // Already known.
        assert!(!config.learn(&Lesson::Exception("JR".to_owned())));
        config.validate().unwrap();
    }

    #[test]
    fn a_learned_rule_never_overrides_the_users_own_choice() {
        use crate::learning::Lesson;
        let mut config = Config {
            auto_correct_exceptions: vec!["Ghbdsn".to_owned()],
            auto_correct_rules: vec!["руку keep".to_owned()],
            ..Config::default()
        };
        assert!(!config.learn(&Lesson::Rule("ghbdsn".to_owned())));
        assert!(!config.learn(&Lesson::Rule("руку".to_owned())));
        assert!(config.learn(&Lesson::Rule("щл".to_owned())));
        assert!(!config.learn(&Lesson::Rule("щл".to_owned())));
        assert_eq!(config.auto_correct_rules, ["руку keep", "щл correct"]);
        config.validate().unwrap();
    }

    #[test]
    fn transform_shortcuts_are_optional_validated_and_distinct() {
        let saved = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!saved.contains("transliterate_hotkey") && !saved.contains("case_hotkey"));
        assert!(Config::default().transform_hotkeys().unwrap().is_empty());

        let config = Config {
            transliterate_hotkey: "CmdOrCtrl+Alt+T".to_owned(),
            case_hotkey: "CmdOrCtrl+Alt+U".to_owned(),
            ..Config::default()
        };
        config.validate().unwrap();
        assert_eq!(config.transform_hotkeys().unwrap().len(), 2);

        for clash in [Config::default().hotkey, "CmdOrCtrl+Alt+U".to_owned()] {
            let config = Config {
                transliterate_hotkey: clash.clone(),
                case_hotkey: "CmdOrCtrl+Alt+U".to_owned(),
                ..Config::default()
            };
            assert!(config.validate().is_err(), "{clash} must be rejected");
        }
        let invalid = Config {
            case_hotkey: "Ctrl+".to_owned(),
            ..Config::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn switching_correction_off_per_app_toggles_one_entry() {
        let mut config = Config::default();
        assert!(config.toggle_auto_correct_app("com.tinyspeck.slackmacgap"));
        assert_eq!(
            config.auto_correct_disabled_apps,
            ["com.tinyspeck.slackmacgap"]
        );
        // Bundle identifiers compare case-insensitively, as matching does.
        assert!(!config.toggle_auto_correct_app("COM.tinyspeck.SlackMacGap"));
        assert!(config.auto_correct_disabled_apps.is_empty());
        let saved = toml::to_string_pretty(&config).unwrap();
        assert!(
            !saved.contains("auto_correct_disabled_apps"),
            "an empty list must not make older versions reject the file"
        );
    }

    #[test]
    fn a_broken_file_falls_back_to_defaults_and_names_the_problem() {
        let path =
            std::env::temp_dir().join(format!("upyr-broken-config-{}.toml", std::process::id()));
        fs::write(&path, "hotkey = [unterminated").unwrap();
        let defaults = toml::to_string(&Config::default()).unwrap();
        let (config, problem) = Config::load_or_default_from(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(toml::to_string(&config).unwrap(), defaults);
        let problem = problem.expect("the problem is reported");
        assert!(problem.contains(&path.display().to_string()), "{problem}");

        let (config, problem) = Config::load_or_default_from(&path);
        assert_eq!(
            toml::to_string(&config).unwrap(),
            defaults,
            "a missing file means defaults"
        );
        assert_eq!(problem, None);
    }

    #[test]
    fn settings_saves_keep_changes_made_elsewhere_while_the_window_was_open() {
        let base = Config::default();
        // In the window: automatic correction turned on.
        let edited = Config {
            auto_correct: true,
            ..base.clone()
        };
        // Meanwhile, outside it: a terminal app, a rule and a Globe setting.
        let mut current = base.clone();
        current.terminal_typing_apps = vec!["com.example.terminal".to_owned()];
        current.auto_correct_rules = vec!["ghbdsn keep".to_owned()];
        current.globe_switch.tap_window_ms = 900;

        let merged = Config::merge_edits(&base, &edited, &current).unwrap();
        assert!(merged.auto_correct);
        assert_eq!(merged.terminal_typing_apps, ["com.example.terminal"]);
        assert_eq!(merged.auto_correct_rules, ["ghbdsn keep"]);
        assert_eq!(merged.globe_switch.tap_window_ms, 900);
    }

    #[test]
    fn settings_saves_win_for_fields_the_window_changed() {
        let base = Config::default();
        let edited = Config {
            hotkey: "Ctrl+Alt+J".to_owned(),
            auto_correct_exceptions: vec!["GitHub".to_owned()],
            ..base.clone()
        };
        let current = Config {
            hotkey: "Ctrl+Alt+K".to_owned(),
            auto_correct_exceptions: vec!["Codex".to_owned()],
            ..base.clone()
        };
        let merged = Config::merge_edits(&base, &edited, &current).unwrap();
        assert_eq!(merged.hotkey, "Ctrl+Alt+J");
        assert_eq!(merged.auto_correct_exceptions, ["GitHub"]);
        // Clearing a list in the window clears it on disk too.
        let base_with_rules = Config {
            auto_correct_rules: vec!["zzz correct".to_owned()],
            ..base.clone()
        };
        let merged = Config::merge_edits(&base_with_rules, &base, &base_with_rules).unwrap();
        assert!(merged.auto_correct_rules.is_empty());
    }

    #[test]
    fn correction_rules_are_validated_and_omitted_until_used() {
        let saved = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(
            !saved.contains("auto_correct_rules"),
            "an empty rule list must not make older versions reject the file"
        );

        let config = Config {
            auto_correct_rules: vec!["ghbdsn keep".to_owned(), "*zzz* correct".to_owned()],
            ..Config::default()
        };
        config.validate().unwrap();

        let invalid = Config {
            auto_correct_rules: vec!["ghbdsn fix".to_owned()],
            ..Config::default()
        };
        let error = format!("{:#}", invalid.validate().unwrap_err());
        assert!(error.contains("ghbdsn fix"), "{error}");
    }

    #[test]
    fn hotkey_override_replaces_a_freshly_loaded_hotkey_and_is_validated() {
        let loaded = Config {
            hotkey: "Ctrl+Alt+K".to_owned(),
            ..Config::default()
        };

        assert_eq!(
            loaded.clone().with_hotkey_override(None).unwrap().hotkey,
            "Ctrl+Alt+K"
        );
        assert_eq!(
            loaded
                .clone()
                .with_hotkey_override(Some("Ctrl+Alt+J"))
                .unwrap()
                .hotkey,
            "Ctrl+Alt+J"
        );
        let conflict = loaded
            .clone()
            .with_hotkey_override(Some(&loaded.last_word_hotkey))
            .unwrap_err();
        assert!(format!("{conflict:#}").contains("must be different"));
        let invalid = loaded.with_hotkey_override(Some("Ctrl+")).unwrap_err();
        assert!(format!("{invalid:#}").contains("--hotkey"));
    }

    #[test]
    fn undo_shortcut_conflicts_only_matter_where_undo_is_registered() {
        let config = Config {
            hotkey: "CmdOrCtrl+Alt+Z".to_owned(),
            undo_hotkey: "CmdOrCtrl+Alt+Z".to_owned(),
            ..Config::default()
        };

        #[cfg(target_os = "macos")]
        assert!(
            config
                .validate()
                .unwrap_err()
                .to_string()
                .contains("undo_hotkey must differ")
        );
        #[cfg(not(target_os = "macos"))]
        config.validate().unwrap();
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
