//! Pointer presses on an open menu, in the drawn shell.
//!
//! The class closed here: a press on a panel of an open menu read as a press
//! outside it. The menu bar's dismissal runs before the row under the pointer,
//! so pressing a submenu entry shut the whole menu, and pressing a disabled
//! row closed the drop it leaves open. A submenu a display button opens is the
//! same class: it was drawn as the View menu's flyout at the menu bar, far
//! from the button, and a second press on the button let the dismissal close
//! it and the press open it again.
//!
//! The cases are read at run time: every submenu entry and every disabled row
//! of every menu, from the registry the bar draws, and every display button,
//! from the sources. A new submenu, a new disabled row, or a new display
//! button is pressed on the run that adds it.
//!
//! Not covered: the panels' look, which the snapshots own, and a press in one
//! window while a button's submenu is out in another.

use std::path::Path;

use gpui::{Entity, Modifiers, MouseButton, Pixels, Point, VisualTestContext, point, px};

use crate::app::AppState;
use crate::app::viewer_row_tests::{bounds_of, shell_with_take};
use crate::chrome::menu_items::{DropEntry, drop_entries};
use crate::interned::intern;
use crate::test_support::{bounds_inside, drawn_shell};
use crate::types::AppMenu;

fn click(visual: &mut VisualTestContext, at: Point<Pixels>) {
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

/// The centre of the element drawn with `selector`, which must be on screen.
fn centre(visual: &mut VisualTestContext, selector: &str) -> Point<Pixels> {
    let bounds = bounds_of(visual, selector);
    let window = visual.update(|window, _| window.viewport_size());
    let at = bounds.center();
    assert!(
        at.x < window.width && at.y < window.height,
        "{selector} is drawn off the window at {bounds:?}"
    );
    at
}

/// Open `menu` as a click on its title does, with `flyout` out.
fn open(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    menu: AppMenu,
    flyout: Option<usize>,
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.close_menu();
            state.status_message = None;
            state.toggle_menu(menu);
            if let Some(at) = flyout {
                state.open_menu_flyout(at);
            }
            cx.notify();
        });
    });
    visual.run_until_parked();
}

#[gpui::test]
fn a_press_on_a_submenu_entry_opens_its_flyout_and_keeps_the_drop(cx: &mut gpui::TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-press-submenu");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut pressed = 0;
    for menu in AppMenu::ALL {
        let subs: Vec<(usize, &'static str)> = visual.read(|app| {
            drop_entries(&state.read(app).menu_rows(menu))
                .iter()
                .enumerate()
                .filter_map(|(at, entry)| match entry {
                    DropEntry::Sub { caption, .. } => Some((at, *caption)),
                    DropEntry::Row(_) => None,
                })
                .collect()
        });
        for (at, caption) in subs {
            // The pointer opens the flyout on its way to the click, and the
            // click must not shut it again; pressed with no move first, the
            // click opens it.
            for moved_first in [false, true] {
                open(visual, &state, menu, None);
                let selector = format!("menu-row-menu-sub-{}-{caption}", menu.label());
                let at_entry = centre(visual, &selector);
                if moved_first {
                    visual.simulate_mouse_move(at_entry, None::<MouseButton>, Modifiers::none());
                    visual.run_until_parked();
                }
                click(visual, at_entry);
                let seen = visual.read(|app| {
                    let state = state.read(app);
                    (state.open_menu, state.menu_flyout.map(|(row, _)| row))
                });
                assert_eq!(
                    seen,
                    (Some(menu), Some(at)),
                    "pressing {menu:?} > {caption} (moved there first: {moved_first})"
                );
                pressed += 1;
            }
        }
    }
    assert!(pressed > 0, "no menu draws a submenu");
}

#[gpui::test]
fn a_press_on_a_disabled_row_states_why_and_keeps_the_drop(cx: &mut gpui::TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-press-disabled");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut pressed = 0;
    for menu in AppMenu::ALL {
        // Every disabled row, with the submenu entry it sits under.
        let blocked: Vec<(Option<usize>, &'static str, &'static str)> = visual.read(|app| {
            let state = state.read(app);
            let items = state.menu_rows(menu);
            let mut blocked = Vec::new();
            for (at, entry) in drop_entries(&items).iter().enumerate() {
                let (flyout, rows) = match entry {
                    DropEntry::Row(row) => (None, std::slice::from_ref(row)),
                    DropEntry::Sub { rows, .. } => (Some(at), rows.as_slice()),
                };
                for row in rows {
                    let item = &items[*row];
                    if let Some(reason) = item.blocked_by(state) {
                        blocked.push((flyout, item.id, reason));
                    }
                }
            }
            blocked
        });
        for (flyout, id, reason) in blocked {
            open(visual, &state, menu, flyout);
            let at = centre(visual, &format!("menu-row-{id}"));
            click(visual, at);
            let seen = visual.read(|app| {
                let state = state.read(app);
                (state.open_menu, state.status_message.clone())
            });
            assert_eq!(
                seen,
                (Some(menu), Some(reason.to_owned())),
                "pressing the disabled {menu:?} row {id}"
            );
            pressed += 1;
        }
    }
    assert!(pressed > 0, "an empty project offers no disabled menu row");
}

/// Every button drawn through `display_btn`, by its element id.
fn display_buttons() -> Vec<&'static str> {
    let mut files = Vec::new();
    crate::test_support::collect_rs(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    let needle = concat!("display_", "btn(");
    let mut ids = Vec::new();
    for file in files {
        if file.to_string_lossy().ends_with("_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("read a source file");
        for (at, _) in text.match_indices(needle) {
            // The definition names its parameter, not an id.
            let Some(quoted) = text[at + needle.len()..].trim_start().strip_prefix('"') else {
                continue;
            };
            let id = quoted.split('"').next().expect("a quoted element id");
            ids.push(intern(id));
        }
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Whether each row of the View submenu `caption` is checked, in the order the
/// submenu draws them.
fn checked_rows(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    caption: &str,
) -> Vec<bool> {
    visual.read(|app| {
        let state = state.read(app);
        state
            .menu_rows(AppMenu::View)
            .iter()
            .filter(|item| item.under == Some(caption))
            .map(|item| item.is_checked(state))
            .collect()
    })
}

#[gpui::test]
fn a_display_button_holds_its_submenu_under_itself(cx: &mut gpui::TestAppContext) {
    let buttons = display_buttons();
    for expected in ["timeline-display-settings", "viewer-display-settings"] {
        assert!(
            buttons.contains(&expected),
            "the scan did not reach the display button {expected}: {buttons:?}"
        );
    }
    let (state, visual, media) = shell_with_take(cx, "menu-press-display");
    visual.update(|_, app| app.set_reduce_motion(true));
    // Split Viewer needs a Source tab to put beside Program.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.open_source(media.raw()), "the Source tab opens");
            state.show_program();
            cx.notify();
        })
    });
    visual.run_until_parked();
    let held = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let state = state.read(app);
            (state.open_menu, state.menu_owner, state.display_menu())
        })
    };
    // Each button holds out a submenu of its own.
    let mut captions = Vec::new();
    for &owner in &buttons {
        let button = bounds_of(visual, owner);
        click(visual, button.center());
        let (_, _, caption) = held(visual);
        let caption = caption.unwrap_or_else(|| panic!("{owner} opened no View submenu"));
        assert!(
            !captions.iter().any(|&(_, c)| c == caption),
            "{owner} opens {caption}, which another display button opens: {captions:?}"
        );
        captions.push((owner, caption));
        click(visual, button.center());
    }
    let caption_of = |owner: &str| {
        captions
            .iter()
            .find(|&&(o, _)| o == owner)
            .map(|&(_, c)| c)
            .expect("every button has a caption")
    };
    for (index, owner) in buttons.iter().copied().enumerate() {
        let caption = caption_of(owner);
        let submenu = format!("{owner}-submenu");
        let button = bounds_of(visual, owner);

        click(visual, button.center());
        assert_eq!(
            held(visual),
            (Some(AppMenu::View), Some(owner), Some(caption)),
            "{owner} opened its submenu"
        );
        let panel = bounds_of(visual, &submenu);
        assert!(
            (panel.top() - button.bottom()).abs() < px(1.0)
                && (panel.left() - button.left()).abs() < px(1.0),
            "{owner}'s submenu {panel:?} does not hang from the button {button:?}"
        );
        assert_eq!(
            visual.debug_bounds("menu-drop-View"),
            None,
            "{owner} opened the View title's drop as well"
        );
        let ring = bounds_of(visual, "focus-ring");
        assert!(
            bounds_inside(ring, panel),
            "the lit row {ring:?} is not in {owner}'s submenu {panel:?}"
        );

        // A press on the panel between its rows is the submenu's, and there is
        // no bar beside it for Right to walk to.
        click(visual, panel.origin + point(px(2.0), px(2.0)));
        visual.simulate_keystrokes("right");
        visual.run_until_parked();
        assert_eq!(
            held(visual),
            (Some(AppMenu::View), Some(owner), Some(caption)),
            "a press on the edge of {owner}'s submenu, then Right"
        );

        // A second press on the button is the button's: it closes the submenu.
        click(visual, button.center());
        assert_eq!(held(visual), (None, None, None), "{owner} pressed twice");
        assert_eq!(visual.debug_bounds(intern(&submenu)), None);

        // A press elsewhere closes it and lands where it was aimed.
        click(visual, button.center());
        let ruler = bounds_of(visual, "timeline-ruler");
        let playhead = |visual: &mut VisualTestContext| {
            visual.read(|app| state.read(app).store.playhead_ticks)
        };
        let before = playhead(visual);
        let aim = px(20.0 + 40.0 * index as f32);
        click(visual, point(ruler.left() + aim, ruler.center().y));
        assert_eq!(held(visual), (None, None, None), "a ruler press");
        assert_ne!(playhead(visual), before, "the ruler press did not land");

        // The View title opens its own drop in place of the button's submenu.
        click(visual, button.center());
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.toggle_menu(AppMenu::View);
                cx.notify();
            });
        });
        visual.run_until_parked();
        assert_eq!(held(visual), (Some(AppMenu::View), None, None));
        assert!(visual.debug_bounds("menu-drop-View").is_some());
        assert_eq!(visual.debug_bounds(intern(&submenu)), None);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();

        // A press on a choice runs it and closes the submenu. An unchecked
        // one, so the press moves a check mark whatever kind of row it is.
        let before = checked_rows(visual, &state, caption);
        let choice = before
            .iter()
            .position(|checked| !checked)
            .expect("a display choice that is off");
        click(visual, button.center());
        for _ in 0..choice {
            visual.simulate_keystrokes("down");
        }
        visual.run_until_parked();
        let lit = bounds_of(visual, "focus-ring");
        click(visual, lit.center());
        let after = checked_rows(visual, &state, caption);
        assert!(
            after[choice],
            "pressing choice {choice} in {owner}'s submenu: {after:?}"
        );
        assert_eq!(
            held(visual),
            (None, None, None),
            "the choice closed the submenu"
        );

        // A press on another display button, when the window draws one, moves
        // the submenu there.
        click(visual, button.center());
        let mut open = owner;
        if let Some(&other) = buttons.iter().find(|&&id| id != owner) {
            let at_other = bounds_of(visual, other).center();
            click(visual, at_other);
            assert_eq!(
                held(visual),
                (Some(AppMenu::View), Some(other), Some(caption_of(other))),
                "a press on {other} while {owner} held the submenu out"
            );
            assert_eq!(visual.debug_bounds(intern(&submenu)), None);
            open = other;
        }

        // Escape leaves the menu: no drop is left behind at the View title.
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert_eq!(
            held(visual),
            (None, None, None),
            "Escape in {open}'s submenu"
        );
        assert_eq!(visual.debug_bounds("menu-drop-View"), None);
    }
}
