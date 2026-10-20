use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use aria_models::{AudioEffectSpec, Project};
use aria_plugin::{
    CallError, DecoderSpec, EncoderSpec, Exporter, ExporterSpec, ForeignDecoder, Importer,
    ImporterSpec, InterchangeReport, PluginContext, TransactError, Transaction, host_wire,
};

use super::{
    ActiveExport, ActiveThreadGuard, CallPhase, HostState, InstanceState, LiveInstance,
    WasmPluginShared,
};
use crate::audio::WasmAudioFactory;
use crate::bindings;
use crate::decoders::WasmDecoder;
use crate::encoders::WasmEncoder;
use crate::runtime::classify_trap;

impl WasmPluginShared {
    pub(crate) fn call_import(
        &self,
        ctx: &PluginContext,
        importer_id: &str,
        file: &Path,
    ) -> Result<bindings::ImportResult, String> {
        let path_str = file.to_string_lossy().into_owned();
        self.run_with_instance(ctx, CallPhase::Import, |live, _engine| {
            live.store.data_mut().active_read_path = Some(file.to_path_buf());
            live.plugin
                .current()?
                .call_import(&mut live.store, importer_id, &path_str)
        })
    }

    pub(crate) fn call_export(
        &self,
        ctx: &PluginContext,
        exporter_id: &str,
        project_json: &str,
    ) -> Result<(bindings::Report, Vec<u8>), String> {
        let stage_path = self
            .package
            .dir
            .join(".wasm-export-stage")
            .to_string_lossy()
            .into_owned();
        let mut guard = self.state.lock();
        let live = match &mut *guard {
            InstanceState::Mounted(live) => live,
            InstanceState::Faulted(reason) => {
                return Err(format!("plugin {} has faulted: {reason}", self.manifest.id));
            }
            InstanceState::Unmounted => {
                return Err(format!("plugin {} is not mounted", self.manifest.id));
            }
        };

        self.prepare_call(&mut live.store, CallPhase::Export, ctx);
        live.store.data_mut().active_export = Some(ActiveExport {
            path: stage_path.clone(),
            bytes: Vec::new(),
        });

        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            live.plugin.current().and_then(|p| {
                p.call_export(&mut live.store, exporter_id, project_json, &stage_path)
            })
        };
        let captured = live
            .store
            .data_mut()
            .active_export
            .take()
            .map(|exp| exp.bytes)
            .unwrap_or_default();
        Self::finish_call(&mut live.store);

        match result {
            Ok(Ok(report)) => Ok((report, captured)),
            Ok(Err(guest_err)) => Err(guest_err),
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                ctx.fail(&self.manifest.id, reason.clone());
                Err(reason)
            }
        }
    }

    pub(crate) fn call_audio_create(
        &self,
        ctx: &PluginContext,
        effect_id: &str,
        sample_rate: u32,
    ) -> Result<u64, String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_create(&mut live.store, effect_id, sample_rate)
        })
    }

    pub(crate) fn call_audio_process(
        &self,
        ctx: &PluginContext,
        instance: u64,
        params: &[bindings::AudioParam],
        block: &bindings::AudioBlock,
    ) -> Result<bindings::StereoBuffers, String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_process(&mut live.store, instance, params, block)
        })
    }

    pub(crate) fn call_audio_reset(
        &self,
        ctx: &PluginContext,
        instance: u64,
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_reset(&mut live.store, instance)
        })
    }

    pub(crate) fn call_audio_drop(&self, ctx: &PluginContext, instance: u64) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Audio, |live, _engine| {
            live.plugin
                .current()?
                .call_audio_drop(&mut live.store, instance)
        })
    }

    fn run_encoder_call<T>(
        &self,
        ctx: &PluginContext,
        encode_path: &Path,
        f: impl FnOnce(&mut LiveInstance) -> wasmtime::Result<Result<T, String>>,
    ) -> Result<T, String> {
        let mut guard = self.state.lock();
        let live = match &mut *guard {
            InstanceState::Mounted(live) => live,
            InstanceState::Faulted(reason) => {
                return Err(format!("plugin {} has faulted: {reason}", self.manifest.id));
            }
            InstanceState::Unmounted => {
                return Err(format!("plugin {} is not mounted", self.manifest.id));
            }
        };
        self.prepare_call(&mut live.store, CallPhase::Encode, ctx);
        live.store.data_mut().active_encode_path = Some(encode_path.to_path_buf());
        let result = {
            let _thread_guard = ActiveThreadGuard::enter(&self.active_thread);
            let _tick_guard = self.engine.enter_call();
            f(live)
        };
        Self::finish_call(&mut live.store);
        match result {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(guest_err)) => Err(guest_err),
            Err(trap_err) => {
                let reason = classify_trap(&trap_err);
                *guard = InstanceState::Faulted(reason.clone());
                drop(guard);
                std::panic::resume_unwind(Box::new(reason));
            }
        }
    }

    pub(crate) fn call_encoder_open(
        &self,
        ctx: &PluginContext,
        encoder_id: &str,
        path: &Path,
        config: &bindings::EncoderConfig,
    ) -> Result<u64, String> {
        let path_str = path.to_string_lossy().into_owned();
        self.run_encoder_call(ctx, path, |live| {
            live.plugin.current()?.call_encoder_open(
                &mut live.store,
                encoder_id,
                &path_str,
                *config,
            )
        })
    }

    pub(crate) fn call_encoder_push_frame(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        frame: &bindings::EncoderFrame,
    ) -> Result<(), String> {
        self.run_encoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_encoder_push_frame(&mut live.store, instance, frame)
        })
    }

    pub(crate) fn call_encoder_push_audio(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        audio: &bindings::EncoderAudio,
    ) -> Result<(), String> {
        self.run_encoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_encoder_push_audio(&mut live.store, instance, audio)
        })
    }

    pub(crate) fn call_encoder_finish(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
    ) -> Result<(), String> {
        self.run_encoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_encoder_finish(&mut live.store, instance)
        })
    }

    pub(crate) fn call_encoder_drop(
        &self,
        ctx: &PluginContext,
        instance: u64,
    ) -> Result<(), String> {
        self.run_with_instance(ctx, CallPhase::Encode, |live, _engine| {
            live.plugin
                .current()?
                .call_encoder_drop(&mut live.store, instance)
        })
    }

    fn run_decoder_call<T>(
        &self,
        ctx: &PluginContext,
        path: &Path,
        f: impl FnOnce(&mut LiveInstance) -> wasmtime::Result<T>,
    ) -> Result<T, CallError> {
        if *self.active_thread.lock() == Some(std::thread::current().id()) {
            return Err(CallError::Failed(format!(
                "plugin {} cannot decode from inside one of its own calls",
                self.manifest.id
            )));
        }
        self.run_with_instance(ctx, CallPhase::Decode, |live, _engine| {
            live.store.data_mut().active_read_path = Some(path.to_path_buf());
            f(live).map(Ok)
        })
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_probe(
        &self,
        ctx: &PluginContext,
        decoder_id: &str,
        path: &Path,
    ) -> Result<bindings::MediaProbe, CallError> {
        let path_str = path.to_string_lossy();
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_probe(&mut live.store, decoder_id, &path_str)
        })?
        .map_err(call_error)
    }

    pub(crate) fn call_decoder_open_video(
        &self,
        ctx: &PluginContext,
        decoder_id: &str,
        path: &Path,
    ) -> Result<bindings::OpenedVideo, CallError> {
        let path_str = path.to_string_lossy();
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_open_video(&mut live.store, decoder_id, &path_str)
        })?
        .map_err(call_error)
    }

    pub(crate) fn call_decoder_seek(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        pts: i64,
    ) -> Result<(), CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_seek(&mut live.store, instance, pts)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_next_frame(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
    ) -> Result<Option<bindings::DecodedFrame>, CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_next_frame(&mut live.store, instance)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_open_audio(
        &self,
        ctx: &PluginContext,
        decoder_id: &str,
        path: &Path,
        sample_rate: u32,
        channels: u16,
    ) -> Result<u64, CallError> {
        let path_str = path.to_string_lossy();
        self.run_decoder_call(ctx, path, |live| {
            live.plugin.current()?.call_decoder_open_audio(
                &mut live.store,
                decoder_id,
                &path_str,
                sample_rate,
                channels,
            )
        })?
        .map_err(call_error)
    }

    pub(crate) fn call_decoder_read_audio(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        max_samples: u32,
    ) -> Result<Vec<f32>, CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_read_audio(&mut live.store, instance, max_samples)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_seek_audio(
        &self,
        ctx: &PluginContext,
        path: &Path,
        instance: u64,
        frame: i64,
    ) -> Result<(), CallError> {
        self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_seek_audio(&mut live.store, instance, frame)
        })?
        .map_err(CallError::Failed)
    }

    pub(crate) fn call_decoder_close(&self, ctx: &PluginContext, path: &Path, instance: u64) {
        // A plugin that faulted or unmounted holds no streams to close.
        let _ = self.run_decoder_call(ctx, path, |live| {
            live.plugin
                .current()?
                .call_decoder_close(&mut live.store, instance)
        });
    }
}

fn call_error(error: bindings::DecodeError) -> CallError {
    match error {
        bindings::DecodeError::Unsupported(reason) => CallError::Unsupported(reason),
        bindings::DecodeError::Failed(reason) => CallError::Failed(reason),
    }
}

struct WasmImporter {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl Importer for WasmImporter {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let result = self
            .shared
            .call_import(&self.ctx, &self.id, file)
            .map_err(TransactError::Aborted)?;
        host_wire::apply_import_commands(tx, &result.commands_json, result.skipped, false)
    }
}

struct WasmExporter {
    shared: Arc<WasmPluginShared>,
    ctx: PluginContext,
    id: String,
}

impl Exporter for WasmExporter {
    fn export(&self, project: &Project, out: &mut dyn Write) -> std::io::Result<InterchangeReport> {
        let project_json = serde_json::to_string(project)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let (report, bytes) = self
            .shared
            .call_export(&self.ctx, &self.id, &project_json)
            .map_err(std::io::Error::other)?;
        out.write_all(&bytes)?;
        Ok(InterchangeReport {
            skipped: report.skipped,
        })
    }
}

impl HostState {
    pub(super) fn host_register_importer(
        &mut self,
        spec: bindings::ImporterSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-importer")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let id = spec.id.clone();
        let host_spec = ImporterSpec {
            id: spec.id,
            name: spec.name,
            extensions: spec.extensions,
        };
        ctx.register_importer(
            host_spec,
            WasmImporter {
                shared,
                ctx: ctx.clone(),
                id,
            },
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    pub(super) fn host_register_exporter(
        &mut self,
        spec: bindings::ExporterSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-exporter")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let id = spec.id.clone();
        let host_spec = ExporterSpec {
            id: spec.id,
            name: spec.name,
            extension: spec.extension,
        };
        ctx.register_exporter(
            host_spec,
            WasmExporter {
                shared,
                ctx: ctx.clone(),
                id,
            },
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    pub(super) fn host_register_encoder(
        &mut self,
        spec: bindings::EncoderSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-encoder")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let id = spec.id.clone();
        let host_spec = EncoderSpec {
            id: spec.id,
            name: spec.name,
            extension: spec.extension,
            audio: spec.audio,
        };
        ctx.register_encoder(host_spec, WasmEncoder::new(shared, ctx.clone(), id))
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    pub(super) fn host_register_decoder(
        &mut self,
        spec: bindings::DecoderSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-decoder")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let decoder = WasmDecoder::new(shared, ctx.clone(), spec.id.clone());
        ctx.register_decoder(
            DecoderSpec {
                id: spec.id,
                name: spec.name,
                extensions: spec.extensions,
                priority: spec.priority,
            },
            ForeignDecoder::new(decoder),
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    pub(super) fn host_register_audio_effect(
        &mut self,
        spec: bindings::AudioEffectSpec,
    ) -> Result<(), String> {
        let ctx = self.require_mount_phase("register-audio-effect")?;
        let shared = self
            .shared
            .upgrade()
            .ok_or_else(|| "plugin dropped".to_owned())?;
        let param_count = spec.params.len();
        let effect_id = spec.id.clone();
        let model_spec = AudioEffectSpec {
            id: spec.id,
            category: String::new(),
            label: spec.label,
            params: spec
                .params
                .into_iter()
                .map(|p| {
                    host_wire::audio_param_spec(p.name, p.label, p.default_value, p.min, p.max)
                })
                .collect(),
            latency: spec.latency,
            sidechain: spec.sidechain,
        };
        let factory = WasmAudioFactory::new(shared, ctx.clone(), effect_id, param_count);
        ctx.register_audio_effect(model_spec, factory)
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}
