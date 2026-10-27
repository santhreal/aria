//! An unchanged workspace is laid out by the window's size alone.
//!
//! The class closed here: a dock whose frames depend on the sizes the window
//! passed through. A first launch draws once at its 1440 px unmaximized size
//! before the window manager maximizes it, and the shipped pixel widths were
//! derived from the first size alone, so Editing opened with side frames a
//! third wider than their shipped pixel widths, and choosing it again drew
//! them at those widths.
//!
//! For every shipped workspace, read from `layouts::WORKSPACES` at run time,
//! chosen before the first size and drawn through each window history, the
//! split weights after the history are the weights the workspace ships with
//! at the last size. A workspace with a recorded edit keeps the edit's weights
//! through a window that grows and shrinks back.
//!
//! Not covered: Custom, saved and plugin workspaces, which are not rebuilt for
//! a width, and floating windows.

use super::*;
use crate::dock::{DockTree, Node};

/// The size every history ends at: a maximized 1920 x 1080 display less its
/// title bar.
const LAST: (f32, f32) = (1920.0, 1059.0);

/// Window sizes drawn before [`LAST`]: none, a first launch's unmaximized
/// window, a window smaller than the frames' minimum sizes, and a maximized
/// window restored down and maximized again.
const HISTORIES: [&[(f32, f32)]; 4] = [
    &[],
    &[(1440.0, 900.0)],
    &[(1024.0, 640.0)],
    &[LAST, (1280.0, 800.0)],
];

/// Every split weight of `tree`, depth first.
fn weights_of(tree: &DockTree) -> Vec<f32> {
    fn walk(node: &Node, out: &mut Vec<f32>) {
        if let Node::Split { children, .. } = node {
            for (weight, child) in children {
                out.push(*weight);
                walk(child, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&tree.root, &mut out);
    out
}

fn same(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

/// The weights workspace `id` ships with at [`LAST`].
fn shipped(id: &str) -> Vec<f32> {
    weights_of(
        &layouts::workspace_tree_for_width(id, LAST.0, &[])
            .unwrap_or_else(|| panic!("{id} builds no dock tree")),
    )
}

/// Draw `sizes` in turn and return the dock's weights after the last.
fn drawn_through(state: &mut AppState, sizes: &[(f32, f32)]) -> Vec<f32> {
    for &(w, h) in sizes {
        state.observe_main_window_size(w, h);
    }
    weights_of(&state.dock_tree)
}

#[test]
fn a_first_launch_opens_at_the_shipped_widths() {
    let cx = gpui::TestAppContext::single();
    let subject = entity(state_in(&dir()), &cx);
    let drawn = cx.update(|app| {
        subject.update(app, |state: &mut AppState, _| {
            drawn_through(state, &[(1440.0, 900.0), LAST])
        })
    });
    let first = layouts::WORKSPACES[0].id;
    assert!(
        same(&drawn, &shipped(first)),
        "{first} opened at {drawn:?}, shipped {:?}",
        shipped(first)
    );
}

#[test]
fn an_unchanged_workspace_is_laid_out_by_the_window_size_alone() {
    let mut failures = Vec::new();
    for ws in &layouts::WORKSPACES {
        for history in HISTORIES {
            let cx = gpui::TestAppContext::single();
            let subject = entity(state_in(&dir()), &cx);
            let sizes: Vec<(f32, f32)> = history.iter().copied().chain([LAST]).collect();
            let drawn = cx.update(|app| {
                subject.update(app, |state: &mut AppState, cx| {
                    assert!(state.apply_workspace(ws.id, cx), "{} did not apply", ws.id);
                    drawn_through(state, &sizes)
                })
            });
            if !same(&drawn, &shipped(ws.id)) {
                failures.push(format!(
                    "{} through {sizes:?}: {drawn:?}, shipped {:?}",
                    ws.id,
                    shipped(ws.id)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_edited_workspace_keeps_its_shares_through_a_resize() {
    let mut failures = Vec::new();
    for ws in &layouts::WORKSPACES {
        let cx = gpui::TestAppContext::single();
        let subject = entity(state_in(&dir()), &cx);
        let (edited, drawn) = cx.update(|app| {
            subject.update(app, |state: &mut AppState, cx| {
                assert!(state.apply_workspace(ws.id, cx), "{} did not apply", ws.id);
                state.observe_main_window_size(LAST.0, LAST.1);
                assert!(
                    state.drag_split_by_pixels(&[0], 0, 40.0, LAST.0),
                    "{}: the upper row's first divider did not move",
                    ws.id
                );
                state.record_dock_edit(cx);
                let edited = weights_of(&state.dock_tree);
                (edited, drawn_through(state, &[(2560.0, 1400.0), LAST]))
            })
        });
        if !same(&drawn, &edited) {
            failures.push(format!("{}: edited {edited:?}, drawn {drawn:?}", ws.id));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
