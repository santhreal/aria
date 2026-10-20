//! Bidirectional reference documentation checks for `docs/reference/keyframes-and-graph.md`.
//!
//! # Why
//!
//! Closes the defect class where a `PropertyGroup`, `RevealAction`,
//! `KeyframeIconShape`, `TemporalEase` constant, `GraphMode`, `GraphHandleSide`,
//! `DriverCode` limit (`MAX_DRIVER_LANGUAGE`, `MAX_DRIVER_SOURCE`), or
//! `aria_render::driver` registration / budget (`DEFAULT_DRIVER_BUDGET`,
//! `DriverLanguageRegistration`) changes without updating
//! `docs/reference/keyframes-and-graph.md`.
//!
//! Not caught: prose descriptions in the table summary columns.

use std::sync::Arc;

use aria_models::{
    EASY_EASE_INFLUENCE, Easing, MAX_DRIVER_LANGUAGE, MAX_DRIVER_SOURCE, MAX_TEMPORAL_INFLUENCE,
    MIN_TEMPORAL_INFLUENCE, ParamValue, TemporalEase,
};
use aria_render::{
    DEFAULT_DRIVER_BUDGET, DriverInput, DriverLanguage, DriverProgram, DriverScope, InputClip,
    InputProperty, driver_languages, register_driver_language,
};

use crate::app::canvas_overlay::motion_path::{PathParam, TangentSide};
use crate::app::property_rows::graph::{GRAPH_PAD_Y, graph_axis_ys};
use crate::app::property_rows::{
    GRAPH_EDITOR_H, GraphHandleSide, GraphMode, KeyframeIconShape, PROPERTY_ROW_H, PropertyGroup,
    RevealAction,
};

use crate::test_support::doc_tables::{
    heading_body as section, read_doc, strip_ticks as unquote, table_rows,
};

#[test]
fn keyframes_and_graph_doc_matches_registries() {
    let doc_buf = read_doc("reference/keyframes-and-graph.md");
    #[allow(non_snake_case)]
    let DOC = doc_buf.as_str();
    // 1. PropertyGroup::ALL & PROPERTY_ROW_H.
    let group_sec = section(
        DOC,
        "## Timeline property outline groups (`PropertyGroup::ALL`)",
    );
    assert!(
        group_sec.contains(&format!("PROPERTY_ROW_H = {PROPERTY_ROW_H:.1}")),
        "keyframes-and-graph.md must document PROPERTY_ROW_H = {PROPERTY_ROW_H:.1}"
    );
    for g in PropertyGroup::ALL {
        match g {
            PropertyGroup::Transform
            | PropertyGroup::Effects
            | PropertyGroup::Masks
            | PropertyGroup::Audio => {}
        }
    }
    let group_rows = table_rows(group_sec);
    let doc_groups: Vec<(String, bool)> = group_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 4, "malformed PropertyGroup row: {r:?}");
            (unquote(r[0]).to_string(), unquote(r[3]) == "true")
        })
        .collect();
    let expected_groups: Vec<(String, bool)> = PropertyGroup::ALL
        .iter()
        .map(|&g| (format!("{g:?}"), g == PropertyGroup::Transform))
        .collect();
    assert_eq!(
        doc_groups, expected_groups,
        "docs/reference/keyframes-and-graph.md PropertyGroup table drifted"
    );

    // 2. RevealAction (exhaustive match + table check).
    let all_reveals = [
        RevealAction::Position,
        RevealAction::Scale,
        RevealAction::Rotation,
        RevealAction::Opacity,
        RevealAction::Anchor,
        RevealAction::Effects,
        RevealAction::Masks,
        RevealAction::Audio,
        RevealAction::AnimatedOrModified,
        RevealAction::Modified,
        RevealAction::CloseAll,
    ];
    for a in all_reveals {
        match a {
            RevealAction::Position
            | RevealAction::Scale
            | RevealAction::Rotation
            | RevealAction::Opacity
            | RevealAction::Anchor
            | RevealAction::Effects
            | RevealAction::Masks
            | RevealAction::Audio
            | RevealAction::AnimatedOrModified
            | RevealAction::Modified
            | RevealAction::CloseAll => {}
        }
    }
    let reveal_rows = table_rows(section(DOC, "## Property reveal actions (`RevealAction`)"));
    let doc_reveals: Vec<String> = reveal_rows
        .iter()
        .map(|r| unquote(r[0]).to_string())
        .collect();
    let expected_reveals: Vec<String> = all_reveals.iter().map(|a| format!("{a:?}")).collect();
    assert_eq!(
        doc_reveals, expected_reveals,
        "docs/reference/keyframes-and-graph.md RevealAction table drifted"
    );

    // 3. KeyframeIconShape & TemporalEase.
    let icon_sec = section(
        DOC,
        "## Keyframe icon shapes and temporal ease (`KeyframeIconShape` and `TemporalEase`)",
    );
    assert!(
        icon_sec.contains(&format!(
            "`{MIN_TEMPORAL_INFLUENCE}..={MAX_TEMPORAL_INFLUENCE:.1}`"
        )) && icon_sec.contains(&format!("{EASY_EASE_INFLUENCE:.8}")),
        "keyframes-and-graph.md missing TemporalEase constants"
    );
    let all_icons = [
        KeyframeIconShape::Linear,
        KeyframeIconShape::Bezier,
        KeyframeIconShape::EaseIn,
        KeyframeIconShape::EaseOut,
        KeyframeIconShape::Hold,
        KeyframeIconShape::Roving,
    ];
    for i in all_icons {
        match i {
            KeyframeIconShape::Linear
            | KeyframeIconShape::Bezier
            | KeyframeIconShape::EaseIn
            | KeyframeIconShape::EaseOut
            | KeyframeIconShape::Hold
            | KeyframeIconShape::Roving => {}
        }
    }
    assert_eq!(
        KeyframeIconShape::classify(0, 2, Easing::Linear),
        KeyframeIconShape::Linear
    );
    assert_eq!(
        KeyframeIconShape::classify(0, 2, Easing::EaseInOut),
        KeyframeIconShape::Bezier
    );
    assert_eq!(
        KeyframeIconShape::classify(0, 2, Easing::EaseOut),
        KeyframeIconShape::EaseIn
    );
    assert_eq!(
        KeyframeIconShape::classify(0, 2, Easing::EaseIn),
        KeyframeIconShape::EaseOut
    );
    assert_eq!(
        KeyframeIconShape::classify(0, 2, Easing::Hold),
        KeyframeIconShape::Hold
    );
    assert_eq!(
        KeyframeIconShape::classify(
            1,
            3,
            Easing::Temporal {
                in_ease: None,
                out_ease: None,
                roving: true,
                hold: false,
                points: [
                    EASY_EASE_INFLUENCE,
                    EASY_EASE_INFLUENCE,
                    1.0 - EASY_EASE_INFLUENCE,
                    1.0 - EASY_EASE_INFLUENCE,
                ],
            }
        ),
        KeyframeIconShape::Roving
    );
    assert_eq!(
        TemporalEase::default(),
        TemporalEase {
            speed: 0.0,
            influence: EASY_EASE_INFLUENCE,
        }
    );
    let icon_rows = table_rows(icon_sec.split("\n### ").next().unwrap_or(icon_sec));
    let doc_icons: Vec<String> = icon_rows
        .iter()
        .map(|r| unquote(r[0]).to_string())
        .collect();
    let expected_icons: Vec<String> = all_icons.iter().map(|i| format!("{i:?}")).collect();
    assert_eq!(
        doc_icons, expected_icons,
        "docs/reference/keyframes-and-graph.md KeyframeIconShape table drifted"
    );

    let easing_sec = section(DOC, "### Outgoing segment interpolation (`Easing`)");
    let easing_samples = [
        ("Linear", "linear", Easing::Linear, "false", "false", true),
        ("EaseIn", "ease_in", Easing::EaseIn, "false", "true", false),
        (
            "EaseOut",
            "ease_out",
            Easing::EaseOut,
            "false",
            "true",
            false,
        ),
        (
            "EaseInOut",
            "ease_in_out",
            Easing::EaseInOut,
            "false",
            "true",
            false,
        ),
        ("Hold", "hold", Easing::Hold, "true", "false", false),
        (
            "Bezier",
            "bezier",
            Easing::Bezier {
                points: [0.25, 0.1, 0.25, 1.0],
            },
            "false",
            "true",
            false,
        ),
        (
            "Temporal",
            "temporal",
            Easing::Temporal {
                in_ease: Some(TemporalEase::EASY_EASE),
                out_ease: None,
                roving: false,
                hold: false,
                points: [
                    EASY_EASE_INFLUENCE,
                    EASY_EASE_INFLUENCE,
                    1.0 - EASY_EASE_INFLUENCE,
                    1.0 - EASY_EASE_INFLUENCE,
                ],
            },
            "hold",
            "!hold && (in_ease.is_some() or out_ease.is_some())",
            false,
        ),
    ];
    for &(_, _, sample, _, _, _) in &easing_samples {
        match sample {
            Easing::Linear
            | Easing::EaseIn
            | Easing::EaseOut
            | Easing::EaseInOut
            | Easing::Hold
            | Easing::Bezier { .. }
            | Easing::Temporal { .. } => {}
        }
    }
    let easing_rows = table_rows(easing_sec);
    let doc_easings: Vec<(String, String, String, String, bool)> = easing_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]) == "yes",
            )
        })
        .collect();
    let expected_easings: Vec<(String, String, String, String, bool)> = easing_samples
        .iter()
        .map(|&(name, tag, sample, hold_s, bez_s, is_def)| {
            let json = serde_json::to_string(&sample).expect("serialize Easing");
            assert!(
                json.contains(tag),
                "expected Easing::{name} JSON {json} to contain `{tag}`"
            );
            if !matches!(sample, Easing::Temporal { .. }) {
                assert_eq!(sample.is_hold().to_string(), hold_s);
                assert_eq!(sample.is_bezier_like().to_string(), bez_s);
            }
            (
                name.to_string(),
                tag.to_string(),
                hold_s.to_string(),
                bez_s.to_string(),
                is_def && sample == Easing::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_easings, expected_easings,
        "docs/reference/keyframes-and-graph.md Easing table drifted"
    );

    // 4. GraphMode, GraphHandleSide, GRAPH_EDITOR_H, GRAPH_PAD_Y, graph_axis_ys.
    let graph_sec = section(
        DOC,
        "## Timeline graph editor (`GraphMode`, `GraphHandleSide`, `GraphDragTarget`)",
    );
    let ys = graph_axis_ys(GRAPH_EDITOR_H);
    assert!(
        graph_sec.contains(&format!("GRAPH_EDITOR_H = {GRAPH_EDITOR_H:.1}"))
            && graph_sec.contains(&format!("GRAPH_PAD_Y = {GRAPH_PAD_Y:.1}"))
            && graph_sec.contains(&format!(
                "graph_axis_ys({GRAPH_EDITOR_H:.1}) = [{:.1}, {:.1}, {:.1}]",
                ys[0], ys[1], ys[2]
            )),
        "keyframes-and-graph.md missing graph geometry constants"
    );
    let all_modes = [GraphMode::Value, GraphMode::Speed];
    for m in all_modes {
        match m {
            GraphMode::Value | GraphMode::Speed => {}
        }
    }
    for s in [GraphHandleSide::Incoming, GraphHandleSide::Outgoing] {
        match s {
            GraphHandleSide::Incoming | GraphHandleSide::Outgoing => {}
        }
    }
    let mode_rows = table_rows(graph_sec.split("\n### ").next().unwrap_or(graph_sec));
    let doc_modes: Vec<(String, String, String, bool)> = mode_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 4, "malformed GraphMode row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_modes = vec![
        (
            "Value".to_string(),
            "graph-mode-value".to_string(),
            "Value".to_string(),
            GraphMode::Value == GraphMode::default(),
        ),
        (
            "Speed".to_string(),
            "graph-mode-speed".to_string(),
            "Speed".to_string(),
            GraphMode::Speed == GraphMode::default(),
        ),
    ];
    assert_eq!(
        doc_modes, expected_modes,
        "docs/reference/keyframes-and-graph.md GraphMode table drifted"
    );

    let path_sec = section(
        DOC,
        "### On-canvas motion path (`PathParam::ALL` and `TangentSide::ALL`)",
    );
    let mut path_tables = Vec::new();
    for block in path_sec.split("\n\n") {
        let r = table_rows(block);
        if !r.is_empty() {
            path_tables.push(r);
        }
    }
    assert_eq!(
        path_tables.len(),
        2,
        "expected 2 tables under On-canvas motion path"
    );
    let doc_path_params: Vec<(String, String, String, bool)> = path_tables[0]
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]) == "true",
            )
        })
        .collect();
    let expected_path_params: Vec<(String, String, String, bool)> = PathParam::ALL
        .iter()
        .map(|&p| {
            (
                format!("{p:?}"),
                format!("{:?}", p.param()),
                p.id().to_string(),
                p.bends(),
            )
        })
        .collect();
    assert_eq!(
        doc_path_params, expected_path_params,
        "docs/reference/keyframes-and-graph.md PathParam::ALL table drifted"
    );

    let doc_tangent_sides: Vec<String> = path_tables[1]
        .iter()
        .map(|r| unquote(r[0]).to_string())
        .collect();
    let expected_tangent_sides: Vec<String> =
        TangentSide::ALL.iter().map(|s| format!("{s:?}")).collect();
    assert_eq!(
        doc_tangent_sides, expected_tangent_sides,
        "docs/reference/keyframes-and-graph.md TangentSide::ALL table drifted"
    );

    // 5. Property drivers (DriverCode, DriverLanguageRegistration, DEFAULT_DRIVER_BUDGET).
    let drv_sec = section(
        DOC,
        "## Property drivers (`DriverCode`, `PropertyDriver`, `DriverLanguageRegistration`)",
    );
    assert!(
        drv_sec.contains(&format!("MAX_DRIVER_LANGUAGE = {MAX_DRIVER_LANGUAGE}"))
            && drv_sec.contains(&format!("MAX_DRIVER_SOURCE = {MAX_DRIVER_SOURCE}"))
            && drv_sec.contains(&format!(
                "`DEFAULT_DRIVER_BUDGET` is `{}` ms",
                DEFAULT_DRIVER_BUDGET.as_millis()
            )),
        "keyframes-and-graph.md missing driver limits or budget"
    );
    for clip_ref in [InputClip::This, InputClip::Named("a".into())] {
        match clip_ref {
            InputClip::This | InputClip::Named(_) => {}
        }
    }
    for prop_ref in [
        InputProperty::Id("opacity".into()),
        InputProperty::Effect {
            effect: "blur".into(),
            param: "radius".into(),
        },
    ] {
        match prop_ref {
            InputProperty::Id(_) | InputProperty::Effect { .. } => {}
        }
    }

    struct DummyProg;
    impl DriverProgram for DummyProg {
        fn inputs(&self) -> &[DriverInput] {
            &[]
        }
        fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
            Ok(scope.value())
        }
    }
    struct DummyLang;
    impl DriverLanguage for DummyLang {
        fn compile(&self, _source: &str) -> Result<Arc<dyn DriverProgram>, String> {
            Ok(Arc::new(DummyProg))
        }
    }
    let reg = register_driver_language("test.keyframes_doc_check", Arc::new(DummyLang))
        .expect("register driver language");
    assert!(driver_languages().contains(&"test.keyframes_doc_check".to_string()));
    drop(reg);
    assert!(!driver_languages().contains(&"test.keyframes_doc_check".to_string()));

    let scope_rows = table_rows(drv_sec);
    let doc_scope_methods: Vec<String> = scope_rows
        .iter()
        .map(|r| unquote(r[0]).to_string())
        .collect();
    let expected_scope_methods = vec![
        "time(&self)",
        "property(&self)",
        "clip_time(&self)",
        "frame_rate(&self)",
        "duration(&self)",
        "canvas(&self)",
        "value(&self)",
        "value_at(&self, seconds: f64)",
        "key_count(&self)",
        "key_time(&self, index: usize)",
        "input(&mut self, index: usize)",
        "input_at(&mut self, index: usize, seconds: f64)",
        "clip_info(&mut self, clip: &InputClip)",
        "seed(&self)",
        "expired(&self)",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    assert_eq!(
        doc_scope_methods, expected_scope_methods,
        "docs/reference/keyframes-and-graph.md DriverScope methods table drifted"
    );
}
