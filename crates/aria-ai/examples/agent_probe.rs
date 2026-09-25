//! Hand-run end-to-end agent probe: a real prompt, the configured live
//! model, and a real engine editing a fixture project. Not a test.
//!
//! ```bash
//! cargo run -p aria-ai --example agent_probe -- "cut the first 3 seconds of the selected clip"
//! ```

use std::sync::atomic::AtomicBool;

use aria_ai::agent::{AgentConfig, AgentEvent, Applied, EngineBridge, run_prompt};
use aria_ai::config::provider_from_ai;
use aria_ai::{
    EditorContext, Lowered, ProjectSummary, SummaryRequest, WireCommand, summarize_for, validate,
};
use aria_commands::EditOutcome;
use aria_engine::{ApplyOutcome, Engine, EngineConfig};
use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

struct EngineHost {
    engine: Engine,
}

impl EngineBridge for EngineHost {
    fn summary(&mut self, request: SummaryRequest) -> ProjectSummary {
        summarize_for(self.engine.project(), request)
    }
    fn apply(&mut self, command: &WireCommand) -> Result<Applied, String> {
        match validate(command, self.engine.project()).map_err(|r| r.message)? {
            Lowered::Edit(lowered) => match self.engine.apply(lowered) {
                Ok(ApplyOutcome::Edited(outcome)) => Ok(Applied::Edited(outcome)),
                Ok(other) => Err(format!("unexpected engine outcome: {other:?}")),
                Err(e) => Err(e.to_string()),
            },
            // No renderer and no job board here: saying so beats reporting a
            // render that nothing is doing.
            Lowered::Queued(work) => Err(format!("this probe cannot carry out {work:?}")),
        }
    }
    fn rehearse(&mut self, command: &WireCommand) -> Result<Option<EditOutcome>, String> {
        match validate(command, self.engine.project()).map_err(|r| r.message)? {
            Lowered::Edit(lowered) => match self.engine.apply(lowered) {
                Ok(ApplyOutcome::Edited(outcome)) => Ok(Some(outcome)),
                Ok(other) => Err(format!("unexpected engine outcome: {other:?}")),
                Err(e) => Err(e.to_string()),
            },
            // Validated, and that is all a rehearsal of it can be: the work
            // needs a renderer this probe does not have.
            Lowered::Queued(_) => Ok(None),
        }
    }
    fn begin_group(&mut self) {
        self.engine.begin_group();
    }
    fn end_group(&mut self) {
        self.engine.commit_group();
    }
    fn rollback_group(&mut self) {
        self.engine.rollback_group();
    }
}

fn main() {
    const R24: Rational = Rational::FPS_24;
    let prompt = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "cut the first 3 seconds of the selected clip".to_string());

    let path = aria_settings::default_config_path();
    let ai = match aria_settings::load(&path) {
        Ok(settings) => settings.ai,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let provider = provider_from_ai(&ai).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });

    // Fixture: one 10s clip (of a 60s source) on V1, currently selected.
    let mut project = Project::new("probe", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/probe.mp4",
        1920,
        1080,
        R24,
        60 * 24,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();

    let engine = Engine::with_project(EngineConfig { undo_limit: 64 }, project).unwrap();
    let mut host = EngineHost { engine };

    let context = EditorContext {
        selected_clips: vec![clip.raw()],
        playhead_seconds: 0.0,
        ..Default::default()
    };

    println!("model: {}  prompt: {prompt:?}\n", ai.model);
    let cancel = AtomicBool::new(false);
    let outcome = run_prompt(
        &provider,
        &mut host,
        &context,
        &aria_ai::AgentExtensions::default(),
        &[],
        &prompt,
        &AgentConfig::default(),
        &cancel,
        &mut |event| match event {
            AgentEvent::TextDelta(t) => {
                print!("{t}");
                use std::io::Write;
                std::io::stdout().flush().ok();
            }
            AgentEvent::ReasoningDelta(t) => eprintln!("  reasoning: {t}"),
            AgentEvent::Action(a) => println!("  ⚙ {}", a.description),
            AgentEvent::HostAction { name, summary } => println!("  ⚙ {name}: {summary}"),
            AgentEvent::Image(image) => println!("  ◫ {}", image.label),
        },
    );

    println!("\nstatus: {:?}", outcome.status);
    let placed = host.engine.project().clip(clip).unwrap();
    println!(
        "clip {} now: timeline {:?} ticks, source start {:?}",
        clip.raw(),
        (placed.timeline.start.value, placed.timeline.duration.value),
        placed.source_range().map(|s| s.start.value),
    );
}
