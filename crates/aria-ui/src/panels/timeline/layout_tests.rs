//! Layout invariants of the Timeline's header column, track headers, lane
//! sides, footer and graph editor, measured on the drawn shell.
//!
//! The class closed here is a control or a label drawn off its grid: a toggle
//! at another size than its neighbours, a label that runs into a control or
//! past its column, two rows of a column that disagree about an edge, a value
//! axis label at a y the curve does not map its value to, a side of the stack
//! drawn as an empty band, and a playhead field left open with its caret after
//! the pointer went elsewhere. Every sweep runs at the outline widths the
//! settings allow, from the minimum through the width where the timecode steps
//! down to the maximum, so a layout that holds at the default and breaks narrow
//! goes red.
//!
//! Not caught: colours, glyph shapes inside a hit box, and text measured by a
//! real font: the test text system lays every glyph 0.6 em wide, which is wider
//! than the shipped figures, so a width that fits here fits on screen. Which
//! lane toggles show at rest is checked on the registry's rule, not on the
//! drawn opacity, which the test frame does not report.

use gpui::{Bounds, Modifiers, MouseButton, Pixels, TextRunLayout, VisualTestContext, px};

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::app::property_rows::GRAPH_EDITOR_H;
use crate::app::property_rows::graph::graph_axis_ys;
use crate::surfaces::lane_header::{LANE_TOGGLES, LaneFlags, Rest};
use crate::theme::Theme;

use super::EMPTY_SIDE_H;
use super::graph::AXIS_GUTTER;
use super::header::{HEADER_ROW_H, TIMECODE_FULL_MIN, TOOLS_W};

/// Outline widths the sweeps run at: the settings' range, the widths either
/// side of the timecode's step, and the default.
const WIDTHS: [f32; 7] = [
    crate::settings::OUTLINE_MIN,
    TIMECODE_FULL_MIN - 1.0,
    TIMECODE_FULL_MIN,
    crate::settings::OUTLINE_DEFAULT,
    360.0,
    420.0,
    crate::settings::OUTLINE_MAX,
];

/// The header toggles, left to right.
const HEADER_TOGGLES: [&str; 5] = [
    "timeline-snap",
    "timeline-linked",
    "timeline-marker",
    "timeline-display-settings",
    "timeline-graph",
];

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

fn intern(selector: String) -> &'static str {
    crate::interned::intern(&selector)
}

fn bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("the Timeline draws no {selector}"))
}

fn runs(visual: &mut VisualTestContext) -> Vec<TextRunLayout> {
    visual.update(|window, _| window.rendered_text_runs().to_vec())
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= EPS
}

fn change(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    edit: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            edit(state);
            cx.notify();
        })
    });
    visual.run_until_parked();
}

/// A drawn shell with one still on V1, selected, and no audio lanes.
fn shell_with_still<'a>(
    cx: &'a mut gpui::TestAppContext,
    tag: &str,
) -> (gpui::Entity<AppState>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, tag);
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    change(visual, &state, |state| {
        let media = state.import_media(still).expect("import the still");
        state
            .drop_media_on_timeline(media, None, 0)
            .expect("place the still");
        let clip = state.store.sequence.tracks[0].clips[0].id;
        state.select_clip(aria_models::ClipId::from_raw(clip));
    });
    // A second draw: the first records the panel's frame, which the drawn
    // outline width reads.
    change(visual, &state, |_| {});
    (state, visual)
}

/// Runs whose bounds lie inside `area`.
fn runs_in(runs: &[TextRunLayout], area: Bounds<Pixels>) -> Vec<TextRunLayout> {
    runs.iter()
        .filter(|run| area.contains(&run.bounds.center()))
        .cloned()
        .collect()
}

/// No two of `runs` overlap, and none leaves `area`.
fn assert_clear(runs: &[TextRunLayout], area: Bounds<Pixels>, what: &str) {
    for (i, a) in runs.iter().enumerate() {
        assert!(
            crate::test_support::bounds_inside(a.bounds, area),
            "{what}: `{}` at {:?} runs out of {area:?}",
            a.text,
            a.bounds
        );
        for b in &runs[i + 1..] {
            assert!(
                !a.bounds.intersects(&b.bounds),
                "{what}: `{}` and `{}` overlap",
                a.text,
                b.text
            );
        }
    }
}

/// The header column is two rows by two columns at every outline width: the
/// timecode alone in the left column, centred on the two rows with its first
/// digit on the column's text edge; the search field on the toggles' two
/// edges; every toggle a 20 px row control, one even step from the next; the
/// stack of field and toggles as far under the header's top as over its
/// bottom; and no label over another or past the column.
#[gpui::test]
fn the_header_column_is_one_grid_at_every_outline_width(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell_with_still(cx, "timeline-header-grid");
    let clock = visual.read(|app| state.read(app).clock_display.clone());
    for width in WIDTHS {
        change(visual, &state, |state| state.set_outline_width(width));
        let header = bounds(visual, "timeline-header-controls");
        assert!(
            near(header.size.width, px(width)),
            "{width}: the column is drawn {:?} wide",
            header.size.width
        );
        let toggles: Vec<Bounds<Pixels>> =
            HEADER_TOGGLES.iter().map(|id| bounds(visual, id)).collect();
        for (id, toggle) in HEADER_TOGGLES.iter().zip(&toggles) {
            assert!(
                near(toggle.size.width, px(Theme::ICON_HIT_ROW))
                    && near(toggle.size.height, px(Theme::ICON_HIT_ROW)),
                "{width}: {id} is {:?}, not a 20 px row control",
                toggle.size
            );
        }
        for (pair, ids) in toggles.windows(2).zip(HEADER_TOGGLES.windows(2)) {
            assert!(
                near(pair[1].left() - pair[0].right(), px(Theme::SPACE_XS)),
                "{width}: {} and {} stand {:?} apart, not one step",
                ids[0],
                ids[1],
                pair[1].left() - pair[0].right()
            );
        }
        let search = bounds(visual, "property-search");
        assert!(
            near(search.left(), toggles[0].left()) && near(search.right(), toggles[4].right()),
            "{width}: the search field {search:?} and the toggles {:?}..{:?} do not share edges",
            toggles[0].left(),
            toggles[4].right()
        );
        assert!(
            near(toggles[4].right() - toggles[0].left(), px(TOOLS_W)),
            "{width}: the toggles do not span the right column"
        );
        let (over, under) = (
            search.top() - header.top(),
            header.bottom() - toggles[0].bottom(),
        );
        assert!(
            near(over, under),
            "{width}: the field stands {over:?} under the header's top, the toggles \
             {under:?} over its bottom"
        );

        let drawn = runs(visual);
        let inside = runs_in(&drawn, header);
        let find = |text: &str| {
            inside
                .iter()
                .find(|run| run.text.as_ref() == text)
                .unwrap_or_else(|| panic!("{width}: the header draws no `{text}`: {inside:?}"))
                .bounds
        };
        let timecode = find(&clock);
        let left_column: Vec<&str> = inside
            .iter()
            .filter(|run| run.bounds.right() <= search.left())
            .map(|run| run.text.as_ref())
            .collect();
        assert_eq!(
            left_column,
            [clock.as_ref()],
            "{width}: the left column draws more than the timecode"
        );
        assert!(
            near(timecode.left(), header.left() + px(Theme::SPACE_SM)),
            "{width}: the timecode starts at {:?}, not on the column's text edge at {:?}",
            timecode.left(),
            header.left() + px(Theme::SPACE_SM)
        );
        assert!(
            near(toggles[0].center().y - search.center().y, px(HEADER_ROW_H)),
            "{width}: the header rows are {:?} apart, not one row",
            toggles[0].center().y - search.center().y
        );
        let tc_field = bounds(visual, "timeline-tc");
        let middle = (search.center().y + toggles[0].center().y) / 2.0;
        assert!(
            near(tc_field.center().y, middle),
            "{width}: the timecode centers at {:?}, not between the rows at {middle:?}",
            tc_field.center().y
        );
        assert!(
            timecode.right() <= search.left() - px(Theme::SPACE_SM) + px(EPS),
            "{width}: the timecode runs to {:?}, into the search field's gap at {:?}",
            timecode.right(),
            search.left()
        );
        assert_clear(&inside, header, &format!("header at {width}"));
    }
}

/// The timecode reads as figures until it is opened, shows its caret while it
/// holds the keys, and a press anywhere else closes it. The Viewer's field
/// follows the same rule.
#[gpui::test]
fn a_playhead_field_closes_on_a_press_elsewhere(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell_with_still(cx, "timeline-tc-rest");
    let caret = |visual: &mut VisualTestContext| visual.debug_bounds("text-caret").is_some();
    for (field, site, elsewhere) in [
        ("timeline-tc", ClockSite::Timeline, "timeline-video-canvas"),
        ("playhead-tc", ClockSite::Viewer, "timeline-header-controls"),
    ] {
        assert!(!caret(visual), "{field}: a caret is drawn at rest");
        let at = bounds(visual, field).center();
        visual.simulate_click(at, Modifiers::none());
        visual.run_until_parked();
        assert!(
            visual.read(|app| state.read(app).clock_field(site).0),
            "{field}: a click did not open the field"
        );
        assert!(caret(visual), "{field}: the open field draws no caret");

        let away = bounds(visual, elsewhere);
        let away = gpui::point(away.right() - px(4.0), away.bottom() - px(4.0));
        visual.simulate_mouse_down(away, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_up(away, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        assert!(
            !visual.read(|app| state.read(app).clock_focused()),
            "{field}: a press elsewhere left the field open"
        );
        assert!(!caret(visual), "{field}: the caret outlived the field");
    }
}

/// Every control in a track header is one size and on the header's centre
/// line; the name sits between the twirl and the first toggle and never
/// reaches either; the patch leads the row.
#[gpui::test]
fn a_track_header_is_one_row_of_equal_controls(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell_with_still(cx, "timeline-track-header");
    let flags = visual.read(|app| {
        let lanes = &state.read(app).store.sequence.tracks;
        let lane = lanes
            .iter()
            .find(|lane| !lane.clips.is_empty())
            .expect("the still's lane");
        assert_eq!(lanes.len(), 1, "the fixture is one lane, V1");
        LaneFlags::of(lane)
    });
    let lane_id = flags.id;
    for width in WIDTHS {
        change(visual, &state, |state| state.set_outline_width(width));
        let header = bounds(visual, "track-header-V1");
        let mut lead = Vec::new();
        let mut toggles = Vec::new();
        for row in LANE_TOGGLES.iter().filter(|row| (row.shown)(flags)) {
            let control = bounds(visual, intern(format!("lane-{}-{}", row.id, lane_id)));
            assert!(
                near(control.size.width, px(Theme::ICON_HIT_ROW))
                    && near(control.size.height, px(Theme::ICON_HIT_ROW)),
                "{width}: the {} toggle is {:?}",
                row.id,
                control.size
            );
            assert!(
                near(control.center().y, header.center().y),
                "{width}: the {} toggle is off the header's center line",
                row.id
            );
            match row.rest {
                Rest::Lead => lead.push(control),
                _ => toggles.push(control),
            }
        }
        let twirl = bounds(visual, intern(format!("track-prop-twirl-{}", lane_id)));
        assert!(
            near(twirl.size.width, px(Theme::ICON_HIT_ROW))
                && near(twirl.center().y, header.center().y),
            "{width}: the twirl is {twirl:?}, off the controls' size or center line"
        );
        let name = bounds(visual, intern(format!("track-name-{}", lane_id)));
        let first_toggle = toggles
            .iter()
            .map(|b| b.left())
            .fold(header.right(), |a, b| a.min(b));
        assert!(
            name.left() >= twirl.right() - px(EPS) && name.right() <= first_toggle + px(EPS),
            "{width}: the name {name:?} reaches the twirl or the toggles"
        );
        for lead in &lead {
            assert!(
                lead.right() <= twirl.left(),
                "{width}: the patch is not before the name"
            );
        }
        let drawn = runs(visual);
        for run in runs_in(&drawn, name) {
            assert!(
                crate::test_support::bounds_inside(run.bounds, name),
                "{width}: the name `{}` runs out of its box",
                run.text
            );
        }
    }
}

/// At rest a track header draws the source patch, mute and solo on audio
/// lanes, and any other toggle only while its lane departs from a new lane
/// (collapsed, locked, hidden, sync lock off, muted or soloed on a video lane)
/// or is the edit target. A new toggle is a new row here, so the suite asks
/// when it draws.
#[test]
fn a_track_header_at_rest_draws_only_what_its_lane_calls_for() {
    let fresh = LaneFlags {
        id: 1,
        muted: false,
        solo: false,
        locked: false,
        enabled: true,
        collapsed: false,
        targeted: false,
        sync_lock: true,
        channel: Some(aria_models::SourceChannel::Video),
        patched: false,
    };
    let at_rest = |lane: LaneFlags, audio: bool| -> Vec<&str> {
        LANE_TOGGLES
            .iter()
            .filter(|row| (row.shown)(lane) && row.shows_at_rest(lane, audio))
            .map(|row| row.id)
            .collect()
    };
    assert_eq!(at_rest(fresh, false), ["patch"]);
    assert_eq!(at_rest(fresh, true), ["patch", "mute", "solo"]);
    for row in LANE_TOGGLES {
        let departed = match row.id {
            "patch" => continue,
            "mute" => LaneFlags {
                muted: true,
                ..fresh
            },
            "solo" => LaneFlags {
                solo: true,
                ..fresh
            },
            "collapse" => LaneFlags {
                collapsed: true,
                ..fresh
            },
            "lock" => LaneFlags {
                locked: true,
                ..fresh
            },
            "vis" => LaneFlags {
                enabled: false,
                ..fresh
            },
            "target" => LaneFlags {
                targeted: true,
                ..fresh
            },
            "sync" => LaneFlags {
                sync_lock: false,
                ..fresh
            },
            other => panic!("decide when the `{other}` toggle draws at rest"),
        };
        assert_eq!(
            at_rest(departed, false),
            ["patch", row.id],
            "a video lane that departs on `{}` draws the wrong toggles at rest",
            row.id
        );
    }
    let lead: Vec<&str> = LANE_TOGGLES
        .iter()
        .filter(|row| row.rest == Rest::Lead)
        .map(|row| row.id)
        .collect();
    assert_eq!(lead, ["patch"]);
}

/// A sequence with no audio lanes draws the audio side as one labelled strip,
/// and the video side takes the rest of the stack.
#[gpui::test]
fn a_side_without_lanes_is_one_labelled_strip(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell_with_still(cx, "timeline-empty-side");
    let audio_lanes = visual.read(|app| {
        let state = state.read(app);
        state.store.sequence.tracks.len() - state.visual_lane_count()
    });
    assert_eq!(audio_lanes, 0, "the fixture has no audio lanes");
    let audio = bounds(visual, "timeline-audio-canvas");
    let video = bounds(visual, "timeline-video-canvas");
    assert!(
        near(audio.size.height, px(EMPTY_SIDE_H)),
        "the empty audio side is {:?} tall",
        audio.size.height
    );
    assert!(
        video.size.height > audio.size.height * 4.0,
        "the video side did not take the stack: {video:?}"
    );
    let label = runs(visual)
        .into_iter()
        .find(|run| run.text.as_ref() == "No audio tracks")
        .expect("the empty side states the missing side");
    assert!(
        near(label.bounds.center().y, audio.center().y)
            || (f32::from(label.bounds.center().y) - f32::from(audio.center().y)).abs() <= 1.0,
        "the label is off the strip"
    );
}

/// The graph editor's value axis labels stand on the header column's right
/// edge at the ys the curve maps the top, middle and bottom values to, the
/// graph's grid lines are at the same ys, and the tools share one row clear of
/// the axis.
#[gpui::test]
fn the_graph_axis_stands_on_the_curve_ys(cx: &mut gpui::TestAppContext) {
    let (state, visual) = shell_with_still(cx, "timeline-graph-axis");
    change(visual, &state, |state| state.toggle_timeline_graph_editor());
    let ys = graph_axis_ys(GRAPH_EDITOR_H);
    for width in WIDTHS {
        change(visual, &state, |state| state.set_outline_width(width));
        let header = bounds(visual, "timeline-graph-header");
        let canvas = bounds(visual, "timeline-graph-canvas");
        let mut labels = Vec::new();
        for (selector, y) in [
            ("graph-axis-top", ys[0]),
            ("graph-axis-mid", ys[1]),
            ("graph-axis-bottom", ys[2]),
        ] {
            let label = bounds(visual, selector);
            assert!(
                near(label.center().y - canvas.top(), px(y)),
                "{width}: {selector} centers at {:?}, the curve maps it to {y}",
                label.center().y - canvas.top()
            );
            assert!(
                near(label.right(), header.right() - px(1.0 + Theme::SPACE_SM)),
                "{width}: {selector} ends at {:?}, off the column's edge",
                label.right()
            );
            labels.push(label);
        }
        let tools: Vec<Bounds<Pixels>> = [
            "graph-mode-value",
            "graph-mode-speed",
            "graph-btn-easy-ease",
            "graph-btn-hold",
        ]
        .into_iter()
        .map(|id| bounds(visual, id))
        .collect();
        for tool in &tools {
            assert!(
                near(tool.center().y, tools[0].center().y),
                "{width}: the graph tools are on two rows"
            );
            assert!(
                tool.left() >= header.left() + px(Theme::SPACE_SM) - px(EPS)
                    && tool.right() <= header.right() - px(1.0 + Theme::SPACE_SM) + px(EPS),
                "{width}: a graph tool at {tool:?} leaves the column's padding"
            );
            for label in &labels {
                assert!(
                    !tool.intersects(label),
                    "{width}: a tool sits on an axis label"
                );
            }
        }
        let grid: Vec<f32> = visual.update(|window, _| {
            let scale = window.scale_factor();
            window
                .painted_quads()
                .iter()
                .filter(|quad| {
                    let b = quad.bounds;
                    (b.size.height.0 / scale - 1.0).abs() < 0.01
                        && (b.size.width.0 / scale - f32::from(canvas.size.width)).abs() < 1.0
                        && b.origin.x.0 / scale >= f32::from(canvas.left()) - 0.5
                })
                .map(|quad| quad.bounds.origin.y.0 / scale - f32::from(canvas.top()))
                .collect()
        });
        for y in ys {
            assert!(
                grid.iter().any(|g| (g - y).abs() <= EPS),
                "{width}: no grid line at {y}: {grid:?}"
            );
        }
        let drawn = runs_in(&runs(visual), header);
        let gutter = header.right() - px(1.0 + Theme::SPACE_SM + AXIS_GUTTER);
        for run in &drawn {
            let in_name_row = run.bounds.center().y < header.top() + px(HEADER_ROW_H);
            let is_label = labels
                .iter()
                .any(|label| label.contains(&run.bounds.center()));
            if in_name_row && !is_label {
                assert!(
                    run.bounds.right() <= gutter + px(EPS),
                    "{width}: `{}` runs into the axis gutter",
                    run.text
                );
            }
        }
        assert_clear(&drawn, header, &format!("graph header at {width}"));
    }
}

/// A narrow Timeline keeps its time area: the drawn outline column narrows to
/// leave the 160 px of time area timeline.md states, never under the settings
/// minimum, and the setting itself is untouched. The 160 is written here
/// rather than read from `OUTLINE_TIME_MIN`, so a change to the constant is a
/// change to this test.
#[test]
fn a_narrow_timeline_keeps_its_time_area() {
    use crate::app::timeline_view::drawn_outline;
    use crate::settings::{OUTLINE_MAX, OUTLINE_MIN};
    const TIME_AREA: f32 = 160.0;
    for setting in [OUTLINE_MIN, 300.0, OUTLINE_MAX] {
        assert_eq!(drawn_outline(setting, None), setting);
        for panel in [400.0, 460.0, 640.0, 1600.0] {
            let drawn = drawn_outline(setting, Some(panel));
            assert!(drawn <= setting, "{setting} in {panel}: drawn wider");
            assert!(
                drawn >= OUTLINE_MIN,
                "{setting} in {panel}: under the minimum"
            );
            assert!(
                panel - drawn >= TIME_AREA || drawn == OUTLINE_MIN,
                "{setting} in {panel}: {} px of time area",
                panel - drawn
            );
        }
    }
}
