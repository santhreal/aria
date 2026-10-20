//! Shared host operations for external plugins (`aria-plugin` native C ABI and
//! `aria-plugin-wasm` WIT components).
//!
//! Each host decodes its guest ABI representation and calls the functions in
//! this module for validation, state transitions, and service operations that
//! behave identically across both runtimes.

use std::sync::Arc;

use aria_commands::Command;
use aria_compositor::{CpuFrameIn, CpuFrameOut, RgbaImage};
use aria_core::{EncoderConfig, RationalTime};
use aria_models::{ClipTypeIcon, EffectParamKind, EffectParamSpec, Preset};
use aria_render::{EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE};
use serde_json::json;

use crate::{
    AnalyzerResult, ClipRef, EditorView, Event, InterchangeReport, MAX_WIDGET_ID_BYTES, Origin,
    PluginContext, ProjectHost, SettingsService, TransactError, Transaction, ViewState,
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

        assert!(validate_driver_source(" \t\n ").is_err());
        assert!(validate_driver_source("value + 1").is_ok());
        assert!(parse_analyzer_result("{bad").is_err());
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
