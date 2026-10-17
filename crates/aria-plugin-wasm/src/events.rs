//! Event subscription bridging between `PluginContext` and WASM guest `event` exports.

use std::sync::Arc;

use aria_plugin::{
    CommandApplied, CommandProposed, DropAction, Dropped, Event, ExportFinished, ExportStarted,
    InOutChanged, JobFinished, JobOutcome, JobProgress, MediaImported, MediaOffline, MediaRelinked,
    PlaybackStarted, PlaybackStopped, PlayheadMoved, PluginContext, ProjectClosed, ProjectOpened,
    ProjectSaved, RenderFinished, RenderKind, RenderResult, ResolveDropped, SelectionChanged,
};
use serde_json::{Value, json};

use crate::host::WasmPluginShared;

/// Private sentinel event type used to invoke guest `unmount` when the plugin's registrations drop.
pub(crate) struct UnmountSentinel;

impl Event for UnmountSentinel {}

/// Subscribe the WASM plugin to `event_kind` on `ctx` during `mount`.
pub(crate) fn subscribe_event(
    ctx: &PluginContext,
    shared: &Arc<WasmPluginShared>,
    event_kind: &str,
) -> Result<(), String> {
    match event_kind {
        "command-applied" | "CommandApplied" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &CommandApplied| {
                let payload = json!({
                    "command": ev.command,
                    "outcome": ev.outcome,
                    "revision": ev.revision,
                    "origin": ev.origin.to_string(),
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "command-applied", &payload);
            });
            Ok(())
        }
        "command-proposed" | "CommandProposed" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_waterfall(move |ev: &mut CommandProposed| {
                let payload = json!({
                    "command": ev.command,
                    "origin": ev.origin.to_string(),
                })
                .to_string();
                if let Ok(Some(response_json)) =
                    shared.call_event(&ctx_clone, "command-proposed", &payload)
                {
                    apply_command_proposed_response(ev, &response_json);
                }
            });
            Ok(())
        }
        kind if parse_resolve_dropped_subscription(kind).is_some() => {
            let (dispatch_kind, priority) =
                parse_resolve_dropped_subscription(event_kind).expect("checked Some");
            let dispatch_kind = dispatch_kind.to_owned();
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_serial_with_priority(priority, move |ev: &ResolveDropped| {
                let payload = resolve_dropped_payload(ev);
                match shared.call_event(&ctx_clone, &dispatch_kind, &payload) {
                    Ok(Some(response_json)) => parse_drop_action(&response_json),
                    _ => None,
                }
            });
            Ok(())
        }
        "project-opened" | "ProjectOpened" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &ProjectOpened| {
                let payload = json!({ "path": ev.path }).to_string();
                let _ = shared.call_event(&ctx_clone, "project-opened", &payload);
            });
            Ok(())
        }
        "project-saved" | "ProjectSaved" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &ProjectSaved| {
                let payload = json!({ "path": ev.path }).to_string();
                let _ = shared.call_event(&ctx_clone, "project-saved", &payload);
            });
            Ok(())
        }
        "project-closed" | "ProjectClosed" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |_ev: &ProjectClosed| {
                let _ = shared.call_event(&ctx_clone, "project-closed", "{}");
            });
            Ok(())
        }
        "selection-changed" | "SelectionChanged" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &SelectionChanged| {
                let payload = json!({ "clips": ev.clips }).to_string();
                let _ = shared.call_event(&ctx_clone, "selection-changed", &payload);
            });
            Ok(())
        }
        "playhead-moved" | "PlayheadMoved" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &PlayheadMoved| {
                let payload = json!({ "at": ev.at }).to_string();
                let _ = shared.call_event(&ctx_clone, "playhead-moved", &payload);
            });
            Ok(())
        }
        "in-out-changed" | "InOutChanged" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &InOutChanged| {
                let payload = json!({
                    "mark_in": ev.mark_in,
                    "mark_out": ev.mark_out,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "in-out-changed", &payload);
            });
            Ok(())
        }
        "playback-started" | "PlaybackStarted" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &PlaybackStarted| {
                let payload = json!({
                    "at": ev.at,
                    "speed": ev.speed,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "playback-started", &payload);
            });
            Ok(())
        }
        "playback-stopped" | "PlaybackStopped" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &PlaybackStopped| {
                let payload = json!({ "at": ev.at }).to_string();
                let _ = shared.call_event(&ctx_clone, "playback-stopped", &payload);
            });
            Ok(())
        }
        "media-imported" | "MediaImported" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &MediaImported| {
                let payload = json!({
                    "media": ev.media,
                    "path": ev.path,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "media-imported", &payload);
            });
            Ok(())
        }
        "media-relinked" | "MediaRelinked" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &MediaRelinked| {
                let payload = json!({
                    "media": ev.media,
                    "path": ev.path,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "media-relinked", &payload);
            });
            Ok(())
        }
        "media-offline" | "MediaOffline" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &MediaOffline| {
                let payload = json!({
                    "media": ev.media,
                    "path": ev.path,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "media-offline", &payload);
            });
            Ok(())
        }
        "render-finished" | "RenderFinished" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &RenderFinished| {
                let kind = match ev.kind {
                    RenderKind::Clip(clip) => json!({ "clip": clip }),
                    RenderKind::ImageSequence => json!("image-sequence"),
                };
                let payload = json!({
                    "kind": kind,
                    "path": ev.path,
                    "result": render_result_json(&ev.result),
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "render-finished", &payload);
            });
            Ok(())
        }
        "export-started" | "ExportStarted" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &ExportStarted| {
                let payload = json!({
                    "path": ev.path,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "export-started", &payload);
            });
            Ok(())
        }
        "export-finished" | "ExportFinished" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &ExportFinished| {
                let payload = json!({
                    "path": ev.path,
                    "result": render_result_json(&ev.result),
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "export-finished", &payload);
            });
            Ok(())
        }
        "job-progress" | "JobProgress" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &JobProgress| {
                let payload = json!({
                    "job": ev.job,
                    "label": ev.label,
                    "progress": ev.progress,
                    "detail": ev.detail,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "job-progress", &payload);
            });
            Ok(())
        }
        "job-finished" | "JobFinished" => {
            let shared = Arc::clone(shared);
            let ctx_clone = ctx.clone();
            ctx.on_event(move |ev: &JobFinished| {
                let outcome = job_outcome_name(ev.outcome);
                let payload = json!({
                    "job": ev.job,
                    "label": ev.label,
                    "outcome": outcome,
                    "detail": ev.detail,
                })
                .to_string();
                let _ = shared.call_event(&ctx_clone, "job-finished", &payload);
            });
            Ok(())
        }
        other => Err(format!("unknown event kind: {other}")),
    }
}

fn render_result_json(result: &RenderResult) -> Value {
    match result {
        RenderResult::Written => json!({ "status": "written" }),
        RenderResult::Failed(reason) => json!({ "status": "failed", "reason": reason }),
        RenderResult::Cancelled => json!({ "status": "cancelled" }),
    }
}

fn job_outcome_name(outcome: JobOutcome) -> &'static str {
    match outcome {
        JobOutcome::Done => "done",
        JobOutcome::Cancelled => "cancelled",
        JobOutcome::Failed => "failed",
    }
}

fn apply_command_proposed_response(ev: &mut CommandProposed, response_json: &str) {
    let trimmed = response_json.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
        if let Some(reason) = val
            .get("veto")
            .or_else(|| val.get("refuse"))
            .and_then(Value::as_str)
        {
            ev.refuse(reason.to_owned());
            return;
        }
    }
    if let Ok(replacement) = serde_json::from_str::<aria_commands::Command>(trimmed) {
        ev.command = replacement;
    }
}

fn parse_resolve_dropped_subscription(raw: &str) -> Option<(&str, i32)> {
    let (base, priority) = match raw.split_once(':') {
        Some((base, prio_str)) => (base, prio_str.trim().parse::<i32>().ok()?),
        None => (raw, 0),
    };
    match base {
        "resolve-dropped" | "ResolveDropped" => Some(("resolve-dropped", priority)),
        "resolve-dropped-path" | "ResolveDroppedPath" => Some(("resolve-dropped-path", priority)),
        _ => None,
    }
}

fn resolve_dropped_payload(ev: &ResolveDropped) -> String {
    match &ev.item {
        Dropped::Path(path) => {
            let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
            json!({
                "item": &ev.item,
                "path": path,
                "extension": extension,
            })
            .to_string()
        }
        Dropped::Url(url) => json!({
            "item": &ev.item,
            "url": url,
        })
        .to_string(),
    }
}

pub(crate) fn drop_action_wire_name(action: DropAction) -> &'static str {
    match action {
        DropAction::OpenProject => "open-project",
        DropAction::ImportMedia => "import-media",
        DropAction::ApplyLut => "apply-lut",
        DropAction::InstallSkill => "install-skill",
        DropAction::ImportTimeline => "import-timeline",
        DropAction::Handled => "handled",
    }
}

pub(crate) fn parse_drop_action(response_json: &str) -> Option<DropAction> {
    let trimmed = response_json.trim();
    if trimmed.is_empty() {
        return None;
    }
    let token = serde_json::from_str::<String>(trimmed).unwrap_or_else(|_| trimmed.to_owned());
    let action = match token.as_str() {
        "OpenProject" | "open-project" => DropAction::OpenProject,
        "ImportMedia" | "import-media" => DropAction::ImportMedia,
        "ApplyLut" | "apply-lut" => DropAction::ApplyLut,
        "InstallSkill" | "install-skill" => DropAction::InstallSkill,
        "ImportTimeline" | "import-timeline" => DropAction::ImportTimeline,
        "Handled" | "handled" => DropAction::Handled,
        _ => return None,
    };
    let _ = drop_action_wire_name(action);
    Some(action)
}
