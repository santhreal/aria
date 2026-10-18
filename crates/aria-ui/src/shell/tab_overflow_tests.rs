//! A tab row draws whole tabs that read their titles, and lists the rest.
//!
//! The class closed here: a frame holding more tabs than its width fits,
//! drawing each one as ".", ".." or "..." and the active one possibly not at
//! all. The panels are read from the registry when the test runs, so a panel
//! added to the app is in every row here without an edit. For every tab count
//! from one to the registry's size, the frame holds that many panels, the
//! narrowest minimums first, and is drawn beside a second frame in a window
//! of the Editing layout's width (a narrower window draws the small-window
//! layout, which has no tab rows) at every width from its minimum to 400 px
//! in 20 px steps, and in 1 px steps at the widths around the one where every
//! tab sits at its narrowest, with its overflow menu open and a
//! different tab active from one width to the next. Each drawn tab must stand inside the
//! frame and read its title's first three letters and an ellipsis, or its
//! whole title, end before the row's controls, and the active tab's close
//! must be whole or gone; the active
//! tab must be drawn; and the drawn tabs and the
//! overflow menu's rows must hold the frame's tabs once each. The reference
//! case, the Editing layout's left frame with five panels at 320 px and
//! 160 px, is pressed through: the control opens the menu, and a row picked
//! there becomes the active, drawn tab.
//!
//! Not covered: a floating window's tab row, which draws in a window of its
//! own, and a frame between the 20 px steps.

use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::dock::{
    Axis, DockTree, Frame, FrameId, Node, PANEL_HISTORY, PANEL_INFO, PANEL_INSPECTOR,
    PANEL_MARKERS, PANEL_PROJECT, PanelId, Zone,
};
use crate::interned::intern;
use crate::test_support::drawn_shell;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;
/// The widest frame the sweep draws.
const MAX_W: f32 = 400.0;
/// The sweep's width step.
const STEP: f32 = 20.0;
/// Letters of its title a truncated tab reads.
const LEAD: usize = 3;
/// The window the Editing layout is built for.
const WINDOW_W: f32 = 1920.0;
/// A split's weights cover the splitters too, so a frame lands within one
/// splitter's width of the width asked for.
const SPLITTER_SLACK: f32 = 8.0;

/// Draw the shell twice: the first draw records each frame's width, which the
/// tab row fits its tabs to.
pub(super) fn redraw(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) {
    for _ in 0..2 {
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
    }
}

pub(super) fn frame_bounds(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    frame: FrameId,
) -> Bounds<Pixels> {
    let [x, y, w, h] = visual
        .read(|app| state.read(app).dock_frame_rects.lock().get(&frame).copied())
        .unwrap_or_else(|| panic!("frame {} was not drawn", frame.0));
    Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
}

/// Open the overflow menu of `frame_id` through its control, when the row
/// draws one and its menu is not open already.
fn open_overflow(visual: &mut VisualTestContext, frame_id: FrameId, what: &str) {
    if visual.debug_bounds("dock-tab-more-card").is_some() {
        return;
    }
    let button = intern(&format!("dock-tab-more-button-{}", frame_id.0));
    let Some(button) = visual.debug_bounds(button) else {
        return;
    };
    visual.simulate_click(button.center(), Modifiers::none());
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("dock-tab-more-card").is_some(),
        "{what}: pressing the overflow control opened no menu"
    );
}

/// Check the tab row of `frame`, drawn with its overflow menu open, and return
/// the tabs it draws and the tabs its menu lists.
fn check_row(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    frame_id: FrameId,
    what: &str,
) -> (Vec<PanelId>, Vec<PanelId>) {
    let frame = frame_bounds(state, visual, frame_id);
    let (tabs, active) = visual.read(|app| {
        let tree = &state.read(app).dock_tree;
        let frame = tree.frame(frame_id).expect("the frame is in the tree");
        (frame.tabs.clone(), frame.active_panel())
    });
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let (mut drawn, mut listed) = (Vec::new(), Vec::new());
    for &panel in &tabs {
        let tab = visual.debug_bounds(intern(&format!("dock-tab-{panel}")));
        let row = visual.debug_bounds(intern(&format!("dock-tab-more-{panel}")));
        assert!(
            tab.is_some() != row.is_some(),
            "{what}: {panel} is {} the tab row and {} the overflow menu",
            if tab.is_some() { "in" } else { "not in" },
            if row.is_some() { "in" } else { "not in" },
        );
        let Some(tab) = tab else {
            listed.push(panel);
            continue;
        };
        drawn.push(panel);
        assert!(
            tab.left() >= frame.left() - px(EPS) && tab.right() <= frame.right() + px(EPS),
            "{what}: the {panel} tab {tab:?} runs out of the frame {frame:?}"
        );
        let title = visual.read(|app| state.read(app).dock_panel_title(panel));
        let lead: String = title.chars().take(LEAD).collect();
        let text = runs
            .iter()
            .find(|run| tab.contains(&run.bounds.center()))
            .map(|run| run.text.to_string())
            .unwrap_or_else(|| panic!("{what}: the {panel} tab draws no title"));
        assert!(
            text == title || (text.ends_with('…') && text.starts_with(&lead)),
            "{what}: the {panel} tab reads `{text}`, not `{title}` or `{lead}…`"
        );
        // A close is drawn whole or not at all, never a sliver of its glyph.
        if panel == active {
            let close = visual
                .debug_bounds(intern(&format!("dock-tab-close-{panel}")))
                .unwrap_or_else(|| panic!("{what}: the active {panel} tab draws no close"));
            let w = f32::from(close.size.width);
            assert!(
                w <= EPS || (w - super::DOCK_TAB_CLOSE).abs() <= EPS,
                "{what}: the active {panel} tab's close is {w} px wide, neither whole nor gone"
            );
        }
    }
    assert!(
        drawn.contains(&active),
        "{what}: the active tab {active} is not drawn; the row draws {drawn:?}"
    );
    let button = visual.debug_bounds(intern(&format!("dock-tab-more-button-{}", frame_id.0)));
    assert_eq!(
        button.is_some(),
        !listed.is_empty(),
        "{what}: the overflow control is {} with {listed:?} not drawn",
        if button.is_some() { "drawn" } else { "missing" },
    );
    if let Some(button) = button {
        assert!(
            button.right() <= frame.right() + px(EPS),
            "{what}: the overflow control {button:?} runs out of the frame {frame:?}"
        );
    }
    // The tabs end where the row's controls start: a tab that runs under the
    // overflow control or the menu button is cut off there.
    let menu = visual
        .debug_bounds(intern(&format!("dock-frame-menu-{}", frame_id.0)))
        .unwrap_or_else(|| panic!("{what}: the frame draws no menu button"));
    let controls = button.map_or(menu.left(), |button| button.left());
    for &panel in &drawn {
        let tab = visual
            .debug_bounds(intern(&format!("dock-tab-{panel}")))
            .expect("a drawn tab has bounds");
        assert!(
            tab.right() <= controls + px(EPS),
            "{what}: the {panel} tab {tab:?} runs under the controls from {controls:?}"
        );
    }
    (drawn, listed)
}

#[gpui::test]
fn every_tab_row_draws_readable_tabs_and_lists_the_rest(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "tab-overflow");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut panels = visual.read(|app| state.read(app).registered_panel_ids());
    assert!(panels.len() > 2, "the ui plugin registers {panels:?}");
    // Narrowest minimum first, so most counts can be drawn narrow.
    let min_w = |panel: PanelId, visual: &mut VisualTestContext| {
        visual.read(|app| state.read(app).dock_panel_min_size(panel).0)
    };
    let mut keyed: Vec<(f32, PanelId)> = panels.iter().map(|&p| (min_w(p, visual), p)).collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    panels = keyed.iter().map(|&(_, p)| p).collect();
    let version = DockTree::editing().version;
    let widest = *panels.last().expect("the registry is not empty");
    // The frame under test is the right one, drawn last, so a selector the
    // left frame's tab row shares with it reads the right frame's tab. The
    // left frame holds a panel the right one does not, or, when the right
    // one holds every panel, its active panel.
    let two_frames = |tabs: &[PanelId], active: usize, share: f32| {
        let filler = if tabs.contains(&widest) {
            tabs[active]
        } else {
            widest
        };
        DockTree {
            version,
            root: Node::Split {
                axis: Axis::Row,
                children: vec![
                    (
                        1.0 - share,
                        Node::Frame(Frame::new(FrameId::LEFT, vec![filler])),
                    ),
                    (
                        share,
                        Node::Frame(Frame {
                            id: FrameId::RIGHT,
                            tabs: tabs.to_vec(),
                            active,
                        }),
                    ),
                ],
            },
            floating: Vec::new(),
            focused: FrameId::RIGHT,
            maximized: None,
        }
    };
    visual.simulate_resize(size(px(WINDOW_W), px(1080.0)));

    // The frame width at which every one of `tabs` is drawn at its least
    // width: their least widths, the menu button and the frame's border.
    let tight = |tabs: &[PanelId], visual: &mut VisualTestContext| -> f32 {
        visual.update(|_, cx| {
            let cx: &gpui::App = cx;
            let font = super::tab_fit::tab_font(&crate::theme::Theme::of(cx));
            let at = |pad: f32, gap: f32| {
                let read = state.read(cx);
                let least: f32 = tabs
                    .iter()
                    .map(|&p| {
                        super::tab_fit::least_width(read.dock_panel_title(p), pad, gap, &font, cx)
                    })
                    .sum();
                least + super::tab_fit::label_width("≡", &font, cx) + 2.0 * pad + 2.0
            };
            let wide = at(crate::theme::Theme::SPACE_SM, crate::theme::Theme::SPACE_XS);
            if wide < super::DOCK_TAB_COMPACT_W {
                at(crate::theme::Theme::SPACE_XS, 0.0)
            } else {
                wide
            }
        })
    };

    let mut cases = 0;
    for count in 1..=panels.len() {
        let tabs = &panels[..count];
        let frame_min = keyed[..count].iter().map(|&(w, _)| w).fold(0.0, f32::max);
        let squeeze = tight(tabs, visual).ceil();
        let mut widths: Vec<f32> = (0..)
            .map(|step| frame_min + STEP * step as f32)
            .take_while(|&w| w <= MAX_W)
            .chain((-3..6).map(|over| squeeze + over as f32))
            .filter(|&w| w >= frame_min && w <= MAX_W)
            .collect();
        widths.sort_by(f32::total_cmp);
        widths.dedup();
        for (step, width) in widths.into_iter().enumerate() {
            let active = (step * 5 + count) % count;
            // The row's splitter and borders take a few pixels, so the share
            // is corrected by what the frame drew.
            let mut share = width / WINDOW_W;
            let mut drawn_w = 0.0;
            for _ in 0..3 {
                visual.update(|_, app| {
                    state.update(app, |state, cx| {
                        state.dock_tree = two_frames(tabs, active, share);
                        cx.notify();
                    })
                });
                redraw(&state, visual);
                drawn_w = f32::from(frame_bounds(&state, visual, FrameId::RIGHT).size.width);
                if (drawn_w - width).abs() <= EPS {
                    break;
                }
                share *= width / drawn_w;
            }
            assert!(
                (drawn_w - width).abs() <= EPS,
                "{count} tabs: the frame drew {drawn_w} px wide, not {width}"
            );
            let what = format!("{count} tabs in a {width} px frame, tab {active} active");
            open_overflow(visual, FrameId::RIGHT, &what);
            check_row(&state, visual, FrameId::RIGHT, &what);
            cases += 1;
        }
    }
    assert!(cases >= panels.len(), "the sweep drew {cases} rows");
}

#[gpui::test]
fn the_overflow_control_lists_the_tabs_a_narrow_frame_has_no_room_for(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "tab-overflow-press");
    visual.update(|_, app| app.set_reduce_motion(true));
    for width in [320.0, 160.0] {
        let what = format!("the Editing left frame at {width} px");
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                let mut tree = DockTree::editing_for_width(WINDOW_W);
                for panel in [PANEL_HISTORY, PANEL_INFO, PANEL_MARKERS] {
                    tree.dock(panel, FrameId::LEFT, Zone::Center);
                }
                let row = tree
                    .split_weights(&[0])
                    .expect("the Editing upper area is a row split");
                let left = width / WINDOW_W;
                let rest: f32 = row[1..].iter().sum();
                let weights: Vec<f32> = std::iter::once(left)
                    .chain(row[1..].iter().map(|w| w / rest * (1.0 - left)))
                    .collect();
                assert!(tree.set_weights(&[0], &weights), "{what}: no weights");
                // The last tab, which a narrow row drops first, is active.
                assert!(tree.activate(PANEL_MARKERS));
                state.dock_tree = tree;
                cx.notify();
            })
        });
        redraw(&state, visual);
        let frame = frame_bounds(&state, visual, FrameId::LEFT);
        assert!(
            (f32::from(frame.size.width) - width).abs() <= SPLITTER_SLACK,
            "{what}: the frame drew {:?} wide",
            frame.size.width
        );
        let tabs = visual.read(|app| {
            state
                .read(app)
                .dock_tree
                .frame(FrameId::LEFT)
                .map(|f| f.tabs.clone())
        });
        assert_eq!(
            tabs,
            Some(vec![
                PANEL_PROJECT,
                PANEL_INSPECTOR,
                PANEL_HISTORY,
                PANEL_INFO,
                PANEL_MARKERS
            ]),
            "{what}: the frame holds other tabs"
        );
        let Some(button) =
            visual.debug_bounds(intern(&format!("dock-tab-more-button-{}", FrameId::LEFT.0)))
        else {
            assert!(width > 160.0, "{what}: five tabs fit at 160 px");
            continue;
        };
        assert!(
            button.right() <= frame.right() + px(EPS),
            "{what}: the overflow control {button:?} runs out of the frame"
        );
        open_overflow(visual, FrameId::LEFT, &what);
        let (drawn, listed) = check_row(&state, visual, FrameId::LEFT, &what);
        assert!(
            drawn.contains(&PANEL_MARKERS),
            "{what}: the active tab is not drawn"
        );
        let picked = listed[0];
        let row = visual
            .debug_bounds(intern(&format!("dock-tab-more-{picked}")))
            .expect("a listed tab has a row");
        visual.simulate_click(row.center(), Modifiers::none());
        redraw(&state, visual);
        let active = visual.read(|app| {
            state
                .read(app)
                .dock_tree
                .frame(FrameId::LEFT)
                .map(|f| f.active_panel())
        });
        assert_eq!(
            active,
            Some(picked),
            "{what}: picking {picked} left it inactive"
        );
        assert!(
            visual.debug_bounds("dock-tab-more-card").is_none(),
            "{what}: the menu stayed open after a pick"
        );
        assert!(
            visual
                .debug_bounds(intern(&format!("dock-tab-{picked}")))
                .is_some(),
            "{what}: the picked tab {picked} is not drawn"
        );
    }
}
