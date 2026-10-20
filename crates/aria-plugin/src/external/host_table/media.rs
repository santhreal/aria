use std::ffi::c_void;
use std::io::Write;
use std::panic::resume_unwind;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aria_plugin_abi::{
    AriaAudioBlock, AriaAudioParam, AriaEncoderAudio, AriaEncoderConfig, AriaEncoderFrame,
    AriaResult, AriaStr,
};

use super::specs::{
    RawAudioEffectSpec, RawEncoderSpec, RawExportReport, RawExporterSpec, RawImportResult,
    RawImporterSpec,
};
use super::{call_ctx, read_str};
use crate::external::{ExportCallError, NativePluginState, decoders};
use crate::{
    AudioBlock, AudioEffectSpec, AudioProcessor, AudioProcessorFactory, Command,
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, EffectParamKind, EffectParamSpec, EncodeError,
    Encoder, EncoderConfig, EncoderFrame, EncoderSpec, Exporter, ExporterSpec, Importer,
    ImporterSpec, InterchangeReport, ParamSignal, PluginContext, Project, RationalTime,
    TransactError, Transaction, VideoEncoder, VideoFrame, encoder_pts,
};

pub(super) extern "C" fn host_register_importer(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_importer`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let raw_json = read_str(spec_json, "importer spec")?;
            let raw: RawImporterSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid importer spec JSON: {err}"))?;
            let importer = NativeImporter {
                importer_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_importer(
                    ImporterSpec {
                        id: raw.id,
                        name: raw.name,
                        extensions: raw.extensions,
                    },
                    importer,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_exporter(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_exporter`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let raw_json = read_str(spec_json, "exporter spec")?;
            let raw: RawExporterSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid exporter spec JSON: {err}"))?;
            let exporter = NativeExporter {
                exporter_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_exporter(
                    ExporterSpec {
                        id: raw.id,
                        name: raw.name,
                        extension: raw.extension,
                    },
                    exporter,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_audio_effect(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_audio_effect`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            let raw_json = read_str(spec_json, "audio effect spec")?;
            let raw: RawAudioEffectSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid audio effect spec JSON: {err}"))?;
            let param_count = raw.params.len();
            let params = raw
                .params
                .into_iter()
                .map(|p| EffectParamSpec {
                    name: p.name,
                    label: p.label,
                    kind: EffectParamKind::Scalar,
                    default: p.default_value,
                    min: p.min,
                    max: p.max,
                    default_color: [0; 4],
                    default_vec2: [0.0; 2],
                    ..Default::default()
                })
                .collect();
            let spec = AudioEffectSpec {
                id: raw.id.clone(),
                category: raw.category,
                label: raw.label,
                params,
                latency: raw.latency,
                sidechain: raw.sidechain,
            };
            let factory = NativeAudioFactory {
                effect_id: raw.id,
                param_count,
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_audio_effect(spec, factory)
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_encoder(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_encoder`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_encoders() {
                return Err(
                    "plugin table does not include encoder callbacks (encoder_open..encoder_drop)"
                        .to_owned(),
                );
            }
            let raw_json = read_str(spec_json, "encoder spec")?;
            let raw: RawEncoderSpec = serde_json::from_str(raw_json)
                .map_err(|err| format!("invalid encoder spec JSON: {err}"))?;
            let encoder = NativeEncoder {
                encoder_id: raw.id.clone(),
                state: Arc::clone(ctx.state),
                scoped_ctx: ctx.scoped_ctx.clone(),
            };
            ctx.scoped_ctx
                .register_encoder(
                    EncoderSpec {
                        id: raw.id,
                        name: raw.name,
                        extension: raw.extension,
                        audio: raw.audio,
                    },
                    encoder,
                )
                .map(drop)
                .map_err(|err| err.to_string())
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

pub(super) extern "C" fn host_register_decoder(
    host_ctx: *mut c_void,
    spec_json: AriaStr,
) -> AriaResult {
    // SAFETY: `host_ctx` and `spec_json` are valid for the duration of `host_register_decoder`.
    let result = unsafe {
        call_ctx(host_ctx).and_then(|ctx| {
            if !ctx.in_mount {
                return Err("registration calls are permitted only during mount".to_owned());
            }
            if !ctx.state.table.has_decoders() {
                return Err(
                    "plugin table does not include decoder callbacks (decoder_probe..decoder_close)"
                        .to_owned(),
                );
            }
            decoders::register(ctx, read_str(spec_json, "decoder spec")?)
        })
    };
    match result {
        Ok(()) => AriaResult::ok_empty(),
        Err(err) => AriaResult::err(err),
    }
}

struct NativeImporter {
    importer_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl Importer for NativeImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let path_str = file.to_string_lossy();
        let raw_json = self
            .state
            .call_import(&self.scoped_ctx, &self.importer_id, &path_str)?;
        let parsed: RawImportResult = serde_json::from_str(&raw_json)
            .map_err(|err| TransactError::Aborted(format!("invalid import result JSON: {err}")))?;
        let commands: Vec<Command> = if parsed.commands_json.trim().is_empty() {
            Vec::new()
        } else {
            serde_json::from_str(&parsed.commands_json).map_err(|err| {
                TransactError::Aborted(format!("invalid import commands JSON: {err}"))
            })?
        };
        for command in commands {
            tx.apply(command)?;
        }
        Ok(InterchangeReport {
            skipped: parsed.skipped,
        })
    }
}

struct NativeExporter {
    exporter_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

static EXPORT_STAGE_SEQ: AtomicU64 = AtomicU64::new(1);

impl Exporter for NativeExporter {
    fn export(&self, project: &Project, out: &mut dyn Write) -> std::io::Result<InterchangeReport> {
        let project_json = serde_json::to_string(project).map_err(std::io::Error::other)?;
        let dir = self.state.resolve_storage_dir(&self.scoped_ctx);
        std::fs::create_dir_all(&dir)?;
        let seq = EXPORT_STAGE_SEQ.fetch_add(1, Ordering::Relaxed);
        let stage_path = dir.join(format!(".export-{}-{seq}.stage", self.exporter_id));
        let stage_str = stage_path.to_string_lossy().into_owned();
        let outcome = self.state.call_export(
            &self.scoped_ctx,
            &self.exporter_id,
            &project_json,
            &stage_str,
        );
        match outcome {
            Ok(report_json) => {
                let bytes = match std::fs::read(&stage_path) {
                    Ok(bytes) => bytes,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
                    Err(err) => {
                        let _ = std::fs::remove_file(&stage_path);
                        return Err(err);
                    }
                };
                let _ = std::fs::remove_file(&stage_path);
                out.write_all(&bytes)?;
                let parsed: RawExportReport = if report_json.trim().is_empty() {
                    RawExportReport {
                        skipped: Vec::new(),
                    }
                } else {
                    serde_json::from_str(&report_json).map_err(std::io::Error::other)?
                };
                Ok(InterchangeReport {
                    skipped: parsed.skipped,
                })
            }
            Err(ExportCallError::Failed(msg)) => {
                let _ = std::fs::remove_file(&stage_path);
                Err(std::io::Error::other(msg))
            }
            Err(ExportCallError::Panicked(msg)) => {
                let _ = std::fs::remove_file(&stage_path);
                resume_unwind(Box::new(msg));
            }
        }
    }
}

struct NativeAudioFactory {
    effect_id: String,
    param_count: usize,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl AudioProcessorFactory for NativeAudioFactory {
    fn create(&self, sample_rate: u32) -> Box<dyn AudioProcessor> {
        match self
            .state
            .call_audio_create(&self.scoped_ctx, &self.effect_id, sample_rate)
        {
            Ok(instance) => Box::new(NativeAudioProcessor {
                effect_id: self.effect_id.clone(),
                instance,
                param_count: self.param_count,
                state: Arc::clone(&self.state),
                scoped_ctx: self.scoped_ctx.clone(),
            }),
            Err(msg) => resume_unwind(Box::new(msg)),
        }
    }
}

struct NativeAudioProcessor {
    effect_id: String,
    instance: u64,
    param_count: usize,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl AudioProcessor for NativeAudioProcessor {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let mut scratch_params = Vec::with_capacity(self.param_count);
        for slot in 0..self.param_count {
            let param = match block.param(slot) {
                Some(ParamSignal::Constant(val)) => AriaAudioParam {
                    constant: val,
                    per_frame: std::ptr::null(),
                    per_frame_len: 0,
                },
                Some(ParamSignal::PerFrame(values)) => AriaAudioParam {
                    constant: 0.0,
                    per_frame: values.as_ptr(),
                    per_frame_len: values.len(),
                },
                None => AriaAudioParam {
                    constant: 0.0,
                    per_frame: std::ptr::null(),
                    per_frame_len: 0,
                },
            };
            scratch_params.push(param);
        }
        let frames = block.frames();
        let position = block.position();
        let sample_rate = block.sample_rate();
        let (has_sidechain, sidechain_left, sidechain_right) = match block.sidechain() {
            Some((left, right)) => (1, left.as_ptr(), right.as_ptr()),
            None => (0, std::ptr::null(), std::ptr::null()),
        };
        let (left, right) = block.channels();
        let mut raw_block = AriaAudioBlock {
            left: left.as_mut_ptr(),
            right: right.as_mut_ptr(),
            frames,
            position,
            sample_rate,
            has_sidechain,
            sidechain_left,
            sidechain_right,
        };
        if let Err(msg) = self.state.call_audio_process(
            &self.scoped_ctx,
            &self.effect_id,
            self.instance,
            &scratch_params,
            &mut raw_block,
        ) {
            resume_unwind(Box::new(msg));
        }
    }

    fn reset(&mut self) {
        if let Err(msg) =
            self.state
                .call_audio_reset(&self.scoped_ctx, &self.effect_id, self.instance)
        {
            resume_unwind(Box::new(msg));
        }
    }
}

impl Drop for NativeAudioProcessor {
    fn drop(&mut self) {
        self.state.call_audio_drop(&self.scoped_ctx, self.instance);
    }
}

struct NativeEncoder {
    encoder_id: String,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl Encoder for NativeEncoder {
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let path_str = path.to_string_lossy();
        let (width, height) = config.size;
        let (audio_sample_rate, audio_channels) = config.audio.map_or(
            (EXPORT_AUDIO_RATE, u32::from(EXPORT_AUDIO_CHANNELS)),
            |audio| (audio.sample_rate, u32::from(audio.channels)),
        );
        let raw_cfg = AriaEncoderConfig {
            width,
            height,
            fps_num: i64::from(config.frame_rate.num),
            fps_den: config.frame_rate.den,
            video_bitrate_kbps: config.bitrate_kbps.unwrap_or(0),
            audio_bitrate_kbps: 0,
        };
        let instance = self
            .state
            .call_encoder_open(&self.scoped_ctx, &self.encoder_id, &path_str, &raw_cfg)
            .map_err(EncodeError::Start)?;
        Ok(Box::new(NativeEncoderSession {
            encoder_id: self.encoder_id.clone(),
            instance,
            finished: false,
            audio_sample_rate,
            audio_channels,
            state: Arc::clone(&self.state),
            scoped_ctx: self.scoped_ctx.clone(),
        }))
    }
}

struct NativeEncoderSession {
    encoder_id: String,
    instance: u64,
    finished: bool,
    audio_sample_rate: u32,
    audio_channels: u32,
    state: Arc<NativePluginState>,
    scoped_ctx: PluginContext,
}

impl VideoEncoder for NativeEncoderSession {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let frame = EncoderFrame::new(frame)?;
        let raw = AriaEncoderFrame {
            rgba: frame.rgba.as_ptr(),
            rgba_len: frame.rgba.len(),
            width: frame.width,
            height: frame.height,
            stride: frame.stride,
            pts_value: frame.pts.value,
            pts_rate: frame.pts.rate.num,
        };
        self.state
            .call_encoder_push_frame(&self.scoped_ctx, self.instance, &raw)
            .map_err(EncodeError::Encode)
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let pts = encoder_pts(pts)?;
        let raw = AriaEncoderAudio {
            samples: samples.as_ptr(),
            samples_len: samples.len(),
            sample_rate: self.audio_sample_rate,
            channels: self.audio_channels,
            pts_value: pts.value,
            pts_rate: pts.rate.num,
        };
        self.state
            .call_encoder_push_audio(&self.scoped_ctx, self.instance, &raw)
            .map_err(EncodeError::Encode)
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        self.finished = true;
        self.state
            .call_encoder_finish(&self.scoped_ctx, self.instance)
            .map_err(EncodeError::Encode)
    }

    fn choice(&self) -> aria_core::EncoderChoice {
        aria_core::EncoderChoice::only(self.encoder_id.clone(), None)
    }
}

impl Drop for NativeEncoderSession {
    fn drop(&mut self) {
        if !self.finished {
            self.state
                .call_encoder_drop(&self.scoped_ctx, self.instance);
        }
    }
}
