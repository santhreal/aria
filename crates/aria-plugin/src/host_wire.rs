//! Shared host operations for external plugins (`aria-plugin` native C ABI and
//! `aria-plugin-wasm` WIT components).
//!
//! Each host decodes its guest ABI representation and calls the functions in
//! this module for validation, state transitions, and service operations that
//! behave identically across both runtimes.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use aria_commands::Command;
use aria_compositor::{CpuFrameIn, CpuFrameOut, RgbaImage};
use aria_core::{EncoderConfig, RationalTime};
use aria_models::{ClipTypeIcon, EffectParamKind, EffectParamSpec, Preset};
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE};
use serde_json::json;

pub use crate::editor_events::rescale_time;
use crate::package::{FsScope, Package};
use crate::{
    AnalyzerResult, ClipRef, EditorView, Event, FrameRequest, FrameSize, InterchangeReport,
    MAX_RENDER_FRAME_DIMENSION, MAX_WIDGET_ID_BYTES, Origin, PluginContext, ProjectHost,
    RenderHost, SettingsService, TransactError, Transaction, ViewState,
};

/// Sentinel event type used so `EventBus` holds an unmount drop guard inside a
/// plugin's registration list without adding an entry to
/// [`crate::MountedPluginInfo::services`].
struct UnmountSentinel;

impl Event for UnmountSentinel {}

struct UnmountGuard<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for UnmountGuard<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// Run `on_drop` when `ctx`'s plugin registrations are dropped on unmount or
/// fault.
pub fn on_unmount(ctx: &PluginContext, on_drop: impl FnOnce() + Send + Sync + 'static) {
    let guard = UnmountGuard(Some(on_drop));
    ctx.on_event(move |_: &UnmountSentinel| {
        let _keep = &guard;
    });
}

/// Serialize the current project snapshot of `host` to JSON.
pub fn project_snapshot_json(host: &ProjectHost) -> Result<String, String> {
    let snapshot = host.snapshot();
    serde_json::to_string(snapshot.as_ref()).map_err(|err| err.to_string())
}

/// Serialize `state` as the JSON object returned by `editor_view` /
/// `editor-view`, using `default_playhead` and `default_speed` when no view
/// state is active.
pub fn editor_view_json(
    state: Option<&ViewState>,
    default_playhead: Option<RationalTime>,
    default_speed: i32,
) -> String {
    match state {
        Some(view) => json!({
            "selection": view.selection,
            "playhead": view.playhead,
            "mark_in": view.mark_in,
            "mark_out": view.mark_out,
            "playing": view.playing,
            "speed": view.speed,
        })
        .to_string(),
        None => json!({
            "selection": Vec::<u64>::new(),
            "playhead": default_playhead,
            "mark_in": Option::<RationalTime>::None,
            "mark_out": Option::<RationalTime>::None,
            "playing": false,
            "speed": default_speed,
        })
        .to_string(),
    }
}

/// Return the [`ClipRef`] list for the current selection in `view_ctx` against
/// `host`.
pub fn selected_clips_for_view(view_ctx: &PluginContext, host: &ProjectHost) -> Vec<ClipRef> {
    let selection = view_ctx
        .try_get::<EditorView>()
        .and_then(|view| view.current())
        .map(|state| state.selection)
        .unwrap_or_default();
    host.selected_clips(&selection)
}

/// Parse `commands_json`, apply the transaction to `host` under
/// `Origin::plugin(plugin_id)` with `label`, and return the receipt JSON.
pub fn project_transact_json(
    host: &ProjectHost,
    plugin_id: &str,
    label: String,
    commands_json: &str,
) -> Result<String, String> {
    let commands: Vec<Command> = serde_json::from_str(commands_json)
        .map_err(|err| format!("invalid commands JSON: {err}"))?;
    let receipt = host
        .transact(Origin::plugin(plugin_id), label, commands)
        .map_err(|err| err.to_string())?;
    Ok(json!({
        "revision": receipt.revision,
        "outcomes": receipt.outcomes,
    })
    .to_string())
}

/// Look up `key` in the [`SettingsService`] registered on `ctx` and serialize
/// the value to JSON when present.
pub fn setting_get_json(ctx: &PluginContext, key: &str) -> Result<Option<String>, String> {
    let Some(settings) = ctx.try_get::<dyn SettingsService>() else {
        return Ok(None);
    };
    match settings.get(key) {
        Some(val) => serde_json::to_string(&val)
            .map(Some)
            .map_err(|err| err.to_string()),
        None => Ok(None),
    }
}

/// Parse `commands_json` (treating an empty or whitespace-only string as no
/// commands when `allow_empty` is set), apply every command to `tx`, and return
/// an [`InterchangeReport`] with `skipped`.
pub fn apply_import_commands(
    tx: &mut Transaction<'_>,
    commands_json: &str,
    skipped: Vec<String>,
    allow_empty: bool,
) -> Result<InterchangeReport, TransactError> {
    let commands: Vec<Command> = if allow_empty && commands_json.trim().is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(commands_json)
            .map_err(|err| TransactError::Aborted(format!("invalid import commands JSON: {err}")))?
    };
    for command in commands {
        tx.apply(command)?;
    }
    Ok(InterchangeReport { skipped })
}

/// Build the scalar [`EffectParamSpec`] for an external audio effect parameter.
pub fn audio_param_spec(
    name: String,
    label: String,
    default: f32,
    min: f32,
    max: f32,
) -> EffectParamSpec {
    EffectParamSpec {
        name,
        label,
        kind: EffectParamKind::Scalar,
        default,
        min,
        max,
        default_color: [0; 4],
        default_vec2: [0.0; 2],
        ..Default::default()
    }
}

/// Return `(sample_rate, channels)` for `config`, falling back to
/// `EXPORT_AUDIO_RATE` and `EXPORT_AUDIO_CHANNELS` when `config.audio` is
/// `None`.
pub fn encoder_audio_rate_and_channels(config: &EncoderConfig) -> (u32, u32) {
    config.audio.map_or(
        (EXPORT_AUDIO_RATE, u32::from(EXPORT_AUDIO_CHANNELS)),
        |audio| (audio.sample_rate, u32::from(audio.channels)),
    )
}

/// Parse `preset_json` and register the preset on `ctx`.
pub fn register_preset_json(ctx: &PluginContext, preset_json: &str) -> Result<(), String> {
    let preset: Preset =
        serde_json::from_str(preset_json).map_err(|err| format!("invalid preset JSON: {err}"))?;
    ctx.register_preset(preset)
        .map(drop)
        .map_err(|err| err.to_string())
}

/// Register the theme `id` from `toml` on `ctx`.
pub fn register_theme_toml(ctx: &PluginContext, id: &str, toml: &str) -> Result<(), String> {
    ctx.register_theme(id, toml)
        .map(drop)
        .map_err(|err| err.to_string())
}

/// Validate `pixels` as a `width` by `height` RGBA8 image and register it as
/// texture `id` on `ctx`.
pub fn register_rgba_texture(
    ctx: &PluginContext,
    id: impl Into<String>,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> Result<(), String> {
    let id = id.into();
    let image = RgbaImage::try_new(width, height, pixels)
        .map_err(|err| format!("texture `{id}`: {err}"))?;
    ctx.register_texture(id, &image)
        .map(drop)
        .map_err(|err| err.to_string())
}

/// Build a [`ClipTypeIcon`] from an optional SVG string.
pub fn clip_type_icon(icon_svg: Option<String>) -> Option<ClipTypeIcon> {
    icon_svg.map(|svg| ClipTypeIcon {
        svg: Arc::from(svg),
        key: 0,
    })
}

/// Reject an empty or whitespace-only driver expression source string.
pub fn validate_driver_source(source: &str) -> Result<(), String> {
    if source.trim().is_empty() {
        return Err("driver expression is empty".to_owned());
    }
    Ok(())
}

/// Parse the JSON returned by an analyzer's `finish` callback.
pub fn parse_analyzer_result(json: &str) -> Result<AnalyzerResult, String> {
    serde_json::from_str(json).map_err(|err| format!("invalid analyzer result JSON: {err}"))
}

/// Copy every row of `input` into `out` as packed `u16` RGBA channels.
pub fn pack_cpu_frame_in(input: CpuFrameIn<'_>, out: &mut Vec<u16>) {
    let width = input.width();
    let height = input.height();
    let expected_len = (height as usize) * (width as usize) * 4;
    out.clear();
    out.reserve(expected_len);
    for y in 0..height {
        out.extend_from_slice(input.row(y));
    }
}

/// Copy packed `u16` RGBA channels from `pixels` into the rows of `output`.
pub fn unpack_cpu_frame_out(pixels: &[u16], output: &mut CpuFrameOut<'_>) {
    let row_elems = (output.width() as usize) * 4;
    if row_elems == 0 {
        return;
    }
    for (y, row) in pixels.chunks_exact(row_elems).enumerate() {
        output.row_mut(y as u32).copy_from_slice(row);
    }
}

/// Strip control characters from `reason` and cap at [`MAX_WIDGET_ID_BYTES`]
/// bytes on a character boundary, returning `fallback` when nothing printable
/// remains.
pub fn sanitize_fault_widget_id(reason: &str, fallback: &str) -> Arc<str> {
    let mut sanitized = String::with_capacity(reason.len().min(MAX_WIDGET_ID_BYTES));
    for ch in reason.chars().filter(|c| !c.is_control()) {
        if sanitized.len() + ch.len_utf8() > MAX_WIDGET_ID_BYTES {
            break;
        }
        sanitized.push(ch);
    }
    if sanitized.trim().is_empty() {
        Arc::from(fallback)
    } else {
        Arc::from(sanitized)
    }
}

/// Check whether `raw_path` is an absolute path without `..` covered by
/// `package.manifest.permissions.fs` (including [`FsScope::Project`] when the
/// open project on `host` has a file path).
pub fn check_project_fs_path(
    package: &Package,
    host: &ProjectHost,
    raw_path: &Path,
    mode: &str,
) -> Result<PathBuf, String> {
    if raw_path.as_os_str().is_empty() {
        return Err(format!(
            "invalid project path: path cannot be empty; pass an absolute .aria file path for {mode}"
        ));
    }
    if !raw_path.is_absolute()
        || raw_path
            .components()
            .any(|comp| matches!(comp, Component::ParentDir))
    {
        return Err(format!(
            "invalid project path {:?}: must be an absolute path without `..`",
            raw_path
        ));
    }
    let project_dir = host
        .project_path()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    for scope in &package.manifest.permissions.fs {
        match scope {
            FsScope::Path(prefix) if raw_path.starts_with(prefix) => {
                return Ok(raw_path.to_path_buf());
            }
            FsScope::Project => {
                if let Some(dir) = &project_dir
                    && raw_path.starts_with(dir)
                {
                    return Ok(raw_path.to_path_buf());
                }
            }
            _ => {}
        }
    }
    Err(format!(
        "permission denied: {}: fs {mode} for {:?} is not granted by permissions.fs",
        package.manifest.id, raw_path
    ))
}

/// Render `request` at `time` from `host` using the [`RenderHost`] registered
/// on `ctx`.
pub fn render_frame(
    ctx: &PluginContext,
    host: &ProjectHost,
    time: RationalTime,
    request: &FrameRequest,
) -> Result<RgbaImage, String> {
    if time.rate.num <= 0 || time.rate.den <= 0 {
        return Err(format!(
            "invalid frame time rate {}/{}: rate numerator and denominator must be positive",
            time.rate.num, time.rate.den
        ));
    }
    match request.size {
        FrameSize::Canvas => {}
        FrameSize::Fit {
            max_width,
            max_height,
        } => {
            if max_width == 0 || max_height == 0 {
                return Err(format!(
                    "invalid frame fit size {max_width}x{max_height}: width and height must be at least 1"
                ));
            }
            if max_width > MAX_RENDER_FRAME_DIMENSION || max_height > MAX_RENDER_FRAME_DIMENSION {
                return Err(format!(
                    "frame fit size {max_width}x{max_height} exceeds maximum dimension {MAX_RENDER_FRAME_DIMENSION}"
                ));
            }
        }
        FrameSize::Exact { width, height } => {
            if width == 0 || height == 0 {
                return Err(format!(
                    "invalid exact frame size {width}x{height}: width and height must be at least 1"
                ));
            }
            if width > MAX_RENDER_FRAME_DIMENSION || height > MAX_RENDER_FRAME_DIMENSION {
                return Err(format!(
                    "exact frame size {width}x{height} exceeds maximum dimension {MAX_RENDER_FRAME_DIMENSION}"
                ));
            }
        }
    }
    let render_host = ctx.try_get::<RenderHost>().ok_or_else(|| {
        "RenderHost service is not registered; enable a GPU device or register RenderHost on PluginContext".to_owned()
    })?;
    let snapshot = host.snapshot();
    render_host
        .frame(&snapshot, time, request)
        .map_err(|err| err.to_string())
}

/// Move the playhead on `ctx`'s [`EditorView`] to `time`, clamping to the open
/// sequence duration on `host` when the sequence is non-empty.
pub fn editor_seek(
    ctx: &PluginContext,
    host: &ProjectHost,
    time: RationalTime,
) -> Result<RationalTime, String> {
    if time.rate.num <= 0 || time.rate.den <= 0 {
        return Err(format!(
            "invalid seek time rate {}/{}: rate numerator and denominator must be positive",
            time.rate.num, time.rate.den
        ));
    }
    if time.value < 0 {
        return Err(format!(
            "invalid negative seek time {} ticks: pass a non-negative timestamp",
            time.value
        ));
    }
    let view = ctx
        .try_get::<EditorView>()
        .ok_or_else(|| "EditorView service is not registered".to_owned())?;
    let snapshot = host.snapshot();
    let timeline = snapshot.timeline();
    let rescaled = rescale_time(time, timeline.frame_rate);
    let duration = rescale_time(timeline.duration(), timeline.frame_rate).value;
    let target = if duration > 0 {
        RationalTime::new(rescaled.value.clamp(0, duration), timeline.frame_rate)
    } else {
        rescaled
    };
    view.seek(target, ctx.events())
}

/// Parse `tick_json` as a [`RationalTime`] and call [`editor_seek`].
pub fn editor_seek_json(
    ctx: &PluginContext,
    host: &ProjectHost,
    tick_json: &str,
) -> Result<RationalTime, String> {
    let tick: RationalTime =
        serde_json::from_str(tick_json).map_err(|err| format!("invalid tick JSON: {err}"))?;
    editor_seek(ctx, host, tick)
}

/// Start playback on `ctx`'s [`EditorView`] at signed integer multiplier `speed`.
pub fn editor_play(ctx: &PluginContext, speed: i32) -> Result<(), String> {
    let view = ctx
        .try_get::<EditorView>()
        .ok_or_else(|| "EditorView service is not registered".to_owned())?;
    view.play(speed, ctx.events())
}

/// Stop playback on `ctx`'s [`EditorView`].
pub fn editor_stop(ctx: &PluginContext) -> Result<(), String> {
    let view = ctx
        .try_get::<EditorView>()
        .ok_or_else(|| "EditorView service is not registered".to_owned())?;
    view.stop(ctx.events())
}

/// Open the project at `path` on `host` under `Origin::plugin(plugin_id)`.
pub fn project_open(host: &ProjectHost, plugin_id: &str, path: PathBuf) -> Result<(), String> {
    host.open(&Origin::plugin(plugin_id), path)
        .map_err(|err| err.to_string())
}

/// Save the open project on `host` under `Origin::plugin(plugin_id)`.
pub fn project_save(host: &ProjectHost, plugin_id: &str) -> Result<PathBuf, String> {
    host.save(&Origin::plugin(plugin_id))
        .map_err(|err| err.to_string())
}

/// Save the open project on `host` to `path` under `Origin::plugin(plugin_id)`.
pub fn project_save_as(
    host: &ProjectHost,
    plugin_id: &str,
    path: PathBuf,
) -> Result<PathBuf, String> {
    host.save_as(&Origin::plugin(plugin_id), path)
        .map_err(|err| err.to_string())
}

/// A project session change initiated outside [`Origin::Editor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionChange {
    /// A project was opened at `path`.
    Opened { path: Option<PathBuf> },
    /// The open project was saved to `path`.
    Saved { path: PathBuf },
}

/// Drain queued [`SessionChange`] items from `host`.
pub fn take_session_changes(host: &ProjectHost) -> Vec<SessionChange> {
    host.take_session_changes()
}

pub(crate) fn record_session_outcome(
    changes: &mut Vec<SessionChange>,
    origin: &Origin,
    outcome: &aria_engine::ApplyOutcome,
    engine: &aria_engine::Engine,
) {
    if origin == &Origin::Editor {
        return;
    }
    match outcome {
        aria_engine::ApplyOutcome::Opened
        | aria_engine::ApplyOutcome::Loaded
        | aria_engine::ApplyOutcome::AppliedTemplate => {
            changes.push(SessionChange::Opened {
                path: engine.project_path().cloned(),
            });
        }
        aria_engine::ApplyOutcome::Saved => {
            if let Some(path) = engine.project_path().cloned() {
                changes.push(SessionChange::Saved { path });
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use aria_core::{AudioEncoderConfig, ColorSpace, Rational};
    use aria_engine::{Engine, EngineConfig};
    use parking_lot::Mutex;

    use super::*;
    use crate::{Plugin, PluginError, PluginManifest};

    struct DummyPlugin {
        manifest: PluginManifest,
        dropped: Arc<AtomicUsize>,
    }

    impl Plugin for DummyPlugin {
        fn manifest(&self) -> &PluginManifest {
            &self.manifest
        }

        fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
            let dropped = Arc::clone(&self.dropped);
            on_unmount(ctx, move || {
                dropped.fetch_add(1, Ordering::SeqCst);
            });
            Ok(())
        }
    }

    #[test]
    fn on_unmount_runs_once_when_plugin_unmounts_without_adding_service_entry() {
        let ctx = PluginContext::new();
        let dropped = Arc::new(AtomicUsize::new(0));
        let plugin = DummyPlugin {
            manifest: PluginManifest::new("test.unmount", "Unmount Test", "1.0.0", &[], &[]),
            dropped: Arc::clone(&dropped),
        };
        ctx.mount(&plugin).expect("mount");
        let mounted = ctx.mounted();
        assert_eq!(mounted.len(), 1);
        assert!(mounted[0].services.is_empty());
        assert_eq!(dropped.load(Ordering::SeqCst), 0);

        ctx.unmount("test.unmount").expect("unmount");
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn project_and_editor_view_helpers_round_trip_snapshot_view_and_transactions() {
        let ctx = PluginContext::new();
        let engine = Arc::new(Mutex::new(
            Engine::new(EngineConfig::default()).expect("engine"),
        ));
        let host = Arc::new(ProjectHost::new(engine, &ctx));

        let snap = project_snapshot_json(&host).expect("snapshot json");
        assert!(snap.contains("\"timeline\""), "{snap}");

        let none_native = editor_view_json(None, Some(RationalTime::new(0, Rational::FPS_24)), 1);
        let none_wasm = editor_view_json(None, None, 0);
        assert!(none_native.contains("\"speed\":1"), "{none_native}");
        assert!(none_wasm.contains("\"playhead\":null"), "{none_wasm}");
        assert!(none_wasm.contains("\"speed\":0"), "{none_wasm}");

        assert!(selected_clips_for_view(&ctx, &host).is_empty());

        let receipt = project_transact_json(&host, "test.plugin", "Noop".into(), "[]")
            .expect("empty transaction");
        assert!(receipt.contains("\"revision\":"), "{receipt}");
        assert!(receipt.contains("\"outcomes\":[]"), "{receipt}");

        let bad = project_transact_json(&host, "test.plugin", "Bad".into(), "not-json")
            .expect_err("invalid commands json must fail");
        assert!(bad.contains("invalid commands JSON"), "{bad}");
    }

    #[test]
    fn import_audio_encoder_and_contribution_helpers_validate_and_convert() {
        let param = audio_param_spec("gain".into(), "Gain".into(), 1.0, 0.0, 2.0);
        assert_eq!(param.name, "gain");
        assert_eq!(param.kind, EffectParamKind::Scalar);
        assert_eq!((param.default, param.min, param.max), (1.0, 0.0, 2.0));

        let mut cfg = EncoderConfig::new((1920, 1080), Rational::FPS_24, ColorSpace::default());
        assert_eq!(
            encoder_audio_rate_and_channels(&cfg),
            (EXPORT_AUDIO_RATE, u32::from(EXPORT_AUDIO_CHANNELS))
        );
        cfg.audio = Some(AudioEncoderConfig {
            sample_rate: 44_100,
            channels: 1,
        });
        assert_eq!(encoder_audio_rate_and_channels(&cfg), (44_100, 1));

        assert_eq!(
            validate_driver_source(" \t\n "),
            Err("driver expression is empty".to_owned())
        );
        assert_eq!(validate_driver_source("value + 1"), Ok(()));
        assert!(
            parse_analyzer_result("{bad")
                .unwrap_err()
                .starts_with("invalid analyzer result JSON:")
        );
        assert_eq!(
            parse_analyzer_result(r#"{"beats":[10,20]}"#).unwrap(),
            AnalyzerResult {
                beats: vec![10, 20],
                ..Default::default()
            }
        );
        assert!(clip_type_icon(None).is_none());
        assert_eq!(
            clip_type_icon(Some("<svg/>".into()))
                .expect("icon")
                .svg
                .as_ref(),
            "<svg/>"
        );

        let sanitized = sanitize_fault_widget_id("\0\n  ", "fallback-id");
        assert_eq!(sanitized.as_ref(), "fallback-id");
        let long_reason = "x".repeat(MAX_WIDGET_ID_BYTES + 32);
        let capped = sanitize_fault_widget_id(&long_reason, "fallback-id");
        assert_eq!(capped.len(), MAX_WIDGET_ID_BYTES);
    }
}
