//! A deferred renderer handed a device draws on that device.
//!
//! WHY: a frame sink that draws into another device's textures hands that
//! device over before the frame (`FrameSink::adopt_gpu`), and so does
//! [`Renderer::set_gpu`]. A deferred renderer that dropped the hand-over
//! would bring up a device of its own at the frame and composite into a
//! texture of another one. Pins that a hand-over before the first frame
//! leaves the renderer on the handed device, with its pipelines built.
//!
//! Not covered: a host with no device, where the first frame of a renderer
//! handed nothing fails with the bring-up error.

use aria_compositor::GpuContext;
use aria_models::{Generator, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::super::Renderer;

#[test]
fn a_deferred_renderer_draws_on_the_device_it_is_handed() {
    let mut renderer = Renderer::deferred();
    assert!(
        renderer.gpu().is_none(),
        "a deferred renderer holds no device"
    );
    assert_eq!(renderer.readback_count(), 0);
    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        return;
    };
    renderer.set_gpu(gpu.share());
    assert!(
        renderer.device.compositor().is_some(),
        "the hand-over built the pipelines on the handed device"
    );

    let rate = Rational::FPS_24;
    let mut project = Project::new("device", rate);
    let lane = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            lane,
            Generator::SolidColor {
                rgba: [0, 160, 0, 255],
            },
            TimeRange::at_rate(0, 12, rate),
        )
        .expect("a solid on an empty lane");
    let frame = renderer
        .render_frame_fit(&project, RationalTime::new(0, rate), 16, 9)
        .expect("a frame on the handed device");
    assert_eq!(frame.pixels[..4], [0, 160, 0, 255]);
    assert_eq!(renderer.readback_count(), 1, "one frame, one readback");
}
