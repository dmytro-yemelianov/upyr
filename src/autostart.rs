#[cfg(any(target_os = "linux", target_os = "windows"))]
use std::{env, fs, path::PathBuf};

use anyhow::Result;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use anyhow::{Context, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartStatus {
    pub enabled: bool,
    pub location: String,
    pub state: AutostartState,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartState {
    Disabled,
    Enabled,
    Stale,
    Broken,
}

impl AutostartStatus {
    fn new(state: AutostartState, location: String, detail: Option<String>) -> Self {
        Self {
            enabled: state == AutostartState::Enabled,
            location,
            state,
            detail,
        }
    }

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    fn simple(enabled: bool, location: String) -> Self {
        Self::new(
            if enabled {
                AutostartState::Enabled
            } else {
                AutostartState::Disabled
            },
            location,
            None,
        )
    }
}

pub fn status() -> Result<AutostartStatus> {
    platform::status()
}

pub fn enable() -> Result<AutostartStatus> {
    platform::enable()?;
    status()
}

pub fn disable() -> Result<AutostartStatus> {
    platform::disable()?;
    status()
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn background_executable() -> Result<PathBuf> {
    let current = env::current_exe().context("could not locate the Upyr executable")?;
    let current = fs::canonicalize(&current).unwrap_or(current);
    if current.file_stem().and_then(|name| name.to_str()) != Some("upyr-settings") {
        return Ok(current);
    }

    for name in ["upyr-background", "upyr"] {
        let candidate = current.with_file_name(format!("{name}{}", env::consts::EXE_SUFFIX));
        if candidate.is_file() {
            return Ok(fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }
    bail!("the background Upyr executable is missing next to Upyr Settings")
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        env, fs,
        path::{Path, PathBuf},
    };

    use anyhow::{Context, Result, bail};
    use directories::BaseDirs;

    use super::{AutostartState, AutostartStatus};

    const LABEL: &str = "dev.Upyr.Upyr";

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Installation {
        bundle: PathBuf,
        executable: PathBuf,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum LaunchAgentInspection {
        Enabled,
        Stale(String),
        Broken(String),
    }

    pub fn status() -> Result<AutostartStatus> {
        let path = entry_path()?;
        let location = path.display().to_string();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(AutostartStatus::new(
                    AutostartState::Disabled,
                    location,
                    None,
                ));
            }
            Err(error) => {
                return Ok(AutostartStatus::new(
                    AutostartState::Broken,
                    location,
                    Some(format!("the LaunchAgent cannot be inspected: {error}")),
                ));
            }
        };
        if !metadata.file_type().is_file() {
            return Ok(AutostartStatus::new(
                AutostartState::Broken,
                location,
                Some("the LaunchAgent path is not a regular file".to_owned()),
            ));
        }

        let installation = match current_installation() {
            Ok(installation) => installation,
            Err(error) => {
                return Ok(AutostartStatus::new(
                    AutostartState::Broken,
                    location,
                    Some(format!(
                        "the current Upyr installation is not eligible: {error:#}"
                    )),
                ));
            }
        };
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) => {
                return Ok(AutostartStatus::new(
                    AutostartState::Broken,
                    location,
                    Some(format!("the LaunchAgent cannot be read: {error}")),
                ));
            }
        };
        let inspection = inspect_launch_agent(&contents, &installation, Path::is_file);
        Ok(status_from_inspection(inspection, location))
    }

    pub fn enable() -> Result<()> {
        let path = entry_path()?;
        let installation = current_installation()?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.file_type().is_file() {
                    bail!(
                        "refusing to replace the non-regular LaunchAgent at {}",
                        path.display()
                    );
                }
                let existing = fs::read_to_string(&path).with_context(|| {
                    format!(
                        "failed to inspect the existing LaunchAgent at {}",
                        path.display()
                    )
                })?;
                match inspect_launch_agent(&existing, &installation, Path::is_file) {
                    LaunchAgentInspection::Enabled => return Ok(()),
                    LaunchAgentInspection::Stale(_) => {}
                    LaunchAgentInspection::Broken(detail) => {
                        bail!(
                            "refusing to overwrite the broken LaunchAgent at {}: {detail}",
                            path.display()
                        );
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to inspect LaunchAgent path {}", path.display())
                });
            }
        }
        let contents = launch_agent_contents(&installation.executable);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create LaunchAgents directory {}",
                    parent.display()
                )
            })?;
        }
        fs::write(&path, contents)
            .with_context(|| format!("failed to write LaunchAgent at {}", path.display()))
    }

    pub fn disable() -> Result<()> {
        let path = entry_path()?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("failed to remove LaunchAgent at {}", path.display())),
        }
    }

    fn entry_path() -> Result<PathBuf> {
        BaseDirs::new()
            .map(|dirs| {
                dirs.home_dir()
                    .join("Library/LaunchAgents")
                    .join(format!("{LABEL}.plist"))
            })
            .context("the operating system did not provide a home directory")
    }

    fn current_installation() -> Result<Installation> {
        let current = env::current_exe().context("could not locate the Upyr executable")?;
        let current = fs::canonicalize(&current)
            .with_context(|| format!("could not resolve {}", current.display()))?;
        let base_dirs =
            BaseDirs::new().context("the operating system did not provide a home directory")?;
        let installation = installation_for_executable(&current, base_dirs.home_dir())
            .map_err(anyhow::Error::msg)?;
        if !installation.executable.is_file() {
            bail!(
                "the packaged background executable is missing at {}",
                installation.executable.display()
            );
        }
        let executable = fs::canonicalize(&installation.executable).with_context(|| {
            format!(
                "could not resolve the packaged background executable at {}",
                installation.executable.display()
            )
        })?;
        let bundle = fs::canonicalize(&installation.bundle).with_context(|| {
            format!(
                "could not resolve the Upyr application bundle at {}",
                installation.bundle.display()
            )
        })?;
        if !executable.starts_with(bundle.join("Contents/MacOS"))
            || executable.file_name().and_then(|name| name.to_str()) != Some("upyr-background")
        {
            bail!(
                "the packaged background executable resolves outside {}",
                bundle.display()
            );
        }
        if executable.to_str().is_none() {
            bail!("the packaged background executable path is not valid UTF-8");
        }
        validate_bundle_identifier(&bundle)?;
        Ok(Installation { bundle, executable })
    }

    fn validate_bundle_identifier(bundle: &Path) -> Result<()> {
        let info_path = bundle.join("Contents/Info.plist");
        let contents = fs::read_to_string(&info_path)
            .with_context(|| format!("could not read {}", info_path.display()))?;
        let identifier = plist_string(&contents, "CFBundleIdentifier")
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("could not inspect {}", info_path.display()))?;
        if identifier != LABEL {
            bail!("the application bundle identifier is {identifier:?}, expected {LABEL:?}");
        }
        Ok(())
    }

    fn installation_for_executable(
        current: &Path,
        home: &Path,
    ) -> std::result::Result<Installation, String> {
        if !current.is_absolute() {
            return Err("the executable path is not absolute".to_owned());
        }
        if current.starts_with("/Volumes")
            || current
                .components()
                .any(|component| component.as_os_str() == "AppTranslocation")
        {
            return Err(
                "Upyr is running from a disk image or App Translocation; move Upyr.app to Applications and reopen it"
                    .to_owned(),
            );
        }

        let bundle = outermost_app_bundle(current)
            .ok_or_else(|| "Upyr is not running from a packaged .app bundle".to_owned())?;
        let system_applications = Path::new("/Applications");
        let user_applications = home.join("Applications");
        if !bundle.starts_with(system_applications) && !bundle.starts_with(&user_applications) {
            return Err(format!(
                "{} is not an installed application; move Upyr.app to /Applications or ~/Applications and reopen it",
                bundle.display()
            ));
        }

        let contents = bundle.join("Contents");
        if !current.starts_with(contents.join("MacOS"))
            && !current.starts_with(contents.join("Helpers"))
        {
            return Err(
                "the executable is outside the application bundle's executable directories"
                    .to_owned(),
            );
        }
        Ok(Installation {
            executable: contents.join("MacOS/upyr-background"),
            bundle,
        })
    }

    fn outermost_app_bundle(path: &Path) -> Option<PathBuf> {
        path.ancestors()
            .filter(|ancestor| {
                ancestor
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
            })
            .last()
            .map(Path::to_path_buf)
    }

    fn status_from_inspection(
        inspection: LaunchAgentInspection,
        location: String,
    ) -> AutostartStatus {
        match inspection {
            LaunchAgentInspection::Enabled => {
                AutostartStatus::new(AutostartState::Enabled, location, None)
            }
            LaunchAgentInspection::Stale(detail) => {
                AutostartStatus::new(AutostartState::Stale, location, Some(detail))
            }
            LaunchAgentInspection::Broken(detail) => {
                AutostartStatus::new(AutostartState::Broken, location, Some(detail))
            }
        }
    }

    fn inspect_launch_agent(
        contents: &str,
        expected: &Installation,
        is_file: impl Fn(&Path) -> bool,
    ) -> LaunchAgentInspection {
        let label = match plist_string(contents, "Label") {
            Ok(label) => label,
            Err(detail) => return LaunchAgentInspection::Broken(detail),
        };
        if label != LABEL {
            return LaunchAgentInspection::Broken(format!(
                "the LaunchAgent label is {label:?}, expected {LABEL:?}"
            ));
        }

        let arguments = match plist_string_array(contents, "ProgramArguments") {
            Ok(arguments) => arguments,
            Err(detail) => return LaunchAgentInspection::Broken(detail),
        };
        if arguments.len() != 2 || arguments[1] != "run" {
            return LaunchAgentInspection::Broken(
                "ProgramArguments must contain the Upyr executable followed by `run`".to_owned(),
            );
        }

        let configured = PathBuf::from(&arguments[0]);
        if !configured.is_absolute() {
            return LaunchAgentInspection::Broken(
                "the configured executable path is not absolute".to_owned(),
            );
        }
        let configured_bundle = outermost_app_bundle(&configured);
        if configured != expected.executable
            || configured_bundle.as_deref() != Some(expected.bundle.as_path())
        {
            return LaunchAgentInspection::Stale(format!(
                "the LaunchAgent points to {}, but the current installation is {}",
                configured.display(),
                expected.executable.display()
            ));
        }
        if !is_file(&configured) {
            return LaunchAgentInspection::Broken(format!(
                "the configured executable is missing at {}",
                configured.display()
            ));
        }
        LaunchAgentInspection::Enabled
    }

    fn plist_string(contents: &str, key: &str) -> std::result::Result<String, String> {
        let value = plist_value_after_key(contents, key)?;
        parse_xml_string(value).map(|(value, _)| value)
    }

    fn plist_string_array(contents: &str, key: &str) -> std::result::Result<Vec<String>, String> {
        let value = plist_value_after_key(contents, key)?;
        let mut remaining = value
            .strip_prefix("<array>")
            .ok_or_else(|| format!("plist key {key:?} is not an array"))?;
        let mut values = Vec::new();
        loop {
            remaining = remaining.trim_start();
            if remaining.starts_with("</array>") {
                return Ok(values);
            }
            let (value, rest) = parse_xml_string(remaining)?;
            values.push(value);
            remaining = rest;
        }
    }

    fn plist_value_after_key<'a>(
        contents: &'a str,
        wanted: &str,
    ) -> std::result::Result<&'a str, String> {
        let mut remaining = contents;
        while let Some(key_start) = remaining.find("<key>") {
            let after_start = &remaining[key_start + "<key>".len()..];
            let key_end = after_start
                .find("</key>")
                .ok_or_else(|| "the plist contains an unterminated key".to_owned())?;
            let key = xml_unescape(after_start[..key_end].trim())?;
            let after_key = &after_start[key_end + "</key>".len()..];
            if key == wanted {
                return Ok(after_key.trim_start());
            }
            remaining = after_key;
        }
        Err(format!("the plist does not contain key {wanted:?}"))
    }

    fn parse_xml_string(value: &str) -> std::result::Result<(String, &str), String> {
        let value = value
            .strip_prefix("<string>")
            .ok_or_else(|| "the plist value is not a string".to_owned())?;
        let end = value
            .find("</string>")
            .ok_or_else(|| "the plist contains an unterminated string".to_owned())?;
        let decoded = xml_unescape(&value[..end])?;
        Ok((decoded, &value[end + "</string>".len()..]))
    }

    fn xml_unescape(value: &str) -> std::result::Result<String, String> {
        let mut decoded = String::with_capacity(value.len());
        let mut remaining = value;
        while let Some(entity_start) = remaining.find('&') {
            decoded.push_str(&remaining[..entity_start]);
            let entity = &remaining[entity_start..];
            let entity_end = entity
                .find(';')
                .ok_or_else(|| "the plist contains an unterminated XML entity".to_owned())?;
            let replacement = match &entity[1..entity_end] {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                unknown => {
                    return Err(format!(
                        "the plist contains unsupported XML entity &{unknown};"
                    ));
                }
            };
            decoded.push(replacement);
            remaining = &entity[entity_end + 1..];
        }
        decoded.push_str(remaining);
        Ok(decoded)
    }

    fn launch_agent_contents(executable: &std::path::Path) -> String {
        let executable = xml_escape(&executable.to_string_lossy());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{executable}</string>
        <string>run</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
</dict>
</plist>
"#
        )
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    #[cfg(test)]
    mod tests {
        use std::path::{Path, PathBuf};

        use super::*;

        #[test]
        fn launch_agent_escapes_executable_path() {
            let contents = launch_agent_contents(Path::new("/tmp/Upyr & Friends/upyr"));

            assert!(contents.contains("/tmp/Upyr &amp; Friends/upyr"));
            assert!(contents.contains("<string>run</string>"));
            assert!(contents.contains(LABEL));
        }

        #[test]
        fn installed_application_paths_are_accepted() {
            let system = installation_for_executable(
                Path::new("/Applications/Upyr.app/Contents/MacOS/upyr"),
                Path::new("/Users/test"),
            )
            .expect("system installation should be eligible");
            assert_eq!(
                system.executable,
                PathBuf::from("/Applications/Upyr.app/Contents/MacOS/upyr-background")
            );

            let user = installation_for_executable(
                Path::new("/Users/test/Applications/Upyr.app/Contents/MacOS/upyr-background"),
                Path::new("/Users/test"),
            );
            assert!(user.is_ok());
        }

        #[test]
        fn nested_settings_bundle_resolves_outer_application() {
            let installation = installation_for_executable(
                Path::new(
                    "/Applications/Upyr.app/Contents/Helpers/Upyr Settings.app/Contents/MacOS/upyr-settings",
                ),
                Path::new("/Users/test"),
            )
            .expect("nested settings app should resolve to its host");

            assert_eq!(installation.bundle, PathBuf::from("/Applications/Upyr.app"));
            assert_eq!(
                installation.executable,
                PathBuf::from("/Applications/Upyr.app/Contents/MacOS/upyr-background")
            );
        }

        #[test]
        fn transient_and_unpackaged_paths_are_rejected() {
            let home = Path::new("/Users/test");
            for path in [
                "/Volumes/Upyr/Upyr.app/Contents/MacOS/upyr-background",
                "/Volumes/App Translocation/Upyr.app/Contents/MacOS/upyr-background",
                "/private/var/folders/zz/AppTranslocation/Upyr.app/Contents/MacOS/upyr-background",
                "/tmp/Upyr.app/Contents/MacOS/upyr-background",
                "/usr/local/bin/upyr-background",
            ] {
                assert!(
                    installation_for_executable(Path::new(path), home).is_err(),
                    "{path} must not be eligible for launch at login"
                );
            }
        }

        #[test]
        fn launch_agent_round_trips_as_enabled() {
            let executable =
                Path::new("/Applications/Upyr & Friends.app/Contents/MacOS/upyr-background");
            let installation = Installation {
                bundle: PathBuf::from("/Applications/Upyr & Friends.app"),
                executable: executable.to_path_buf(),
            };
            let contents = launch_agent_contents(executable);

            assert_eq!(
                inspect_launch_agent(&contents, &installation, |_| true),
                LaunchAgentInspection::Enabled
            );
        }

        #[test]
        fn moved_application_is_reported_as_stale() {
            let old = Path::new("/Applications/Old Upyr.app/Contents/MacOS/upyr-background");
            let current = Installation {
                bundle: PathBuf::from("/Applications/Upyr.app"),
                executable: PathBuf::from("/Applications/Upyr.app/Contents/MacOS/upyr-background"),
            };

            assert!(matches!(
                inspect_launch_agent(&launch_agent_contents(old), &current, |_| false),
                LaunchAgentInspection::Stale(_)
            ));

            let status = status_from_inspection(
                LaunchAgentInspection::Stale("moved".to_owned()),
                "/tmp/agent.plist".to_owned(),
            );
            assert_eq!(status.state, AutostartState::Stale);
            assert!(!status.enabled);
            assert_eq!(status.detail.as_deref(), Some("moved"));
        }

        #[test]
        fn malformed_launch_agent_is_reported_as_broken() {
            let installation = Installation {
                bundle: PathBuf::from("/Applications/Upyr.app"),
                executable: PathBuf::from("/Applications/Upyr.app/Contents/MacOS/upyr-background"),
            };
            let malformed = launch_agent_contents(&installation.executable)
                .replace("<string>run</string>", "<string>settings</string>");

            assert!(matches!(
                inspect_launch_agent(&malformed, &installation, |_| true),
                LaunchAgentInspection::Broken(_)
            ));
        }

        #[test]
        fn matching_but_missing_executable_is_broken() {
            let installation = Installation {
                bundle: PathBuf::from("/Applications/Upyr.app"),
                executable: PathBuf::from("/Applications/Upyr.app/Contents/MacOS/upyr-background"),
            };

            assert!(matches!(
                inspect_launch_agent(
                    &launch_agent_contents(&installation.executable),
                    &installation,
                    |_| false
                ),
                LaunchAgentInspection::Broken(_)
            ));
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{fs, path::PathBuf};

    use anyhow::{Context, Result, bail};
    use directories::BaseDirs;

    use super::AutostartStatus;

    const FILE_NAME: &str = "dev.Upyr.Upyr.desktop";

    pub fn status() -> Result<AutostartStatus> {
        let path = entry_path()?;
        Ok(AutostartStatus::simple(
            path.is_file(),
            path.display().to_string(),
        ))
    }

    pub fn enable() -> Result<()> {
        let path = entry_path()?;
        let executable = super::background_executable()?;
        let Some(executable) = executable.to_str() else {
            bail!("the Upyr executable path is not valid UTF-8");
        };
        let contents = desktop_entry_contents(executable);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create autostart directory {}", parent.display())
            })?;
        }
        fs::write(&path, contents)
            .with_context(|| format!("failed to write autostart entry at {}", path.display()))
    }

    pub fn disable() -> Result<()> {
        let path = entry_path()?;
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error)
                .with_context(|| format!("failed to remove autostart entry at {}", path.display())),
        }
    }

    fn entry_path() -> Result<PathBuf> {
        BaseDirs::new()
            .map(|dirs| dirs.config_dir().join("autostart").join(FILE_NAME))
            .context("the operating system did not provide a configuration directory")
    }

    fn desktop_entry_contents(executable: &str) -> String {
        let executable = desktop_exec_quote(executable);
        format!(
            "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Upyr\nComment=English-Ukrainian keyboard layout fixer\nExec={executable} run\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        )
    }

    fn desktop_exec_quote(value: &str) -> String {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('`', "\\`")
            .replace('$', "\\$")
            .replace('%', "%%");
        format!("\"{escaped}\"")
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn desktop_entry_quotes_executable_path() {
            let contents = desktop_entry_contents("/tmp/Upyr $Test/upyr");

            assert!(contents.contains("Exec=\"/tmp/Upyr \\$Test/upyr\" run"));
            assert!(contents.contains("Terminal=false"));
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::process::Command;

    use anyhow::{Context, Result, bail};

    use super::AutostartStatus;

    const REGISTRY_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Upyr";

    pub fn status() -> Result<AutostartStatus> {
        let output = Command::new("reg")
            .args(["query", REGISTRY_KEY, "/v", VALUE_NAME])
            .output()
            .context("failed to query the Windows startup registry")?;
        Ok(AutostartStatus::simple(
            output.status.success(),
            format!(r"{REGISTRY_KEY}\{VALUE_NAME}"),
        ))
    }

    pub fn enable() -> Result<()> {
        let executable = super::background_executable()?;
        let command_line = format!("\"{}\" run", executable.display());
        let status = Command::new("reg")
            .args([
                "add",
                REGISTRY_KEY,
                "/v",
                VALUE_NAME,
                "/t",
                "REG_SZ",
                "/d",
                &command_line,
                "/f",
            ])
            .status()
            .context("failed to update the Windows startup registry")?;
        if !status.success() {
            bail!("Windows rejected the startup registry update");
        }
        Ok(())
    }

    pub fn disable() -> Result<()> {
        if !status()?.enabled {
            return Ok(());
        }
        let status = Command::new("reg")
            .args(["delete", REGISTRY_KEY, "/v", VALUE_NAME, "/f"])
            .status()
            .context("failed to update the Windows startup registry")?;
        if !status.success() {
            bail!("Windows rejected removal of the startup registry value");
        }
        Ok(())
    }
}
