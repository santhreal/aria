//! Spacing and symmetry invariants of the drawn editor, measured on every
//! painted quad, text run and named box of the shell, in every workspace the
//! switcher offers, at three window sizes.
//!
//! The classes closed here:
//!
//! - A docked frame off the gutter grid: two frames closer or farther apart
//!   than [`DOCK_GUTTER`], or a frame at another distance from the bars and the
//!   window's edges than from its neighbours.
//! - A doubled rule: two parallel grey lines of different elements that touch
//!   or stand one pixel apart, which reads as one thick line or as a line with
//!   a sliver beside it. A panel border against its frame's border, a divider
//!   against a ruler's edge, a ruler tick against the outline column's divider
//!   and the graph editor's rule against the ruler's are all members.
//! - A tab label off its tab's centre line: an active tab whose underline
//!   takes layout and lifts its label above its neighbours'.
//! - A property value off its row's line: a readout whose underline takes
//!   layout and lifts the number above the keyframe navigator beside it.
//! - A centred control off its bar's centre line while both side groups fit
//!   their halves: the workspace switcher, the breadcrumb and the transport.
//! - A round control cut by the box that clips it: a handle on a fitted
//!   picture's edge drawn as half a dot. A control cut at an edge that a
//!   scroll frame fades, because its content continues past it, is scrolled
//!   out of view rather than cut.
//!
//! Exempt: what the canvases that draw the edit paint, the lanes and the graph
//! editor's curves. A clip, a keyframe and a waveform stand where the edit puts
//! them and may meet any line, for example a clip at time zero against the
//! outline column's divider.
//!
//! Not caught: a colour that is neither grey nor clear, spacing inside one
//! canvas's own paint, text measured by a real font (the test text system lays
//! every glyph 0.6 em wide), and window sizes other than the three below.

use gpui::{Bounds, Hsla, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::interned::intern;
use crate::test_support::{drawn_shell_at, wait_out_splash};
use crate::theme::Theme;

use super::DOCK_GUTTER;

const SIZES: [(f32, f32); 3] = [(1600.0, 976.0), (1280.0, 800.0), (1920.0, 1080.0)];

/// Layout rounds every edge to the device pixel grid, and the test window
/// draws at scale 2, so an edge lands within half a pixel of its float place.
const EPS: f32 = 0.51;

/// The canvases whose paint is the edit itself.
const CONTENT: [&str; 3] = [
    "timeline-video-canvas",
    "timeline-audio-canvas",
    "timeline-graph-canvas",
];

/// Left, top, right, bottom.
type Edges = [f32; 4];

fn edges(at: Bounds<Pixels>) -> Edges {
    [
        f32::from(at.left()),
        f32::from(at.top()),
        f32::from(at.right()),
        f32::from(at.bottom()),
    ]
}

fn intersect(a: Edges, b: Edges) -> Edges {
    [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ]
}

fn is_empty(at: Edges) -> bool {
    at[2] - at[0] <= 0.01 || at[3] - at[1] <= 0.01
}

fn inside(inner: Edges, outer: Edges, slack: f32) -> bool {
    inner[0] >= outer[0] - slack
        && inner[1] >= outer[1] - slack
        && inner[2] <= outer[2] + slack
        && inner[3] <= outer[3] + slack
}

/// Walks every workspace at every size, with the graph editor in each state
/// of `graph`, and hands `check` the drawn shell of each case.
fn walk(
    cx: &mut TestAppContext,
    graph: &[bool],
    mut check: impl FnMut(&str, &gpui::Entity<AppState>, &mut VisualTestContext),
) {
    cx.executor().allow_parking();
    let fixtures =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    for (width, height) in SIZES {
        let at = Bounds::new(point(px(0.), px(0.)), size(px(width), px(height)));
        let (state, mut visual) = drawn_shell_at(cx, &format!("spacing-{width}x{height}"), at);
        let visual = &mut visual;
        visual.update(|_, app| app.set_reduce_motion(true));
        let clip = fixtures.join("fixture-640x360-24fps-3s-av.mp4");
        let still = fixtures.join("texture.png");
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                let media = state.import_media(clip).expect("import the clip");
                state
                    .drop_media_on_timeline(media, None, 0)
                    .expect("place the clip");
                state.import_media(still).expect("import the still");
                let first = state
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .map(|clip| aria_models::ClipId::from_raw(clip.id))
                    .next()
                    .expect("a clip");
                state.select_clip(first);
                cx.notify();
            })
        });
        wait_out_splash(visual);
        let rows: Vec<&'static str> = visual.read(|app| {
            state
                .read(app)
                .workspace_rows()
                .into_iter()
                .map(|row| row.id)
                .collect()
        });
        assert!(!rows.is_empty(), "the switcher offers no workspace");
        for id in rows {
            for &open in graph {
                visual.update(|_, app| {
                    state.update(app, |state, cx| {
                        assert!(state.apply_workspace(id, cx), "{id} applies");
                        if state.timeline_graph_open() != open {
                            state.toggle_timeline_graph_editor();
                        }
                        cx.notify();
                    })
                });
                visual.run_until_parked();
                // A frame that is not drawn in this workspace keeps no rect.
                visual.read(|app| state.read(app).dock_frame_rects.lock().clear());
                // The fades the check reads are this draw's alone.
                crate::edge_fade::take_painted();
                visual.update(|window, _| window.refresh());
                let case = format!(
                    "{id} at {width}x{height}, graph editor {}",
                    if open { "open" } else { "closed" }
                );
                check(&case, &state, visual);
            }
        }
    }
}

/// A painted quad in logical pixels.
struct Painted {
    at: Edges,
    /// The box that clips it.
    mask: Edges,
    fill: Option<Hsla>,
    border: Hsla,
    /// Top, right, bottom, left.
    widths: [f32; 4],
    /// Top left, top right, bottom right, bottom left.
    radii: [f32; 4],
}

fn painted(visual: &mut VisualTestContext) -> Vec<Painted> {
    visual.update(|window, _| {
        let scale = window.scale_factor();
        let edges = |at: Bounds<gpui::ScaledPixels>| {
            let (left, top) = (at.origin.x.0 / scale, at.origin.y.0 / scale);
            [
                left,
                top,
                left + at.size.width.0 / scale,
                top + at.size.height.0 / scale,
            ]
        };
        window
            .painted_quads()
            .iter()
            .map(|quad| Painted {
                at: edges(quad.bounds),
                mask: edges(quad.content_mask.bounds),
                fill: quad
                    .background
                    .as_solid()
                    .filter(|_| !quad.background.is_transparent()),
                border: quad.border_color,
                widths: [
                    quad.border_widths.top.0 / scale,
                    quad.border_widths.right.0 / scale,
                    quad.border_widths.bottom.0 / scale,
                    quad.border_widths.left.0 / scale,
                ],
                radii: [
                    quad.corner_radii.top_left.0 / scale,
                    quad.corner_radii.top_right.0 / scale,
                    quad.corner_radii.bottom_right.0 / scale,
                    quad.corner_radii.bottom_left.0 / scale,
                ],
            })
            .collect()
    })
}

/// The boxes of the canvases that draw the edit, where this case draws them.
fn content(visual: &mut VisualTestContext) -> Vec<Edges> {
    CONTENT
        .iter()
        .filter_map(|selector| visual.debug_bounds(selector))
        .map(edges)
        .collect()
}

fn bounds(visual: &mut VisualTestContext, case: &str, selector: &'static str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{case}: the shell draws no {selector}"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Axis {
    Horizontal,
    Vertical,
}

/// One straight grey line: the band it covers across its axis and its extent
/// along it, both inside the box that clips it.
struct Rule {
    axis: Axis,
    across: [f32; 2],
    along: [f32; 2],
    quad: usize,
}

/// A colour that reads as a rule: grey, and not faded to nothing. An accent
/// line (a tab's underline, the playhead, a focus ring) is a mark, not a rule.
fn rule_colour(color: Hsla) -> bool {
    color.s < 0.2 && color.a > 0.03
}

/// Every rule painted outside the content canvases: each side of a grey
/// border, without the corners it rounds, and each grey fill at most 3 px
/// thick.
fn rules(quads: &[Painted], exempt: &[Edges]) -> Vec<Rule> {
    let mut out = Vec::new();
    for (index, quad) in quads.iter().enumerate() {
        let shown = intersect(quad.at, quad.mask);
        if is_empty(shown) || exempt.iter().any(|region| inside(shown, *region, 0.5)) {
            continue;
        }
        let [l, t, r, b] = quad.at;
        let mask = quad.mask;
        let mut push = |axis: Axis, across: [f32; 2], along: [f32; 2]| {
            let (clip_across, clip_along) = match axis {
                Axis::Horizontal => ([mask[1], mask[3]], [mask[0], mask[2]]),
                Axis::Vertical => ([mask[0], mask[2]], [mask[1], mask[3]]),
            };
            let across = [across[0].max(clip_across[0]), across[1].min(clip_across[1])];
            let along = [along[0].max(clip_along[0]), along[1].min(clip_along[1])];
            if across[1] - across[0] > 0.01 && along[1] - along[0] > 0.01 {
                out.push(Rule {
                    axis,
                    across,
                    along,
                    quad: index,
                });
            }
        };
        let [top, right, bottom, left] = quad.widths;
        let [top_left, top_right, bottom_right, bottom_left] = quad.radii;
        if rule_colour(quad.border) {
            if top > 0.0 {
                push(
                    Axis::Horizontal,
                    [t, t + top],
                    [l + top_left, r - top_right],
                );
            }
            if bottom > 0.0 {
                push(
                    Axis::Horizontal,
                    [b - bottom, b],
                    [l + bottom_left, r - bottom_right],
                );
            }
            if left > 0.0 {
                push(
                    Axis::Vertical,
                    [l, l + left],
                    [t + top_left, b - bottom_left],
                );
            }
            if right > 0.0 {
                push(
                    Axis::Vertical,
                    [r - right, r],
                    [t + top_right, b - bottom_right],
                );
            }
        }
        let square = quad.radii.iter().all(|radius| *radius <= 0.5);
        if quad.fill.is_some_and(rule_colour) && square {
            let (w, h) = (r - l, b - t);
            if h <= 3.0 && w >= 8.0 {
                push(Axis::Horizontal, [t, b], [l, r]);
            } else if w <= 3.0 && h >= 8.0 {
                push(Axis::Vertical, [l, r], [t, b]);
            }
        }
    }
    out
}

/// Every pair of rules of different elements that run side by side: parallel,
/// touching or one pixel apart, and alongside each other for 8 px or more.
fn doubled(quads: &[Painted], exempt: &[Edges]) -> Vec<String> {
    let mut rules = rules(quads, exempt);
    rules.sort_by(|a, b| {
        a.axis
            .cmp(&b.axis)
            .then(a.across[0].total_cmp(&b.across[0]))
    });
    let mut out = Vec::new();
    for (i, a) in rules.iter().enumerate() {
        for b in &rules[i + 1..] {
            if b.axis != a.axis || b.across[0] > a.across[1] + 1.01 {
                break;
            }
            let (qa, qb) = (&quads[a.quad], &quads[b.quad]);
            // gpui paints an element's fill and its border as two quads.
            if a.quad == b.quad || qa.at == qb.at {
                continue;
            }
            // Rules that overlap paint the same pixels once.
            let gap = (b.across[0] - a.across[1]).max(a.across[0] - b.across[1]);
            if !(-0.01..=1.01).contains(&gap) {
                continue;
            }
            let alongside = a.along[1].min(b.along[1]) - a.along[0].max(b.along[0]);
            if alongside >= 8.0 {
                out.push(format!(
                    "{:?} rules at {:?} and {:?} run side by side for {alongside} px: \
                     the quads at {:?} and {:?}",
                    a.axis, a.across, b.across, qa.at, qb.at
                ));
            }
        }
    }
    out
}

/// Two rules of different elements never run side by side, with the graph
/// editor open or closed: a frame too short for the track stack puts the
/// graph editor straight under the ruler.
#[gpui::test]
fn no_two_rules_run_side_by_side(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    walk(cx, &[false, true], |case, _, visual| {
        let exempt = content(visual);
        let quads = painted(visual);
        failures.extend(
            doubled(&quads, &exempt)
                .into_iter()
                .map(|failure| format!("{case}: {failure}")),
        );
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every docked frame stands [`DOCK_GUTTER`] from each neighbour and from the
/// edges of the dock area, which are the bars and the window's sides, and no
/// two frames overlap.
#[gpui::test]
fn docked_frames_stand_one_gutter_apart_and_in(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    let mut frames_seen = 0;
    walk(cx, &[false], |case, state, visual| {
        let area = edges(bounds(visual, case, "dock-area"));
        let frames: Vec<Edges> = visual.read(|app| {
            let state = state.read(app);
            let rects = state.dock_frame_rects.lock();
            state
                .dock_tree
                .root_frames()
                .iter()
                .filter_map(|frame| rects.get(&frame.id))
                .map(|[x, y, w, h]| [*x, *y, x + w, y + h])
                .collect()
        });
        assert!(!frames.is_empty(), "{case}: no docked frame is drawn");
        frames_seen += frames.len();
        let inner = [
            area[0] + DOCK_GUTTER,
            area[1] + DOCK_GUTTER,
            area[2] - DOCK_GUTTER,
            area[3] - DOCK_GUTTER,
        ];
        let near = |a: f32, b: f32| (a - b).abs() <= EPS;
        for (i, frame) in frames.iter().enumerate() {
            let others: Vec<&Edges> = frames
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, other)| other)
                .collect();
            let rows_meet = |other: &Edges| frame[3].min(other[3]) - frame[1].max(other[1]) > EPS;
            let columns_meet =
                |other: &Edges| frame[2].min(other[2]) - frame[0].max(other[0]) > EPS;
            let sides = [
                (
                    "left",
                    near(frame[0], inner[0])
                        || others
                            .iter()
                            .any(|o| rows_meet(o) && near(o[2] + DOCK_GUTTER, frame[0])),
                ),
                (
                    "top",
                    near(frame[1], inner[1])
                        || others
                            .iter()
                            .any(|o| columns_meet(o) && near(o[3] + DOCK_GUTTER, frame[1])),
                ),
                (
                    "right",
                    near(frame[2], inner[2])
                        || others
                            .iter()
                            .any(|o| rows_meet(o) && near(frame[2] + DOCK_GUTTER, o[0])),
                ),
                (
                    "bottom",
                    near(frame[3], inner[3])
                        || others
                            .iter()
                            .any(|o| columns_meet(o) && near(frame[3] + DOCK_GUTTER, o[1])),
                ),
            ];
            for (side, on_grid) in sides {
                if !on_grid {
                    failures.push(format!(
                        "{case}: the {side} edge of the frame at {frame:?} stands neither \
                         {DOCK_GUTTER} px in from the dock area {area:?} nor {DOCK_GUTTER} px \
                         from a neighbor"
                    ));
                }
            }
            if !inside(*frame, inner, EPS) {
                failures.push(format!(
                    "{case}: the frame at {frame:?} reaches into the dock area's margin \
                     {inner:?}"
                ));
            }
            for other in &frames[i + 1..] {
                let both = intersect(*frame, *other);
                if both[2] - both[0] > EPS && both[3] - both[1] > EPS {
                    failures.push(format!(
                        "{case}: the frames at {frame:?} and {other:?} overlap"
                    ));
                }
            }
        }
    });
    assert!(frames_seen > 0, "the walk drew no frame");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The run that draws `title` inside `tab`: the whole title, or the part of
/// it a narrow tab keeps. Matched by text, since a panel scrolled under a tab
/// row paints runs there that its clip hides.
fn label_in<'a>(
    runs: &'a [gpui::TextRunLayout],
    tab: Bounds<Pixels>,
    title: &str,
) -> Option<&'a gpui::TextRunLayout> {
    runs.iter().find(|run| {
        let kept = run.text.trim_end_matches('…');
        !kept.trim().is_empty() && title.starts_with(kept) && tab.contains(&run.bounds.center())
    })
}

/// Every tab's label stands on its tab's centre line, the tab in front and the
/// others alike: dock tabs and Viewer tabs. The Viewer draws its tabs only
/// while a Source tab is open, so each case opens one with Program in front.
/// The Inspector's tabs are walked at every strip width by
/// `panels::inspector::strip_tests`, which also centres each label across.
#[gpui::test]
fn every_tab_label_stands_on_its_tab_centre_line(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    let mut checked = [0usize; 2];
    walk(cx, &[false], |case, state, visual| {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                if state.sources.tabs().is_empty() {
                    let still = state.store.media.last().expect("the still").id;
                    assert!(state.open_source(still), "the Source tab opens");
                    state.show_program();
                    cx.notify();
                }
            })
        });
        visual.run_until_parked();
        visual.update(|window, _| window.refresh());
        let runs: Vec<gpui::TextRunLayout> =
            visual.update(|window, _| window.rendered_text_runs().to_vec());
        let mut tabs: Vec<(usize, &'static str, String)> = visual.read(|app| {
            let read = state.read(app);
            read.dock_tree
                .root_frames()
                .iter()
                .flat_map(|frame| frame.tabs.iter())
                .map(|panel| {
                    (
                        0,
                        intern(&format!("dock-tab-{panel}")),
                        read.dock_panel_title(panel).to_string(),
                    )
                })
                .collect()
        });
        tabs.push((1, "viewer-tab-program", "Program".to_string()));
        let sources: Vec<(usize, &'static str, String)> = visual.read(|app| {
            state
                .read(app)
                .sources
                .tabs()
                .iter()
                .enumerate()
                .map(|(index, view)| {
                    let kind = if view.clip.is_some() {
                        "Clip"
                    } else {
                        "Source"
                    };
                    (
                        1,
                        intern(&format!("viewer-tab-source-{index}")),
                        format!("{kind}: {}", view.name),
                    )
                })
                .collect()
        });
        tabs.extend(sources);
        for (kind, selector, title) in tabs {
            let Some(tab) = visual.debug_bounds(selector) else {
                continue;
            };
            // A tab too narrow for its title draws none.
            let Some(label) = label_in(&runs, tab, &title) else {
                continue;
            };
            checked[kind] += 1;
            let down = f32::from(label.bounds.center().y - tab.center().y);
            if down.abs() > EPS {
                failures.push(format!(
                    "{case}: `{}` in {selector} stands {down} px off the tab's center line \
                     ({:?} in {tab:?})",
                    label.text, label.bounds
                ));
            }
        }
    });
    assert!(
        checked.iter().all(|count| *count > 0),
        "the walk checked {checked:?} dock and Viewer tab labels"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The keyframe navigator's previous-key glyph, drawn right of a property's
/// value in the Effect Controls rows and the Timeline's property rows.
const PREVIOUS_KEY: &str = "\u{2039}";

/// Every property value stands on the line of the keyframe navigator beside
/// it. Each child of the row is centred on the row and each box's top lands on
/// the device pixel grid, so the two centres stand within half a device pixel
/// of each other; a value whose box is taller than its text on one side stands
/// a whole device pixel off.
#[gpui::test]
fn every_value_stands_on_the_line_of_its_keyframe_navigator(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    let mut checked = 0usize;
    walk(cx, &[false], |case, _, visual| {
        let (runs, pixel) = visual.update(|window, _| {
            (
                window.rendered_text_runs().to_vec(),
                1.0 / window.scale_factor(),
            )
        });
        let gap = |value: &gpui::TextRunLayout, glyph: &gpui::TextRunLayout| {
            f32::from(glyph.bounds.left() - value.bounds.right())
        };
        for glyph in runs.iter().filter(|run| run.text.as_ref() == PREVIOUS_KEY) {
            // The value is the run that ends nearest left of the glyph on its
            // line, at most one inset away.
            let value = runs
                .iter()
                .filter(|run| {
                    !run.text.trim().is_empty()
                        && (0.0..=Theme::SPACE_MD).contains(&gap(run, glyph))
                        && run.bounds.top() < glyph.bounds.bottom()
                        && glyph.bounds.top() < run.bounds.bottom()
                })
                .min_by(|a, b| gap(a, glyph).total_cmp(&gap(b, glyph)));
            let Some(value) = value else {
                continue;
            };
            checked += 1;
            let down = f32::from(value.bounds.center().y - glyph.bounds.center().y);
            if down.abs() > pixel / 2.0 + 0.01 {
                failures.push(format!(
                    "{case}: the value `{}` stands {down} px off the line of the keyframe \
                     navigator beside it ({:?} against {:?})",
                    value.text, value.bounds, glyph.bounds
                ));
            }
        }
    });
    assert!(
        checked > 0,
        "the walk drew no property value beside a keyframe navigator"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// How far `item` stands right of `bar`'s centre, when both side groups'
/// contents fit the halves the item leaves them. `left_end` is where the left
/// group's content ends and `right_start` where the right group's begins;
/// `pad` is the bar's side padding and `gap` the gap on each side of the item.
fn off_centre(
    bar: Bounds<Pixels>,
    pad: f32,
    gap: f32,
    left_end: Pixels,
    item: Bounds<Pixels>,
    right_start: Pixels,
) -> Option<f32> {
    let half =
        (f32::from(bar.size.width) - 2.0 * pad - f32::from(item.size.width) - 2.0 * gap) / 2.0;
    let left = f32::from(left_end - bar.left()) - pad;
    let right = f32::from(bar.right() - right_start) - pad;
    (left <= half - 1.0 && right <= half - 1.0).then(|| f32::from(item.center().x - bar.center().x))
}

/// The workspace switcher, the breadcrumb and the transport each stand on
/// their bar's centre line whenever the groups beside them fit their halves.
/// A bar too narrow for that is not checked; the walk has to check each bar at
/// least once.
#[gpui::test]
fn centred_controls_stand_on_their_bar_centre_line(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    let mut checked = [0usize; 3];
    walk(cx, &[false], |case, _, visual| {
        let mut verdict = |index: usize, what: &str, off: Option<f32>| {
            if let Some(off) = off {
                checked[index] += 1;
                if off.abs() > EPS {
                    failures.push(format!(
                        "{case}: the {what} stands {off} px off its bar's center line"
                    ));
                }
            }
        };
        let bar = bounds(visual, case, "tool-bar");
        let left = bounds(visual, case, "edit-mode").right();
        let item = bounds(visual, case, "workspace-switcher");
        let right = bounds(visual, case, "tool-bar-export").left();
        verdict(
            0,
            "workspace switcher",
            off_centre(bar, Theme::SPACE_MD, Theme::SPACE_XS, left, item, right),
        );

        let bar = bounds(visual, case, "menu-bar");
        let left = bounds(visual, case, "bar-drag-left").left();
        let item = bounds(visual, case, "breadcrumb");
        let right = bounds(visual, case, "bar-drag-right").right();
        verdict(
            1,
            "breadcrumb",
            off_centre(bar, Theme::SPACE_MD, Theme::SPACE_XS, left, item, right),
        );

        // The Viewer's row wraps between groups when it is too narrow; the
        // transport centres only on a row of one line.
        if let Some(bar) = visual.debug_bounds("viewer-row") {
            let picture = bounds(visual, case, "viewer-row-picture");
            let item = bounds(visual, case, "viewer-row-transport");
            let display = bounds(visual, case, "viewer-row-display");
            let one_line = [picture, display]
                .iter()
                .all(|group| f32::from((group.center().y - item.center().y).abs()) <= EPS);
            if one_line {
                let left = bounds(visual, case, "preview-proxies-toggle").right();
                let right = bounds(visual, case, "preview-handles-transform").left();
                verdict(
                    2,
                    "transport",
                    off_centre(bar, Theme::SPACE_MD, Theme::SPACE_SM, left, item, right),
                );
            }
        }
    });
    assert!(
        checked.iter().all(|count| *count > 0),
        "the walk checked the switcher, the breadcrumb and the transport {checked:?} times"
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Whether each edge of `mask` that `at` runs past is an edge a scroll frame
/// fades, which it does only while its content continues past that edge.
fn scrolled_past(at: Edges, mask: Edges, fades: &[gpui::EdgeFade]) -> bool {
    let faded = |side: usize| {
        fades.iter().any(|fade| {
            let b = fade.bands;
            let band = [b.left, b.top, b.right, b.bottom][side];
            band > px(0.0) && (edges(fade.bounds)[side] - mask[side]).abs() <= EPS
        })
    };
    let past = [
        at[0] < mask[0] - 0.26,
        at[1] < mask[1] - 0.26,
        at[2] > mask[2] + 0.26,
        at[3] > mask[3] + 0.26,
    ];
    past.iter()
        .enumerate()
        .all(|(side, &runs_past)| !runs_past || faded(side))
}

/// No round control is cut by the box that clips it. A dot, a handle or a
/// round button drawn as a part reads as a layout fault, not as a shape. A
/// control at the edge of a scroll frame whose content continues past it is
/// scrolled, and the frame fades that edge.
#[gpui::test]
fn no_round_control_is_cut_by_its_clip(cx: &mut TestAppContext) {
    let mut failures = Vec::new();
    let mut rounds = 0usize;
    walk(cx, &[false], |case, _, visual| {
        let exempt = content(visual);
        let fades = crate::edge_fade::take_painted();
        for quad in painted(visual) {
            let (w, h) = (quad.at[2] - quad.at[0], quad.at[3] - quad.at[1]);
            let round = w >= 4.0
                && (w - h).abs() <= 0.01
                && quad.radii.iter().all(|radius| *radius >= w / 2.0 - 0.01);
            let seen = quad.fill.is_some_and(|fill| fill.a > 0.03)
                || (quad.widths.iter().any(|width| *width > 0.0) && quad.border.a > 0.03);
            let shown = intersect(quad.at, quad.mask);
            if !round
                || !seen
                || is_empty(shown)
                || exempt.iter().any(|region| inside(shown, *region, 0.5))
            {
                continue;
            }
            rounds += 1;
            // Half a device pixel: an edge on the clip's own edge rounds
            // either way.
            if !inside(quad.at, quad.mask, 0.26) && !scrolled_past(quad.at, quad.mask, &fades) {
                failures.push(format!(
                    "{case}: the round control at {:?} is cut by its clip {:?}",
                    quad.at, quad.mask
                ));
            }
        }
    });
    assert!(rounds > 0, "the walk drew no round control");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
