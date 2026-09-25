//! Tests for per-item pool positions within bins and the pool root.
//!
//! Closes defect class: Freeform view item coordinates not persisting across saves
//! or losing per-bin independence when an item exists across multiple folders.
//! Honest limit: this suite verifies model persistence, bin scoping, and migration
//! from older serialized formats; UI rasterization and drag hitboxes are tested
//! in `aria-ui`.

use super::*;
use crate::bin::PoolPosition;

#[test]
fn a_pool_position_survives_save_and_load() {
    let mut project = Project::new("test_project", R24);
    let media = project.add_media(sample_media(R24, 100));
    let bin = project.add_bin("B-Roll", None).expect("add bin");

    // Position at root and in bin
    project
        .set_pool_item_position(None, media, PoolPosition::new(120.0, 80.0))
        .expect("set root position");
    project
        .set_pool_item_position(Some(bin), media, PoolPosition::new(340.5, 210.0))
        .expect("set bin position");

    assert_eq!(
        project.pool_item_position(None, media),
        Some(PoolPosition::new(120.0, 80.0))
    );
    assert_eq!(
        project.pool_item_position(Some(bin), media),
        Some(PoolPosition::new(340.5, 210.0))
    );

    let json = serde_json::to_string(&project).expect("serialize");
    let back: Project = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        back.pool_item_position(None, media),
        Some(PoolPosition::new(120.0, 80.0))
    );
    assert_eq!(
        back.pool_item_position(Some(bin), media),
        Some(PoolPosition::new(340.5, 210.0))
    );
}

#[test]
fn the_same_item_in_two_bins_keeps_two_positions() {
    let mut project = Project::new("multi_bin", R24);
    let media = project.add_media(sample_media(R24, 100));
    let bin1 = project.add_bin("Interviews", None).expect("bin1");
    let bin2 = project.add_bin("Selects", None).expect("bin2");

    project
        .set_pool_item_position(Some(bin1), media, PoolPosition::new(50.0, 60.0))
        .expect("set bin1 position");
    project
        .set_pool_item_position(Some(bin2), media, PoolPosition::new(500.0, 600.0))
        .expect("set bin2 position");

    assert_eq!(
        project.pool_item_position(Some(bin1), media),
        Some(PoolPosition::new(50.0, 60.0))
    );
    assert_eq!(
        project.pool_item_position(Some(bin2), media),
        Some(PoolPosition::new(500.0, 600.0))
    );
}

#[test]
fn an_old_file_without_positions_loads_and_reports_none() {
    let mut project = Project::new("legacy", R24);
    let media = project.add_media(sample_media(R24, 100));
    let bin = project.add_bin("OldBin", None).expect("bin");

    // Serialize without setting any positions
    let json = serde_json::to_string(&project).expect("serialize");
    assert!(
        !json.contains("positions"),
        "unarranged pool writes no positions"
    );
    assert!(
        !json.contains("root_positions"),
        "unarranged root writes no root_positions"
    );

    let back: Project = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back.pool_item_position(Some(bin), media),
        None,
        "absent means unarranged, not (0,0)"
    );
    assert_eq!(
        back.pool_item_position(None, media),
        None,
        "absent root means unarranged, not (0,0)"
    );
}

#[test]
fn removing_an_item_from_a_bin_drops_only_that_bins_position() {
    let mut project = Project::new("removal", R24);
    let media = project.add_media(sample_media(R24, 100));
    let bin1 = project.add_bin("BinA", None).expect("bin1");
    let bin2 = project.add_bin("BinB", None).expect("bin2");

    project
        .set_pool_item_position(Some(bin1), media, PoolPosition::new(10.0, 20.0))
        .expect("pos1");
    project
        .set_pool_item_position(Some(bin2), media, PoolPosition::new(30.0, 40.0))
        .expect("pos2");

    let dropped = project
        .remove_pool_item_position(Some(bin1), media)
        .expect("remove from bin1");
    assert_eq!(dropped, Some(PoolPosition::new(10.0, 20.0)));

    assert_eq!(
        project.pool_item_position(Some(bin1), media),
        None,
        "bin1 position is dropped"
    );
    assert_eq!(
        project.pool_item_position(Some(bin2), media),
        Some(PoolPosition::new(30.0, 40.0)),
        "bin2 position remains intact"
    );
}

#[test]
fn unknown_media_or_bin_returns_honest_error() {
    let mut project = Project::new("errors", R24);
    let missing_media = MediaId::next();
    let missing_bin = BinId::next();

    assert!(matches!(
        project.set_pool_item_position(None, missing_media, PoolPosition::new(0.0, 0.0)),
        Err(ModelError::UnknownMedia(_))
    ));

    let media = project.add_media(sample_media(R24, 100));
    assert!(matches!(
        project.set_pool_item_position(Some(missing_bin), media, PoolPosition::new(0.0, 0.0)),
        Err(ModelError::UnknownBin(_))
    ));
}
