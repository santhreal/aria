//! Fixtures and one sample scene per layer topology.
//!
//! The assets and the scenes that describe them live here; the sweep that
//! renders them and the assertions it checks live in [`super`].

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use aria_core::{Rational, RationalTime, RgbaImage};
use aria_models::{
    BlendMode, CanvasAspect, CanvasSettings, Generator, MatteSource, MediaId, MediaSource, Project,
    TimeRange, TrackKind, sticker_catalog,
};
use aria_shapes::{BezierPath, PathPoint, SdfParams};
use aria_text::{TextAlign, TextStyle, TextVerticalAlign};

use crate::scene::{LayerSource, ResolvedPass, Scene, SceneLayer, SizeSpec};
use crate::{Renderer, export_to_file};

use super::{Expect, H, INK_ALPHA, Topology, W};

/// The H.264 clip the `media` topology decodes, encoded once per test process.
/// Its flat fill survives the YUV round trip within a few counts.
///
/// A host whose encoder refuses leaves the file missing, which fails the media
/// proof with the decoder's own error instead of poisoning every other proof in
/// the sweep.
static VIDEO_CLIP: LazyLock<PathBuf> = LazyLock::new(encode_flat_clip);

/// Encoded size of [`VIDEO_CLIP`]. Well above the hardware encoders' minimum
/// frame size, which a canvas-sized 64×64 clip is not.
const VIDEO_SIZE: [u32; 2] = [320, 192];

/// The number of frames [`VIDEO_CLIP`] holds.
const VIDEO_FRAMES: i64 = 2;

/// The colour [`VIDEO_CLIP`] is filled with.
const VIDEO_FILL: [u8; 4] = [40, 80, 160, 255];

/// Minimal Lottie: 100×100, an 80×80 red rectangle centred on a transparent
/// field, so both the ink and the hole around it are known.
const LOTTIE_RED_RECT: &str = r#"{
  "v": "5.7.0", "fr": 30, "ip": 0, "op": 60, "w": 100, "h": 100,
  "layers": [{
    "ty": 4, "ip": 0, "op": 60, "st": 0,
    "ks": {"o": {"a": 0, "k": 100}, "p": {"a": 0, "k": [50, 50]},
           "a": {"a": 0, "k": [0, 0, 0]}, "s": {"a": 0, "k": [100, 100, 100]},
           "r": {"a": 0, "k": 0}},
    "shapes": [
      {"ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [80, 80]}, "r": {"a": 0, "k": 0}},
      {"ty": "fl", "c": {"a": 0, "k": [1, 0, 0, 1]}, "o": {"a": 0, "k": 100}}
    ]
  }]
}"#;

fn encode_flat_clip() -> PathBuf {
    let fps = Rational::FPS_30;
    let mut project = Project::new("realize-conformance-media", fps);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [0, 0, 0],
        resolution: Some(VIDEO_SIZE),
    });
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor { rgba: VIDEO_FILL },
            TimeRange::at_rate(0, VIDEO_FRAMES, fps),
        )
        .expect("place the solid generator");

    // One fixed path: the pid in the name made this fixture one file per run,
    // and the fixture is re-encoded from the same project every time anyway.
    let path = std::env::temp_dir().join("aria_realize_conformance.mp4");
    let mut renderer = Renderer::new_headless().expect("headless renderer for the media fixture");
    if let Err(e) = export_to_file(&mut renderer, &project, &path) {
        eprintln!("the media fixture could not be encoded ({e}); the media proof will fail");
    }
    path
}

/// The on-disk assets and media pool every sample draws from.
pub(super) struct Fixtures {
    _dir: tempfile::TempDir,
    pub(super) project: Project,
    still: MediaId,
    video: MediaId,
    video_time: RationalTime,
    lottie: String,
}

impl Fixtures {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");

        let still_path = dir.path().join("still.png");
        write_solid_png(&still_path, 32, 32, [0, 0, 255, 255]);
        let lottie = dir.path().join("anim.json");
        std::fs::write(&lottie, LOTTIE_RED_RECT).expect("write the lottie fixture");

        let mut project = Project::new("realize-conformance", Rational::FPS_30);
        let still = project.add_media(MediaSource::image(&still_path, 32, 32));
        let video = project.add_media(MediaSource::new(
            VIDEO_CLIP.as_path(),
            VIDEO_SIZE[0],
            VIDEO_SIZE[1],
            Rational::FPS_30,
            VIDEO_FRAMES,
            false,
        ));
        Self {
            _dir: dir,
            project,
            still,
            video,
            video_time: RationalTime::new(0, Rational::FPS_30),
            lottie: lottie.to_string_lossy().into_owned(),
        }
    }
}

fn write_solid_png(path: &Path, width: u32, height: u32, rgba: [u8; 4]) {
    let file = std::fs::File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    let flat: Vec<u8> = rgba
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    writer.write_image_data(&flat).expect("png data");
}

// ---------------------------------------------------------------------------
// Samples
// ---------------------------------------------------------------------------

/// A centred layer with no look attached: the neutral carrier every sample
/// varies from.
pub(super) fn placed(source: LayerSource, size: SizeSpec) -> SceneLayer {
    SceneLayer {
        clip: None,
        source,
        center: [W as f32 * 0.5, H as f32 * 0.5],
        anchor_point: [0.5, 0.5],
        size,
        rotation: 0.0,
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    }
}

pub(super) fn full_canvas(source: LayerSource) -> SceneLayer {
    placed(source, SizeSpec::Fixed([W as f32, H as f32]))
}

/// A 32 px quad centred on the canvas, leaving a 16 px border to probe.
fn quad(source: LayerSource) -> SceneLayer {
    placed(source, SizeSpec::Fixed([32.0, 32.0]))
}

pub(super) const CENTER: [u32; 2] = [W / 2, H / 2];
/// A corner square well outside a 32 px centred quad.
const OUTSIDE: [u32; 4] = [0, 0, 7, 7];
/// The centred 32 px quad's own footprint.
const QUAD_RECT: [u32; 4] = [W / 2 - 15, H / 2 - 15, W / 2 + 14, H / 2 + 14];

/// The text every text sample shapes, and the style it shapes with.
pub(super) fn text_style() -> TextStyle {
    TextStyle {
        color: [255, 255, 0, 255],
        align: TextAlign::Center,
        vertical_align: TextVerticalAlign::Middle,
        ..TextStyle::new(28.0)
    }
}

pub(super) const TEXT_CONTENT: &str = "HI";

/// One sample per topology, with the pixels it owes.
///
/// `renderer` is borrowed so text and sticker expectations come from the very
/// rasterizer and the very catalog bytes the render will use, instead of from
/// numbers typed here that art can drift away from.
pub(super) fn topologies(fx: &Fixtures, renderer: &mut Renderer) -> Vec<Topology> {
    let mut all = vec![
        // Solid: a flat fill is the one topology whose exact output is a
        // constant, so it also pins the quad's footprint.
        Topology {
            layer: quad(LayerSource::Solid([255, 0, 0, 255])),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [255, 0, 0, 255],
                    tol: 2,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
        // Shape: an SDF ellipse inscribed in the padded quad.
        Topology {
            layer: quad(LayerSource::Shape {
                params: SdfParams::Ellipse,
                fill: [0, 255, 0, 255],
                stroke: None,
                pad: 2.0,
            }),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [0, 255, 0, 255],
                    tol: 2,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
        // PathShape: a CPU-rasterized closed square, placed at its raster size.
        Topology {
            layer: placed(
                LayerSource::PathShape {
                    path: square_path(24.0),
                    fill: [0, 128, 255, 255],
                    stroke: None,
                    raster_scale: 1.0,
                },
                SizeSpec::BitmapScaled([1.0, 1.0]),
            ),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [0, 128, 255, 255],
                    tol: 2,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
        // Still: a decoded PNG.
        Topology {
            layer: quad(LayerSource::Still { media: fx.still }),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [0, 0, 255, 255],
                    tol: 2,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
        // Media: a decoded video frame. The tolerance is the YUV round trip,
        // not the placement.
        Topology {
            layer: quad(LayerSource::Media {
                media: fx.video,
                source_time: fx.video_time,
            }),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: VIDEO_FILL,
                    tol: 12,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
        // Lottie: the fixture's 80×80 red rect inside a transparent 100×100
        // field, so the hole around the ink is part of the proof.
        Topology {
            layer: quad(LayerSource::Lottie {
                path: fx.lottie.clone(),
                local_time: 0.0,
            }),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [255, 0, 0, 255],
                    tol: 3,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
        sticker_topology(),
        text_topology(renderer),
        // CanvasPass: geometry-free, so it is proven by what it does to a
        // canvas that already has pixels on it.
        Topology {
            layer: SceneLayer {
                effects: vec![ResolvedPass::new(
                    "color_overlay",
                    // Catalog order: colour rgba, offset xy, amount.
                    vec![0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0],
                )],
                ..full_canvas(LayerSource::CanvasPass)
            },
            under: vec![full_canvas(LayerSource::Solid([255, 0, 0, 255]))],
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [0, 255, 0, 255],
                    tol: 3,
                },
                Expect::Pixel {
                    at: [4, 4],
                    rgba: [0, 255, 0, 255],
                    tol: 3,
                },
            ],
        },
        // Transition: both sides must be realized, so the midpoint of a
        // crossfade between two opaque primaries is checked, not either side.
        Topology {
            layer: full_canvas(LayerSource::Transition {
                outgoing: Box::new(full_canvas(LayerSource::Solid([255, 0, 0, 255]))),
                incoming: Box::new(full_canvas(LayerSource::Solid([0, 0, 255, 255]))),
                transition_id: "crossfade".into(),
                progress: 0.5,
            }),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [128, 0, 128, 255],
                    tol: 4,
                },
                Expect::Pixel {
                    at: [4, 4],
                    rgba: [128, 0, 128, 255],
                    tol: 4,
                },
            ],
        },
        nest_topology(),
        // Matte: a full-canvas content cut down to the stencil's 32 px quad,
        // so both the kept and the cut region are checked.
        Topology {
            layer: full_canvas(LayerSource::Matte {
                layers: Box::new([
                    full_canvas(LayerSource::Solid([255, 0, 0, 255])),
                    quad(LayerSource::Solid([255, 255, 255, 255])),
                ]),
                read: MatteSource::Alpha,
                invert: false,
                softness: 0.0,
            }),
            under: Vec::new(),
            expect: vec![
                Expect::Pixel {
                    at: CENTER,
                    rgba: [255, 0, 0, 255],
                    tol: 2,
                },
                Expect::Empty { rect: OUTSIDE },
            ],
        },
    ];
    all.sort_by_key(|t| t.kind());
    all
}

/// The nest sample: a canvas-sized sequence frame holding a red left half and
/// a blue right half, placed as the 32 px quad, so both inner layers must
/// reach the canvas, each on its own side of the quad.
fn nest_topology() -> Topology {
    let half = |x: f32, fill: [u8; 4]| SceneLayer {
        center: [x, H as f32 * 0.5],
        ..placed(
            LayerSource::Solid(fill),
            SizeSpec::Fixed([W as f32 * 0.5, H as f32]),
        )
    };
    let mut scene = Scene::empty(W, H, [0, 0, 0, 0]);
    scene.layers = vec![
        half(W as f32 * 0.25, [255, 0, 0, 255]),
        half(W as f32 * 0.75, [0, 0, 255, 255]),
    ];
    Topology {
        layer: quad(LayerSource::Nest {
            scene: Box::new(scene),
            frame: [0.0, 0.0, 1.0, 1.0],
        }),
        under: Vec::new(),
        expect: vec![
            Expect::Pixel {
                at: [CENTER[0] - 8, CENTER[1]],
                rgba: [255, 0, 0, 255],
                tol: 2,
            },
            Expect::Pixel {
                at: [CENTER[0] + 8, CENTER[1]],
                rgba: [0, 0, 255, 255],
                tol: 2,
            },
            Expect::Empty { rect: OUTSIDE },
        ],
    }
}

/// A closed square `side` px on a side, in path-local pixels.
pub(super) fn square_path(side: f32) -> BezierPath {
    BezierPath {
        points: vec![
            PathPoint::corner([0.0, 0.0]),
            PathPoint::corner([side, 0.0]),
            PathPoint::corner([side, side]),
            PathPoint::corner([0.0, side]),
        ],
        closed: true,
    }
}

/// The sticker sample, with its ink budget taken from the catalog art itself.
fn sticker_topology() -> Topology {
    let spec = sticker_catalog()
        .first()
        .expect("the sticker catalog ships at least one asset");
    let frames = aria_decoder::decode_animation(&spec.bytes).expect("decode the catalog art");
    let art = &frames.first().expect("catalog art has a frame").image;
    let source_coverage = coverage_of(art);
    assert!(
        source_coverage > 0.1,
        "catalog art '{}' is nearly empty ({source_coverage:.3} inked); it cannot prove a draw",
        spec.id
    );
    Topology {
        layer: quad(LayerSource::Sticker {
            asset: spec.id.clone(),
            local_time: 0.0,
        }),
        under: Vec::new(),
        expect: vec![
            // Minification without mipmaps samples sparsely, so the composited
            // silhouette is a fraction of the art's own, never all of it.
            Expect::Coverage {
                rect: QUAD_RECT,
                min: source_coverage * 0.6,
            },
            Expect::Empty { rect: OUTSIDE },
        ],
    }
}

/// The text sample, with its ink budget and footprint taken from the very
/// rasterizer that will draw it.
fn text_topology(renderer: &mut Renderer) -> Topology {
    let style = text_style();
    let image = renderer.text.rasterize(TEXT_CONTENT, &style);
    assert!(
        image.width > 0 && image.height > 0,
        "no font on this host shaped '{TEXT_CONTENT}'; the text topology cannot be proven"
    );
    assert!(
        image.width < W && image.height < H,
        "text raster {}×{} does not fit the conformance canvas",
        image.width,
        image.height
    );
    let ink = centered_rect(image.width, image.height);
    Topology {
        layer: placed(
            LayerSource::Text {
                content: TEXT_CONTENT.into(),
                style,
                animation: None,
            },
            SizeSpec::BitmapScaled([1.0, 1.0]),
        ),
        under: Vec::new(),
        expect: vec![
            Expect::Coverage {
                rect: ink,
                min: coverage_of(&image) * 0.8,
            },
            Expect::InkColor {
                rect: ink,
                rgb: [255, 255, 0],
                tol: 3,
            },
        ],
    }
}

/// The canvas rect a `width`×`height` bitmap centred on the canvas occupies,
/// inset by a pixel so bilinear edge softening is not read as missing ink.
fn centered_rect(width: u32, height: u32) -> [u32; 4] {
    let x0 = (W - width) / 2 + 1;
    let y0 = (H - height) / 2 + 1;
    [x0, y0, x0 + width - 3, y0 + height - 3]
}

/// Fraction of pixels with ink.
fn coverage_of(image: &RgbaImage) -> f32 {
    let inked = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > INK_ALPHA)
        .count();
    inked as f32 / (image.width * image.height) as f32
}
