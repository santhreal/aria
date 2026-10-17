//! Decoder plugins: a registered decoder reads its extension for import and
//! render, a panic in it fails its plugin and unclaims the extension, and
//! registration refuses malformed specs.
//!
//! Its own test binary: the decoder list is process-wide, and the extensions
//! here are claimed by no other test. Does not cover the WASM and native ABI
//! tiers, whose tests drive their own decoders.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use aria_core::{
    ColorSpace, CpuImage, DecodeError, FrameData, PixelFormat, Plane, Rational, RationalTime, Rect,
    Rotation, SourceInfo, TimeRange, VideoDecoder, VideoFrame, resample,
};
use aria_models::{Project, TrackKind};
use aria_plugin::{
    ContributionKind, DecoderSpec, ExternalDecoder, MediaProbe, Plugin, PluginContext, PluginError,
    PluginManifest,
};
use aria_render::Renderer;

const FPS_24: Rational = Rational::FPS_24;
const SIZE: (u32, u32) = (64, 36);
const FRAMES: i64 = 48;

/// Reads a one-byte file as a solid grey clip of [`FRAMES`] frames whose luma
/// is that byte. With `panic_in_frames`, decoding a frame panics.
struct Solid {
    panic_in_frames: &'static AtomicBool,
}

fn luma(path: &Path) -> Result<u8, DecodeError> {
    let bytes = std::fs::read(path).map_err(|err| DecodeError::Io(err.to_string()))?;
    match bytes.as_slice() {
        [y] => Ok(*y),
        _ => Err(DecodeError::Open("a solid file is one byte".into())),
    }
}

impl ExternalDecoder for Solid {
    fn probe(&self, path: &Path) -> Result<MediaProbe, DecodeError> {
        luma(path)?;
        Ok(MediaProbe {
            width: SIZE.0,
            height: SIZE.1,
            frame_rate: FPS_24,
            frame_count: FRAMES,
            has_audio: false,
            is_image: false,
            video_codec: Some("solid".into()),
            audio_codec: None,
        })
    }

    fn open_video(&self, path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        Ok(Box::new(SolidFrames {
            info: SourceInfo {
                coded_size: SIZE,
                display_size: SIZE,
                rotation: Rotation::None,
                pixel_format: PixelFormat::Yuv420p,
                color: ColorSpace::BT709,
                frame_rate: FPS_24,
                time_base: FPS_24,
                duration: Some(RationalTime::new(FRAMES, FPS_24)),
            },
            y: luma(path)?,
            cursor: 0,
            panic_in_frames: self.panic_in_frames,
        }))
    }
}

struct SolidFrames {
    info: SourceInfo,
    y: u8,
    cursor: i64,
    panic_in_frames: &'static AtomicBool,
}

impl VideoDecoder for SolidFrames {
    fn info(&self) -> &SourceInfo {
        &self.info
    }

    fn seek(&mut self, target: RationalTime) -> Result<(), DecodeError> {
        self.cursor = resample(target, FPS_24).value.clamp(0, FRAMES);
        Ok(())
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, DecodeError> {
        assert!(
            !self.panic_in_frames.load(Ordering::Relaxed),
            "solid frame decode failed"
        );
        if self.cursor >= FRAMES {
            return Ok(None);
        }
        let pts = RationalTime::new(self.cursor, FPS_24);
        self.cursor += 1;
        let (w, h) = (SIZE.0 as usize, SIZE.1 as usize);
        let planes = vec![
            Plane::new(vec![self.y; w * h], w, h),
            Plane::new(vec![128; w / 2 * (h / 2)], w / 2, h / 2),
            Plane::new(vec![128; w / 2 * (h / 2)], w / 2, h / 2),
        ];
        Ok(Some(VideoFrame::new(
            pts,
            PixelFormat::Yuv420p,
            ColorSpace::BT709,
            SIZE,
            Rect::from_size(SIZE.0, SIZE.1),
            Rotation::None,
            FrameData::Cpu(CpuImage::new(planes)),
        )))
    }
}

struct SolidPlugin {
    manifest: PluginManifest,
    extension: &'static str,
    panic_in_frames: &'static AtomicBool,
}

impl Plugin for SolidPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_decoder(
            DecoderSpec {
                id: "solid".into(),
                name: "Solid grey".into(),
                extensions: vec![self.extension.to_ascii_uppercase()],
                priority: 0,
            },
            Solid {
                panic_in_frames: self.panic_in_frames,
            },
        )?;
        Ok(())
    }
}

fn plugin(id: &'static str, extension: &'static str, panics: &'static AtomicBool) -> SolidPlugin {
    SolidPlugin {
        manifest: PluginManifest::new(id, "Solid", "1.0.0", &[], &[]),
        extension,
        panic_in_frames: panics,
    }
}

/// A one-byte file named `name` holding `y`, in a fresh directory.
fn solid_file(dir: &Path, name: &str, y: u8) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, [y]).expect("writes the file");
    path
}

/// The project with `path` imported and placed at 0 on a video track.
fn placed(path: &Path) -> Project {
    let media = aria_engine::import_media(path).expect("the plugin decoder probes the file");
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Video, "V1");
    let id = project.add_media(media);
    project
        .add_clip(
            track,
            id,
            TimeRange::at_rate(0, FRAMES, FPS_24),
            RationalTime::new(0, FPS_24),
        )
        .expect("places the clip");
    project
}

fn grey(y: u8) -> u8 {
    // Limited-range luma to full-range grey.
    ((f32::from(y) - 16.0) * 255.0 / 219.0)
        .round()
        .clamp(0.0, 255.0) as u8
}

#[test]
fn a_mounted_decoder_imports_and_renders_its_files_until_unmounted() {
    static NEVER: AtomicBool = AtomicBool::new(false);
    let dir = tempfile::tempdir().expect("a temp dir");
    let dark = solid_file(dir.path(), "dark.ariasolid", 60);
    let light = solid_file(dir.path(), "light.AriaSolid", 180);
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.solid", "ariasolid", &NEVER))
        .expect("mounts");

    let listed = ctx.decoders();
    assert_eq!(listed.len(), 1);
    assert_eq!(&*listed[0].plugin_id, "test.solid");
    assert_eq!(listed[0].spec.extensions, ["ariasolid"]);
    let info = ctx.mounted();
    let contributions = &info
        .iter()
        .find(|info| info.manifest.id == "test.solid")
        .expect("listed")
        .contributions;
    assert_eq!(contributions.len(), 1);
    assert_eq!(contributions[0].kind, ContributionKind::Decoder);

    let probed = aria_decoder::probe(&dark).expect("probes through the plugin");
    assert_eq!((probed.width, probed.height), SIZE);
    assert_eq!((probed.frame_rate, probed.frame_count), (FPS_24, FRAMES));
    assert_eq!(probed.video_codec.as_deref(), Some("solid"));

    let projects = [(placed(&dark), 60), (placed(&light), 180)];
    match Renderer::new_headless() {
        Ok(mut renderer) => {
            for (project, y) in &projects {
                let image = renderer
                    .render_frame(project, RationalTime::new(5, FPS_24))
                    .expect("renders the plugin's frame");
                let [r, g, b, a] = image.pixel(image.width / 2, image.height / 2);
                let want = grey(*y);
                for (channel, got) in [("r", r), ("g", g), ("b", b)] {
                    assert!(
                        got.abs_diff(want) <= 4,
                        "luma {y}: {channel} {got}, expected ~{want}"
                    );
                }
                assert_eq!(a, 255);
            }
            let cost = ctx.cost_of("test.solid").expect("a mounted plugin's cost");
            assert!(cost.frame.calls > 0, "decoding recorded no frame cost");
        }
        Err(err) => eprintln!("skipping the render half: no headless GPU: {err}"),
    }

    ctx.unmount("test.solid").expect("unmounts");
    assert!(ctx.decoders().is_empty());
    assert!(
        aria_decoder::probe(&dark).is_err(),
        "an unmounted plugin's decoder still read its file"
    );
}

#[test]
fn a_panic_while_decoding_fails_the_plugin_and_unclaims_its_extension() {
    static PANICS: AtomicBool = AtomicBool::new(false);
    let dir = tempfile::tempdir().expect("a temp dir");
    let path = solid_file(dir.path(), "clip.ariapanic", 90);
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.panics", "ariapanic", &PANICS))
        .expect("mounts");

    let mut decoder = aria_decoder::open_video_decoder(&path, aria_decoder::OutputMode::Cpu)
        .expect("opens through the plugin");
    PANICS.store(true, Ordering::Relaxed);
    let err = decoder
        .frame_at(RationalTime::new(0, FPS_24))
        .expect_err("the panic is an error");
    assert!(
        err.to_string()
            .contains("decoder solid panicked in frame_at: solid frame decode failed"),
        "{err}"
    );

    let failures = ctx.failures();
    let failure = failures
        .iter()
        .find(|failure| failure.id == "test.panics")
        .expect("the plugin failed");
    assert!(
        failure.reason.contains("decoder solid panicked"),
        "{}",
        failure.reason
    );
    assert!(
        ctx.decoders().is_empty(),
        "a failed plugin's decoder is listed"
    );
    assert!(
        !aria_decoder::external_decoder_extensions().contains(&"ariapanic".to_owned()),
        "a failed plugin's decoder still claims its extension"
    );
}

#[test]
fn a_malformed_spec_is_refused() {
    static NEVER: AtomicBool = AtomicBool::new(false);
    let ctx = PluginContext::new();
    let spec = |id: &str, extensions: &[&str]| DecoderSpec {
        id: id.into(),
        name: "Solid".into(),
        extensions: extensions.iter().map(|ext| (*ext).to_owned()).collect(),
        priority: 0,
    };
    let solid = || Solid {
        panic_in_frames: &NEVER,
    };
    for (spec, reason) in [
        (spec("", &["ariarefused"]), "a decoder needs an id"),
        (spec("none", &[]), "none: a decoder reads no extension"),
        (spec("dot", &[".mov"]), "dot: the extension \".mov\""),
        (spec("empty", &[""]), "empty: the extension \"\""),
    ] {
        let err = ctx
            .register_decoder(spec, solid())
            .expect_err("a malformed spec is refused");
        assert!(err.to_string().contains(reason), "{err}");
    }
    let held = ctx
        .register_decoder(spec("twice", &["ariarefused"]), solid())
        .expect("registers");
    let err = ctx
        .register_decoder(spec("twice", &["ariarefused"]), solid())
        .expect_err("an id is registered once");
    assert!(
        err.to_string().contains("a decoder is registered as twice"),
        "{err}"
    );
    drop(held);
    assert!(
        !aria_decoder::external_decoder_extensions().contains(&"ariarefused".to_owned()),
        "a dropped registration still claims its extension"
    );
}
