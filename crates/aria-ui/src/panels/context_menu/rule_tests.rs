//! A context menu draws one rule between two groups of rows, and no rule
//! anywhere else.
//!
//! The classes closed here: a menu whose groups run together as one list, so
//! the lane heights read as more commands of the lane; and a rule left behind
//! where every row of a group was withheld, which draws a rule at the card's
//! edge or two rules in a row. The group of an entry is counted from the
//! `MenuItem::Rule` entries its table lists before it, so a table that gains a
//! rule or a row is held to the rule without a change here, and every target the
//! dressed timeline offers a menu on is opened.
//!
//! Not covered: the rule above the rows plugins add, which only a mounted
//! plugin draws.

use gpui::{Bounds, Pixels, TestAppContext, px};

use super::grid_tests::{dress, rows_of};
use crate::interned::intern;
use crate::test_support::drawn_shell;

const EPS: f32 = 0.51;

#[gpui::test]
fn a_context_menu_draws_one_rule_between_two_groups_and_none_elsewhere(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "ctx-rules");
    visual.update(|_, app| app.set_reduce_motion(true));
    let targets = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let targets = dress(state);
            cx.notify();
            targets
        })
    });
    visual.run_until_parked();
    // Text the panels draw with no menu open, which the card may lie over.
    let underneath = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let mut faults = Vec::new();
    let mut separated = 0;
    for target in targets {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.close_context();
                state.open_context(target.clone(), 240.0, 40.0);
                cx.notify();
            })
        });
        visual.run_until_parked();
        let entries = visual.read(|app| rows_of(state.read(app), &target));
        let card = visual
            .debug_bounds("context-menu-card")
            .unwrap_or_else(|| panic!("the menu on {target:?} drew no card"));
        let mut runs: Vec<(String, Bounds<Pixels>)> = visual.update(|window, _| {
            window
                .rendered_text_runs()
                .iter()
                .filter(|run| {
                    card.contains(&run.bounds.center())
                        && !underneath
                            .iter()
                            .any(|under| under.text == run.text && under.bounds == run.bounds)
                })
                .map(|run| (run.text.to_string(), run.bounds))
                .collect()
        });
        runs.sort_by(|a, b| {
            a.1.top()
                .partial_cmp(&b.1.top())
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        runs.dedup();
        // Where each entry is drawn, in the table's order.
        let mut drawn: Vec<(String, Bounds<Pixels>, usize)> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let (name, at) = match &entry.caption {
                Some(caption) => {
                    let nth = entries[..index]
                        .iter()
                        .filter(|before| before.caption.as_ref() == Some(caption))
                        .count();
                    let at = runs
                        .iter()
                        .filter(|(text, _)| text == caption)
                        .map(|(_, at)| *at)
                        .nth(nth);
                    (format!("`{caption}`"), at)
                }
                None => (
                    "the label swatches".to_string(),
                    visual.debug_bounds("ctx-labels"),
                ),
            };
            match at {
                Some(at) => drawn.push((name, at, entry.group)),
                None => faults.push(format!("{target:?}: {name} is not drawn")),
            }
        }
        let rules: Vec<Bounds<Pixels>> = (0..)
            .map_while(|n| visual.debug_bounds(intern(&format!("ctx-rule-{n}"))))
            .collect();
        let mut placed = 0;
        for pair in drawn.windows(2) {
            let ((above, upper, upper_group), (below, lower, lower_group)) = (&pair[0], &pair[1]);
            let between = rules
                .iter()
                .filter(|rule| {
                    rule.top() >= upper.bottom() - px(EPS) && rule.bottom() <= lower.top() + px(EPS)
                })
                .count();
            let expected = usize::from(upper_group != lower_group);
            if between != expected {
                faults.push(format!(
                    "{target:?}: {between} rules between {above} (group {upper_group}) and {below} (group {lower_group}), {expected} expected"
                ));
            }
            placed += between;
            separated += expected;
        }
        if rules.len() != placed {
            faults.push(format!(
                "{target:?}: {} rules drawn, {placed} of them between two groups: {rules:?}",
                rules.len()
            ));
        }
    }
    assert!(
        faults.is_empty(),
        "context menu rules out of place:\n{}",
        faults.join("\n")
    );
    // The clip, lane, junction, effect and keyframe menus each draw several
    // groups in this timeline, so the assertions above measured real rules.
    assert!(
        separated >= 5,
        "only {separated} group boundaries were drawn"
    );
}
