//! Driving the pool's drops: every gesture that files something, on a project
//! dressed with a folder and a source.
//!
//! A drop has no button, so the panels build a payload and one function says what
//! it means. This census builds the same payloads and calls the same function,
//! and reads the gesture list back out of `Dropped::what` so a payload nobody
//! dropped is a failure rather than a gap.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::sent_census::Watch;
use crate::surfaces::pool_drop::{self, Dropped};

/// A dressed pool: one source, one folder, and a second folder beside it, which
/// is the smallest project every gesture below has somewhere to land in.
struct Pool {
    state: AppState,
    media: u64,
    first: u64,
    second: u64,
}

fn dressed() -> Pool {
    let (mut state, _) = with_clip();
    let media = state
        .store
        .media
        .first()
        .expect("the fixture imports one source")
        .id;
    state.create_bin().expect("the first folder lands");
    state.cancel_bin_rename();
    let first = state.current_bin.expect("the first folder is open");
    // Back to the root, so the second folder is a sibling rather than a child.
    state.open_bin(None);
    state.create_bin().expect("the second folder lands");
    state.cancel_bin_rename();
    let second = state.current_bin.expect("the second folder is open");
    state.open_bin(None);
    assert_ne!(first, second, "the two folders are the same folder");
    Pool {
        state,
        media,
        first,
        second,
    }
}

/// Every gesture the pool accepts, built against a dressed project.
///
/// One per arm of `Dropped::what`; `every_gesture_the_pool_accepts_is_dropped`
/// holds this list to that match.
fn gestures(pool: &Pool) -> Vec<Dropped> {
    vec![
        Dropped::MediaOnBin {
            media: pool.media,
            bin: Some(pool.first),
        },
        Dropped::BinOnBin {
            bin: pool.second,
            parent: Some(pool.first),
        },
        Dropped::MediaAtSpot {
            media: pool.media,
            x: 40.0,
            y: 60.0,
        },
    ]
}

/// Every command a drop asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for index in 0..gestures(&dressed()).len() {
        let mut pool = dressed();
        let drop = gestures(&pool).swap_remove(index);
        let what = drop.what();
        let watch = Watch::on(&pool.state);
        pool_drop::apply(&mut pool.state, drop)
            .unwrap_or_else(|err| panic!("dropping {what} was refused: {err}"));
        out.extend(watch.take());
    }
    out
}

/// Every gesture named by `Dropped::what`, read out of its own match.
///
/// Derived from source rather than listed: the compiler forces a new payload to
/// have an arm there, so reading the arms makes a payload nobody drops go red.
fn gestures_in_source() -> BTreeSet<String> {
    let source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/surfaces/pool_drop.rs"),
    )
    .expect("the drop registry is readable");
    let body = source
        .split_once("pub fn what(&self)")
        .expect("`Dropped::what` names the gestures")
        .1
        .split_once("\n    }")
        .expect("`Dropped::what` closes")
        .0;
    body.lines()
        .filter_map(|line| line.split_once("=> \""))
        .map(|(_, rest)| {
            rest.split_once('"')
                .expect("a gesture is a quoted string")
                .0
                .to_string()
        })
        .collect()
}

#[test]
fn every_gesture_the_pool_accepts_is_dropped() {
    // Fail by default on a new payload: `what` must cover it to compile, so it
    // appears here whether or not anyone dropped it.
    let pool = dressed();
    let dropped: BTreeSet<String> = gestures(&pool)
        .iter()
        .map(|drop| drop.what().to_string())
        .collect();
    assert_eq!(
        dropped.len(),
        gestures(&pool).len(),
        "two gestures in the census are the same one"
    );
    assert_eq!(
        dropped,
        gestures_in_source(),
        "the dropped gestures and the gestures the pool accepts are not the same set"
    );
}

#[test]
fn every_drop_files_something() {
    // A drop that dispatches nothing is a payload that vanishes: the drag ends,
    // the highlight clears, and the source is where it was.
    for index in 0..gestures(&dressed()).len() {
        let mut pool = dressed();
        let drop = gestures(&pool).swap_remove(index);
        let what = drop.what();
        let watch = Watch::on(&pool.state);
        pool_drop::apply(&mut pool.state, drop).expect("the drop lands");
        assert!(
            !watch.take().is_empty(),
            "dropping {what} asked for nothing"
        );
    }
}

#[test]
fn a_source_lands_in_the_folder_it_was_dropped_on_and_comes_back_out() {
    // The root is a destination, not the absence of one: a source dropped on the
    // root crumb has to leave the folder it was in.
    let mut pool = dressed();
    pool_drop::apply(
        &mut pool.state,
        Dropped::MediaOnBin {
            media: pool.media,
            bin: Some(pool.first),
        },
    )
    .expect("the source files");
    assert_eq!(
        bin_of(&pool.state, pool.media),
        Some(pool.first),
        "the source is not in the folder it was dropped on"
    );

    pool_drop::apply(
        &mut pool.state,
        Dropped::MediaOnBin {
            media: pool.media,
            bin: None,
        },
    )
    .expect("the source comes back out");
    assert_eq!(
        bin_of(&pool.state, pool.media),
        None,
        "the source did not come back out to the pool root"
    );
}

#[test]
fn a_folder_dropped_into_its_own_subtree_is_refused_without_an_edit() {
    // The project owns this rule. What matters here is that the panel gets the
    // refusal to report instead of a tree that quietly ate a folder.
    let mut pool = dressed();
    pool_drop::apply(
        &mut pool.state,
        Dropped::BinOnBin {
            bin: pool.second,
            parent: Some(pool.first),
        },
    )
    .expect("the folder nests");
    let watch = Watch::on(&pool.state);
    let refused = pool_drop::apply(
        &mut pool.state,
        Dropped::BinOnBin {
            bin: pool.first,
            parent: Some(pool.second),
        },
    );
    assert!(
        refused.is_err(),
        "a folder was moved inside its own subtree"
    );
    assert_eq!(
        parent_of(&pool.state, pool.second),
        Some(pool.first),
        "the refused move changed the tree"
    );
    assert_eq!(
        watch.take().len(),
        1,
        "a refused drop asked for something other than the move it was refused"
    );
}

#[test]
fn a_source_dropped_on_a_spot_keeps_that_spot() {
    // The freeform view draws each tile where the pool says it is, so a drop that
    // dispatched without the coordinates would send every tile back to its
    // default column.
    let mut pool = dressed();
    pool_drop::apply(
        &mut pool.state,
        Dropped::MediaAtSpot {
            media: pool.media,
            x: 40.0,
            y: 60.0,
        },
    )
    .expect("the spot lands");
    let at = pool
        .state
        .store
        .media
        .iter()
        .find(|item| item.id == pool.media)
        .expect("the source is in the pool")
        .position;
    assert_eq!(at, Some((40.0, 60.0)), "the tile did not keep its spot");
}

#[test]
fn the_panels_file_a_drop_through_the_registry() {
    // The census calls `pool_drop::apply`. That measures the drops only while the
    // panels build their payloads and call the same function.
    //
    // Not covered: the highlight a drag draws over a target, or which element
    // accepts which payload type. Both are the panel's.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, count) in [
        ("panels/sidebar_bins.rs", 4),
        ("panels/sidebar_media_grid.rs", 1),
    ] {
        let drawn = std::fs::read_to_string(src.join(file))
            .unwrap_or_else(|err| panic!("{file} is readable: {err}"));
        assert_eq!(
            drawn.matches("pool_drop::apply").count(),
            count,
            "{file} does not file its {count} drops through the registry"
        );
    }
}

fn bin_of(state: &AppState, media: u64) -> Option<u64> {
    state
        .store
        .media
        .iter()
        .find(|item| item.id == media)
        .expect("the source is in the pool")
        .bin
}

fn parent_of(state: &AppState, bin: u64) -> Option<u64> {
    state
        .store
        .bins
        .iter()
        .find(|item| item.id == bin)
        .expect("the folder is in the tree")
        .parent
}
