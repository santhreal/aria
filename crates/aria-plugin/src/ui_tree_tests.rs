//! Tests for the panel tree and its events.
//!
//! Closes the class "a tree or an event the host accepts that a panel could not
//! have meant": every node kind is checked against its limits at the boundary,
//! and every event kind against the control it claims to come from. It does
//! not cover drawing; `aria-ui` owns that.

use super::*;

/// One node of every kind, each inside its limits.
fn every_kind() -> Vec<UiNode> {
    vec![
        UiNode::column([UiNode::text("inside")]),
        UiNode::row([UiNode::Separator]),
        UiNode::heading("Levels"),
        UiNode::toned("clipped", Tone::Danger),
        UiNode::field("Peak", "-1.0 dB"),
        UiNode::Separator,
        UiNode::button("go", "Go"),
        UiNode::toggle("live", "Live", true),
        UiNode::Slider {
            id: "gain".into(),
            label: "Gain".into(),
            value: 0.5,
            min: 0.0,
            max: 1.0,
            step: 0.25,
        },
        UiNode::input("name", "", "Name"),
        UiNode::choice("mode", "Mode", ["A", "B"], 1),
        UiNode::progress("Rendering", 0.5),
        UiNode::Image {
            width: 2,
            height: 1,
            rgba: Arc::from([0u8; 8].as_slice()),
        },
    ]
}

/// Which kind a node is. Fails to compile when `UiNode` grows a variant, so a
/// new kind turns this file red until `every_kind` holds one.
fn kind(node: &UiNode) -> &'static str {
    match node {
        UiNode::Column { .. } => "column",
        UiNode::Row { .. } => "row",
        UiNode::Heading { .. } => "heading",
        UiNode::Text { .. } => "text",
        UiNode::Field { .. } => "field",
        UiNode::Separator => "separator",
        UiNode::Button { .. } => "button",
        UiNode::Toggle { .. } => "toggle",
        UiNode::Slider { .. } => "slider",
        UiNode::Input { .. } => "input",
        UiNode::Choice { .. } => "choice",
        UiNode::Progress { .. } => "progress",
        UiNode::Image { .. } => "image",
    }
}

#[test]
fn every_kind_checks_and_round_trips_through_json_under_its_tag() {
    let nodes = every_kind();
    let mut kinds: Vec<&str> = nodes.iter().map(kind).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(kinds.len(), 13, "every_kind is missing a kind");

    for node in nodes {
        node.check()
            .unwrap_or_else(|err| panic!("{}: {err}", kind(&node)));
        let json = serde_json::to_value(&node).expect("serializes");
        assert_eq!(json["kind"], kind(&node), "{json}");
        let back: UiNode = serde_json::from_value(json).expect("deserializes");
        assert_eq!(back, node);
    }
    let whole = UiNode::column(every_kind());
    whole.check().expect("the whole set checks as one tree");
}

/// The wire shape a native plugin writes: optional fields may be left out.
#[test]
fn optional_fields_default_when_absent() {
    let parsed: UiNode = serde_json::from_str(
        r#"{"kind":"column","children":[
            {"kind":"button","id":"go","label":"Go"},
            {"kind":"text","text":"plain"},
            {"kind":"input","id":"q","value":""},
            {"kind":"slider","id":"s","label":"S","value":1,"min":0,"max":2},
            {"kind":"progress","fraction":0.25}
        ]}"#,
    )
    .expect("parses");
    assert_eq!(
        parsed,
        UiNode::column([
            UiNode::button("go", "Go"),
            UiNode::text("plain"),
            UiNode::input("q", "", ""),
            UiNode::slider("s", "S", 1.0, 0.0..=2.0),
            UiNode::progress("", 0.25),
        ])
    );
    assert!(serde_json::from_str::<UiNode>(r#"{"kind":"marquee"}"#).is_err());
}

fn nested(depth: usize) -> UiNode {
    let mut node = UiNode::Separator;
    for _ in 1..depth {
        node = UiNode::column([node]);
    }
    node
}

fn flat(nodes: usize) -> UiNode {
    UiNode::column((1..nodes).map(|_| UiNode::Separator))
}

#[test]
fn depth_and_node_count_are_held_at_their_limits() {
    assert_eq!(nested(MAX_TREE_DEPTH).check(), Ok(()));
    assert_eq!(nested(MAX_TREE_DEPTH + 1).check(), Err(TreeError::TooDeep));
    assert_eq!(flat(MAX_TREE_NODES).check(), Ok(()));
    assert_eq!(
        flat(MAX_TREE_NODES + 1).check(),
        Err(TreeError::TooManyNodes)
    );
}

#[test]
fn text_is_held_per_string_and_per_tree() {
    let at = "x".repeat(MAX_TEXT_BYTES);
    assert_eq!(UiNode::text(at.as_str()).check(), Ok(()));
    let over = "x".repeat(MAX_TEXT_BYTES + 1);
    assert_eq!(
        UiNode::text(over.as_str()).check(),
        Err(TreeError::TextTooLong(MAX_TEXT_BYTES + 1))
    );
    // Every string counts: a label, an option, a placeholder.
    assert_eq!(
        UiNode::choice("c", "C", [over.as_str()], 0).check(),
        Err(TreeError::TextTooLong(MAX_TEXT_BYTES + 1))
    );
    let fill = MAX_TREE_TEXT_BYTES / MAX_TEXT_BYTES;
    let full = UiNode::column((0..fill).map(|_| UiNode::text(at.as_str())));
    assert_eq!(full.check(), Ok(()));
    let past = UiNode::column(
        (0..fill)
            .map(|_| UiNode::text(at.as_str()))
            .chain([UiNode::text("x")]),
    );
    assert_eq!(past.check(), Err(TreeError::TooMuchText));
}

#[test]
fn control_ids_are_non_empty_bounded_and_unique() {
    let cases = [
        (UiNode::button("", "Go"), TreeError::EmptyId),
        (UiNode::button("  ", "Go"), TreeError::EmptyId),
        (
            UiNode::button("a\nb", "Go"),
            TreeError::BadId("a\nb".into()),
        ),
        (
            UiNode::button("x".repeat(MAX_WIDGET_ID_BYTES + 1), "Go"),
            TreeError::BadId("x".repeat(32)),
        ),
        (
            UiNode::row([UiNode::button("go", "A"), UiNode::toggle("go", "B", false)]),
            TreeError::DuplicateId("go".into()),
        ),
        (
            UiNode::column([
                UiNode::row([UiNode::input("q", "", "")]),
                UiNode::row([UiNode::choice("q", "Q", ["a"], 0)]),
            ]),
            TreeError::DuplicateId("q".into()),
        ),
    ];
    for (node, err) in cases {
        assert_eq!(node.check(), Err(err));
    }
    assert_eq!(
        UiNode::button("x".repeat(MAX_WIDGET_ID_BYTES), "Go").check(),
        Ok(())
    );
}

fn slider(value: f64, min: f64, max: f64, step: f64) -> UiNode {
    UiNode::Slider {
        id: "s".into(),
        label: "S".into(),
        value,
        min,
        max,
        step,
    }
}

#[test]
fn a_slider_needs_a_finite_ordered_range_holding_its_value() {
    let bad_range = TreeError::BadRange("s".into());
    let outside = TreeError::OutOfRange("s".into());
    let cases = [
        (slider(f64::NAN, 0.0, 1.0, 0.0), bad_range.clone()),
        (slider(0.5, 0.0, f64::INFINITY, 0.0), bad_range.clone()),
        (slider(1.0, 1.0, 1.0, 0.0), bad_range.clone()),
        (slider(0.5, 1.0, 0.0, 0.0), bad_range.clone()),
        (slider(0.5, 0.0, 1.0, -0.1), bad_range.clone()),
        (slider(0.5, 0.0, 1.0, 1.5), bad_range),
        (slider(-0.01, 0.0, 1.0, 0.0), outside.clone()),
        (slider(1.01, 0.0, 1.0, 0.0), outside),
    ];
    for (node, err) in cases {
        assert_eq!(node.check(), Err(err), "{node:?}");
    }
    for value in [0.0, 1.0] {
        assert_eq!(slider(value, 0.0, 1.0, 1.0).check(), Ok(()));
    }
}

#[test]
fn choices_progress_and_images_are_held_to_their_shapes() {
    let opts = |n: usize| (0..n).map(|i| i.to_string()).collect::<Vec<_>>();
    assert_eq!(
        UiNode::choice("c", "C", Vec::<String>::new(), 0).check(),
        Err(TreeError::BadOptions("c".into()))
    );
    assert_eq!(
        UiNode::choice("c", "C", opts(MAX_CHOICE_OPTIONS + 1), 0).check(),
        Err(TreeError::BadOptions("c".into()))
    );
    assert_eq!(
        UiNode::choice("c", "C", opts(MAX_CHOICE_OPTIONS), MAX_CHOICE_OPTIONS - 1).check(),
        Ok(())
    );
    assert_eq!(
        UiNode::choice("c", "C", opts(2), 2).check(),
        Err(TreeError::OutOfRange("c".into()))
    );

    for fraction in [-0.01, 1.01, f32::NAN] {
        assert_eq!(
            UiNode::progress("", fraction).check(),
            Err(TreeError::BadFraction),
            "{fraction}"
        );
    }

    let image = |width: u32, height: u32, bytes: usize| UiNode::Image {
        width,
        height,
        rgba: Arc::from(vec![0u8; bytes]),
    };
    let bad = |width, height, bytes| TreeError::BadImage {
        width,
        height,
        bytes,
    };
    assert_eq!(image(0, 1, 0).check(), Err(bad(0, 1, 0)));
    assert_eq!(image(2, 2, 15).check(), Err(bad(2, 2, 15)));
    assert_eq!(
        image(MAX_IMAGE_SIDE + 1, 1, (MAX_IMAGE_SIDE as usize + 1) * 4).check(),
        Err(bad(
            MAX_IMAGE_SIDE + 1,
            1,
            (MAX_IMAGE_SIDE as usize + 1) * 4
        ))
    );
    assert_eq!(
        image(MAX_IMAGE_SIDE, 1, MAX_IMAGE_SIDE as usize * 4).check(),
        Ok(())
    );

    // One buffer shared by every node: the budget counts each node's bytes,
    // shared or not.
    let side = MAX_IMAGE_SIDE as usize;
    let pixels: Arc<[u8]> = Arc::from(vec![0u8; side * side * 4]);
    let full = || UiNode::Image {
        width: MAX_IMAGE_SIDE,
        height: MAX_IMAGE_SIDE,
        rgba: Arc::clone(&pixels),
    };
    let fits = MAX_TREE_IMAGE_BYTES / pixels.len();
    let at = UiNode::column((0..fits).map(|_| full()));
    assert_eq!(at.check(), Ok(()));
    let past = UiNode::column((0..fits).map(|_| full()).chain([image(1, 1, 4)]));
    assert_eq!(past.check(), Err(TreeError::TooMuchImage));
}

fn panel() -> UiNode {
    UiNode::column([
        UiNode::row([
            UiNode::button("go", "Go"),
            UiNode::Button {
                id: "off".into(),
                label: "Off".into(),
                disabled: true,
            },
        ]),
        UiNode::toggle("live", "Live", false),
        slider(0.5, 0.0, 1.0, 0.25),
        UiNode::input("name", "", ""),
        UiNode::choice("mode", "Mode", ["A", "B"], 0),
    ])
}

#[test]
fn an_event_matching_its_control_passes_unchanged() {
    let events = [
        PanelEvent::Pressed { id: "go".into() },
        PanelEvent::Toggled {
            id: "live".into(),
            on: true,
        },
        PanelEvent::Slid {
            id: "s".into(),
            value: 0.75,
        },
        PanelEvent::Submitted {
            id: "name".into(),
            text: "Take 2".into(),
        },
        PanelEvent::Chose {
            id: "mode".into(),
            index: 1,
        },
    ];
    for event in events {
        assert_eq!(event.clone().conform(&panel()), Ok(event));
    }
}

#[test]
fn a_slid_value_is_clamped_and_snapped_to_the_step() {
    let slid = |value| PanelEvent::Slid {
        id: "s".into(),
        value,
    };
    for (sent, got) in [(0.37, 0.25), (0.38, 0.5), (5.0, 1.0), (-3.0, 0.0)] {
        assert_eq!(slid(sent).conform(&panel()), Ok(slid(got)), "{sent}");
    }
    assert_eq!(
        slid(f64::NAN).conform(&panel()),
        Err(EventMismatch::Wrong("s".into()))
    );
    // A step that does not divide the range never lands past `max`.
    let snapped = snap(1.0, 0.0, 1.0, 0.3);
    assert!(snapped <= 1.0 && (snapped - 0.9).abs() < 1e-12, "{snapped}");
    assert_eq!(snap(0.7, 0.0, 1.0, 0.0), 0.7);
}

#[test]
fn an_event_its_control_cannot_produce_is_refused() {
    let cases = [
        (
            PanelEvent::Pressed { id: "nope".into() },
            EventMismatch::NoControl("nope".into()),
        ),
        (
            PanelEvent::Pressed { id: "off".into() },
            EventMismatch::Disabled("off".into()),
        ),
        (
            PanelEvent::Pressed { id: "live".into() },
            EventMismatch::Wrong("live".into()),
        ),
        (
            PanelEvent::Toggled {
                id: "go".into(),
                on: true,
            },
            EventMismatch::Wrong("go".into()),
        ),
        (
            PanelEvent::Chose {
                id: "mode".into(),
                index: 2,
            },
            EventMismatch::Wrong("mode".into()),
        ),
        (
            PanelEvent::Submitted {
                id: "name".into(),
                text: "two\nlines".into(),
            },
            EventMismatch::Wrong("name".into()),
        ),
        (
            PanelEvent::Submitted {
                id: "name".into(),
                text: "x".repeat(MAX_TEXT_BYTES + 1).into(),
            },
            EventMismatch::Wrong("name".into()),
        ),
        (
            PanelEvent::Slid {
                id: "mode".into(),
                value: 0.0,
            },
            EventMismatch::Wrong("mode".into()),
        ),
    ];
    for (event, err) in cases {
        assert_eq!(event.clone().conform(&panel()), Err(err), "{event:?}");
    }
}

#[test]
fn find_reaches_controls_at_any_depth() {
    let tree = nested(MAX_TREE_DEPTH);
    assert!(tree.find("anything").is_none());
    let deep = UiNode::column([UiNode::row([UiNode::column([UiNode::toggle(
        "deep", "Deep", false,
    )])])]);
    assert!(matches!(deep.find("deep"), Some(UiNode::Toggle { .. })));
}
