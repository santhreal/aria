//! Every drop on the menu bar stands on one row grid.
//!
//! The class closed here: a menu row whose height, caption or shortcut moves
//! with its content. A row with a check mark, a plugin icon, a submenu arrow or
//! a long caption drew its caption or its chord at another x than its
//! neighbours, so the column the eye runs down broke.
//!
//! Each menu of [`AppMenu::ALL`] is opened in a drawn shell. Its rows must be
//! one row height tall at one pitch, its captions must start at one x, and its
//! shortcuts must end at one x inside the row's padding.
//!
//! A plugin row's icon is drawn in the lead column, the column a toggle row
//! draws its check mark in: a command with an icon is placed in every menu and
//! its icon must stand in that column while its caption keeps the menu's.
//!
//! Not covered: flyouts, which open one at a time and share the row; an icon
//! in a context menu; and text past the window's bottom.

use aria_plugin::{CommandIcon, CommandSpec, MenuCommand, MenuPath, PluginContext};
use gpui::{Bounds, Pixels, TestAppContext, px};

use crate::components::drawn::{self, Drawn};

use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::types::AppMenu;

const EPS: f32 = 0.51;

#[gpui::test]
fn every_menu_stands_on_one_row_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-grid");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut faults = Vec::new();
    for menu in AppMenu::ALL {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.close_menu();
                state.toggle_menu(menu);
                cx.notify();
            });
        });
        visual.run_until_parked();
        let entries: Vec<(&'static str, String, Option<String>)> = visual.read(|app| {
            super::drawn_entries(menu, state.read(app))
                .into_iter()
                .map(|entry| (entry.id, entry.caption, entry.chord))
                .collect()
        });
        assert!(!entries.is_empty(), "{menu:?} draws no rows");
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        let mut rows: Vec<Bounds<Pixels>> = Vec::new();
        let mut captions = Vec::new();
        let mut chords = Vec::new();
        for (id, caption, chord) in &entries {
            let row = visual
                .debug_bounds(crate::interned::intern(&format!("menu-row-{id}")))
                .unwrap_or_else(|| panic!("{menu:?}: row {id} was not drawn"));
            let in_row = |text: &str| {
                runs.iter()
                    .find(|run| row.contains(&run.bounds.center()) && run.text.as_ref() == text)
                    .map(|run| run.bounds)
            };
            match in_row(caption) {
                Some(at) => captions.push((caption.clone(), at.left())),
                None => faults.push(format!("{menu:?}: `{caption}` drew no caption in its row")),
            }
            if let Some(chord) = chord {
                match in_row(chord) {
                    Some(at) => chords.push((caption.clone(), at.right(), row.right())),
                    None => faults.push(format!("{menu:?}: `{caption}` drew no `{chord}`")),
                }
            }
            rows.push(row);
        }
        for (row, (_, caption, _)) in rows.iter().zip(&entries) {
            if (row.size.height - px(Theme::ROW_HEIGHT)).abs() > px(EPS) {
                faults.push(format!(
                    "{menu:?}: `{caption}` is {:?} tall",
                    row.size.height
                ));
            }
            if (row.left() - rows[0].left()).abs() > px(EPS)
                || (row.right() - rows[0].right()).abs() > px(EPS)
            {
                faults.push(format!("{menu:?}: `{caption}` spans {row:?}"));
            }
        }
        for pair in rows.windows(2) {
            let pitch = pair[1].top() - pair[0].top();
            if (pitch - px(Theme::ROW_HEIGHT)).abs() > px(EPS) {
                faults.push(format!("{menu:?}: a row pitch of {pitch:?}"));
            }
        }
        if let Some((_, first)) = captions.first() {
            for (caption, left) in &captions {
                if (*left - *first).abs() > px(EPS) {
                    faults.push(format!(
                        "{menu:?}: `{caption}` starts at {left:?}, the column at {first:?}"
                    ));
                }
            }
        }
        // The chord's box is laid out on whole pixels and its text measured in
        // fractions of one, so the run may end up to a pixel short of the box.
        for (caption, right, row_right) in &chords {
            let column = *row_right - px(1.0 + Theme::SPACE_SM);
            if *right > column + px(EPS) || *right < column - px(1.0) {
                faults.push(format!(
                    "{menu:?}: `{caption}`'s shortcut ends at {right:?}, the column at {column:?}"
                ));
            }
        }
    }
    assert!(
        faults.is_empty(),
        "menu rows off the grid:\n{}",
        faults.join("\n")
    );
}

#[gpui::test]
fn a_plugin_row_draws_its_icon_in_the_lead_column(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "menu-lead");
    visual.update(|_, app| app.set_reduce_motion(true));
    let ctx = visual.read(|app| state.read(app).ctx.clone());
    let mut spec = CommandSpec::new("lead.icon", "Lead Icon");
    spec.icon = Some(
        CommandIcon::from_svg(
            br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><rect width="16" height="16"/></svg>"#,
        )
        .expect("a square is an icon"),
    );
    let _command = ctx
        .register_command(spec, |_: &PluginContext| Ok(()))
        .expect("the command registers");
    // Icons drawn by the panels underneath the drop when no menu is open: a
    // tall menu such as Sequence reaches the Timeline header's toggle row.
    drawn::start();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let underneath = drawn::take();
    let mut faults = Vec::new();
    for menu in AppMenu::ALL {
        let location = super::super::plugin_menus::location_of(menu).label();
        let _row = ctx
            .register_menu_entry(MenuCommand {
                command: "lead.icon".into(),
                path: MenuPath::parse(location).expect("a menu path"),
            })
            .expect("the menu entry registers");
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.close_menu();
                state.toggle_menu(menu);
                cx.notify();
            });
        });
        visual.run_until_parked();
        drawn::start();
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
        let images = drawn::take();
        let entries: Vec<(&'static str, String, Option<bool>)> = visual.read(|app| {
            super::drawn_entries(menu, state.read(app))
                .into_iter()
                .filter(|entry| !entry.sub)
                .map(|entry| (entry.id, entry.caption, entry.checked))
                .collect()
        });
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        let mut row_of = |id: &str| {
            visual
                .debug_bounds(crate::interned::intern(&format!("menu-row-{id}")))
                .unwrap_or_else(|| panic!("{menu:?}: row {id} was not drawn"))
        };
        let run_in = |row: &Bounds<Pixels>, text: &str| {
            runs.iter()
                .find(|run| row.contains(&run.bounds.center()) && run.text.as_ref() == text)
                .map(|run| run.bounds)
        };
        let plugin = row_of("lead.icon");
        // The lead column starts inside the row's 1 px border and its padding.
        let column = plugin.left() + px(1.0 + Theme::SPACE_SM);
        let icons: Vec<Bounds<Pixels>> = images
            .iter()
            .filter(|(kind, at)| {
                *kind == Drawn::Image
                    && !underneath.contains(&(*kind, *at))
                    && plugin.contains(&at.center())
            })
            .map(|(_, at)| *at)
            .collect();
        match icons.as_slice() {
            [icon] => {
                if (icon.left() - column).abs() > px(EPS)
                    || (icon.size.width - px(Theme::ICON_SIZE)).abs() > px(EPS)
                {
                    faults.push(format!(
                        "{menu:?}: the icon spans {:?}..{:?}, the lead column {column:?}..{:?}",
                        icon.left(),
                        icon.right(),
                        column + px(Theme::ICON_SIZE)
                    ));
                }
            }
            drawn => faults.push(format!(
                "{menu:?}: the plugin row drew {} icons, not one",
                drawn.len()
            )),
        }
        // Every caption, the plugin row's among them, starts one gap past the
        // lead column, and every check mark is centred in it.
        let caption_x = column + px(Theme::ICON_SIZE + Theme::SPACE_SM);
        for (id, caption, checked) in &entries {
            let row = row_of(id);
            match run_in(&row, caption) {
                Some(at) if (at.left() - caption_x).abs() > px(EPS) => faults.push(format!(
                    "{menu:?}: `{caption}` starts at {:?}, the caption column at {caption_x:?}",
                    at.left()
                )),
                Some(_) => {}
                None => faults.push(format!("{menu:?}: `{caption}` drew no caption")),
            }
            if *checked == Some(true) {
                let centre = column + px(Theme::ICON_SIZE / 2.0);
                match run_in(&row, "✓") {
                    Some(at) if (at.center().x - centre).abs() > px(EPS) => faults.push(format!(
                        "{menu:?}: `{caption}`'s check is centered at {:?}, the column at {centre:?}",
                        at.center().x
                    )),
                    Some(_) => {}
                    None => faults.push(format!("{menu:?}: `{caption}` drew no check mark")),
                }
            }
        }
    }
    assert!(
        faults.is_empty(),
        "plugin icons off the lead column:\n{}",
        faults.join("\n")
    );
}
