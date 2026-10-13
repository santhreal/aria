//! Effects, generators, and transitions registered while the editor runs reach
//! every list the editor offers them in, and leave those lists when the
//! registration drops.
//!
//! Closes the class "a list cached from the catalog never sees a later
//! registration": the command palette, the inspector's Add list, the
//! Generators grid, and the transition chips each rebuild when the catalog
//! revision moves. Also covers placing a generator as a clip. Does not cover
//! drawing; `aria-render`'s `registered_render` test does.
//!
//! A separate test binary: a registration in the lib tests would change the
//! catalog every other lib test reads.

use std::path::PathBuf;

use aria_compositor::{EffectDefinition, PassRegistration, TransitionDefinition};
use aria_models::{ClipSource, EffectSpec, Generator, TrackKind, TransitionSpec};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::app::palette::Offered;
use aria_ui::types::{CatalogDrag, CatalogDragKind};

const EFFECT_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(input_tex, aria_sampler, in.uv);
}";

const TRANSITION_FS: &str = "@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(incoming_tex, aria_sampler, in.uv);
}";

fn state(name: &str) -> AppState {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("scratch data dir");
    AppState::new(PluginContext::new(), dir).expect("state")
}

fn offered(state: &AppState, id: &str) -> bool {
    state.palette_catalog().iter().any(|offer| {
        offer.id.as_str() == id && matches!(&offer.action, Offered::Effect(at) if at.as_str() == id)
    })
}

fn in_add_list(state: &AppState, id: &str) -> bool {
    state
        .effect_browse_list()
        .iter()
        .any(|(spec, _)| spec.id == id)
}

#[test]
fn a_registered_effect_joins_and_leaves_the_palette_and_the_add_list() {
    let state = state("registered-effect");
    let id = "test.ui.palette_copy";
    // Builds and caches the palette before the registration.
    assert!(!offered(&state, id));
    let registration = aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            label: "Palette Copy".into(),
            params: Vec::new(),
            generator: false,
        },
        passes: vec![EFFECT_FS.into()],
    })
    .expect("the effect registers");
    assert!(
        offered(&state, id),
        "the palette kept the catalog from before the registration"
    );
    assert!(in_add_list(&state, id));

    drop(registration);
    assert!(
        !offered(&state, id),
        "the palette kept an effect that was removed"
    );
    assert!(!in_add_list(&state, id));
}

#[test]
fn a_registered_transition_joins_and_leaves_the_transition_chips() {
    let id = "test.ui.chip_cut";
    let listed = || {
        aria_ui::catalog_text::transitions()
            .iter()
            .any(|choice| choice.id == id)
    };
    // Builds and caches the share before the registration.
    assert!(!listed());
    let registration = aria_compositor::register_transition(TransitionDefinition {
        spec: TransitionSpec {
            id: id.into(),
            label: "Chip Cut".into(),
        },
        shader: TRANSITION_FS.into(),
    })
    .expect("the transition registers");
    assert!(listed(), "the chips kept the share from before the registration");

    drop(registration);
    assert!(!listed(), "the chips kept a transition that was removed");
}

fn register_generator(id: &str, label: &str) -> PassRegistration {
    aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            label: label.into(),
            params: Vec::new(),
            generator: true,
        },
        passes: vec![EFFECT_FS.into()],
    })
    .expect("the generator registers")
}

fn in_generator_grid(id: &str) -> bool {
    aria_ui::catalog_text::generators()
        .iter()
        .any(|choice| choice.id == id)
}

/// How many clips the open project holds.
fn clip_count(state: &AppState) -> usize {
    let engine = state.engine().lock();
    engine
        .project()
        .timeline()
        .tracks_ordered()
        .map(|track| track.clips().count())
        .sum()
}

/// The one clip a generator placed: a transparent solid on a sticker lane,
/// named after the generator, with the generator first in its chain.
fn assert_generator_clip(state: &AppState, id: &str, label: &str, start: i64) {
    let engine = state.engine().lock();
    let project = engine.project();
    let timeline = project.timeline();
    let placed: Vec<_> = timeline
        .tracks_ordered()
        .flat_map(|track| track.clips().map(move |clip| (track, clip)))
        .collect();
    let [(track, clip)] = placed.as_slice() else {
        panic!("expected one clip, found {}", placed.len());
    };
    assert_eq!(track.kind, TrackKind::Sticker);
    assert_eq!(
        clip.content,
        ClipSource::Generated(Generator::SolidColor { rgba: [0; 4] })
    );
    assert_eq!(clip.name.as_deref(), Some(label));
    let effects: Vec<&str> = clip.effects.iter().map(|fx| fx.effect_id.as_str()).collect();
    assert_eq!(effects, [id]);
    assert_eq!(clip.timeline.start.value, start);
}

#[test]
fn a_registered_generator_is_offered_and_placed_as_one_clip_in_one_undo_step() {
    let mut state = state("registered-generator");
    state.new_project().expect("a project");
    let id = "test.ui.bars";
    // Builds and caches the grid and the palette before the registration.
    assert!(!in_generator_grid(id));
    assert!(state.palette_catalog().iter().all(|offer| offer.id != id));
    let registration = register_generator(id, "Bars");

    assert!(in_generator_grid(id), "the grid kept the share from before");
    assert!(
        state.palette_catalog().iter().any(|offer| offer.id == id
            && matches!(&offer.action, Offered::Generator(at) if at.as_str() == id)),
        "the palette does not offer the generator as a clip"
    );
    assert!(
        !aria_ui::catalog_text::generators()
            .iter()
            .any(|choice| aria_models::shipped_effect_catalog()
                .iter()
                .any(|spec| spec.id == choice.id.as_str())),
        "a shipped effect that changes its input is listed as a generator"
    );

    state.playhead.value = 12;
    state.add_effect_generator(id).expect("places the generator");
    assert_generator_clip(&state, id, "Bars", 12);
    assert!(state.undo(), "an undo step was recorded");
    assert_eq!(
        clip_count(&state),
        0,
        "one undo left part of the placement behind"
    );

    let drag = CatalogDrag {
        kind: CatalogDragKind::Generator,
        id: id.into(),
        label: "Bars".into(),
    };
    state
        .drop_catalog_on_track(&drag, None, 48)
        .expect("the dropped generator lands");
    assert_generator_clip(&state, id, "Bars", 48);

    drop(registration);
    assert!(!in_generator_grid(id), "the grid kept a removed generator");
    assert!(state.palette_catalog().iter().all(|offer| offer.id != id));
    let before = clip_count(&state);
    let err = state
        .add_effect_generator(id)
        .expect_err("no generator renders a removed id");
    assert!(err.to_string().contains(&format!("no generator `{id}`")), "{err}");
    assert_eq!(clip_count(&state), before);
}
