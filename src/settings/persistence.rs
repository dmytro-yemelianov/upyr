//! Desktop persistence boundary, shared by all settings renderers.
use super::draft::SettingsDraft;
use crate::config::Config;
use anyhow::Result;

pub(super) trait SettingsStore {
    fn load(&self) -> Result<Config>;
    fn write(&self, config: &Config) -> Result<()>;
}

pub(super) struct LocalStore;
impl SettingsStore for LocalStore {
    fn load(&self) -> Result<Config> {
        Config::load()
    }
    fn write(&self, config: &Config) -> Result<()> {
        config.write(true).map(|_| ())
    }
}

pub(super) fn save(draft: &mut SettingsDraft, store: &impl SettingsStore) -> Result<()> {
    let current = store.load()?;
    let saved = draft.prepare_save(&current)?;
    store.write(&saved)?;
    draft.accept(saved);
    Ok(())
}

pub(super) fn reload(draft: &mut SettingsDraft, store: &impl SettingsStore) -> Result<()> {
    let current = store.load()?;
    draft.accept(current);
    Ok(())
}

pub(super) fn save_edits(base: &Config, edited: Config) -> Result<Config> {
    let mut draft = SettingsDraft::from_edits(base, edited);
    save(&mut draft, &LocalStore)?;
    Ok(draft.edited)
}

#[cfg(test)]
mod tests {
    use super::super::draft::SettingsAction;
    use super::*;
    use std::cell::{Cell, RefCell};
    struct Store {
        config: RefCell<Config>,
        fail_read: Cell<bool>,
        fail_write: Cell<bool>,
    }
    impl SettingsStore for Store {
        fn load(&self) -> Result<Config> {
            anyhow::ensure!(!self.fail_read.get(), "read failed");
            Ok(self.config.borrow().clone())
        }
        fn write(&self, config: &Config) -> Result<()> {
            anyhow::ensure!(!self.fail_write.get(), "write failed");
            self.config.replace(config.clone());
            Ok(())
        }
    }
    #[test]
    fn persistence_failures_keep_edits_and_success_accepts_concurrent_values() {
        let store = Store {
            config: RefCell::new(Config::default()),
            fail_read: Cell::new(false),
            fail_write: Cell::new(true),
        };
        let mut draft = SettingsDraft::new(store.load().unwrap());
        draft.apply(SettingsAction::Minimum("6".into())).unwrap();
        assert!(save(&mut draft, &store).is_err());
        assert!(draft.dirty());
        assert_eq!(store.config.borrow().auto_correct_min_word_length, 4);
        store.fail_read.set(true);
        store.fail_write.set(false);
        assert!(save(&mut draft, &store).is_err());
        assert!(reload(&mut draft, &store).is_err());
        assert!(draft.dirty());
        store.fail_read.set(false);
        store.config.borrow_mut().restore_clipboard = false;
        save(&mut draft, &store).unwrap();
        assert!(!draft.dirty());
        assert!(!draft.edited.restore_clipboard);
        assert_eq!(draft.edited.auto_correct_min_word_length, 6);
        draft.apply(SettingsAction::Minimum("7".into())).unwrap();
        reload(&mut draft, &store).unwrap();
        assert!(!draft.dirty());
        assert_eq!(draft.minimum, "6");
    }
}
