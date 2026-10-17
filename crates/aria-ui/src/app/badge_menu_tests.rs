//! The menu the fx badge on a clip bar opens: what it lists, and what choosing
//! a row does to the line the bar draws.
//!
//! Closes the class "the badge lists a property the bar cannot draw, or a
//! choice the bar does not follow": a row for a picture property on an audio
//! lane, a block left out of the list (an effect's parameters, a mask's), a row
//! whose choice lands on another property, and the lane default stored as a
//! choice, which would pin a clip moved to the other kind of lane to the wrong
//! property. The rows are read from the menu, so a new kind of property is
//! covered when the catalog lists it. Not covered: drawing the rows, which the
//! census in `drawn_census_menu.rs` reaches through the same tables, and the
//! press on the painted badge, which `panels/timeline/clip/bar_tests.rs` owns.

use std::collections::BTreeSet;

use aria_models::{ClipParam, MaskKind, ParamScope};

use super::fixtures::{blurred_clip, with_audio};
use crate::surfaces::context_menu::{BADGE_ITEMS, BadgeMenu, MenuItem, draw_property};
use crate::types::ActivePanel;

fn menu(state: &super::AppState, clip: aria_models::ClipId) -> BadgeMenu {
    BadgeMenu::of(state, clip.raw()).expect("the clip is projected")
}

#[test]
fn every_property_the_badge_lists_becomes_the_line_its_bar_draws() {
    let (mut state, clip) = blurred_clip();
    state
        .add_selected_mask(MaskKind::Rectangle)
        .expect("the clip takes a mask");
    state.sync_from_engine();
    let opened = menu(&state, clip);
    assert!(!opened.audio);
    assert_eq!(opened.drawn, Some(ClipParam::Opacity));
    assert!(!opened.chosen);

    let blur = aria_models::effect_spec("gaussian_blur")
        .expect("the blur ships")
        .label
        .clone();
    let mut blocks = BTreeSet::new();
    for property in opened.properties.iter() {
        let (block, _) = property
            .label
            .split_once(": ")
            .unwrap_or_else(|| panic!("{} is not grouped by block", property.label));
        blocks.insert(block.to_owned());
        draw_property(&mut state, &opened, property.param).expect("the clip's own property");
        state.sync_from_engine();
        let after = menu(&state, clip);
        assert_eq!(after.drawn, Some(property.param), "{}", property.label);
        assert_eq!(
            after.chosen,
            property.param != ClipParam::Opacity,
            "{}: the lane default is stored as no choice",
            property.label
        );
    }
    // A media clip on a picture lane lists its sound too, as its property
    // rows do.
    assert_eq!(
        blocks,
        BTreeSet::from([
            "Transform".to_owned(),
            blur,
            "Mask 1".to_owned(),
            "Audio".to_owned()
        ])
    );
}

#[test]
fn a_badge_on_an_audio_lane_lists_no_picture_property() {
    let (state, clip) = with_audio();
    let opened = menu(&state, clip);
    assert!(opened.audio);
    assert_eq!(opened.drawn, Some(ClipParam::Volume));
    assert!(
        opened
            .properties
            .iter()
            .any(|property| property.param == ClipParam::Volume),
        "Volume is missing from {:?}",
        opened.properties
    );
    let engine = state.engine().lock();
    let model = engine
        .project()
        .clip(clip)
        .expect("the clip is in the project");
    for property in opened.properties.iter() {
        let row = model
            .line_row(property.param, true)
            .unwrap_or_else(|| panic!("{} has no row on an audio lane", property.label));
        assert_ne!(row.scope, ParamScope::Visual, "{}", property.label);
    }
}

#[test]
fn show_in_effect_controls_selects_the_clip_and_opens_the_panel() {
    let (mut state, clip) = blurred_clip();
    state.clear_selection();
    state.active_panel = ActivePanel::Timeline;
    let opened = menu(&state, clip);
    let row = BADGE_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .find(|row| row.id == "ctx-badge-controls")
        .expect("the row is in the table");
    (row.run)(&mut state, &opened).expect("the row runs");
    assert_eq!(state.selection, [clip]);
    assert_eq!(state.active_panel, ActivePanel::Inspector);
}
