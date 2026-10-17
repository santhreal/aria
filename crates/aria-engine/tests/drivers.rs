//! `SetParamDriver` through the engine: attach, refuse a cycle, keep code
//! that does not compile, remove, undo, and redo.
//!
//! Closes the class "a driver edit the editor cannot take back, or one that
//! lets a cycle into the project". Evaluation is tested in aria-render and
//! aria-plugin; this file registers a test language whose source is the name
//! of the clip whose opacity it copies.

mod common;

use std::sync::{Arc, LazyLock};

use aria_commands::{Command, EditCommand};
use aria_engine::Engine;
use aria_models::{ClipId, ClipParam, DriverCode, Generator, ParamValue, TrackKind};
use aria_render::{
    DriverInput, DriverLanguage, DriverLanguageRegistration, DriverProgram, DriverScope, InputClip,
    InputProperty,
};
use common::{temp_engine, tr};

const LANGUAGE: &str = "test.copy";

/// Source is a clip name; the program returns that clip's opacity. Empty
/// source returns the keyframed value, and `!` does not compile.
struct CopyOpacity;

struct CopyProgram(Vec<DriverInput>);

impl DriverLanguage for CopyOpacity {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        if source == "!" {
            return Err("line 1, column 1: unexpected !".into());
        }
        let inputs = if source.is_empty() {
            Vec::new()
        } else {
            vec![DriverInput {
                clip: InputClip::Named(source.into()),
                property: InputProperty::Id("opacity".into()),
            }]
        };
        Ok(Arc::new(CopyProgram(inputs)))
    }
}

impl DriverProgram for CopyProgram {
    fn inputs(&self) -> &[DriverInput] {
        &self.0
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        if self.0.is_empty() {
            Ok(scope.value())
        } else {
            scope.input(0)
        }
    }
}

static REGISTERED: LazyLock<DriverLanguageRegistration> = LazyLock::new(|| {
    aria_render::register_driver_language(LANGUAGE, Arc::new(CopyOpacity))
        .expect("register the test language")
});

/// A text clip on its own lane at [0, 48), named `name`.
fn named_text(engine: &mut Engine, lane: &str, name: &str) -> ClipId {
    let track = common::add_track(engine, TrackKind::Text, lane);
    let clip = common::created_clip(
        engine
            .apply(Command::Edit(EditCommand::AddGenerated {
                track,
                generator: Generator::text(name),
                timeline: tr(0, 48),
            }))
            .expect("add the text clip"),
    );
    engine
        .apply(Command::Edit(EditCommand::SetClipName {
            clip,
            name: Some(name.into()),
        }))
        .expect("name the clip");
    clip
}

fn code(source: &str) -> DriverCode {
    DriverCode::new(LANGUAGE, source)
}

fn set(engine: &mut Engine, clip: ClipId, code: Option<DriverCode>) -> Result<(), String> {
    engine
        .apply(Command::Edit(EditCommand::SetParamDriver {
            clip,
            param: ClipParam::Opacity,
            code,
        }))
        .map(|_| ())
        .map_err(|err| err.to_string())
}

fn driver(engine: &Engine, clip: ClipId) -> Option<DriverCode> {
    engine
        .project()
        .clip(clip)
        .and_then(|clip| clip.driver(ClipParam::Opacity))
        .cloned()
}

#[test]
fn a_driver_attaches_refuses_a_cycle_and_undoes() {
    LazyLock::force(&REGISTERED);
    let (_dir, mut engine) = temp_engine();
    let base = named_text(&mut engine, "T1", "Base");
    let logo = named_text(&mut engine, "T2", "Logo");

    set(&mut engine, logo, Some(code("Base"))).expect("Logo copies Base");
    assert_eq!(driver(&engine, logo), Some(code("Base")));

    // Base copying Logo would read Base's own opacity back.
    let refused = set(&mut engine, base, Some(code("Logo"))).expect_err("a cycle is refused");
    assert!(
        refused.ends_with(
            "this driver would read its own value back: Base opacity -> Logo opacity -> \
             Base opacity"
        ),
        "{refused}"
    );
    assert_eq!(
        driver(&engine, base),
        None,
        "the refusal left the clip alone"
    );

    // A clip reading itself is the shortest cycle.
    let refused = set(&mut engine, base, Some(code("Base"))).expect_err("a self-read is refused");
    assert!(
        refused.ends_with("Base opacity -> Base opacity"),
        "{refused}"
    );

    // Code that does not compile, or names a language nothing registers, is
    // kept: it is reported at render until it compiles.
    set(&mut engine, base, Some(code("!"))).expect("broken code is kept");
    assert_eq!(driver(&engine, base), Some(code("!")));
    let foreign = DriverCode::new("nobody.language", "anything");
    set(&mut engine, base, Some(foreign.clone())).expect("an unknown language is kept");
    assert_eq!(driver(&engine, base), Some(foreign));

    // Each edit is one undo step, in order.
    assert!(engine.undo());
    assert_eq!(driver(&engine, base), Some(code("!")));
    assert!(engine.undo());
    assert_eq!(driver(&engine, base), None);
    assert!(engine.undo());
    assert_eq!(driver(&engine, logo), None);
    assert!(engine.redo());
    assert_eq!(driver(&engine, logo), Some(code("Base")));

    // Removing is an edit too.
    set(&mut engine, logo, None).expect("remove the driver");
    assert_eq!(driver(&engine, logo), None);
    assert!(engine.undo());
    assert_eq!(driver(&engine, logo), Some(code("Base")));
}

#[test]
fn a_property_that_cannot_take_a_driver_is_refused() {
    LazyLock::force(&REGISTERED);
    let (_dir, mut engine) = temp_engine();
    let clip = named_text(&mut engine, "T1", "Title");
    let refused = engine
        .apply(Command::Edit(EditCommand::SetParamDriver {
            clip,
            param: ClipParam::Speed,
            code: Some(code("")),
        }))
        .expect_err("a speed ramp has its own curve");
    assert!(
        refused.to_string().contains("cannot take a driver"),
        "{refused}"
    );
    assert_eq!(
        engine.project().clip(clip).map(|clip| clip.drivers.len()),
        Some(0)
    );
}
