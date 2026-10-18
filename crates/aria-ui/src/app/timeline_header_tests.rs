//! The timeline header's controls, pressed in the drawn shell.
//!
//! The class closed here: a header button drawn with no command behind it or
//! with a sibling's command, and a property search that takes no keys, lets a
//! bound key through while it holds them, or keeps them after a press
//! elsewhere. Each press is read back as the whole tuple of header state, so a
//! button that moves a second setting turns the test red too.
//!
//! Not covered: the toggles' lit look, which a snapshot owns, and the
//! timecode, which `viewer_scrub_tests` drives.

use gpui::{Modifiers, MouseButton, VisualTestContext};

use crate::app::AppState;
use crate::app::viewer_row_tests::shell_with_take;
use crate::interned::intern;

fn press(visual: &mut VisualTestContext, selector: &'static str) {
    let at = visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the timeline header draws {selector}"))
        .center();
    visual.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

/// Snapping, linked selection, the graph editor, the marker count, and the
/// display settings menu.
type Header = (bool, bool, bool, usize, bool);

fn header(visual: &mut VisualTestContext, state: &gpui::Entity<AppState>) -> Header {
    visual.read(|app| {
        let state = state.read(app);
        (
            state.snap_enabled,
            state.linked_selection,
            state.timeline_graph_open(),
            state.store.sequence.markers.len(),
            state.display_menu() == Some(crate::chrome::menu_items::TIMELINE_DISPLAY),
        )
    })
}

#[gpui::test]
fn each_header_button_runs_its_own_command(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "timeline-header-buttons");
    let mut want = header(visual, &state);
    assert!(!want.4, "the display settings menu starts shut");
    // The display settings button goes last: the menu it opens stays out.
    let presses: [(&str, fn(&mut Header)); 5] = [
        ("timeline-snap", |want| want.0 = !want.0),
        ("timeline-linked", |want| want.1 = !want.1),
        ("timeline-graph", |want| want.2 = !want.2),
        ("timeline-marker", |want| want.3 += 1),
        ("timeline-display-settings", |want| want.4 = true),
    ];
    for (selector, effect) in presses {
        press(visual, selector);
        effect(&mut want);
        assert_eq!(header(visual, &state), want, "after pressing {selector}");
    }
}

#[gpui::test]
fn the_property_search_holds_the_keys_until_a_press_elsewhere(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "timeline-header-search");
    let read = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let state = state.read(app);
            (
                state.property_search_focused(),
                state.property_query().to_owned(),
            )
        })
    };
    let before = header(visual, &state);

    press(visual, "property-search");
    assert_eq!(
        read(visual),
        (true, String::new()),
        "the press opened the search"
    );
    // `s` and `m` are snapping and add marker outside the box.
    visual.simulate_keystrokes("p o s m");
    assert_eq!(read(visual), (true, "posm".to_owned()));
    visual.simulate_keystrokes("backspace");
    assert_eq!(read(visual), (true, "pos".to_owned()));
    assert_eq!(
        header(visual, &state),
        before,
        "a key typed into the search ran a command"
    );

    // A press elsewhere gives the keys back and keeps the filter.
    press(visual, "timeline-ruler");
    assert_eq!(read(visual), (false, "pos".to_owned()));
    visual.simulate_keystrokes("s");
    assert_eq!(read(visual), (false, "pos".to_owned()));
    assert_ne!(
        header(visual, &state).0,
        before.0,
        "S toggles snapping once the search lets go"
    );

    press(visual, "property-search-clear");
    assert_eq!(
        read(visual),
        (false, String::new()),
        "the clear button emptied it"
    );
}
