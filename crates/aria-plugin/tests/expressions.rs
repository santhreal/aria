//! `aria.expressions` drivers evaluated through a real project.
//!
//! Closes the class "an expression that evaluates alone does not drive what
//! the renderer draws": a reference to another clip resolves by the clip's
//! name after that clip's own driver, composition values come from the
//! project, a failing expression leaves the keyframed value and records a
//! fault with the failing position, and a cycle written through references
//! is refused when attached and stays a cycle at evaluation although the
//! language rewrites the error it is handed. The language itself is tested in
//! `src/expressions/tests.rs`.

use std::sync::{Arc, LazyLock};

use parking_lot::{Mutex, MutexGuard};

use aria_models::{
    ClipId, ClipParam, DriverCode, MediaSource, ParamValue, Project, Rational, RationalTime,
    TimeRange, TrackKind,
};
use aria_plugin::{EXPRESSIONS_LANGUAGE, Expressions};
use aria_render::{
    DriverFrame, DriverLanguageRegistration, check_driver, driver_fault, register_driver_language,
};

const FPS: Rational = Rational::FPS_24;

static REGISTERED: LazyLock<DriverLanguageRegistration> = LazyLock::new(|| {
    register_driver_language(EXPRESSIONS_LANGUAGE, Arc::new(Expressions))
        .unwrap_or_else(|err| panic!("{err}"))
});
/// Faults are process-wide and keyed by clip id, which every project in this
/// file numbers alike.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    LazyLock::force(&REGISTERED);
    SERIAL.lock()
}

/// A clip on V1 and a clip named "Logo" on V2, both on frames 0..48.
fn two_clips() -> (Project, ClipId, ClipId) {
    let mut project = Project::new("expressions", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/expressions.mp4",
        640,
        360,
        FPS,
        600,
        false,
    ));
    let mut place = |lane: &str| {
        let track = project.add_track(TrackKind::Video, lane);
        project
            .add_clip(
                track,
                media,
                TimeRange::at_rate(0, 48, FPS),
                RationalTime::new(0, FPS),
            )
            .unwrap_or_else(|err| panic!("{err}"))
    };
    let clip = place("V1");
    let logo = place("V2");
    project
        .set_clip_name(logo, Some("Logo".into()))
        .unwrap_or_else(|err| panic!("{err}"));
    (project, clip, logo)
}

fn code(source: &str) -> DriverCode {
    DriverCode::new(EXPRESSIONS_LANGUAGE, source)
}

fn drive(project: &mut Project, id: ClipId, param: ClipParam, source: &str) {
    project
        .set_param_driver(id, param, Some(code(source)))
        .unwrap_or_else(|err| panic!("{err}"));
}

/// `param` on `id` at timeline tick `tick`, after drivers.
fn driven(project: &Project, id: ClipId, param: ClipParam, tick: f64) -> ParamValue {
    let clip = project
        .clip(id)
        .unwrap_or_else(|| panic!("clip {} is gone", id.raw()));
    let driven = DriverFrame::new(project).apply(clip, tick);
    param
        .sample_at(&driven, driven.param_tick_f(tick))
        .unwrap_or_else(|| panic!("{param:?} is not on the clip"))
}

#[test]
fn a_named_reference_reads_the_other_clip_after_its_own_driver() {
    let _serial = serial();
    let (mut project, clip, logo) = two_clips();
    drive(&mut project, logo, ClipParam::Opacity, "80");
    drive(
        &mut project,
        clip,
        ClipParam::Opacity,
        "thisComp.layer(\"Logo\").transform.opacity / 2",
    );
    drive(
        &mut project,
        clip,
        ClipParam::Position,
        "[thisComp.width * 0.6, thisComp.height / 2]",
    );
    drive(&mut project, clip, ClipParam::Rotation, "time * 90");
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 12.0),
        ParamValue::Scalar(0.4)
    );
    assert_eq!(
        driven(&project, clip, ClipParam::Position, 12.0),
        ParamValue::Vec2([0.1, 0.0])
    );
    assert_eq!(
        driven(&project, clip, ClipParam::Rotation, 12.0),
        ParamValue::Scalar(45.0)
    );
    for id in [clip, logo] {
        for param in [ClipParam::Opacity, ClipParam::Position, ClipParam::Rotation] {
            assert_eq!(driver_fault(id, param), None, "{param:?}");
        }
    }

    // A wiggle is the same on every render of a frame and moves between
    // frames. Gradient noise is zero on its lattice, which a 4 Hz wiggle at
    // 24 fps meets every sixth frame, so these frames sit between lattice
    // points.
    drive(&mut project, clip, ClipParam::Rotation, "wiggle(4, 30)");
    let first = driven(&project, clip, ClipParam::Rotation, 13.0);
    assert_ne!(first, ParamValue::Scalar(0.0));
    assert_eq!(first, driven(&project, clip, ClipParam::Rotation, 13.0));
    assert_ne!(first, driven(&project, clip, ClipParam::Rotation, 31.0));
}

#[test]
fn a_failing_expression_keeps_the_keyframed_value_and_records_where() {
    let _serial = serial();
    let (mut project, clip, _) = two_clips();
    drive(
        &mut project,
        clip,
        ClipParam::Opacity,
        "let a = 1\nnope * a",
    );
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 5.0),
        ParamValue::Scalar(1.0)
    );
    let fault = driver_fault(clip, ClipParam::Opacity).unwrap_or_else(|| panic!("no fault"));
    assert_eq!(fault.message, "line 2, column 1: nope is not defined");
    assert!(!fault.stopped);

    // A compile error is recorded the same way, and fixing the code clears it.
    drive(&mut project, clip, ClipParam::Opacity, "1 +");
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 5.0),
        ParamValue::Scalar(1.0)
    );
    let fault = driver_fault(clip, ClipParam::Opacity).unwrap_or_else(|| panic!("no fault"));
    assert!(fault.message.starts_with("line 1, column 4:"), "{fault:?}");
    drive(&mut project, clip, ClipParam::Opacity, "50");
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 5.0),
        ParamValue::Scalar(0.5)
    );
    assert_eq!(driver_fault(clip, ClipParam::Opacity), None);

    // A clip the sequence does not hold is a fault at the reference.
    drive(
        &mut project,
        clip,
        ClipParam::Opacity,
        "thisComp.layer(\"Nobody\").transform.opacity",
    );
    driven(&project, clip, ClipParam::Opacity, 5.0);
    let fault = driver_fault(clip, ClipParam::Opacity).unwrap_or_else(|| panic!("no fault"));
    assert_eq!(
        fault.message,
        "line 1, column 35: no clip in the sequence is named \"Nobody\""
    );
}

#[test]
fn a_cycle_through_references_is_refused_and_stays_a_cycle_when_evaluated() {
    let _serial = serial();
    let (mut project, clip, logo) = two_clips();
    project
        .set_clip_name(clip, Some("Base".into()))
        .unwrap_or_else(|err| panic!("{err}"));
    drive(
        &mut project,
        logo,
        ClipParam::Opacity,
        "thisComp.layer(\"Base\").transform.opacity",
    );
    let back = code("thisComp.layer(\"Logo\").opacity + 1");
    let refused = check_driver(&project, clip, ClipParam::Opacity, &back);
    assert_eq!(
        refused,
        Err(
            "this driver would read its own value back: Base opacity -> Logo opacity -> \
             Base opacity"
                .into()
        )
    );

    // Attached anyway, as a rename can make one: both drivers fault as a
    // cycle, though the language puts a position in front of the error, and
    // both properties keep their keyframed value.
    drive(
        &mut project,
        clip,
        ClipParam::Opacity,
        "thisComp.layer(\"Logo\").opacity + 1",
    );
    for id in [clip, logo] {
        assert_eq!(
            driven(&project, id, ClipParam::Opacity, 5.0),
            ParamValue::Scalar(1.0)
        );
        let fault = driver_fault(id, ClipParam::Opacity).unwrap_or_else(|| panic!("no fault"));
        assert!(
            fault
                .message
                .starts_with("drivers read each other in a cycle: "),
            "{fault:?}"
        );
    }
}
