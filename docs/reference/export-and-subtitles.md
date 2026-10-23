# Export, Interchange, and Subtitles Reference

Reference for video export codecs, platform encoder capabilities, export sheet ladders, timeline interchange formats, subtitle formats, and caption commit options.

## Video codecs (`VideoCodec`)

Defined in `crates/aria-core/src/encode.rs` (`VideoCodec::ALL`). `VideoCodec::from_key` matches keys case-insensitively and accepts `avc` for `H264` and `hevc` for `H265`.

| Variant | Key | Aliases | Picker label | Default |
| --- | --- | --- | --- | --- |
| `H264` | `h264` | `h264`, `avc` | H.264 | `yes` |
| `H265` | `h265` | `h265`, `hevc` | H.265 | `no` |

## Platform encoder capabilities (`EncoderCapability` and `AUDIO_CAPABILITY`)

Defined in `crates/aria-encoder/src/lib.rs` (`capability()` and `AUDIO_CAPABILITY`), `crates/aria-render/src/export_png.rs` (`PngSequenceEncoder`), `crates/aria-render/src/export_audio.rs` (`EXPORT_AUDIO_RATE`, `EXPORT_AUDIO_CHANNELS`), and `crates/aria-render/src/error.rs` (`MIN_EXPORT_DIM`).

### Platform video encoders (`aria_encoder::capability`)

| Target cfg | Backend | Container | Extension | Default video | Audio | Available | Codecs | Honours bitrate |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `target_vendor = "apple"` | `VideoToolbox` | `MP4` | `mp4` | `H.264` | `AAC` | `true` | `H.264` | `false` |
| `target_os = "windows"` | `Media Foundation` | `MP4` | `mp4` | `H.264` | `AAC` | `true` | `H.264` | `false` |
| `target_os = "android"` | `MediaCodec` | `MP4` | `mp4` | `H.264` | `AAC` | `true` | `H.264` | `false` |
| `target_os = "linux"` | `GStreamer` | `MP4` | `mp4` | `H.264` | `AAC` | `true` | `H.264, H.265` | `true` |
| `other` | `none` | `none` | `(empty)` | `none` | `none` | `false` | `none` | `false` |

### Linux encoder elements (`aria_encoder::gst`)

Defined in `crates/aria-encoder/src/gst_element.rs` (`candidates`, `SOFTWARE_ENCODERS`, `SOFTWARE_SPEED_PRESET`). The GStreamer backend builds each element in order and encodes with the first one that opens its device.

| Codec | Elements in order | Software element | Software speed preset |
| --- | --- | --- | --- |
| `H.264` | `nvh264enc`, `nvcudah264enc`, `vaapih264enc`, `x264enc` | `x264enc` | `veryfast` |
| `H.265` | `nvh265enc`, `nvcudah265enc`, `vaapih265enc`, `x265enc` | `x265enc` | `veryfast` |

`x264enc` also runs `tune=zerolatency` with trellis quantization off. A hardware element whose GStreamer plugin failed to load in the process is skipped without being built again. Only `x265enc` has `videoconvert` in front of it, because it takes no NV12.

The GStreamer encoder takes CPU NV12 frames. An export frame that is one BT.709 NV12 clip decoded on the CPU and drawn unchanged over the whole canvas at the output size goes to the encoder as decoded, with no composite and no RGBA to NV12 conversion. Every other frame is composited on the GPU and converted.

Each NV12 frame is written with rows as wide as the frame. When GStreamer's default NV12 layout pads the rows to 4 bytes, as at a width 2 more than a multiple of 4, the frame buffer has a `GstVideoMeta` stating its row stride and plane offsets.

### Sound-only and frame-sequence sinks and export constants

| Item | Value | Source |
| --- | --- | --- |
| `AUDIO_CAPABILITY.container` | `WAV` | `crates/aria-encoder/src/lib.rs` |
| `AUDIO_CAPABILITY.codec` | `16-bit PCM` | `crates/aria-encoder/src/lib.rs` |
| `AUDIO_CAPABILITY.extension` | `wav` | `crates/aria-encoder/src/lib.rs` |
| `AUDIO_CAPABILITY.label()` | `WAV · 16-bit PCM` | `crates/aria-encoder/src/lib.rs` |
| `PngSequenceEncoder::choice().encoder` | `png` | `crates/aria-render/src/export_png.rs` |
| `PngSequenceEncoder::choice().hardware` | `Some(false)` | `crates/aria-render/src/export_png.rs` |
| `EXPORT_AUDIO_RATE` | `48000` | `crates/aria-render/src/export_audio.rs` |
| `EXPORT_AUDIO_CHANNELS` | `2` | `crates/aria-render/src/export_audio.rs` |
| `MIN_EXPORT_DIM` | `16` | `crates/aria-render/src/error.rs` |

## Export sheet ladders (`ExportOptions`)

Defined in `crates/aria-ui/src/app/export_options.rs`. `ExportOptions::default()` selects `ExportScale::Full`, `ExportRate::Project`, `ExportGop::Codec`, `audio = true`, `marked_range = false`, `VideoCodec::H264`, `ExportBitrate::Encoder`, and `plugin_encoder = None`.

### Output scale (`ExportScale::ALL`)

Each dimension is scaled by `(v * num / den).max(2)` and rounded down to an even integer (`even_export_size`).

| Variant | Label | Ratio (`num/den`) | Default |
| --- | --- | --- | --- |
| `Full` | 100% | `1/1` | `yes` |
| `Threequarter` | 75% | `3/4` | `no` |
| `Half` | 50% | `1/2` | `no` |
| `Quarter` | 25% | `1/4` | `no` |

### Output frame rate (`ExportRate::ALL`)

| Variant | Label | Resolved rate (`num/den`) | Default |
| --- | --- | --- | --- |
| `Project` | Project | `project` | `yes` |
| `Fps24` | 24 | `24/1` | `no` |
| `Fps25` | 25 | `25/1` | `no` |
| `Fps30` | 30 | `30/1` | `no` |
| `Fps50` | 50 | `50/1` | `no` |
| `Fps60` | 60 | `60/1` | `no` |

### Keyframe spacing (`ExportGop::ALL`)

| Variant | Label | Max interval (`frames`) | Default |
| --- | --- | --- | --- |
| `Codec` | Codec default | `none` | `yes` |
| `Frames(15)` | Every 15 frames | `15` | `no` |
| `Frames(30)` | Every 30 frames | `30` | `no` |
| `Frames(60)` | Every 60 frames | `60` | `no` |

### Target video bitrate (`ExportBitrate::ALL`)

| Variant | Label | Target (`kbps`) | Default |
| --- | --- | --- | --- |
| `Encoder` | Encoder default | `none` | `yes` |
| `Kbps(4000)` | 4 Mb/s | `4000` | `no` |
| `Kbps(8000)` | 8 Mb/s | `8000` | `no` |
| `Kbps(16000)` | 16 Mb/s | `16000` | `no` |
| `Kbps(40000)` | 40 Mb/s | `40000` | `no` |

### Empty export refusals (`EmptyExport::ALL`)

Defined in `crates/aria-render/src/error.rs` (`EmptyExport::ALL`).

| Variant | Condition |
| --- | --- |
| `Sequence` | No track holds a clip, so the sequence has zero duration to render. |
| `Range` | The timeline has content, but the requested `[from, to)` range covers zero frames. |
| `Silence` | A sound-only export whose mix is silent across the entire exported span. |

## Timeline interchange and subtitle formats (`aria-interchange`)

Defined in `crates/aria-interchange/src/lib.rs` (`FORMATS`, `MAX_FILE_BYTES`) and `crates/aria-interchange/src/subtitles.rs` (`SUBTITLE_FORMATS`). `InterchangePlugin` (`aria.interchange`) registers an importer and an exporter for each format. Timeline interchange reads at most `MAX_FILE_BYTES` (`268435456` bytes, 256 MiB).

### Timeline interchange formats (`FORMATS`)

| ID | Menu name | Write extension | Read extensions |
| --- | --- | --- | --- |
| `aria.interchange.edl` | CMX 3600 EDL | `edl` | `edl` |
| `aria.interchange.fcpxml` | Final Cut Pro XML | `fcpxml` | `fcpxml` |
| `aria.interchange.otio` | OpenTimelineIO | `otio` | `otio` |

### Subtitle formats (`SUBTITLE_FORMATS`)

Subtitle import places cues onto new text tracks as `Generator::Text` clips at the open timeline's frame rate; overlapping cues advance to additional numbered text tracks. Subtitle export writes enabled text tracks in start-time order (`HH:MM:SS,mmm` for SubRip, `HH:MM:SS.mmm` with `&`, `<`, and `>` escaped for WebVTT).

| ID | Menu name | Extension |
| --- | --- | --- |
| `aria.interchange.srt` | SubRip subtitles | `srt` |
| `aria.interchange.vtt` | WebVTT subtitles | `vtt` |

## Transcription and caption commit options (`CaptionCommit`)

Defined in `crates/aria-ui/src/app/captions.rs`.

### Caption commit targets (`CaptionCommit::ALL`)

| Variant | Chip ID | Label | Default |
| --- | --- | --- | --- |
| `TextClips` | `commit-clips` | Text clips | `yes` |
| `Markers` | `commit-markers` | Markers | `no` |

### Transcription constants

| Constant | Value | Meaning |
| --- | --- | --- |
| `WHISPER_SAMPLE_RATE` | `16000` | Audio sample rate in Hz fed to Whisper inference |
| `MAX_PCM_FRAMES` | `28800000` | Maximum audio span per transcription run (30 minutes at 16 kHz mono) |
| `THREAD_CHOICES` | `1, 2, 4, 8` | Decoder thread count chips offered in the Captions panel |
| `TRANSCRIPT_ANALYZER` | `aria.transcription` | Storage analyzer ID in the moments index |
| `TRANSCRIPT_ANALYZER_VERSION` | `1` | Transcript result schema version in the moments index |
