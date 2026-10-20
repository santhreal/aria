//! The wire form of the events a native or WASM plugin subscribes to by kind.
//!
//! An external plugin names a kind in `subscribe` during `mount` and receives
//! each event as a call to its `event` export: the kind, and the event as a JSON
//! object. [`subscribe`] is the one translation for both ABIs. The native and
//! WASM bridges supply only the call into their plugin, so a plugin receives the
//! same kind and payload, and its reply is read the same way, under either.
//!
//! The kinds, their payloads, and their replies are listed in
//! `docs/plugins/events.md`, and a test holds that table to [`WireKind::ALL`].

use std::borrow::Cow;
use std::path::Path;

use aria_commands::Command;
use serde_json::{Value, json};

use crate::{
    CommandApplied, CommandProposed, DropAction, Dropped, Event, ExportFinished, ExportStarted,
    InOutChanged, JobFinished, JobOutcome, JobProgress, MediaImported, MediaOffline, MediaRelinked,
    PlaybackStarted, PlaybackStopped, PlayheadMoved, PluginContext, ProjectClosed, ProjectOpened,
    ProjectSaved, RenderFinished, RenderKind, RenderResult, ResolveDropped, SelectionChanged,
};

/// Declares [`WireKind`] from one list, so [`WireKind::ALL`], the wire names,
/// and the type names cannot disagree.
macro_rules! wire_kinds {
    ($($(#[$meta:meta])* $kind:ident = $name:literal,)+) => {
        /// An event a native or WASM plugin can subscribe to.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum WireKind {
            $($(#[$meta])* $kind,)+
        }

        impl WireKind {
            /// Every kind, in the order the Events chapter lists them.
            pub const ALL: &'static [WireKind] = &[$(WireKind::$kind,)+];

            /// The kind as a plugin subscribes to it and receives it in `event`.
            pub const fn name(self) -> &'static str {
                match self {
                    $(WireKind::$kind => $name,)+
                }
            }

            /// The name of the host event type, which `subscribe` also accepts.
            pub const fn event_type(self) -> &'static str {
                match self {
                    $(WireKind::$kind => stringify!($kind),)+
                }
            }
        }
    };
}

wire_kinds! {
    /// The waterfall run before a command reaches the engine.
    CommandProposed = "command-proposed",
    /// The serial event that asks which plugin claims a dropped path or URL.
    ResolveDropped = "resolve-dropped",
    CommandApplied = "command-applied",
    ProjectOpened = "project-opened",
    ProjectClosed = "project-closed",
    ProjectSaved = "project-saved",
    SelectionChanged = "selection-changed",
    PlayheadMoved = "playhead-moved",
    InOutChanged = "in-out-changed",
    PlaybackStarted = "playback-started",
    PlaybackStopped = "playback-stopped",
    MediaImported = "media-imported",
    MediaRelinked = "media-relinked",
    MediaOffline = "media-offline",
    RenderFinished = "render-finished",
    ExportStarted = "export-started",
    ExportFinished = "export-finished",
    JobProgress = "job-progress",
    JobFinished = "job-finished",
}

impl WireKind {
    /// The kind `name` spells: its wire name, its event type, or
    /// `resolve-dropped-path`, an alias of [`WireKind::ResolveDropped`].
    pub fn from_name(name: &str) -> Option<Self> {
        if matches!(name, "resolve-dropped-path" | "ResolveDroppedPath") {
            return Some(Self::ResolveDropped);
        }
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.name() == name || kind.event_type() == name)
    }
}

/// A parsed `subscribe` argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Subscription {
    pub kind: WireKind,
    /// The serial handler's priority, set by a `:<n>` suffix; 0 without one.
    /// Higher runs first.
    pub priority: i32,
}

impl Subscription {
    /// Parse `kind` or `kind:<priority>`. Only `resolve-dropped` takes a
    /// priority, since only a serial event orders its handlers by one.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let (name, priority) = match raw.split_once(':') {
            Some((name, priority)) => (name, Some(priority)),
            None => (raw, None),
        };
        let kind = WireKind::from_name(name).ok_or_else(|| {
            let known: Vec<&str> = WireKind::ALL.iter().map(|kind| kind.name()).collect();
            format!(
                "unknown event kind {name:?}; subscribe to one of {}",
                known.join(", ")
            )
        })?;
        let priority = match priority {
            None => 0,
            Some(_) if kind != WireKind::ResolveDropped => {
                return Err(format!(
                    "event kind {name:?} takes no priority; subscribe to {:?}",
                    kind.name()
                ));
            }
            Some(text) => text.trim().parse::<i32>().map_err(|_| {
                format!("event priority {text:?} in {raw:?} is not a 32-bit integer")
            })?,
        };
        Ok(Self { kind, priority })
    }
}

/// Subscribe a plugin, during its mount, to the kind `raw` names.
///
/// `deliver(ctx, kind, payload)` calls the plugin's `event` export on `ctx`, a
/// clone of the plugin's scoped context, and returns the plugin's reply:
/// `None` when it returned none or failed. `kind` is always the wire name,
/// whichever spelling `raw` used.
pub fn subscribe<F>(ctx: &PluginContext, raw: &str, deliver: F) -> Result<(), String>
where
    F: Fn(&PluginContext, &str, &str) -> Option<String> + Send + Sync + 'static,
{
    let Subscription { kind, priority } = Subscription::parse(raw)?;
    let name = kind.name();
    let scoped = ctx.clone();
    match kind {
        WireKind::CommandProposed => {
            ctx.on_waterfall(move |proposed: &mut CommandProposed| {
                let Some(payload) = encoded(name, proposed_payload(proposed)) else {
                    return;
                };
                if let Some(reply) = deliver(&scoped, name, &payload) {
                    apply_proposed_reply(proposed, &reply);
                }
            });
        }
        WireKind::ResolveDropped => {
            ctx.on_serial_with_priority(priority, move |dropped: &ResolveDropped| {
                let payload = dropped_payload(&dropped.item).to_string();
                deliver(&scoped, name, &payload).and_then(|reply| parse_drop_reply(&reply))
            });
        }
        WireKind::CommandApplied => notify(ctx, scoped, name, deliver, |ev: &CommandApplied| {
            let mut payload = json!({
                "outcome": ev.outcome,
                "revision": ev.revision,
                "origin": ev.origin.to_string(),
            });
            payload["command"] = serde_json::to_value(&ev.command)?;
            Ok(payload)
        }),
        WireKind::ProjectOpened => notify(ctx, scoped, name, deliver, |ev: &ProjectOpened| {
            Ok(json!({ "path": ev.path.as_deref().map(text) }))
        }),
        WireKind::ProjectClosed => notify(ctx, scoped, name, deliver, |ev: &ProjectClosed| {
            Ok(json!({ "path": ev.path.as_deref().map(text) }))
        }),
        WireKind::ProjectSaved => notify(ctx, scoped, name, deliver, |ev: &ProjectSaved| {
            Ok(json!({ "path": text(&ev.path) }))
        }),
        WireKind::SelectionChanged => {
            notify(ctx, scoped, name, deliver, |ev: &SelectionChanged| {
                Ok(json!({ "clips": ev.clips }))
            });
        }
        WireKind::PlayheadMoved => notify(ctx, scoped, name, deliver, |ev: &PlayheadMoved| {
            Ok(json!({ "at": ev.at }))
        }),
        WireKind::InOutChanged => notify(ctx, scoped, name, deliver, |ev: &InOutChanged| {
            Ok(json!({ "mark_in": ev.mark_in, "mark_out": ev.mark_out }))
        }),
        WireKind::PlaybackStarted => notify(ctx, scoped, name, deliver, |ev: &PlaybackStarted| {
            Ok(json!({ "at": ev.at, "speed": ev.speed }))
        }),
        WireKind::PlaybackStopped => notify(ctx, scoped, name, deliver, |ev: &PlaybackStopped| {
            Ok(json!({ "at": ev.at }))
        }),
        WireKind::MediaImported => notify(ctx, scoped, name, deliver, |ev: &MediaImported| {
            Ok(json!({ "media": ev.media, "path": text(&ev.path) }))
        }),
        WireKind::MediaRelinked => notify(ctx, scoped, name, deliver, |ev: &MediaRelinked| {
            Ok(json!({ "media": ev.media, "path": text(&ev.path) }))
        }),
        WireKind::MediaOffline => notify(ctx, scoped, name, deliver, |ev: &MediaOffline| {
            Ok(json!({ "media": ev.media, "path": text(&ev.path) }))
        }),
        WireKind::RenderFinished => notify(ctx, scoped, name, deliver, |ev: &RenderFinished| {
            let kind = match ev.kind {
                RenderKind::Clip(clip) => json!({ "clip": clip }),
                RenderKind::ImageSequence => json!("image-sequence"),
            };
            Ok(json!({
                "kind": kind,
                "path": text(&ev.path),
                "result": result_payload(&ev.result),
            }))
        }),
        WireKind::ExportStarted => notify(ctx, scoped, name, deliver, |ev: &ExportStarted| {
            Ok(json!({ "path": text(&ev.path) }))
        }),
        WireKind::ExportFinished => notify(ctx, scoped, name, deliver, |ev: &ExportFinished| {
            Ok(json!({ "path": text(&ev.path), "result": result_payload(&ev.result) }))
        }),
        WireKind::JobProgress => notify(ctx, scoped, name, deliver, |ev: &JobProgress| {
            Ok(json!({
                "job": ev.job,
                "label": ev.label,
                "progress": ev.progress,
                "detail": ev.detail,
            }))
        }),
        WireKind::JobFinished => notify(ctx, scoped, name, deliver, |ev: &JobFinished| {
            let outcome = match ev.outcome {
                JobOutcome::Done => "done",
                JobOutcome::Failed => "failed",
                JobOutcome::Cancelled => "cancelled",
            };
            Ok(json!({
                "job": ev.job,
                "label": ev.label,
                "outcome": outcome,
                "detail": ev.detail,
            }))
        }),
    }
    Ok(())
}

/// Register a notification handler that sends `payload(event)` to the plugin
/// and discards its reply.
fn notify<E, F>(
    ctx: &PluginContext,
    scoped: PluginContext,
    name: &'static str,
    deliver: F,
    payload: fn(&E) -> Result<Value, serde_json::Error>,
) where
    E: Event,
    F: Fn(&PluginContext, &str, &str) -> Option<String> + Send + Sync + 'static,
{
    ctx.on_event(move |event: &E| {
        if let Some(payload) = encoded(name, payload(event)) {
            let _ = deliver(&scoped, name, &payload);
        }
    });
}

/// The payload as JSON text, or `None` with a warning when a field does not
/// serialize: a command naming a path that is not UTF-8.
fn encoded(name: &str, payload: Result<Value, serde_json::Error>) -> Option<String> {
    match payload {
        Ok(payload) => Some(payload.to_string()),
        Err(err) => {
            tracing::warn!(kind = name, %err, "event payload has no JSON form; not delivered");
            None
        }
    }
}

/// A path as JSON text. A path that is not UTF-8 has each invalid sequence
/// replaced by U+FFFD, so the event still reaches the plugin.
fn text(path: &Path) -> Cow<'_, str> {
    path.to_string_lossy()
}

fn proposed_payload(proposed: &CommandProposed) -> Result<Value, serde_json::Error> {
    let (tracks, clips) = match &proposed.command {
        Command::Edit(edit) => {
            let targets = edit.targets();
            (targets.tracks, targets.clips)
        }
        Command::Project(_) => (Vec::new(), Vec::new()),
    };
    let mut payload = json!({
        "origin": proposed.origin.to_string(),
        "target_tracks": tracks,
        "target_clips": clips,
    });
    payload["command"] = serde_json::to_value(&proposed.command)?;
    Ok(payload)
}

fn dropped_payload(item: &Dropped) -> Value {
    match item {
        Dropped::Path(path) => json!({
            "kind": "path",
            "path": text(path),
            "extension": path.extension().map_or(Cow::Borrowed(""), |ext| ext.to_string_lossy()),
        }),
        Dropped::Url(url) => json!({ "kind": "url", "url": url }),
    }
}

fn result_payload(result: &RenderResult) -> Value {
    match result {
        RenderResult::Written => json!({ "status": "written" }),
        RenderResult::Failed(reason) => json!({ "status": "failed", "reason": reason }),
        RenderResult::Cancelled => json!({ "status": "cancelled" }),
    }
}

/// Apply a plugin's reply to a proposed command: `{"veto": reason}` or
/// `{"refuse": reason}` refuses it, and `{"rewrite": command}` or a bare
/// command replaces it. Any other reply leaves the command as proposed.
fn apply_proposed_reply(proposed: &mut CommandProposed, reply: &str) {
    let reply = reply.trim();
    if reply.is_empty() {
        return;
    }
    let Ok(mut value) = serde_json::from_str::<Value>(reply) else {
        return;
    };
    if let Some(reason) = value
        .get("veto")
        .or_else(|| value.get("refuse"))
        .and_then(Value::as_str)
    {
        proposed.refuse(reason.to_owned());
        return;
    }
    let replacement = match value.get_mut("rewrite").map(Value::take) {
        Some(rewrite) => rewrite,
        None => value,
    };
    if let Ok(command) = serde_json::from_value::<Command>(replacement) {
        proposed.command = command;
    }
}

/// The drop action a plugin's reply claims: a JSON string, a bare word, or
/// `{"claim_drop": action}`, in its wire or type spelling.
fn parse_drop_reply(reply: &str) -> Option<DropAction> {
    let reply = reply.trim();
    if reply.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(reply) {
        Ok(Value::String(action)) => drop_action(&action),
        Ok(value) => value
            .get("claim_drop")
            .and_then(Value::as_str)
            .and_then(drop_action),
        Err(_) => drop_action(reply),
    }
}

fn drop_action(token: &str) -> Option<DropAction> {
    Some(match token {
        "open-project" | "OpenProject" => DropAction::OpenProject,
        "import-media" | "ImportMedia" => DropAction::ImportMedia,
        "apply-lut" | "ApplyLut" => DropAction::ApplyLut,
        "install-skill" | "InstallSkill" => DropAction::InstallSkill,
        "import-timeline" | "ImportTimeline" => DropAction::ImportTimeline,
        "handled" | "Handled" => DropAction::Handled,
        _ => return None,
    })
}
