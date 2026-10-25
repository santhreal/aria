//! Driving the label palette: every swatch, on every target it is drawn for.
//!
//! The palette is two blocks of coloured squares rather than a list of rows, so
//! the registry declares the write and the panel declares the picture. What is
//! measured here is the write: press every swatch on the selection and on a pool
//! row, and report the commands.

use std::collections::BTreeSet;

use aria_models::LabelColor;

use crate::app::premiere_tests::with_clip;
use crate::surfaces::labels::{self, LabelTarget};

/// Every command the label palette asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for target in LabelTarget::ALL {
        for swatch in labels::swatches() {
            let (mut state, target) = aimed(target);
            let watch = crate::app::sent_census::Watch::on(&state);
            target
                .write(&mut state, swatch)
                .expect("a label write is never refused on the fixture");
            out.extend(watch.take());
        }
    }
    let (mut state, _) = with_clip();
    let watch = crate::app::sent_census::Watch::on(&state);
    labels::select_group(&mut state, LabelColor::ALL[0]);
    out.extend(watch.take());
    out
}

/// A fresh fixture with `target` aimed at it: the pool palette at the item that
/// fixture imported, the clip palette at its selection.
///
/// Aimed per state rather than once, because a pool id belongs to the project it
/// was imported into and means nothing in the next one.
fn aimed(target: LabelTarget) -> (crate::app::AppState, LabelTarget) {
    let (state, _) = with_clip();
    let media = state
        .store
        .media
        .first()
        .expect("the fixture imports one pool item")
        .id;
    let target = target.on_media(media);
    (state, target)
}

#[test]
fn every_swatch_writes_a_label() {
    // A swatch that writes nothing is a coloured square: it draws, it closes the
    // menu, and the clip keeps whatever label it had.
    for target in LabelTarget::ALL {
        for swatch in labels::swatches() {
            let (mut state, target) = aimed(target);
            let watch = crate::app::sent_census::Watch::on(&state);
            target.write(&mut state, swatch).expect("the write lands");
            assert!(
                !watch.take().is_empty(),
                "the {} swatch for {:?} asked for nothing",
                target.id_prefix(),
                swatch
            );
        }
    }
}

#[test]
fn the_palette_offers_every_colour_and_a_way_out() {
    // Fail by default on a new colour: the palette is built from
    // `LabelColor::ALL`, so a colour added to the model appears here without an
    // edit, and a palette that stopped offering the clear would be a label an
    // operator cannot take off.
    let swatches: Vec<Option<LabelColor>> = labels::swatches().collect();
    assert_eq!(
        swatches.len(),
        LabelColor::ALL.len() + 1,
        "the palette is not one swatch per color plus a clear"
    );
    assert_eq!(
        swatches.last(),
        Some(&None),
        "the palette's last swatch is not the clear"
    );
    for color in LabelColor::ALL {
        assert!(
            swatches.contains(&Some(color)),
            "the palette does not offer {color:?}"
        );
    }
}

#[test]
fn a_clip_label_and_a_pool_label_are_written_by_different_targets() {
    // One palette, two writes. A target that wrote to the selection while drawn
    // on a pool row would label whatever happened to be selected, which is the
    // clip the pointer was not on.
    let (mut state, clip) = with_clip();
    let media = state
        .store
        .media
        .first()
        .expect("the fixture imports one pool item")
        .id;
    let color = LabelColor::ALL[0];

    LabelTarget::Media(media)
        .write(&mut state, Some(color))
        .expect("label the pool row");
    state.sync_from_engine();
    assert_eq!(
        state.media_label(media),
        Some(color),
        "the pool row kept no label"
    );
    assert_eq!(
        state.selected_label(),
        None,
        "labeling a pool row labeled the selection too"
    );

    LabelTarget::Selection
        .write(&mut state, Some(color))
        .expect("label the selection");
    state.sync_from_engine();
    assert_eq!(
        state.selected_label(),
        Some(color),
        "the selection kept no label"
    );
    assert!(state.selection.contains(&clip), "the selection moved");
}

#[test]
fn the_palette_is_drawn_from_the_registry() {
    // The census presses `LabelTarget::write`. That says nothing about the drawn
    // palette unless the drawn palette calls the same function over the same
    // list.
    //
    // Not covered: a swatch drawn with the wrong colour. The picture is the
    // panel's, and no census can see a fill.
    let drawn = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/panels/context_menu/label.rs"),
    )
    .expect("the drawn palette is readable");
    for line in ["labels::swatches()", "target.write(state, label)"] {
        assert!(
            drawn.contains(line),
            "the drawn palette has no `{line}`, so the census measures a write it does not make"
        );
    }
}
