//! Every item of a picture view draws one picture slot of one size, and a
//! Tiles row draws its text beside its picture.
//!
//! The classes closed here: an item that draws no slot because it has no frame
//! to show (an audio file, a file that is gone), which puts its name out of
//! line with the names around it; a slot stretched across the panel, which
//! draws the picture in a band of empty plate; and a Tiles row whose text is
//! stacked under its picture while the panel has room beside it, or drawn past
//! the panel's edge when it has not. The views are read from
//! [`PoolView::ALL`], so a view added later is held to the same rules.
//!
//! Not covered: the picture's pixels, which the test platform does not decode,
//! and where the Freeform canvas places each item.

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, px, size};

use super::layout_tests::{bounds, project_width};
use crate::app::AppState;
use crate::app::fixtures::asset;
use crate::app::media::media_facts;
use crate::app::pool_preview::{THUMB_H, THUMB_W};
use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::types::PoolView;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// A window tall enough that every item of the mixed pool, its text wrapped
/// under its picture in the narrowest frame, lies inside the Project panel.
/// An item past the panel's foot shares its place with the Timeline under the
/// panel, and a read of the runs by place takes the Timeline's text for the
/// item's.
const TALL: f32 = 2400.0;

/// Whether `inner` lies inside `outer` from top to bottom.
fn down_inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.top() >= outer.top() - px(EPS) && inner.bottom() <= outer.bottom() + px(EPS)
}

/// A drawn shell whose pool holds a video, an audio file, a still, and a
/// second video whose file is gone.
fn mixed_pool<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (gpui::Entity<AppState>, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, tag);
    visual.simulate_resize(size(px(1920.0), px(TALL)));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            for name in [
                "fixture-640x360-24fps-3s.mp4",
                "fixture-sine-48k-2s.wav",
                "texture.png",
            ] {
                state
                    .import_media(asset(name))
                    .unwrap_or_else(|err| panic!("import {name}: {err:?}"));
            }
            let gone = state
                .import_media(asset("fixture-640x360-30fps-3s.mp4"))
                .expect("import the second video")
                .raw();
            let item = state
                .store_mut()
                .media
                .iter_mut()
                .find(|item| item.id == gone)
                .expect("the second video is in the pool");
            item.missing = true;
            cx.notify();
        })
    });
    visual.run_until_parked();
    (state, visual)
}

/// Draw the pool in `view` and return how many items it lists.
fn show(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext, view: PoolView) -> usize {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.set_pool_view(view);
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.read(|app| {
        let state = state.read(app);
        state.pool_rows(state.sidebar_tab).len()
    })
}

/// The text runs that start inside `item` and outside its picture `slot`: the
/// item's text, not a stand-in drawn while the picture decodes.
///
/// A run is placed by where it starts. A run that wraps reports the width of
/// its whole text on one line, so its centre and right edge can fall outside
/// the box that wrapped it.
fn text_of(
    visual: &mut VisualTestContext,
    item: Bounds<Pixels>,
    slot: Bounds<Pixels>,
) -> Vec<(String, Bounds<Pixels>)> {
    visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| {
                let start = run.bounds.origin + gpui::point(px(EPS), px(EPS));
                item.contains(&start) && !slot.contains(&start) && !run.text.is_empty()
            })
            .map(|run| (run.text.to_string(), run.bounds))
            .collect()
    })
}

/// WHY: Tiles drew its picture in a box as wide as the panel, so a 16:9 frame
/// sat between two bands of empty plate, and an audio file or a missing file
/// drew no box at all, so its name started where the pictures of its
/// neighbours did. Every item of a view that draws pictures now draws one slot,
/// the same size as every other item's, and a view whose items fill the rail
/// draws it at the thumbnail's size. A view that draws no pictures draws no
/// slot, because a slot is what costs a decode.
#[gpui::test]
fn every_item_of_a_picture_view_draws_one_slot_of_one_size(cx: &mut TestAppContext) {
    let (state, visual) = mixed_pool(cx, "pool-slots");
    for &view in PoolView::ALL {
        let rows = show(&state, visual, view);
        assert_eq!(rows, 4, "{view:?} lists {rows} items, the pool holds 4");
        let slots: Vec<Option<Bounds<Pixels>>> = (0..rows)
            .map(|index| visual.debug_bounds(intern(&format!("pool-picture-{index}"))))
            .collect();
        if !view.picture() {
            assert!(
                slots.iter().all(Option::is_none),
                "{view:?} draws no pictures, yet draws a picture slot: {slots:?}"
            );
            continue;
        }
        let slots: Vec<Bounds<Pixels>> = slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| {
                slot.unwrap_or_else(|| panic!("{view:?}: item {index} draws no picture slot"))
            })
            .collect();
        let size = match view.width() {
            None => gpui::size(px(THUMB_W as f32), px(THUMB_H as f32)),
            Some(_) => slots[0].size,
        };
        for (index, slot) in slots.iter().enumerate() {
            assert!(
                (slot.size.width - size.width).abs() <= px(EPS)
                    && (slot.size.height - size.height).abs() <= px(EPS),
                "{view:?}: item {index} draws a {:?} picture slot, the view's slot is {size:?}",
                slot.size
            );
        }
        if view.width().is_none() {
            for (index, slot) in slots.iter().enumerate() {
                assert!(
                    (slot.left() - slots[0].left()).abs() <= px(EPS),
                    "{view:?}: item {index}'s picture starts at {:?}, item 0's at {:?}",
                    slot.left(),
                    slots[0].left()
                );
            }
        }
    }
}

/// WHY: a Tiles row stacked its name and facts under its picture, so a row was
/// as tall as a picture and two lines of text and the launch layout showed
/// little more than one item. The text now stands beside the picture, and the
/// row is as tall as the picture unless its text needs more. In a frame too
/// narrow for the two side by side, the text wraps under the picture and stays
/// inside the panel rather than being squeezed past its edge.
#[gpui::test]
fn a_tiles_row_reads_beside_its_picture_and_under_it_when_narrow(cx: &mut TestAppContext) {
    let (state, visual) = mixed_pool(cx, "pool-tiles");
    let rows = show(&state, visual, PoolView::Tiles);
    let gone = visual.read(|app| {
        let state = state.read(app);
        let rows = state.pool_rows(state.sidebar_tab);
        rows.iter().position(|item| item.missing)
    });
    let row_padding = 2.0 * Theme::SPACE_XS + 2.0;
    for index in 0..rows {
        let item = bounds(visual, &format!("pool-item-{index}"));
        let slot = bounds(visual, &format!("pool-picture-{index}"));
        let runs = text_of(visual, item, slot);
        assert!(!runs.is_empty(), "item {index} draws no text");
        for (text, run) in &runs {
            assert!(
                run.left() >= slot.right() - px(EPS),
                "item {index}: `{text}` at {run:?} is not beside the picture {slot:?}"
            );
        }
        if Some(index) != gone {
            assert!(
                (item.size.height - slot.size.height - px(row_padding)).abs() <= px(EPS),
                "item {index} is {:?} tall around a {:?} picture: its text does not fit beside it",
                item.size.height,
                slot.size.height
            );
        }
    }

    project_width(&state, visual, 160.0);
    let panel = bounds(visual, "sidebar");
    for index in 0..rows {
        let item = bounds(visual, &format!("pool-item-{index}"));
        let slot = bounds(visual, &format!("pool-picture-{index}"));
        let name = bounds(visual, &format!("pool-name-{index}"));
        assert!(
            name.top() >= slot.bottom() - px(EPS),
            "in a 160 px frame, item {index}'s name {name:?} is not under its picture {slot:?}"
        );
        // The boxes, not the runs: a wrapped run reports its text's width on
        // one line (see `text_of`), and the item is the box its text wraps in.
        for (what, at) in [
            ("the item", item),
            ("the picture", slot),
            ("the name", name),
        ] {
            assert!(
                at.left() >= panel.left() - px(EPS) && at.right() <= panel.right() + px(EPS),
                "in a 160 px frame, item {index}: {what} at {at:?} runs out of the panel {panel:?}"
            );
        }
        assert!(
            down_inside(item, panel),
            "in a 160 px frame, item {index} at {item:?} lies past the panel {panel:?}"
        );
        for (text, run) in text_of(visual, item, slot) {
            assert!(
                run.left() >= item.left() && run.top() >= slot.bottom() - px(EPS),
                "in a 160 px frame, item {index}: `{text}` at {run:?} is not under its picture {slot:?}"
            );
        }
    }
}

/// WHY: a row's facts were one string that wrapped wherever the row ran out of
/// room, so a line could start with the `·` between two facts, and a List row,
/// which reads as one line, could take two. Every view that draws the facts
/// now draws each line of them on one line: whole beside a picture at the
/// launch width, and cut with an ellipsis in the narrowest frame. The views
/// are read from [`PoolView::ALL`], so a view added later is held to the same
/// rule.
///
/// Not covered: which facts a line holds, which `codec_tests` asserts.
#[gpui::test]
fn every_line_of_a_rows_facts_stays_one_line(cx: &mut TestAppContext) {
    let (state, visual) = mixed_pool(cx, "pool-facts");
    // The codec names of a real H.264 file: long enough that a line holding
    // them wraps in a narrow frame unless it keeps to one line.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            for item in &mut state.store_mut().media {
                item.video_codec = Some("H.264 (High Profile)".into());
                item.audio_codec = Some("MPEG-4 AAC".into());
            }
            cx.notify();
        })
    });
    for (frame, width) in [("the launch frame", None), ("a 160 px frame", Some(160.0))] {
        if let Some(width) = width {
            project_width(&state, visual, width);
        }
        for &view in PoolView::ALL {
            let rows = show(&state, visual, view);
            let facts: Vec<_> = visual.read(|app| {
                let state = state.read(app);
                state
                    .pool_rows(state.sidebar_tab)
                    .iter()
                    .map(media_facts)
                    .collect()
            });
            assert_eq!(facts.len(), rows);
            for (index, facts) in facts.iter().enumerate() {
                let lines: Vec<String> = if !view.facts() {
                    Vec::new()
                } else if view.inline() {
                    vec![facts.line()]
                } else {
                    std::iter::once(facts.picture.clone())
                        .chain(facts.streams.clone())
                        .collect()
                };
                let drawn =
                    visual.debug_bounds(intern(&format!("pool-facts-{index}-{}", lines.len())));
                assert!(
                    drawn.is_none(),
                    "{view:?} in {frame}: item {index} draws a facts line past its {}",
                    lines.len()
                );
                if lines.is_empty() {
                    continue;
                }
                let name = bounds(visual, &format!("pool-name-{index}"));
                let panel = bounds(visual, "sidebar");
                for (at, line) in lines.iter().enumerate() {
                    let tag = format!("{view:?} in {frame}: item {index}, facts line {at}");
                    let block = bounds(visual, &format!("pool-facts-{index}-{at}"));
                    assert!(
                        down_inside(block, panel),
                        "{tag} at {block:?} lies past the panel {panel:?}"
                    );
                    assert!(
                        block.size.height <= name.size.height + px(EPS),
                        "{tag} is {:?} tall, one line is {:?}: `{line}` wrapped",
                        block.size.height,
                        name.size.height
                    );
                    let runs: Vec<String> = visual.update(|window, _| {
                        window
                            .rendered_text_runs()
                            .iter()
                            .filter(|run| {
                                block.contains(&(run.bounds.origin + gpui::point(px(EPS), px(EPS))))
                            })
                            .map(|run| run.text.to_string())
                            .collect()
                    });
                    let [text] = runs.as_slice() else {
                        panic!("{tag} draws {runs:?}, one run of `{line}` was expected");
                    };
                    let whole = width.is_none() && !view.inline();
                    let cut = text
                        .strip_suffix('…')
                        .is_some_and(|kept| line.starts_with(kept.trim_end()));
                    assert!(
                        text == line || (!whole && cut),
                        "{tag} draws `{text}`, the line is `{line}`"
                    );
                }
            }
        }
    }
}
