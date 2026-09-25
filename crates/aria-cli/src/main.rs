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

mod analyze;
mod args;
mod demo;
mod export;
mod fail;
mod frame;
mod inspect;
mod new;
mod project;

use args::{Args, ArgsError, Verb};
use fail::Failure;

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(argv) {
        Ok(args) => args,
        Err(ArgsError::NoVerb | ArgsError::Help) => {
            print!("{}", args::USAGE);
            return std::process::ExitCode::SUCCESS;
        }
        Err(err) => return report(Failure::from(err)),
    };
    match run(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(failure) => report(failure),
    }
}

fn run(args: &Args) -> Result<(), Failure> {
    match args.verb {
        Verb::Demo => {
            args.reject_unknown(&[])?;
            demo::run(args.positional.first().map(Into::into)).map_err(Failure::Failed)
        }
        Verb::New => {
            args.reject_unknown(new::FLAGS)?;
            new::run(args)
        }
        Verb::Frame => {
            args.reject_unknown(frame::FLAGS)?;
            frame::run(args)
        }
        Verb::Export => {
            args.reject_unknown(export::FLAGS)?;
            export::run(args)
        }
        Verb::Inspect => {
            args.reject_unknown(inspect::FLAGS)?;
            inspect::run(args)
        }
        Verb::Analyze => {
            args.reject_unknown(analyze::FLAGS)?;
            analyze::run(args)
        }
    }
}

/// Print a failure the way a shell expects: the reason on stderr, the usage
/// block only when the command line itself was wrong, and the matching code.
fn report(failure: Failure) -> std::process::ExitCode {
    eprintln!("aria: {}", failure.message());
    if matches!(failure, Failure::Usage(_)) {
        eprintln!();
        eprint!("{}", args::USAGE);
    }
    std::process::ExitCode::from(u8::try_from(failure.code()).unwrap_or(1))
}
