//! CLI entry point for `aria-bench`, the Aria performance harness.

use std::path::PathBuf;
use std::process::ExitCode;

use aria_bench::env_probe::probe_environment;
use aria_bench::fixtures::{FixtureId, ensure_all_fixtures, ensure_fixture};
use aria_bench::scenarios::{BenchContext, run_scenario};
use aria_bench::{
    MachineProfile, ScenarioId, ScenarioReport, Verdict, all_exit_code, default_fixtures_dir,
    default_results_dir, default_workspace_root, write_report,
};

fn usage() -> &'static str {
    "Usage:
  aria-bench fixtures [--fixtures-dir <dir>] [fixture-name]
  aria-bench <scenario> [--machine <Desktop|Server|Laptop>] [--fixtures-dir <dir>] [--results-dir <dir>]
  aria-bench all [--machine <Desktop|Server|Laptop>] [--fixtures-dir <dir>] [--results-dir <dir>]

Scenarios:
  startup, project-io, play-start, playback, av-sync, scrub,
  param-latency, timeline-frame, edit-apply, export, audio-load,
  memory, plugin-cost"
}

struct CliArgs {
    target: String,
    fixture_filter: Option<String>,
    machine: Option<MachineProfile>,
    fixtures_dir: PathBuf,
    results_dir: PathBuf,
}

fn parse_args(args: Vec<String>) -> Result<CliArgs, String> {
    let mut target: Option<String> = None;
    let mut fixture_filter: Option<String> = None;
    let mut machine: Option<MachineProfile> = None;
    let mut fixtures_dir = default_fixtures_dir();
    let mut results_dir = default_results_dir();

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "-h" | "--help" | "help" => return Err(usage().to_owned()),
            "--machine" => {
                idx += 1;
                let val = args
                    .get(idx)
                    .ok_or_else(|| "--machine requires <Desktop|Server|Laptop>".to_owned())?;
                machine = Some(MachineProfile::parse(val)?);
            }
            "--fixtures-dir" => {
                idx += 1;
                let val = args
                    .get(idx)
                    .ok_or_else(|| "--fixtures-dir requires a path".to_owned())?;
                fixtures_dir = PathBuf::from(val);
            }
            "--results-dir" => {
                idx += 1;
                let val = args
                    .get(idx)
                    .ok_or_else(|| "--results-dir requires a path".to_owned())?;
                results_dir = PathBuf::from(val);
            }
            arg if arg.starts_with('-') => {
                return Err(format!("unknown flag '{arg}'\n\n{}", usage()));
            }
            positional => {
                if target.is_none() {
                    target = Some(positional.to_owned());
                } else if target.as_deref() == Some("fixtures") && fixture_filter.is_none() {
                    fixture_filter = Some(positional.to_owned());
                } else {
                    return Err(format!(
                        "unexpected positional argument '{positional}'\n\n{}",
                        usage()
                    ));
                }
            }
        }
        idx += 1;
    }

    let Some(target) = target else {
        return Err(usage().to_owned());
    };

    Ok(CliArgs {
        target,
        fixture_filter,
        machine,
        fixtures_dir,
        results_dir,
    })
}

fn print_report_summary(report: &ScenarioReport, out_path: &std::path::Path) {
    println!("{} -> {}", report.summary_line(), out_path.display());
    for t in &report.targets {
        match t.verdict {
            Verdict::Pass | Verdict::Fail => {
                println!(
                    "  - {:<32} {}: measured={:.3} {} (target {} {:.3} {})",
                    t.name,
                    t.verdict,
                    t.measured.unwrap_or(0.0),
                    t.unit,
                    t.comparison.symbol(),
                    t.target,
                    t.unit
                );
            }
            Verdict::Skipped => {
                println!(
                    "  - {:<32} skipped (required={}): {}",
                    t.name,
                    t.required_on_machine,
                    t.reason.as_deref().unwrap_or("")
                );
            }
        }
    }
    for r in &report.rules {
        println!(
            "  * rule {:<27} {}: {} ({})",
            r.rule,
            r.verdict,
            r.measured
                .map(|n| format!("{n:.1} {}", r.unit))
                .unwrap_or_else(|| "n/a".to_owned()),
            r.reason.as_deref().unwrap_or("ok")
        );
    }
}

fn run_one_and_save(ctx: &BenchContext, scenario: ScenarioId) -> Result<ScenarioReport, String> {
    let report = run_scenario(ctx, scenario)?;
    let out_path = write_report(&ctx.results_dir, &report)?;
    print_report_summary(&report, &out_path);
    Ok(report)
}

fn main() -> ExitCode {
    let raw_args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(raw_args) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };

    if cli.target == "fixtures" {
        if let Some(name) = cli.fixture_filter {
            let Some(id) = FixtureId::parse(&name) else {
                eprintln!("unknown fixture '{name}'");
                return ExitCode::from(2);
            };
            match ensure_fixture(&cli.fixtures_dir, id) {
                Ok(out) => {
                    println!(
                        "{:<16} {:<10} ({} bytes, {:.0}ms) -> {}",
                        out.name,
                        if out.cached { "cached" } else { "generated" },
                        out.bytes,
                        out.generation_ms,
                        out.path.display()
                    );
                    return ExitCode::SUCCESS;
                }
                Err(err) => {
                    eprintln!("fixture {} failed: {err}", id.as_str());
                    return ExitCode::from(1);
                }
            }
        }

        match ensure_all_fixtures(&cli.fixtures_dir) {
            Ok(outcomes) => {
                for out in outcomes {
                    println!(
                        "{:<16} {:<10} ({} bytes, {:.0}ms) -> {}",
                        out.name,
                        if out.cached { "cached" } else { "generated" },
                        out.bytes,
                        out.generation_ms,
                        out.path.display()
                    );
                }
                return ExitCode::SUCCESS;
            }
            Err(err) => {
                eprintln!("fixtures generation failed: {err}");
                return ExitCode::from(1);
            }
        }
    }

    let env = probe_environment(&default_workspace_root(), cli.machine);
    let scratch_dir = std::env::temp_dir().join("aria-bench-work");
    let ctx = BenchContext::new(env, cli.fixtures_dir, cli.results_dir, scratch_dir);

    if cli.target == "all" {
        if let Err(err) = ensure_all_fixtures(&ctx.fixtures_dir) {
            eprintln!("failed to prepare fixtures before running all: {err}");
            return ExitCode::from(1);
        }
        let mut reports = Vec::with_capacity(ScenarioId::ALL.len());
        for scenario in ScenarioId::ALL {
            match run_one_and_save(&ctx, scenario) {
                Ok(report) => reports.push(report),
                Err(err) => {
                    eprintln!("scenario {} failed with error: {err}", scenario.as_str());
                    return ExitCode::from(1);
                }
            }
        }
        let code = all_exit_code(&reports);
        return ExitCode::from(code as u8);
    }

    let scenario = match ScenarioId::parse(&cli.target) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("{err}\n\n{}", usage());
            return ExitCode::from(2);
        }
    };

    match run_one_and_save(&ctx, scenario) {
        Ok(report) => {
            if report.contributes_failure() {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(err) => {
            eprintln!("scenario {} failed: {err}", scenario.as_str());
            ExitCode::from(1)
        }
    }
}
