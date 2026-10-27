use std::io::{self, IsTerminal, Read};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use tracing_subscriber::EnvFilter;
use upyr::{
    Direction, app, autostart,
    config::{Config, config_path},
    convert, convert_with_mapping, system_layout,
};

#[derive(Debug, Parser)]
#[command(
    name = "upyr",
    version,
    about = "Fix text typed with the wrong English/Ukrainian keyboard layout"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Configure the integrated macOS Globe-key switcher.
    #[cfg(target_os = "macos")]
    Globe {
        #[command(subcommand)]
        action: GlobeAction,
    },
    /// Run the background global-hotkey listener (the default command).
    Run {
        /// Override the configured shortcut, e.g. CmdOrCtrl+Alt+Space.
        #[arg(long)]
        hotkey: Option<String>,
    },
    /// Convert provided text, or stdin when no text argument is given.
    Convert {
        #[arg(short, long, value_enum, default_value_t = CliDirection::Smart)]
        direction: CliDirection,
        /// Derive character pairs from installed OS layouts instead of using the built-in map.
        #[arg(long)]
        installed: bool,
        #[arg(allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Write a documented default configuration file.
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Print the configuration file path.
    ConfigPath,
    /// Open the native settings window.
    Settings,
    /// Print runtime configuration and active input-source diagnostics.
    Doctor,
    /// Manage user-level launch-at-login integration.
    Autostart {
        #[command(subcommand)]
        action: AutostartAction,
    },
}

#[cfg(target_os = "macos")]
#[derive(Debug, Subcommand)]
enum GlobeAction {
    /// Enable clean Globe-key taps in Upyr's background process.
    Enable,
    /// Disable switching and restore the previous macOS Globe-key setting.
    Disable,
    /// Restore the macOS setting after an unclean exit; also disables switching.
    Restore,
    /// Uninstall standalone GlobeSwitch, then enable the integrated switcher.
    Migrate,
    /// Print configuration, permission and standalone-agent diagnostics.
    Status,
    /// Print enabled input-source IDs for globe_switch.skipped_layouts.
    List,
}

#[derive(Debug, Clone, Copy, Subcommand)]
enum AutostartAction {
    Enable,
    Disable,
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliDirection {
    Smart,
    EnglishToUkrainian,
    UkrainianToEnglish,
}

impl From<CliDirection> for Direction {
    fn from(direction: CliDirection) -> Self {
        match direction {
            CliDirection::Smart => Self::Smart,
            CliDirection::EnglishToUkrainian => Self::EnglishToUkrainian,
            CliDirection::UkrainianToEnglish => Self::UkrainianToEnglish,
        }
    }
}

fn main() -> Result<()> {
    initialize_logging();
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run { hotkey: None }) {
        #[cfg(target_os = "macos")]
        Command::Globe { action } => manage_globe(action),
        Command::Run { hotkey } => {
            let mut config = Config::load()?;
            if let Some(hotkey) = hotkey {
                config.hotkey = hotkey;
            }
            app::run(config)
        }
        Command::Convert {
            direction,
            installed,
            text,
        } => {
            let input = read_input(text)?;
            let direction = direction.into();
            let conversion = if installed {
                let mapping = system_layout::installed_mapping()?
                    .context("the OS did not expose a usable English/Ukrainian mapping")?;
                convert_with_mapping(&input, direction, &mapping)
            } else {
                convert(&input, direction)
            };
            print!("{}", conversion.text);
            if io::stdin().is_terminal() {
                println!();
            }
            Ok(())
        }
        Command::Init { force } => {
            let path = Config::default().write(force)?;
            println!("Created {}", path.display());
            Ok(())
        }
        Command::ConfigPath => {
            println!("{}", config_path()?.display());
            Ok(())
        }
        Command::Settings => upyr::settings::run(),
        Command::Doctor => doctor(),
        Command::Autostart { action } => manage_autostart(action),
    }
}

fn doctor() -> Result<()> {
    let path = config_path()?;
    let config = Config::load()?;
    println!("Upyr {}", env!("CARGO_PKG_VERSION"));
    println!("Platform: {}", std::env::consts::OS);
    println!(
        "Configuration: {} ({})",
        path.display(),
        if path.exists() {
            "loaded"
        } else {
            "using defaults"
        }
    );
    println!("Layout following: {}", config.switch_layout);
    println!("Config schema: {}", config.config_version);
    #[cfg(target_os = "macos")]
    {
        println!(
            "Globe-key switching: {} ({:?}, {} ms)",
            config.globe_switch.enabled,
            config.globe_switch.cycle_order,
            config.globe_switch.tap_window_ms
        );
        println!(
            "Globe-key diagnostics: {}",
            upyr::globe_switch::diagnostics()
        );
    }
    #[cfg(target_os = "macos")]
    println!(
        "Accessibility access: {}",
        if upyr::accessibility::is_trusted() {
            "granted"
        } else {
            "not granted"
        }
    );
    println!(
        "Automatic correction: {} ({:?}, minimum {} characters)",
        if config.auto_correct {
            "enabled"
        } else {
            "disabled"
        },
        config.auto_correct_sensitivity,
        config.auto_correct_min_word_length
    );
    println!(
        "Terminal typing apps: {}",
        if config.terminal_typing_apps.is_empty() {
            "disabled".to_owned()
        } else {
            config.terminal_typing_apps.join(", ")
        }
    );
    println!(
        "Slash-command apps: {}",
        if config.slash_command_apps.is_empty() {
            "disabled".to_owned()
        } else {
            config.slash_command_apps.join(", ")
        }
    );
    #[cfg(target_os = "macos")]
    {
        let output = upyr::audio_output::current();
        println!("Undo correction shortcut: {}", config.undo_hotkey);
        println!(
            "Audio output: {} — {}; Upyr volume {}% (headphone limit {}%)",
            output.name,
            output.kind.label(),
            output.kind.volume(&config.sounds),
            config.sounds.headphone_volume_percent
        );
    }
    println!(
        "Modifier gesture: {:?} -> {:?} ({} ms)",
        config.modifier_gesture, config.modifier_gesture_action, config.modifier_gesture_timeout_ms
    );
    match system_layout::installed_mapping() {
        Ok(Some(mapping)) => println!("Character mapping: {} OS-derived pairs", mapping.len()),
        Ok(None) => println!("Character mapping: built-in English/Ukrainian fallback"),
        Err(error) => println!("Character mapping: built-in fallback ({error:#})"),
    }
    let startup = autostart::status()?;
    println!(
        "Launch at login: {} ({})",
        match startup.state {
            autostart::AutostartState::Disabled => "disabled",
            autostart::AutostartState::Enabled => "enabled",
            autostart::AutostartState::Stale => "stale",
            autostart::AutostartState::Broken => "broken",
        },
        startup.location
    );
    if let Some(detail) = startup.detail {
        println!("Launch at login detail: {detail}");
    }

    match system_layout::current() {
        Ok(Some(source)) => println!(
            "Active input source: {} ({}) — {}",
            source.source_name,
            source.source_id,
            source
                .layout
                .map_or("not mapped".to_owned(), |layout| format!("{layout:?}"))
        ),
        Ok(None) => println!("Active input source: unavailable on this platform"),
        Err(error) => println!("Active input source: unavailable ({error:#})"),
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn manage_globe(action: GlobeAction) -> Result<()> {
    if matches!(action, GlobeAction::List) {
        for source in system_layout::enabled_sources()? {
            println!("{}\t{}", source.source_id, source.source_name);
        }
        return Ok(());
    }
    let mut config = Config::load()?;
    if matches!(action, GlobeAction::Status) {
        println!("Globe-key switching: {}", config.globe_switch.enabled);
        println!("{}", upyr::globe_switch::diagnostics());
        return Ok(());
    }
    if matches!(action, GlobeAction::Migrate) {
        let home = directories::BaseDirs::new().context("could not find the home directory")?;
        let executable = home
            .home_dir()
            .join("Applications/GlobeSwitch.app/Contents/MacOS/globe-switch");
        let agent = home
            .home_dir()
            .join("Library/LaunchAgents/dev.yemelianov.globe-switch.plist");
        if executable.is_file() {
            let status = std::process::Command::new(&executable)
                .arg("uninstall")
                .status()
                .context("could not uninstall standalone GlobeSwitch")?;
            anyhow::ensure!(
                status.success(),
                "standalone GlobeSwitch uninstall failed; Upyr was not enabled"
            );
        } else {
            anyhow::ensure!(
                !agent.exists(),
                "standalone GlobeSwitch has no uninstaller; run `globe-switch uninstall` first"
            );
        }
    }
    config.globe_switch.enabled = matches!(action, GlobeAction::Enable | GlobeAction::Migrate);
    config.write(true)?;
    if !config.globe_switch.enabled {
        upyr::globe_switch::restore_system_setting()?;
    }
    println!(
        "Globe-key switching {}. Running Upyr reloads this setting automatically.",
        if config.globe_switch.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    if config.globe_switch.enabled {
        println!("Start Upyr if it is not running. Grant input permission if requested.");
    }
    Ok(())
}

fn manage_autostart(action: AutostartAction) -> Result<()> {
    let status = match action {
        AutostartAction::Enable => autostart::enable()?,
        AutostartAction::Disable => autostart::disable()?,
        AutostartAction::Status => autostart::status()?,
    };
    println!(
        "Launch at login is {} ({})",
        match status.state {
            autostart::AutostartState::Disabled => "disabled",
            autostart::AutostartState::Enabled => "enabled",
            autostart::AutostartState::Stale => "stale",
            autostart::AutostartState::Broken => "broken",
        },
        status.location
    );
    if let Some(detail) = status.detail {
        println!("Launch at login detail: {detail}");
    }
    Ok(())
}

fn read_input(arguments: Vec<String>) -> Result<String> {
    if !arguments.is_empty() {
        return Ok(arguments.join(" "));
    }
    if io::stdin().is_terminal() {
        bail!("provide text as an argument or pipe it on stdin");
    }

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("failed to read stdin")?;
    Ok(input)
}

fn initialize_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("upyr=info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
