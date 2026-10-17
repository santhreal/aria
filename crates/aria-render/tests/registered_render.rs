//! Effects and transitions registered at run time, drawn through the same
//! resolve and composite path a project frame takes in preview and export.
//!
//! Closes the class "a registered pass compiles but the frame path drops what
//! it needs": the parameter value, the clip-relative time, and the transition
//! progress each reach the shader, a generator draws over the transparent
//! solid that carries it, and a dropped registration falls back to what
//! `docs/spec/plugins.md` states for a missing plugin: an effect passes its
//! input through, a generator draws the `Missing generator` slate, and a
//! transition plays as a cut. Does not cover device-specific pipeline
//! refusals, which `aria-compositor`'s tests exercise.
//!
//! A separate test binary, so the registrations never reach a test that reads
//! the shipped catalog.

use aria_compositor::{EffectDefinition, PassRegistration, TransitionDefinition};
use aria_models::{
    ClipId, EffectParamKind, EffectParamSpec, EffectSpec, Generator, GraphPass, PassInput, Project,
    Rational, RationalTime, TimeRange, TrackKind, TransitionSpec,
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

fn register_time_gain(id: &str, time_dependent: bool) -> PassRegistration {
    aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            category: String::new(),
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
                ..Default::default()
            }],
            generator: false,
            stateful: false,
            time_dependent,
            ..Default::default()
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
    let registration = register_time_gain("test.render.time_gain", true);
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    // Starts at half a second, so a time measured from the sequence start reads
    // 0.5 seconds too late and fails the red channel.
    let clip = solid(&mut project, track, [0, 0, 255, 255], 12, 48);
    project
        .add_effect(clip, "test.render.time_gain")
        .expect("a registered effect is in the catalog");
    project
        .set_effect_param(clip, 0, 0, 0.25)
        .expect("sets gain");

    let Some(mut renderer) = renderer() else {
        return;
    };
    // 24 ticks at 24 fps, 12 after the clip starts: 0.5 seconds.
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [128, 64, 0, 255],
        "time 0.5, gain 0.25",
    );
    // 0.25 seconds into the clip.
    let image = renderer.render_frame(&project, rt(18)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [64, 64, 0, 255],
        "time 0.25, gain 0.25",
    );

    drop(registration);
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [0, 0, 255, 255],
        "a removed effect draws its input",
    );
}

/// An effect that does not declare `time_dependent` reads time 0 at every
/// tick, so its passes at equal parameters compare equal across frames and its
/// frames share a cache key.
#[test]
fn an_effect_without_time_dependence_reads_time_zero() {
    let registration = register_time_gain("test.render.timeless_gain", false);
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = solid(&mut project, track, [0, 0, 255, 255], 12, 48);
    project
        .add_effect(clip, "test.render.timeless_gain")
        .expect("a registered effect is in the catalog");
    project
        .set_effect_param(clip, 0, 0, 0.25)
        .expect("sets gain");

    let early = aria_render::resolve(&project, rt(18)).expect("resolves");
    let late = aria_render::resolve(&project, rt(24)).expect("resolves");
    assert_eq!(late.layers[0].effects[0].time, 0.0, "the pass reads time 0");
    assert_eq!(
        early.layers[0].effects, late.layers[0].effects,
        "a pass that does not read the time is equal at every tick"
    );

    let Some(mut renderer) = renderer() else {
        return;
    };
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(image.pixel(960, 540), [0, 64, 0, 255], "time 0, gain 0.25");
    drop(registration);
}

/// The progress through a registered transition reaches its shader, and
/// dropping the registration plays the junction as a cut.
#[test]
fn a_registered_transition_reads_its_progress() {
    let registration = aria_compositor::register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: "test.render.progress".into(),
            label: "Progress".into(),
            ..Default::default()
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
    project
        .set_transition_duration(left, 24)
        .expect("sets duration");

    let Some(mut renderer) = renderer() else {
        return;
    };
    // The window spans ticks 12 to 36; tick 24 is halfway.
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(image.pixel(960, 540), [128, 0, 0, 255], "progress 0.5");

    drop(registration);
    let image = renderer.render_frame(&project, rt(23)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [0, 255, 0, 255],
        "a removed transition shows the outgoing clip before the cut",
    );
    let image = renderer.render_frame(&project, rt(24)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [0, 0, 255, 255],
        "a removed transition shows the incoming clip from the cut",
    );
}

/// Green is the horizontal position in the frame.
const RAMP_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(0.0, in.uv.x, 0.0, 1.0);
}";

/// A generator clip is a transparent solid carrying the effect: the effect
/// draws over the whole frame, and with the registration dropped the clip
/// draws the `Missing generator` slate, whose background covers the centre.
#[test]
fn a_generator_draws_over_the_transparent_solid_that_carries_it() {
    let registration = aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: "test.render.ramp".into(),
            category: String::new(),
            label: "Ramp".into(),
            params: Vec::new(),
            generator: true,
            stateful: false,
            ..Default::default()
        },
        passes: vec![RAMP_FS.into()],
    })
    .expect("the generator registers");
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = solid(&mut project, track, [0, 0, 0, 0], 0, 24);
    project
        .add_effect(clip, "test.render.ramp")
        .expect("a registered generator is in the catalog");

    let Some(mut renderer) = renderer() else {
        return;
    };
    let image = renderer.render_frame(&project, rt(12)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [0, 128, 0, 255],
        "the frame's centre",
    );
    assert_near(image.pixel(480, 540), [0, 64, 0, 255], "a quarter across");

    drop(registration);
    let image = renderer.render_frame(&project, rt(12)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [38, 38, 38, 255],
        "a removed generator draws the slate's background",
    );
}

/// The referenced layer's colour, opaque.
const LAYER_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let l = textureSample(layer_matte, aria_sampler, in.uv);
    return vec4<f32>(l.rgb, 1.0);
}";

/// Register an effect `id` whose one pass draws the clip its `matte` layer
/// parameter references.
fn register_layer_read(id: &str) -> PassRegistration {
    aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            label: "Layer Read".into(),
            params: vec![EffectParamSpec {
                name: "matte".into(),
                label: "Matte".into(),
                kind: EffectParamKind::Layer,
                ..Default::default()
            }],
            ..Default::default()
        },
        passes: vec![
            GraphPass::fragment(LAYER_FS).reading([PassInput::Source, PassInput::layer("matte")]),
        ],
    })
    .expect("the effect registers")
}

/// Append the effect `id` to `reader`, its layer parameter on `referenced`.
fn read_layer(project: &mut Project, id: &str, reader: ClipId, referenced: ClipId) {
    let index = project
        .timeline()
        .clip(reader)
        .expect("reader")
        .effects
        .len();
    project
        .add_effect(reader, id)
        .expect("a registered effect is in the catalog");
    let mut fx = project.timeline().clip(reader).expect("reader").effects[index].clone();
    fx.set_layer_param(0, Some(referenced))
        .expect("a layer slot");
    project
        .set_effect_instance(reader, index, fx)
        .expect("the reference is accepted");
}

/// An effect pass reading a layer parameter draws the frame of the clip the
/// parameter references at the same instant. The reading clip is on top: with
/// the registration dropped it shows its own colour, so the referenced colour
/// cannot come from the clip beneath showing through. A reference closing a
/// cycle is refused by the model.
#[test]
fn a_layer_parameter_reads_the_referenced_clip() {
    let id = "test.render.layer_read";
    let registration = register_layer_read(id);
    let mut project = Project::new("p", FPS_24);
    let below = project.add_track(TrackKind::Sticker, "S1");
    let above = project.add_track(TrackKind::Sticker, "S2");
    let referenced = solid(&mut project, below, [10, 200, 30, 255], 0, 24);
    let reader = solid(&mut project, above, [220, 20, 20, 255], 0, 24);
    read_layer(&mut project, id, reader, referenced);
    project
        .add_effect(referenced, id)
        .expect("a registered effect is in the catalog");
    let mut back = project
        .timeline()
        .clip(referenced)
        .expect("referenced")
        .effects[0]
        .clone();
    back.set_layer_param(0, Some(reader)).expect("a layer slot");
    let refused = project
        .set_effect_instance(referenced, 0, back)
        .expect_err("a reference closing a cycle is refused");
    assert!(refused.to_string().contains("cycle"), "{refused}");
    project
        .remove_effect(referenced, 0)
        .expect("drops the unset effect");

    let Some(mut renderer) = renderer() else {
        return;
    };
    let image = renderer.render_frame(&project, rt(12)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [10, 200, 30, 255],
        "the referenced clip's colour",
    );

    drop(registration);
    let image = renderer.render_frame(&project, rt(12)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [220, 20, 20, 255],
        "the reading clip draws on top",
    );
}

/// A referenced clip's own layer references resolve before it, though no
/// visible lane draws either clip: the reader on top shows the colour two
/// references down.
#[test]
fn a_referenced_clip_reads_its_own_references_on_hidden_lanes() {
    let id = "test.render.layer_chain";
    let registration = register_layer_read(id);
    let mut project = Project::new("p", FPS_24);
    let source_lane = project.add_track(TrackKind::Sticker, "S1");
    let middle_lane = project.add_track(TrackKind::Sticker, "S2");
    let top_lane = project.add_track(TrackKind::Sticker, "S3");
    let source = solid(&mut project, source_lane, [10, 200, 30, 255], 0, 24);
    let middle = solid(&mut project, middle_lane, [50, 50, 200, 255], 0, 24);
    let reader = solid(&mut project, top_lane, [220, 20, 20, 255], 0, 24);
    read_layer(&mut project, id, middle, source);
    read_layer(&mut project, id, reader, middle);
    for lane in [source_lane, middle_lane] {
        project
            .timeline_mut()
            .track_mut(lane)
            .expect("lane")
            .enabled = false;
    }

    let Some(mut renderer) = renderer() else {
        return;
    };
    let image = renderer.render_frame(&project, rt(12)).expect("render");
    assert_near(
        image.pixel(960, 540),
        [10, 200, 30, 255],
        "the colour two references down",
    );
    drop(registration);
}
