wit_bindgen::generate!({
    world: "plugin",
    path: "wit",
});

const COMMAND_ID: &str = "__COMMAND_ID__";
const COMMAND_TITLE: &str = "__COMMAND_TITLE__";

struct SamplePlugin;

impl Guest for SamplePlugin {
    fn mount() -> Result<(), String> {
        log(LogLevel::Info, "mounted __PLUGIN_ID__");
        register_command(COMMAND_ID, COMMAND_TITLE)?;
        Ok(())
    }

    fn unmount() -> Result<(), String> {
        Ok(())
    }

    fn command_run(id: String, _args_json: String) -> Result<(), String> {
        match id.as_str() {
            COMMAND_ID => {
                log(LogLevel::Info, "ran __COMMAND_ID__");
                storage_set("last_run", COMMAND_ID.as_bytes())?;
                Ok(())
            }
            other => Err(format!("unknown command: {other}")),
        }
    }

    fn event(_kind: String, _event_json: String) -> Result<Option<String>, String> {
        Ok(None)
    }

    fn import(importer_id: String, _path: String) -> Result<ImportResult, String> {
        Err(format!("unknown importer: {importer_id}"))
    }

    fn export(
        exporter_id: String,
        _project_json: String,
        _path: String,
    ) -> Result<Report, String> {
        Err(format!("unknown exporter: {exporter_id}"))
    }

    fn audio_create(effect_id: String, _sample_rate: u32) -> Result<Instance, String> {
        Err(format!("unknown audio effect: {effect_id}"))
    }

    fn audio_process(
        instance: Instance,
        _params: Vec<AudioParam>,
        _block: AudioBlock,
    ) -> Result<StereoBuffers, String> {
        Err(format!("unknown audio instance: {instance}"))
    }

    fn audio_reset(instance: Instance) -> Result<(), String> {
        Err(format!("unknown audio instance: {instance}"))
    }

    fn audio_drop(_instance: Instance) -> Result<(), String> {
        Ok(())
    }

    fn panel_view(_id: String) -> Vec<UiNode> {
        Vec::new()
    }

    fn panel_event(id: String, _event: PanelEvent) -> Result<(), String> {
        Err(format!("unknown panel: {id}"))
    }

    fn encoder_open(
        encoder_id: String,
        _path: String,
        _config: EncoderConfig,
    ) -> Result<Instance, String> {
        Err(format!("unknown encoder: {encoder_id}"))
    }

    fn encoder_push_frame(instance: Instance, _frame: EncoderFrame) -> Result<(), String> {
        Err(format!("unknown encoder instance: {instance}"))
    }

    fn encoder_push_audio(instance: Instance, _audio: EncoderAudio) -> Result<(), String> {
        Err(format!("unknown encoder instance: {instance}"))
    }

    fn encoder_finish(instance: Instance) -> Result<(), String> {
        Err(format!("unknown encoder instance: {instance}"))
    }

    fn encoder_drop(_instance: Instance) -> Result<(), String> {
        Ok(())
    }

    fn overlay_scene(_id: String) -> Vec<OverlayShape> {
        Vec::new()
    }

    fn overlay_event(id: String, _event: OverlayEvent) -> Result<(), String> {
        Err(format!("unknown overlay: {id}"))
    }

    fn job_step(job: u64) -> Result<JobStep, String> {
        Err(format!("unknown job: {job}"))
    }

    fn job_end(_job: u64, _cancelled: bool) {}

    fn decoder_probe(decoder_id: String, _path: String) -> Result<MediaProbe, DecodeError> {
        Err(DecodeError::Unsupported(format!("unknown decoder: {decoder_id}")))
    }

    fn decoder_open_video(decoder_id: String, _path: String) -> Result<OpenedVideo, DecodeError> {
        Err(DecodeError::Unsupported(format!("unknown decoder: {decoder_id}")))
    }

    fn decoder_seek(instance: Instance, _pts: i64) -> Result<(), String> {
        Err(format!("unknown video stream: {instance}"))
    }

    fn decoder_next_frame(instance: Instance) -> Result<Option<DecodedFrame>, String> {
        Err(format!("unknown video stream: {instance}"))
    }

    fn decoder_open_audio(
        decoder_id: String,
        _path: String,
        _sample_rate: u32,
        _channels: u16,
    ) -> Result<Instance, DecodeError> {
        Err(DecodeError::Unsupported(format!("unknown decoder: {decoder_id}")))
    }

    fn decoder_read_audio(instance: Instance, _max_samples: u32) -> Result<Vec<f32>, String> {
        Err(format!("unknown audio stream: {instance}"))
    }

    fn decoder_seek_audio(instance: Instance, _frame: i64) -> Result<(), String> {
        Err(format!("unknown audio stream: {instance}"))
    }

    fn decoder_close(_instance: Instance) {}

    fn ai_tool_call(name: String, _arguments_json: String) -> Result<AiToolOutput, String> {
        Err(format!("unknown AI tool: {name}"))
    }

    fn scope_measure(id: String, _frame: ScopeFrame) -> Result<ScopeDrawing, String> {
        Err(format!("unknown scope: {id}"))
    }

    /// Process a premultiplied RGBA16F (`list<u16>`) video frame.
    fn cpu_effect_process(
        id: String,
        _params: Vec<EffectParam>,
        _frame: VideoFrame,
    ) -> Result<VideoFrame, String> {
        Err(format!("unknown CPU effect: {id}"))
    }

    fn driver_evaluate(
        id: String,
        _expr: String,
        _vars: DriverVars,
    ) -> Result<ParamValue, String> {
        Err(format!("unknown driver language: {id}"))
    }

    fn analyzer_open(id: String, _sample_rate: u32, _channels: u16) -> Result<Instance, String> {
        Err(format!("unknown analyzer: {id}"))
    }

    fn analyzer_push(instance: Instance, _samples: Vec<f32>) -> Result<(), String> {
        Err(format!("unknown analyzer instance: {instance}"))
    }

    fn analyzer_finish(instance: Instance) -> Result<String, String> {
        Err(format!("unknown analyzer instance: {instance}"))
    }

    fn analyzer_drop(_instance: Instance) {}
}

export!(SamplePlugin);
