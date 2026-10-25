//! Driving the inspector's audio, animation, speed, template, text and
//! transition blocks: every chip, toggle and stepper they offer, pressed on a
//! clip dressed to have the thing it acts on.
//!
//! # Why
//!
//! The picture blocks became a registry first; these were the rest of the same
//! defect, and the larger half of it: twenty closures and thirteen of the
//! model's commands reachable through no measurable surface. The registry is
//! [`clip_pick`](crate::surfaces::clip_pick), and its arms are read out of its
//! own source here, so an arm added to the enum has no dressed state and no green
//! suite until someone writes one.
//!
//! Each block's chips come from the catalog the panel loops over: the animation
//! choices per slot, the speed presets, the audio roles, the slot-media kinds,
//! the text effects and the installed fonts. Every member is pressed, not one
//! representative.

use std::collections::BTreeSet;

use aria_models::{
    AnimationSlot, AudioRole, ClipId, Generator, MediaId, ParentKeep, SlotMedia, TextAlignH,
    TextAlignV, TextCase, TextStyle,
};

use crate::app::AppState;
use crate::app::clip_effects::PARAM_COLORS;
use crate::app::clip_text::TextSwitch;
use crate::app::fixtures::{the_pair, twin_selected};
use crate::app::sent_census::Watch;
use crate::surfaces::clip_pick::{self, Pick};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "clip_pick.rs";

/// One pick, with the dressing it is pressed against.
struct Staged {
    pick: Staging,
    dress: Dressing,
}

/// How a stage builds the one state its pick is pressed against.
///
/// A function rather than a built state so that one state is alive at a time: a
/// state carries a compositor device, and sixty of them at once exhausts the
/// adapter on a shared machine.
#[derive(Clone, Copy)]
enum Dressing {
    /// What a plain fixture builds.
    Plain(fn() -> AppState),
    /// A clip animating with the one preset that exposes a named knob.
    ///
    /// The knob rows are filtered through `anim_knob_available`, so a knob is
    /// drawn on that preset and on nothing else, and which preset that is comes
    /// out of the catalog rather than out of a list kept here.
    Animating(&'static str),
    /// A clip animating in exactly one slot.
    ///
    /// One slot and not all three: the model excludes a combo animation and the
    /// edge ones from each other, so a clip dressed with all three keeps only
    /// whichever was written last, and clearing either edge would then be a
    /// press on an empty slot.
    AnimatingIn(AnimationSlot),
}

impl Dressing {
    fn build(self) -> AppState {
        match self {
            Self::Plain(build) => build(),
            Self::Animating(knob) => {
                let spec = the_preset_exposing(knob);
                // A per-character knob only exists on a text preset, and the
                // command layer refuses a text preset on anything else.
                let mut state = if spec.text_only { a_title() } else { a_clip() };
                state
                    .set_selected_animation(&spec.id)
                    .unwrap_or_else(|err| panic!("applying {}: {err}", spec.id));
                state
            }
            Self::AnimatingIn(slot) => {
                // A title rather than a media clip: a text clip is offered every
                // preset the catalog ships, so every slot has one to apply.
                let mut state = a_title();
                let choice = state
                    .animation_choices(slot)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| panic!("the catalog ships no {slot:?} animation"));
                state
                    .set_selected_animation(choice.id)
                    .unwrap_or_else(|err| panic!("applying {}: {err}", choice.id));
                state
            }
        }
    }
}

/// How a pick is known.
///
/// Most are known before the state exists. One names something the project has
/// to already hold — a pool item's id — and a media id is process-unique, so an
/// id read out of one build names nothing in the next one. That pick is read off
/// the state it is pressed against, which is also how the panel gets it: the chip
/// carries the id of the pool row it was drawn from.
enum Staging {
    Fixed(Pick),
    OfTheProject(fn(&AppState) -> Pick),
}

impl Staging {
    fn of(self, state: &AppState) -> Pick {
        match self {
            Self::Fixed(pick) => pick,
            Self::OfTheProject(read) => read(state),
        }
    }
}

/// Every pick these blocks offer.
fn staged() -> Vec<Staged> {
    let mut out = Vec::new();
    let mut push = |pick: Pick, dress: fn() -> AppState| {
        out.push(Staged {
            pick: Staging::Fixed(pick),
            dress: Dressing::Plain(dress),
        })
    };

    // Audio. The role chips and the fades are drawn on a clip with sound; the
    // reverse and pitch-lock toggles belong to any media clip.
    push(Pick::ToggleReverse, a_sound_clip);
    push(Pick::TogglePitchLock, a_sound_clip);
    for role in AudioRole::ALL {
        push(Pick::Role(Some(role)), a_sound_clip);
    }
    push(Pick::Role(None), a_tagged_clip);
    push(Pick::FadeIn(2), a_sound_clip);
    push(Pick::FadeOut(2), a_sound_clip);

    // Animation. The ids carry their own slot, so the choices are read per slot,
    // and they are read off a title: a text clip is offered the per-character
    // presets too, so enumerating on a media clip would leave those unpressed.
    for slot in crate::app::clip_anim::SLOTS {
        for choice in a_title().animation_choices(slot) {
            push(Pick::Animation(choice.id), a_title);
        }
    }
    for slot in crate::app::clip_anim::SLOTS {
        out.push(Staged {
            pick: Staging::Fixed(Pick::ClearAnimation(slot)),
            dress: Dressing::AnimatingIn(slot),
        });
    }
    // A knob row is drawn only where the preset exposes that knob, and which
    // knobs a preset has is a catalog fact, so the dressing names the knob and
    // reads the preset out of the catalog rather than searching built states.
    for knob in animation_knobs() {
        let slot = the_preset_exposing(knob).slot;
        out.push(Staged {
            pick: Staging::Fixed(Pick::AnimationKnob {
                slot,
                knob,
                by: 0.25,
            }),
            dress: Dressing::Animating(knob),
        });
    }
    let mut push = |pick: Pick, dress: fn() -> AppState| {
        out.push(Staged {
            pick: Staging::Fixed(pick),
            dress: Dressing::Plain(dress),
        })
    };

    // Speed.
    for spec in crate::app::clip_speed::speed_presets() {
        push(Pick::SpeedPreset(spec.id.clone()), a_clip);
    }
    push(Pick::ClearSpeedCurve, a_ramped_clip);

    // Template.
    out.push(Staged {
        pick: Staging::OfTheProject(|state| Pick::ReplaceMedia(the_spare_source(state))),
        dress: Dressing::Plain(a_clip_and_a_spare_source),
    });
    let mut push = |pick: Pick, dress: fn() -> AppState| {
        out.push(Staged {
            pick: Staging::Fixed(pick),
            dress: Dressing::Plain(dress),
        })
    };
    for accepts in slot_kinds() {
        push(Pick::SlotAccepts(accepts), a_slot);
    }
    push(Pick::ToggleReplaceable, a_clip);
    push(Pick::SlotOrder(1), a_slot);
    push(Pick::ClearSlotLabel, a_labelled_slot);
    push(Pick::ToggleTextEditable, a_title);

    // Text.
    for spec in aria_models::text_effect_catalog() {
        push(Pick::TextEffect(spec.id.clone()), a_title);
    }
    push(Pick::ClearTextEffect, a_title_with_an_effect);
    push(Pick::TextPreset(a_text_preset()), a_title);
    if let Some(family) = a_font() {
        push(Pick::Font(family), a_title);
    }
    for forward in [true, false] {
        push(Pick::CycleFont { forward }, a_title);
    }
    // Character and Paragraph. A choice equal to the one in force is inert, so
    // the default case and alignments are pressed on a title moved off them.
    let fresh = TextStyle::default();
    for switch in TextSwitch::ALL {
        push(Pick::TextSwitch(switch), a_title);
    }
    for case in TextCase::ALL {
        let dress: fn() -> AppState = if case == fresh.case {
            a_title_off_its_default_case
        } else {
            a_title
        };
        push(Pick::TextCase(case), dress);
    }
    for align in TextAlignH::ALL {
        let dress: fn() -> AppState = if align == fresh.align_h {
            a_title_off_its_default_align_h
        } else {
            a_title
        };
        push(Pick::TextAlignH(align), dress);
    }
    for align in TextAlignV::ALL {
        let dress: fn() -> AppState = if align == fresh.align_v {
            a_title_off_its_default_align_v
        } else {
            a_title
        };
        push(Pick::TextAlignV(align), dress);
    }
    // Color swatches, one press per color row, read the way the panel reads
    // them off a title wearing every treatment, off a stroked shape, and off a
    // clip wearing every layer style. Each row is pressed with a swatch other
    // than the color in force, which is inert.
    let styles = crate::app::clip_styles::STYLES
        .iter()
        .map(|(style, _)| (*style, super::picks::a_styled_clip as fn() -> AppState));
    for (group, dress) in COLOR_GROUPS.into_iter().chain(styles) {
        let rows = dress().param_colors(group);
        assert!(!rows.is_empty(), "the {group} block offers no color row");
        for row in rows {
            let rgba = PARAM_COLORS
                .iter()
                .map(|(_, _, rgba)| *rgba)
                .find(|rgba| *rgba != row.value)
                .expect("the swatches hold more than one color");
            push(
                Pick::ParamColor {
                    param: row.param,
                    rgba,
                },
                dress,
            );
        }
    }

    // Transition. A junction needs a neighbour, so the fixture supplies one.
    for spec in aria_models::transition_catalog() {
        push(Pick::Transition(spec.id.clone()), a_junction);
    }
    push(Pick::TransitionLength(1), a_blended_junction);
    for align in aria_models::TransitionAlignment::ALL {
        let dress: fn() -> AppState = if align == aria_models::TransitionAlignment::Center {
            a_blended_junction_off_center
        } else {
            a_blended_junction
        };
        push(Pick::TransitionAlignment(align), dress);
    }
    push(
        Pick::TransitionParam {
            slot: 0,
            edit: crate::surfaces::clip_pick::TransitionSlotEdit::NudgeScalar(0.05),
        },
        a_parameterized_junction,
    );
    push(Pick::ClearTransition, a_blended_junction);

    // Parent. The menu's clips are read off the project the row is drawn from,
    // because a clip id is process-unique; each is pressed kept in place and
    // with Shift held.
    push(Pick::ReleaseParent, a_parented_clip);
    out.push(Staged {
        pick: Staging::OfTheProject(|state| Pick::Parent {
            parent: the_offered_parent(state),
            keep: ParentKeep::Place,
        }),
        dress: Dressing::Plain(a_stacked_clip),
    });
    out.push(Staged {
        pick: Staging::OfTheProject(|state| Pick::Parent {
            parent: the_offered_parent(state),
            keep: ParentKeep::Snap,
        }),
        dress: Dressing::Plain(a_stacked_clip),
    });

    out
}

/// Every command these blocks asked for, one press per staged pick.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in staged() {
        let mut state = stage.dress.build();
        let pick = stage.pick.of(&state);
        let watch = Watch::on(&state);
        let what = pick.what();
        clip_pick::apply(&mut state, pick)
            .unwrap_or_else(|err| panic!("picking {what} was refused: {err}"));
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_pick_the_registry_offers_is_pressed() {
    let pressed: BTreeSet<String> = staged()
        .into_iter()
        .map(|stage| {
            // A pick read off the project needs a project to read: one state,
            // built and dropped, per stage.
            let state = stage.dress.build();
            stage.pick.of(&state).what().to_string()
        })
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these blocks offer picks no dressed clip presses: {missing:?}. The \
         answer is a stage in `staged`, dressed so the pick has something to \
         act on, never an exception"
    );
}

#[test]
fn every_pick_pressed_asks_the_model_for_something() {
    for stage in staged() {
        let mut state = stage.dress.build();
        let pick = stage.pick.of(&state);
        let watch = Watch::on(&state);
        let what = pick.what();
        clip_pick::apply(&mut state, pick)
            .unwrap_or_else(|err| panic!("picking {what} was refused: {err}"));
        assert!(
            !watch.take().is_empty(),
            "picking {what} sent nothing, so either the chip is inert or its \
             stage is dressed with nothing for it to act on"
        );
    }
}

/// With a second clip selected that holds what the drawn clip holds, every
/// pick leaves the two alike, in at most one undo step, and that step takes
/// the pick back on both. The copy sits on a lane of the drawn clip's kind,
/// with a copy of its junction and transition where it has one. The picks are
/// the staged ones, so a pick added to these blocks that reaches the drawn clip
/// alone is red here with no edit to this test.
#[test]
fn every_pick_reaches_a_second_selected_clip_holding_the_same_thing() {
    for stage in staged() {
        let mut state = stage.dress.build();
        // Read before the copy exists, as the chip was drawn before it.
        let pick = stage.pick.of(&state);
        let (drawn, twin) = twin_selected(&mut state);
        let before = the_pair(&state, drawn, twin);
        let what = pick.what();
        assert_eq!(
            before.1, before.0,
            "the copy for {what} differs from its clip"
        );
        let depth = state.history_depth();
        clip_pick::apply(&mut state, pick)
            .unwrap_or_else(|err| panic!("picking {what} on two clips was refused: {err}"));
        let (after, twin_after) = the_pair(&state, drawn, twin);
        assert_eq!(
            twin_after, after,
            "{what} reached the drawn clip and not the other selected clip"
        );
        let steps = state.history_depth() - depth;
        assert!(steps <= 1, "{what} on two clips took {steps} undo steps");
        if steps == 1 {
            assert!(state.undo(), "{what} left nothing to undo");
            assert_eq!(
                the_pair(&state, drawn, twin),
                before,
                "one undo did not take {what} back on both clips"
            );
        }
    }
}

#[test]
fn the_census_reaches_every_command_these_blocks_claim() {
    let sent = dispatched();
    for label in [
        "Set audio levels",
        "Set pitch lock",
        "Set audio role",
        "Set animation",
        "Set speed",
        "Set speed curve",
        "Replace clip media",
        "Mark replaceable",
        "Mark text editable",
        "Edit generator",
        "Add transition",
        "Remove transition",
        "Set transition length",
        "Set transition alignment",
        "Set transition parameters",
        "Set parent",
    ] {
        assert!(
            sent.contains(label),
            "these blocks sent {sent:?}, which does not include `{label}`"
        );
    }
}

#[test]
fn a_pick_with_nothing_selected_is_inert() {
    for stage in staged() {
        let mut state = stage.dress.build();
        // The pick is read before the selection goes, because that is the order
        // a click happens in: the chip was drawn from a project that
        // had a selection.
        let pick = stage.pick.of(&state);
        state.clear_selection();
        let watch = Watch::on(&state);
        let what = pick.what();
        clip_pick::apply(&mut state, pick)
            .unwrap_or_else(|err| panic!("picking {what} with no selection: {err}"));
        let sent = watch.take();
        assert!(
            sent.is_empty(),
            "picking {what} with nothing selected sent {sent:?}"
        );
    }
}

#[test]
fn these_blocks_call_no_setter_of_their_own() {
    super::no_setter_escapes(REGISTRY, "inspector");
}

#[test]
fn every_catalog_these_blocks_loop_over_has_members() {
    for (label, count) in [
        ("audio roles", AudioRole::ALL.len()),
        ("slot media kinds", slot_kinds().len()),
        (
            "speed presets",
            crate::app::clip_speed::speed_presets().len(),
        ),
        ("text effects", aria_models::text_effect_catalog().len()),
        ("transitions", aria_models::transition_catalog().len()),
        ("animation knobs", animation_knobs().len()),
        ("text colors", a_dressed_title().param_colors("text").len()),
        (
            "shape colors",
            super::inspector::a_shape().param_colors("shape").len(),
        ),
    ] {
        assert!(count > 0, "the {label} these blocks loop over are empty");
    }
    assert!(
        !a_clip().animation_choices(AnimationSlot::In).is_empty(),
        "the entrance slot offers no animation, so the block draws no chips"
    );
    assert!(
        a_font().is_some(),
        "this host has no fonts installed, so the font rows cannot be measured \
         here: the list comes from the system, not from a fixture"
    );
}

// --- dressed states ------------------------------------------------------

fn a_clip() -> AppState {
    crate::app::premiere_tests::with_clip().0
}

/// A clip with sound, on a lane that can hold it: an audio role is a property of
/// an audio track, so the model refuses one on a clip that landed on a video
/// lane. The fades, the role chips, reverse and pitch lock are all drawn here.
fn a_sound_clip() -> AppState {
    crate::app::fixtures::with_audio().0
}

fn a_tagged_clip() -> AppState {
    let mut state = a_sound_clip();
    let role = AudioRole::ALL
        .into_iter()
        .next()
        .expect("the model ships a role");
    state
        .set_selected_audio_role(Some(role))
        .expect("the tag lands");
    state
}

/// The animation knobs the panel steps, read out of the same table it loops
/// over.
fn animation_knobs() -> Vec<&'static str> {
    crate::app::clip_anim::ANIM_KNOBS
        .iter()
        .map(|(id, ..)| *id)
        .collect()
}

/// The one preset a knob row is drawn on.
///
/// The panel filters the knob rows through `anim_knob_available`, so a knob
/// pressed on a preset without it is a row nobody drew, and the model's own
/// setter is inert there. Which preset carries which knob is a catalog fact, so
/// it is read out of the catalog: the first spec in catalog order that exposes
/// it, which is also the first one the inspector would draw the row under.
fn the_preset_exposing(knob: &'static str) -> &'static aria_models::AnimationSpec {
    aria_models::animation_catalog()
        .iter()
        .find(|spec| exposes(spec, knob))
        .unwrap_or_else(|| {
            panic!(
                "no animation preset exposes `{knob}`, so the inspector draws a \
                 row nothing can reach"
            )
        })
}

/// Whether a preset exposes the knob `id` names, read off the spec's own flags.
fn exposes(spec: &aria_models::AnimationSpec, id: &str) -> bool {
    match id {
        "anim-speed" => spec.knobs.speed,
        "anim-intensity" => spec.knobs.intensity,
        "anim-stagger" => spec.knobs.stagger,
        other => panic!("the knob table gained `{other}` and this census cannot dress it"),
    }
}

fn a_ramped_clip() -> AppState {
    let mut state = a_clip();
    let preset = &crate::app::clip_speed::speed_presets()
        .first()
        .expect("the model ships a ramp")
        .id;
    state
        .set_selected_speed_preset(preset)
        .expect("the ramp lands");
    state
}

/// A project with a second source in the pool for the selected clip to be
/// repointed at.
fn a_clip_and_a_spare_source() -> AppState {
    let mut state = a_clip();
    state
        .import_media(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/fixture-640x360-24fps-3s.mp4"),
        )
        .expect("import a second source to swap in");
    state.sync_from_engine();
    state
}

/// The pool item the selected clip is not already playing, which is what the
/// swap chip is drawn from.
///
/// Read off the project rather than remembered from the import: a media id is
/// process-unique, so an id kept from an earlier build names nothing here.
fn the_spare_source(state: &AppState) -> MediaId {
    let playing = state.selected_ui_clip().and_then(|clip| clip.media_id);
    let spare = state
        .store
        .media
        .iter()
        .map(|item| item.id)
        .find(|id| playing != Some(*id))
        .expect("the dressing imported a second pool item");
    MediaId::from_raw(spare)
}

/// A clip with another clip over the same span on the lane above, so the
/// Parent row offers that one.
fn a_stacked_clip() -> AppState {
    crate::app::fixtures::stacked_pair().0
}

fn a_parented_clip() -> AppState {
    crate::app::fixtures::parented_pair().0
}

/// The first clip the Parent row offers the selected clip, read the way the
/// row reads it.
fn the_offered_parent(state: &AppState) -> ClipId {
    *state
        .selected_parent_choices()
        .expect("a selected clip")
        .eligible
        .first()
        .expect("the Parent row offers a clip")
}

/// What a template slot may be restricted to, read out of the same choices the
/// panel loops over: an audio clip is offered fewer kinds than a visual one, and
/// a chip the model would refuse is never drawn.
fn slot_kinds() -> Vec<SlotMedia> {
    a_clip()
        .selected_slot_choices()
        .into_iter()
        .map(|(kind, _)| kind)
        .collect()
}

/// The clip marked as a template slot, which is what the accepts chips, the
/// order stepper and the hint act on.
fn a_slot() -> AppState {
    let mut state = a_clip();
    state
        .toggle_selected_replaceable()
        .expect("the slot mark lands");
    state
}

fn a_labelled_slot() -> AppState {
    let mut state = a_slot();
    state
        .set_selected_slot_label("Logo here")
        .expect("the hint lands");
    state
}

fn a_title() -> AppState {
    let mut state = a_clip();
    state
        .add_generated(Generator::text("Title"))
        .expect("the title lands on the timeline");
    let placed = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .find(|clip| clip.generator.is_some())
        .expect("the title is on the timeline")
        .id;
    state.select_clip(aria_models::ClipId::from_raw(placed));
    state
}

/// The groups whose color rows the swatches are pressed on, with the clip that
/// has every color of the group. The layer styles join them from
/// `clip_styles::STYLES` at run time, so a style added there is pressed too.
const COLOR_GROUPS: [(&str, fn() -> AppState); 2] = [
    ("text", a_dressed_title),
    ("shape", super::inspector::a_shape),
];

/// A title wearing the outline, the shadow and the card, so every colour the
/// Character panel draws has a row.
fn a_dressed_title() -> AppState {
    let mut state = a_title();
    for switch in TextSwitch::TREATMENTS {
        let face = state.selected_text_face().expect("a title is selected");
        if !face.is_on(switch) {
            state
                .toggle_selected_text_switch(switch)
                .expect("the treatment lands");
        }
    }
    state
}

fn a_title_off_its_default_case() -> AppState {
    let mut state = a_title();
    let other = TextCase::ALL
        .into_iter()
        .find(|case| *case != TextStyle::default().case)
        .expect("the model ships more than one case");
    state.set_selected_text_case(other).expect("the case lands");
    state
}

fn a_title_off_its_default_align_h() -> AppState {
    let mut state = a_title();
    let other = TextAlignH::ALL
        .into_iter()
        .find(|align| *align != TextStyle::default().align_h)
        .expect("the model ships more than one alignment");
    state
        .set_selected_text_align_h(other)
        .expect("the alignment lands");
    state
}

fn a_title_off_its_default_align_v() -> AppState {
    let mut state = a_title();
    let other = TextAlignV::ALL
        .into_iter()
        .find(|align| *align != TextStyle::default().align_v)
        .expect("the model ships more than one alignment");
    state
        .set_selected_text_align_v(other)
        .expect("the alignment lands");
    state
}

fn a_title_with_an_effect() -> AppState {
    let mut state = a_title();
    let id = &aria_models::text_effect_catalog()
        .first()
        .expect("the model ships a text effect")
        .id;
    state
        .set_selected_text_effect(id)
        .expect("the effect lands");
    state
}

/// A style worth applying: the fields a preset carries, filled from the model's
/// own catalogs so nothing here names a font or an animation the app lacks.
fn a_text_preset() -> aria_cloud::dto::TextPreset {
    aria_cloud::dto::TextPreset {
        id: "census".to_string(),
        name: "Census".to_string(),
        category: String::new(),
        font_family: String::new(),
        font_size: 72.0,
        fill: [255, 255, 255, 255],
        animation_in: None,
        animation_out: None,
        animation_combo: None,
        sample_text: String::new(),
    }
}

/// The first installed font family, or none on a host with no fonts.
fn a_font() -> Option<String> {
    a_clip().font_families().first().cloned()
}

/// Two clips abutting on one lane, the left one selected: a transition needs a
/// junction, and a junction needs a neighbour.
fn a_junction() -> AppState {
    let (mut state, clip) = crate::app::fixtures::blended_junction();
    state.select_clip(clip);
    state
        .clear_selected_transition()
        .expect("taking the fixture's blend off");
    state
}

fn a_blended_junction() -> AppState {
    let (mut state, clip) = crate::app::fixtures::blended_junction();
    state.select_clip(clip);
    state
}

fn a_blended_junction_off_center() -> AppState {
    let mut state = a_blended_junction();
    state
        .set_selected_transition_alignment(aria_models::TransitionAlignment::Start)
        .expect("the start alignment lands");
    state
}

fn a_parameterized_junction() -> AppState {
    let mut state = a_blended_junction();
    let spec = aria_models::transition_catalog()
        .into_iter()
        .find(|spec| !spec.params.is_empty())
        .expect("the transition catalog ships a parameterized blend");
    state
        .set_selected_transition(&spec.id)
        .expect("the parameterized blend lands");
    state
}
