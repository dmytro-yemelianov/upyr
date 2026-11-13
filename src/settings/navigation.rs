//! Shared navigation and correction-page content for native and egui settings.
//! No rendering, disk I/O or OS actions belong in this module.

pub(super) const CORRECTION_TITLE: &str = "Automatic correction";
pub(super) const CORRECTION_DESCRIPTION: &str =
    "Fix the keyboard layout while typing or at a word boundary.";
pub(super) const CORRECTION_TOGGLE: &str = "Correct the wrong keyboard layout automatically";
pub(super) const MIDWORD_HELP: &str = "During a word: clear prefixes (сщте → cont) switch from 4 letters, or your higher minimum. Requires Switch OS layout.";
pub(super) const CONTEXT_HELP: &str =
    "At a word boundary: short words can wait for context (nj vb → то ми; Ш have → I have).";
pub(super) const CORRECTION_FOOTNOTE: &str =
    "Exceptions and Keep rules are respected. Processing stays on this device.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SettingsTab {
    General,
    Automatic,
    Exceptions,
    Shortcuts,
    Feedback,
    Advanced,
    About,
}

impl SettingsTab {
    pub(super) const ALL: [Self; 7] = [
        Self::General,
        Self::Automatic,
        Self::Exceptions,
        Self::Shortcuts,
        Self::Feedback,
        Self::Advanced,
        Self::About,
    ];

    #[cfg(target_os = "macos")]
    pub(super) fn index(self) -> isize {
        Self::ALL
            .iter()
            .position(|tab| *tab == self)
            .expect("known settings tab") as isize
    }

    pub(super) fn label(self) -> &'static str {
        super::i18n::tr(match self {
            Self::General => "General",
            Self::Automatic => "Correction",
            Self::Exceptions => "Exceptions",
            Self::Shortcuts => "Shortcuts",
            Self::Feedback => "Feedback",
            Self::Advanced => "Advanced",
            Self::About => "About",
        })
    }

    pub(super) const fn command(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Automatic => "automatic",
            Self::Exceptions => "exceptions",
            Self::Shortcuts => "shortcuts",
            Self::Feedback => "feedback",
            Self::Advanced => "advanced",
            Self::About => "about",
        }
    }

    pub(super) fn from_command(command: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tab| tab.command() == command)
    }

    pub(super) const fn cli_argument(self) -> Option<&'static str> {
        match self {
            Self::About => Some("--about"),
            Self::General
            | Self::Automatic
            | Self::Exceptions
            | Self::Shortcuts
            | Self::Feedback
            | Self::Advanced => None,
        }
    }
}

pub(super) struct SearchParameter {
    pub(super) tab: SettingsTab,
    pub(super) label: &'static str,
    terms: &'static str,
}

pub(super) const SEARCH_PARAMETERS: &[SearchParameter] = &[
    SearchParameter {
        tab: SettingsTab::General,
        label: "Interface language",
        terms: "language ukrainian english мова інтерфейсу українська англійська",
    },
    #[cfg(target_os = "macos")]
    SearchParameter {
        tab: SettingsTab::General,
        label: "Globe key switching",
        terms: "globe fn instant keyboard layout switcher clean tap",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Conversion direction",
        terms: "smart english ukrainian language mapping",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Switch OS layout",
        terms: "input source follow converted text",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Restore clipboard",
        terms: "pasteboard copy contents",
    },
    SearchParameter {
        tab: SettingsTab::General,
        label: "Launch at login",
        terms: "autostart startup boot",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Automatic correction",
        terms: "auto fix after space detection automatic correction автовиправлення розкладка",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "During a word",
        terms: "mid word midword prefix early switching typing середина середині слова префікс",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Short-word context",
        terms: "short word context adjacent mixed languages nj vb контекст короткі слова",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Sensitivity",
        terms: "confidence conservative balanced aggressive",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Minimum word length",
        terms: "characters threshold",
    },
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Correction delay",
        terms: "space milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Exceptions,
        label: "Never correct list",
        terms: "exceptions dictionary words ignore exclude table add remove",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Convert selection shortcut",
        terms: "hotkey record key combination",
    },
    #[cfg(target_os = "macos")]
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Correct words in terminals",
        terms: "terminal iterm ghostty kitty warp alacritty wezterm termius shell typing backspace",
    },
    #[cfg(target_os = "macos")]
    SearchParameter {
        tab: SettingsTab::Automatic,
        label: "Slash commands in Superset",
        terms: "slash command prefix terminal cli tui",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Fix previous word shortcut",
        terms: "hotkey record backspace key combination",
    },
    #[cfg(target_os = "macos")]
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Undo auto-correction",
        terms: "undo restore last correction original text layout hotkey command option z скасувати",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Modifier gesture",
        terms: "double control shift trigger",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Gesture action",
        terms: "selection previous word",
    },
    SearchParameter {
        tab: SettingsTab::Shortcuts,
        label: "Gesture timeout",
        terms: "double press milliseconds",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Language indicator",
        terms: "flag floating cursor pointer overlay english ukrainian",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Indicator duration",
        terms: "flag overlay milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Indicator style",
        terms: "letters flag both language display shows preview globe",
    },
    #[cfg(target_os = "macos")]
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "All country flags",
        terms: "countries territories regions gallery collection catalogue picker choose assign override automatic revert",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Sound feedback",
        terms: "audio effects enable disable",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Sound volume",
        terms: "audio loud quiet percent preview",
    },
    #[cfg(target_os = "macos")]
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Headphone limit",
        terms: "headphones headset airpods quiet volume output gain навушники гучність",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Sound pack",
        terms: "theme original arcade pfxr anime reactions audio",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Keyboard sounds",
        terms: "typing clicks keys space enter escape tab option command control shift caps delete backspace",
    },
    SearchParameter {
        tab: SettingsTab::Feedback,
        label: "Event sounds",
        terms: "automatic correction manual conversion layout switch pause resume error preview",
    },
    SearchParameter {
        tab: SettingsTab::Advanced,
        label: "Copy delay",
        terms: "clipboard milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Advanced,
        label: "Paste delay",
        terms: "clipboard milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::Advanced,
        label: "Clipboard restore delay",
        terms: "pasteboard milliseconds timing",
    },
    SearchParameter {
        tab: SettingsTab::About,
        label: "Check for updates",
        terms: "update upgrade releases version download оновлення версія завантаження",
    },
    SearchParameter {
        tab: SettingsTab::About,
        label: "Version and license",
        terms: "about release semver copyright mit",
    },
    SearchParameter {
        tab: SettingsTab::About,
        label: "Privacy and implementation",
        terms: "local only no tracking telemetry analytics n-gram model rust security source",
    },
];

impl SearchParameter {
    pub(super) fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return false;
        }
        let haystack = format!(
            "{} {} {} {}",
            self.tab.label(),
            self.label,
            self.terms,
            super::i18n::translate(crate::config::UiLanguage::Ukrainian, self.label)
        )
        .to_lowercase();
        query.split_whitespace().all(|word| haystack.contains(word))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_search_finds_behaviour_in_either_language() {
        for query in [
            "MID WORD",
            "typing prefix",
            "середині слова",
            "short context",
            "контекст",
            "nj vb",
        ] {
            assert!(
                SEARCH_PARAMETERS
                    .iter()
                    .any(|entry| entry.tab == SettingsTab::Automatic && entry.matches(query)),
                "{query}"
            );
        }
        assert!(!SEARCH_PARAMETERS.iter().any(|entry| entry.matches("   ")));
        assert!(
            !SEARCH_PARAMETERS
                .iter()
                .any(|entry| entry.matches("context nonexistent-setting"))
        );
    }
}
