//! A menu bar drop and each of its flyouts draw one rule between two groups of
//! rows, and no rule anywhere else.
//!
//! The classes closed here: a long drop whose shipped rows are drawn as one
//! undivided list, the plugin group under them aside; a group mark that draws
//! nothing, on a row of the drop, on a row of a submenu, or on
//! a row of a flyout; a rule at a list's edge or two rules in a row; and plugin
//! rows that run on from the shipped rows above them. The groups are read row
//! by row from the `rule` and `flyout_rule` marks of the rows the bar draws, and
//! every menu of [`AppMenu::ALL`] and every flyout of each is opened, so a
//! menu, row or mark added later is held to the rule with no change here.
//!
//! Not covered: whether the rows of a group belong together, which only
//! reading the menu shows; and the separators of the native macOS bar, which
//! `native_menus_tests` counts.

use aria_plugin::{CommandSpec, MenuCommand, MenuPath, PluginContext};
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, px};

use crate::app::AppState;
use crate::chrome::menu_items::{DropEntry, MenuItem, drop_entries};
use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::types::AppMenu;

const EPS: f32 = 0.51;

/// A drop with more shipped entries than this draws at least one rule between
/// two of them.
const GLANCE: usize = 8;

/// The plugin command placed at the top level of every menu.
const TOP: &str = "rule.top";
/// The plugin command placed in File's shipped Export submenu.
const SHIPPED_SUB: &str = "rule.export";
/// The plugin command placed in a group no shipped row uses.
const OWN_SUB: &str = "rule.group";
/// Every plugin command placed: their rows are not the shipped rows a long
/// drop groups.
const PLACED: [&str; 3] = [TOP, SHIPPED_SUB, OWN_SUB];

/// One drawn entry of a list: its name in a fault, its row, and whether its
/// mark asks for a rule above it.
type Entry = (String, Bounds<Pixels>, bool);

/// Check one drawn list against its rules.
fn check(list: &str, drawn: &[Entry], rules: &[Bounds<Pixels>], faults: &mut Vec<String>) {
    let mut placed = 0;
    for pair in drawn.windows(2) {
        let ((above, upper, _), (below, lower, marked)) = (&pair[0], &pair[1]);
        let between = rules
            .iter()
            .filter(|rule| {
                rule.top() >= upper.bottom() - px(EPS) && rule.bottom() <= lower.top() + px(EPS)
            })
            .count();
        let expected = usize::from(*marked);
        if between != expected {
            faults.push(format!(
                "{list}: {between} rules between `{above}` and `{below}`, {expected} expected"
            ));
        }
        placed += between;
    }
    if rules.len() != placed {
        faults.push(format!(
            "{list}: {} rules drawn, {placed} of them between two groups: {rules:?}",
            rules.len()
        ));
    }
}

fn row_of(visual: &mut VisualTestContext, id: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(intern(&format!("menu-row-{id}")))
        .unwrap_or_else(|| panic!("row {id} was not drawn"))
}

fn rules_of(visual: &mut VisualTestContext, list: &str) -> Vec<Bounds<Pixels>> {
    (0..)
        .map_while(|n| visual.debug_bounds(intern(&format!("menu-rule-{list}-{n}"))))
        .collect()
}

fn redraw(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    change: impl FnOnce(&mut AppState),
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            change(state);
            cx.notify();
        });
    });
    visual.run_until_parked();
}

#[gpui::test]
fn a_menu_draws_one_rule_between_two_groups_and_none_elsewhere(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-rules");
    visual.update(|_, app| app.set_reduce_motion(true));
    let ctx = visual.read(|app| state.read(app).ctx.clone());
    let mut held = Vec::new();
    for (id, title) in [
        (TOP, "Rule Top"),
        (SHIPPED_SUB, "Rule Export"),
        (OWN_SUB, "Rule Group"),
    ] {
        held.push(
            ctx.register_command(CommandSpec::new(id, title), |_: &PluginContext| Ok(()))
                .expect("the command registers"),
        );
    }
    let mut place = |command: &str, path: &str| {
        held.push(
            ctx.register_menu_entry(MenuCommand {
                command: command.into(),
                path: MenuPath::parse(path).expect("a menu path"),
            })
            .expect("the menu entry registers"),
        );
    };
    // The top-level rows first, so the group row of Edit follows a plugin row
    // that already opened the plugin group.
    for menu in AppMenu::ALL {
        place(TOP, super::super::plugin_menus::location_of(menu).label());
    }
    place(SHIPPED_SUB, "File/Export");
    place(OWN_SUB, "Edit/Plugin Tools");

    let mut faults = Vec::new();
    for menu in AppMenu::ALL {
        redraw(&state, visual, |state| {
            state.close_menu();
            state.toggle_menu(menu);
        });
        let items: Vec<MenuItem> = visual.read(|app| state.read(app).menu_rows(menu).into_owned());
        let entries = drop_entries(&items);
        let holds = |entry: &DropEntry, row: usize| match entry {
            DropEntry::Row(at) => *at == row,
            DropEntry::Sub { rows, .. } => rows.contains(&row),
        };
        // An entry is marked when a row it draws is. The shipped rows of a
        // long drop start a group below its first entry: the group the plugin
        // rows open under them does not count.
        let mut marked = vec![false; entries.len()];
        let mut grouped = false;
        for (row, item) in items.iter().enumerate().filter(|(_, item)| item.rule) {
            let at = entries
                .iter()
                .position(|entry| holds(entry, row))
                .unwrap_or_else(|| panic!("{menu:?}: row {row} is drawn as no entry"));
            marked[at] = true;
            grouped |= at > 0 && !PLACED.contains(&item.id);
        }
        let shipped = entries
            .iter()
            .filter(|entry| match entry {
                DropEntry::Row(at) => !PLACED.contains(&items[*at].id),
                DropEntry::Sub { rows, .. } => {
                    rows.iter().any(|at| !PLACED.contains(&items[*at].id))
                }
            })
            .count();
        if shipped > GLANCE && !grouped {
            faults.push(format!(
                "{menu:?}: {shipped} shipped entries drawn as one group"
            ));
        }
        // The plugin rows open a group of their own below the shipped rows.
        match items.iter().position(|item| item.id == TOP) {
            Some(at) if !items[at].rule => {
                faults.push(format!("{menu:?}: the plugin row starts no group"));
            }
            Some(_) => {}
            None => faults.push(format!("{menu:?}: the plugin row was not drawn")),
        }
        let label = menu.label();
        let drawn: Vec<Entry> = entries
            .iter()
            .zip(&marked)
            .map(|(entry, marked)| {
                let id = match entry {
                    DropEntry::Row(at) => items[*at].id.to_string(),
                    DropEntry::Sub { caption, .. } => format!("menu-sub-{label}-{caption}"),
                };
                (id.clone(), row_of(visual, &id), *marked)
            })
            .collect();
        let rules = rules_of(visual, label);
        check(&format!("{menu:?}"), &drawn, &rules, &mut faults);

        for (index, entry) in entries.iter().enumerate() {
            let DropEntry::Sub { caption, rows } = entry else {
                continue;
            };
            // Drawn out whether or not a row in it can run: the rules are
            // the drawing's, and a blocked submenu opens no flyout.
            redraw(&state, visual, |state| {
                state.menu_flyout = Some((index, None))
            });
            let list = format!("menu-sub-{label}-{caption}");
            let drawn: Vec<Entry> = rows
                .iter()
                .enumerate()
                .map(|(child, at)| {
                    let item = &items[*at];
                    let marked = child > 0 && item.flyout_rule;
                    (item.id.to_string(), row_of(visual, item.id), marked)
                })
                .collect();
            let rules = rules_of(visual, &list);
            check(
                &format!("{menu:?} > {caption}"),
                &drawn,
                &rules,
                &mut faults,
            );
            let first = rows.first().map(|at| items[*at].id);
            // A plugin row in a shipped submenu opens a group of the flyout; in
            // a group of its own it follows the rows above with no rule.
            for at in rows {
                let item = &items[*at];
                let wanted = match item.id {
                    SHIPPED_SUB => Some((false, true)),
                    OWN_SUB => Some((false, false)),
                    _ => None,
                };
                if let Some(wanted) = wanted
                    && (item.rule, item.flyout_rule) != wanted
                {
                    faults.push(format!(
                        "{menu:?} > {caption}: {} is marked {:?}, {wanted:?} expected (first row {first:?})",
                        item.id,
                        (item.rule, item.flyout_rule)
                    ));
                }
            }
        }
    }
    assert!(
        faults.is_empty(),
        "menu bar rules out of place:\n{}",
        faults.join("\n")
    );
}
