use std::path::Path;
use std::sync::Arc;

use super::HostState;
use crate::bindings;
use aria_models::{ClipId, MarkerColor, MarkerKind, SequenceId};
use aria_plugin::package::Access;
use aria_plugin::{
    ClipRef, EditorView, FrameRequest, FrameSize, FrameSource, PluginContext, ProjectHost,
    Rational, RationalTime, TimeRange, host_wire,
};

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
        host_wire::project_snapshot_json(&host)
    }

    pub(super) fn host_editor_view(&mut self) -> Result<String, String> {
        let (ctx, _host) = self.require_project_read()?;
        let state = ctx.try_get::<EditorView>().and_then(|view| view.current());
        Ok(host_wire::editor_view_json(state.as_ref(), None, 0))
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
        Ok(host_wire::selected_clips_for_view(&ctx, &host)
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
        host_wire::project_transact_json(&host, &self.package.manifest.id, label, &commands_json)
    }

    pub(super) fn host_project_undo(&mut self) -> Result<bool, String> {
        let (_ctx, host) = self.require_project_write()?;
        host.undo()
            .map(|reverted| reverted.is_some())
            .map_err(|err| err.to_string())
    }

    pub(super) fn host_render_frame(
        &mut self,
        source: bindings::FrameSource,
        time: bindings::RationalTime,
        size: bindings::FrameSize,
    ) -> Result<bindings::RenderedFrame, String> {
        let (ctx, host) = self.require_project_read()?;
        let time = wit_to_rational_time(time)?;
        let source = match source {
            bindings::FrameSource::Open => FrameSource::Open,
            bindings::FrameSource::Sequence(seq) => {
                FrameSource::Sequence(seq.map(SequenceId::from_raw))
            }
            bindings::FrameSource::Clip(id) => FrameSource::Clip(ClipId::from_raw(id)),
        };
        let size = match size {
            bindings::FrameSize::Canvas => FrameSize::Canvas,
            bindings::FrameSize::Fit((max_width, max_height)) => FrameSize::Fit {
                max_width,
                max_height,
            },
            bindings::FrameSize::Exact((width, height)) => FrameSize::Exact { width, height },
        };
        let req = FrameRequest {
            source,
            size,
            motion_blur: false,
        };
        let image = host_wire::render_frame(&ctx, &host, time, &req)?;
        Ok(bindings::RenderedFrame {
            width: image.width,
            height: image.height,
            data: image.pixels,
        })
    }

    pub(super) fn host_editor_seek(
        &mut self,
        time: bindings::RationalTime,
    ) -> Result<bindings::RationalTime, String> {
        let (ctx, host) = self.require_project_write()?;
        let time = wit_to_rational_time(time)?;
        let target = host_wire::editor_seek(&ctx, &host, time)?;
        Ok(rational_time_to_wit(target))
    }

    pub(super) fn host_editor_play(&mut self, speed: i32) -> Result<(), String> {
        let (ctx, _host) = self.require_project_write()?;
        host_wire::editor_play(&ctx, speed)
    }

    pub(super) fn host_editor_stop(&mut self) -> Result<(), String> {
        let (ctx, _host) = self.require_project_write()?;
        host_wire::editor_stop(&ctx)
    }

    pub(super) fn host_project_open(&mut self, path: String) -> Result<(), String> {
        let (ctx, host) = self.require_project_write()?;
        let allowed = match host_wire::check_project_fs_path(
            &self.package,
            &host,
            Path::new(&path),
            "read",
        ) {
            Ok(allowed) => allowed,
            Err(err) => {
                if err.starts_with("permission denied:") {
                    let _ = ctx.has::<super::storage::FsPermission>();
                }
                return Err(err);
            }
        };
        host_wire::project_open(&host, &self.package.manifest.id, allowed)
    }

    pub(super) fn host_project_save(&mut self) -> Result<String, String> {
        let (ctx, host) = self.require_project_write()?;
        let current_path = host.project_path().ok_or_else(|| {
            "project has no file path; call project-save-as with a destination path first"
                .to_owned()
        })?;
        if let Err(err) =
            host_wire::check_project_fs_path(&self.package, &host, &current_path, "write")
        {
            if err.starts_with("permission denied:") {
                let _ = ctx.has::<super::storage::FsPermission>();
            }
            return Err(err);
        }
        let saved = host_wire::project_save(&host, &self.package.manifest.id)?;
        Ok(saved.to_string_lossy().into_owned())
    }

    pub(super) fn host_project_save_as(&mut self, path: String) -> Result<String, String> {
        let (ctx, host) = self.require_project_write()?;
        let allowed =
            match host_wire::check_project_fs_path(&self.package, &host, Path::new(&path), "write")
            {
                Ok(allowed) => allowed,
                Err(err) => {
                    if err.starts_with("permission denied:") {
                        let _ = ctx.has::<super::storage::FsPermission>();
                    }
                    return Err(err);
                }
            };
        let saved = host_wire::project_save_as(&host, &self.package.manifest.id, allowed)?;
        Ok(saved.to_string_lossy().into_owned())
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
