//! `aria` — the editor's command line.
//!
//! Everything here drives the same crates the editor drives: `aria-models` loads
//! the project, `aria-render` resolves and composites it, `aria-encoder` writes
//! the file, `aria-detect` feeds the detectors. Nothing is reimplemented for the
//! terminal, so a frame rendered here is the frame the editor shows, and a
//! detector run here finds what the editor finds.
//!
//! ```text
//! aria demo out.png
//! aria frame project.aria still.png --at 12.5
//! aria export project.aria out.mp4 --at 0 --to 30 --codec h265
//! aria inspect project.aria --clips
//! aria analyze voice.wav --silence
//! ```

// Every module's `print!` family writes through `out` rather than std's, which
// panics on a closed pipe. A `macro_rules!` defined here is in scope in every
// module declared below it, ahead of the std prelude's.
macro_rules! print {
    ($($arg:tt)*) => { $crate::out::stdout(format_args!($($arg)*)) };
}
macro_rules! println {
    () => { $crate::out::stdout(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::out::stdout(format_args!("{}\n", format_args!($($arg)*))) };
}
macro_rules! eprint {
    ($($arg:tt)*) => { $crate::out::stderr(format_args!($($arg)*)) };
}
macro_rules! eprintln {
    () => { $crate::out::stderr(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::out::stderr(format_args!("{}\n", format_args!($($arg)*))) };
}

mod analyze;
mod args;
#[cfg(test)]
mod count_tests;
mod demo;
mod export;
mod fail;
mod frame;
mod inspect;
mod new;
mod out;
mod plugin;
mod project;

use std::borrow::Cow;

use args::{Args, ArgsError, Verb};
use fail::Failure;

fn main() -> ! {
    let code = {
        let argv: Vec<String> = std::env::args().skip(1).collect();
        let verb = argv.first().and_then(|word| Verb::named(word));
        match Args::parse(argv) {
            Ok(args) => match run(&args) {
                Ok(()) => 0,
                Err(failure) => report(&failure, verb),
            },
            Err(ArgsError::NoVerb) => {
                print!("{}", args::USAGE);
                0
            }
            Err(ArgsError::Help(verb)) => {
                print!("{}", help_text(verb));
                0
            }
            Err(ArgsError::Version) => {
                println!("aria {}", env!("CARGO_PKG_VERSION"));
                0
            }
            Err(err) => report(&Failure::from(err), verb),
        }
    };
    // An export decodes its sources, and a process that decoded on NVDEC must
    // not end through `exit` (see `exit_process`). Every value of the command
    // is dropped by here.
    aria_decoder::exit_process(out::finish(code))
}

fn run(args: &Args) -> Result<(), Failure> {
    // The built-in driver language, so a driven property renders here as it
    // does in the editor. Held until the command ends.
    let builtin = aria_plugin::PluginContext::new();
    builtin
        .mount(&aria_plugin::ExpressionsPlugin)
        .map_err(|err| Failure::Failed(err.to_string()))?;
    if args.verb != Verb::Plugin {
        // `aria plugin` checks the flags and arity of its sub-command instead.
        args.reject_unknown(args.verb.flags())?;
        if let Some(max) = args.verb.max_positionals() {
            args.reject_extra_positional(max)?;
        }
        args.reject_unwritable_output()?;
        args.reject_misnamed_output()?;
    }
    match args.verb {
        Verb::Demo => demo::run(args.positional.first().map(Into::into)).map_err(Failure::Failed),
        Verb::New => new::run(args),
        Verb::Frame => frame::run(args),
        Verb::Export => export::run(args),
        Verb::Inspect => inspect::run(args),
        Verb::Analyze => analyze::run(args),
        Verb::Plugin => plugin::run(args),
    }
}

/// Print a failure the way a shell expects and return its code.
fn report(failure: &Failure, verb: Option<Verb>) -> u8 {
    eprint!("{}", report_text(failure, verb));
    u8::try_from(failure.code()).unwrap_or(1)
}

/// The reason, then for a wrong command line the synopsis of the command it
/// names (none for an unknown command) and where the full list is.
fn report_text(failure: &Failure, verb: Option<Verb>) -> String {
    let mut text = format!("aria: {}\n", failure.message());
    if matches!(failure, Failure::Usage(_)) {
        text.push('\n');
        if let Some(verb) = verb {
            text.push_str(&args::synopsis(verb));
            text.push('\n');
        }
        text.push_str(EVERY_COMMAND);
    }
    text
}

/// The line after a single command's entries that points to the full list.
const EVERY_COMMAND: &str = "Run `aria --help` for every command.\n";

/// What `--help` prints: after a command, the entries of that command and
/// where the full list is; otherwise the usage block.
fn help_text(verb: Option<Verb>) -> Cow<'static, str> {
    match verb {
        Some(verb) => Cow::Owned(format!("{}\n{EVERY_COMMAND}", args::synopsis(verb))),
        None => Cow::Borrowed(args::USAGE),
    }
}
