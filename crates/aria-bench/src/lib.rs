//! Performance benchmark harness for Aria (`docs/spec/performance.md`).
//!
//! Drives the shipped binary and production entry points across the 13
//! release-target scenarios, records raw samples and hardware metadata to
//! `bench/results/<scenario>.json`, and evaluates each measurement against the
//! reference machine's target.

pub mod alloc_tracker;
pub mod env_probe;
pub mod fixtures;
pub(crate) mod process;
pub mod scenarios;

use std::path::{Path, PathBuf};

use aria_plugin::PluginContext;
use serde::{Deserialize, Serialize};

/// Reference machine classes defined in `docs/spec/performance.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MachineProfile {
    Desktop,
    Server,
    Laptop,
}

impl MachineProfile {
    pub fn parse(input: &str) -> Result<Self, String> {
        match input.trim().to_ascii_lowercase().as_str() {
            "desktop" => Ok(Self::Desktop),
            "server" => Ok(Self::Server),
            "laptop" => Ok(Self::Laptop),
            other => Err(format!(
                "unknown machine profile '{other}'; expected Desktop, Server, or Laptop"
            )),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Server => "Server",
            Self::Laptop => "Laptop",
        }
    }
}

impl std::fmt::Display for MachineProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The 13 release-target scenarios defined in `docs/spec/performance.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScenarioId {
    #[serde(rename = "startup")]
    Startup,
    #[serde(rename = "project-io")]
    ProjectIo,
    #[serde(rename = "play-start")]
    PlayStart,
    #[serde(rename = "playback")]
    Playback,
    #[serde(rename = "av-sync")]
    AvSync,
    #[serde(rename = "scrub")]
    Scrub,
    #[serde(rename = "param-latency")]
    ParamLatency,
    #[serde(rename = "timeline-frame")]
    TimelineFrame,
    #[serde(rename = "edit-apply")]
    EditApply,
    #[serde(rename = "export")]
    Export,
    #[serde(rename = "audio-load")]
    AudioLoad,
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "plugin-cost")]
    PluginCost,
}

impl ScenarioId {
    pub const ALL: [Self; 13] = [
        Self::Startup,
        Self::ProjectIo,
        Self::PlayStart,
        Self::Playback,
        Self::AvSync,
        Self::Scrub,
        Self::ParamLatency,
        Self::TimelineFrame,
        Self::EditApply,
        Self::Export,
        Self::AudioLoad,
        Self::Memory,
        Self::PluginCost,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::ProjectIo => "project-io",
            Self::PlayStart => "play-start",
            Self::Playback => "playback",
            Self::AvSync => "av-sync",
            Self::Scrub => "scrub",
            Self::ParamLatency => "param-latency",
            Self::TimelineFrame => "timeline-frame",
            Self::EditApply => "edit-apply",
            Self::Export => "export",
            Self::AudioLoad => "audio-load",
            Self::Memory => "memory",
            Self::PluginCost => "plugin-cost",
        }
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        match input.trim() {
            "startup" => Ok(Self::Startup),
            "project-io" => Ok(Self::ProjectIo),
            "play-start" => Ok(Self::PlayStart),
            "playback" | "reverse" => Ok(Self::Playback),
            "av-sync" => Ok(Self::AvSync),
            "scrub" => Ok(Self::Scrub),
            "param-latency" => Ok(Self::ParamLatency),
            "timeline-frame" => Ok(Self::TimelineFrame),
            "edit-apply" => Ok(Self::EditApply),
            "export" => Ok(Self::Export),
            "audio-load" => Ok(Self::AudioLoad),
            "memory" => Ok(Self::Memory),
            "plugin-cost" => Ok(Self::PluginCost),
            other => {
                let valid: Vec<&str> = Self::ALL.iter().map(|s| s.as_str()).collect();
                Err(format!(
                    "unknown scenario '{other}'; valid scenarios: {}",
                    valid.join(", ")
                ))
            }
        }
    }
}

impl std::fmt::Display for ScenarioId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Outcome of a target or scenario evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
    Skipped,
}

impl Verdict {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skipped => "skipped",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Direction of comparison against a numeric target threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparison {
    AtMost,
    AtLeast,
    Equals,
}

impl Comparison {
    pub fn check(self, measured: f64, target: f64) -> bool {
        if !measured.is_finite() || !target.is_finite() {
            return false;
        }
        match self {
            Self::AtMost => measured <= target,
            Self::AtLeast => measured >= target,
            Self::Equals => (measured - target).abs() <= 1e-9,
        }
    }

    pub const fn symbol(self) -> &'static str {
        match self {
            Self::AtMost => "<=",
            Self::AtLeast => ">=",
            Self::Equals => "==",
        }
    }
}

/// Hardware and build provenance recorded with every benchmark run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentInfo {
    pub machine: MachineProfile,
    pub hostname: String,
    pub os: String,
    pub cpu: String,
    pub gpu: String,
    pub driver: String,
    pub wgpu_adapter: String,
    pub commit: String,
    pub profile: String,
}

/// One target metric evaluated against the reference machine's threshold.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetEvaluation {
    pub name: String,
    pub description: String,
    pub comparison: Comparison,
    pub target: f64,
    pub unit: String,
    pub measured: Option<f64>,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub required_on_machine: bool,
}

impl TargetEvaluation {
    /// Evaluate a measured value against `target` using `comparison`.
    pub fn measured(
        name: impl Into<String>,
        description: impl Into<String>,
        comparison: Comparison,
        target: f64,
        unit: impl Into<String>,
        measured: f64,
    ) -> Self {
        let name = name.into();
        let description = description.into();
        let unit = unit.into();
        let passed = comparison.check(measured, target);
        let (verdict, reason) = if passed {
            (Verdict::Pass, None)
        } else {
            (
                Verdict::Fail,
                Some(format!(
                    "{name}: measured {measured:.3} {unit}, target {} {target:.3} {unit}",
                    comparison.symbol()
                )),
            )
        };
        Self {
            name,
            description,
            comparison,
            target,
            unit,
            measured: Some(measured),
            verdict,
            reason,
            required_on_machine: true,
        }
    }

    /// Record a target that could not be measured on the current machine.
    pub fn skipped(
        name: impl Into<String>,
        description: impl Into<String>,
        comparison: Comparison,
        target: f64,
        unit: impl Into<String>,
        reason: impl Into<String>,
        required_on_machine: bool,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            comparison,
            target,
            unit: unit.into(),
            measured: None,
            verdict: Verdict::Skipped,
            reason: Some(reason.into()),
            required_on_machine,
        }
    }

    /// Whether this target causes `all` to exit non-zero.
    pub fn fails_gate(&self) -> bool {
        match self.verdict {
            Verdict::Pass => false,
            Verdict::Fail => true,
            Verdict::Skipped => self.required_on_machine,
        }
    }
}

/// Raw sample series with summary quantiles.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SampleSeries {
    pub name: String,
    pub unit: String,
    pub values: Vec<f64>,
    pub min: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
    pub mean: f64,
}

impl SampleSeries {
    pub fn from_values(name: impl Into<String>, unit: impl Into<String>, values: Vec<f64>) -> Self {
        let name = name.into();
        let unit = unit.into();
        if values.is_empty() {
            return Self {
                name,
                unit,
                values,
                min: 0.0,
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                max: 0.0,
                mean: 0.0,
            };
        }
        let mut sorted: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if sorted.is_empty() {
            return Self {
                name,
                unit,
                values,
                min: 0.0,
                p50: 0.0,
                p95: 0.0,
                p99: 0.0,
                max: 0.0,
                mean: 0.0,
            };
        }
        let min = sorted[0];
        let max = sorted[sorted.len() - 1];
        let sum: f64 = sorted.iter().sum();
        let mean = sum / (sorted.len() as f64);
        let p50 = quantile_sorted(&sorted, 0.50);
        let p95 = quantile_sorted(&sorted, 0.95);
        let p99 = quantile_sorted(&sorted, 0.99);
        Self {
            name,
            unit,
            values,
            min,
            p50,
            p95,
            p99,
            max,
            mean,
        }
    }
}

fn quantile_sorted(sorted: &[f64], q: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * q.clamp(0.0, 1.0)).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Counter-backed per-frame rule check recorded in the scenario report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleCheck {
    pub rule: String,
    pub verdict: Verdict,
    pub measured: Option<f64>,
    pub unit: String,
    pub reason: Option<String>,
    pub required_on_machine: bool,
}

impl RuleCheck {
    pub fn fails_gate(&self) -> bool {
        match self.verdict {
            Verdict::Pass => false,
            Verdict::Fail => true,
            Verdict::Skipped => self.required_on_machine,
        }
    }
}

/// Complete JSON report written to `bench/results/<scenario>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioReport {
    pub scenario: String,
    pub machine: MachineProfile,
    pub gpu: String,
    pub driver: String,
    pub wgpu_adapter: String,
    pub commit: String,
    pub profile: String,
    pub environment: EnvironmentInfo,
    pub verdict: Verdict,
    pub reason: Option<String>,
    pub targets: Vec<TargetEvaluation>,
    pub rules: Vec<RuleCheck>,
    pub samples: Vec<SampleSeries>,
    pub notes: Vec<String>,
}

impl ScenarioReport {
    /// Construct a report from an environment, target evaluations, rule checks,
    /// raw sample series, and notes.
    pub fn new(
        scenario: ScenarioId,
        env: EnvironmentInfo,
        targets: Vec<TargetEvaluation>,
        rules: Vec<RuleCheck>,
        samples: Vec<SampleSeries>,
        notes: Vec<String>,
    ) -> Self {
        let (verdict, reason) = compute_overall_verdict(&targets, &rules);
        Self {
            scenario: scenario.as_str().to_string(),
            machine: env.machine,
            gpu: env.gpu.clone(),
            driver: env.driver.clone(),
            wgpu_adapter: env.wgpu_adapter.clone(),
            commit: env.commit.clone(),
            profile: env.profile.clone(),
            environment: env,
            verdict,
            reason,
            targets,
            rules,
            samples,
            notes,
        }
    }

    /// Whether this scenario report causes `aria-bench all` to exit non-zero.
    pub fn contributes_failure(&self) -> bool {
        self.verdict == Verdict::Fail
            || self.targets.iter().any(TargetEvaluation::fails_gate)
            || self.rules.iter().any(RuleCheck::fails_gate)
    }

    /// One-line human summary for terminal output.
    pub fn summary_line(&self) -> String {
        let metrics: Vec<String> = self
            .targets
            .iter()
            .map(|t| match t.measured {
                Some(v) => format!(
                    "{}={:.2}{} ({} {:.2}{}: {})",
                    t.name,
                    v,
                    t.unit,
                    t.comparison.symbol(),
                    t.target,
                    t.unit,
                    t.verdict
                ),
                None => format!("{}=skipped ({})", t.name, t.verdict),
            })
            .collect();
        let detail = if metrics.is_empty() {
            self.reason.clone().unwrap_or_default()
        } else {
            metrics.join(", ")
        };
        match &self.reason {
            Some(r) if self.verdict != Verdict::Pass => {
                format!(
                    "[{}] {:<15} | {} | {}",
                    self.verdict.as_str().to_uppercase(),
                    self.scenario,
                    detail,
                    r
                )
            }
            _ => format!(
                "[{}] {:<15} | {}",
                self.verdict.as_str().to_uppercase(),
                self.scenario,
                detail
            ),
        }
    }
}

/// Compute the aggregate scenario verdict and reason from individual targets and rules.
pub fn compute_overall_verdict(
    targets: &[TargetEvaluation],
    rules: &[RuleCheck],
) -> (Verdict, Option<String>) {
    let mut failure_reasons = Vec::new();
    let mut skip_reasons = Vec::new();
    let mut any_pass = false;

    for t in targets {
        match t.verdict {
            Verdict::Pass => any_pass = true,
            Verdict::Fail => {
                failure_reasons.push(t.reason.clone().unwrap_or_else(|| {
                    format!(
                        "{} missed target ({} {} {})",
                        t.name,
                        t.comparison.symbol(),
                        t.target,
                        t.unit
                    )
                }));
            }
            Verdict::Skipped => {
                let msg = t
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("{} skipped", t.name));
                if t.required_on_machine {
                    failure_reasons.push(msg);
                } else {
                    skip_reasons.push(msg);
                }
            }
        }
    }

    for r in rules {
        match r.verdict {
            Verdict::Pass => any_pass = true,
            Verdict::Fail => {
                failure_reasons.push(
                    r.reason
                        .clone()
                        .unwrap_or_else(|| format!("rule failed: {}", r.rule)),
                );
            }
            Verdict::Skipped => {
                let msg = r
                    .reason
                    .clone()
                    .unwrap_or_else(|| format!("rule skipped: {}", r.rule));
                if r.required_on_machine {
                    failure_reasons.push(msg);
                } else {
                    skip_reasons.push(msg);
                }
            }
        }
    }

    if !failure_reasons.is_empty() {
        (Verdict::Fail, Some(failure_reasons.join("; ")))
    } else if !skip_reasons.is_empty() && !any_pass {
        (Verdict::Skipped, Some(skip_reasons.join("; ")))
    } else if !skip_reasons.is_empty() {
        (Verdict::Skipped, Some(skip_reasons.join("; ")))
    } else {
        (Verdict::Pass, None)
    }
}

/// Compute the process exit code for `aria-bench all`.
/// Returns `0` only when all 13 scenarios are present and none has a failing or
/// required-skipped target.
pub fn all_exit_code(reports: &[ScenarioReport]) -> i32 {
    if reports.len() < ScenarioId::ALL.len() {
        return 1;
    }
    for scenario in ScenarioId::ALL {
        let Some(report) = reports.iter().find(|r| r.scenario == scenario.as_str()) else {
            return 1;
        };
        if report.contributes_failure() {
            return 1;
        }
    }
    0
}

/// Write a scenario report as pretty-printed JSON to `<results_dir>/<scenario>.json`.
pub fn write_report(results_dir: &Path, report: &ScenarioReport) -> Result<PathBuf, String> {
    std::fs::create_dir_all(results_dir).map_err(|err| {
        format!(
            "failed to create results directory {}: {err}",
            results_dir.display()
        )
    })?;
    let path = results_dir.join(format!("{}.json", report.scenario));
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| format!("failed to serialize report {}: {err}", report.scenario))?;
    std::fs::write(&path, format!("{json}\n"))
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(path)
}

/// Read a scenario report from disk.
pub fn read_report(path: &Path) -> Result<ScenarioReport, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str(&raw).map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

/// Mount all built-in plugins in the order `aria-desktop` mounts them.
pub fn mount_builtin_plugins(ctx: &PluginContext, data_dir: &Path) -> Result<(), String> {
    let _ = aria_ui::plugins::mount_all(ctx, data_dir);
    Ok(())
}

/// Resolve the workspace root directory from `CARGO_MANIFEST_DIR` or current working directory.
pub fn default_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(manifest_dir)
}

/// Default `bench/fixtures` directory under the workspace root.
pub fn default_fixtures_dir() -> PathBuf {
    default_workspace_root().join("bench/fixtures")
}

/// Default `bench/results` directory under the workspace root.
pub fn default_results_dir() -> PathBuf {
    default_workspace_root().join("bench/results")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_env(machine: MachineProfile) -> EnvironmentInfo {
        EnvironmentInfo {
            machine,
            hostname: "santhserver".to_string(),
            os: "Linux 6.8 x86_64".to_string(),
            cpu: "AMD EPYC".to_string(),
            gpu: "NVIDIA GeForce RTX 3080 Ti".to_string(),
            driver: "NVIDIA 550.54.14".to_string(),
            wgpu_adapter: "llvmpipe (Vulkan)".to_string(),
            commit: "0123456789abcdef".to_string(),
            profile: "release".to_string(),
        }
    }

    /// Every scenario named in `docs/spec/performance.md` parses from its CLI
    /// string and formats back to the identical name.
    #[test]
    fn all_thirteen_scenarios_round_trip_by_name() {
        assert_eq!(ScenarioId::ALL.len(), 13);
        for scenario in ScenarioId::ALL {
            let parsed = ScenarioId::parse(scenario.as_str()).unwrap_or_else(|err| panic!("{err}"));
            assert_eq!(parsed, scenario);
            assert_eq!(scenario.to_string(), scenario.as_str());
        }
        assert_eq!(ScenarioId::parse("reverse"), Ok(ScenarioId::Playback));
        assert!(ScenarioId::parse("nonexistent-scenario").is_err());
    }

    /// JSON report serialization preserves every field across round-trip.
    #[test]
    fn scenario_report_json_round_trip_preserves_all_fields() {
        let env = sample_env(MachineProfile::Server);
        let targets = vec![
            TargetEvaluation::measured(
                "open_interactive_ms",
                "project-10k open to interactive in 500 ms",
                Comparison::AtMost,
                500.0,
                "ms",
                142.5,
            ),
            TargetEvaluation::measured(
                "save_ms",
                "project-10k save in 200 ms",
                Comparison::AtMost,
                200.0,
                "ms",
                88.25,
            ),
        ];
        let samples = vec![SampleSeries::from_values(
            "open_interactive_ms",
            "ms",
            vec![140.0, 142.5, 145.0],
        )];
        let report = ScenarioReport::new(
            ScenarioId::ProjectIo,
            env,
            targets,
            Vec::new(),
            samples,
            vec!["verified 10,000 clips".to_string()],
        );
        assert_eq!(report.verdict, Verdict::Pass);
        assert!(!report.contributes_failure());

        let json = serde_json::to_string_pretty(&report).unwrap_or_else(|err| panic!("{err}"));
        let decoded: ScenarioReport =
            serde_json::from_str(&json).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(decoded, report);
    }

    /// Target evaluation distinguishes boundary pass, boundary fail, and
    /// required vs optional skipped targets.
    #[test]
    fn target_evaluation_and_all_exit_code_semantics() {
        let env = sample_env(MachineProfile::Server);

        let mut passing_reports: Vec<ScenarioReport> = ScenarioId::ALL
            .iter()
            .map(|&id| {
                ScenarioReport::new(
                    id,
                    env.clone(),
                    vec![TargetEvaluation::measured(
                        "metric",
                        "within budget",
                        Comparison::AtMost,
                        10.0,
                        "ms",
                        10.0,
                    )],
                    Vec::new(),
                    Vec::new(),
                    Vec::new(),
                )
            })
            .collect();
        assert_eq!(all_exit_code(&passing_reports), 0);

        // An optional skip on a machine without hardware encoder does not fail `all`.
        passing_reports[9] = ScenarioReport::new(
            ScenarioId::Export,
            env.clone(),
            vec![TargetEvaluation::skipped(
                "hw_export_realtime_x",
                "hardware encoder 4x realtime",
                Comparison::AtLeast,
                4.0,
                "x",
                "no hardware encoder on host",
                false,
            )],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(passing_reports[9].verdict, Verdict::Skipped);
        assert!(!passing_reports[9].contributes_failure());
        assert_eq!(all_exit_code(&passing_reports), 0);

        // A required skip fails `all`.
        passing_reports[9] = ScenarioReport::new(
            ScenarioId::Export,
            env.clone(),
            vec![TargetEvaluation::skipped(
                "hw_export_realtime_x",
                "hardware encoder 4x realtime",
                Comparison::AtLeast,
                4.0,
                "x",
                "required hardware encoder missing",
                true,
            )],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(passing_reports[9].verdict, Verdict::Fail);
        assert!(passing_reports[9].contributes_failure());
        assert_eq!(all_exit_code(&passing_reports), 1);

        // A missed target fails `all`.
        passing_reports[9] = ScenarioReport::new(
            ScenarioId::Export,
            env,
            vec![TargetEvaluation::measured(
                "hw_export_realtime_x",
                "hardware encoder 4x realtime",
                Comparison::AtLeast,
                4.0,
                "x",
                2.5,
            )],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(passing_reports[9].verdict, Verdict::Fail);
        assert_eq!(all_exit_code(&passing_reports), 1);
    }

    /// Allocation tracker counts allocations on the calling thread and resets cleanly.
    #[test]
    fn allocation_tracker_counts_heap_allocations_on_current_thread() {
        let (sum, stats) = alloc_tracker::measure_thread_allocs(|| {
            let mut v = Vec::<u64>::with_capacity(64);
            v.extend(0..64);
            v.iter().sum::<u64>()
        });
        assert_eq!(sum, (0..64u64).sum::<u64>());
        assert!(stats.allocations >= 1);
        assert!(stats.bytes >= 64 * 8);

        let (val, zero_stats) = alloc_tracker::measure_thread_allocs(|| 42u64 + 58u64);
        assert_eq!(val, 100);
        assert_eq!(zero_stats.allocations, 0);
        assert_eq!(zero_stats.bytes, 0);
    }

    /// Fixture identifiers round-trip through CLI names and support the
    /// `long-gop-10m` alias from `docs/spec/performance.md`.
    #[test]
    fn fixture_ids_and_rule_checks_round_trip() {
        assert_eq!(fixtures::FixtureId::ALL.len(), 6);
        for fixture in fixtures::FixtureId::ALL {
            let parsed = fixtures::FixtureId::parse(fixture.as_str())
                .unwrap_or_else(|| panic!("failed to parse {}", fixture.as_str()));
            assert_eq!(parsed, fixture);
            assert!(!fixture.file_name().is_empty());
        }
        assert_eq!(
            fixtures::FixtureId::parse("long-gop-10m"),
            Some(fixtures::FixtureId::LongGopScrub)
        );
        assert!(fixtures::FixtureId::parse("unknown-fixture").is_none());

        let env = sample_env(MachineProfile::Desktop);
        let report_with_unchecked_rule = ScenarioReport::new(
            ScenarioId::Playback,
            env.clone(),
            vec![TargetEvaluation::measured(
                "dropped_frames",
                "0 dropped frames",
                Comparison::AtMost,
                0.0,
                "frames",
                0.0,
            )],
            vec![RuleCheck {
                rule: "No glFinish on the render thread".to_string(),
                verdict: Verdict::Skipped,
                measured: None,
                unit: "count".to_string(),
                reason: Some("missing counter".to_string()),
                required_on_machine: false,
            }],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(report_with_unchecked_rule.verdict, Verdict::Skipped);
        assert!(!report_with_unchecked_rule.contributes_failure());

        let report_with_failed_rule = ScenarioReport::new(
            ScenarioId::Playback,
            env,
            vec![TargetEvaluation::measured(
                "dropped_frames",
                "0 dropped frames",
                Comparison::AtMost,
                0.0,
                "frames",
                0.0,
            )],
            vec![RuleCheck {
                rule: "No heap allocation on the render thread after 1 s".to_string(),
                verdict: Verdict::Fail,
                measured: Some(4.0),
                unit: "allocs/frame".to_string(),
                reason: Some("observed 4 allocations".to_string()),
                required_on_machine: true,
            }],
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(report_with_failed_rule.verdict, Verdict::Fail);
        assert!(report_with_failed_rule.contributes_failure());
    }
}
