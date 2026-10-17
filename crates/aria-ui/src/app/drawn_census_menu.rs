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

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::surfaces::context_menu::{
    BADGE_ITEMS, BadgeMenu, CLIP_ITEMS, ClipMenu, EFFECT_ITEMS, EffectMenu, MenuItem, POOL_ITEMS,
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
    };
    press(CLIP_ITEMS, CLIP_STAGES, &mut out);
    press(TRANSITION_ITEMS, JUNCTION_STAGES, &mut out);
    press(TRACK_ITEMS, LANE_STAGES, &mut out);
    press(POOL_ITEMS, POOL_STAGES, &mut out);
    press(PREVIEW_ITEMS, PREVIEW_STAGES, &mut out);
    press(RECENT_ITEMS, RECENT_STAGES, &mut out);
    press(EFFECT_ITEMS, EFFECT_STAGES, &mut out);
    press(BADGE_ITEMS, BADGE_STAGES, &mut out);
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

/// Press one table across its states.
fn press<F: Clone>(items: &'static [MenuItem<F>], stages: &[Stage<F>], out: &mut Pressed) {
    for entry in items {
        let MenuItem::Row(row) = entry else {
            continue;
        };
        let mut offered = false;
        let mut acted = false;
        for stage in stages {
            let (mut state, facts) = (stage.dress)();
            if !(row.when)(&facts) {
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
            let _ = stage.what;
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

/// What an operator can see of the session, in one string.
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
        "an item the timeline holds was offered Remove from project"
    );
    assert!(
        rows.contains(&"ctx-pool-in-use"),
        "an item the timeline holds did not say so"
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
    // error the operator did not cause, and must not spend a history entry.
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
    // menu is built from the same rows, so the one structural fact worth pinning
    // is that each table reaches the panel that draws it and the census that
    // presses it. The tables are read from their declarations, so a new table
    // turns this red until both reach it.
    //
    // Not covered: a drawn row that ignores its table entry and calls the session
    // itself. No census can see that, because a census drives the entry.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let read = |path: std::path::PathBuf| {
        std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    };
    let drawn = read(src.join("panels/context_menu/drawn.rs"));
    let census = read(src.join("app/drawn_census_menu.rs"));
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
        tables.len() >= 8,
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
