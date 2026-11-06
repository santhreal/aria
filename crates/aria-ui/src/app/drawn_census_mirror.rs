//! The clip menu and the menu bar name and offer one command one way.
//!
//! Every clip menu row is listed below with the bar row whose command it runs,
//! or with none. A row with a bar twin draws the twin's label, is offered
//! exactly while the twin would run, and draws the twin's check mark, in every
//! state the clip census dresses. The twin is read from the registry the bar
//! draws, across every menu of [`AppMenu::ALL`]. A row with no twin draws words
//! no bar row draws, so a second name for a bar command cannot hide there.
//!
//! The class closed: a clip row that names a bar command in words of its own
//! ("Link Clips" beside the bar's "Link"), or that is offered against facts of
//! its own, which offered Link on a linked pair and Group on a group. A clip
//! row added without an entry here turns the suite red.
//!
//! Not caught: a twinned row whose `run` sends a different command than its
//! twin, and a row with no twin that names a bar command in other words.

use crate::chrome::menu_items::items_for;
use crate::surfaces::context_menu::{CLIP_ITEMS, MenuItem};
use crate::types::AppMenu;

use super::menu::CLIP_STAGES;

/// Each clip menu row and the bar row whose command it runs, in the order the
/// menu draws them.
const TWINS: [(&str, Option<&str>); 21] = [
    ("ctx-cut", Some("edit-cut")),
    ("ctx-copy", Some("edit-copy")),
    ("ctx-duplicate", Some("edit-dup")),
    ("ctx-delete", Some("edit-clear")),
    ("ctx-ripple-delete", Some("edit-ripple")),
    ("ctx-split", Some("seq-add-edit")),
    // Opens the frame-count bar, which no bar row opens.
    ("ctx-shift-selection", None),
    ("ctx-enable", Some("clip-enable")),
    ("ctx-link", Some("seq-link")),
    ("ctx-unlink", Some("seq-unlink")),
    // Acts on the clip under the pointer, which no bar row reads.
    ("ctx-move-into-sync", None),
    ("ctx-group", Some("clip-group")),
    ("ctx-ungroup", Some("clip-ungroup")),
    // Acts on the clip under the pointer, not on the selection.
    ("ctx-release-parent", None),
    ("ctx-speed-duration", Some("clip-speed-duration")),
    ("ctx-time-remap", Some("clip-time-remap")),
    ("ctx-freeze", Some("seq-freeze")),
    ("ctx-audio-gain", Some("clip-audio-gain")),
    ("ctx-extract-audio", Some("seq-extract-audio")),
    ("ctx-rename-clip", Some("clip-rename")),
    // Brings the Inspector forward, which no bar row does.
    ("ctx-properties", None),
];

#[test]
fn every_clip_row_is_listed_with_its_bar_twin() {
    let drawn: Vec<&str> = CLIP_ITEMS
        .iter()
        .filter_map(MenuItem::row)
        .map(|row| row.id)
        .collect();
    let listed: Vec<&str> = TWINS.iter().map(|(id, _)| *id).collect();
    assert_eq!(drawn, listed, "a clip menu row has no recorded bar twin");
}

#[test]
fn a_clip_row_draws_offers_and_checks_as_its_bar_twin() {
    for stage in CLIP_STAGES {
        let (state, facts) = (stage.dress)();
        let bar: Vec<_> = AppMenu::ALL
            .into_iter()
            .map(|menu| items_for(menu, &state))
            .collect();
        let bar_rows = || bar.iter().flat_map(|items| items.iter());
        for row in CLIP_ITEMS.iter().filter_map(MenuItem::row) {
            let what = stage.what;
            let id = row.id;
            let label = (row.label)(&facts);
            let twin = TWINS
                .iter()
                .find(|(listed, _)| *listed == id)
                .and_then(|(_, twin)| *twin);
            let Some(twin) = twin else {
                let same: Vec<&str> = bar_rows()
                    .filter(|item| item.label == label)
                    .map(|item| item.id)
                    .collect();
                assert!(
                    same.is_empty(),
                    "{what}: {id} draws {label:?}, which the bar rows {same:?} draw"
                );
                continue;
            };
            let bar_row = bar_rows()
                .find(|item| item.id == twin)
                .unwrap_or_else(|| panic!("no bar row {twin}"));
            assert_eq!(label, bar_row.label, "{what}: {id} names {twin} otherwise");
            assert_eq!(
                (row.when)(&facts),
                bar_row.blocked_by(&state).is_none(),
                "{what}: {id} is offered unlike {twin}"
            );
            assert_eq!(
                row.checked.is_some(),
                bar_row.checked.is_some(),
                "{what}: {id} and {twin} disagree on drawing a check mark"
            );
            assert_eq!(
                row.is_checked(&facts),
                bar_row.is_checked(&state),
                "{what}: {id} is checked unlike {twin}"
            );
        }
    }
}
