//! Census of the line each dock panel draws in place of content it does not
//! have.
//!
//! The class closed here: one panel's empty state pinned to its top-left
//! corner while another's stands centred, and a pool whose search hides every
//! item offering to import files. What is asserted, for every registered dock
//! panel in the emptiest project a session holds, a new one with its Main lane
//! removed:
//!
//! - A panel recorded as drawing a line draws it with
//!   [`empty_state`](crate::components::empty_state), inside its body.
//! - The line is centred across its element, with as much room above the line
//!   and the action under it as below them.
//! - The element fills the room the panel's content would: at least
//!   [`FILL_HEIGHT`] of the body's height and [`FILL_WIDTH`] of its width.
//! - A panel the table does not name fails the census until a decision for it
//!   is recorded.
//!
//! Not covered: lines drawn only with a selection or a search
//! (`color-ungraded`, `effects-search`, `generators`, `inspector`), apart from
//! the pool's search, asserted on its own below.

use std::path::PathBuf;

use gpui::{Bounds, Pixels};

use crate::dock::{
    PANEL_ALIGN, PANEL_ASSISTANT, PANEL_AUDIO_METERS, PANEL_CHARACTER, PANEL_COLOR, PANEL_EFFECTS,
    PANEL_HISTORY, PANEL_INFO, PANEL_INSPECTOR, PANEL_JOBS, PANEL_MARKERS, PANEL_MIXER,
    PANEL_PARAGRAPH, PANEL_PROJECT, PANEL_SCOPES, PANEL_TIMELINE, PANEL_VIEWER, PanelId,
};
use crate::interned::intern;
use crate::test_support::panel_bounds;

/// What a dock panel draws in a project with no media, lanes or selection.
enum Empty {
    /// The line drawn as `empty-state-{id}`, and the selector of the action
    /// drawn under it, when it offers one.
    Line(&'static str, Option<&'static str>),
    /// The line drawn as `empty-state-{id}` while the panel draws one, checked
    /// as an [`Empty::Line`] is, and what the panel draws in its place.
    Either(&'static str, &'static str),
    /// Content, named.
    Content(&'static str),
}

/// Every built-in dock panel and what it draws in a project with no media,
/// lanes or selection.
const PANELS: &[(PanelId, Empty)] = &[
    (PANEL_PROJECT, Empty::Line("media", None)),
    (
        PANEL_VIEWER,
        Empty::Either("viewer", "the sequence's canvas, once a frame lands"),
    ),
    (PANEL_TIMELINE, Empty::Line("timeline", None)),
    (
        PANEL_INSPECTOR,
        Empty::Content("the sequence's rate, frame and duration"),
    ),
    (PANEL_MIXER, Empty::Line("mixer", None)),
    (PANEL_ASSISTANT, Empty::Line("assistant", None)),
    (PANEL_SCOPES, Empty::Content("the scope cards")),
    (PANEL_COLOR, Empty::Line("color", None)),
    (PANEL_INFO, Empty::Content("a line under each section")),
    (PANEL_EFFECTS, Empty::Content("the effect folders")),
    (
        PANEL_ALIGN,
        Empty::Content("the align and distribute buttons"),
    ),
    (PANEL_CHARACTER, Empty::Line("character-panel", None)),
    (PANEL_PARAGRAPH, Empty::Line("paragraph-panel", None)),
    (PANEL_AUDIO_METERS, Empty::Content("the meters at silence")),
    (PANEL_JOBS, Empty::Line("jobs-panel", None)),
    (
        PANEL_HISTORY,
        Empty::Content("the undo steps, Project opened first"),
    ),
    (PANEL_MARKERS, Empty::Line("markers", Some("marker-add"))),
];

/// The least share of its panel body's height an empty state fills. One that
/// collapsed to its line is a few rows tall.
const FILL_HEIGHT: f32 = 0.4;

/// The least share of its panel body's width an empty state fills.
const FILL_WIDTH: f32 = 0.6;

/// How far apart two lengths that should agree may be, in pixels: a rounding
/// at each edge.
const SLACK: f32 = 1.5;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn bounds(visual: &mut gpui::VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    panel_bounds(visual, intern(&selector))
}

/// Asserts the geometry of the line drawn as `empty-state-{id}` in `panel`'s
/// body.
fn assert_centred(
    visual: &mut gpui::VisualTestContext,
    panel: PanelId,
    id: &str,
    action: Option<&'static str>,
) {
    let body = bounds(visual, format!("panel-body-{panel}"))
        .unwrap_or_else(|| panic!("{panel} is in front of its frame and draws no body"));
    let element = bounds(visual, format!("empty-state-{id}"))
        .unwrap_or_else(|| panic!("{panel} draws no empty-state-{id}"));
    let line = bounds(visual, format!("empty-state-{id}-line"))
        .unwrap_or_else(|| panic!("empty-state-{id} draws no line"));
    let (body, element, line) = (f32_bounds(body), f32_bounds(element), f32_bounds(line));

    assert!(
        element.0 >= body.0 - SLACK
            && element.1 >= body.1 - SLACK
            && element.2 <= body.2 + SLACK
            && element.3 <= body.3 + SLACK,
        "empty-state-{id} at {element:?} is outside {panel}'s body at {body:?}"
    );
    let centre = |(left, _, right, _): (f32, f32, f32, f32)| (left + right) / 2.0;
    assert!(
        (centre(line) - centre(element)).abs() <= SLACK,
        "{panel}: the line's centre {} is off its element's {}",
        centre(line),
        centre(element)
    );
    let bottom = match action {
        Some(action) => f32::from(
            bounds(visual, action.to_string())
                .unwrap_or_else(|| panic!("{panel} draws no {action} under its line"))
                .bottom(),
        ),
        None => line.3,
    };
    let (above, below) = (line.1 - element.1, element.3 - bottom);
    assert!(
        (above - below).abs() <= SLACK,
        "{panel}: {above} px above the line and {below} px below what it draws"
    );
    let (height, width) = (element.3 - element.1, element.2 - element.0);
    assert!(
        height >= FILL_HEIGHT * (body.3 - body.1) && width >= FILL_WIDTH * (body.2 - body.0),
        "{panel}: empty-state-{id} is {width}x{height} in a body {}x{}",
        body.2 - body.0,
        body.3 - body.1
    );
}

/// `bounds` as left, top, right, bottom.
fn f32_bounds(bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
    (
        f32::from(bounds.left()),
        f32::from(bounds.top()),
        f32::from(bounds.right()),
        f32::from(bounds.bottom()),
    )
}

#[gpui::test]
fn every_dock_panel_centres_the_line_it_draws_when_empty(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "empty-states");
    // The census reads every panel at rest: a panel that fades in is drawn
    // 4 px below its place until the fade ends.
    visual.update(|_, app| app.set_reduce_motion(true));
    let registered = visual.read(|app| state.read(app).registered_panel_ids());
    // A new project holds its Main lane, and the Mixer draws a strip for every
    // lane that can hold sound; with the lane removed it draws its line.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let lanes: Vec<u64> = state
                .store
                .sequence
                .tracks
                .iter()
                .map(|lane| lane.id)
                .collect();
            for lane in lanes {
                state
                    .remove_track(aria_models::TrackId::from_raw(lane))
                    .unwrap_or_else(|err| panic!("remove lane {lane}: {err:?}"));
            }
            cx.notify();
        })
    });
    for &panel in &registered {
        assert!(
            PANELS.iter().any(|&(listed, _)| listed == panel),
            "{panel} is a dock panel with no record of what it draws when empty: add it to PANELS"
        );
    }
    for (panel, empty) in PANELS {
        let panel = *panel;
        assert!(
            registered.contains(&panel),
            "{panel} is in PANELS and not registered"
        );
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.focus_dock_panel(panel);
                cx.notify();
            })
        });
        let content = match *empty {
            Empty::Line(id, action) => {
                assert_centred(visual, panel, id, action);
                continue;
            }
            Empty::Either(id, _) if bounds(visual, format!("empty-state-{id}")).is_some() => {
                assert_centred(visual, panel, id, None);
                continue;
            }
            Empty::Either(_, content) | Empty::Content(content) => content,
        };
        assert!(
            bounds(visual, format!("panel-body-{panel}")).is_some(),
            "{panel} is in front of its frame and draws no body, where it draws {content}"
        );
    }
}

/// A search that hides every item in a pool that holds some says so, and an
/// empty pool offers the import.
#[gpui::test]
fn a_search_that_hides_the_pool_says_nothing_matches(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "empty-pool-search");
    let shown = |visual: &mut gpui::VisualTestContext| {
        (
            bounds(visual, "empty-state-media".into()).is_some(),
            bounds(visual, "empty-state-media-search".into()).is_some(),
        )
    };
    let set_query = |visual: &mut gpui::VisualTestContext, query: &str| {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.media_query = query.to_string();
                cx.notify();
            })
        });
    };
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_dock_panel(PANEL_PROJECT);
            cx.notify();
        })
    });
    assert_eq!(shown(visual), (true, false), "an empty pool");
    set_query(visual, "no item is named this");
    assert_eq!(
        shown(visual),
        (true, false),
        "a search of an empty pool still offers the import"
    );

    let media = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(fixture()).expect("import fixture");
            cx.notify();
            media.raw()
        })
    });
    assert_eq!(
        shown(visual),
        (false, true),
        "a search that hides the pool's one item"
    );
    set_query(visual, "texture");
    assert_eq!(shown(visual), (false, false), "a search that finds it");
    set_query(visual, "");
    assert_eq!(shown(visual), (false, false), "no search");

    // The pool's total also counts the items of folders below the open one,
    // which the unfiltered grid leaves out: a root whose one item is filed
    // away is not a search that hid it.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.create_bin().expect("create a folder");
            let bin = state.current_bin.expect("the new folder is open");
            state
                .move_media_to_bin(media, Some(bin))
                .expect("file the item");
            state.open_bin(None);
            cx.notify();
        })
    });
    assert_eq!(
        shown(visual),
        (true, false),
        "a root whose one item is in a folder, with no search"
    );
}
