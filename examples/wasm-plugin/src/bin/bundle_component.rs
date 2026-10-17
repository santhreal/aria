//! Encodes a `wasm32-unknown-unknown` core module into a WebAssembly component.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = env::args_os().skip(1);
    let Some(input) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: bundle-component <core-module.wasm> <output-component.wasm> [extra-output.wasm]"
        );
        return ExitCode::from(2);
    };
    let Some(output) = args.next().map(PathBuf::from) else {
        eprintln!(
            "usage: bundle-component <core-module.wasm> <output-component.wasm> [extra-output.wasm]"
        );
        return ExitCode::from(2);
    };
    let extra_output = args.next().map(PathBuf::from);

    let core_bytes = match fs::read(&input) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("failed to read {}: {err}", input.display());
            return ExitCode::from(1);
        }
    };

    let component_bytes = match wit_component::ComponentEncoder::default()
        .module(&core_bytes)
        .and_then(|encoder| encoder.validate(true).encode())
    {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!(
                "failed to encode component from {}: {err:#}",
                input.display()
            );
            return ExitCode::from(1);
        }
    };

    if let Err(err) = write_output(&output, &component_bytes) {
        eprintln!("{err}");
        return ExitCode::from(1);
    }

    if let Some(extra) = extra_output {
        if let Err(err) = write_output(&extra, &component_bytes) {
            eprintln!("{err}");
            return ExitCode::from(1);
        }
    }

    ExitCode::SUCCESS
}

fn write_output(path: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
        }
    }
    fs::write(path, bytes).map_err(|err| format!("failed to write {}: {err}", path.display()))
}
