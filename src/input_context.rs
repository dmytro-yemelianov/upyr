#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputContext {
    Gui,
    Terminal,
}

pub(crate) fn active() -> InputContext {
    platform::active()
}

/// Bundle identifiers of terminals where pasting is unreliable, so automatic
/// correction types the replacement instead, once the user opts the app in.
#[cfg(any(target_os = "macos", test))]
pub(crate) const KNOWN_TERMINAL_APPS: &[&str] = &[
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

/// Bundle identifiers of remote-desktop and virtual-machine clients. Their
/// keys reach another system that reads them with its own layout, and the
/// editing shortcuts a correction posts arrive there as other keys: Command
/// as the Windows key, Option+Shift as the Windows layout switch. Automatic
/// correction stays off in them.
#[cfg(any(target_os = "macos", test))]
const REMOTE_APPS: &[&str] = &[
    "com.microsoft.rdc.macos",
    "com.apple.ScreenSharing",
    "com.citrix.receiver.icaviewer.mac",
    "com.p5sys.jump.mac.viewer",
    "com.parallels.desktop.console",
    "com.philandro.anydesk",
    "com.realvnc.vncviewer",
    "com.teamviewer.TeamViewer",
    "com.utmapp.UTM",
    "com.vmware.fusion",
    "org.virtualbox.app.VirtualBoxVM",
];

#[cfg(any(target_os = "macos", test))]
fn is_remote(bundle_identifier: Option<&str>) -> bool {
    bundle_identifier.is_some_and(|identifier| {
        REMOTE_APPS
            .iter()
            .any(|known| identifier.eq_ignore_ascii_case(known))
    })
}

/// Whether automatic correction runs in the app being typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) enum AppCorrection {
    Running,
    /// The user paused it there from the menu bar.
    Paused,
    /// The app sends its keys to another computer or a virtual machine.
    Remote,
}

impl AppCorrection {
    /// The app, as (bundle identifier, display name), against the apps the
    /// user paused correction in.
    #[cfg(any(target_os = "macos", test))]
    fn of(app: Option<&(String, String)>, paused_apps: &[String]) -> Self {
        let Some((id, name)) = app else {
            return Self::Running;
        };
        if is_remote(Some(id)) {
            Self::Remote
        } else if paused_apps
            .iter()
            .any(|listed| listed.eq_ignore_ascii_case(id) || listed.eq_ignore_ascii_case(name))
        {
            Self::Paused
        } else {
            Self::Running
        }
    }

    pub(crate) fn is_off(self) -> bool {
        self != Self::Running
    }
}

#[cfg(any(target_os = "macos", test))]
fn classify(bundle_identifier: Option<&str>, localized_name: Option<&str>) -> InputContext {
    const TERMINAL_BUNDLE_IDENTIFIERS: &[&str] = KNOWN_TERMINAL_APPS;
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

    /// Bundle identifier and display name of the app receiving keystrokes.
    pub(super) fn frontmost_app() -> Option<(String, String)> {
        let application = NSWorkspace::sharedWorkspace().frontmostApplication()?;
        let id = application.bundleIdentifier()?.to_string();
        let name = application
            .localizedName()
            .map_or_else(|| id.clone(), |name| name.to_string());
        Some((id, name))
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

/// The app receiving keystrokes, as (bundle identifier, display name).
#[cfg(target_os = "macos")]
pub(crate) fn frontmost_app() -> Option<(String, String)> {
    platform::frontmost_app()
}

/// Whether automatic correction runs in `app`, the frontmost app as
/// [`frontmost_app`] reports it.
#[cfg(target_os = "macos")]
pub(crate) fn correction_in(
    app: Option<&(String, String)>,
    config: &crate::config::Config,
) -> AppCorrection {
    AppCorrection::of(app, &config.auto_correct_disabled_apps)
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
    fn recognizes_remote_desktop_and_virtual_machine_apps() {
        assert!(is_remote(Some("com.microsoft.rdc.macos")));
        assert!(is_remote(Some("COM.MICROSOFT.RDC.MACOS")));
        assert!(is_remote(Some("com.apple.ScreenSharing")));
        assert!(is_remote(Some("com.utmapp.UTM")));
        assert!(!is_remote(Some("com.apple.TextEdit")));
        assert!(!is_remote(None));
    }

    #[test]
    fn correction_is_off_in_remote_apps_and_apps_the_user_paused() {
        let app = |id: &str, name: &str| Some((id.to_owned(), name.to_owned()));
        let paused = vec!["com.tinyspeck.slackmacgap".to_owned()];
        assert_eq!(
            AppCorrection::of(app("com.microsoft.rdc.macos", "Windows App").as_ref(), &[]),
            AppCorrection::Remote
        );
        assert_eq!(
            AppCorrection::of(app("com.tinyspeck.slackmacgap", "Slack").as_ref(), &paused),
            AppCorrection::Paused
        );
        assert_eq!(
            AppCorrection::of(app("com.apple.TextEdit", "TextEdit").as_ref(), &paused),
            AppCorrection::Running
        );
        assert_eq!(AppCorrection::of(None, &paused), AppCorrection::Running);
        assert!(AppCorrection::Remote.is_off());
        assert!(AppCorrection::Paused.is_off());
        assert!(!AppCorrection::Running.is_off());
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
