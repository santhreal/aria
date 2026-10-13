//! Effects and transitions registered at run time, drawn through the same
//! resolve and composite path a project frame takes in preview and export.
//!
//! Closes the class "a registered pass compiles but the frame path drops what
//! it needs": the parameter value, the clip-relative time, and the transition
//! progress each reach the shader, and a dropped registration falls back to
//! the documented passthrough and crossfade. Does not cover device-specific
//! pipeline refusals, which `aria-compositor`'s tests exercise.
//!
//! A separate test binary, so the registrations never reach a test that reads
//! the shipped catalog.

use aria_compositor::{EffectDefinition, PassRegistration, TransitionDefinition};
use aria_models::{
    EffectParamKind, EffectParamSpec, EffectSpec, Generator, Project, Rational, RationalTime,
    TimeRange, TrackKind, TransitionSpec,
};
use aria_render::Renderer;

const FPS_24: Rational = Rational::FPS_24;

/// Red is the clip-relative time, green the `gain` parameter.
const TIME_GAIN_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(fract(aria.time), param_gain(), 0.0, 1.0);
}";

/// Red is the transition progress.
const PROGRESS_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(aria.progress, 0.0, 0.0, 1.0);
}";

fn rt(value: i64) -> RationalTime {
    RationalTime::new(value, FPS_24)
}

fn renderer() -> Option<Renderer> {
    match Renderer::new_headless() {
        Ok(renderer) => Some(renderer),
        Err(err) => {
            eprintln!("skipping: no headless GPU available: {err}");
            None
        }
    }
}

fn register_time_gain(id: &str) -> PassRegistration {
    aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            label: "Time Gain".into(),
            params: vec![EffectParamSpec {
                name: "gain".into(),
                label: "Gain".into(),
                kind: EffectParamKind::Scalar,
                default: 1.0,
                min: 0.0,
                max: 1.0,
                default_color: [0; 4],
                default_vec2: [0.0; 2],
            }],
        },
        passes: vec![TIME_GAIN_FS.into()],
    })
    .expect("the effect registers")
}

fn solid(
    project: &mut Project,
    track: aria_models::TrackId,
    rgba: [u8; 4],
    start: i64,
    len: i64,
) -> aria_models::ClipId {
    project
        .add_generated(
            track,
            Generator::SolidColor { rgba },
            TimeRange::at_rate(start, len, FPS_24),
        )
        .expect("adds a solid")
}

fn assert_near(actual: [u8; 4], expected: [u8; 4], what: &str) {
    for (a, e) in actual.iter().zip(expected) {
        assert!(
            a.abs_diff(e) <= 3,
            "{what}: got {actual:?}, expected ~{expected:?}"
        );
    }
}

/// The parameter and the time since the clip started reach the shader of a
/// registered effect, and dropping the registration draws the clip unchanged.
#[test]
fn a_registered_effect_reads_its_parameter_and_clip_time() {
    let registration = register_time_gain("test.render.time_gain");
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    // Starts at half a second, so a time measured from the sequence start reads
    // 0.5 seconds too late and fails the red channel.
    let clip = solid(&mut project, track, [0, 0, 255, 255], 12, 48);
    project
        .add_effect(clip, "test.render.time_gain")
        .expect("a registered effect is in the catalog");
    project.set_effect_param(clip, 0, 0, 0.25).expect("sets gain");

    let Some(mut renderer) = renderer() else {
        return;
    };
    // 24 ticks at 24 fps, 12 after the clip starts: 0.5 seconds.
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(image.pixel(960, 540), [128, 64, 0, 255], "time 0.5, gain 0.25");
    // 0.25 seconds into the clip.
    let image = renderer.render_frame(&project, rt(18)).expect("render");
    assert_near(image.pixel(960, 540), [64, 64, 0, 255], "time 0.25, gain 0.25");

    drop(registration);
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [0, 0, 255, 255],
        "a removed effect draws its input",
    );
}

/// The progress through a registered transition reaches its shader, and
/// dropping the registration draws the crossfade instead.
#[test]
fn a_registered_transition_reads_its_progress() {
    let registration = aria_compositor::register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: "test.render.progress".into(),
            label: "Progress".into(),
        },
        shader: PROGRESS_FS.into(),
    })
    .expect("the transition registers");
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let left = solid(&mut project, track, [0, 255, 0, 255], 0, 24);
    solid(&mut project, track, [0, 0, 255, 255], 24, 24);
    project
        .add_transition(left, "test.render.progress")
        .expect("a registered transition is in the catalog");
    project.set_transition_duration(left, 24).expect("sets duration");

    let Some(mut renderer) = renderer() else {
        return;
    };
    // The window spans ticks 12 to 36; tick 24 is halfway.
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(image.pixel(960, 540), [128, 0, 0, 255], "progress 0.5");

    drop(registration);
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [0, 128, 128, 255],
        "a removed transition crossfades",
    );
}
