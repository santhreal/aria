//! Effects, generators, transitions, and clip types registered while the editor
//! runs reach every list the editor offers them in, and leave those lists when
//! the registration drops.
//!
//! Closes the class "a list cached from the catalog never sees a later
//! registration": the command palette, the inspector's Add list, the
//! Generators grid, and the transition chips each rebuild when the catalog
//! revision moves. Also covers placing a generator or a clip type as a clip,
//! a clip type's grid row following its generator's registration, and a clip
//! type that declares its fields: placed, edited through the inspector as one
//! undo step, saved and reopened with the value, and drawn by its callback.
//! Drawing other registrations is covered by `aria-render`'s
//! `registered_render` test.
//!
//! A separate test binary: a registration in the lib tests would change the
//! catalog every other lib test reads.

use std::path::PathBuf;

use aria_compositor::{EffectDefinition, PassRegistration, TransitionDefinition};
use aria_models::{
    ClipSource, ClipTypeSpec, EffectParamKind, EffectParamSpec, EffectSpec, Generator, MarkerColor,
    TrackKind, TransitionSpec, register_clip_type,
};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::app::palette::Offered;
use aria_ui::types::{AppMenu, CatalogDrag, CatalogDragKind};

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

/// A state with the built-in plugins mounted and a project open.
///
/// The palette offers an effect as its Effect menu row, which comes from the
/// menu registry the UI plugin mounts, and Home hides that menu's rows: a bare
/// state offers no effect at all, before a registration or after it.
fn mounted_state(name: &str) -> AppState {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("scratch data dir");
    let ctx = PluginContext::new();
    // A headless machine can fail the GPU-bound mounts; the menu registry is
    // not among them.
    let _ = aria_ui::plugins::mount_all(&ctx, &dir);
    let mut state = AppState::new(ctx, dir).expect("state");
    state.new_project().expect("a project to edit");
    state
}

/// Whether the palette offers the effect `spec` as its Effect menu row, under
/// its category.
fn offered(state: &AppState, spec: &EffectSpec) -> bool {
    let label = format!("{}: {}", spec.category(), spec.label);
    state.palette_catalog().iter().any(|offer| {
        let Offered::Menu(AppMenu::Effect, row) = offer.action else {
            return false;
        };
        offer.label == label.as_str() && row.ends_with(spec.id.as_str())
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
    let state = mounted_state("registered-effect");
    let id = "test.ui.palette_copy";
    let spec = EffectSpec {
        id: id.into(),
        category: String::new(),
        label: "Palette Copy".into(),
        params: Vec::new(),
        generator: false,
        stateful: false,
        ..Default::default()
    };
    // Builds and caches the palette before the registration.
    assert!(!offered(&state, &spec));
    let registration = aria_compositor::register_effect(EffectDefinition {
        spec: spec.clone(),
        passes: vec![EFFECT_FS.into()],
    })
    .expect("the effect registers");
    assert!(
        offered(&state, &spec),
        "the palette kept the catalog from before the registration"
    );
    assert!(in_add_list(&state, id));

    drop(registration);
    assert!(
        !offered(&state, &spec),
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
            ..Default::default()
        },
        shader: TRANSITION_FS.into(),
    })
    .expect("the transition registers");
    assert!(
        listed(),
        "the chips kept the share from before the registration"
    );

    drop(registration);
    assert!(!listed(), "the chips kept a transition that was removed");
}

fn register_generator(id: &str, label: &str, params: Vec<EffectParamSpec>) -> PassRegistration {
    aria_compositor::register_effect(EffectDefinition {
        spec: EffectSpec {
            id: id.into(),
            category: String::new(),
            label: label.into(),
            params,
            generator: true,
            stateful: false,
            ..Default::default()
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
    let effects: Vec<&str> = clip
        .effects
        .iter()
        .map(|fx| fx.effect_id.as_str())
        .collect();
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
    let registration = register_generator(id, "Bars", Vec::new());

    assert!(in_generator_grid(id), "the grid kept the share from before");
    assert!(
        state.palette_catalog().iter().any(|offer| offer.id == id
            && matches!(&offer.action, Offered::Generator(at) if at.as_str() == id)),
        "the palette does not offer the generator as a clip"
    );
    assert!(
        !aria_ui::catalog_text::generators().iter().any(|choice| {
            aria_models::shipped_effect_catalog()
                .iter()
                .any(|spec| !spec.generator && spec.id == choice.id.as_str())
        }),
        "a shipped effect that changes its input is listed as a generator"
    );

    state.playhead.value = 12;
    state
        .add_effect_generator(id)
        .expect("places the generator");
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
    assert!(
        err.to_string().contains(&format!("no generator `{id}`")),
        "{err}"
    );
    assert_eq!(clip_count(&state), before);
}

/// A vec2 parameter on the layer, `[0, 0]` to `[1, 1]`.
fn vec2_param(name: &str) -> EffectParamSpec {
    EffectParamSpec {
        name: name.into(),
        label: name.into(),
        kind: EffectParamKind::Vec2,
        default: 0.0,
        min: 0.0,
        max: 1.0,
        default_color: [0; 4],
        default_vec2: [0.25, 0.75],
        ..Default::default()
    }
}

/// The Generators grid's row for the clip type `id`.
fn clip_type_row(id: &str) -> Option<aria_ui::catalog_text::ClipTypeChoice> {
    aria_ui::catalog_text::clip_types()
        .iter()
        .find(|row| row.id == id)
        .cloned()
}

/// The one clip the project holds.
fn only_clip(state: &AppState) -> aria_models::Clip {
    let engine = state.engine().lock();
    let timeline = engine.project().timeline();
    let clips: Vec<_> = timeline
        .tracks_ordered()
        .flat_map(|track| track.clips())
        .collect();
    let [clip] = clips.as_slice() else {
        panic!("expected one clip, found {}", clips.len());
    };
    (*clip).clone()
}

#[test]
fn a_registered_clip_type_is_offered_placed_and_detached_one_undo_step_each() {
    let mut state = state("registered-clip-type");
    state.new_project().expect("a project");
    let (id, generator) = ("test.ui.lower-third", "test.ui.third-card");
    // Builds and caches the grid and the palette before the registration.
    assert_eq!(clip_type_row(id), None);
    assert!(state.palette_catalog().iter().all(|offer| offer.id != id));
    let mut spec = ClipTypeSpec::new(id, "Lower third", generator, MarkerColor::Orange);
    spec.handles = vec!["anchor".into()];
    let (registration, _) = register_clip_type(spec).expect("the clip type registers");

    // Listed before its generator, with the reason it cannot be placed.
    let row = clip_type_row(id).expect("the grid kept the share from before");
    assert_eq!(
        row.unavailable.as_deref(),
        Some(format!("the generator `{generator}` is not registered").as_str())
    );
    let err = state
        .add_clip_type(id)
        .expect_err("a type with no generator was placed");
    assert!(
        err.to_string().contains("Lower third cannot be placed"),
        "{err}"
    );
    assert_eq!(clip_count(&state), 0, "a refused placement left a clip");

    let card = register_generator(generator, "Card", vec![vec2_param("anchor")]);
    let row = clip_type_row(id).expect("listed");
    assert_eq!(
        row.unavailable, None,
        "the grid did not follow the generator's registration"
    );
    assert_eq!(row.colour, MarkerColor::Orange);
    assert!(
        state.palette_catalog().iter().any(|offer| offer.id == id
            && matches!(
                &offer.action,
                Offered::ClipType { id: at, placeable: true, .. } if at.as_str() == id
            )),
        "the palette does not offer the type as placeable"
    );

    state.playhead.value = 12;
    state.add_clip_type(id).expect("places the type");
    assert_generator_clip(&state, generator, "Lower third", 12);
    assert_eq!(only_clip(&state).clip_type.as_deref(), Some(id));
    assert!(state.undo(), "an undo step was recorded");
    assert_eq!(
        clip_count(&state),
        0,
        "one undo left part of the placement behind"
    );

    let drag = CatalogDrag {
        kind: CatalogDragKind::ClipType,
        id: id.into(),
        label: "Lower third".into(),
    };
    state
        .drop_catalog_on_track(&drag, None, 48)
        .expect("the dropped type lands");
    assert_generator_clip(&state, generator, "Lower third", 48);
    let placed = only_clip(&state);
    state.select_clip(placed.id);
    let shown = state
        .selected_ui_clip()
        .and_then(|ui| ui.clip_type)
        .expect("the inspector shows the type");
    assert_eq!(shown.label.as_ref(), "Lower third");
    assert_eq!(shown.colour, Some(MarkerColor::Orange));
    assert_eq!(shown.missing, None);
    assert_eq!(shown.handles.len(), 1, "the anchor handle is not drawn");
    assert_eq!(state.overlay_type_handles(), [[0.25, 0.75]]);

    state
        .detach_selected_clip_type()
        .expect("the type detaches");
    let detached = only_clip(&state);
    assert_eq!(detached.clip_type, None);
    assert_eq!(
        detached.effects.len(),
        1,
        "detaching dropped the generator from the chain"
    );
    assert_eq!(detached.name.as_deref(), Some("Lower third"));
    assert!(state.undo(), "the detach left nothing to undo");
    assert_eq!(only_clip(&state).clip_type.as_deref(), Some(id));

    drop(card);
    drop(registration);
    assert_eq!(clip_type_row(id), None, "the grid kept a removed type");
    assert!(state.palette_catalog().iter().all(|offer| offer.id != id));
    assert_eq!(
        only_clip(&state).clip_type.as_deref(),
        Some(id),
        "dropping the type rewrote the clip"
    );
}

#[path = "registered_catalog/defined.rs"]
mod defined;
