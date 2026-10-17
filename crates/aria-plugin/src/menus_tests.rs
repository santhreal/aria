//! Contracts for menu paths.
//!
//! Closes the class "a path is accepted that no menu draws": every location is
//! read back from its own label, and every malformed path is refused with the
//! reason. Drawing the rows is the app's, and its tests own it.

use super::*;

#[test]
fn every_location_reads_back_from_its_label_in_any_case() {
    for location in MenuLocation::ALL {
        assert_eq!(MenuLocation::parse(location.label()), Some(location));
        assert_eq!(
            MenuLocation::parse(&location.label().to_ascii_lowercase()),
            Some(location)
        );
        let path = MenuPath::parse(location.label()).expect("a bare location");
        assert_eq!(path.to_string(), location.label());
    }
}

#[test]
fn only_the_bar_menus_are_in_the_menu_bar() {
    let bar: Vec<&str> = MenuLocation::ALL
        .into_iter()
        .filter(|location| location.in_menu_bar())
        .map(MenuLocation::label)
        .collect();
    assert_eq!(
        bar,
        [
            "File",
            "Edit",
            "Sequence",
            "Markers",
            "Effect",
            "Animation",
            "View",
            "Window",
            "Help"
        ]
    );
}

#[test]
fn a_group_is_kept_and_labels_the_row() {
    let path = MenuPath::parse(" sequence / Color Tools ").expect("a grouped path");
    assert_eq!(
        path,
        MenuPath {
            location: MenuLocation::Sequence,
            group: Some("Color Tools".to_owned()),
        }
    );
    assert_eq!(path.to_string(), "Sequence/Color Tools");
    assert_eq!(
        path.row_label("Reset LUT Stack"),
        "Color Tools: Reset LUT Stack"
    );
    let bare = MenuPath::parse("Clip").expect("a bare path");
    assert_eq!(bare.row_label("Reset LUT Stack"), "Reset LUT Stack");
}

#[test]
fn a_malformed_path_is_refused_with_the_reason() {
    let cases = [
        (
            "Effects/Color Tools",
            "menu \"Effects/Color Tools\" names no menu; it starts with one of File, Edit, Sequence, Markers, Effect, Animation, View, Window, Help, Clip, Track, Media, Viewer, Transition, Effect Header, Clip Badge",
        ),
        (
            "",
            "menu \"\" names no menu; it starts with one of File, Edit, Sequence, Markers, Effect, Animation, View, Window, Help, Clip, Track, Media, Viewer, Transition, Effect Header, Clip Badge",
        ),
        ("Clip/", "menu \"Clip/\" has an empty group"),
        ("Clip/ ", "menu \"Clip/ \" has an empty group"),
        (
            "Sequence/Color/Advanced",
            "menu \"Sequence/Color/Advanced\" nests deeper than one group, such as `Sequence/Color Tools`",
        ),
    ];
    for (text, reason) in cases {
        assert_eq!(MenuPath::parse(text), Err(reason.to_owned()), "{text:?}");
    }
}
