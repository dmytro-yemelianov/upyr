use std::{env, fs, path::PathBuf};

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartStatus {
    pub enabled: bool,
    pub location: String,
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
    #[cfg(target_os = "macos")]
    for ancestor in current.ancestors() {
        if ancestor.file_name().and_then(|name| name.to_str()) == Some("Helpers")
            && let Some(host_contents) = ancestor.parent()
        {
            let candidate = host_contents.join("MacOS/upyr-background");
            if candidate.is_file() {
                return Ok(fs::canonicalize(&candidate).unwrap_or(candidate));
            }
        }
    }
    bail!("the background Upyr executable is missing next to Upyr Settings")
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{fs, path::PathBuf};

    use anyhow::{Context, Result};
    use directories::BaseDirs;

    use super::AutostartStatus;

    const LABEL: &str = "dev.Upyr.Upyr";

    pub fn status() -> Result<AutostartStatus> {
        let path = entry_path()?;
        Ok(AutostartStatus {
            enabled: path.is_file(),
            location: path.display().to_string(),
        })
    }

    pub fn enable() -> Result<()> {
        let path = entry_path()?;
        let executable = super::background_executable()?;
        let contents = launch_agent_contents(&executable);
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
        use std::path::Path;

        use super::*;

        #[test]
        fn launch_agent_escapes_executable_path() {
            let contents = launch_agent_contents(Path::new("/tmp/Upyr & Friends/upyr"));

            assert!(contents.contains("/tmp/Upyr &amp; Friends/upyr"));
            assert!(contents.contains("<string>run</string>"));
            assert!(contents.contains(LABEL));
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
        Ok(AutostartStatus {
            enabled: path.is_file(),
            location: path.display().to_string(),
        })
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
        Ok(AutostartStatus {
            enabled: output.status.success(),
            location: format!(r"{REGISTRY_KEY}\{VALUE_NAME}"),
        })
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
