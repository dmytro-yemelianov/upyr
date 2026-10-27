#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputContext {
    Gui,
    Terminal,
}

pub(crate) fn active() -> InputContext {
    platform::active()
}

#[cfg(any(target_os = "macos", test))]
fn classify(bundle_identifier: Option<&str>, localized_name: Option<&str>) -> InputContext {
    const TERMINAL_BUNDLE_IDENTIFIERS: &[&str] = &[
        "com.apple.Terminal",
        "com.cmuxterm.app",
        "com.googlecode.iterm2",
        "com.mitchellh.ghostty",
        "com.termius-dmg.mac",
        "dev.warp.Warp-Stable",
        "net.kovidgoyal.kitty",
        "org.alacritty",
        "com.github.wez.wezterm",
        "com.superset.desktop",
    ];
    const TERMINAL_NAMES: &[&str] = &[
        "Terminal",
        "cmux",
        "iTerm2",
        "Ghostty",
        "Termius",
        "Warp",
        "kitty",
        "Alacritty",
        "WezTerm",
        "Superset",
    ];

    let matches_bundle = bundle_identifier.is_some_and(|identifier| {
        TERMINAL_BUNDLE_IDENTIFIERS
            .iter()
            .any(|known| identifier.eq_ignore_ascii_case(known))
    });
    let matches_name = localized_name.is_some_and(|name| {
        TERMINAL_NAMES
            .iter()
            .any(|known| name.eq_ignore_ascii_case(known))
    });

    if matches_bundle || matches_name {
        InputContext::Terminal
    } else {
        InputContext::Gui
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2_app_kit::NSWorkspace;
    use tracing::debug;

    use super::{InputContext, classify};

    pub(super) fn matches_application(identifiers: &[String]) -> bool {
        let Some(application) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return false;
        };
        let id = application
            .bundleIdentifier()
            .map(|value| value.to_string());
        let name = application.localizedName().map(|value| value.to_string());
        identifiers.iter().any(|allowed| {
            id.as_ref()
                .is_some_and(|id| id.eq_ignore_ascii_case(allowed))
                || name
                    .as_ref()
                    .is_some_and(|name| name.eq_ignore_ascii_case(allowed))
        })
    }

    pub(super) fn active() -> InputContext {
        let Some(application) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return InputContext::Gui;
        };
        let bundle_identifier = application
            .bundleIdentifier()
            .map(|identifier| identifier.to_string());
        let localized_name = application.localizedName().map(|name| name.to_string());
        let context = classify(bundle_identifier.as_deref(), localized_name.as_deref());
        debug!(
            ?context,
            ?bundle_identifier,
            ?localized_name,
            "classified the active input context"
        );
        context
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn terminal_typing_enabled(config: &crate::config::Config) -> bool {
    platform::matches_application(&config.terminal_typing_apps)
}

#[cfg(target_os = "macos")]
pub(crate) fn slash_commands_enabled(config: &crate::config::Config) -> bool {
    terminal_typing_enabled(config) && platform::matches_application(&config.slash_command_apps)
}

#[cfg(not(target_os = "macos"))]
mod platform {
    use super::InputContext;

    pub(super) fn active() -> InputContext {
        InputContext::Gui
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_supported_terminal_bundles() {
        for identifier in [
            "com.apple.Terminal",
            "com.cmuxterm.app",
            "com.googlecode.iterm2",
            "com.mitchellh.ghostty",
            "com.termius-dmg.mac",
            "dev.warp.Warp-Stable",
            "net.kovidgoyal.kitty",
            "org.alacritty",
            "com.github.wez.wezterm",
            "com.superset.desktop",
        ] {
            assert_eq!(classify(Some(identifier), None), InputContext::Terminal);
        }
    }

    #[test]
    fn recognizes_supported_terminal_names_as_a_fallback() {
        assert_eq!(classify(None, Some("Ghostty")), InputContext::Terminal);
        assert_eq!(classify(None, Some("wezterm")), InputContext::Terminal);
    }

    #[test]
    fn treats_regular_apps_as_gui_contexts() {
        assert_eq!(
            classify(Some("com.apple.TextEdit"), Some("TextEdit")),
            InputContext::Gui
        );
        assert_eq!(classify(None, None), InputContext::Gui);
    }
}
