use super::*;

#[test]
fn a_row_per_tool() {
    for (index, spec) in SHIPPED.iter().enumerate() {
        assert_eq!(
            index_of(spec.tool),
            index,
            "the '{}' row resolves to '{}' instead",
            spec.id,
            SHIPPED[index_of(spec.tool)].id
        );
        assert_eq!(
            tool_spec(spec.tool).id,
            spec.id,
            "{:?} does not resolve to its own row",
            spec.tool
        );
    }
}

#[test]
fn keys_and_ids_are_unique() {
    let count = SHIPPED.len();

    let mut keys: Vec<&str> = SHIPPED.iter().map(|spec| spec.key).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), count, "two tools claim the same key");

    let mut ids: Vec<&str> = SHIPPED.iter().map(|spec| spec.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "two tools claim the same control id");
}

#[test]
fn every_row_is_drawable() {
    for spec in SHIPPED {
        assert!(!spec.label.is_empty(), "{:?} has no name", spec.tool);
        assert!(!spec.key.is_empty(), "{:?} has no key", spec.tool);
    }
}

#[test]
fn every_glyph_is_a_loadable_asset() {
    use gpui::AssetSource as _;

    for spec in SHIPPED {
        let bytes = crate::icons::Assets
            .load(spec.icon)
            .unwrap_or_else(|err| panic!("{:?} glyph {} errored: {err}", spec.tool, spec.icon));
        assert!(
            bytes.is_some(),
            "{:?} glyph {} is not a registered asset, so its button renders blank",
            spec.tool,
            spec.icon
        );
    }
}

#[test]
fn glyphs_are_unique() {
    let mut icons: Vec<&str> = SHIPPED.iter().map(|spec| spec.icon).collect();
    let count = icons.len();
    icons.sort_unstable();
    icons.dedup();
    assert_eq!(icons.len(), count, "two tools draw the same glyph");
}

#[test]
fn one_gesture_per_tool() {
    for spec in SHIPPED {
        let claimed = u8::from(spec.selects)
            + u8::from(spec.pans)
            + u8::from(spec.zooms)
            + u8::from(spec.cuts)
            + u8::from(spec.body.is_some())
            + u8::from(spec.edge.is_some())
            + u8::from(spec.track_select.is_some())
            + u8::from(spec.pens)
            + u8::from(spec.types);
        assert_eq!(
            claimed, 1,
            "{:?} claims {claimed} gestures; exactly one wins a press",
            spec.tool
        );
    }
}

#[test]
fn exactly_one_tool_selects() {
    let selecting: Vec<EditorTool> = SHIPPED
        .iter()
        .filter(|spec| spec.selects)
        .map(|spec| spec.tool)
        .collect();
    assert_eq!(selecting, vec![EditorTool::default()]);
}

#[test]
fn every_gesture_variant_has_one_tool() {
    let body: Vec<BodyDrag> = SHIPPED.iter().filter_map(|spec| spec.body).collect();
    assert_eq!(body, vec![BodyDrag::Slip, BodyDrag::Slide]);

    let edge: Vec<EdgeDrag> = SHIPPED.iter().filter_map(|spec| spec.edge).collect();
    assert_eq!(edge, vec![EdgeDrag::Ripple, EdgeDrag::Roll, EdgeDrag::Rate]);

    let track_select: Vec<TrackSelectDir> = SHIPPED
        .iter()
        .filter_map(|spec| spec.track_select)
        .collect();
    assert_eq!(
        track_select,
        vec![TrackSelectDir::Forward, TrackSelectDir::Backward]
    );

    let pens: Vec<EditorTool> = SHIPPED
        .iter()
        .filter(|spec| spec.pens)
        .map(|spec| spec.tool)
        .collect();
    assert_eq!(pens, vec![EditorTool::Pen]);

    let types: Vec<EditorTool> = SHIPPED
        .iter()
        .filter(|spec| spec.types)
        .map(|spec| spec.tool)
        .collect();
    assert_eq!(types, vec![EditorTool::Type]);
}

#[test]
fn a_control_id_finds_its_row() {
    assert_eq!(
        tool_spec_by_id("tool-roll").map(|spec| spec.tool),
        Some(EditorTool::Roll)
    );
    assert_eq!(
        tool_spec_by_id("tool-pen").map(|spec| spec.tool),
        Some(EditorTool::Pen)
    );
    assert_eq!(
        tool_spec_by_id("tool-type").map(|spec| spec.tool),
        Some(EditorTool::Type)
    );
    assert!(tool_spec_by_id("tool-nonesuch").is_none());
}
