use super::*;

// --- canvas -------------------------------------------------------------

#[test]
fn canvas_defaults_to_auto_black_and_round_trips() {
    let mut timeline = Timeline::new(R24);
    assert!(timeline.canvas().is_default());
    assert_eq!(timeline.canvas().aspect, CanvasAspect::Auto);
    assert_eq!(timeline.canvas().background, [0, 0, 0]);

    // Default settings serialize without the field; pre-canvas files
    // (no `canvas` key) deserialize to the default.
    let json = serde_json::to_value(&timeline).unwrap();
    assert!(json.get("canvas").is_none());
    let loaded: Timeline = serde_json::from_value(json).unwrap();
    assert!(loaded.canvas().is_default());

    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Tall9x16,
        background: [20, 30, 40],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(json["canvas"]["aspect"], "9:16");
    assert_eq!(
        json["canvas"]["background"],
        serde_json::json!([20, 30, 40])
    );
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.canvas(), timeline.canvas());
}

#[test]
fn canvas_partial_fields_serialize_independently() {
    let mut timeline = Timeline::new(R24);
    // Only the aspect set: the black background stays off the wire.
    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        background: [0, 0, 0],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(json["canvas"]["aspect"], "1:1");
    assert!(json["canvas"].get("background").is_none());

    // Only the background set: auto aspect stays off the wire.
    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [255, 255, 255],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert!(json["canvas"].get("aspect").is_none());
    assert_eq!(
        json["canvas"]["background"],
        serde_json::json!([255, 255, 255])
    );
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.canvas().background, [255, 255, 255]);
    assert_eq!(back.canvas().aspect, CanvasAspect::Auto);
}

#[test]
fn canvas_aspect_names_round_trip() {
    for aspect in CanvasAspect::ALL {
        assert_eq!(CanvasAspect::from_name(aspect.name()), Some(aspect));
        match aspect.ratio() {
            None => assert_eq!(aspect, CanvasAspect::Auto),
            Some((w, h)) => {
                assert!(w > 0 && h > 0);
                assert_eq!(aspect.name(), format!("{w}:{h}"));
            }
        }
    }
    assert_eq!(CanvasAspect::from_name("4:3"), None);
}

/// A derived size stays off the wire and an explicit one is written, so a
/// project written before canvas sizes existed still loads, and a delivery size
/// survives a save.
#[test]
fn canvas_resolution_is_optional_on_the_wire() {
    let mut timeline = Timeline::new(R24);
    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: None,
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert!(json["canvas"].get("resolution").is_none());

    timeline.set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: Some([3840, 2160]),
    });
    let json = serde_json::to_value(&timeline).unwrap();
    assert_eq!(
        json["canvas"]["resolution"],
        serde_json::json!([3840, 2160])
    );
    let back: Timeline = serde_json::from_value(json).unwrap();
    assert_eq!(back.canvas().resolution, Some([3840, 2160]));
    assert!(!back.canvas().is_default());
}

/// The encodable bounds are the rule every writer checks, so they are stated
/// once: both edges even, both inside `MIN_DIM..=MAX_DIM`.
#[test]
fn canvas_resolution_bounds_refuse_what_cannot_be_encoded() {
    for size in [
        [1920, 1080],
        [CanvasSettings::MIN_DIM, CanvasSettings::MIN_DIM],
        [CanvasSettings::MAX_DIM, CanvasSettings::MAX_DIM],
    ] {
        assert!(
            CanvasSettings::resolution_is_valid(size),
            "{size:?} should be encodable"
        );
    }
    for size in [
        [0, 0],
        [1920, 0],
        [1921, 1080],
        [1920, 1081],
        [CanvasSettings::MIN_DIM - 2, 1080],
        [CanvasSettings::MAX_DIM + 2, 1080],
    ] {
        assert!(
            !CanvasSettings::resolution_is_valid(size),
            "{size:?} should be refused"
        );
    }
}

/// A baseline names the shorter edge, which is what "1080p" means in both a wide
/// and a vertical framing. Every preset resolves; `Auto` has no shape of its own.
#[test]
fn canvas_baseline_pixels_follow_each_preset_shape() {
    assert_eq!(
        CanvasAspect::Wide16x9.pixels_for_baseline(1080),
        Some([1920, 1080])
    );
    assert_eq!(
        CanvasAspect::Tall9x16.pixels_for_baseline(1080),
        Some([1080, 1920])
    );
    assert_eq!(
        CanvasAspect::Square1x1.pixels_for_baseline(720),
        Some([720, 720])
    );
    assert_eq!(CanvasAspect::Auto.pixels_for_baseline(1080), None);
    for aspect in CanvasAspect::ALL {
        let Some(size) = aspect.pixels_for_baseline(2160) else {
            continue;
        };
        assert!(
            CanvasSettings::resolution_is_valid(size),
            "{} at 2160 produced {size:?}, which is not encodable",
            aspect.name()
        );
        assert_eq!(size[0].min(size[1]), 2160);
    }
}
