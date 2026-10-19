//! Edits in Effect Controls with several clips selected.
//!
//! # Why
//!
//! Closes the class "an edit with several clips selected lands on the first
//! clip alone, lands on a clip that does not have the property, or leaves the
//! row showing one value over clips that read differently". Each path a row's
//! write takes outside the drawn registries is driven against a second clip
//! that holds what the first does: the catalog stepper and its scrub, the gain
//! row, the colour wheels' drag and reset, an effect dropped on the panel, a
//! driver toggled with Alt+click, and every verb of an effect header's menu.
//! Each is checked to give every clip the value the first one landed on, as one
//! undo step that one undo takes back whole, and to leave alone a clip without
//! the property, a clip holding another effect there, a clip whose driver reads
//! otherwise, and every other clip while a title is being typed into. The dash
//! is checked on the rows the panel draws from: a row reads as mixed exactly
//! where another selected clip reads differently at the playhead, on the row's
//! own axis, on every effect slot kind no curve carries, and never on one clip
//! or while typing.
//!
//! The menu rows are read out of the effect menu's table, and the rows that
//! must edit out of its source, so a new verb is pressed here and turns this
//! red until it reaches the other clips.
//!
//! The registries' gestures (picks, blocks, the rack and the keyframe rows)
//! are pressed against the same pair by their own censuses. Not covered: the
//! drawn glyph of the dash, which the readouts print from these flags.

use aria_models::{
    ClipId, ClipParam, EffectChoiceOption, EffectCurvePoint, EffectGradientStop, EffectParamKind,
    EffectParamSpec, EffectSpec, ParamValue, RationalTime, register_effect,
};

use crate::app::AppState;
use crate::app::clip_text::text_caret::tests::typing;
use crate::app::fixtures::{blurred_clip, linked_pair, model_of, the_pair, twin_selected};
use crate::app::grade_controls::WheelControl;
use crate::app::premiere_tests::with_clip;
use crate::surfaces::context_menu::{EFFECT_ITEMS, EffectMenu, MenuItem};
use crate::surfaces::effects_rack::SlotEdit;
use crate::surfaces::keyframe_rows::{self, Row};
use crate::types::{CatalogDrag, CatalogDragKind};

/// Make `clips` the selection, the first one drawn.
fn select(state: &mut AppState, clips: &[ClipId]) {
    state.selection = clips.to_vec();
    state.refresh_projected_selection();
}

/// Run `then` with `clip` the only selected clip, then put `keep` back.
fn on<T>(
    state: &mut AppState,
    clip: ClipId,
    keep: &[ClipId],
    then: impl FnOnce(&mut AppState) -> T,
) -> T {
    select(state, &[clip]);
    let out = then(state);
    select(state, keep);
    out
}

/// What `param` reads on `clip` at the playhead, on `component` of a vector.
fn read(
    state: &mut AppState,
    clip: ClipId,
    keep: &[ClipId],
    param: ClipParam,
    component: Option<u8>,
) -> f32 {
    on(state, clip, keep, |state| {
        state
            .param_scalar(param, component)
            .unwrap_or_else(|| panic!("{clip:?} has no {param:?}"))
    })
}

/// The mixed flag of the drawn clip's row for `param` on `component`.
fn mixed(state: &AppState, group: &str, param: ClipParam, component: Option<u8>) -> bool {
    state
        .param_rows(group)
        .iter()
        .find(|row| row.param == param && row.component == component)
        .unwrap_or_else(|| panic!("no {param:?} {component:?} row in {group}"))
        .mixed
}

/// The ids of `clip`'s effect chain, in render order.
fn chain(state: &AppState, clip: ClipId) -> Vec<String> {
    model_of(state, clip)
        .effects
        .iter()
        .map(|effect| effect.effect_id.clone())
        .collect()
}

/// An effect dragged out of the catalog.
fn effect(id: &str) -> CatalogDrag {
    CatalogDrag {
        kind: CatalogDragKind::Effect,
        id: id.to_owned().into(),
        label: id.to_owned().into(),
    }
}

/// A picture and a copy of it, both selected, the picture drawn.
fn twin_pictures() -> (AppState, ClipId, ClipId) {
    let (mut state, _) = with_clip();
    let (drawn, twin) = twin_selected(&mut state);
    (state, drawn, twin)
}

#[test]
fn a_step_gives_every_selected_clip_the_value_the_first_landed_on() {
    let (mut state, drawn, twin) = twin_pictures();
    let both = [drawn, twin];
    on(&mut state, twin, &both, |state| {
        state
            .set_param_scalar(ClipParam::Opacity, None, 0.4)
            .expect("the copy reads apart")
    });
    let depth = state.history_depth();

    state
        .nudge_shared_param(ClipParam::Opacity, None, -0.25)
        .expect("the step lands");
    let landed = read(&mut state, drawn, &both, ClipParam::Opacity, None);
    assert!((landed - 0.75).abs() < 1e-6, "the drawn clip read {landed}");
    assert_eq!(
        read(&mut state, twin, &both, ClipParam::Opacity, None),
        landed,
        "the copy did not take the value the drawn clip landed on"
    );
    assert_eq!(
        state.history_depth(),
        depth + 1,
        "one step, not one per clip"
    );

    assert!(state.undo());
    assert_eq!(
        read(&mut state, drawn, &both, ClipParam::Opacity, None),
        1.0
    );
    assert_eq!(
        read(&mut state, twin, &both, ClipParam::Opacity, None),
        0.4,
        "one undo left the copy moved"
    );
}

#[test]
fn a_step_on_one_axis_leaves_the_other_axis_of_every_clip_alone() {
    let (mut state, drawn, twin) = twin_pictures();
    let both = [drawn, twin];
    let x = read(&mut state, drawn, &both, ClipParam::Position, Some(0));
    let y = read(&mut state, drawn, &both, ClipParam::Position, Some(1));
    on(&mut state, twin, &both, |state| {
        state
            .set_param_value(ClipParam::Position, ParamValue::Vec2([x + 50.0, y + 80.0]))
            .expect("the copy stands elsewhere")
    });
    assert!(mixed(&state, "transform", ClipParam::Position, Some(0)));
    assert!(mixed(&state, "transform", ClipParam::Position, Some(1)));

    state
        .nudge_shared_param(ClipParam::Position, Some(0), 10.0)
        .expect("the step lands");
    assert_eq!(
        read(&mut state, twin, &both, ClipParam::Position, Some(0)),
        x + 10.0,
        "the copy did not take the drawn clip's x"
    );
    assert_eq!(
        read(&mut state, twin, &both, ClipParam::Position, Some(1)),
        y + 80.0,
        "a step on x moved the copy's y"
    );
    assert!(!mixed(&state, "transform", ClipParam::Position, Some(0)));
    assert!(
        mixed(&state, "transform", ClipParam::Position, Some(1)),
        "the axis the clips still read apart on lost its dash"
    );
}

#[test]
fn a_row_reads_as_mixed_where_the_clips_differ_at_the_playhead() {
    let (mut state, drawn, twin) = twin_pictures();
    let both = [drawn, twin];
    assert!(
        !mixed(&state, "transform", ClipParam::Opacity, None),
        "two clips alike read as mixed"
    );

    // The drawn clip fades from full to 0.4 over twelve ticks; the copy holds.
    let rate = state.store.sequence.frame_rate;
    let start = model_of(&state, drawn).timeline.start.value;
    state.playhead = RationalTime::new(start, rate);
    on(&mut state, drawn, &both, |state| {
        state
            .add_keyframe_at_playhead(ClipParam::Opacity)
            .expect("key the start");
        state.playhead = RationalTime::new(start + 12, rate);
        state
            .set_param_scalar(ClipParam::Opacity, None, 0.4)
            .expect("key the end");
    });
    state.playhead = RationalTime::new(start, rate);
    assert!(
        !mixed(&state, "transform", ClipParam::Opacity, None),
        "the clips read alike where the fade starts"
    );
    state.playhead = RationalTime::new(start + 12, rate);
    assert!(
        mixed(&state, "transform", ClipParam::Opacity, None),
        "the clips read apart where the fade ends"
    );

    select(&mut state, &[drawn]);
    assert!(
        !mixed(&state, "transform", ClipParam::Opacity, None),
        "one clip read as mixed"
    );
}

/// The reset button of a row lights where any selected clip sharing the row
/// holds something other than its default, and a press puts each of them back,
/// so a dash over a drawn clip already at its default keeps a reset that does
/// something. Driven on a number row and on a colour row, which are projected
/// apart.
#[test]
fn a_rows_reset_lights_for_a_copy_off_its_default_and_puts_it_back() {
    let (mut state, drawn, twin) = twin_pictures();
    let both = [drawn, twin];
    let number = |state: &AppState| {
        state
            .param_rows("transform")
            .iter()
            .find(|row| row.param == ClipParam::Opacity)
            .expect("an opacity row")
            .off_default
    };
    assert!(!number(&state), "two clips at their default lit the reset");
    on(&mut state, twin, &both, |state| {
        state
            .set_param_scalar(ClipParam::Opacity, None, 0.4)
            .expect("the copy reads apart")
    });
    assert!(mixed(&state, "transform", ClipParam::Opacity, None));
    assert!(number(&state), "a copy off its default left the reset dim");
    keyframe_rows::apply(&mut state, Row::Reset(ClipParam::Opacity)).expect("the reset lands");
    assert_eq!(
        read(&mut state, twin, &both, ClipParam::Opacity, None),
        1.0,
        "the reset left the copy where it was"
    );
    assert!(
        !number(&state),
        "the reset stayed lit with every clip reset"
    );

    state
        .drop_catalog_on_effect_controls(&effect("color_overlay"), None)
        .expect("the drop lands on both");
    let colour = ClipParam::Effect {
        effect: 0,
        param: 0,
    };
    let swatch = |state: &AppState| {
        let rows = state.effect_param_colors(0);
        let row = rows.first().expect("a colour row");
        (row.mixed, row.off_default)
    };
    assert_eq!(swatch(&state), (false, false), "two default colours");
    on(&mut state, twin, &both, |state| {
        state
            .set_param_color(colour, [0, 0, 255, 255])
            .expect("the copy turns blue")
    });
    assert_eq!(
        swatch(&state),
        (true, true),
        "a copy off its default colour left the reset dim"
    );
    keyframe_rows::apply(&mut state, Row::Reset(colour)).expect("the reset lands");
    assert_eq!(swatch(&state), (false, false), "the copy kept its colour");
}

#[test]
fn a_clip_without_the_property_is_left_alone_and_a_shared_gain_lands_on_both() {
    let mut state = linked_pair();
    let pair = state.selection.clone();
    let [picture, sound] = [pair[0], pair[1]];
    let untouched = model_of(&state, sound);
    let depth = state.history_depth();
    assert!(
        !mixed(&state, "transform", ClipParam::Opacity, None),
        "a clip without opacity made the row mixed"
    );
    state
        .nudge_shared_param(ClipParam::Opacity, None, -0.5)
        .expect("the step lands");
    assert_eq!(
        model_of(&state, sound),
        untouched,
        "a sound clip took an opacity"
    );
    assert_eq!(state.history_depth(), depth + 1);

    on(&mut state, sound, &pair, |state| {
        state
            .set_param_scalar(ClipParam::Volume, None, 0.5)
            .expect("the sound reads apart")
    });
    assert!(mixed(&state, "audio", ClipParam::Volume, None));
    let depth = state.history_depth();
    state
        .nudge_shared_volume_db(-6.0)
        .expect("the gain step lands");
    let landed = read(&mut state, picture, &pair, ClipParam::Volume, None);
    assert!(landed < 1.0, "the drawn clip's gain did not move");
    assert_eq!(
        read(&mut state, sound, &pair, ClipParam::Volume, None),
        landed
    );
    assert!(!mixed(&state, "audio", ClipParam::Volume, None));
    assert_eq!(state.history_depth(), depth + 1);
    assert!(state.undo());
    assert_eq!(read(&mut state, sound, &pair, ClipParam::Volume, None), 0.5);
}

#[test]
fn typing_into_a_title_keeps_every_edit_on_the_title() {
    let (mut state, title, picture) = typing();
    let x = state
        .param_scalar(ClipParam::Position, Some(0))
        .expect("a title stands somewhere");
    state
        .set_param_scalar(ClipParam::Position, Some(0), x + 40.0)
        .expect("the title moves");
    state.add_clip_to_selection(picture);
    assert!(state.text_editing(), "adding a clip ended the typing");
    assert_eq!(state.selected_clip(), Some(title));
    let untouched = model_of(&state, picture);

    assert!(
        !mixed(&state, "transform", ClipParam::Position, Some(0)),
        "rows read the title's characters while typing, not the picture"
    );
    state
        .nudge_shared_param(ClipParam::Position, Some(0), 5.0)
        .expect("the step lands");
    assert_eq!(
        model_of(&state, picture),
        untouched,
        "a step on the typed title moved the picture"
    );

    state.end_text_edit();
    assert!(
        mixed(&state, "transform", ClipParam::Position, Some(0)),
        "out of typing, the picture standing elsewhere reads as mixed"
    );
}

#[test]
fn a_wheel_drag_and_its_reset_reach_every_selected_clip_that_grades() {
    let channels = |state: &mut AppState, clip: ClipId, keep: &[ClipId], control: WheelControl| {
        control
            .rgb_params()
            .map(|param| read(state, clip, keep, ClipParam::Look { param }, None))
    };
    for control in WheelControl::ALL {
        let (mut state, drawn, twin) = twin_pictures();
        let both = [drawn, twin];
        let neutral = channels(&mut state, drawn, &both, control);
        let apart = neutral.map(|value| value + 0.05);
        on(&mut state, twin, &both, |state| {
            for (param, value) in control.rgb_params().into_iter().zip(apart) {
                state
                    .set_param_scalar(ClipParam::Look { param }, None, value)
                    .expect("the copy grades apart");
            }
        });

        let depth = state.history_depth();
        let frame = [400.0, 300.0, 72.0, 72.0];
        state.press_wheel(control, frame, [436.0, 336.0]);
        assert!(state.drag_wheel([436.0 + 18.0, 336.0 - 9.0]));
        assert!(state.release_wheel());
        let landed = channels(&mut state, drawn, &both, control);
        assert_ne!(landed, neutral, "{control:?}: the drag moved nothing");
        assert_eq!(
            channels(&mut state, twin, &both, control),
            landed,
            "{control:?}: the copy did not take the drawn clip's balance"
        );
        assert_eq!(
            state.history_depth(),
            depth + 1,
            "{control:?}: one drag, one step"
        );

        control.reset(&mut state).expect("the reset lands");
        for clip in both {
            assert_eq!(
                channels(&mut state, clip, &both, control),
                neutral,
                "{control:?}: {clip:?} kept a balance through the reset"
            );
        }
        assert_eq!(
            state.history_depth(),
            depth + 2,
            "{control:?}: one reset, one step"
        );
        assert!(state.undo());
        assert!(state.undo());
        assert_eq!(
            channels(&mut state, twin, &both, control),
            apart,
            "{control:?}: two undos left the copy graded"
        );
    }
}

#[test]
fn an_effect_dropped_on_the_panel_lands_on_every_clip_of_the_lane_kind() {
    let mut state = linked_pair();
    let pair = state.selection.clone();
    state
        .drop_catalog_on_effect_controls(&effect("sharpen"), None)
        .expect("the drop lands");
    assert_eq!(chain(&state, pair[0]), ["sharpen"]);
    assert!(
        chain(&state, pair[1]).is_empty(),
        "a clip on a sound lane took a picture effect"
    );

    // A shorter chain takes the drop at its end.
    let (mut state, _) = blurred_clip();
    let (drawn, twin) = twin_selected(&mut state);
    state
        .add_effect_to_selected("vignette")
        .expect("the drawn chain grows");
    let depth = state.history_depth();
    state
        .drop_catalog_on_effect_controls(&effect("grain"), Some(1))
        .expect("the drop lands");
    assert_eq!(chain(&state, drawn), ["gaussian_blur", "grain", "vignette"]);
    assert_eq!(chain(&state, twin), ["gaussian_blur", "grain"]);
    assert_eq!(state.history_depth(), depth + 1, "one drop, one step");
    assert!(state.undo());
    assert_eq!(chain(&state, drawn), ["gaussian_blur", "vignette"]);
    assert_eq!(chain(&state, twin), ["gaussian_blur"]);
}

/// Every effect parameter kind, in declaration order.
const KINDS: [EffectParamKind; 15] = [
    EffectParamKind::Scalar,
    EffectParamKind::Int,
    EffectParamKind::Bool,
    EffectParamKind::Choice,
    EffectParamKind::Color,
    EffectParamKind::Vec2,
    EffectParamKind::Point3,
    EffectParamKind::Size,
    EffectParamKind::Angle,
    EffectParamKind::Curve,
    EffectParamKind::Gradient,
    EffectParamKind::Text,
    EffectParamKind::Font,
    EffectParamKind::File,
    EffectParamKind::Layer,
];

/// A slot of `kind` and the edit that sets it apart from its default, `layer`
/// being a clip a layer slot can name; `None` for a kind a curve carries,
/// whose row compares the values it samples. No wildcard arm: a new kind does
/// not build until it is given one.
fn slot_of(kind: EffectParamKind, layer: ClipId) -> Option<(EffectParamSpec, SlotEdit)> {
    let name = kind.as_str();
    let point = EffectCurvePoint::new;
    let stop = EffectGradientStop::new;
    match kind {
        EffectParamKind::Bool => Some((
            EffectParamSpec::boolean(name, name, false),
            SlotEdit::ToggleBool,
        )),
        EffectParamKind::Choice => Some((
            EffectParamSpec::choice(
                name,
                name,
                vec![
                    EffectChoiceOption::new("a", "A"),
                    EffectChoiceOption::new("b", "B"),
                ],
                "a",
            ),
            SlotEdit::Choice("b".into()),
        )),
        EffectParamKind::Curve => Some((
            EffectParamSpec::curve(name, name, vec![point(0.0, 0.0), point(1.0, 1.0)]),
            SlotEdit::Curve(vec![point(0.0, 0.2), point(1.0, 0.8)]),
        )),
        EffectParamKind::Gradient => Some((
            EffectParamSpec::gradient(
                name,
                name,
                vec![stop(0.0, [0.0, 0.0, 0.0, 1.0]), stop(1.0, [1.0; 4])],
            ),
            SlotEdit::Gradient(vec![
                stop(0.0, [1.0, 0.0, 0.0, 1.0]),
                stop(1.0, [0.0, 0.0, 1.0, 1.0]),
            ]),
        )),
        EffectParamKind::Text => Some((
            EffectParamSpec::text(name, name, "x"),
            SlotEdit::Text("apart".into()),
        )),
        EffectParamKind::Font => Some((
            EffectParamSpec::font(name, name, "Inter"),
            SlotEdit::Text("apart".into()),
        )),
        EffectParamKind::File => Some((
            EffectParamSpec::file(name, name, ""),
            SlotEdit::Text("apart".into()),
        )),
        EffectParamKind::Layer => Some((
            EffectParamSpec::layer(name, name),
            SlotEdit::Layer(Some(layer)),
        )),
        EffectParamKind::Scalar
        | EffectParamKind::Int
        | EffectParamKind::Color
        | EffectParamKind::Vec2
        | EffectParamKind::Point3
        | EffectParamKind::Size
        | EffectParamKind::Angle => None,
    }
}

#[test]
fn an_effect_reads_as_mixed_on_the_slots_the_clips_set_apart() {
    // Registers and drops an effect every other library test can list.
    crate::test_support::catalogs::own_process(|| {
        // The slots no curve carries are the ones the model cannot keyframe.
        for kind in KINDS {
            assert_eq!(
                slot_of(kind, ClipId::from_raw(0)).is_some(),
                !kind.is_keyframeable(),
                "{kind:?}"
            );
        }
        let statics: Vec<EffectParamKind> = KINDS
            .into_iter()
            .filter(|kind| slot_of(*kind, ClipId::from_raw(0)).is_some())
            .collect();
        let mut params: Vec<EffectParamSpec> = statics
            .iter()
            .filter_map(|kind| slot_of(*kind, ClipId::from_raw(0)).map(|(spec, _)| spec))
            .collect();
        let amount_slot = params.len();
        params.push(EffectParamSpec {
            name: "amount".into(),
            label: "Amount".into(),
            kind: EffectParamKind::Scalar,
            default: 0.5,
            min: 0.0,
            max: 1.0,
            ..Default::default()
        });
        let (_guard, _) = register_effect(EffectSpec {
            id: "test.ui.shared_slots".into(),
            label: "Shared Slots".into(),
            category: "Stylize".into(),
            params,
            ..Default::default()
        })
        .expect("register the test effect");
        let (mut state, _) = with_clip();
        state
            .add_effect_to_selected("test.ui.shared_slots")
            .expect("add");
        let (drawn, twin) = twin_selected(&mut state);
        let amount = ClipParam::Effect {
            effect: 0,
            param: amount_slot as u32,
        };
        let amount_mixed = |state: &AppState| {
            state
                .effect_param_rows(0)
                .iter()
                .find(|row| row.param == amount)
                .expect("the amount row")
                .mixed
        };
        assert!(state.effect_slots_mixed(0).is_empty());
        assert!(!amount_mixed(&state));

        // A direct setter edits the drawn clip alone, which sets it apart there.
        for (slot, kind) in statics.iter().enumerate() {
            let (_, edit) = slot_of(*kind, twin).expect("a static kind");
            state
                .edit_selected_effect_slot(0, slot, edit)
                .unwrap_or_else(|err| panic!("{kind:?}: {err}"));
            assert_eq!(state.effect_slots_mixed(0), [slot], "{kind:?} set apart");
            assert!(state.undo());
            assert!(state.effect_slots_mixed(0).is_empty(), "{kind:?} undone");
        }
        state
            .set_selected_effect_param(0, amount_slot, 0.9)
            .expect("amount");
        assert!(amount_mixed(&state), "the amount set apart");

        select(&mut state, &[drawn]);
        assert!(!amount_mixed(&state), "one clip is never mixed");
        state
            .edit_selected_effect_slot(0, 0, SlotEdit::ToggleBool)
            .expect("switch");
        assert!(
            state.effect_slots_mixed(0).is_empty(),
            "one clip is never mixed"
        );

        // A copy holding another effect there is no peer of the slot.
        on(&mut state, twin, &[twin], |state| {
            state.remove_effect(twin, 0).expect("remove");
            state.add_effect_to_selected("vignette").expect("vignette");
        });
        select(&mut state, &[drawn, twin]);
        assert!(state.effect_slots_mixed(0).is_empty());
        assert!(!amount_mixed(&state));
    });
}

#[test]
fn alt_click_toggles_a_driver_only_where_the_drawn_clip_reads_alike() {
    const ROTATION: ClipParam = ClipParam::Rotation;
    let driven = |state: &AppState, clip: ClipId| model_of(state, clip).driver(ROTATION).is_some();
    let (mut state, drawn, twin) = twin_pictures();
    state
        .ctx
        .mount(&aria_plugin::ExpressionsPlugin)
        .expect("mount Expressions");
    let both = [drawn, twin];

    // Neither is driven: both take a driver in one step, and the source box
    // opens on the drawn clip.
    let depth = state.history_depth();
    keyframe_rows::apply(&mut state, Row::Driver(ROTATION)).expect("add");
    assert!(driven(&state, drawn) && driven(&state, twin));
    assert_eq!(state.history_depth(), depth + 1);
    assert_eq!(state.driver_draft().map(|draft| draft.clip), Some(drawn));

    // The copy loses its driver alone, so the drawn clip is driven and the
    // copy is not: a toggle removes the drawn one's and leaves the copy bare.
    on(&mut state, twin, &both, |state| {
        state.toggle_driver(ROTATION).expect("remove the copy's")
    });
    keyframe_rows::apply(&mut state, Row::Driver(ROTATION)).expect("remove");
    assert!(!driven(&state, drawn));
    assert!(
        !driven(&state, twin),
        "a toggle drove the copy the drawn clip left"
    );
}

/// The ids of the effect menu's rows whose press edits the effect: the ones
/// that run through the shared verb, or open the name bar whose commit does.
fn editing_rows() -> Vec<&'static str> {
    let source = include_str!("../surfaces/context_menu/effect.rs");
    source
        .split("MenuItem::Row(MenuRow {")
        .skip(1)
        .filter(|body| body.contains("effect_verb(") || body.contains("begin_rename("))
        .map(|body| {
            let id = body
                .split("id: \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("every row states its id");
            EFFECT_ITEMS
                .iter()
                .filter_map(MenuItem::row)
                .map(|row| row.id)
                .find(|known| *known == id)
                .expect("the source's row is in the table")
        })
        .collect()
}

/// Press the effect menu row `id` opened on the effect at `index` of `clip`,
/// and name the effect when the row opens the name bar.
fn press(state: &mut AppState, clip: ClipId, index: usize, id: &str) {
    let row = EFFECT_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .find(|row| row.id == id)
        .expect("a row of the table");
    let facts = EffectMenu::of(state, clip.raw(), index);
    assert!((row.when)(&facts), "{id} is not offered");
    (row.run)(state, &facts).unwrap_or_else(|err| panic!("{id} was refused: {err}"));
    if state.rename.is_some() {
        while !state.rename_draft.is_empty() {
            state.rename_pop();
        }
        for ch in "Soft".chars() {
            state.rename_push(ch);
        }
        state
            .commit_rename()
            .unwrap_or_else(|err| panic!("{id}: the name was refused: {err}"));
    }
}

/// The blurred picture and a copy of it, both selected, with an effect copied
/// so every row of the menu is offered.
fn blurred_twins() -> (AppState, ClipId, ClipId) {
    let (mut state, _) = blurred_clip();
    let (drawn, twin) = twin_selected(&mut state);
    state.copy_effect(drawn, 0);
    (state, drawn, twin)
}

#[test]
fn every_effect_menu_verb_on_the_drawn_clip_reaches_a_clip_holding_that_effect() {
    let rows: Vec<&'static str> = EFFECT_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .map(|row| row.id)
        .collect();
    let mut edited = Vec::new();
    for id in rows {
        let (mut state, drawn, twin) = blurred_twins();
        let before = the_pair(&state, drawn, twin);
        assert_eq!(before.0, before.1, "the pair starts alike");
        let depth = state.history_depth();
        press(&mut state, drawn, 0, id);
        let after = the_pair(&state, drawn, twin);
        assert_eq!(after.0, after.1, "{id} on the drawn clip parted the pair");
        if after == before {
            assert_eq!(
                state.history_depth(),
                depth,
                "{id} edited nothing but spent a step"
            );
            continue;
        }
        edited.push(id);
        assert_eq!(
            state.history_depth(),
            depth + 1,
            "{id} took more than one step"
        );
        assert!(state.undo());
        assert_eq!(
            the_pair(&state, drawn, twin),
            before,
            "one undo of {id} left a clip edited"
        );
    }
    assert_eq!(
        edited,
        editing_rows(),
        "a row that edits the effect did not edit it in this fixture, or edited it outside the shared verb"
    );
}

#[test]
fn an_effect_menu_verb_reaches_only_the_clip_it_was_opened_on_off_the_drawn_clip() {
    for id in editing_rows() {
        // Opened on the copy, which is selected but not drawn.
        let (mut state, drawn, twin) = blurred_twins();
        let (copy, drawn_model) = (model_of(&state, twin), model_of(&state, drawn));
        press(&mut state, twin, 0, id);
        assert_ne!(model_of(&state, twin), copy, "{id} on the copy did nothing");
        assert_eq!(
            model_of(&state, drawn),
            drawn_model,
            "{id} on the copy reached the drawn clip"
        );

        // Opened on the drawn clip, with the copy holding another effect there.
        let (mut state, drawn, twin) = blurred_twins();
        let both = [drawn, twin];
        on(&mut state, twin, &both, |state| {
            state
                .insert_effect_in_selected("vignette", Some(0))
                .expect("another effect first")
        });
        let copy = model_of(&state, twin);
        press(&mut state, drawn, 0, id);
        assert_eq!(
            model_of(&state, twin),
            copy,
            "{id} reached a clip holding another effect there"
        );
    }
}
