use std::path::{Path, PathBuf};

/// Splits a Windows command line into a program and its arguments.
///
/// Whitespace separates arguments unless it is inside a double-quoted section.
/// A run of backslashes is literal unless it precedes a quote, matching the
/// `CommandLineToArgvW` rules the Windows loader applies to the registered
/// executable path already stored in the database.
pub fn split(command: &str) -> Vec<String> {
    let mut parts = vec![];
    let mut current = String::new();
    let mut quoted = false;
    let mut started = false;
    let mut backslashes = 0usize;
    for character in command.chars() {
        match character {
            '\\' => {
                backslashes += 1;
                started = true;
            }
            '"' => {
                current.push_str(&"\\".repeat(backslashes / 2));
                if backslashes % 2 == 1 {
                    current.push('"');
                } else {
                    quoted = !quoted;
                }
                backslashes = 0;
                started = true;
            }
            _ if character.is_whitespace() && !quoted => {
                current.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                if started {
                    parts.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            _ => {
                current.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                current.push(character);
                started = true;
            }
        }
    }
    current.push_str(&"\\".repeat(backslashes));
    if started {
        parts.push(current);
    }
    parts
}

/// Resolves the program to run for a game.
///
/// A custom command replaces the registered executable as the launch target but
/// keeps the registered executable's directory as the working directory, so a
/// relative program still resolves next to the game. Without a custom command the
/// registered executable is used unchanged.
pub fn resolve(command: Option<&str>, executable: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let Some(command) = command.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok((executable.to_path_buf(), vec![]));
    };
    let mut parts = split(command).into_iter();
    let program = parts
        .next()
        .ok_or_else(|| "起動コマンドを解釈できませんでした。".to_string())?;
    let program = Path::new(&program);
    let program = if program.is_absolute() {
        program.to_path_buf()
    } else {
        executable
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(program)
    };
    Ok((program, parts.collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn parsed(command: &str) -> Vec<String> {
        split(command)
    }

    #[test]
    fn splits_plain_and_quoted_arguments() {
        assert_eq!(parsed(r"C:\G\game.exe"), vec![r"C:\G\game.exe"]);
        assert_eq!(
            parsed(r#""C:\Program Files\game\game.exe" -w -foo"#),
            vec![r"C:\Program Files\game\game.exe", "-w", "-foo"]
        );
        assert_eq!(
            parsed(r#"game.exe --title "my game""#),
            vec!["game.exe", "--title", "my game"]
        );
        assert!(parsed("   ").is_empty());
    }

    #[test]
    fn keeps_backslash_runs_literal_unless_they_escape_a_quote() {
        assert_eq!(parsed(r"cmd -p C:\a\b\"), vec!["cmd", "-p", r"C:\a\b\"]);
        assert_eq!(
            parsed(r#""C:\a\b\\" next"#),
            vec![r"C:\a\b\", "next"],
            "two backslashes before a closing quote become one literal backslash"
        );
        assert_eq!(
            parsed(r#""a\"b" c"#),
            vec![r#"a"b"#, "c"],
            "an escaped quote stays inside the argument"
        );
    }

    #[test]
    fn resolve_uses_the_registered_executable_without_a_custom_command() {
        let executable = Path::new(r"C:\G\game.exe");
        let (program, args) = resolve(None, executable).unwrap();
        assert_eq!(program, executable);
        assert!(args.is_empty());
        let (program, _) = resolve(Some("   "), executable).unwrap();
        assert_eq!(program, executable, "blank custom command is ignored");
    }

    #[test]
    fn resolve_prefers_the_custom_program_and_keeps_the_game_directory() {
        let executable = Path::new(r"C:\G\game.exe");
        let (program, args) = resolve(Some("launcher.exe --jp"), executable).unwrap();
        assert_eq!(program, Path::new(r"C:\G\launcher.exe"));
        assert_eq!(args, vec!["--jp"]);
        let (program, args) = resolve(Some(r#""D:\Other\a b.exe" -x"#), executable).unwrap();
        assert_eq!(program, Path::new(r"D:\Other\a b.exe"));
        assert_eq!(args, vec!["-x"], "an absolute custom program is used as-is");
    }
}
