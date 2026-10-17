//! How a command fails, and what the shell sees when it does.
//!
//! Two kinds, because a script has to tell them apart: a wrong command line is
//! the caller's mistake (exit 2, print the usage block), and a failed command is
//! the machine's (exit 1, print the reason). Collapsing them into one code makes
//! a typo indistinguishable from a missing codec.

use crate::args::ArgsError;

#[derive(Debug, PartialEq)]
pub enum Failure {
    /// The command line was wrong.
    Usage(String),
    /// The command ran and could not finish.
    Failed(String),
    /// A child process exited with a specific non-zero status code.
    Exit { code: i32, message: String },
}

impl Failure {
    /// The process exit code for this failure.
    pub fn code(&self) -> i32 {
        match self {
            Failure::Usage(_) => 2,
            Failure::Failed(_) => 1,
            Failure::Exit { code, .. } => *code,
        }
    }

    /// The one-line reason, as printed on stderr.
    pub fn message(&self) -> &str {
        match self {
            Failure::Usage(message) | Failure::Failed(message) | Failure::Exit { message, .. } => {
                message
            }
        }
    }
}

impl From<ArgsError> for Failure {
    fn from(err: ArgsError) -> Self {
        match err {
            ArgsError::Usage(message) => Failure::Usage(message),
            // `--help` and a bare invocation are handled before a command runs;
            // reaching here at all is a parser mistake, and it is reported as
            // one rather than becoming a silent success.
            ArgsError::Help => Failure::Usage("--help is not a command".into()),
            ArgsError::NoVerb => Failure::Usage("no command given".into()),
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Failed(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The class closed here is "every failure exits 1", which makes a typo
    /// indistinguishable from a real failure in a script.
    #[test]
    fn a_usage_mistake_and_a_failed_run_exit_differently() {
        assert_eq!(Failure::from(ArgsError::Usage("bad".into())).code(), 2);
        assert_eq!(Failure::from("device lost".to_string()).code(), 1);
        assert_eq!(
            Failure::from("device lost".to_string()).message(),
            "device lost"
        );
    }
}
