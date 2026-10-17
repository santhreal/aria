//! Event subscription bridging between `PluginContext` and native plugin `event` callbacks.

use std::sync::Arc;

use serde_json::{Value, json};

use super::NativePluginState;
use crate::{
    Command, CommandApplied, CommandProposed, DropAction, Dropped, Event, ExportFinished,
    ExportStarted, InOutChanged, JobFinished, JobOutcome, JobProgress, MediaImported, MediaOffline,
    MediaRelinked, PlaybackStarted, PlaybackStopped, PlayheadMoved, PluginContext, ProjectClosed,
    ProjectOpened, ProjectSaved, RenderFinished, RenderKind, RenderResult, ResolveDropped,
    SelectionChanged,
};

/// Sentinel event type used only so `EventBus` holds an unmount drop guard inside
/// the plugin's registration list without adding an entry to `MountedPluginInfo::services`.
pub(crate) struct UnmountSentinel;

impl Event for UnmountSentinel {}

pub(crate) fn subscribe_event(
    ctx: &PluginContext,
    state: &Arc<NativePluginState>,
    event_kind: &str,
) -> Result<(), String> {
    let kind_owned = event_kind.to_owned();
    match event_kind {
        "command-proposed" | "CommandProposed" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_waterfall(move |ev: &mut CommandProposed| {
                let (target_tracks, target_clips) = match &ev.command {
                    Command::Edit(edit) => {
                        let targets = edit.targets();
                        (targets.tracks, targets.clips)
                    }
                    Command::Project(_) => (Vec::new(), Vec::new()),
                };
                let mut payload = serde_json::to_value(&ev.command).unwrap_or_else(|_| json!({}));
                if let Value::Object(map) = &mut payload {
                    map.insert(
                        "command".to_owned(),
                        serde_json::to_value(&ev.command).unwrap_or(Value::Null),
                    );
                    map.insert("origin".to_owned(), Value::String(ev.origin.to_string()));
                    map.insert("target_tracks".to_owned(), json!(target_tracks));
                    map.insert("target_clips".to_owned(), json!(target_clips));
                }
                if let Ok(response) = state.call_event(&scoped, &kind_owned, &payload.to_string()) {
                    apply_command_proposed_response(ev, &response);
                }
            });
            Ok(())
        }
        kind if parse_resolve_dropped_subscription(kind).is_some() => {
            let (dispatch_kind, priority) =
                parse_resolve_dropped_subscription(event_kind).expect("checked Some");
            let kind_owned = dispatch_kind.to_owned();
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_serial_with_priority(priority, move |ev: &ResolveDropped| {
                let payload = resolve_dropped_payload(ev);
                match state.call_event(&scoped, &kind_owned, &payload) {
                    Ok(response) => parse_drop_action(&response),
                    Err(()) => None,
                }
            });
            Ok(())
        }
        "command-applied" | "CommandApplied" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &CommandApplied| {
                let payload = json!({
                    "command": ev.command,
                    "outcome": ev.outcome,
                    "revision": ev.revision,
                    "origin": ev.origin.to_string(),
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "project-opened" | "ProjectOpened" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &ProjectOpened| {
                let payload = json!({ "path": ev.path }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "project-closed" | "ProjectClosed" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &ProjectClosed| {
                let payload = json!({ "path": ev.path }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "project-saved" | "ProjectSaved" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &ProjectSaved| {
                let payload = json!({ "path": ev.path }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "selection-changed" | "SelectionChanged" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &SelectionChanged| {
                let payload = json!({ "clips": ev.clips }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "playhead-moved" | "PlayheadMoved" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &PlayheadMoved| {
                let payload = json!({ "at": ev.at }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "in-out-changed" | "InOutChanged" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &InOutChanged| {
                let payload = json!({
                    "mark_in": ev.mark_in,
                    "mark_out": ev.mark_out,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "playback-started" | "PlaybackStarted" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &PlaybackStarted| {
                let payload = json!({
                    "at": ev.at,
                    "speed": ev.speed,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "playback-stopped" | "PlaybackStopped" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &PlaybackStopped| {
                let payload = json!({ "at": ev.at }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "media-imported" | "MediaImported" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &MediaImported| {
                let payload = json!({
                    "media": ev.media,
                    "path": ev.path,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "media-relinked" | "MediaRelinked" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &MediaRelinked| {
                let payload = json!({
                    "media": ev.media,
                    "path": ev.path,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "media-offline" | "MediaOffline" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &MediaOffline| {
                let payload = json!({
                    "media": ev.media,
                    "path": ev.path,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "render-finished" | "RenderFinished" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &RenderFinished| {
                let kind = match ev.kind {
                    RenderKind::Clip(clip) => json!({ "clip": clip }),
                    RenderKind::ImageSequence => json!("image_sequence"),
                };
                let result = render_result_json(&ev.result);
                let payload = json!({
                    "kind": kind,
                    "path": ev.path,
                    "result": result,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "export-started" | "ExportStarted" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &ExportStarted| {
                let payload = json!({ "path": ev.path }).to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "export-finished" | "ExportFinished" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &ExportFinished| {
                let payload = json!({
                    "path": ev.path,
                    "result": render_result_json(&ev.result),
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "job-progress" | "JobProgress" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &JobProgress| {
                let payload = json!({
                    "job": ev.job,
                    "label": ev.label,
                    "progress": ev.progress,
                    "detail": ev.detail,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        "job-finished" | "JobFinished" => {
            let state = Arc::clone(state);
            let scoped = ctx.clone();
            ctx.on_event(move |ev: &JobFinished| {
                let outcome = match ev.outcome {
                    JobOutcome::Done => "done",
                    JobOutcome::Failed => "failed",
                    JobOutcome::Cancelled => "cancelled",
                };
                let payload = json!({
                    "job": ev.job,
                    "label": ev.label,
                    "outcome": outcome,
                    "detail": ev.detail,
                })
                .to_string();
                let _ = state.call_event(&scoped, &kind_owned, &payload);
            });
            Ok(())
        }
        other => Err(format!("unknown event kind: {other}")),
    }
}

fn parse_resolve_dropped_subscription(event_kind: &str) -> Option<(&str, i32)> {
    match event_kind {
        "resolve-dropped" | "ResolveDropped" | "resolve-dropped-path" | "ResolveDroppedPath" => {
            Some((event_kind, 0))
        }
        _ => {
            for prefix in [
                "resolve-dropped:",
                "ResolveDropped:",
                "resolve-dropped-path:",
                "ResolveDroppedPath:",
            ] {
                if let Some(rest) = event_kind.strip_prefix(prefix)
                    && let Ok(priority) = rest.trim().parse::<i32>()
                {
                    let base = prefix.trim_end_matches(':');
                    return Some((base, priority));
                }
            }
            None
        }
    }
}

fn resolve_dropped_payload(ev: &ResolveDropped) -> String {
    match &ev.item {
        Dropped::Path(path) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default();
            json!({
                "kind": "path",
                "path": path,
                "extension": ext,
            })
            .to_string()
        }
        Dropped::Url(url) => json!({
            "kind": "url",
            "url": url,
        })
        .to_string(),
    }
}

fn render_result_json(result: &RenderResult) -> Value {
    match result {
        RenderResult::Written => json!({ "status": "written" }),
        RenderResult::Failed(reason) => json!({ "status": "failed", "reason": reason }),
        RenderResult::Cancelled => json!({ "status": "cancelled" }),
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
        if let Some(replacement) = val
            .get("rewrite")
            .and_then(|v| serde_json::from_value::<Command>(v.clone()).ok())
        {
            ev.command = replacement;
            return;
        }
    }
    if let Ok(replacement) = serde_json::from_str::<Command>(trimmed) {
        ev.command = replacement;
    }
}

fn parse_drop_action(response_json: &str) -> Option<DropAction> {
    let trimmed = response_json.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(action) = serde_json::from_str::<Value>(trimmed).ok().and_then(|val| {
        let action_str = val
            .get("claim_drop")
            .and_then(Value::as_str)
            .or_else(|| val.as_str())?;
        Some(match_drop_action(action_str))
    }) {
        return action;
    }
    match_drop_action(trimmed)
}

fn match_drop_action(token: &str) -> Option<DropAction> {
    match token {
        "OpenProject" | "open-project" => Some(DropAction::OpenProject),
        "ImportMedia" | "import-media" => Some(DropAction::ImportMedia),
        "ApplyLut" | "apply-lut" => Some(DropAction::ApplyLut),
        "InstallSkill" | "install-skill" => Some(DropAction::InstallSkill),
        "ImportTimeline" | "import-timeline" => Some(DropAction::ImportTimeline),
        "Handled" | "handled" => Some(DropAction::Handled),
        _ => None,
    }
}
