//! Source tab contracts: a source opens once and keeps its marks across tab
//! switches, closing a tab moves the focus and the Shift+2 target to tabs
//! that still exist, and the split needs a Source tab to show.
//!
//! Not covered here: the tab row's drawing and pointer routing, which the
//! shell tests drive.

use super::*;

fn view(media: u64) -> SourceView {
    SourceView {
        media,
        name: format!("take {media}").into(),
        duration_ticks: 100,
        tick: 0,
        mark_in: None,
        mark_out: None,
        clip: None,
    }
}

fn names(tabs: &SourceTabs) -> Vec<u64> {
    tabs.tabs().iter().map(|tab| tab.media).collect()
}

#[test]
fn a_source_opens_one_tab_and_keeps_its_marks_across_tabs() {
    let mut tabs = SourceTabs::default();
    assert_eq!(tabs.open(view(1)), 0);
    tabs.focused_mut().expect("focused").mark_in = Some(12);
    assert_eq!(tabs.open(view(2)), 1);
    assert_eq!(tabs.focused().map(|tab| tab.media), Some(2));
    // Opening the first source again focuses its tab, marks intact, instead
    // of a second tab with fresh marks.
    assert_eq!(tabs.open(view(1)), 0);
    assert_eq!(names(&tabs), vec![1, 2]);
    assert_eq!(tabs.focused().and_then(|tab| tab.mark_in), Some(12));
}

#[test]
fn shift_two_swaps_program_and_the_tab_focused_last() {
    let mut tabs = SourceTabs::default();
    assert!(!tabs.swap_program_and_recent(), "nothing to return to");
    tabs.open(view(1));
    tabs.open(view(2));
    tabs.focus(0);
    assert!(tabs.swap_program_and_recent());
    assert!(tabs.focused().is_none(), "Program");
    assert!(tabs.swap_program_and_recent());
    assert_eq!(tabs.focused().map(|tab| tab.media), Some(1));
}

#[test]
fn closing_tabs_keeps_focus_and_recent_on_tabs_that_exist() {
    let mut tabs = SourceTabs::default();
    for media in 1..=4 {
        tabs.open(view(media));
    }
    tabs.focus(2);
    // A tab before the focused one: the focus follows its tab to the left.
    assert_eq!(tabs.close(0).map(|tab| tab.media), Some(1));
    assert_eq!(tabs.focused().map(|tab| tab.media), Some(3));
    assert_eq!(tabs.recent().map(|tab| tab.media), Some(3));
    // The focused tab: Program takes the focus, and the tab now in its
    // place is the one Shift+2 returns to.
    assert_eq!(tabs.close(1).map(|tab| tab.media), Some(3));
    assert!(tabs.focused().is_none());
    assert_eq!(tabs.recent().map(|tab| tab.media), Some(4));
    // The last tab of the row: the one before it takes over.
    tabs.focus(1);
    tabs.close(1);
    assert_eq!(tabs.recent().map(|tab| tab.media), Some(2));
    assert!(tabs.close(5).is_none(), "past the row");
    tabs.close(0);
    assert!(tabs.recent().is_none() && tabs.focused().is_none());
    assert!(tabs.tabs().is_empty());
}

#[test]
fn retain_closes_the_rejected_tabs_and_updates_the_rest() {
    let mut tabs = SourceTabs::default();
    for media in 1..=3 {
        tabs.open(view(media));
    }
    tabs.retain(|tab| {
        tab.duration_ticks = 50;
        tab.media != 2
    });
    assert_eq!(names(&tabs), vec![1, 3]);
    assert!(tabs.tabs().iter().all(|tab| tab.duration_ticks == 50));
    assert_eq!(tabs.focused().map(|tab| tab.media), Some(3));
}

#[test]
fn the_split_needs_a_source_and_ends_with_the_last_tab() {
    let mut tabs = SourceTabs::default();
    assert!(!tabs.toggle_split(), "no Source tab to show");
    assert!(!tabs.split());
    tabs.open(view(1));
    tabs.focus_program();
    assert!(tabs.toggle_split());
    // Program keeps the keys; the left pane shows the tab focused last.
    assert!(tabs.focused().is_none());
    assert_eq!(tabs.split_source().map(|tab| tab.media), Some(1));
    tabs.close(0);
    assert!(!tabs.split(), "nothing left to show on the left");
    assert!(tabs.split_source().is_none());
}
