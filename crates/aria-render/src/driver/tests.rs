//! Property drivers evaluated through the render paths.
//!
//! Closes the class "a driver's value does not reach what is drawn or heard":
//! values reach resolve, named and effect inputs read other drivers, every
//! failure kind leaves the keyframed value and records a fault, cycles are
//! refused at check time and at evaluation, a driver over budget stops until
//! its code changes, and audio envelopes follow the driver per frame. Does not
//! cover a plugin language: `aria-plugin` tests the registration path.

use std::sync::atomic::AtomicUsize;

use aria_models::{
    Clip, MediaSource, Param, Project, Rational, RationalTime, TimeRange, TrackKind, effect_spec,
    visit_clip_params,
};

use super::*;
use crate::resolve;

const FPS: Rational = Rational::FPS_24;
const LANGUAGE: &str = "test.rpn";

/// Faults and the stop record are process-wide, keyed by clip id.
static SERIAL: Mutex<()> = Mutex::new(());
/// Runs of `sleep:` words, to prove a stopped driver is not run.
static SLEEPS: AtomicUsize = AtomicUsize::new(0);
static REGISTERED: LazyLock<DriverLanguageRegistration> = LazyLock::new(|| {
    register_driver_language(LANGUAGE, Arc::new(Rpn)).unwrap_or_else(|err| panic!("{err}"))
});

/// A stack language for these tests. `@this:opacity`, `@Name:position` and
/// `@this:fx:effect/param` declare inputs; `t` and `ct` push sequence and
/// clip seconds, `v` the keyframed value, `in:N` an input, `at:N:S` an input
/// at second `S`; `+`, `*` and `pair` combine; `panic`, `fail`, `nan` and
/// `sleep:MS` misbehave.
struct Rpn;

enum Word {
    Number(f32),
    Time,
    ClipTime,
    Value,
    Input(usize),
    InputAt(usize, f64),
    Add,
    Multiply,
    Pair,
    Panic,
    Fail,
    Sleep(u64),
}

struct Program {
    inputs: Vec<DriverInput>,
    words: Vec<Word>,
}

impl DriverLanguage for Rpn {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        let mut inputs = Vec::new();
        let mut words = Vec::new();
        for token in source.split_whitespace() {
            if let Some(input) = token.strip_prefix('@') {
                let (clip, property) = input.split_once(':').ok_or("an input needs a clip")?;
                let clip = match clip {
                    "this" => InputClip::This,
                    name => InputClip::Named(name.to_string()),
                };
                let property = match property.strip_prefix("fx:") {
                    Some(effect) => {
                        let (effect, param) = effect.split_once('/').ok_or("fx:effect/param")?;
                        InputProperty::Effect {
                            effect: effect.to_string(),
                            param: param.to_string(),
                        }
                    }
                    None => InputProperty::Id(property.to_string()),
                };
                inputs.push(DriverInput { clip, property });
                continue;
            }
            let number = |text: &str| text.parse::<f64>().map_err(|err| err.to_string());
            words.push(match token {
                "t" => Word::Time,
                "ct" => Word::ClipTime,
                "v" => Word::Value,
                "+" => Word::Add,
                "*" => Word::Multiply,
                "pair" => Word::Pair,
                "panic" => Word::Panic,
                "fail" => Word::Fail,
                "nan" => Word::Number(f32::NAN),
                _ => {
                    if let Some(rest) = token.strip_prefix("in:") {
                        Word::Input(number(rest)? as usize)
                    } else if let Some(rest) = token.strip_prefix("at:") {
                        let (index, seconds) = rest.split_once(':').ok_or("at:N:S")?;
                        Word::InputAt(number(index)? as usize, number(seconds)?)
                    } else if let Some(rest) = token.strip_prefix("sleep:") {
                        Word::Sleep(number(rest)? as u64)
                    } else {
                        Word::Number(token.parse().map_err(|_| format!("unknown word {token}"))?)
                    }
                }
            });
        }
        Ok(Arc::new(Program { inputs, words }))
    }
}

fn scalar(value: ParamValue) -> Result<f32, String> {
    match value {
        ParamValue::Scalar(value) => Ok(value),
        other => Err(format!("{other:?} is not a number")),
    }
}

impl DriverProgram for Program {
    fn inputs(&self) -> &[DriverInput] {
        &self.inputs
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        let mut stack: Vec<ParamValue> = Vec::new();
        let pop = |stack: &mut Vec<ParamValue>| stack.pop().ok_or("stack underflow");
        for word in &self.words {
            let value = match *word {
                Word::Number(value) => ParamValue::Scalar(value),
                Word::Time => ParamValue::Scalar(scope.time() as f32),
                Word::ClipTime => ParamValue::Scalar(scope.clip_time() as f32),
                Word::Value => scope.value(),
                Word::Input(index) => scope.input(index)?,
                Word::InputAt(index, seconds) => scope.input_at(index, seconds)?,
                Word::Add | Word::Multiply => {
                    let right = scalar(pop(&mut stack)?)?;
                    let left = scalar(pop(&mut stack)?)?;
                    ParamValue::Scalar(if matches!(word, Word::Add) {
                        left + right
                    } else {
                        left * right
                    })
                }
                Word::Pair => {
                    let y = scalar(pop(&mut stack)?)?;
                    let x = scalar(pop(&mut stack)?)?;
                    ParamValue::Vec2([x, y])
                }
                Word::Panic => panic!("the test driver panicked on purpose"),
                Word::Fail => return Err("boom".into()),
                Word::Sleep(millis) => {
                    SLEEPS.fetch_add(1, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(millis));
                    continue;
                }
            };
            stack.push(value);
        }
        pop(&mut stack).map_err(str::to_string)
    }
}

fn serial() -> parking_lot::MutexGuard<'static, ()> {
    LazyLock::force(&REGISTERED);
    SERIAL.lock()
}

fn code(source: &str) -> Option<DriverCode> {
    Some(DriverCode::new(LANGUAGE, source))
}

/// A project with one video clip on frames 0..48 of lane V1.
fn one_clip(has_audio: bool) -> (Project, ClipId) {
    let mut project = Project::new("drivers", FPS);
    let media = project.add_media(MediaSource::new(
        "/tmp/driven.mp4",
        640,
        360,
        FPS,
        600,
        has_audio,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 48, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap_or_else(|err| panic!("{err}"));
    (project, clip)
}

fn clip(project: &Project, id: ClipId) -> &Clip {
    project
        .clip(id)
        .unwrap_or_else(|| panic!("clip {} is gone", id.raw()))
}

/// `param` on `id` at timeline tick `tick`, after drivers.
fn driven(project: &Project, id: ClipId, param: ClipParam, tick: f64) -> ParamValue {
    let target = clip(project, id);
    let driven = DriverFrame::new(project).apply(target, tick);
    param
        .sample_at(&driven, driven.param_tick_f(tick))
        .unwrap_or_else(|| panic!("{param:?} is not on the clip"))
}

fn drive(project: &mut Project, id: ClipId, param: ClipParam, source: &str) {
    project
        .set_param_driver(id, param, code(source))
        .unwrap_or_else(|err| panic!("{err}"));
}

fn fault(id: ClipId, param: ClipParam) -> DriverFault {
    driver_fault(id, param).unwrap_or_else(|| panic!("no fault on {param:?}"))
}

#[test]
fn resolve_draws_the_driven_value_and_the_keyframed_one_without_a_driver() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    drive(&mut project, id, ClipParam::Opacity, "0.25");
    let scene = resolve(&project, RationalTime::new(5, FPS)).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(scene.layers[0].opacity, 0.25);
    assert_eq!(driver_fault(id, ClipParam::Opacity), None);

    project
        .set_param_driver(id, ClipParam::Opacity, None)
        .unwrap_or_else(|err| panic!("{err}"));
    let scene = resolve(&project, RationalTime::new(5, FPS)).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(scene.layers[0].opacity, 1.0);
}

#[test]
fn time_reads_sequence_seconds_and_clip_time_counts_from_the_clip() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    drive(&mut project, id, ClipParam::Rotation, "t 10 *");
    for (tick, rotation) in [(0.0, 0.0), (12.0, 5.0), (36.0, 15.0)] {
        assert_eq!(
            driven(&project, id, ClipParam::Rotation, tick),
            ParamValue::Scalar(rotation),
            "tick {tick}"
        );
    }
    // A sub-frame tick, as motion blur samples, reads its own time.
    assert_eq!(
        driven(&project, id, ClipParam::Rotation, 6.0),
        ParamValue::Scalar(2.5)
    );
}

#[test]
fn a_named_input_reads_the_other_clip_after_its_own_driver() {
    let _serial = serial();
    let (mut project, follower) = one_clip(false);
    let media = clip(&project, follower)
        .media()
        .unwrap_or_else(|| panic!("no media"));
    let upper = project.add_track(TrackKind::Video, "V2");
    let leader = project
        .add_clip(
            upper,
            media,
            TimeRange::at_rate(0, 48, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap_or_else(|err| panic!("{err}"));
    project
        .set_clip_name(leader, Some("Leader".into()))
        .unwrap_or_else(|err| panic!("{err}"));
    drive(&mut project, leader, ClipParam::Opacity, "0.5");
    drive(
        &mut project,
        follower,
        ClipParam::Opacity,
        "@Leader:opacity in:0 0.5 *",
    );
    assert_eq!(
        driven(&project, follower, ClipParam::Opacity, 10.0),
        ParamValue::Scalar(0.25)
    );

    // A name nothing carries is a fault, and the keyframed value stays.
    drive(
        &mut project,
        follower,
        ClipParam::Opacity,
        "@Nobody:opacity in:0",
    );
    assert_eq!(
        driven(&project, follower, ClipParam::Opacity, 10.0),
        ParamValue::Scalar(1.0)
    );
    assert!(
        fault(follower, ClipParam::Opacity)
            .message
            .contains("no clip in the sequence is named \"Nobody\""),
        "{:?}",
        fault(follower, ClipParam::Opacity)
    );
}

#[test]
fn effect_parameters_are_inputs_and_targets_and_targets_hold_their_range() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    project
        .add_effect(id, "gaussian_blur")
        .unwrap_or_else(|err| panic!("{err}"));
    let spec = effect_spec("gaussian_blur").unwrap_or_else(|| panic!("no gaussian_blur"));
    let radius = spec
        .params
        .iter()
        .position(|param| param.name == "radius")
        .unwrap_or_else(|| panic!("gaussian_blur has no radius"));
    let target = ClipParam::Effect {
        effect: 0,
        param: radius as u32,
    };

    drive(
        &mut project,
        id,
        ClipParam::Opacity,
        &format!(
            "@this:fx:GAUSSIAN_BLUR/{} in:0 0.1 *",
            spec.params[radius].name.to_uppercase()
        ),
    );
    let ParamValue::Scalar(opacity) = driven(&project, id, ClipParam::Opacity, 0.0) else {
        panic!("opacity is not a number");
    };
    assert!(
        (opacity - spec.params[radius].default * 0.1).abs() < 1e-6,
        "{opacity}"
    );

    drive(&mut project, id, target, "1000");
    assert_eq!(
        driven(&project, id, target, 0.0),
        ParamValue::Scalar(spec.params[radius].max)
    );
    // The opacity driver reads the radius after its driver, and holds its
    // own range.
    assert_eq!(
        driven(&project, id, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(1.0)
    );
    assert_eq!(driver_fault(id, ClipParam::Opacity), None);
}

#[test]
fn every_failure_keeps_the_keyframed_value_and_records_why_until_it_succeeds() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    let cases: [(DriverCode, &str); 7] = [
        (
            DriverCode::new(LANGUAGE, "panic"),
            "the driver panicked: the test driver panicked on purpose",
        ),
        (DriverCode::new(LANGUAGE, "fail"), "boom"),
        (
            DriverCode::new(LANGUAGE, "0.1 0.2 pair"),
            "the driver returned a pair for a property that holds a number",
        ),
        (
            DriverCode::new(LANGUAGE, "nan"),
            "the driver returned a value that is not finite",
        ),
        (DriverCode::new(LANGUAGE, "mystery"), "unknown word mystery"),
        (DriverCode::new(LANGUAGE, ""), "stack underflow"),
        (
            DriverCode::new("absent.language", "0.5"),
            "no driver language absent.language is installed; install the plugin that provides it",
        ),
    ];
    for (failing, message) in cases {
        project
            .set_param_driver(id, ClipParam::Opacity, Some(failing.clone()))
            .unwrap_or_else(|err| panic!("{err}"));
        let scene =
            resolve(&project, RationalTime::new(5, FPS)).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(scene.layers[0].opacity, 1.0, "{failing:?}");
        assert_eq!(
            driver_fault(id, ClipParam::Opacity),
            Some(DriverFault {
                clip: id,
                param: ClipParam::Opacity,
                message: message.to_string(),
                stopped: false,
            }),
            "{failing:?}"
        );
        assert!(driver_faults().iter().any(|fault| fault.clip == id));

        let revision = driver_faults_revision();
        drive(&mut project, id, ClipParam::Opacity, "0.5");
        let scene =
            resolve(&project, RationalTime::new(5, FPS)).unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(scene.layers[0].opacity, 0.5, "{failing:?}");
        assert_eq!(driver_fault(id, ClipParam::Opacity), None, "{failing:?}");
        assert!(driver_faults_revision() > revision);
    }
}

#[test]
fn a_value_outside_the_range_is_held_inside_it() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    for (source, opacity) in [("5", 1.0), ("-3", 0.0), ("0.75", 0.75)] {
        drive(&mut project, id, ClipParam::Opacity, source);
        assert_eq!(
            driven(&project, id, ClipParam::Opacity, 0.0),
            ParamValue::Scalar(opacity),
            "{source}"
        );
    }
}

#[test]
fn a_cycle_is_refused_by_the_check_and_faults_every_driver_in_it() {
    let _serial = serial();
    let (mut project, first) = one_clip(false);
    let media = clip(&project, first)
        .media()
        .unwrap_or_else(|| panic!("no media"));
    let upper = project.add_track(TrackKind::Video, "V2");
    let second = project
        .add_clip(
            upper,
            media,
            TimeRange::at_rate(0, 48, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap_or_else(|err| panic!("{err}"));
    for (id, name) in [(first, "First"), (second, "Second")] {
        project
            .set_clip_name(id, Some(name.into()))
            .unwrap_or_else(|err| panic!("{err}"));
    }
    let reads_second = DriverCode::new(LANGUAGE, "@Second:opacity in:0");
    let reads_first = DriverCode::new(LANGUAGE, "@First:opacity in:0");
    assert_eq!(
        check_driver(&project, first, ClipParam::Opacity, &reads_second),
        Ok(())
    );
    drive(
        &mut project,
        first,
        ClipParam::Opacity,
        "@Second:opacity in:0",
    );
    assert_eq!(
        check_driver(&project, second, ClipParam::Opacity, &reads_first),
        Err(
            "this driver would read its own value back: Second opacity -> First opacity -> \
             Second opacity"
                .to_string()
        )
    );
    assert_eq!(
        check_driver(
            &project,
            first,
            ClipParam::Opacity,
            &DriverCode::new(LANGUAGE, "@this:opacity in:0"),
        ),
        Err("this driver would read its own value back: First opacity -> First opacity".into())
    );
    assert_eq!(
        check_driver(
            &project,
            first,
            ClipParam::Opacity,
            &DriverCode::new(LANGUAGE, "@this:rotation in:0")
        ),
        Ok(())
    );
    assert_eq!(
        check_driver(
            &project,
            first,
            ClipParam::Opacity,
            &DriverCode::new(LANGUAGE, "mystery")
        ),
        Err("unknown word mystery".into())
    );

    // A cycle the check never saw, as a rename makes, faults both drivers
    // and draws both at their keyframed values. Evaluation ends.
    drive(
        &mut project,
        second,
        ClipParam::Opacity,
        "@First:opacity in:0",
    );
    let scene = resolve(&project, RationalTime::new(5, FPS)).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        scene
            .layers
            .iter()
            .map(|layer| layer.opacity)
            .collect::<Vec<_>>(),
        [1.0, 1.0]
    );
    for id in [first, second] {
        let fault = fault(id, ClipParam::Opacity);
        assert!(
            fault.message.starts_with(CYCLE_PREFIX) && !fault.stopped,
            "{fault:?}"
        );
    }
}

const CYCLE_PREFIX: &str = "drivers read each other in a cycle: ";

#[test]
fn a_driver_over_budget_is_stopped_until_its_code_changes() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    let over = driver_budget().as_millis() as u64 * 2 + 10;
    drive(
        &mut project,
        id,
        ClipParam::Opacity,
        &format!("sleep:{over} 0.5"),
    );
    let before = SLEEPS.load(Ordering::SeqCst);
    assert_eq!(
        driven(&project, id, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(1.0)
    );
    let stopped = fault(id, ClipParam::Opacity);
    assert!(stopped.stopped, "{stopped:?}");
    assert!(
        stopped
            .message
            .ends_with("budget, and is stopped until its code changes"),
        "{stopped:?}"
    );
    for tick in 1..4 {
        assert_eq!(
            driven(&project, id, ClipParam::Opacity, f64::from(tick)),
            ParamValue::Scalar(1.0)
        );
    }
    assert_eq!(
        SLEEPS.load(Ordering::SeqCst),
        before + 1,
        "a stopped driver ran"
    );

    drive(&mut project, id, ClipParam::Opacity, "sleep:0 0.5");
    assert_eq!(
        driven(&project, id, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(0.5)
    );
    assert_eq!(driver_fault(id, ClipParam::Opacity), None);
}

#[test]
fn input_at_reads_another_time_and_rejects_a_time_that_is_not_a_number() {
    let _serial = serial();
    let (mut project, id) = one_clip(false);
    drive(&mut project, id, ClipParam::Rotation, "t 10 *");
    // Opacity reads the rotation at half a second, scaled into range.
    drive(
        &mut project,
        id,
        ClipParam::Opacity,
        "@this:rotation at:0:0.5 0.01 *",
    );
    let ParamValue::Scalar(opacity) = driven(&project, id, ClipParam::Opacity, 36.0) else {
        panic!("opacity is not a number");
    };
    assert!((opacity - 0.05).abs() < 1e-6, "{opacity}");

    drive(
        &mut project,
        id,
        ClipParam::Opacity,
        "@this:rotation at:0:NaN",
    );
    assert_eq!(
        driven(&project, id, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(1.0)
    );
    assert_eq!(
        fault(id, ClipParam::Opacity).message,
        "an input was read at a time that is not a number"
    );
}

#[test]
fn the_audio_envelope_follows_the_driver_at_every_frame() {
    let _serial = serial();
    let (mut project, id) = one_clip(true);
    assert_eq!(
        driven_envelope(&project, clip(&project, id), ClipParam::Volume),
        None
    );
    drive(&mut project, id, ClipParam::Volume, "ct 0.5 *");
    let Some(Param::Keyframed { keyframes }) =
        driven_envelope(&project, clip(&project, id), ClipParam::Volume)
    else {
        panic!("no driven envelope");
    };
    assert_eq!(keyframes.len(), 48);
    for (frame, key) in keyframes.iter().enumerate() {
        assert_eq!(key.tick, frame as i64);
        assert!(
            (key.value - frame as f32 / 24.0 * 0.5).abs() < 1e-6,
            "frame {frame}: {}",
            key.value
        );
    }

    // A driver that never produces a value leaves the keyframed envelope.
    drive(&mut project, id, ClipParam::Volume, "fail");
    assert_eq!(
        driven_envelope(&project, clip(&project, id), ClipParam::Volume),
        None
    );
}

#[test]
fn a_language_registration_ends_when_dropped() {
    let _serial = serial();
    let id = "test.temporary";
    let registration =
        register_driver_language(id, Arc::new(Rpn)).unwrap_or_else(|err| panic!("{err}"));
    assert!(driver_languages().iter().any(|language| language == id));
    let code = DriverCode::new(id, "0.5");
    assert!(compile_driver(&code).is_ok());
    assert!(register_driver_language("has space", Arc::new(Rpn)).is_err());

    // A second host registering the id shadows the first until it drops its
    // registration, and the id is listed once.
    let (mut project, clip) = one_clip(false);
    project
        .set_param_driver(clip, ClipParam::Opacity, Some(DriverCode::new(id, "0.25")))
        .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(0.25)
    );
    let shadow =
        register_driver_language(id, Arc::new(Doubled)).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        driver_languages()
            .iter()
            .filter(|language| *language == id)
            .count(),
        1
    );
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(0.5)
    );
    drop(shadow);
    assert_eq!(
        driven(&project, clip, ClipParam::Opacity, 0.0),
        ParamValue::Scalar(0.25)
    );

    // A language without pick whip syntax answers nothing, and its template
    // is empty.
    let own = DriverInput {
        clip: InputClip::This,
        property: InputProperty::Id("opacity".into()),
    };
    assert_eq!(driver_reference(id, &own), None);
    assert_eq!(driver_template(id), Some(String::new()));

    drop(registration);
    assert!(!driver_languages().iter().any(|language| language == id));
    assert_eq!(driver_template(id), None);
    assert_eq!(
        compile_driver(&code).map(drop),
        Err(format!(
            "no driver language {id} is installed; install the plugin that provides it"
        ))
    );
}

/// [`Rpn`] with its result doubled, to tell two registrations of one id apart.
struct Doubled;

impl DriverLanguage for Doubled {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        Rpn.compile(&format!("{source} 2 *"))
    }
}

/// The pick whip addresses every property a clip shows. A property it cannot
/// address, an effect parameter addressed by a label another effect also
/// carries, or a clip addressed by something other than its title fails.
#[test]
fn the_pick_whip_addresses_every_property_of_a_clip() {
    let _serial = serial();
    let (mut project, follower) = one_clip(false);
    let media = clip(&project, follower)
        .media()
        .unwrap_or_else(|| panic!("no media"));
    let upper = project.add_track(TrackKind::Video, "V2");
    let leader = project
        .add_clip(
            upper,
            media,
            TimeRange::at_rate(0, 48, FPS),
            RationalTime::new(0, FPS),
        )
        .unwrap_or_else(|err| panic!("{err}"));
    project
        .set_clip_name(leader, Some("Leader".into()))
        .unwrap_or_else(|err| panic!("{err}"));
    // The second blur carries the first one's label, so only the first is
    // reachable by label.
    for _ in 0..2 {
        project
            .add_effect(leader, "gaussian_blur")
            .unwrap_or_else(|err| panic!("{err}"));
    }
    let spec = effect_spec("gaussian_blur").unwrap_or_else(|| panic!("no gaussian_blur"));

    let mut params = Vec::new();
    visit_clip_params(clip(&project, leader), |row| params.push(row.param));
    assert!(
        params
            .iter()
            .any(|param| matches!(param, ClipParam::Effect { effect: 1, .. })),
        "the second effect's parameters are listed"
    );
    for param in params {
        let input = driver_input(&project, follower, leader, param)
            .unwrap_or_else(|| panic!("{param:?} has no address"));
        assert_eq!(input.clip, InputClip::Named("Leader".into()), "{param:?}");
        match (param, &input.property) {
            (
                ClipParam::Effect {
                    effect: 0,
                    param: slot,
                },
                InputProperty::Effect {
                    effect,
                    param: label,
                },
            ) => {
                assert_eq!(*effect, spec.label);
                assert_eq!(*label, spec.params[slot as usize].label);
            }
            (ClipParam::Effect { effect: 0, .. }, other) => {
                panic!("the first blur is addressed by label, not {other:?}")
            }
            (_, InputProperty::Id(_)) => {}
            (_, other) => panic!("{param:?} is addressed by catalog id, not {other:?}"),
        }
        let own = driver_input(&project, leader, leader, param)
            .unwrap_or_else(|| panic!("{param:?} has no address on its own clip"));
        assert_eq!(own.clip, InputClip::This, "{param:?}");
    }
}
