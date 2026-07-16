use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::layout::Direction;

pub const CURRENT_CONFIG_VERSION: u32 = 3;

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
    /// Correct a confidently recognized word after the user presses Space.
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
        fs::write(&path, source)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
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
        if !(10..=2_000).contains(&self.copy_delay_ms) {
            bail!("copy_delay_ms must be between 10 and 2000");
        }
        if self.paste_delay_ms > 2_000 {
            bail!("paste_delay_ms must be at most 2000");
        }
        if self.restore_delay_ms > 5_000 {
            bail!("restore_delay_ms must be at most 5000");
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
        assert!(!decoded.auto_correct);
        assert_eq!(decoded.modifier_gesture, ModifierGesture::Disabled);
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
    }
}
