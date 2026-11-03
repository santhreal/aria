use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use aria_core::Rational;
use aria_models::{ClipId, SequenceId};
use aria_plugin_abi::{
    ARIA_FRAME_SIZE_CANVAS, ARIA_FRAME_SIZE_EXACT, ARIA_FRAME_SIZE_FIT, ARIA_FRAME_SOURCE_CLIP,
    ARIA_FRAME_SOURCE_OPEN, ARIA_FRAME_SOURCE_SEQUENCE, AriaBytesResult, AriaFrameRequest,
    AriaResult, AriaStr,
};

use super::{FsPermission, ProjectWrite, call_ctx, read_str};
use crate::external::HostCallContext;
use crate::package::Access;
use crate::{
    EditorView, FrameRequest, FrameSize, FrameSource, ProjectHost, RationalTime, TransactError,
    host_wire,
};

pub(super) extern "C" fn host_project_snapshot(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_project_snapshot`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for project_snapshot"
                        .to_owned(),
                );
            }
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            host_wire::project_snapshot_json(&host)
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_editor_view(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_editor_view`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<EditorView>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for editor_view"
                        .to_owned(),
                );
            }
            let view_state = ctx
                .unscoped_ctx
                .try_get::<EditorView>()
                .and_then(|view| view.current());
            Ok(host_wire::editor_view_json(
                view_state.as_ref(),
                Some(RationalTime::new(0, aria_core::Rational::FPS_24)),
                1,
            ))
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

fn require_project_read_host<'a>(
    ctx: &'a HostCallContext<'a>,
    fn_name: &str,
) -> Result<Arc<ProjectHost>, String> {
    if ctx.state.package.manifest.permissions.project == Access::None {
        let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
        return Err(format!(
            "permission denied: project = \"read\" or \"read-write\" is required for {fn_name}"
        ));
    }
    ctx.scoped_ctx
        .try_get::<ProjectHost>()
        .ok_or_else(|| "ProjectHost service is not registered".to_owned())
}

fn optional_sequence_id(sequence_id: u64) -> Option<crate::SequenceId> {
    (sequence_id != 0).then_some(crate::SequenceId::from_raw(sequence_id))
}

pub(super) extern "C" fn host_clip(host_ctx: *mut c_void, id: u64) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_clip`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(ctx, "clip")?;
            match host.clip(crate::ClipId::from_raw(id)) {
                Some(found) => serde_json::to_string(&found)
                    .map(Some)
                    .map_err(|err| err.to_string()),
                None => Ok(None),
            }
        })
    };
    match result {
        Ok(Some(json)) => AriaResult::ok(json),
        Ok(None) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_clips_in_range(
    host_ctx: *mut c_void,
    sequence_id: u64,
    range_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `range_json` are valid for the duration of `host_clips_in_range`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(ctx, "clips_in_range")?;
            let raw = read_str(range_json, "time range JSON")?;
            let range: crate::TimeRange = serde_json::from_str(raw)
                .map_err(|err| format!("invalid time range JSON: {err}"))?;
            let clips = host.clips_in_range(optional_sequence_id(sequence_id), range);
            serde_json::to_string(&clips).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_clips_at(
    host_ctx: *mut c_void,
    sequence_id: u64,
    tick_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `tick_json` are valid for the duration of `host_clips_at`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(ctx, "clips_at")?;
            let raw = read_str(tick_json, "tick JSON")?;
            let tick: RationalTime =
                serde_json::from_str(raw).map_err(|err| format!("invalid tick JSON: {err}"))?;
            let clips = host.clips_at(optional_sequence_id(sequence_id), tick);
            serde_json::to_string(&clips).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_selected_clips(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_selected_clips`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(ctx, "selected_clips")?;
            let clips = host_wire::selected_clips_for_view(&ctx.unscoped_ctx, &host);
            serde_json::to_string(&clips).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_markers_in_range(
    host_ctx: *mut c_void,
    sequence_id: u64,
    range_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `range_json` are valid for the duration of `host_markers_in_range`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_read_host(ctx, "markers_in_range")?;
            let raw = read_str(range_json, "time range JSON")?;
            let range: crate::TimeRange = serde_json::from_str(raw)
                .map_err(|err| format!("invalid time range JSON: {err}"))?;
            let markers = host.markers_in_range(optional_sequence_id(sequence_id), range);
            serde_json::to_string(&markers).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_in_out(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_in_out`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<EditorView>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for in_out"
                        .to_owned(),
                );
            }
            let view_state = ctx
                .unscoped_ctx
                .try_get::<EditorView>()
                .and_then(|view| view.current());
            let in_out = view_state.as_ref().map(crate::in_out).unwrap_or_default();
            serde_json::to_string(&in_out).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_project_transact(
    host_ctx: *mut c_void,
    label: AriaStr,
    commands_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx`, `label`, and `commands_json` are valid for the duration of `host_project_transact`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            match ctx.state.package.manifest.permissions.project {
                Access::None => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_transact"
                            .to_owned(),
                    );
                }
                Access::Read => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectWrite>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_transact"
                            .to_owned(),
                    );
                }
                Access::ReadWrite => {}
            }
            let label = read_str(label, "transaction label")?;
            let commands_raw = read_str(commands_json, "commands JSON")?;
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            host_wire::project_transact_json(&host, &ctx.state.id, label.to_owned(), commands_raw)
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_project_undo(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_project_undo`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            match ctx.state.package.manifest.permissions.project {
                Access::None => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_undo"
                            .to_owned(),
                    );
                }
                Access::Read => {
                    let _ = ctx.scoped_ctx.try_get::<ProjectWrite>();
                    return Err(
                        "permission denied: project = \"read-write\" is required for project_undo"
                            .to_owned(),
                    );
                }
                Access::ReadWrite => {}
            }
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            let stepped = catch_unwind(AssertUnwindSafe(|| host.undo()))
                .map_err(|_| TransactError::Reentrant.to_string())?
                .map_err(|err| err.to_string())?;
            Ok(if stepped { "true" } else { "false" }.to_owned())
        })
    };
    match result {
        Ok(val) => AriaResult::ok(val),
        Err(err) => AriaResult::err(err),
    }
}

fn require_project_write<'a>(
    ctx: &'a HostCallContext<'a>,
    call_name: &str,
) -> Result<Arc<ProjectHost>, String> {
    match ctx.state.package.manifest.permissions.project {
        Access::None => {
            let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
            return Err(format!(
                "permission denied: project = \"read-write\" is required for {call_name}"
            ));
        }
        Access::Read => {
            let _ = ctx.scoped_ctx.try_get::<ProjectWrite>();
            return Err(format!(
                "permission denied: project = \"read-write\" is required for {call_name}"
            ));
        }
        Access::ReadWrite => {}
    }
    ctx.scoped_ctx
        .try_get::<ProjectHost>()
        .ok_or_else(|| "ProjectHost service is not registered".to_owned())
}

pub(super) extern "C" fn host_render_frame(
    host_ctx: *mut c_void,
    request: *mut AriaFrameRequest,
) -> AriaBytesResult {
    // SAFETY: `host_ctx` and `request` are valid for the duration of `host_render_frame`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if ctx.state.package.manifest.permissions.project == Access::None {
                let _ = ctx.scoped_ctx.try_get::<ProjectHost>();
                return Err(
                    "permission denied: project = \"read\" or \"read-write\" is required for render_frame"
                        .to_owned(),
                );
            }
            if request.is_null() {
                return Err("render_frame request pointer is null".to_owned());
            }
            let req = &mut *request;
            let source = match req.source_kind {
                ARIA_FRAME_SOURCE_OPEN => FrameSource::Open,
                ARIA_FRAME_SOURCE_SEQUENCE => {
                    FrameSource::Sequence(
                        (req.source_id != 0).then(|| SequenceId::from_raw(req.source_id)),
                    )
                }
                ARIA_FRAME_SOURCE_CLIP => FrameSource::Clip(ClipId::from_raw(req.source_id)),
                other => {
                    return Err(format!(
                        "invalid frame source_kind {other}: expected ARIA_FRAME_SOURCE_OPEN (0), ARIA_FRAME_SOURCE_SEQUENCE (1), or ARIA_FRAME_SOURCE_CLIP (2)"
                    ));
                }
            };
            let size = match req.size_kind {
                ARIA_FRAME_SIZE_CANVAS => FrameSize::Canvas,
                ARIA_FRAME_SIZE_FIT => FrameSize::Fit {
                    max_width: req.width,
                    max_height: req.height,
                },
                ARIA_FRAME_SIZE_EXACT => FrameSize::Exact {
                    width: req.width,
                    height: req.height,
                },
                other => {
                    return Err(format!(
                        "invalid frame size_kind {other}: expected ARIA_FRAME_SIZE_CANVAS (0), ARIA_FRAME_SIZE_FIT (1), or ARIA_FRAME_SIZE_EXACT (2)"
                    ));
                }
            };
            if req.time_rate_num <= 0 || req.time_rate_den <= 0 {
                return Err(format!(
                    "invalid frame time rate {}/{}: rate numerator and denominator must be positive",
                    req.time_rate_num, req.time_rate_den
                ));
            }
            let time = RationalTime::new(
                req.time_value,
                Rational::new(req.time_rate_num, req.time_rate_den),
            );
            let host = ctx
                .scoped_ctx
                .try_get::<ProjectHost>()
                .ok_or_else(|| "ProjectHost service is not registered".to_owned())?;
            let frame_req = FrameRequest {
                source,
                size,
                motion_blur: false,
            };
            let image = host_wire::render_frame(&ctx.unscoped_ctx, &host, time, &frame_req)?;
            req.width = image.width;
            req.height = image.height;
            Ok(image.pixels)
        })
    };
    match result {
        Ok(bytes) => AriaBytesResult::found(bytes),
        Err(err) => AriaBytesResult::err(err),
    }
}

pub(super) extern "C" fn host_editor_seek(host_ctx: *mut c_void, tick_json: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `tick_json` are valid for the duration of `host_editor_seek`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_write(ctx, "editor_seek")?;
            let raw = read_str(tick_json, "tick_json")?;
            let target = host_wire::editor_seek_json(&ctx.unscoped_ctx, &host, raw)?;
            serde_json::to_string(&target).map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(json) => AriaResult::ok(json),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_editor_play(host_ctx: *mut c_void, speed: i32) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_editor_play`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let _host = require_project_write(ctx, "editor_play")?;
            host_wire::editor_play(&ctx.unscoped_ctx, speed)?;
            Ok("ok".to_owned())
        })
    };
    match result {
        Ok(msg) => AriaResult::ok(msg),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_editor_stop(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_editor_stop`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let _host = require_project_write(ctx, "editor_stop")?;
            host_wire::editor_stop(&ctx.unscoped_ctx)?;
            Ok("ok".to_owned())
        })
    };
    match result {
        Ok(msg) => AriaResult::ok(msg),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_project_open(host_ctx: *mut c_void, path: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `path` are valid for the duration of `host_project_open`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_write(ctx, "project_open")?;
            let raw_path = read_str(path, "path")?;
            let allowed = match host_wire::check_project_fs_path(
                &ctx.state.package,
                &host,
                Path::new(raw_path),
                "read",
            ) {
                Ok(allowed) => allowed,
                Err(err) => {
                    if err.starts_with("permission denied:") {
                        let _ = ctx.scoped_ctx.try_get::<FsPermission>();
                    }
                    return Err(err);
                }
            };
            catch_unwind(AssertUnwindSafe(|| {
                host_wire::project_open(&host, &ctx.state.id, allowed)
            }))
            .map_err(|_| TransactError::Reentrant.to_string())??;
            Ok("ok".to_owned())
        })
    };
    match result {
        Ok(msg) => AriaResult::ok(msg),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_project_save(host_ctx: *mut c_void) -> AriaResult {
    // SAFETY: `host_ctx` is valid for the duration of `host_project_save`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_write(ctx, "project_save")?;
            let current_path = host.project_path().ok_or_else(|| {
                "project has no file path; call project_save_as with a destination path first"
                    .to_owned()
            })?;
            if let Err(err) =
                host_wire::check_project_fs_path(&ctx.state.package, &host, &current_path, "write")
            {
                if err.starts_with("permission denied:") {
                    let _ = ctx.scoped_ctx.try_get::<FsPermission>();
                }
                return Err(err);
            }
            let saved = catch_unwind(AssertUnwindSafe(|| {
                host_wire::project_save(&host, &ctx.state.id)
            }))
            .map_err(|_| TransactError::Reentrant.to_string())??;
            Ok(saved.to_string_lossy().into_owned())
        })
    };
    match result {
        Ok(path) => AriaResult::ok(path),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_project_save_as(host_ctx: *mut c_void, path: AriaStr) -> AriaResult {
    // SAFETY: `host_ctx` and `path` are valid for the duration of `host_project_save_as`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            let host = require_project_write(ctx, "project_save_as")?;
            let raw_path = read_str(path, "path")?;
            let allowed = match host_wire::check_project_fs_path(
                &ctx.state.package,
                &host,
                Path::new(raw_path),
                "write",
            ) {
                Ok(allowed) => allowed,
                Err(err) => {
                    if err.starts_with("permission denied:") {
                        let _ = ctx.scoped_ctx.try_get::<FsPermission>();
                    }
                    return Err(err);
                }
            };
            let saved = catch_unwind(AssertUnwindSafe(|| {
                host_wire::project_save_as(&host, &ctx.state.id, allowed)
            }))
            .map_err(|_| TransactError::Reentrant.to_string())??;
            Ok(saved.to_string_lossy().into_owned())
        })
    };
    match result {
        Ok(path) => AriaResult::ok(path),
        Err(err) => AriaResult::err(err),
    }
}
