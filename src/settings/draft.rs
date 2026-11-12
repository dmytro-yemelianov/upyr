//! Pure settings edits. Renderers own focus; the host owns I/O and OS effects.
use crate::config::{AutoCorrectSensitivity, Config};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub(super) struct SettingsDraft {
    base: Config,
    pub edited: Config,
    pub minimum: String,
    pub delay: String,
}

#[derive(Debug, Clone)]
pub(super) enum SettingsAction {
    Automatic(bool),
    SwitchLayout(bool),
    Sensitivity(AutoCorrectSensitivity),
    Minimum(String),
    Delay(String),
    AddExceptions(String),
    EditException(usize, String),
    RemoveException(usize),
    Revert,
}

impl SettingsDraft {
    pub fn new(config: Config) -> Self {
        Self::from_edits(&config, config.clone())
    }

    pub fn from_edits(base: &Config, edited: Config) -> Self {
        Self {
            base: base.clone(),
            minimum: edited.auto_correct_min_word_length.to_string(),
            delay: edited.auto_correct_delay_ms.to_string(),
            edited,
        }
    }

    pub fn dirty(&self) -> bool {
        self.edited != self.base
            || self.minimum != self.base.auto_correct_min_word_length.to_string()
            || self.delay != self.base.auto_correct_delay_ms.to_string()
    }

    pub fn apply(&mut self, action: SettingsAction) -> Result<()> {
        match action {
            SettingsAction::Automatic(value) => self.edited.auto_correct = value,
            SettingsAction::SwitchLayout(value) => self.edited.switch_layout = value,
            SettingsAction::Sensitivity(value) => self.edited.auto_correct_sensitivity = value,
            SettingsAction::Minimum(value) => self.minimum = value,
            SettingsAction::Delay(value) => self.delay = value,
            SettingsAction::AddExceptions(value) => {
                self.edited
                    .auto_correct_exceptions
                    .extend(parse_exceptions(&value));
                self.edited.auto_correct_exceptions =
                    normalize_exceptions(&self.edited.auto_correct_exceptions);
            }
            SettingsAction::EditException(index, value) => {
                let Some(row) = self.edited.auto_correct_exceptions.get_mut(index) else {
                    bail!("Exception row does not exist");
                };
                *row = value;
            }
            SettingsAction::RemoveException(index) => {
                if index >= self.edited.auto_correct_exceptions.len() {
                    bail!("Exception row does not exist");
                }
                self.edited.auto_correct_exceptions.remove(index);
            }
            SettingsAction::Revert => self.accept(self.base.clone()),
        }
        Ok(())
    }

    pub fn candidate(&self) -> Result<Config> {
        let mut config = self.edited.clone();
        config.auto_correct_min_word_length = self
            .minimum
            .trim()
            .parse()
            .context("Minimum word length must be an integer from 2 to 32")?;
        config.auto_correct_delay_ms = self
            .delay
            .trim()
            .parse()
            .context("Pause after Space must be an integer from 10 to 250 ms")?;
        config.auto_correct_exceptions = normalize_exceptions(&config.auto_correct_exceptions);
        config.validate()?;
        Ok(config)
    }

    pub fn prepare_save(&self, current: &Config) -> Result<Config> {
        let merged = Config::merge_edits(&self.base, &self.candidate()?, current)?;
        merged.validate()?;
        Ok(merged)
    }

    pub fn accept(&mut self, saved: Config) {
        *self = Self::new(saved);
    }
}

/// Split commas and Unicode whitespace into case-insensitively unique words.
pub(super) fn parse_exceptions(source: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    source
        .split(|character: char| character == ',' || character.is_whitespace())
        .filter(|word| !word.is_empty())
        .filter(|word| seen.insert(word.to_lowercase()))
        .map(str::to_owned)
        .collect()
}

/// Re-parses exception rows after inline edits, so Save never meets a blank,
/// duplicate or multi-word entry that configuration validation would reject.
pub(super) fn normalize_exceptions<I>(words: I) -> Vec<String>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let joined = words
        .into_iter()
        .map(|word| word.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join("\n");
    parse_exceptions(&joined)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_numbers_remain_editable_and_revert_restores_base() {
        let mut draft = SettingsDraft::new(Config::default());
        for value in ["", "abc", "0", "999"] {
            draft.apply(SettingsAction::Minimum(value.into())).unwrap();
            assert_eq!(draft.minimum, value);
            assert!(draft.dirty());
            assert!(draft.candidate().is_err());
        }
        draft.apply(SettingsAction::Revert).unwrap();
        assert!(!draft.dirty());
        draft.apply(SettingsAction::Delay("0".into())).unwrap();
        assert!(draft.candidate().is_err());
    }
    #[test]
    fn identical_actions_normalize_unicode_and_merge_only_edits() {
        let base = Config::default();
        let mut a = SettingsDraft::new(base.clone());
        let mut b = a.clone();
        for action in [
            SettingsAction::Automatic(true),
            SettingsAction::AddExceptions("Ґанок, Їжак їжак".into()),
            SettingsAction::EditException(0, "Іван Єва".into()),
            SettingsAction::Minimum("5".into()),
        ] {
            a.apply(action.clone()).unwrap();
            b.apply(action).unwrap();
        }
        assert_eq!(a.candidate().unwrap(), b.candidate().unwrap());
        let mut current = base;
        current.restore_clipboard = !current.restore_clipboard;
        current.auto_correct_rules = vec!["ghbdsn correct en".into()];
        let merged = a.prepare_save(&current).unwrap();
        assert_eq!(merged.restore_clipboard, current.restore_clipboard);
        assert_eq!(merged.auto_correct_rules, current.auto_correct_rules);
        assert_eq!(merged.auto_correct_exceptions, ["Іван", "Єва", "Їжак"]);
        assert_eq!(merged.auto_correct_min_word_length, 5);
        a.accept(merged);
        assert!(!a.dirty());
    }
}
