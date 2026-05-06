// Slash commands. Adding a command = one declarative entry.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandAction {
    NewSession,
    OpenSessions,
    ShowHelp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandError {
    Empty,
    Unknown(String),
    Busy(&'static str),
    UnexpectedArgs(&'static str),
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty command"),
            Self::Unknown(name) => write!(f, "unknown command: /{name}  (try /help)"),
            Self::Busy(name) => write!(f, "/{name}: finish the current turn first"),
            Self::UnexpectedArgs(name) => {
                write!(f, "/{name}: this command does not take arguments")
            }
        }
    }
}

pub(crate) struct Command {
    pub(crate) name: &'static str,
    pub(crate) help: &'static str,
    pub(crate) idle_only: bool,
    action: CommandAction,
}

const COMMANDS: &[Command] = &[
    Command {
        name: "new",
        help: "start a new session",
        idle_only: true,
        action: CommandAction::NewSession,
    },
    Command {
        name: "sessions",
        help: "pick a saved session to resume",
        idle_only: true,
        action: CommandAction::OpenSessions,
    },
    Command {
        name: "help",
        help: "list commands",
        idle_only: false,
        action: CommandAction::ShowHelp,
    },
];

/// The portion of the input that's still naming a command, or `None` if the
/// input isn't a `/` command being typed (no slash, or whitespace seen).
fn typing_command(input: &str) -> Option<&str> {
    input
        .strip_prefix('/')
        .filter(|s| !s.contains(char::is_whitespace))
}

pub(crate) fn palette_active(input: &str) -> bool {
    typing_command(input).is_some()
}

pub(crate) fn palette_matches(input: &str) -> Vec<&'static Command> {
    let Some(prefix) = typing_command(input) else {
        return Vec::new();
    };
    COMMANDS
        .iter()
        .filter(|c| c.name.starts_with(prefix))
        .collect()
}

pub(crate) fn dispatch(input: &str, busy: bool) -> Result<CommandAction, CommandError> {
    let (name, args) = match input.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, rest.trim()),
        None => (input, ""),
    };

    if name.is_empty() {
        return Err(CommandError::Empty);
    }

    let Some(cmd) = COMMANDS.iter().find(|c| c.name == name) else {
        return Err(CommandError::Unknown(name.into()));
    };

    if cmd.idle_only && busy {
        return Err(CommandError::Busy(cmd.name));
    }

    if !args.is_empty() {
        return Err(CommandError::UnexpectedArgs(cmd.name));
    }

    Ok(cmd.action)
}

pub(crate) fn help_text() -> String {
    let width = COMMANDS.iter().map(|c| c.name.len()).max().unwrap_or(0);
    let mut body = String::from("Commands:\n");
    for cmd in COMMANDS {
        body.push_str(&format!(
            "  /{:<width$}  {}\n",
            cmd.name,
            cmd.help,
            width = width
        ));
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_active_only_when_naming_a_command() {
        assert!(palette_active("/"));
        assert!(palette_active("/n"));
        assert!(palette_active("/sessions"));
        assert!(!palette_active(""));
        assert!(!palette_active("hello"));
        assert!(!palette_active("/sessions abc")); // past the name
    }

    #[test]
    fn palette_matches_filters_by_prefix() {
        let names = |input: &str| -> Vec<&str> {
            palette_matches(input).into_iter().map(|c| c.name).collect()
        };

        assert_eq!(names("/"), vec!["new", "sessions", "help"]);
        assert_eq!(names("/se"), vec!["sessions"]);
        assert_eq!(names("/h"), vec!["help"]);
        assert!(names("/x").is_empty());
        // Whitespace hides the palette regardless of prefix match.
        assert!(names("/help foo").is_empty());
    }

    #[test]
    fn dispatch_rejects_unknown_command() {
        assert_eq!(
            dispatch("nope", false),
            Err(CommandError::Unknown("nope".into()))
        );
    }

    #[test]
    fn dispatch_idle_only_command_blocked_during_turn() {
        assert_eq!(dispatch("new", true), Err(CommandError::Busy("new")));
    }

    #[test]
    fn dispatch_rejects_args_for_no_arg_commands() {
        assert_eq!(
            dispatch("new extra", false),
            Err(CommandError::UnexpectedArgs("new"))
        );
    }

    #[test]
    fn dispatch_returns_command_action() {
        assert_eq!(dispatch("new", false), Ok(CommandAction::NewSession));
        assert_eq!(dispatch("sessions", false), Ok(CommandAction::OpenSessions));
        assert_eq!(dispatch("help", true), Ok(CommandAction::ShowHelp));
    }
}
