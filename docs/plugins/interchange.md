# Importers, exporters, encoders, and decoders

## Importers and exporters

A plugin adds a timeline interchange format with an importer, an exporter, or
both. An importer reads a file into the open project through a
[`Transaction`](project.md#project); an exporter writes the open timeline.

```rust
use std::io::Write;
use std::path::Path;

use aria_models::TrackKind;
use aria_plugin::{
    Command, EditCommand, Exporter, ExporterSpec, Importer, ImporterSpec,
    InterchangeReport, Project, TransactError, Transaction,
};

struct LaneList;

impl Importer for LaneList {
    fn import(
        &self,
        file: &Path,
        tx: &mut Transaction<'_>,
    ) -> Result<InterchangeReport, TransactError> {
        let text = std::fs::read_to_string(file)
            .map_err(|err| TransactError::Aborted(err.to_string()))?;
        for name in text.lines() {
            tx.apply(Command::Edit(EditCommand::AddTrack {
                kind: TrackKind::Video,
                name: name.to_owned(),
                index: None,
                pinned: true,
            }))?;
        }
        Ok(InterchangeReport::default())
    }
}

impl Exporter for LaneList {
    fn export(
        &self,
        project: &Project,
        out: &mut dyn Write,
    ) -> std::io::Result<InterchangeReport> {
        for track in project.timeline().tracks_ordered() {
            writeln!(out, "{}", track.name)?;
        }
        Ok(InterchangeReport::default())
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_importer(
        ImporterSpec {
            id: "example.lanes".into(),
            name: "Lane list".into(),
            extensions: vec!["lanes".into()],
        },
        LaneList,
    )?;
    ctx.register_exporter(
        ExporterSpec {
            id: "example.lanes".into(),
            name: "Lane list".into(),
            extension: "lanes".into(),
        },
        LaneList,
    )?;
    Ok(())
}
```

Both returned `Registration`s are held on the terms of `register_effect`.

| Call | Result |
| --- | --- |
| `ctx.importers()`, `ctx.exporters()` | Every registered spec, in registration order |
| `ctx.importer_for(path)` | The importer for the extension of `path` |
| `ctx.import_file(&host, path)` | Runs that importer and returns its report and the transaction's `Receipt` |
| `ctx.export_file(id, &project, path)` | Runs the exporter registered as `id` and returns its report |

Extensions match without regard to case and are stored lowercase. When two
importers list one extension, the one registered first reads it. Built-in
plugins mount before external ones.

An import is one transaction named `Import <file name>`, or
`<plugin id>: Import <file name>` for a plugin's importer: one undo step. When
the importer returns an error or panics, nothing of the import remains and
`import_file` returns `InterchangeError::Import`.

An export writes to `.<file name>.partial` beside the destination, syncs it,
and renames it over the destination after the exporter returns. When the
exporter returns an error or panics, the destination is unchanged, the partial
file is removed, and `export_file` returns `InterchangeError::Export` or
`InterchangeError::ExportPanicked`. An exporter refuses a timeline its format
cannot hold with `std::io::Error::other(reason)`.

`InterchangeReport::skipped` lists, one line each, what the file held or the
project holds that the format could not carry.

A panic in `import` or `export` fails the plugin for the session.

Registration returns `PluginError::InterchangeRefused(message)` when:

- the id is empty, or another importer or exporter holds it;
- an importer lists no extension;
- an extension is empty or holds anything but ASCII letters and digits,
  such as a leading dot.

## Encoders

A plugin adds an output format to the media Export dialog by registering an
[`EncoderSpec`] and an [`Encoder`] handler during `mount`:

```rust
use std::path::Path;
use aria_plugin::{
    EncodeError, Encoder, EncoderConfig, EncoderSpec, PluginContext, PluginError, RationalTime,
    VideoEncoder, VideoFrame,
};

struct WebmVp9;

struct WebmSession {
    // Muxer state writing to the staged path.
}

impl VideoEncoder for WebmSession {
    fn push(&mut self, frame: &VideoFrame) -> Result<(), EncodeError> {
        let _ = frame;
        Ok(())
    }

    fn push_audio(&mut self, samples: &[f32], pts: RationalTime) -> Result<(), EncodeError> {
        let _ = (samples, pts);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), EncodeError> {
        Ok(())
    }
}

impl Encoder for WebmVp9 {
    fn open(
        &self,
        path: &Path,
        config: &EncoderConfig,
    ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
        let _ = (path, config);
        Ok(Box::new(WebmSession {}))
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_encoder(
        EncoderSpec {
            id: "example.webm-vp9".into(),
            name: "WebM (VP9)".into(),
            extension: "webm".into(),
            audio: true,
        },
        WebmVp9,
    )?;
    Ok(())
}
```

The returned `Registration` is held on the terms of `register_effect` and is
listed as `ContributionKind::Encoder`.

| Call | Result |
| --- | --- |
| `ctx.encoders()` | Every registered `Arc<EncoderSpec>`, in registration order |
| `ctx.encoder(id)` | The `Arc<EncoderSpec>` registered under `id` |
| `ctx.open_encoder(id, path, &config)` | Opens a guarded `Box<dyn VideoEncoder>` writing to `path` |

Selecting a registered encoder in the Export dialog sets the destination file
extension to `spec.extension`, hides the platform Codec and Bitrate rows, and
hides the Audio row when `spec.audio` is `false` (forcing video-only export).
When an export pass runs through
`aria_render::export_to_file_with_encoder_observed`, the encoder writes into a
private sibling staging directory (`TransactionalFile`) and atomically replaces
the destination only after `VideoEncoder::finish` succeeds. Cancellation or
failure discards the staging file and leaves an existing destination untouched.
If a plugin encoder unmounts while an export job is queued, that job fails
naming the encoder id instead of falling back to the platform encoder.

A panic in `Encoder::open`, `VideoEncoder::push`, `VideoEncoder::push_audio`,
or `VideoEncoder::finish` is caught, fails the owning plugin for the session,
drops its registrations, and returns `EncodeError`.

`register_encoder` returns `PluginError::EncoderRefused(message)` when:

- `id` is empty, or another encoder holds it;
- `extension` is empty or holds anything but ASCII letters and digits, such as
  a leading dot (stored lowercase).

In the native C ABI tier (`aria-sdk`), register an encoder with
`Host::register_encoder(EncoderSpec, impl Encoder)` and return a
`Box<dyn EncoderSession>` receiving RGBA8 frames (`EncoderFrame`) and
interleaved `f32` audio (`EncoderAudio`). Tables whose `struct_size` is at
least `ARIA_PLUGIN_V1_ENCODERS_SIZE` (`AriaPluginV1::has_encoders()`) participate in
encoder registration; earlier `ARIA_PLUGIN_V1_MIN_SIZE` and
`ARIA_PLUGIN_V1_PANELS_SIZE` tables remain loadable without encoder callbacks.

## Decoders

A plugin reads a media format by registering a [`DecoderSpec`] and an
[`ExternalDecoder`] during `mount`. Import, the Viewer, export, thumbnails,
and analysis open a file whose extension the spec lists through the decoder:

```rust
use std::path::Path;
use aria_plugin::{
    DecodeError, DecoderSpec, ExternalDecoder, MediaProbe, PluginContext, PluginError,
    VideoDecoder,
};

struct Y4m;

impl ExternalDecoder for Y4m {
    fn probe(&self, path: &Path) -> Result<MediaProbe, DecodeError> {
        let _ = path;
        Err(DecodeError::unsupported("not a y4m file"))
    }

    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        let _ = path;
        Err(DecodeError::unsupported("not a y4m file"))
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_decoder(
        DecoderSpec {
            id: "y4m".into(),
            name: "YUV4MPEG2".into(),
            extensions: vec!["y4m".into()],
            priority: 0,
        },
        Y4m,
    )?;
    Ok(())
}
```

The returned `Registration` is held on the terms of `register_effect` and is
listed as `ContributionKind::Decoder`.

| Call | Result |
| --- | --- |
| `ExternalDecoder::probe` | Size, frame rate, frame count, audio presence, and codec names, without decoding |
| `ExternalDecoder::open_video` | A `VideoDecoder` over the video track |
| `ExternalDecoder::open_audio` | An `AudioReader` producing interleaved `f32` at the mixer's rate and channel count; the default returns `DecodeError::Unsupported` |
| `ctx.decoders()` | Every registered `PluginDecoder` (plugin id and `Arc<DecoderSpec>`), in registration order |

The decoders that claim a file's extension are tried by descending
`priority`, then in registration order, and the platform decoder is tried
last. `DecodeError::Unsupported` passes the file to the next decoder. Any
other error also passes it on, and is appended to the platform decoder's error
when no decoder opens the file.

Frames are CPU planes in `PixelFormat::Nv12` or `PixelFormat::Yuv420p`. A
`VideoDecoder` follows the seek contract in `aria_core::VideoDecoder`: `seek`
lands at or before the target, and `next_frame` returns frames in
presentation order.

Every import dialog offers the extensions of every registered decoder after
the shipped ones, and a dropped file with one of them is imported as media.

A panic in any call into the decoder, or into a `VideoDecoder` or
`AudioReader` it returned, is caught, fails the owning plugin for the session,
drops its registrations, and returns `DecodeError::Decode`. The time of each
frame decode is recorded as the plugin's frame cost in the Plugins list.

`register_decoder` returns `PluginError::DecoderRefused(message)` when:

- `id` is empty, or the plugin already registered a decoder under it;
- `extensions` is empty;
- an extension is empty or holds anything but ASCII letters and digits, such
  as a leading dot (stored lowercase).

In the native C ABI tier (`aria-sdk`), register a decoder with
`Host::register_decoder(DecoderSpec, impl Decoder)`. `Decoder::probe` returns a
`DecoderProbe`, `Decoder::open_video` returns a `VideoInfo` and a
`Box<dyn VideoStream>` whose `next_frame` fills host-allocated planes, and
`Decoder::open_audio` returns a `Box<dyn AudioStream>`. Return
`DecodeFailure::Unsupported` to pass the file to the next decoder. Tables whose
`struct_size` is at least `ARIA_PLUGIN_V1_DECODERS_SIZE` (`AriaPluginV1::has_decoders()`)
register decoders; shorter tables remain loadable without them. See
[Native plugins](native.md#c-abi-tables-and-size-negotiation).

In the WebAssembly tier, call `register-decoder` during `mount` and implement
the `decoder-*` exports; see [Media Decoders](wasm.md#media-decoders).
