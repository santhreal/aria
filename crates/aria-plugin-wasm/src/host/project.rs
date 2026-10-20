use std::sync::Arc;

use aria_commands::Command;
use aria_models::{ClipId, MarkerColor, MarkerKind, SequenceId};
use aria_plugin::package::Access;
use aria_plugin::{
    ClipRef, EditorView, Origin, PluginContext, ProjectHost, Rational, RationalTime, TimeRange,
};
use serde_json::json;

use super::HostState;
use crate::bindings;

/// Marker type used with `PluginContext::has` to record a `CapabilityDenial` when a plugin
/// with `permissions.project = "read"` attempts a write operation (`project-transact` or `project-undo`).
struct ProjectWritePermission;

impl HostState {
    fn require_project_read(&self) -> Result<(PluginContext, Arc<ProjectHost>), String> {
        let ctx = self.require_ctx()?;
        if self.package.manifest.permissions.project == Access::None {
            let _ = ctx.try_get::<ProjectHost>();
            return Err(format!(
                "permission denied: {}: project access requires permissions.project = \"read\" or \"read-write\"",
                self.package.manifest.id
            ));
        }
        let host = ctx
            .try_get::<ProjectHost>()
            .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
        Ok((ctx, host))
    }

    fn require_project_write(&self) -> Result<(PluginContext, Arc<ProjectHost>), String> {
        let ctx = self.require_ctx()?;
        match self.package.manifest.permissions.project {
            Access::None => {
                let _ = ctx.try_get::<ProjectHost>();
                return Err(format!(
                    "permission denied: {}: project write requires permissions.project = \"read-write\"",
                    self.package.manifest.id
                ));
            }
            Access::Read => {
                let _ = ctx.has::<ProjectWritePermission>();
                return Err(format!(
                    "permission denied: {}: project write requires permissions.project = \"read-write\" (have \"read\")",
                    self.package.manifest.id
                ));
            }
            Access::ReadWrite => {}
        }
        let host = ctx
            .try_get::<ProjectHost>()
            .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
        Ok((ctx, host))
    }

    pub(super) fn host_project_snapshot(&mut self) -> Result<String, String> {
        let (_ctx, host) = self.require_project_read()?;
        let snapshot = host.snapshot();
        serde_json::to_string(snapshot.as_ref()).map_err(|err| err.to_string())
    }

    pub(super) fn host_editor_view(&mut self) -> Result<String, String> {
        let (ctx, _host) = self.require_project_read()?;
        let state = ctx.try_get::<EditorView>().and_then(|view| view.current());
        match state {
            Some(view) => Ok(json!({
                "selection": view.selection,
                "playhead": view.playhead,
                "mark_in": view.mark_in,
                "mark_out": view.mark_out,
                "playing": view.playing,
                "speed": view.speed,
            })
            .to_string()),
            None => Ok(json!({
                "selection": [],
                "playhead": null,
                "mark_in": null,
                "mark_out": null,
                "playing": false,
                "speed": 0,
            })
            .to_string()),
        }
    }

    pub(super) fn host_clip(&mut self, id: u64) -> Result<Option<bindings::ClipRef>, String> {
        let (_ctx, host) = self.require_project_read()?;
        Ok(host.clip(ClipId::from_raw(id)).map(clip_ref_to_wit))
    }

    pub(super) fn host_clips_in_range(
        &mut self,
        sequence: Option<u64>,
        range: bindings::TimeRange,
    ) -> Result<Vec<bindings::ClipRef>, String> {
        let (_ctx, host) = self.require_project_read()?;
        let range = wit_to_time_range(range)?;
        Ok(host
            .clips_in_range(sequence.map(SequenceId::from_raw), range)
            .into_iter()
            .map(clip_ref_to_wit)
            .collect())
    }

    pub(super) fn host_clips_at(
        &mut self,
        sequence: Option<u64>,
        tick: bindings::RationalTime,
    ) -> Result<Vec<bindings::ClipRef>, String> {
        let (_ctx, host) = self.require_project_read()?;
        let tick = wit_to_rational_time(tick)?;
        Ok(host
            .clips_at(sequence.map(SequenceId::from_raw), tick)
            .into_iter()
            .map(clip_ref_to_wit)
            .collect())
    }

    pub(super) fn host_selected_clips(&mut self) -> Result<Vec<bindings::ClipRef>, String> {
        let (ctx, host) = self.require_project_read()?;
        let selection = ctx
            .try_get::<EditorView>()
            .and_then(|view| view.current())
            .map(|state| state.selection)
            .unwrap_or_default();
        Ok(host
            .selected_clips(&selection)
            .into_iter()
            .map(clip_ref_to_wit)
            .collect())
    }

    pub(super) fn host_markers_in_range(
        &mut self,
        sequence: Option<u64>,
        range: bindings::TimeRange,
    ) -> Result<Vec<bindings::Marker>, String> {
        let (_ctx, host) = self.require_project_read()?;
        let range = wit_to_time_range(range)?;
        Ok(host
            .markers_in_range(sequence.map(SequenceId::from_raw), range)
            .into_iter()
            .map(marker_to_wit)
            .collect())
    }

    pub(super) fn host_in_out(&mut self) -> Result<bindings::InOutMarks, String> {
        let (ctx, _host) = self.require_project_read()?;
        let state = ctx.try_get::<EditorView>().and_then(|view| view.current());
        Ok(match state {
            Some(view) => bindings::InOutMarks {
                mark_in: view.mark_in.map(rational_time_to_wit),
                mark_out: view.mark_out.map(rational_time_to_wit),
            },
            None => bindings::InOutMarks {
                mark_in: None,
                mark_out: None,
            },
        })
    }

    pub(super) fn host_project_transact(
        &mut self,
        label: String,
        commands_json: String,
    ) -> Result<String, String> {
        let (_ctx, host) = self.require_project_write()?;
        let commands: Vec<Command> = serde_json::from_str(&commands_json)
            .map_err(|err| format!("invalid commands JSON: {err}"))?;
        let origin = Origin::plugin(&self.package.manifest.id);
        let receipt = host
            .transact(origin, label, commands)
            .map_err(|err| err.to_string())?;
        Ok(json!({ "revision": receipt.revision, "outcomes": receipt.outcomes }).to_string())
    }

    pub(super) fn host_project_undo(&mut self) -> Result<bool, String> {
        let (_ctx, host) = self.require_project_write()?;
        host.undo().map_err(|err| err.to_string())
    }
}

fn rational_time_to_wit(time: RationalTime) -> bindings::RationalTime {
    bindings::RationalTime {
        value: time.value,
        rate_num: time.rate.num,
        rate_den: time.rate.den,
    }
}

fn wit_to_rational_time(time: bindings::RationalTime) -> Result<RationalTime, String> {
    let rate = Rational::new(time.rate_num, time.rate_den);
    if !rate.is_valid() {
        return Err(format!(
            "invalid rational rate {}/{}: num and den must be positive",
            time.rate_num, time.rate_den
        ));
    }
    Ok(RationalTime::new(time.value, rate))
}

fn wit_to_time_range(range: bindings::TimeRange) -> Result<TimeRange, String> {
    let start = wit_to_rational_time(range.start)?;
    let duration = wit_to_rational_time(range.duration)?;
    TimeRange::new(start, duration).map_err(|err| format!("invalid time range: {err}"))
}

fn clip_ref_to_wit(clip: ClipRef) -> bindings::ClipRef {
    bindings::ClipRef {
        id: clip.id.raw(),
        track: clip.track.raw(),
        start: rational_time_to_wit(clip.start),
        end: rational_time_to_wit(clip.end),
        kind: clip.kind,
        name: clip.name,
    }
}

fn marker_to_wit(marker: aria_models::Marker) -> bindings::Marker {
    let color = match marker.color {
        MarkerColor::Teal => bindings::MarkerColor::Teal,
        MarkerColor::Blue => bindings::MarkerColor::Blue,
        MarkerColor::Purple => bindings::MarkerColor::Purple,
        MarkerColor::Pink => bindings::MarkerColor::Pink,
        MarkerColor::Red => bindings::MarkerColor::Red,
        MarkerColor::Orange => bindings::MarkerColor::Orange,
        MarkerColor::Yellow => bindings::MarkerColor::Yellow,
        MarkerColor::Green => bindings::MarkerColor::Green,
    };
    let kind = match marker.kind {
        MarkerKind::Comment => bindings::MarkerKind::Comment,
        MarkerKind::Chapter => bindings::MarkerKind::Chapter,
        MarkerKind::Segmentation => bindings::MarkerKind::Segmentation,
    };
    bindings::Marker {
        id: marker.id.raw(),
        tick: rational_time_to_wit(marker.tick),
        duration: rational_time_to_wit(marker.duration),
        name: marker.name,
        color,
        comment: marker.comment,
        kind,
    }
}
