//! A press on a Project panel list entry acts once per click, as one undo
//! step.
//!
//! WHY: every row of the place and apply lists acted on each mouse down, so a
//! double-click on a title placed two titles, and a double-click on an
//! animation, a mask or a transition applied it twice, each as its own undo
//! step. A placement that needed a new lane was two steps as well, the first
//! named `Add track`. The class closed here: an entry of any list in [`LISTS`]
//! that acts on the second press of a double-click, or whose one act takes
//! more than one Undo to take back. The tabs are the body tests' own list,
//! which every new tab joins or fails.
//!
//! Not covered: plugin clip type rows, which need an installed plugin, and
//! stock and SFX results, which need the network. Their press listeners hold
//! the same `click_count` check, and their placements the same
//! `place_generated` step.

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, Pixels, Point, TestAppContext, VisualTestContext,
};

use super::body_tests::{LISTS, open};
use super::layout_tests::bounds;
use crate::app::AppState;
use crate::test_support::drawn_shell;
use crate::types::SidebarTab;

/// The center and text of the topmost entry drawn in the list `id`, under its
/// usage line, or of the entry reading `label` when one is given.
fn first_entry(
    visual: &mut VisualTestContext,
    id: &str,
    label: Option<&str>,
) -> (Point<Pixels>, String) {
    let list = bounds(visual, id);
    let usage = bounds(visual, &format!("{id}-usage"));
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let run = runs
        .iter()
        .filter(|run| list.contains(&run.bounds.center()) && !usage.contains(&run.bounds.center()))
        .filter(|run| label.is_none_or(|label| run.text.as_ref() == label))
        .min_by(|a, b| f32::from(a.bounds.top()).total_cmp(&f32::from(b.bounds.top())))
        .unwrap_or_else(|| panic!("{id} draws no entry (reading {label:?})"));
    (run.bounds.center(), run.text.to_string())
}

/// One press and release at `at`, the `click_count`th of a click.
fn press(visual: &mut VisualTestContext, at: Point<Pixels>, click_count: usize) {
    visual.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count,
        first_mouse: false,
    });
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

fn depth(visual: &mut VisualTestContext, state: &gpui::Entity<AppState>) -> usize {
    visual.read(|app| state.read(app).history_depth())
}

#[gpui::test]
fn a_double_click_on_a_list_entry_acts_once(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "sidebar-entry-press");
    visual.update(|_, app| app.set_reduce_motion(true));

    // The clip the apply lists act on: a title, placed by one press on a
    // sequence with no title lane, so the placement adds one.
    open(&state, visual, SidebarTab::Titles);
    let (title, _) = first_entry(visual, "sidebar-titles", None);
    let before = depth(visual, &state);
    press(visual, title, 1);
    let (target, after, label, placement) = visual.read(|app| {
        let state = state.read(app);
        let target = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .next()
            .map(|clip| aria_models::ClipId::from_raw(clip.id))
            .expect("a press on a title places one");
        let placement = aria_commands::EditCommand::AddGenerated {
            track: aria_models::TrackId::from_raw(0),
            generator: aria_models::Generator::text("Title"),
            timeline: aria_models::TimeRange::at_rate(0, 1, state.playhead.rate),
        }
        .label();
        let after = state.history_depth();
        let label = after.checked_sub(1).and_then(|row| {
            state
                .history_entries()
                .get(row)
                .map(|entry| entry.label.clone())
        });
        (target, after, label, placement)
    });
    assert_eq!(
        (after, label.as_deref()),
        (before + 1, Some(placement)),
        "a title placed with the lane it needs is one undo step named after the placement"
    );

    // A second title at the first one's end, on its lane, so the first has a
    // right cut for the Transitions list to act on.
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let rate = state.playhead.rate;
            let end = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| &track.clips)
                .map(|clip| clip.timeline_end_ticks)
                .max()
                .unwrap_or(0);
            state.playhead = aria_models::RationalTime::new(end, rate);
            state
                .add_generated(aria_models::Generator::text("Title"))
                .expect("a title at the first one's end");
            state.playhead = aria_models::RationalTime::new(0, rate);
        })
    });
    visual.run_until_parked();

    for (tab, id) in LISTS {
        open(&state, visual, tab);
        // Generators lists a plugin's clip types first, and which clip types are
        // registered depends on the tests that ran before in this process, so
        // the list's own first generator is pressed.
        let generator = matches!(tab, SidebarTab::Generators)
            .then(|| {
                crate::catalog_text::generators()
                    .first()
                    .map(|g| g.label.to_string())
            })
            .flatten();
        let (at, entry) = first_entry(visual, id, generator.as_deref());
        for (gesture, presses) in [("a click", &[1][..]), ("a double-click", &[1, 2][..])] {
            visual.update(|_, app| state.update(app, |state, _| state.select_clip(target)));
            visual.run_until_parked();
            let before = depth(visual, &state);
            for &count in presses {
                press(visual, at, count);
            }
            let (after, status) = visual.read(|app| {
                let state = state.read(app);
                (state.history_depth(), state.status_message.clone())
            });
            assert_eq!(
                after,
                before + 1,
                "{gesture} on {tab:?}'s first entry, {entry:?}, took the undo list from {before} \
                 to {after} steps, not one step further (status: {status:?})"
            );
            let undone = visual.update(|_, app| state.update(app, |state, _| state.undo()));
            assert!(undone, "{gesture} on {tab:?} left no step to undo");
            visual.run_until_parked();
        }
    }
}
