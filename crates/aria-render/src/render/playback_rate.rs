//! A measured playback run: what rate the real render path keeps, and how many
//! frames it misses, for a sequence whose cost is known.
//!
//! Frame cost was only ever observable one frame at a time
//! ([`FrameStats`](crate::FrameStats)) and against a fixed millisecond
//! threshold (`SLOW_FRAME_LOG_MS`). Neither states whether preview keeps up
//! with the sequence's own rate, so a
//! regression that doubled per-frame cost stayed invisible as long as it stayed
//! under 150 ms.
//!
//! What is measured here is the preview entry point
//! ([`Renderer::render_frame_into_with`]) over consecutive frames of one
//! project, timed the way playback times them: wall clock around the call that
//! produces the picture, into a buffer that is reused rather than reallocated,
//! with the first frames excluded because the first render of a session pays
//! for pipeline and decoder setup that no later frame pays again.
//!
//! The threshold is a rate, not a duration, and it is relaxed for a software
//! adapter: `lavapipe` rasterizes on the CPU at a fraction of a GPU's rate, and
//! failing on a machine with no GPU would only teach everyone to ignore this
//! test. The report is printed either way, because the number is the point.

use std::time::Instant;

use aria_core::{Rational, RationalTime};
use aria_models::{CanvasAspect, CanvasSettings, Generator, Project, TimeRange, TrackKind};

use crate::{FrameSink, PlaybackMeter, ResolveOverrides};

use super::{Renderer, SeekPolicy};

/// The measured sequence: small enough that the cost is the pipeline's rather
/// than the pixel count's, big enough that a per-frame regression shows.
const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

/// Frames measured, and frames rendered before measuring starts.
const FRAMES: i64 = 120;
const WARM_UP: i64 = 4;

/// Frames a hardware adapter may miss and still pass. Not zero: the run shares
/// a machine with the rest of the suite, and one descheduled frame in a hundred
/// is scheduling noise, not a render regression.
const HARDWARE_TOLERANCE: f64 = 0.05;

/// What a software rasterizer must still manage, as a share of the target. A
/// CPU rasterizer is an order of magnitude off a GPU; below this it is broken
/// rather than slow.
const SOFTWARE_FLOOR: f64 = 0.1;

/// One reusable buffer for every frame of the run.
///
/// Playback hands its pixels to a surface it already owns, so a measurement
/// that allocated per frame would be measuring the allocator. The buffer grows
/// once, on the first frame, and is written in place after that.
struct Buffer {
    pixels: Vec<u8>,
    size: (u32, u32),
}

impl FrameSink for Buffer {
    fn pixels(&mut self, width: u32, height: u32) -> &mut [u8] {
        let needed = (width as usize) * (height as usize) * 4;
        if self.pixels.len() != needed {
            self.pixels.resize(needed, 0);
        }
        self.size = (width, height);
        &mut self.pixels
    }
}

/// The measured project: a solid generator over [`FRAMES`] frames at 24 fps.
///
/// A generator rather than a media clip on purpose: decode cost belongs to the
/// decoder's own benchmark, and mixing it in here would make this test report
/// the speed of whatever codec the host happens to have.
fn sequence(rate: Rational) -> Project {
    let mut project = Project::new("playback-rate", rate);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [0, 0, 0],
        resolution: Some([WIDTH, HEIGHT]),
    });
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [30, 90, 200, 255],
            },
            TimeRange::at_rate(0, FRAMES, rate),
        )
        .expect("place the measured generator");
    project
}

/// **Playback is measured against the sequence's own rate, and the frames it
/// misses are counted.**
///
/// The class this closes: per-frame cost had no target. A change that made every
/// frame twice as expensive passed every existing test, because each frame was
/// still well inside the 150 ms slow-frame log and no test compared a run
/// against a rate at all.
///
/// Skips only when the host has no GPU adapter — the same skip the conformance
/// sweep allows — and relaxes to [`SOFTWARE_FLOOR`] on a CPU rasterizer.
#[test]
fn playback_rate_report_measures_a_known_sequence() {
    let Ok(mut renderer) = Renderer::new_headless() else {
        eprintln!("skipping the playback rate report: no headless GPU");
        return;
    };
    let rate = Rational::FPS_24;
    let project = sequence(rate);
    let mut buffer = Buffer {
        pixels: Vec::new(),
        size: (0, 0),
    };

    // The first frames of a session build pipelines and caches. Playback pays
    // that once; a rate measured over it would report the startup, not the run.
    for frame in 0..WARM_UP {
        render_one(&mut renderer, &project, rate, frame, &mut buffer);
    }

    let mut meter = PlaybackMeter::at_rate(rate);
    for frame in WARM_UP..FRAMES {
        let started = Instant::now();
        render_one(&mut renderer, &project, rate, frame, &mut buffer);
        meter.frame(started.elapsed());
    }

    let report = meter.report();
    let software = renderer.gpu.is_software();
    println!(
        "playback rate report: {} ({})",
        report.label(),
        if software { "software" } else { "hardware" }
    );

    assert_eq!(
        report.frames,
        u32::try_from(FRAMES - WARM_UP).expect("the frame count fits"),
        "every frame after the warm-up must be measured"
    );
    assert_eq!(
        buffer.size,
        (WIDTH, HEIGHT),
        "the run rendered a canvas it was not asked for"
    );
    assert!(
        report.worst_ms > 0.0,
        "a frame that takes no time at all was not rendered"
    );

    if software {
        assert!(
            report.render_fps >= report.target_fps * SOFTWARE_FLOOR,
            "a software rasterizer must still manage {:.0}% of {:.0} fps: {}",
            SOFTWARE_FLOOR * 100.0,
            report.target_fps,
            report.label()
        );
    } else {
        assert!(
            report.keeps_up(HARDWARE_TOLERANCE),
            "preview must hold the sequence's rate with at most {:.0}% of frames late: {}",
            HARDWARE_TOLERANCE * 100.0,
            report.label()
        );
    }
}

/// One frame through the preview entry point, exactly as the editor asks for it.
fn render_one(
    renderer: &mut Renderer,
    project: &Project,
    rate: Rational,
    frame: i64,
    buffer: &mut Buffer,
) {
    renderer
        .render_frame_into_with(
            project,
            RationalTime::new(frame, rate),
            ResolveOverrides::default(),
            SeekPolicy::Exact,
            buffer,
        )
        .unwrap_or_else(|err| panic!("frame {frame} of the measured sequence: {err}"));
}

/// The warm-up is excluded, not skipped: a run that measured it would report the
/// session's first frame as playback's rate, and the first frame of a GPU
/// session costs pipeline creation.
#[test]
fn the_warm_up_is_smaller_than_the_run_it_precedes() {
    const {
        assert!(
            WARM_UP > 0,
            "a session's first frame is not a playback frame"
        );
        assert!(
            FRAMES - WARM_UP >= 100,
            "a rate measured over fewer than a hundred frames is one frame's noise"
        );
    }
}
