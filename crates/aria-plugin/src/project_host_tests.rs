//! The project host's contract: a group or a transaction lands as one undo
//! step or not at all, `CommandApplied` follows only what can no longer roll
//! back, a `CommandProposed` handler cannot start an edit, and an edit from
//! another thread is refused within a bounded wait instead of blocking.
//!
//! Not caught here: a shipped handler that locks the engine; the editor's
//! mounted-plugin suite proposes through every shipped handler for that.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Duration;

use aria_commands::{Command, EditCommand};
use aria_engine::{Engine, EngineConfig};
use aria_models::{Project, TrackId, TrackKind};
use parking_lot::Mutex;

use super::{CommandApplied, Origin, ProjectHost};
use crate::Registration;
use crate::context::PluginContext;
use crate::events::EventBus;
use crate::hooks::CommandProposed;

/// Long enough that only a blocked edit misses it.
const BOUND: Duration = Duration::from_secs(10);

const REFUSAL: &str = "lanes named refused are not allowed";

struct Fixture {
    host: Arc<ProjectHost>,
    ctx: PluginContext,
}

/// A host over an empty project, with one handler that refuses any lane
/// named "refused".
fn fixture() -> (Fixture, Registration) {
    let ctx = PluginContext::new();
    let engine = Engine::new(EngineConfig::default()).expect("engine");
    let host = Arc::new(ProjectHost::new(Arc::new(Mutex::new(engine)), &ctx));
    let policy = ctx.on_waterfall::<CommandProposed, _>(|proposed| {
        if lane_name(&proposed.command).as_deref() == Some("refused") {
            proposed.refuse(REFUSAL);
        }
    });
    (Fixture { host, ctx }, policy)
}

fn add(name: &str) -> Command {
    Command::Edit(EditCommand::AddTrack {
        kind: TrackKind::Video,
        name: name.to_string(),
        index: None,
        pinned: true,
    })
}

/// Fails in the engine: the lane does not exist.
fn missing_lane() -> Command {
    Command::Edit(EditCommand::SetTrackLocked {
        track: TrackId::from_raw(999),
        locked: true,
    })
}

fn lane_name(command: &Command) -> Option<String> {
    match command {
        Command::Edit(EditCommand::AddTrack { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

/// Lane names, sorted: stack order is not part of these contracts.
fn lane_names(project: &Project) -> Vec<String> {
    let mut names: Vec<String> = project
        .timeline()
        .tracks_ordered()
        .map(|track| track.name.clone())
        .collect();
    names.sort();
    names
}

fn lanes(host: &ProjectHost) -> Vec<String> {
    lane_names(&host.snapshot())
}

fn history(host: &ProjectHost) -> Vec<(String, bool)> {
    host.engine()
        .lock()
        .history_entries()
        .iter()
        .map(|entry| (entry.label.to_string(), entry.applied))
        .collect()
}

/// Nothing holds the project and no group is open, in the host or the engine.
fn assert_idle(host: &ProjectHost) {
    let idle = {
        let sequence = host.sequence.lock();
        sequence.owner.is_none()
            && sequence.holds == 0
            && sequence.groups == 0
            && sequence.announced.is_empty()
    };
    assert!(idle, "the host must release the project after every edit");
    assert!(
        !host.engine().lock().in_group(),
        "the engine must have no group left open"
    );
}

/// Every `CommandApplied` a handler received, as lane or label, origin, and
/// revision.
struct Announcements {
    entries: Arc<Mutex<Vec<(String, Origin, u64)>>>,
    bus: Arc<EventBus>,
}

impl Announcements {
    /// What was delivered, once everything posted so far has been.
    fn delivered(&self) -> Vec<(String, Origin, u64)> {
        assert!(
            self.bus.flush(BOUND),
            "delivery must finish within the bound"
        );
        self.entries.lock().clone()
    }
}

fn record_applied(ctx: &PluginContext) -> (Announcements, Registration) {
    let entries: Arc<Mutex<Vec<(String, Origin, u64)>>> = Arc::default();
    let sink = Arc::clone(&entries);
    let registration = ctx.on_event::<CommandApplied, _>(move |applied| {
        let name = lane_name(&applied.command).unwrap_or_else(|| applied.command.label().into());
        sink.lock()
            .push((name, applied.origin.clone(), applied.revision));
    });
    let log = Announcements {
        entries,
        bus: Arc::clone(ctx.event_bus()),
    };
    (log, registration)
}

fn announced(log: &Announcements) -> Vec<(String, Origin)> {
    log.delivered()
        .into_iter()
        .map(|(name, origin, _)| (name, origin))
        .collect()
}

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

#[path = "project_host_tests_contention.rs"]
mod contention;
#[path = "project_host_tests_events.rs"]
mod events;
#[path = "project_host_tests_transactions.rs"]
mod transactions;
