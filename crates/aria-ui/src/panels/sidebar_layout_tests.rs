//! The Project panel's rows fit the panel, and its Media tab holds two
//! control rows.
//!
//! The classes closed here: a row of controls wider than the panel, drawn
//! past its edge or wrapped onto a stray line; and a control row added
//! between the filter row and the items, or under the items above the footer.
//! With media in the pool, at the launch layout's width, the groups have the
//! panel's whole text width to themselves, the count stands on the group
//! row's text edge, no text runs past the panel's edges, and below the groups
//! the Media tab draws its filter row, its items, and its footer, each one
//! line tall where it is a control row, with nothing else.
//!
//! Not covered: the glyphs, which the test text system does not raster. A
//! control drawn inside an item is the item's.

use gpui::{Bounds, FontWeight, Pixels, TestAppContext, VisualTestContext, px};

use crate::components::drawn::{self, Drawn};
use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::theme::Theme;
use crate::types::SidebarGroup;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the Project panel draws no {selector}"))
}

/// A drawn shell with one still imported into the pool.
fn pool_with_a_still<'a>(
    cx: &'a mut TestAppContext,
    tag: &str,
) -> (
    gpui::Entity<crate::app::AppState>,
    &'a mut VisualTestContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, tag);
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.import_media(still).expect("import the still");
            cx.notify();
        })
    });
    visual.run_until_parked();
    (state, visual)
}

/// Set the Project panel's column to `width` px of the 1920 px row, and draw
/// twice: the panel reads its width from the frame the previous draw laid
/// out, so the second draw is the one at `width`. Ends on a full draw, which
/// keeps every view's debug bounds and text runs.
fn project_width(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
    width: f32,
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let row = state
                .dock_tree
                .split_weights(&[0])
                .expect("the launch layout's upper area is a row split");
            let (_, rest) = row.split_first().expect("a row has children");
            let left = width / 1920.0;
            let total: f32 = rest.iter().sum();
            let weights: Vec<f32> = std::iter::once(left)
                .chain(rest.iter().map(|w| w / total * (1.0 - left)))
                .collect();
            assert!(state.dock_tree.set_weights(&[0], &weights));
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
}

#[gpui::test]
fn the_project_panel_rows_fit_the_panel(cx: &mut TestAppContext) {
    let (state, visual) = pool_with_a_still(cx, "sidebar-layout");

    let panel = bounds(visual, "sidebar");
    let groups: Vec<Bounds<Pixels>> = SidebarGroup::ALL
        .iter()
        .map(|group| bounds(visual, &format!("sidebar-group-{}", group.label())))
        .collect();
    // The group row is the panel's whole text width, stepped out by a chip's
    // padding on each side: nothing else stands on it, so the chips wrap only
    // when the panel is narrower than they are.
    let row = bounds(visual, "sidebar-groups");
    let edge = Theme::SPACE_MD - Theme::SPACE_SM;
    assert!(
        (row.left() - (panel.left() + px(edge))).abs() <= px(EPS)
            && row.right() >= panel.right() - px(1.0 + edge + EPS),
        "the group row {row:?} is not the panel's {panel:?} width"
    );
    for (group, chip) in SidebarGroup::ALL.iter().zip(&groups) {
        assert!(
            chip.left() >= row.left() - px(EPS) && chip.right() <= row.right() + px(EPS),
            "the {group:?} chip {chip:?} runs out of the group row {row:?}"
        );
    }
    let header = bounds(visual, "pool-header");
    let import = bounds(visual, "media-import");
    assert!(
        (import.right() - header.right()).abs() <= px(EPS),
        "Import {import:?} does not end the filter row {header:?}"
    );

    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let inside: Vec<_> = runs
        .iter()
        .filter(|run| panel.contains(&run.bounds.center()))
        .collect();
    let count = inside
        .iter()
        .find(|run| run.text.as_ref() == "1 item(s)")
        .expect("the pool's count is not drawn");
    // The first group chip's label and the count stand on one text edge: the
    // chip row steps out by its chips' padding.
    let first = SidebarGroup::ALL[0].label();
    let label = inside
        .iter()
        .find(|run| run.text.as_ref() == first && groups[0].contains(&run.bounds.center()))
        .expect("the first group's label is drawn");
    assert!(
        (label.bounds.left() - count.bounds.left()).abs() <= px(EPS),
        "`{}` starts at {:?}, the count at {:?}",
        label.text,
        label.bounds.left(),
        count.bounds.left()
    );
    for run in inside {
        assert!(
            run.bounds.left() >= panel.left() - px(EPS)
                && run.bounds.right() <= panel.right() + px(EPS),
            "`{}` at {:?} runs past the panel {panel:?}",
            run.text,
            run.bounds
        );
    }

    // Down to the panel's minimum width (160 px), the group row and the footer
    // each hold one row and never wrap onto a second line.
    project_width(&state, visual, 160.0);
    let one_row = px(Theme::ROW_HEIGHT + Theme::SPACE_XS + 2.0);
    for bar in ["sidebar-groups", "pool-header", "pool-footer"] {
        let at = bounds(visual, bar);
        assert!(
            at.size.height <= one_row,
            "at 160 px {bar} is {:?} tall, more than one row",
            at.size.height
        );
    }
}

/// Every group's label reads whole while the group row holds the labels at
/// the compact chip padding, at every Project panel width from 400 px down.
///
/// WHY: the row chose its padding by a fixed panel width, 260 px, so the
/// launch layout's 280 px panel kept the regular padding and cut four groups
/// to "Graphi…", "Effe…", "Libr…" and "Analy…" while the compact padding left
/// room for all five. The padding is now chosen by measuring the labels. The
/// walk must draw a width where the regular padding does not fit and the
/// compact one does, or it cannot tell the two rules apart. Does not cover a
/// row too narrow for the compact padding, where the inactive groups shrink
/// with an ellipsis by design.
#[gpui::test]
fn every_group_reads_whole_while_the_compact_padding_holds_the_row(cx: &mut TestAppContext) {
    let (state, visual) = pool_with_a_still(cx, "sidebar-groups-fit");
    let labels: Vec<(&'static str, f32)> = visual.update(|_, app| {
        let font = Theme::of(app).ui_font(FontWeight::NORMAL);
        SidebarGroup::ALL
            .iter()
            .map(|group| {
                let advance =
                    app.text_system()
                        .advance_for_text(group.label(), &font, Theme::TEXT_UI);
                (group.label(), f32::from(advance))
            })
            .collect()
    });
    let sum: f32 = labels.iter().map(|(_, advance)| advance).sum();
    let count = labels.len() as f32;
    // The compact padding on both sides of every chip, and the first chip's
    // wider left inset, which keeps its label on the panel's text edge.
    let compact = sum + count * 2.0 * Theme::SPACE_XS + (Theme::SPACE_SM - Theme::SPACE_XS);
    let regular = sum + count * 2.0 * Theme::SPACE_SM + (count - 1.0) * Theme::SPACE_XXS;
    let (mut held, mut decisive) = (0, 0);
    for width in (160..=400).rev().step_by(4) {
        project_width(&state, visual, width as f32);
        let row = f32::from(bounds(visual, "sidebar-groups").size.width);
        if row < compact + EPS {
            continue;
        }
        held += 1;
        if row < regular {
            decisive += 1;
        }
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        for (label, _) in &labels {
            let chip = bounds(visual, &format!("sidebar-group-{label}"));
            assert!(
                runs.iter()
                    .any(|run| run.text.as_ref() == *label && chip.contains(&run.bounds.center())),
                "in a {width} px panel the {row} px group row holds the labels at the \
                 compact padding ({compact} px), and the {label} chip {chip:?} does not \
                 read its label whole"
            );
        }
    }
    assert!(
        held > 0 && decisive > 0,
        "the walk drew {held} rows that hold the labels, {decisive} of them narrower than the \
         regular padding ({regular} px), so it cannot tell the padding rules apart"
    );
}

/// WHY: the Media tab once drew five control rows over its items (the tab
/// chips with Import, the filter, the folder strip, the count with the views,
/// the orders), so the pool started a third of the way down the panel. At
/// rest it draws the filter row above the items and the footer below them,
/// and between the group row and the panel's bottom nothing else: every text
/// run and image there is in one of the two rows or in an item. Each row is
/// one control tall, so a row that grows a second line fails too. Catches a
/// control row re-added anywhere in the tab, the tab chips drawn again, a row
/// that wraps at the launch width, and the footer moved above the items.
#[gpui::test]
fn at_rest_the_media_tab_draws_two_control_rows_around_its_items(cx: &mut TestAppContext) {
    let (state, visual) = pool_with_a_still(cx, "pool-rows");
    drawn::start();
    visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
    visual.run_until_parked();
    let boxes = drawn::take();

    let panel = bounds(visual, "sidebar");
    let groups = bounds(visual, "sidebar-groups");
    let header = bounds(visual, "pool-header");
    let items = bounds(visual, "pool-items");
    let footer = bounds(visual, "pool-footer");
    assert!(
        header.bottom() <= items.top() + px(EPS) && items.bottom() <= footer.top() + px(EPS),
        "the filter row {header:?}, the items {items:?} and the footer {footer:?} are out of order"
    );
    let one_control = px(Theme::INPUT_HEIGHT.max(Theme::ICON_HIT) + EPS);
    assert!(
        header.size.height <= one_control,
        "the filter row is {:?} tall, more than one control",
        header.size.height
    );
    assert!(
        footer.size.height <= one_control + px(Theme::SPACE_XS + 1.0),
        "the footer is {:?} tall, more than one control",
        footer.size.height
    );

    let rows = visual.read(|app| state.read(app).pool_rows(state.read(app).sidebar_tab).len());
    let mut held: Vec<Bounds<Pixels>> = (0..rows)
        .map(|index| bounds(visual, &format!("pool-item-{index}")))
        .collect();
    held.extend(visual.debug_bounds(intern("pool-properties")));
    let placed = |at: &Bounds<Pixels>| {
        let mid = at.center();
        if header.contains(&mid) || footer.contains(&mid) {
            return true;
        }
        items.contains(&mid) && held.iter().any(|item| item.contains(&mid))
    };
    let below = |at: &Bounds<Pixels>| {
        let mid = at.center();
        panel.contains(&mid) && mid.y > groups.bottom()
    };

    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let stray: Vec<String> = runs
        .iter()
        .filter(|run| !drawn::is_marker(run) && below(&run.bounds) && !placed(&run.bounds))
        .map(|run| format!("`{}` at {:?}", run.text, run.bounds))
        .chain(
            boxes
                .iter()
                .filter(|(kind, at)| *kind == Drawn::Image && below(at) && !placed(at))
                .map(|(_, at)| format!("an image at {at:?}")),
        )
        .collect();
    assert!(
        stray.is_empty(),
        "the Media tab draws outside its filter row, items and footer:\n{}",
        stray.join("\n")
    );
}
