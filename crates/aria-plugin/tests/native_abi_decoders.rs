//! Native C ABI decoders. An `aria-sdk` plugin registers a decoder with
//! `Host::register_decoder`, and `aria_decoder` opens the extensions it lists
//! through the host's `decoder_*` calls: the probe, the stream description,
//! exact planes at a seek target counted in the plugin's own time base, end of
//! stream, a backward seek, and audio reads and seeks counted in samples. A
//! panic in a frame decode fails the plugin and unclaims its extension, and a
//! plugin table without the decoder entries cannot register a decoder.
//!
//! Its own test binary: the decoder list is process-wide, and each test here
//! claims an extension no other test claims.
//!
//! Not covered: a plugin built as a separate `cdylib`; the in-process table
//! has the same layout `aria_sdk::export!` builds.

#![allow(unsafe_code)]

use std::path::Path;
use std::sync::LazyLock;

use aria_core::{ColorSpace, FrameData, PixelFormat, Rational, RationalTime, VideoFrame};
use aria_plugin::PluginContext;
use aria_plugin::external::{from_table, host_table};
use aria_plugin::package::{MANIFEST_FILE, Package};
use aria_plugin_abi::{ARIA_API_MAJOR, ARIA_PLUGIN_V1_JOBS_SIZE};
use aria_sdk::{
    AudioStream, DecodeFailure, Decoder, DecoderProbe, DecoderSpec, FrameColor, FrameFormat,
    FramePlane, Host, Plugin as SdkPlugin, PluginRuntime, VideoInfo, VideoStream,
};
use tempfile::TempDir;

#[macro_use]
#[path = "support/plugin_table.rs"]
mod plugin_table;

const FPS_24: Rational = Rational::FPS_24;
/// The plugin's time base: two ticks per frame, so a host that confuses
/// ticks with frames lands on the wrong frame.
const TICKS: Rational = Rational::new(48, 1);
const SIZE: (u32, u32) = (8, 4);
/// Audio length in sample frames.
const AUDIO_FRAMES: i64 = 10;

/// Luma of pixel `(x, y)` in frame `index`: distinct per pixel and per frame,
/// so a swapped plane, row, or frame shows.
fn luma(index: u8, x: usize, y: usize) -> u8 {
    index * 40 + (y * 8 + x) as u8
}

/// Sample `channel` of sample frame `frame`.
fn sample(frame: i64, channel: usize) -> f32 {
    frame as f32 * 0.01 + channel as f32 * 0.5
}

/// A ramp file: byte 0 is its frame count, byte 1 makes frame decodes panic
/// when nonzero.
fn read_ramp(path: &Path) -> Result<(u8, bool), DecodeFailure> {
    match std::fs::read(path)
        .map_err(|err| err.to_string())?
        .as_slice()
    {
        [frames, panic] => Ok((*frames, *panic != 0)),
        other => Err(DecodeFailure::Unsupported(format!(
            "a ramp file is 2 bytes, not {}",
            other.len()
        ))),
    }
}

struct Ramp;

impl Decoder for Ramp {
    fn probe(&self, _host: &Host, path: &Path) -> Result<DecoderProbe, DecodeFailure> {
        let (frames, _) = read_ramp(path)?;
        Ok(DecoderProbe {
            width: SIZE.0,
            height: SIZE.1,
            frame_rate_num: 24,
            frame_rate_den: 1,
            frame_count: i64::from(frames),
            has_audio: true,
            is_image: false,
            video_codec: Some("ramp".to_owned()),
            audio_codec: Some("ramp-pcm".to_owned()),
        })
    }

    fn open_video(
        &self,
        _host: &Host,
        path: &Path,
    ) -> Result<(VideoInfo, Box<dyn VideoStream>), DecodeFailure> {
        let (frames, panic) = read_ramp(path)?;
        let info = VideoInfo {
            width: SIZE.0,
            height: SIZE.1,
            format: FrameFormat::Yuv420p,
            color: FrameColor::Bt601,
            frame_rate_num: 24,
            frame_rate_den: 1,
            time_base_num: 48,
            time_base_den: 1,
            duration: Some(i64::from(frames) * 2),
        };
        let stream = RampFrames {
            frames: i64::from(frames),
            next: 0,
            panic,
        };
        Ok((info, Box::new(stream)))
    }

    fn open_audio(
        &self,
        _host: &Host,
        path: &Path,
        _sample_rate: u32,
        channels: u16,
    ) -> Result<Box<dyn AudioStream>, DecodeFailure> {
        read_ramp(path)?;
        Ok(Box::new(RampAudio {
            channels: usize::from(channels),
            next: 0,
        }))
    }
}

struct RampFrames {
    frames: i64,
    next: i64,
    panic: bool,
}

impl VideoStream for RampFrames {
    /// Even frames are sync frames, so the host rolls forward from one.
    fn seek(&mut self, pts: i64) -> Result<(), String> {
        let frame = (pts / 2).clamp(0, self.frames);
        self.next = frame - frame % 2;
        Ok(())
    }

    fn next_frame(&mut self, planes: &mut [FramePlane<'_>]) -> Result<Option<i64>, String> {
        assert!(!self.panic, "ramp frame decode exploded");
        if self.next >= self.frames {
            return Ok(None);
        }
        let index = self.next as u8;
        for (n, plane) in planes.iter_mut().enumerate() {
            for y in 0..plane.rows {
                for x in 0..plane.stride {
                    plane.data[y * plane.stride + x] = match n {
                        0 => luma(index, x, y),
                        1 => 100 + index,
                        _ => 200 - index,
                    };
                }
            }
        }
        self.next += 1;
        Ok(Some((self.next - 1) * 2))
    }
}

struct RampAudio {
    channels: usize,
    next: i64,
}

impl AudioStream for RampAudio {
    fn read(&mut self, out: &mut [f32]) -> Result<usize, String> {
        let frames = ((out.len() / self.channels) as i64).min(AUDIO_FRAMES - self.next);
        let samples = frames.max(0) as usize * self.channels;
        for (at, slot) in out[..samples].iter_mut().enumerate() {
            *slot = sample(self.next + (at / self.channels) as i64, at % self.channels);
        }
        self.next += frames.max(0);
        Ok(samples)
    }

    fn seek(&mut self, frame: i64) -> Result<(), String> {
        self.next = frame.clamp(0, AUDIO_FRAMES);
        Ok(())
    }
}

/// Registers [`Ramp`] for `extension`.
struct Guest {
    extension: &'static str,
}

impl SdkPlugin for Guest {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_decoder(
            DecoderSpec {
                id: "ramp".to_owned(),
                name: "Ramp".to_owned(),
                extensions: vec![self.extension.to_owned()],
                priority: 0,
            },
            Ramp,
        )
    }
}

fn bound(guest: Guest) -> PluginRuntime<Guest> {
    let runtime = PluginRuntime::new(guest);
    // SAFETY: `host_table()` is the host's static `AriaHostV1`.
    assert!(unsafe { runtime.bind_host(host_table(), ARIA_API_MAJOR) });
    runtime
}

/// The plugin `table` as package `id`, loaded on a fresh context.
fn loaded(
    temp: &TempDir,
    id: &str,
    table: &aria_plugin_abi::AriaPluginV1,
) -> (PluginContext, Box<dyn aria_plugin::Plugin>) {
    let dir = temp.path().join(id);
    std::fs::create_dir_all(&dir).expect("create package dir");
    let manifest = format!(
        r#"[plugin]
id = "{id}"
name = "Native Decoders"
version = "0.1.0"
api = 1
publisher = "Aria Tests"
description = "Native C-ABI decoder test package"
license = "MIT"
"#
    );
    std::fs::write(dir.join(MANIFEST_FILE), manifest).expect("write plugin.toml");
    let pkg = Package::open(&dir).expect("open package");
    let plugin = from_table(&pkg, *table, Some(temp.path())).expect("from_table");
    (PluginContext::new(), plugin)
}

fn write_ramp(temp: &TempDir, name: &str, frames: u8, panic: bool) -> std::path::PathBuf {
    let path = temp.path().join(name);
    std::fs::write(&path, [frames, u8::from(panic)]).expect("write ramp");
    path
}

/// The planes of a CPU frame as `(bytes, stride, rows)`.
fn cpu_planes(frame: &VideoFrame) -> Vec<(Vec<u8>, usize, usize)> {
    let FrameData::Cpu(cpu) = &frame.data else {
        panic!("a plugin decoder returns CPU frames");
    };
    cpu.planes
        .iter()
        .map(|plane| (plane.data.clone(), plane.stride, plane.rows))
        .collect()
}

fn ramp_planes(index: u8) -> Vec<(Vec<u8>, usize, usize)> {
    let y = (0..4)
        .flat_map(|y| (0..8).map(move |x| luma(index, x, y)))
        .collect();
    vec![
        (y, 8, 4),
        (vec![100 + index; 8], 4, 2),
        (vec![200 - index; 8], 4, 2),
    ]
}

#[test]
fn native_decoder_probes_decodes_seeks_and_reads_audio_through_the_host() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(|| {
        bound(Guest {
            extension: "natramp",
        })
    });
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativedecoders.ramp", &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount");
    let listed = ctx.decoders();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].spec.id, "ramp");
    assert_eq!(listed[0].spec.extensions, ["natramp"]);

    let clip = write_ramp(&temp, "clip.NATRAMP", 5, false);
    let probed = aria_decoder::probe(&clip).expect("probes through the plugin");
    assert_eq!((probed.width, probed.height), SIZE);
    assert_eq!((probed.frame_rate, probed.frame_count), (FPS_24, 5));
    assert_eq!(probed.video_codec.as_deref(), Some("ramp"));
    assert_eq!(probed.audio_codec.as_deref(), Some("ramp-pcm"));
    assert!(probed.has_audio && !probed.is_image);

    let mut decoder = aria_decoder::open_video_decoder(&clip, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    let info = decoder.info().clone();
    assert_eq!(info.coded_size, SIZE);
    assert_eq!(info.pixel_format, PixelFormat::Yuv420p);
    assert_eq!(info.color, ColorSpace::BT601);
    assert_eq!((info.frame_rate, info.time_base), (FPS_24, TICKS));
    assert_eq!(info.duration, Some(RationalTime::new(10, TICKS)));

    let third = decoder
        .frame_at(RationalTime::new(3, FPS_24))
        .expect("decodes")
        .expect("frame 3 exists");
    assert_eq!(third.pts, RationalTime::new(6, TICKS));
    assert_eq!(cpu_planes(&third), ramp_planes(3));
    let fourth = decoder.next_frame().expect("decodes").expect("frame 4");
    assert_eq!(fourth.pts, RationalTime::new(8, TICKS));
    assert_eq!(cpu_planes(&fourth), ramp_planes(4));
    assert!(
        decoder.next_frame().expect("end of stream").is_none(),
        "a frame past the last one"
    );
    let first = decoder
        .frame_at(RationalTime::new(1, FPS_24))
        .expect("seeks back")
        .expect("frame 1 exists");
    assert_eq!(cpu_planes(&first), ramp_planes(1));
    drop(decoder);

    let mut audio = aria_decoder::open_audio_reader(&clip, 48_000, 2).expect("opens audio");
    let mut block = [0.0f32; 8];
    assert_eq!(audio.read(&mut block).expect("reads"), 4, "sample frames");
    let want: Vec<f32> = (0..4)
        .flat_map(|frame| (0..2).map(move |channel| sample(frame, channel)))
        .collect();
    assert_eq!(block.as_slice(), want.as_slice());
    assert_eq!(audio.position(), Some(4));
    audio.seek_to_frame(7).expect("seeks");
    let mut tail = [0.0f32; 8];
    assert_eq!(audio.read(&mut tail).expect("reads"), 3, "frames 7, 8, 9");
    assert_eq!(
        &tail[..6],
        &[
            sample(7, 0),
            sample(7, 1),
            sample(8, 0),
            sample(8, 1),
            sample(9, 0),
            sample(9, 1)
        ]
    );
    assert_eq!(audio.read(&mut tail).expect("end of stream"), 0);
    drop(audio);

    let other = temp.path().join("other.natramp");
    std::fs::write(&other, b"not a ramp").expect("write");
    let refused = aria_decoder::probe(&other);
    assert!(
        refused.is_err(),
        "an unsupported file falls through to the platform decoder, which reads no ramp: {refused:?}"
    );
    assert!(ctx.failures().is_empty(), "{:?}", ctx.failures());
    assert!(ctx.is_mounted("test.nativedecoders.ramp"));
}

#[test]
fn a_panicking_native_frame_decode_fails_the_plugin_and_unclaims_its_extension() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(|| {
        bound(Guest {
            extension: "natboom",
        })
    });
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativedecoders.panic", &plugin_table!(RT));
    ctx.mount(plugin.as_ref()).expect("mount");
    assert!(aria_decoder::external_decoder_extensions().contains(&"natboom".to_owned()));

    let clip = write_ramp(&temp, "boom.natboom", 3, true);
    let mut decoder = aria_decoder::open_video_decoder(&clip, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    let err = decoder
        .frame_at(RationalTime::new(0, FPS_24))
        .expect_err("a panic is an error")
        .to_string();
    assert!(err.contains("ramp frame decode exploded"), "{err}");
    drop(decoder);

    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].id, "test.nativedecoders.panic");
    assert!(
        failures[0].reason.contains("ramp frame decode exploded"),
        "{}",
        failures[0].reason
    );
    assert!(
        ctx.decoders().is_empty(),
        "a failed plugin's decoder is listed"
    );
    assert!(
        !aria_decoder::external_decoder_extensions().contains(&"natboom".to_owned()),
        "a failed plugin's decoder still claims .natboom"
    );
}

#[test]
fn a_plugin_table_without_decoder_entries_cannot_register_a_decoder() {
    static RT: LazyLock<PluginRuntime<Guest>> = LazyLock::new(|| {
        bound(Guest {
            extension: "natshort",
        })
    });
    let mut table = plugin_table!(RT);
    table.struct_size = ARIA_PLUGIN_V1_JOBS_SIZE;
    assert!(table.has_jobs() && !table.has_decoders());
    let temp = TempDir::new().expect("temp dir");
    let (ctx, plugin) = loaded(&temp, "test.nativedecoders.short", &table);

    let err = ctx
        .mount(plugin.as_ref())
        .expect_err("register_decoder is refused")
        .to_string();
    assert!(err.contains("decoder callbacks"), "{err}");
    assert!(ctx.decoders().is_empty());
    assert!(!aria_decoder::external_decoder_extensions().contains(&"natshort".to_owned()));
}
