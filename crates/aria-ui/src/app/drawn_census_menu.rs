//! Driving the context menu's tables, one row at a time.
//!
//! Same rule as the parent module: press the row's own `run`, never a
//! re-implementation of it. Two more rules this file keeps.
//!
//! A row is pressed against a state of its own, because an edit that lands
//! changes what the next row would ask for, and the census is after what each
//! control sends rather than what a sequence of clicks sends.
//!
//! And a row is pressed only in a state where the menu would have offered it,
//! against facts read by the registry's own constructors. A census that presses a
//! row the menu keeps hidden measures a click nobody can make; one that reads the
//! facts its own way measures itself. Every row must be offered by at least one
//! of the dressed states below, so a row that no state can reach turns this red
//! rather than going quietly unmeasured.

use std::collections::BTreeSet;
use std::mem::discriminant;
use std::path::PathBuf;

use aria_models::{ClipParam, Easing};

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::surfaces::context_menu::{
    BADGE_ITEMS, BadgeMenu, CLIP_ITEMS, ClipMenu, EFFECT_ITEMS, EffectMenu, KEYFRAME_ITEMS,
    KeyframeMenu, MASK_PATH_KEY_ITEMS, MaskPathKeyMenu, MenuItem, MenuRow, POOL_ITEMS,
    PREVIEW_ITEMS, PoolMenu, RECENT_ITEMS, TRACK_ITEMS, TRANSITION_ITEMS, TrackMenu, apply_blend,
    draw_property,
};
use crate::types::ActivePanel;

use crate::app::fixtures;

/// What pressing every table produced.
struct Pressed {
    /// Commands the rows dispatched.
    sent: BTreeSet<&'static str>,
    /// Rows that were offered somewhere, and did nothing wherever they were
    /// pressed.
    inert: Vec<&'static str>,
    /// Rows no dressed state offered, so nothing pressed them.
    unoffered: Vec<&'static str>,
    /// Rows the census pressed at least once, in table order.
    ids: Vec<&'static str>,
    /// Each row's id and the text it draws, once per dressed state that offers
    /// it, rows that leave the editor included.
    labels: Vec<(&'static str, &'static str)>,
    /// Every state the census saw a row that draws a check mark in: checked or
    /// not, in a dressed state and in the state each press of its table left.
    checks: BTreeSet<(&'static str, bool)>,
    /// Each press of a row that draws a check mark: its id, and whether it was
    /// checked before the press and after it.
    toggled: Vec<(&'static str, bool, bool)>,
}

/// The facts a table's rows read, gathered again from the state a press left,
/// as a menu opened on the same target would gather them, or `None` when the
/// target is gone.
trait Reread: Clone {
    fn reread(&self, state: &AppState) -> Option<Self>;
}

impl Reread for ClipMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state, self.clip))
    }
}

impl Reread for TrackMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.track)
    }
}

impl Reread for PoolMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state, self.media))
    }
}

impl Reread for EffectMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Some(Self::of(state, self.clip, self.index))
    }
}

impl Reread for BadgeMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.clip)
    }
}

impl Reread for KeyframeMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.clip.raw(), self.param, self.tick)
    }
}

impl Reread for MaskPathKeyMenu {
    fn reread(&self, state: &AppState) -> Option<Self> {
        Self::of(state, self.clip.raw(), self.mask, self.tick)
    }
}

/// The preview menu reads no facts.
impl Reread for () {
    fn reread(&self, _: &AppState) -> Option<Self> {
        Some(())
    }
}

/// A recents menu reads only the path it was opened on.
impl Reread for PathBuf {
    fn reread(&self, _: &AppState) -> Option<Self> {
        Some(self.clone())
    }
}

/// One dressed state a table is pressed against, with the facts a menu opened on
/// it would have gathered.
struct Stage<F> {
    /// What the state holds, so a failure can name it.
    what: &'static str,
    dress: fn() -> (AppState, F),
}

/// Every command the context menu's rows dispatched.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    pressed().sent
}

/// Press every pressable row in every table, in every state that offers it.
fn pressed() -> Pressed {
    let mut out = Pressed {
        sent: BTreeSet::new(),
        inert: Vec::new(),
        unoffered: Vec::new(),
        ids: Vec::new(),
        labels: Vec::new(),
        checks: BTreeSet::new(),
        toggled: Vec::new(),
    };
    press(CLIP_ITEMS, CLIP_STAGES, &mut out);
    press(TRANSITION_ITEMS, JUNCTION_STAGES, &mut out);
    press(TRACK_ITEMS, LANE_STAGES, &mut out);
    press(POOL_ITEMS, POOL_STAGES, &mut out);
    press(PREVIEW_ITEMS, PREVIEW_STAGES, &mut out);
    press(RECENT_ITEMS, RECENT_STAGES, &mut out);
    press(EFFECT_ITEMS, EFFECT_STAGES, &mut out);
    press(BADGE_ITEMS, BADGE_STAGES, &mut out);
    press(KEYFRAME_ITEMS, KEYFRAME_STAGES, &mut out);
    press(MASK_PATH_KEY_ITEMS, PATH_KEY_STAGES, &mut out);
    out.sent.extend(blends());
    out.sent.extend(properties());
    out
}

/// Press every property a badge menu lists, on a clip with an effect and a
/// mask, so each kind of property row is driven.
///
/// The property rows are generated from the clip, so what is driven is the call
/// they are built around, [`draw_property`]. One state serves every row: the
/// rows differ only in the property they name, and choosing one changes no
/// other row's request.
fn properties() -> BTreeSet<&'static str> {
    let (mut state, menu) = (BADGE_STAGES[0].dress)();
    let watch = crate::app::sent_census::Watch::on(&state);
    for property in menu.properties.iter() {
        draw_property(&mut state, &menu, property.param).expect("the clip's own property is drawn");
    }
    watch.take().into_iter().collect()
}

/// Press every blend the catalog offers, on a junction that can carry one.
///
/// The blend rows are generated from the catalog rather than listed in a table,
/// so what is driven is the call the generated rows are built around,
/// [`apply_blend`].
fn blends() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for spec in crate::app::clip_transitions::transition_choices() {
        let (mut state, junction) = fixtures::blended_junction();
        let facts = ClipMenu::of(&state, junction.raw());
        let watch = crate::app::sent_census::Watch::on(&state);
        apply_blend(&mut state, &facts, &spec.id).expect("the catalog's own blend is accepted");
        out.extend(watch.take());
    }
    out
}

/// Press one table across its states, and read the text of every row each
/// state offers. A row that leaves the editor is read and never pressed. After
/// each press the facts are read again, and every check mark the table draws
/// is read against them.
fn press<F: Reread>(items: &'static [MenuItem<F>], stages: &[Stage<F>], out: &mut Pressed) {
    let checkable = || {
        items
            .iter()
            .filter_map(MenuItem::row)
            .filter(|row| row.checked.is_some())
    };
    for entry in items {
        let Some(row) = entry.row() else {
            continue;
        };
        let outside = matches!(entry, MenuItem::Outside(_));
        let mut offered = false;
        let mut acted = false;
        for stage in stages {
            let (mut state, facts) = (stage.dress)();
            if !(row.when)(&facts) {
                continue;
            }
            out.labels.push((row.id, (row.label)(&facts)));
            if row.checked.is_some() {
                out.checks.insert((row.id, row.is_checked(&facts)));
            }
            if outside {
                continue;
            }
            // Dressed away from the panel a row switches to, so a row whose whole
            // effect is "show the inspector" has an effect to show.
            state.active_panel = ActivePanel::Timeline;
            offered = true;
            let before = fingerprint(&state);
            let watch = crate::app::sent_census::Watch::on(&state);
            let refused = (row.run)(&mut state, &facts).is_err();
            let sent = watch.take();
            acted |= refused || !sent.is_empty() || fingerprint(&state) != before;
            out.sent.extend(sent);
            if let Some(after) = facts.reread(&state) {
                for other in checkable() {
                    out.checks.insert((other.id, other.is_checked(&after)));
                }
                if row.checked.is_some() {
                    out.toggled
                        .push((row.id, row.is_checked(&facts), row.is_checked(&after)));
                }
            }
            let _ = stage.what;
        }
        if outside {
            continue;
        }
        if !offered {
            out.unoffered.push(row.id);
            continue;
        }
        out.ids.push(row.id);
        if !acted {
            out.inert.push(row.id);
        }
    }
}

/// The visible session state, in one string.
///
/// A menu row either edits the project, moves the session, refuses, or does
/// nothing at all; this is how the last case is told from the other three
/// without naming a field per row.
fn fingerprint(state: &AppState) -> String {
    format!(
        "{} {} {} {:?} {:?} {:?} {:?} {:?} {:?} {} {} {} {} {} {} {} {} {} {} {:?}",
        state.revision,
        state.playhead.value,
        state.selection.len(),
        state.zoom.to_bits(),
        state.scroll_x.to_bits(),
        discriminant(&state.active_panel),
        discriminant(&state.dialog),
        discriminant(&state.route),
        state.status_message,
        state.last_error.is_some(),
        state.rename.is_some(),
        state.clipboard.is_some(),
        state.context_menu.is_some(),
        state.preset_draft.is_some(),
        state.pending_intent.is_some(),
        state.picks_asked(),
        state.recents.len(),
        state.ui_settings.sidebar_collapsed,
        state.ui_settings.inspector_collapsed,
        state.dock_tree,
    )
}

/// A clip menu opens over a still, over a clip with a picture, and over a linked
/// pair: three states between them offer every clip row.
const CLIP_STAGES: &[Stage<ClipMenu>] = &[
    Stage {
        what: "a still image on the timeline",
        dress: || {
            let (state, clip) = with_clip();
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
    Stage {
        what: "a clip with a picture and sound",
        dress: || {
            let (state, clip) = fixtures::with_video();
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
    Stage {
        what: "a clip the editor would lift rather than ripple",
        dress: || {
            let (mut state, clip) = with_clip();
            state.edit_mode = crate::types::EditMode::Overwrite;
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
    Stage {
        what: "two clips, linked",
        dress: || {
            let state = fixtures::linked_pair();
            let clip = state
                .selection
                .first()
                .copied()
                .expect("the pair is selected");
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
    Stage {
        what: "a clip with a parent",
        dress: || {
            let (state, clip) = fixtures::parented_pair();
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
    Stage {
        what: "two clips, grouped",
        dress: || {
            let mut state = fixtures::linked_pair();
            state
                .unlink_selected()
                .expect("the fixture's pair comes apart");
            state.group_selected().expect("the pair forms a group");
            let clip = state
                .selection
                .first()
                .copied()
                .expect("the group is selected");
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
];

/// A junction menu only makes sense over a junction that carries a blend.
const JUNCTION_STAGES: &[Stage<ClipMenu>] = &[
    Stage {
        what: "a junction carrying a blend",
        dress: || {
            let (state, clip) = fixtures::blended_junction();
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
    Stage {
        what: "a junction carrying a start-aligned blend",
        dress: || {
            let (mut state, clip) = fixtures::blended_junction();
            state
                .set_clip_transition_alignment(clip, aria_models::TransitionAlignment::Start)
                .expect("the start alignment lands");
            let facts = ClipMenu::of(&state, clip.raw());
            (state, facts)
        },
    },
];

/// A lane menu opens on the lane the pointer is over: a picture lane, a sound
/// lane, and one with lanes on both sides of it.
const LANE_STAGES: &[Stage<TrackMenu>] = &[
    Stage {
        what: "the only video lane",
        dress: || {
            let (state, _) = with_clip();
            let lane = first_lane(&state);
            let facts = TrackMenu::of(&state, lane).expect("the lane is projected");
            (state, facts)
        },
    },
    Stage {
        what: "an audio lane",
        dress: || {
            let (state, _) = fixtures::with_audio();
            let lane = state
                .store
                .sequence
                .tracks
                .iter()
                .find(|lane| lane.kind == crate::types::UiTrackKind::Audio)
                .map(|lane| lane.id)
                .expect("the audio fixture lands on a sound lane");
            let facts = TrackMenu::of(&state, lane).expect("the lane is projected");
            (state, facts)
        },
    },
    Stage {
        what: "a lane with lanes above and below it",
        dress: || {
            let (state, lane) = fixtures::middle_lane();
            let facts = TrackMenu::of(&state, lane).expect("the lane is projected");
            (state, facts)
        },
    },
];

/// A pool menu opens on an item the timeline holds, one nothing holds, and one
/// already decoding through a proxy.
const POOL_STAGES: &[Stage<PoolMenu>] = &[
    Stage {
        what: "an item a clip holds",
        dress: || {
            let (state, _) = with_clip();
            let media = first_media(&state);
            let facts = PoolMenu::of(&state, media);
            (state, facts)
        },
    },
    Stage {
        what: "an item nothing holds",
        dress: || {
            let (state, media) = fixtures::free_pool_item();
            let facts = PoolMenu::of(&state, media);
            (state, facts)
        },
    },
    Stage {
        what: "an item decoding through a proxy",
        dress: || {
            let (state, media) = fixtures::proxied_pool_item();
            let facts = PoolMenu::of(&state, media);
            (state, facts)
        },
    },
];

/// The preview menu reads nothing, so one state is every state.
const PREVIEW_STAGES: &[Stage<()>] = &[Stage {
    what: "a timeline with a clip on it",
    dress: || {
        let (state, _) = with_clip();
        (state, ())
    },
}];

/// A recents menu opens on a project that is on the list.
const RECENT_STAGES: &[Stage<PathBuf>] = &[Stage {
    what: "a project on the recents list",
    dress: fixtures::remembered_project,
}];

/// An effect menu opens on a blur turned up from its default, with nothing
/// copied and with an effect copied, so the paste row is offered in one.
const EFFECT_STAGES: &[Stage<EffectMenu>] = &[
    Stage {
        what: "a blur turned up, nothing copied",
        dress: || {
            let (state, clip) = fixtures::blurred_clip();
            let facts = EffectMenu::of(&state, clip.raw(), 0);
            (state, facts)
        },
    },
    Stage {
        what: "a blur turned up, an effect copied",
        dress: || {
            let (mut state, clip) = fixtures::blurred_clip();
            state.copy_effect(clip, 0);
            state.status_message = None;
            let facts = EffectMenu::of(&state, clip.raw(), 0);
            (state, facts)
        },
    },
];

/// A badge menu opens on a clip carrying an effect and a mask, so it lists a
/// property of every block.
const BADGE_STAGES: &[Stage<BadgeMenu>] = &[Stage {
    what: "a blurred clip with a rectangle mask",
    dress: || {
        let (mut state, clip) = fixtures::blurred_clip();
        state
            .add_selected_mask(aria_models::MaskKind::Rectangle)
            .expect("the fixture's clip takes a mask");
        state.sync_from_engine();
        let facts = BadgeMenu::of(&state, clip.raw()).expect("the clip is projected");
        (state, facts)
    },
}];

/// A keyframe's menu opens on the middle of three Position keyframes: linear,
/// roving with keyframes copied, and held.
const KEYFRAME_STAGES: &[Stage<KeyframeMenu>] = &[
    Stage {
        what: "a linear keyframe between two others",
        dress: || fixtures::keyframe_menu(|_, _, _| {}),
    },
    Stage {
        what: "a roving keyframe, keyframes copied",
        dress: || {
            fixtures::keyframe_menu(|state, clip, tick| {
                state
                    .toggle_roving_keyframe(clip, ClipParam::Position, tick)
                    .expect("the middle keyframe roves");
                state.copy_selected_keyframes();
                state.status_message = None;
            })
        },
    },
    Stage {
        what: "a held keyframe",
        dress: || {
            fixtures::keyframe_menu(|state, _, _| {
                state
                    .toggle_hold_selected_or_playhead()
                    .expect("the keyframe holds");
            })
        },
    },
];

/// A Path keyframe's menu opens on the first of two keyframes, linear and held.
const PATH_KEY_STAGES: &[Stage<MaskPathKeyMenu>] = &[
    Stage {
        what: "a linear Path keyframe",
        dress: || fixtures::path_key_menu(|_, _, _| {}),
    },
    Stage {
        what: "a held Path keyframe",
        dress: || {
            fixtures::path_key_menu(|state, clip, tick| {
                state
                    .ease_mask_path_key(clip, 0, tick, Easing::Hold)
                    .expect("the Path keyframe holds");
            })
        },
    },
];

/// The lane the fixture projects first.
fn first_lane(state: &AppState) -> u64 {
    state
        .store
        .sequence
        .tracks
        .first()
        .expect("the fixture projects a lane")
        .id
}

/// The pool item the fixture imported.
fn first_media(state: &AppState) -> u64 {
    state
        .store
        .media
        .first()
        .expect("the fixture imports one pool item")
        .id
}

#[test]
fn no_menu_row_is_inert() {
    // A row that asks for nothing, refuses nothing and changes nothing is a dead
    // control: it draws, it highlights, it closes the menu, and the project is
    // exactly as it was.
    let pressed = pressed();
    assert!(
        pressed.inert.is_empty(),
        "these menu rows did nothing wherever they were offered: {:?}",
        pressed.inert
    );
}

#[test]
fn every_menu_row_is_offered_by_some_dressed_state() {
    // Fail by default: a row whose `when` no state here satisfies is a row the
    // census cannot press, which is how a surface goes unmeasured. The answer is a
    // state that offers it, not an exception.
    let pressed = pressed();
    assert!(
        pressed.unoffered.is_empty(),
        "no dressed state offers these menu rows, so nothing pressed them: {:?}",
        pressed.unoffered
    );
}

#[test]
fn the_census_presses_every_row_it_can() {
    // Guards the loop in `pressed`: a `press` call dropped from it would leave a
    // whole table unmeasured while every other test stayed green.
    let rows = all_ids().len() - outside_rows().len();
    assert_eq!(
        pressed().ids.len(),
        rows,
        "the census pressed a different number of rows than the tables declare"
    );
}

/// Every context menu row is in Title Case, as the menu bar's rows and the
/// palette's entries are.
///
/// The class closed here: a row in sentence case beside Title Case ones, as
/// `Fit timeline`, `Remove from project` and `Delete file` were. The text is
/// read from each row's own label in every dressed state that offers it, so a
/// row whose label follows the facts is read in each state the census reaches,
/// and a new row or table is read on the run that adds it.
///
/// Not covered: the blend and property rows, whose text is catalog data, and a
/// label variant no dressed state reaches.
#[test]
fn every_context_menu_row_is_in_title_case() {
    let labels = pressed().labels;
    let read: BTreeSet<&str> = labels.iter().map(|&(id, _)| id).collect();
    let unread: Vec<&str> = all_ids()
        .into_iter()
        .filter(|id| !read.contains(id))
        .collect();
    assert!(unread.is_empty(), "the census read no label for {unread:?}");
    let offenders: Vec<String> = labels
        .iter()
        .filter_map(|&(id, label)| {
            crate::commands::casing_tests::check_title_case(label)
                .err()
                .map(|reason| format!("{id}: {reason}"))
        })
        .collect();
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// A row that switches a state draws a check mark while the state holds, and
/// its label reads the same in both states.
///
/// The class closed here: a toggle that drew no state, as Enable did, so the
/// row read the same over an enabled clip and a disabled one; and a choice that
/// spelled its state with a `• ` prefix, as the lane heights and the transition
/// alignments did. Every row that declares `checked` must be seen checked and
/// seen unchecked, across the dressed states and the states each press of its
/// table leaves, and a press on an unchecked row must check it. The rows are
/// the ones the census reads, so a row that declares `checked` later is held to
/// both on the run that adds it.
///
/// Not covered: a check that reads the opposite of its state, which
/// `a_toggle_row_is_checked_while_its_state_holds` and the lane height and
/// alignment tests catch, and the blend and property rows, whose checks
/// `panels::context_menu::grid_tests` reads from the drawn menu.
#[test]
fn a_row_that_switches_a_state_shows_it_with_a_check_mark() {
    let pressed = pressed();
    let spelled: Vec<String> = pressed
        .labels
        .iter()
        .filter(|(_, label)| label.contains('•'))
        .map(|(id, label)| format!("{id}: {label:?}"))
        .collect();
    assert!(
        spelled.is_empty(),
        "these rows spell their state in their label instead of a check mark:\n{}",
        spelled.join("\n")
    );
    let checkable: BTreeSet<&str> = pressed.checks.iter().map(|&(id, _)| id).collect();
    assert!(
        checkable.contains("ctx-enable"),
        "the census read no Enable check mark: {checkable:?}"
    );
    let one_sided: Vec<String> = checkable
        .iter()
        .filter_map(|&id| {
            let seen = |checked| pressed.checks.contains(&(id, checked));
            match (seen(true), seen(false)) {
                (true, true) => None,
                (true, false) => Some(format!("{id} was never seen unchecked")),
                (false, _) => Some(format!("{id} was never seen checked")),
            }
        })
        .collect();
    assert!(one_sided.is_empty(), "{}", one_sided.join("\n"));
    let unmoved: Vec<&str> = pressed
        .toggled
        .iter()
        .filter(|&&(_, before, after)| !before && !after)
        .map(|&(id, _, _)| id)
        .collect();
    assert!(
        unmoved.is_empty(),
        "a press on these unchecked rows left them unchecked: {unmoved:?}"
    );
}

/// Enable, Time Remapping and Duck Source are checked exactly while the state
/// their press switches holds, as a menu opened on the target reads it.
///
/// The class closed here: a check that reads the opposite of its state, which
/// the census cannot tell from a right one, since both are seen checked and
/// unchecked.
#[test]
fn a_toggle_row_is_checked_while_its_state_holds() {
    let (mut state, clip) = with_clip();
    let enable = row_in(CLIP_ITEMS, "ctx-enable");
    let remap = row_in(CLIP_ITEMS, "ctx-time-remap");
    let facts = ClipMenu::of(&state, clip.raw());
    assert!(
        enable.is_checked(&facts),
        "an enabled clip drew Enable unchecked"
    );
    assert!(
        !remap.is_checked(&facts),
        "a clip at flat speed drew Time Remapping checked"
    );
    state.toggle_enabled_selected().expect("the clip disables");
    state.toggle_time_remap().expect("the clip takes a ramp");
    let facts = ClipMenu::of(&state, clip.raw());
    assert!(
        !enable.is_checked(&facts),
        "a disabled clip drew Enable checked"
    );
    assert!(
        remap.is_checked(&facts),
        "a ramped clip drew Time Remapping unchecked"
    );

    let (mut state, _) = fixtures::with_audio();
    let lane = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|lane| lane.kind == crate::types::UiTrackKind::Audio)
        .map(|lane| lane.id)
        .expect("the audio fixture lands on a sound lane");
    let duck = row_in(TRACK_ITEMS, "ctx-track-duck");
    let facts = |state: &AppState| TrackMenu::of(state, lane).expect("the lane is projected");
    assert!(
        !duck.is_checked(&facts(&state)),
        "a lane that is not the voice drew Duck Source checked"
    );
    state
        .toggle_track_duck_source(aria_models::TrackId::from_raw(lane))
        .expect("the lane becomes the voice");
    assert!(
        duck.is_checked(&facts(&state)),
        "the voice lane drew Duck Source unchecked"
    );
}

/// The row `id` of `items`.
fn row_in<F>(items: &'static [MenuItem<F>], id: &str) -> &'static MenuRow<F> {
    items
        .iter()
        .filter_map(MenuItem::row)
        .find(|row| row.id == id)
        .unwrap_or_else(|| panic!("no menu row {id}"))
}

#[test]
fn every_menu_row_has_its_own_element_id() {
    // gpui keys hover state by element id, so two rows sharing one light up
    // together — and the row builder uses the same id as its fade key.
    let mut ids = all_ids();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "two menu rows share an element id");
}

#[test]
fn only_a_row_that_leaves_the_editor_goes_unpressed() {
    // Fail by default: a new `MenuItem::Outside` row turns this red until
    // somebody records why a test must not press it. Nothing else may opt out of
    // the census.
    const DECLARED: [(&str, &str); 1] = [(
        "ctx-recent-reveal",
        "hands the path to the desktop's file manager, which would open a window \
         on whoever ran the suite",
    )];
    let found = outside_rows();
    let declared: Vec<&str> = DECLARED.iter().map(|(id, _)| *id).collect();
    assert_eq!(
        found, declared,
        "a menu row opts out of the census without a recorded reason"
    );
}

#[test]
fn a_row_is_offered_only_when_its_edit_is_possible() {
    // `when` is what keeps the menu honest: an offer that refuses when clicked is
    // worse than no offer. Measured on the plain fixture, whose selection is one
    // unlinked clip holding the one pool item.
    let (state, clip) = with_clip();
    let clip = ClipMenu::of(&state, clip.raw());
    let offered = |items: &'static [MenuItem<ClipMenu>], facts: &ClipMenu| -> Vec<&'static str> {
        items
            .iter()
            .filter_map(MenuItem::row)
            .filter(|row| (row.when)(facts))
            .map(|row| row.id)
            .collect()
    };
    let rows = offered(CLIP_ITEMS, &clip);
    assert!(
        !rows.contains(&"ctx-link"),
        "a single-clip selection was offered Link Clips"
    );
    assert!(
        !rows.contains(&"ctx-unlink"),
        "an unlinked selection was offered Unlink Clips"
    );
    assert!(
        !rows.contains(&"ctx-group"),
        "a single-clip selection was offered Group"
    );
    assert!(
        !rows.contains(&"ctx-ungroup"),
        "an ungrouped selection was offered Ungroup"
    );
    assert!(
        !rows.contains(&"ctx-release-parent"),
        "a clip with no parent was offered Release Parent"
    );
    assert!(rows.contains(&"ctx-split"), "Split was withheld");

    let pool = PoolMenu::of(&state, first_media(&state));
    assert!(pool.in_use, "the fixture's clip holds the pool item");
    let rows: Vec<&str> = POOL_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .filter(|row| (row.when)(&pool))
        .map(|row| row.id)
        .collect();
    assert!(
        !rows.contains(&"ctx-pool-remove"),
        "an item the timeline holds was offered Remove from Project"
    );
    assert!(
        rows.contains(&"ctx-pool-in-use"),
        "an item the timeline holds did not offer Select Clips in Timeline"
    );
}

#[test]
fn every_blend_the_catalog_offers_can_be_put_on_a_junction() {
    // The catalog is data, so a new blend is a data row and reaches the menu with
    // no code change. What must not happen quietly is a catalog entry the model
    // refuses: the row would draw, click, and leave the junction as it was.
    let blends = crate::app::clip_transitions::transition_choices();
    assert!(!blends.is_empty(), "the catalog offers no blend");
    for spec in blends {
        let (mut state, junction) = fixtures::blended_junction();
        let facts = ClipMenu::of(&state, junction.raw());
        apply_blend(&mut state, &facts, &spec.id).expect("the blend is accepted");
        state.sync_from_engine();
        assert_eq!(
            state.clip_transition_id(junction).as_deref(),
            Some(spec.id.as_str()),
            "the junction did not take {}",
            spec.id
        );
    }
}

#[test]
fn a_blend_the_catalog_does_not_offer_is_refused_without_an_edit() {
    // A stale surface asking for a blend that no longer exists must not raise an
    // error no input caused, and must not spend a history entry.
    let (mut state, junction) = fixtures::blended_junction();
    let facts = ClipMenu::of(&state, junction.raw());
    let before = state.clip_transition_id(junction);
    let depth = state.history_depth();
    apply_blend(&mut state, &facts, "no-such-blend").expect("an unknown id is inert, not an error");
    state.sync_from_engine();
    assert_eq!(
        state.clip_transition_id(junction),
        before,
        "an unknown blend id changed the junction"
    );
    assert_eq!(
        state.history_depth(),
        depth,
        "an unknown blend id spent a history entry"
    );
}

#[test]
fn the_menu_is_built_from_the_tables() {
    // Every test here drives rows. They say nothing about the menu unless the
    // menu is built from the same rows, so the structural fact pinned here is
    // that each table reaches the panel that draws it, the census that presses
    // it, and the reference that lists it. The tables are read from their
    // declarations, so a new table turns this red until all three reach it.
    //
    // Not covered: a drawn row that ignores its table entry and calls the session
    // itself. No census can see that, because a census drives the entry.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    };
    let drawn = read(src.join("panels/context_menu/drawn.rs"));
    let census = read(src.join("app/drawn_census_menu.rs"));
    let reference = read(src.join("keys/reference_doc_tests.rs"));
    let mut sources = vec![read(src.join("surfaces/context_menu.rs"))];
    for entry in
        std::fs::read_dir(src.join("surfaces/context_menu")).expect("the tables are listed")
    {
        sources.push(read(entry.expect("a table file").path()));
    }
    let tables: BTreeSet<&str> = sources
        .iter()
        .flat_map(|source| source.lines())
        .filter_map(|line| line.strip_prefix("pub const ")?.split_once(": &[MenuItem<"))
        .map(|(name, _)| name)
        .collect();
    assert!(
        tables.len() >= 10,
        "the table declarations were not found: {tables:?}"
    );
    for table in tables {
        assert!(
            drawn.contains(&format!("drawn(\n                {table},"))
                || drawn.contains(&format!("drawn({table},"))
                || drawn.contains(&format!("            {table},")),
            "the drawn menu never reaches {table}"
        );
        assert!(
            census.contains(&format!("    press({table}, ")),
            "the census never presses {table}"
        );
        assert!(
            census.contains(&format!("ids.extend(ids_of({table}, false));"))
                && census.contains(&format!("ids.extend(ids_of({table}, true));")),
            "the census row lists leave out {table}"
        );
        assert!(
            reference.contains(&format!("\"{table}\"")),
            "docs/reference/menus.md never lists {table}"
        );
    }
}

/// Every row id in every table, in draw order.
fn all_ids() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    ids.extend(ids_of(CLIP_ITEMS, false));
    ids.extend(ids_of(TRANSITION_ITEMS, false));
    ids.extend(ids_of(TRACK_ITEMS, false));
    ids.extend(ids_of(POOL_ITEMS, false));
    ids.extend(ids_of(PREVIEW_ITEMS, false));
    ids.extend(ids_of(RECENT_ITEMS, false));
    ids.extend(ids_of(EFFECT_ITEMS, false));
    ids.extend(ids_of(BADGE_ITEMS, false));
    ids.extend(ids_of(KEYFRAME_ITEMS, false));
    ids.extend(ids_of(MASK_PATH_KEY_ITEMS, false));
    ids
}

/// The rows that declare themselves out of the census, in draw order.
fn outside_rows() -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    ids.extend(ids_of(CLIP_ITEMS, true));
    ids.extend(ids_of(TRANSITION_ITEMS, true));
    ids.extend(ids_of(TRACK_ITEMS, true));
    ids.extend(ids_of(POOL_ITEMS, true));
    ids.extend(ids_of(PREVIEW_ITEMS, true));
    ids.extend(ids_of(RECENT_ITEMS, true));
    ids.extend(ids_of(EFFECT_ITEMS, true));
    ids.extend(ids_of(BADGE_ITEMS, true));
    ids.extend(ids_of(KEYFRAME_ITEMS, true));
    ids.extend(ids_of(MASK_PATH_KEY_ITEMS, true));
    ids
}

/// One table's row ids: the ones that leave the editor, or all of them.
fn ids_of<F>(
    items: &'static [MenuItem<F>],
    only_outside: bool,
) -> impl Iterator<Item = &'static str> {
    items.iter().filter_map(move |entry| match entry {
        MenuItem::Outside(row) => Some(row.id),
        MenuItem::Row(row) if !only_outside => Some(row.id),
        _ => None,
    })
}
