//! Label colours: the saved names, the names files from the eight-colour set
//! load as, and the two fills.
//!
//! Closes the class "a label that does not survive a save": every label is
//! round-tripped through JSON by its own name, every marker colour a file may
//! hold as a label loads as the label the alias names, and an unknown name is
//! rejected rather than read as a default. Not caught: whether two hues read as
//! distinct on screen, which no arithmetic settles.

use super::*;

#[test]
fn every_label_saves_and_loads_by_its_own_name() {
    for label in LabelColor::ALL {
        let saved = serde_json::to_string(&label).expect("save");
        assert_eq!(saved, format!("\"{}\"", label.id()));
        let loaded: LabelColor = serde_json::from_str(&saved).expect("load");
        assert_eq!(loaded, label);
        assert_eq!(LabelColor::from_name(label.name()), Some(label));
        assert_eq!(LabelColor::from_name(label.id()), Some(label));
        assert_eq!(label.name().to_ascii_lowercase(), label.id());
    }
}

#[test]
fn the_sixteen_labels_are_distinct() {
    let ids: std::collections::BTreeSet<_> = LabelColor::ALL.iter().map(|l| l.id()).collect();
    assert_eq!(ids.len(), 16);
    let fills: std::collections::BTreeSet<_> = LabelColor::ALL.iter().map(|l| l.rgb()).collect();
    assert_eq!(fills.len(), 16, "two labels share a fill");
}

#[test]
fn a_label_saved_as_a_marker_colour_loads_as_the_label_of_that_name() {
    for marker in MarkerColor::ALL {
        let saved = serde_json::to_string(&marker).expect("save");
        let loaded: LabelColor = serde_json::from_str(&saved).expect("an old label loads");
        assert_eq!(loaded, LabelColor::from(marker), "{marker:?}");
        assert_eq!(LabelColor::from_name(marker.name()), Some(loaded));
    }
    assert_eq!(LabelColor::from(MarkerColor::Pink), LabelColor::Magenta);
    assert_eq!(LabelColor::from(MarkerColor::Red), LabelColor::Rose);
    assert_eq!(LabelColor::from(MarkerColor::Orange), LabelColor::Mango);
}

#[test]
fn an_unknown_name_is_rejected() {
    assert!(serde_json::from_str::<LabelColor>("\"chartreuse\"").is_err());
    assert_eq!(LabelColor::from_name("chartreuse"), None);
    assert_eq!(LabelColor::from_name(""), None);
}

#[test]
fn the_selected_fill_is_lighter_on_every_channel_short_of_white() {
    for label in LabelColor::ALL {
        let (fill, selected) = (label.rgb(), label.selected_rgb());
        for (base, lit) in fill.into_iter().zip(selected) {
            assert!(lit >= base, "{label:?}");
            assert_eq!(lit, base + (255 - base) / 3, "{label:?}");
        }
        assert_ne!(fill, selected, "{label:?} draws selected as unselected");
    }
}
