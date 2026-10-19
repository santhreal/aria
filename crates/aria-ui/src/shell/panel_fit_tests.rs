//! Every panel keeps its text and its images inside a side frame.
//!
//! The class closed here: a row laid out for a wide panel and drawn past the
//! frame's edge in a narrow one, where the frame clips it: a chip row that does
//! not wrap, a label that does not truncate, a fixed-width column, an image
//! wider than the frame, and a row of buttons that wraps by shortening its
//! labels instead of moving them to the next line.
//!
//! The panels are read from the registry when the test runs, so a panel added
//! to the app is drawn here without an edit. Each is docked into the launch
//! layout's right frame, which has only the window's edge to its right, at its
//! launch width and at the frame's minimum width (the widest minimum of its
//! panels), with a clip selected so the panels that show a selection have one
//! and a file with a long name without spaces in the Project panel's pool;
//! once more at the minimum with a scope reading, which the Scopes panel draws
//! in place of its wait; the Project panel twice more with a bin open, which
//! adds the bin's actions to its action row; and the Viewer twice more with
//! the long name open in a Source tab, which adds the marks and edits row.
//!
//! A line of text is the frame's when its vertical centre is in the frame and
//! its right end is past the frame's left edge. A one-line run must stand
//! inside the frame across its whole width. A wrapped paragraph is recorded
//! with its unwrapped width; it must have at least as many lines as the text
//! takes when wrapped at the frame's right edge, so its right end is inside
//! too. A run is read as wrapped when its height is two or more lines at the
//! first of [`LINE_HEIGHTS`] it is a whole number of. Every image and icon the
//! frame draws, read from [`drawn`], must stand inside the frame across its
//! width. In every row laid out by [`crate::components::Wraps`], each label is
//! one line, no two labels on a line overlap, and a label that ends in an
//! ellipsis is alone on its line. A
//! run or an image that starts inside a
//! region listed in [`SIDEWAYS_SCROLLERS`] scrolls into view and is not
//! checked. Not covered: text cut off at the frame's bottom, which a scrolling
//! panel does by design and the frame-height test covers; an image drawn by
//! other means than [`crate::components::picture`] or an icon; and the glyphs,
//! which the test text system does not raster.

use aria_analysis::WaveformTrace;
use aria_core::RgbaImage;
use gpui::{Bounds, Pixels, TestAppContext, TextRun, TextRunLayout, point, px, size};

use crate::app::scope_view::{ScopeBuffers, measure};
use crate::components::drawn::{self, Drawn};
use crate::dock::{DockTree, FrameId, PANEL_PROJECT, PANEL_SCOPES, PANEL_VIEWER, Zone};
use crate::test_support::drawn_shell;

const EPS: f32 = 0.51;
/// The window the launch layout is built for.
const WINDOW_W: f32 = 1920.0;
/// The narrow pass sets weights over the whole row, splitters included, so
/// the right frame lands within a splitter's width of its minimum.
const SPLITTER_SLACK: f32 = 8.0;
/// The line heights text is set in, in ems: gpui's default, the golden
/// ratio, first, then the ones a panel sets.
const LINE_HEIGHTS: [f32; 4] = [1.618034, 1.5, 1.25, 1.0];
/// Regions that scroll sideways by design: the Audio Track Mixer's strips, as
/// a console's do, and the Timeline's ruler, which runs on with time.
const SIDEWAYS_SCROLLERS: &[&str] = &["mixer-strips", "timeline-ruler"];

/// One drawing of a panel.
#[derive(Clone, Copy)]
struct Pass {
    panel: &'static str,
    narrow: bool,
    measured: bool,
    bin_open: bool,
    source: bool,
}

#[gpui::test]
fn every_panel_keeps_its_text_inside_a_side_frame_down_to_its_minimum_width(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "panel-fit");
    // A split's weights glide; reduced motion lands them on the next draw.
    visual.update(|_, app| app.set_reduce_motion(true));
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    // The longest name among the fixtures, and none of it a space to wrap at.
    let long_name = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-3s-av.mp4");
    let long = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            let long = state.import_media(long_name).expect("import the long name");
            let clip = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .map(|clip| aria_models::ClipId::from_raw(clip.id))
                .next()
                .expect("a clip");
            state.select_clip(clip);
            cx.notify();
            long
        })
    });
    visual.run_until_parked();
    let panels = visual.read(|app| state.read(app).registered_panel_ids());
    assert!(!panels.is_empty(), "the ui plugin registers panels");
    let passes = panels.iter().flat_map(|&panel| {
        let pass = |narrow, measured, bin_open, source| Pass {
            panel,
            narrow,
            measured,
            bin_open,
            source,
        };
        let mut passes = vec![
            pass(false, false, false, false),
            pass(true, false, false, false),
            pass(true, true, false, false),
        ];
        if panel == PANEL_PROJECT {
            passes.extend([
                pass(false, false, true, false),
                pass(true, false, true, false),
            ]);
        }
        if panel == PANEL_VIEWER {
            passes.extend([
                pass(false, false, false, true),
                pass(true, false, false, true),
            ]);
        }
        passes
    });
    let mut misses = Vec::new();
    for pass in passes {
        let Pass {
            panel,
            narrow,
            measured,
            bin_open,
            source,
        } = pass;
        let frame_min = visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.dock_tree = DockTree::editing_for_width(WINDOW_W);
                state.dock_tree.dock(panel, FrameId::RIGHT, Zone::Center);
                state.scope_textures = measured.then(|| {
                    let frame = RgbaImage::new(16, 16, vec![128; 16 * 16 * 4]);
                    measure(&frame, 0, WaveformTrace::Luma, &mut ScopeBuffers::default())
                        .expect("a well-formed frame measures")
                });
                if !bin_open {
                    state.open_bin(None);
                } else if state.current_bin.is_none() {
                    state.create_bin().expect("create a bin");
                    state.cancel_bin_rename();
                }
                if source {
                    assert!(
                        state.open_source(long.raw()),
                        "the long name opens in a Source tab"
                    );
                } else {
                    state.show_program();
                }
                // A frame is as narrow as the widest minimum of its panels.
                let frame_min = state
                    .dock_tree
                    .frame(FrameId::RIGHT)
                    .expect("the launch layout has a right frame")
                    .tabs
                    .iter()
                    .map(|&tab| state.dock_panel_min_size(tab).0)
                    .fold(0.0, f32::max);
                if narrow {
                    // The upper row is the root's first split, the right frame
                    // its last child. Docking a panel that had a frame of its
                    // own drops that frame, so the row holds two or three: the
                    // right frame takes its minimum width and the rest keep
                    // their launch proportions.
                    let row = state
                        .dock_tree
                        .split_weights(&[0])
                        .expect("the launch layout's upper area is a row split");
                    let (_, rest) = row.split_last().expect("a row has children");
                    let right = frame_min / WINDOW_W;
                    let total: f32 = rest.iter().sum();
                    let weights: Vec<f32> = rest
                        .iter()
                        .map(|w| w / total * (1.0 - right))
                        .chain(std::iter::once(right))
                        .collect();
                    assert!(
                        state.dock_tree.set_weights(&[0], &weights),
                        "{panel}: the upper row takes no weights {weights:?}"
                    );
                }
                cx.notify();
                frame_min
            })
        });
        visual.run_until_parked();
        // A second draw: the first records the frame's rect, which some
        // panels read to lay themselves out. It is the one recorded.
        drawn::start();
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
        let boxes = drawn::take();
        let rect = visual.read(|app| {
            state
                .read(app)
                .dock_frame_rects
                .lock()
                .get(&FrameId::RIGHT)
                .copied()
        });
        let what = format!(
            "{panel}{}{}{}",
            if measured { " with a reading" } else { "" },
            if bin_open { " with a bin open" } else { "" },
            if source { " with a Source tab" } else { "" }
        );
        let Some([x, y, w, h]) = rect else {
            misses.push(format!("{what}: the right frame was not drawn"));
            continue;
        };
        if narrow {
            assert!(
                (w - frame_min).abs() <= SPLITTER_SLACK,
                "{what}: the narrow pass drew the right frame {w} px wide, not its {frame_min} px minimum"
            );
        }
        let what = format!("{what} in a {w} px frame");
        let frame = Bounds::new(point(px(x), px(y)), size(px(w), px(h)));
        // In paint order, the wrap rows' markers among them.
        let painted = visual.update(|window, _| window.rendered_text_runs().to_vec());
        let runs: Vec<&TextRunLayout> = painted
            .iter()
            .filter(|run| !drawn::is_marker(run))
            .collect();
        if measured && panel == PANEL_SCOPES {
            assert!(
                runs.iter()
                    .any(|run| run.text.as_ref() == "Vectorscope"
                        && frame.contains(&run.bounds.origin)),
                "{what}: the measured pass drew no reading"
            );
        }
        let scrollers: Vec<Bounds<Pixels>> = SIDEWAYS_SCROLLERS
            .iter()
            .filter_map(|&region| visual.debug_bounds(region))
            .collect();
        // A sideways scroller holds more than it shows: what starts in its
        // band at or right of its left edge scrolls into view.
        let scrolls = |at: &Bounds<Pixels>| {
            let mid = at.center().y;
            scrollers.iter().any(|region| {
                mid >= region.top() && mid <= region.bottom() && at.left() >= region.left()
            })
        };
        let mine = |at: &Bounds<Pixels>| {
            let mid = at.center().y;
            mid >= frame.top() && mid <= frame.bottom() && at.right() > frame.left() + px(EPS)
        };
        let outside = |at: &Bounds<Pixels>| {
            at.left() < frame.left() - px(EPS)
                || at.right() > frame.right() + px(EPS)
                || at.left() >= frame.right() - px(EPS)
        };
        let runs: Vec<&TextRunLayout> = runs
            .into_iter()
            .filter(|run| mine(&run.bounds) && !scrolls(&run.bounds))
            .collect();
        // The same runs in paint order with the wrap rows' markers kept.
        let ordered: Vec<&TextRunLayout> = painted
            .iter()
            .filter(|run| drawn::is_marker(run) || (mine(&run.bounds) && !scrolls(&run.bounds)))
            .collect();
        for run in &runs {
            let place = |why: &str| {
                format!(
                    "{what}: `{}` spans {:?}..{:?}, the frame {:?}..{:?}: {why}",
                    run.text,
                    run.bounds.left(),
                    run.bounds.right(),
                    frame.left(),
                    frame.right()
                )
            };
            let Some(drawn_lines) = lines(run) else {
                misses.push(place(&format!(
                    "{:?} tall, not a whole number of lines at any line height",
                    run.bounds.size.height
                )));
                continue;
            };
            if drawn_lines <= 1 {
                if outside(&run.bounds) {
                    misses.push(place("past the frame"));
                }
                continue;
            }
            if run.bounds.left() < frame.left() - px(EPS)
                || run.bounds.left() >= frame.right() - px(EPS)
            {
                misses.push(place("starts outside the frame"));
                continue;
            }
            let room = frame.right() - run.bounds.left() + px(EPS);
            let fit_lines = visual.update(|window, _| {
                let font = window.text_style().font();
                let style = TextRun {
                    len: run.text.len(),
                    font,
                    ..TextRun::default()
                };
                window
                    .text_system()
                    .shape_text(run.text.clone(), run.font_size, &[style], Some(room), None)
                    .expect("the test text system shapes text")
                    .iter()
                    .map(|line| line.wrap_boundaries().len() + 1)
                    .sum::<usize>()
            });
            if drawn_lines < fit_lines {
                misses.push(place(&format!(
                    "wrapped onto {drawn_lines} lines; inside the frame it takes {fit_lines}"
                )));
            }
        }
        for (_, image) in boxes
            .iter()
            .filter(|(kind, at)| *kind == Drawn::Image && mine(at) && !scrolls(at))
        {
            if outside(image) {
                misses.push(format!(
                    "{what}: an image spans {:?}..{:?}, the frame {:?}..{:?}",
                    image.left(),
                    image.right(),
                    frame.left(),
                    frame.right()
                ));
            }
        }
        for (_, row) in boxes
            .iter()
            .filter(|(kind, at)| *kind == Drawn::WrapRow && mine(at))
        {
            wrap_row_misses(&what, row, &ordered, &mut misses);
        }
        if panel == PANEL_PROJECT {
            // A control row with a bar's padding and rule around it.
            let one_row =
                px(crate::theme::Theme::ROW_HEIGHT + 2.0 * crate::theme::Theme::SPACE_XS + 2.0);
            for bar in ["sidebar-groups", "pool-header", "pool-footer"] {
                let Some(at) = visual.debug_bounds(bar) else {
                    misses.push(format!("{what}: {bar} was not drawn"));
                    continue;
                };
                if at.size.height > one_row {
                    misses.push(format!(
                        "{what}: {bar} is {:?} tall, more than one row",
                        at.size.height
                    ));
                }
            }
        }
    }
    assert!(
        misses.is_empty(),
        "text or images run past their frame:\n{}",
        misses.join("\n")
    );
}

/// How many lines a run drew: its height over the first of
/// [`LINE_HEIGHTS`] it is a whole number of, or `None` for none. A line is
/// that height in ems, or that height rounded to a whole pixel, as a text
/// element sets it.
fn lines(run: &TextRunLayout) -> Option<usize> {
    LINE_HEIGHTS.iter().find_map(|em| {
        let exact = run.font_size * *em;
        [exact, exact.round()].into_iter().find_map(|line| {
            let count = (run.bounds.size.height / line).round().max(1.0);
            ((run.bounds.size.height - line * count).abs() <= px(1.0)).then_some(count as usize)
        })
    })
}

/// A row that wraps keeps every label whole and apart: each label in it is one
/// line, no two on a line overlap, and one that ends in an ellipsis is alone
/// on its line, wider than the row.
///
/// The row's labels are the runs painted after its marker that start inside
/// it and end above its bottom, since a row grows to hold what it lays out.
/// Runs are recorded unclipped; a list's text scrolled under a row drawn below
/// the list was painted before the row's marker and is not the row's.
fn wrap_row_misses(
    what: &str,
    row: &Bounds<Pixels>,
    painted: &[&TextRunLayout],
    misses: &mut Vec<String>,
) {
    let Some(start) = painted
        .iter()
        .position(|run| drawn::is_marker(run) && run.bounds == *row)
    else {
        misses.push(format!(
            "{what}: the row that wraps at {row:?} painted no marker"
        ));
        return;
    };
    let inside = |run: &&&TextRunLayout| {
        let at = run.bounds;
        !drawn::is_marker(run)
            && at.left() >= row.left() - px(EPS)
            && at.left() < row.right()
            && at.top() >= row.top() - px(EPS)
            && at.bottom() <= row.bottom() + px(EPS)
    };
    let members: Vec<&TextRunLayout> = painted[start + 1..]
        .iter()
        .filter(inside)
        .copied()
        .collect();
    for (index, run) in members.iter().enumerate() {
        if let Some(count @ 2..) = lines(run) {
            misses.push(format!(
                "{what}: `{}` wraps onto {count} lines inside a row that wraps, the run {:?}, the row {row:?}",
                run.text, run.bounds
            ));
        }
        let mid = run.bounds.center().y;
        for other in &members[index + 1..] {
            if other.bounds.top() <= mid
                && other.bounds.bottom() >= mid
                && other.bounds.left() < run.bounds.right() - px(EPS)
                && run.bounds.left() < other.bounds.right() - px(EPS)
            {
                misses.push(format!(
                    "{what}: `{}` ({:?}..{:?}) overlaps `{}` ({:?}..{:?}) in a row that wraps",
                    run.text,
                    run.bounds.left(),
                    run.bounds.right(),
                    other.text,
                    other.bounds.left(),
                    other.bounds.right()
                ));
            }
        }
        if !run.text.ends_with('…') {
            continue;
        }
        let beside: Vec<&str> = members
            .iter()
            .filter(|other| !std::ptr::eq(**other, *run))
            .filter(|other| other.bounds.top() <= mid && other.bounds.bottom() >= mid)
            .map(|other| other.text.as_ref())
            .collect();
        if !beside.is_empty() {
            misses.push(format!(
                "{what}: `{}` is shortened beside {beside:?} in a row that wraps",
                run.text
            ));
        }
    }
}
