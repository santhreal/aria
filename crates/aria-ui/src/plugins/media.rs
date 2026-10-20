//! Decode and encode, wrapping the native readers and writers.

use std::path::Path;
use std::sync::Arc;

use aria_core::{DecodeError, EncodeError, EncoderConfig, VideoDecoder, VideoEncoder};
use aria_decoder::{OutputMode, open_video_decoder, probe};
use aria_encoder::open_encoder;
use aria_plugin::{
    DecoderService, DropAction, EncoderService, Plugin, PluginContext, PluginError, PluginManifest,
    ResolveDropped, Service,
};

use super::{has_extension, register_dyn};

pub struct DecoderPlugin;

struct NativeDecoder;

impl Service for NativeDecoder {}

impl DecoderService for NativeDecoder {
    fn can_decode(&self, path: &Path) -> bool {
        probe(path).is_ok()
    }

    fn open(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        open_video_decoder(path, OutputMode::default())
    }
}

impl Plugin for DecoderPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.decoder", "Decoder", "0.1.0", &[], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn DecoderService>(ctx, Arc::new(NativeDecoder));
        // Claim dropped media by extension, the list the import dialogs offer
        // so a picker and a drop cannot disagree. Asked before the project and
        // LUT plugins, so the list must not contain their file types: an
        // extension test here is cheap, and probing every dropped path would
        // open files meant only for another plugin.
        ctx.on_serial::<ResolveDropped, DropAction, _>(|event| {
            event
                .item
                .as_path()
                .is_some_and(|path| {
                    has_extension(path, &crate::app::media::import::media_import_extensions())
                })
                .then_some(DropAction::ImportMedia)
        });
        Ok(())
    }
}

pub struct EncoderPlugin;

struct NativeEncoder;

impl Service for NativeEncoder {}

impl EncoderService for NativeEncoder {
    fn can_encode(&self, config: &EncoderConfig) -> bool {
        config.size.0 > 0 && config.size.1 > 0 && config.frame_rate.is_valid()
    }

    /// The destination is not in [`EncoderConfig`], so this service names one.
    ///
    /// It names one path, not a new one per call, and it creates no directory:
    /// the previous file of the same shape is overwritten. The earlier form made
    /// a `aria-encode` directory and added a file to it on every encode, which is
    /// how a temp dir fills up with nobody having asked for a single file.
    fn create(&self, config: EncoderConfig) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let path = std::env::temp_dir().join(format!(
            "aria-encode-{}x{}-{}.mp4",
            config.size.0, config.size.1, config.frame_rate.num
        ));
        open_encoder(&path, config)
    }
}

impl Plugin for EncoderPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.encoder", "Encoder", "0.1.0", &[], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        register_dyn::<dyn EncoderService>(ctx, Arc::new(NativeEncoder));
        Ok(())
    }
}
