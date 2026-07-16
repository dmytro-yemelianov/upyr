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
        if startup.enabled {
            "enabled"
        } else {
            "disabled"
        },
        startup.location
    );

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

fn manage_autostart(action: AutostartAction) -> Result<()> {
    let status = match action {
        AutostartAction::Enable => autostart::enable()?,
        AutostartAction::Disable => autostart::disable()?,
        AutostartAction::Status => autostart::status()?,
    };
    println!(
        "Launch at login is {} ({})",
        if status.enabled {
            "enabled"
        } else {
            "disabled"
        },
        status.location
    );
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
