//! A cached panel body draws what a full redraw draws.
//!
//! The class closed here is a docked panel that replays a stale drawing: a
//! change the panel draws reached the window, and the panel's cached body was
//! not notified, so the frame shows the panel as it was. Every panel the
//! registry holds is docked in turn, over a fresh project, and every change in
//! [`CHANGES`] is made through the route the app makes it: a narrow
//! [`Shell::redraw`](crate::shell::Shell::redraw) for a moved view or
//! playhead, the pool tile route for a tile hover, the filmstrip route for a
//! landed cell, the frame route for a landed Viewer frame or scope reading,
//! the timeline canvas's pointer listener for a held clip drag, a state
//! notification for the rest. The frame drawn then must equal the frame drawn
//! after a refresh, which builds every body anew. Every panel must also draw
//! differently after at least one change, so a panel added to the registry
//! whose drawing no change here reaches fails until a change it draws is added
//! to [`CHANGES`].
//!
//! The panels are cut into [`SHARDS`] tests by their place in the registry, so
//! the harness draws them on as many threads. Each panel takes the same steps
//! it takes in one pass over the registry.
//!
//! A frame is compared by its quads (bounds, clip, colors, corners, borders),
//! its text runs (text, bounds, size) and its images (bounds, clip, the atlas
//! tile sampled, opacity). Not caught: a stale path.

use gpui::TestAppContext;

use super::tests::{CHANGES, dock_right, drawing, make, opened, shell_of};

/// Tests the registry's panels are cut into.
const SHARDS: usize = 4;

/// The entries of `all` that `shard` draws, with their place in `all`.
fn shard_of<T: Copy>(all: &[T], shard: usize) -> Vec<(usize, T)> {
    assert!(shard < SHARDS, "shard {shard} of {SHARDS}");
    all.iter()
        .copied()
        .enumerate()
        .skip(shard)
        .step_by(SHARDS)
        .collect()
}

#[test]
fn the_shards_draw_every_panel_once() {
    for count in 0..=3 * SHARDS + 1 {
        let all: Vec<usize> = (0..count).collect();
        let mut drawn: Vec<usize> = (0..SHARDS)
            .flat_map(|shard| shard_of(&all, shard))
            .map(|(index, entry)| {
                assert_eq!(index, entry, "a shard numbers a panel by its place");
                entry
            })
            .collect();
        drawn.sort_unstable();
        assert_eq!(drawn, all, "{count} panels over {SHARDS} shards");
    }
}

/// Dock every panel whose place in the registry is `shard` modulo [`SHARDS`],
/// make every change in [`CHANGES`], and hold each frame to a full redraw.
fn cached_bodies_draw_what_a_full_redraw_draws(cx: &mut TestAppContext, shard: usize) {
    cx.executor().allow_parking();
    let panels = {
        let (state, visual) = opened(cx, &format!("panel-cache-{shard}"));
        let panels = visual.read(|app| state.read(app).registered_panel_ids());
        visual.update(|window, _| window.remove_window());
        panels
    };
    assert!(!panels.is_empty(), "the ui plugin registers panels");

    let mut stale = Vec::new();
    let mut unchanged = Vec::new();
    for (index, panel) in shard_of(&panels, shard) {
        // Each panel starts from the same project, so a list that grew under
        // the panels before it (History's steps, the Jobs rows) does not push
        // a change below the frame's edge.
        let (state, visual) = opened(cx, &format!("panel-cache-{shard}-{index}"));
        let shell = shell_of(visual);
        let Some(rect) = dock_right(&state, visual, panel) else {
            stale.push(format!("{panel}: the right frame was not drawn"));
            continue;
        };
        let mut changed = false;
        let mut before = drawing(visual);
        for (at, change) in CHANGES.into_iter().enumerate() {
            make(visual, &shell, change, index * CHANGES.len() + at + 1);
            let cached = drawing(visual);
            visual.update(|window, _| window.refresh());
            visual.run_until_parked();
            let full = drawing(visual);
            if cached != full {
                stale.push(format!(
                    "{panel} after {change:?}: the cached drawing differs from a full redraw\n{}",
                    cached.differences(&full)
                ));
            }
            changed |= before.inside(rect) != full.inside(rect);
            before = full;
        }
        if !changed {
            unchanged.push(panel);
        }
        visual.update(|window, _| window.remove_window());
    }
    assert!(stale.is_empty(), "stale drawings:\n{}", stale.join("\n"));
    assert!(
        unchanged.is_empty(),
        "no change in CHANGES alters the drawing of {unchanged:?}: add one each draws"
    );
}

#[gpui::test]
fn every_cached_panel_body_draws_what_a_full_redraw_draws_shard_0(cx: &mut TestAppContext) {
    cached_bodies_draw_what_a_full_redraw_draws(cx, 0);
}

#[gpui::test]
fn every_cached_panel_body_draws_what_a_full_redraw_draws_shard_1(cx: &mut TestAppContext) {
    cached_bodies_draw_what_a_full_redraw_draws(cx, 1);
}

#[gpui::test]
fn every_cached_panel_body_draws_what_a_full_redraw_draws_shard_2(cx: &mut TestAppContext) {
    cached_bodies_draw_what_a_full_redraw_draws(cx, 2);
}

/// The last shard: a fifth shard added to [`SHARDS`] without a test of its
/// own would leave every fifth panel undrawn, so this fails until one is.
#[gpui::test]
fn every_cached_panel_body_draws_what_a_full_redraw_draws_shard_3(cx: &mut TestAppContext) {
    assert_eq!(SHARDS, 4, "one test per shard");
    cached_bodies_draw_what_a_full_redraw_draws(cx, 3);
}
