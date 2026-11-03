//! `upyr shell-init`: prompt widgets that convert the command line or the
//! word before the cursor with `upyr convert`, for terminals where the
//! desktop app cannot select and paste.

/// A shell with Upyr widgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Zsh,
    Bash,
    Fish,
}

/// The widget script for `shell`, calling Upyr at `program`.
pub fn script(shell: Shell, program: &str) -> String {
    let (template, quoted) = match shell {
        Shell::Zsh => (include_str!("shell/upyr.zsh"), posix_quoted(program)),
        Shell::Bash => (include_str!("shell/upyr.bash"), posix_quoted(program)),
        Shell::Fish => (include_str!("shell/upyr.fish"), fish_quoted(program)),
    };
    template.replace("@UPYR_BIN@", &quoted)
}

/// Single quotes, with each `'` closed, escaped and reopened.
fn posix_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Fish single quotes, where only `\'` and `\\` are escapes.
fn fish_quoted(text: &str) -> String {
    format!("'{}'", text.replace('\\', r"\\").replace('\'', r"\'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_script_calls_the_given_program_and_binds_both_widgets() {
        for shell in [Shell::Zsh, Shell::Bash, Shell::Fish] {
            let script = script(shell, "/opt/upyr/bin/upyr");
            assert!(!script.contains("@UPYR_BIN@"), "{shell:?}");
            assert!(script.contains("'/opt/upyr/bin/upyr'"), "{shell:?}");
            assert!(script.contains("upyr-convert-line"), "{shell:?}");
            assert!(script.contains("upyr-convert-word"), "{shell:?}");
            assert!(script.contains("convert --installed --"), "{shell:?}");
        }
    }

    #[test]
    fn the_program_path_is_quoted_for_each_shell() {
        let path = "/Users/o'neil/My Apps/upyr";
        assert!(script(Shell::Zsh, path).contains(r"'/Users/o'\''neil/My Apps/upyr'"));
        assert!(script(Shell::Bash, path).contains(r"'/Users/o'\''neil/My Apps/upyr'"));
        assert!(script(Shell::Fish, path).contains(r"'/Users/o\'neil/My Apps/upyr'"));
        assert!(script(Shell::Fish, r"C:\upyr").contains(r"'C:\\upyr'"));
    }
}
