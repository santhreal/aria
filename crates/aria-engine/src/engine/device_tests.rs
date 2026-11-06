//! An engine brings up its GPU device on its first frame, and keeps it.
//!
//! WHY: `Engine::new` and `Engine::with_project` brought up a headless device
//! when the engine was built: half a second of driver work, paid by every
//! session that only edits and saves (`aria new`, a scripted edit, every state
//! test in `aria-ui`). Closes that class at the one constructor the others
//! delegate to: an engine that has drawn no frame holds no device. The other
//! direction is pinned too: the first frame brings the device up and later
//! frames draw on it instead of building the pipelines again.
//!
//! Not covered: a host with no device, where the first frame and every later
//! one fail with the bring-up error. That needs a host with no adapter.

use aria_commands::{Command, EditCommand};
use aria_models::{Generator, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::{Engine, EngineConfig};

const RATE: Rational = Rational::FPS_24;

/// Red for the first half second, blue for the second, so the two frames the
/// test draws are different frames.
fn two_colors() -> Project {
    let mut project = Project::new("device", RATE);
    let lane = project.add_track(TrackKind::Sticker, "S1");
    for (start, rgba) in [(0, [200, 0, 0, 255]), (12, [0, 0, 200, 255])] {
        project
            .add_generated(
                lane,
                Generator::SolidColor { rgba },
                TimeRange::at_rate(start, 12, RATE),
            )
            .expect("a solid on an empty stretch of the lane");
    }
    project
}

#[test]
fn an_engine_brings_up_its_device_on_the_first_frame() {
    let mut engine =
        Engine::with_project(EngineConfig::default(), two_colors()).expect("an engine");
    engine
        .apply(Command::Edit(EditCommand::SetProjectName {
            name: "edited".to_string(),
        }))
        .expect("a rename");
    assert!(
        engine.renderer.gpu().is_none(),
        "an engine that has only edited holds no device"
    );
    assert!(
        Engine::new(EngineConfig::default())
            .expect("an empty engine")
            .renderer
            .gpu()
            .is_none(),
        "an empty engine holds no device"
    );

    let red = engine
        .get_frame_fit(RationalTime::new(0, RATE), 16, 9)
        .expect("the first frame");
    assert_eq!(red.pixels[..4], [200, 0, 0, 255], "the first frame");
    assert!(
        engine.renderer.gpu().is_some(),
        "the first frame brought the device up"
    );
    let first = engine.renderer.readback_count();
    assert!(first > 0, "the first frame read its pixels back");

    let blue = engine
        .get_frame_fit(RationalTime::new(12, RATE), 16, 9)
        .expect("the second frame");
    assert_eq!(blue.pixels[..4], [0, 0, 200, 255], "the second frame");
    assert_eq!(
        engine.renderer.readback_count(),
        2 * first,
        "the second frame drew on the compositor the first frame built"
    );
}
