//! Reference WebAssembly component guest plugin for Aria (`world aria:plugin/plugin@1.0.0`).

use std::sync::atomic::AtomicU64;

wit_bindgen::generate!({
    world: "plugin",
    path: "../../crates/aria-plugin-wasm/wit",
});

mod audio;
mod commands;
mod decoder;
mod encoder;
mod jobs;
mod mount;
mod panel;
mod tools;
mod y4m;

const DEFAULT_MARKER_NAME: &str = "wasm-marker-v1";

static NEXT_INSTANCE: AtomicU64 = AtomicU64::new(1);

struct ExampleWasmPlugin;

impl Guest for ExampleWasmPlugin {
    fn mount() -> Result<(), String> {
        mount::mount()
    }

    fn unmount() -> Result<(), String> {
        mount::unmount()
    }

    fn command_run(id: String, args_json: String) -> Result<(), String> {
        commands::command_run(id, args_json)
    }

    fn event(kind: String, event_json: String) -> Result<Option<String>, String> {
        commands::event(kind, event_json)
    }

    fn import(importer_id: String, path: String) -> Result<ImportResult, String> {
        commands::import(importer_id, path)
    }

    fn export(exporter_id: String, project_json: String, path: String) -> Result<Report, String> {
        commands::export(exporter_id, project_json, path)
    }

    fn audio_create(effect_id: String, sample_rate: u32) -> Result<Instance, String> {
        audio::audio_create(effect_id, sample_rate)
    }

    fn audio_process(
        instance: Instance,
        params: Vec<AudioParam>,
        block: AudioBlock,
    ) -> Result<StereoBuffers, String> {
        audio::audio_process(instance, params, block)
    }

    fn audio_reset(instance: Instance) -> Result<(), String> {
        audio::audio_reset(instance)
    }

    fn audio_drop(instance: Instance) -> Result<(), String> {
        audio::audio_drop(instance)
    }

    fn panel_view(id: String) -> Vec<UiNode> {
        panel::panel_view(id)
    }

    fn panel_event(id: String, event: PanelEvent) -> Result<(), String> {
        panel::panel_event(id, event)
    }

    fn overlay_scene(id: String) -> Vec<OverlayShape> {
        panel::overlay_scene(id)
    }

    fn overlay_event(id: String, event: OverlayEvent) -> Result<(), String> {
        panel::overlay_event(id, event)
    }

    fn encoder_open(
        encoder_id: String,
        path: String,
        config: EncoderConfig,
    ) -> Result<Instance, String> {
        encoder::encoder_open(encoder_id, path, config)
    }

    fn encoder_push_frame(instance: Instance, frame: EncoderFrame) -> Result<(), String> {
        encoder::encoder_push_frame(instance, frame)
    }

    fn encoder_push_audio(instance: Instance, audio: EncoderAudio) -> Result<(), String> {
        encoder::encoder_push_audio(instance, audio)
    }

    fn encoder_finish(instance: Instance) -> Result<(), String> {
        encoder::encoder_finish(instance)
    }

    fn encoder_drop(instance: Instance) -> Result<(), String> {
        encoder::encoder_drop(instance)
    }

    fn job_step(job: u64) -> Result<JobStep, String> {
        jobs::job_step(job)
    }

    fn job_end(job: u64, cancelled: bool) {
        jobs::job_end(job, cancelled)
    }

    fn decoder_probe(decoder_id: String, path: String) -> Result<MediaProbe, DecodeError> {
        decoder::decoder_probe(decoder_id, path)
    }

    fn decoder_open_video(decoder_id: String, path: String) -> Result<OpenedVideo, DecodeError> {
        decoder::decoder_open_video(decoder_id, path)
    }

    fn decoder_seek(instance: Instance, pts: i64) -> Result<(), String> {
        decoder::decoder_seek(instance, pts)
    }

    fn decoder_next_frame(instance: Instance) -> Result<Option<DecodedFrame>, String> {
        decoder::decoder_next_frame(instance)
    }

    fn decoder_open_audio(
        decoder_id: String,
        path: String,
        sample_rate: u32,
        channels: u16,
    ) -> Result<Instance, DecodeError> {
        decoder::decoder_open_audio(decoder_id, path, sample_rate, channels)
    }

    fn decoder_read_audio(instance: Instance, max_samples: u32) -> Result<Vec<f32>, String> {
        decoder::decoder_read_audio(instance, max_samples)
    }

    fn decoder_seek_audio(instance: Instance, frame: i64) -> Result<(), String> {
        decoder::decoder_seek_audio(instance, frame)
    }

    fn decoder_close(instance: Instance) {
        decoder::decoder_close(instance)
    }

    fn ai_tool_call(name: String, arguments_json: String) -> Result<AiToolOutput, String> {
        tools::ai_tool_call(name, arguments_json)
    }

    fn scope_measure(id: String, frame: ScopeFrame) -> Result<ScopeDrawing, String> {
        tools::scope_measure(id, frame)
    }

    fn cpu_effect_process(
        id: String,
        params: Vec<EffectParam>,
        frame: VideoFrame,
    ) -> Result<VideoFrame, String> {
        tools::cpu_effect_process(id, params, frame)
    }

    fn driver_evaluate(id: String, expr: String, vars: DriverVars) -> Result<ParamValue, String> {
        tools::driver_evaluate(id, expr, vars)
    }

    fn analyzer_open(id: String, sample_rate: u32, channels: u16) -> Result<Instance, String> {
        audio::analyzer_open(id, sample_rate, channels)
    }

    fn analyzer_push(instance: Instance, samples: Vec<f32>) -> Result<(), String> {
        audio::analyzer_push(instance, samples)
    }

    fn analyzer_finish(instance: Instance) -> Result<String, String> {
        audio::analyzer_finish(instance)
    }

    fn analyzer_drop(instance: Instance) {
        audio::analyzer_drop(instance)
    }
}

export!(ExampleWasmPlugin);
