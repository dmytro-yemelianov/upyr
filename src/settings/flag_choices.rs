//! Staged per-layout overrides. Saving flags merges only the edited layouts.
use std::collections::BTreeMap;

#[derive(Clone)]
pub(super) struct FlagChoices {
    saved: BTreeMap<String, String>,
    pub draft: BTreeMap<String, String>,
}

impl FlagChoices {
    pub fn new(saved: BTreeMap<String, String>) -> Self {
        Self {
            draft: saved.clone(),
            saved,
        }
    }

    pub fn dirty(&self) -> bool {
        self.saved != self.draft
    }

    pub fn revert(&mut self) {
        self.draft.clone_from(&self.saved);
    }

    pub fn accept(&mut self, saved: BTreeMap<String, String>) {
        *self = Self::new(saved);
    }

    pub fn apply_to(&self, current: &mut BTreeMap<String, String>) {
        for id in self.saved.keys().chain(self.draft.keys()) {
            if self.saved.get(id) == self.draft.get(id) {
                continue;
            }
            match self.draft.get(id) {
                Some(flag) => {
                    current.insert(id.clone(), flag.clone());
                }
                None => {
                    current.remove(id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn flags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn saving_only_edited_layouts_preserves_concurrent_choices_and_custom_text() {
        let mut choices = FlagChoices::new(flags(&[("ABC", "GB"), ("other", "Custom")]));
        choices.draft.insert("ABC".into(), "US".into());
        let mut current = flags(&[("ABC", "GB"), ("other", "New custom"), ("added", "UA")]);
        choices.apply_to(&mut current);
        assert_eq!(
            current,
            flags(&[("ABC", "US"), ("other", "New custom"), ("added", "UA")])
        );
        choices.accept(current);
        assert!(!choices.dirty());
    }

    #[test]
    fn automatic_and_revert_do_not_remove_unedited_overrides() {
        let saved = flags(&[("ABC", "GB"), ("ukrainian", "UA")]);
        let mut choices = FlagChoices::new(saved.clone());
        choices.draft.remove("ABC");
        assert!(choices.dirty());
        let mut current = saved.clone();
        choices.apply_to(&mut current);
        assert_eq!(current, flags(&[("ukrainian", "UA")]));
        choices.revert();
        assert!(!choices.dirty());
        assert_eq!(choices.draft, saved);
    }
}
