//! Local line-oriented terminal adapter. No raw mode, global hooks or network.
use super::i18n::{self, tr};
use super::{
    draft::{SettingsAction, SettingsDraft},
    persistence::{self, LocalStore, SettingsStore},
    sensitivity_label,
};
use crate::config::AutoCorrectSensitivity;
use anyhow::{Context, Result, bail};
use std::io::{self, BufRead, IsTerminal, Write};

const HELP: &str = "tab correction|exceptions · auto on|off · layout on|off\n\
sensitivity conservative|balanced|aggressive · min 2..32 · delay 10..250\n\
add WORDS · edit ROW WORDS · remove ROW (rows start at 1)\n\
save · revert · reload · reload! (discard) · quit · quit! (discard) · help";

enum Command {
    Edit(SettingsAction),
    Tab(bool),
    Save,
    Reload(bool),
    Quit(bool),
    Help,
}

fn parse(line: &str) -> Result<Command> {
    let (name, value) = line
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((line.trim(), ""));
    let value = value.trim();
    let boolean = || match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => bail!("Use on or off"),
    };
    let row = |value: &str| -> Result<usize> {
        value
            .parse::<usize>()
            .context("Use a row number starting at 1")?
            .checked_sub(1)
            .context("Rows start at 1")
    };
    Ok(match name.to_lowercase().as_str() {
        "auto" => Command::Edit(SettingsAction::Automatic(boolean()?)),
        "layout" => Command::Edit(SettingsAction::SwitchLayout(boolean()?)),
        "min" => Command::Edit(SettingsAction::Minimum(value.into())),
        "delay" => Command::Edit(SettingsAction::Delay(value.into())),
        "sensitivity" => Command::Edit(SettingsAction::Sensitivity(match value {
            "conservative" => AutoCorrectSensitivity::Conservative,
            "balanced" => AutoCorrectSensitivity::Balanced,
            "aggressive" => AutoCorrectSensitivity::Aggressive,
            _ => bail!("Choose conservative, balanced or aggressive"),
        })),
        "add" => Command::Edit(SettingsAction::AddExceptions(value.into())),
        "edit" => {
            let (index, words) = value
                .split_once(char::is_whitespace)
                .context("Use edit ROW WORDS")?;
            Command::Edit(SettingsAction::EditException(
                row(index)?,
                words.trim().into(),
            ))
        }
        "remove" => Command::Edit(SettingsAction::RemoveException(row(value)?)),
        "tab" => Command::Tab(match value {
            "correction" => false,
            "exceptions" => true,
            _ => bail!("Choose correction or exceptions"),
        }),
        "save" if value.is_empty() => Command::Save,
        "revert" if value.is_empty() => Command::Edit(SettingsAction::Revert),
        "reload" | "reload!" if value.is_empty() => Command::Reload(name.ends_with('!')),
        "quit" | "quit!" if value.is_empty() => Command::Quit(name.ends_with('!')),
        "help" | "" if value.is_empty() => Command::Help,
        _ => bail!("Unknown command. Type help for available commands."),
    })
}

fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

fn render(
    out: &mut impl Write,
    draft: &SettingsDraft,
    exceptions: bool,
    status: &str,
    ansi: bool,
) -> Result<()> {
    i18n::set_language(draft.edited.ui_language);
    if ansi {
        write!(out, "\x1b[2J\x1b[H")?;
    }
    writeln!(
        out,
        "{}",
        i18n::localized_format!(
            "+-- UPYR SETTINGS / {} -- {} --+",
            "+-- НАЛАШТУВАННЯ UPYR / {} -- {} --+",
            if exceptions {
                tr("EXCEPTIONS")
            } else {
                tr("CORRECTION")
            },
            if draft.dirty() {
                tr("UNSAVED")
            } else {
                tr("SAVED")
            }
        )
    )?;
    writeln!(out, "{}", tr("Local editor · Full settings: upyr settings"))?;
    if exceptions {
        if draft.edited.auto_correct_exceptions.is_empty() {
            writeln!(out, "{}", tr("No exceptions yet."))?;
        }
        for (index, word) in draft.edited.auto_correct_exceptions.iter().enumerate() {
            writeln!(out, "{:>3}. {}", index + 1, printable(word))?;
        }
    } else {
        writeln!(
            out,
            "{}",
            i18n::localized_format!(
                "Automatic: {}   Switch OS layout: {}",
                "Автовиправлення: {}   Перемикання розкладки: {}",
                draft.edited.auto_correct,
                draft.edited.switch_layout
            )
        )?;
        writeln!(
            out,
            "{}",
            i18n::localized_format!(
                "Sensitivity: {}",
                "Чутливість: {}",
                sensitivity_label(draft.edited.auto_correct_sensitivity)
            )
        )?;
        writeln!(
            out,
            "{}",
            i18n::localized_format!(
                "Minimum: {}   Pause after Space: {} ms",
                "Мінімум: {}   Пауза після пробілу: {} мс",
                printable(&draft.minimum),
                printable(&draft.delay)
            )
        )?;
        writeln!(
            out,
            "{}\n{}",
            tr(super::navigation::MIDWORD_HELP),
            tr(super::navigation::CONTEXT_HELP)
        )?;
    }
    if let Err(error) = draft.candidate() {
        writeln!(
            out,
            "{}",
            i18n::localized_format!(
                "Invalid: {}",
                "Помилка: {}",
                printable(&i18n::diagnostic(&format!("{error:#}")))
            )
        )?;
    }
    if !status.is_empty() {
        writeln!(out, "{}", printable(&i18n::diagnostic(status)))?;
    }
    let help = if i18n::language() == crate::config::UiLanguage::Ukrainian {
        "tab correction|exceptions · auto on|off · layout on|off\n\
sensitivity conservative|balanced|aggressive · min 2..32 · delay 10..250\n\
add СЛОВА · edit РЯДОК СЛОВА · remove РЯДОК (нумерація від 1)\n\
save (зберегти) · revert (відкотити) · reload (перечитати) · reload! (відкинути зміни)\n\
quit (вийти) · quit! (відкинути зміни й вийти) · help (довідка)"
    } else {
        HELP
    };
    writeln!(out, "{help}")?;
    write!(out, "upyr> ")?;
    out.flush()?;
    Ok(())
}

pub(super) fn run() -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let ansi = stdin.is_terminal() && stdout.is_terminal();
    session(&mut stdin.lock(), &mut stdout.lock(), &LocalStore, ansi)
}

fn session(
    input: &mut impl BufRead,
    output: &mut impl Write,
    store: &impl SettingsStore,
    ansi: bool,
) -> Result<()> {
    let mut draft = SettingsDraft::new(store.load()?);
    let mut exceptions = false;
    let mut status = String::new();
    loop {
        render(output, &draft, exceptions, &status, ansi)?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            anyhow::ensure!(
                !draft.dirty(),
                "Input ended with unsaved changes; nothing was saved"
            );
            return Ok(());
        }
        let result = match parse(&line) {
            Ok(Command::Edit(action)) => draft.apply(action).map(|_| String::new()),
            Ok(Command::Tab(value)) => {
                exceptions = value;
                Ok(String::new())
            }
            Ok(Command::Save) => persistence::save(&mut draft, store)
                .map(|_| "Saved. Upyr will reload these settings.".into()),
            Ok(Command::Reload(discard)) => {
                if draft.dirty() && !discard {
                    Ok("Unsaved changes. Use save, revert or reload!.".into())
                } else {
                    persistence::reload(&mut draft, store).map(|_| "Reloaded from disk.".into())
                }
            }
            Ok(Command::Quit(discard)) => {
                if !draft.dirty() || discard {
                    writeln!(output, "{}", tr("Closed without further changes."))?;
                    return Ok(());
                }
                Ok("Unsaved changes. Use save, revert or quit!.".into())
            }
            Ok(Command::Help) => Ok(String::new()),
            Err(error) => Err(error),
        };
        status = result.unwrap_or_else(|error| format!("Error: {error:#}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::cell::RefCell;
    struct Store(RefCell<Config>);
    impl SettingsStore for Store {
        fn load(&self) -> Result<Config> {
            Ok(self.0.borrow().clone())
        }
        fn write(&self, config: &Config) -> Result<()> {
            *self.0.borrow_mut() = config.clone();
            Ok(())
        }
    }

    #[test]
    fn terminal_uses_ukrainian_default_with_existing_commands() {
        let store = Store(RefCell::new(Config::default()));
        let mut output = Vec::new();
        session(
            &mut io::Cursor::new("add General, Їжак\nsave\nquit\n".as_bytes()),
            &mut output,
            &store,
            false,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("НАЛАШТУВАННЯ UPYR"));
        assert!(output.contains("Збережено. Upyr застосує ці налаштування."));
        assert!(output.contains("Закрито без подальших змін."));
        assert_eq!(
            store.0.borrow().auto_correct_exceptions,
            ["General", "Їжак"]
        );
    }
    #[test]
    fn terminal_replay_uses_shared_edits_and_refuses_dirty_discard() {
        let store = Store(RefCell::new(Config {
            ui_language: crate::config::UiLanguage::English,
            ..Config::default()
        }));
        let mut input = io::Cursor::new("auto on\nmin 6\nadd Ґанок, Їжак їжак\nedit 1 Іван Єва\nsave\nmin invalid\nquit\nreload\nrevert\nquit\n".as_bytes());
        let mut output = Vec::new();
        session(&mut input, &mut output, &store, false).unwrap();
        let saved = store.0.borrow();
        assert_eq!(saved.auto_correct_exceptions, ["Іван", "Єва", "Їжак"]);
        assert_eq!(saved.auto_correct_min_word_length, 6);
        assert!(saved.auto_correct);
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("Use save, revert or quit!"));
        assert!(output.contains("Use save, revert or reload!"));
        assert!(!output.contains('\x1b'));
    }
    #[test]
    fn terminal_output_escapes_control_text_and_eof_never_saves() {
        assert_eq!(printable("Ґ\x1b[2J\r\n\u{9b}Є"), "Ґ�[2J���Є");
        let store = Store(RefCell::new(Config {
            ui_language: crate::config::UiLanguage::English,
            ..Config::default()
        }));
        assert!(
            session(
                &mut io::Cursor::new(b"min 7\n"),
                &mut Vec::new(),
                &store,
                false
            )
            .is_err()
        );
        assert_eq!(store.0.borrow().auto_correct_min_word_length, 4);
        assert!(parse("remove 0").is_err());
        assert!(parse("save unexpected").is_err());
    }
}
