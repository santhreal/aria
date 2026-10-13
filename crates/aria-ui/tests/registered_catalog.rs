//! Effects and transitions registered while the editor runs reach every list
//! the editor offers them in, and leave those lists when the registration
//! drops.
//!
//! Closes the class "a list cached from the catalog never sees a later
//! registration": the command palette, the inspector's Add list, and the
//! transition chips each rebuild when the catalog revision moves. Does not
//! cover drawing; `aria-render`'s `registered_render` test does.
//!
//! A separate test binary: a registration in the lib tests would change the
//! catalog every other lib test reads.

use std::path::PathBuf;

use aria_compositor::{EffectDefinition, TransitionDefinition};
use aria_models::{EffectSpec, TransitionSpec};
use aria_plugin::PluginContext;
use aria_ui::app::AppState;
use aria_ui::app::palette::Offered;

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
