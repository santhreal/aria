//! Bidirectional reference documentation checks for `docs/reference/project-settings.md`.
//!
//! # Why
//!
//! Closes the defect class where a `CanvasAspect`, `CANVAS_BASELINES`, `RATES`,
//! `AudioLayout`, `TimeDisplay`, `MarkerColor`, `MarkerKind`, `LabelColor`,
//! `AudioRole`, `TrackKind` (`track_kind_specs()`), or `TrackDisplay::PRESETS`
//! entry changes in `aria-models` or `aria-ui` without updating
//! `docs/reference/project-settings.md`.
//!
//! Not caught: prose commentary outside the structured tables and constants.

use aria_models::{
    AudioLayout, AudioRole, BakeMedium, CanvasAspect, CanvasSettings, LabelColor, LaneZone,
    MarkerColor, MarkerKind, TimeDisplay, TrackDisplay, track_kind_specs,
};

use crate::app::project_settings::CANVAS_BASELINES;
use crate::surfaces::settings_pick::RATES;
use crate::theme::{Appearance, Theme};


fn unquote(cell: &str) -> &str {
    cell.trim().trim_start_matches('`').trim_end_matches('`')
}

fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let level = heading.bytes().take_while(|&b| b == b'#').count();
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("missing heading {heading:?}"));
    let body = &doc[start + heading.len()..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    let mut offset = 0usize;
    for line in body.lines() {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if (1..=level).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
            return &body[..offset];
        }
        offset += line.len() + 1;
    }
    body
}

fn table_rows(text: &str) -> Vec<Vec<&str>> {
    let mut rows = Vec::new();
    let mut in_table = false;
    let mut past_sep = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            if in_table {
                in_table = false;
                past_sep = false;
            }
            continue;
        }
        let cells: Vec<&str> = trimmed
            .trim_start_matches('|')
            .trim_end_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        if !in_table {
            in_table = true;
            continue;
        }
        if !past_sep {
            past_sep = true;
            continue;
        }
        rows.push(cells);
    }
    rows
}

#[test]
fn project_settings_doc_matches_registries() {
    let doc_buf = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/reference/project-settings.md"),
    )
    .expect("read docs/reference/project-settings.md");
    #[allow(non_snake_case)]
    let DOC = doc_buf.as_str();
    // 1. CanvasAspect::ALL, CanvasSettings::MIN_DIM / MAX_DIM, CANVAS_BASELINES.
    let canvas_sec = section(
        DOC,
        "## Canvas aspects and delivery sizes (`CanvasAspect` and `CanvasSettings`)",
    );
    assert!(
        canvas_sec.contains(&format!(
            "`{}..={}`",
            CanvasSettings::MIN_DIM,
            CanvasSettings::MAX_DIM
        )),
        "project-settings.md must document CanvasSettings::MIN_DIM..=MAX_DIM"
    );
    assert!(
        canvas_sec.contains(&format!("CANVAS_BASELINES = {CANVAS_BASELINES:?}")),
        "project-settings.md must document CANVAS_BASELINES = {CANVAS_BASELINES:?}"
    );
    let aspect_rows = table_rows(canvas_sec);
    let doc_aspects: Vec<(String, String, String, String, bool)> = aspect_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed CanvasAspect row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]) == "yes",
            )
        })
        .collect();
    let expected_aspects: Vec<(String, String, String, String, bool)> = CanvasAspect::ALL
        .iter()
        .map(|&a| {
            let ratio_str = a
                .ratio()
                .map(|(w, h)| format!("{w}:{h}"))
                .unwrap_or_else(|| "-".to_string());
            let px_str = a
                .pixels_for_baseline(1080)
                .map(|[w, h]| format!("{w}x{h}"))
                .unwrap_or_else(|| "-".to_string());
            (
                format!("{a:?}"),
                a.name().to_string(),
                ratio_str,
                px_str,
                a == CanvasAspect::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_aspects, expected_aspects,
        "docs/reference/project-settings.md CanvasAspect table drifted from CanvasAspect::ALL"
    );

    // 2. Sequence frame rate presets (RATES).
    let rates_sec = section(DOC, "## Sequence frame rate presets (`RATES`)");
    let rate_rows = table_rows(rates_sec);
    let doc_rates: Vec<(String, String, i64)> = rate_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed RATES row: {r:?}");
            (
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).parse::<i64>().expect("nominal_fps"),
            )
        })
        .collect();
    let expected_rates: Vec<(String, String, i64)> = RATES
        .iter()
        .map(|&r| {
            (
                format!("{}/{}", r.num, r.den),
                format!("{:.3}", r.as_f64()),
                r.nominal_fps(),
            )
        })
        .collect();
    assert_eq!(
        doc_rates, expected_rates,
        "docs/reference/project-settings.md RATES table drifted from settings_pick::RATES"
    );

    // 3. AudioLayout::ALL.
    let layout_rows = table_rows(section(DOC, "## Audio channel layouts (`AudioLayout`)"));
    let doc_layouts: Vec<(String, String, String, u16, bool)> = layout_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed AudioLayout row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
                unquote(r[3]).parse::<u16>().expect("channels"),
                unquote(r[4]) == "yes",
            )
        })
        .collect();
    let expected_layouts: Vec<(String, String, String, u16, bool)> = AudioLayout::ALL
        .iter()
        .map(|&l| {
            (
                format!("{l:?}"),
                l.name().to_string(),
                l.label().to_string(),
                l.channels(),
                l.is_default(),
            )
        })
        .collect();
    assert_eq!(
        doc_layouts, expected_layouts,
        "docs/reference/project-settings.md AudioLayout table drifted from AudioLayout::ALL"
    );

    // 4. TimeDisplay::ALL & FRAMES_PER_FOOT.
    let time_sec = section(DOC, "## Timecode display formats (`TimeDisplay`)");
    assert!(
        time_sec.contains(&format!(
            "`TimeDisplay::FRAMES_PER_FOOT` is `{}`",
            TimeDisplay::FRAMES_PER_FOOT
        )),
        "project-settings.md must document TimeDisplay::FRAMES_PER_FOOT"
    );
    let time_rows = table_rows(time_sec);
    let doc_times: Vec<(String, String, String, bool)> = time_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed TimeDisplay row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_times: Vec<(String, String, String, bool)> = TimeDisplay::ALL
        .iter()
        .map(|&d| {
            (
                format!("{d:?}"),
                d.name().to_string(),
                d.label().to_string(),
                d.is_default(),
            )
        })
        .collect();
    assert_eq!(
        doc_times, expected_times,
        "docs/reference/project-settings.md TimeDisplay table drifted from TimeDisplay::ALL"
    );

    // 5. MarkerColor::ALL, Theme::marker, and MarkerKind::ALL.
    let marker_sec = section(
        DOC,
        "## Marker colors and kinds (`MarkerColor` and `MarkerKind`)",
    );
    let marker_color_rows = table_rows(marker_sec.split("\n### ").next().unwrap_or(marker_sec));
    let doc_marker_colors: Vec<(String, String, String, String, String)> = marker_color_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed MarkerColor row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]).to_string(),
            )
        })
        .collect();
    let dark_theme = Theme::for_appearance(Appearance::Dark);
    let light_theme = Theme::for_appearance(Appearance::Light);
    let expected_marker_colors: Vec<(String, String, String, String, String)> = MarkerColor::ALL
        .iter()
        .map(|&c| {
            let dark_hsla = dark_theme.marker(c);
            let light_hsla = light_theme.marker(c);
            assert!(
                (dark_hsla.s - 0.68).abs() < 1e-5
                    && (dark_hsla.l - 0.62).abs() < 1e-5
                    && (light_hsla.l - 0.46).abs() < 1e-5,
                "Theme::marker saturation/lightness drifted for {c:?}"
            );
            (
                format!("{c:?}"),
                c.name().to_string(),
                format!("{:?}", c.rgba()),
                format!("{:.2}", dark_hsla.h),
                format!("{:?}", LabelColor::from(c)),
            )
        })
        .collect();
    assert_eq!(
        doc_marker_colors, expected_marker_colors,
        "docs/reference/project-settings.md MarkerColor table drifted from MarkerColor::ALL / Theme::marker"
    );

    let marker_kind_rows = table_rows(section(DOC, "### Marker categories (`MarkerKind`)"));
    let doc_marker_kinds: Vec<(String, String, String, String, bool)> = marker_kind_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 5, "malformed MarkerKind row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]) == "yes",
            )
        })
        .collect();
    let expected_marker_kinds: Vec<(String, String, String, String, bool)> = MarkerKind::ALL
        .iter()
        .map(|&k| {
            (
                format!("{k:?}"),
                k.name().to_string(),
                k.label().to_string(),
                format!("{:?}", k.cycle_next()),
                k.is_default(),
            )
        })
        .collect();
    assert_eq!(
        doc_marker_kinds, expected_marker_kinds,
        "docs/reference/project-settings.md MarkerKind table drifted from MarkerKind::ALL"
    );

    // 6. LabelColor::ALL and clip-type default constants.
    let label_sec = section(DOC, "## Clip and media label colors (`LabelColor`)");
    for (const_name, color) in [
        ("VIDEO", LabelColor::VIDEO),
        ("AUDIO", LabelColor::AUDIO),
        ("GENERATOR", LabelColor::GENERATOR),
        ("NESTED", LabelColor::NESTED),
        ("ADJUSTMENT", LabelColor::ADJUSTMENT),
    ] {
        let token = format!("`LabelColor::{const_name} = LabelColor::{color:?}`");
        assert!(
            label_sec.contains(&token),
            "project-settings.md missing {token}"
        );
    }
    let label_rows = table_rows(label_sec);
    let doc_labels: Vec<(String, String, String, String, String, String)> = label_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 6, "malformed LabelColor row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]).to_string(),
                unquote(r[5]).to_string(),
            )
        })
        .collect();
    let expected_labels: Vec<(String, String, String, String, String, String)> = LabelColor::ALL
        .iter()
        .map(|&l| {
            let alias = match l {
                LabelColor::Rose => "red",
                LabelColor::Mango => "orange",
                LabelColor::Magenta => "pink",
                _ => "-",
            };
            if alias != "-" {
                assert_eq!(LabelColor::from_name(alias), Some(l));
            }
            (
                format!("{l:?}"),
                l.id().to_string(),
                l.name().to_string(),
                format!("{:?}", l.rgb()),
                format!("{:?}", l.selected_rgb()),
                alias.to_string(),
            )
        })
        .collect();
    assert_eq!(
        doc_labels, expected_labels,
        "docs/reference/project-settings.md LabelColor table drifted from LabelColor::ALL"
    );

    // 7. AudioRole::ALL.
    let role_rows = table_rows(section(DOC, "## Audio clip roles (`AudioRole`)"));
    let doc_roles: Vec<(String, String, String)> = role_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 3, "malformed AudioRole row: {r:?}");
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
            )
        })
        .collect();
    let expected_roles: Vec<(String, String, String)> = AudioRole::ALL
        .iter()
        .map(|&r| (format!("{r:?}"), r.id().to_string(), r.label().to_string()))
        .collect();
    assert_eq!(
        doc_roles, expected_roles,
        "docs/reference/project-settings.md AudioRole table drifted from AudioRole::ALL"
    );

    // 8. TrackKind (track_kind_specs()), LaneZone, BakeMedium, and TrackDisplay.
    let track_sec = section(
        DOC,
        "## Timeline track kinds and display heights (`TrackKind` and `TrackDisplay`)",
    );
    for zone in [
        LaneZone::Bus,
        LaneZone::Floor,
        LaneZone::Main,
        LaneZone::Stack,
        LaneZone::Ceiling,
    ] {
        let token = format!("`{zone:?}` (`rank = {}`", zone.rank());
        assert!(
            track_sec.contains(&token),
            "project-settings.md missing LaneZone rank {token}"
        );
    }
    let track_rows = table_rows(track_sec.split("\n### ").next().unwrap_or(track_sec));
    #[allow(clippy::type_complexity)]
    let doc_tracks: Vec<(
        (String, String, String, String, bool, bool, bool, bool),
        (bool, bool, bool, bool, bool, String, u32, String),
    )> = track_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 16, "malformed TrackKind row: {r:?}");
            (
                (
                    unquote(r[0]).to_string(),
                    unquote(r[1]).to_string(),
                    r[2].to_string(),
                    unquote(r[3]).to_string(),
                    unquote(r[4]) == "true",
                    unquote(r[5]) == "true",
                    unquote(r[6]) == "true",
                    unquote(r[7]) == "true",
                ),
                (
                    unquote(r[8]) == "true",
                    unquote(r[9]) == "true",
                    unquote(r[10]) == "true",
                    unquote(r[11]) == "true",
                    unquote(r[12]) == "true",
                    unquote(r[13]).to_string(),
                    unquote(r[14]).parse::<u32>().expect("default_height"),
                    unquote(r[15]).to_string(),
                ),
            )
        })
        .collect();
    #[allow(clippy::type_complexity)]
    let expected_tracks: Vec<(
        (String, String, String, String, bool, bool, bool, bool),
        (bool, bool, bool, bool, bool, String, u32, String),
    )> = track_kind_specs()
        .iter()
        .map(|s| {
            let bake = match BakeMedium::for_lane(s.kind) {
                Ok(m) => format!("{m:?}"),
                Err(_) => "Error".to_string(),
            };
            (
                (
                    format!("{:?}", s.kind),
                    s.id.to_string(),
                    s.label.to_string(),
                    s.default_name.to_string(),
                    s.is_audio,
                    s.is_visual,
                    s.accepts_media,
                    s.supports_transitions,
                ),
                (
                    s.can_be_main,
                    s.carries_audio,
                    s.extracts_audio,
                    s.is_bus,
                    s.agent_creates,
                    format!("{:?}", s.zone),
                    s.default_height,
                    bake,
                ),
            )
        })
        .collect();
    assert_eq!(
        doc_tracks, expected_tracks,
        "docs/reference/project-settings.md TrackKind table drifted from track_kind_specs()"
    );

    let display_sec = section(DOC, "### Track display heights (`TrackDisplay`)");
    assert!(
        display_sec.contains(&format!("DEFAULT_HEIGHT = {}", TrackDisplay::DEFAULT_HEIGHT))
            && display_sec.contains(&format!("BUS_HEIGHT = {}", TrackDisplay::BUS_HEIGHT))
            && display_sec.contains(&format!(
                "HEIGHT = {}..={}",
                TrackDisplay::HEIGHT.start(),
                TrackDisplay::HEIGHT.end()
            )),
        "project-settings.md missing TrackDisplay height constants"
    );
    let preset_rows = table_rows(display_sec);
    let doc_presets: Vec<(String, u32)> = preset_rows
        .iter()
        .map(|r| {
            assert!(r.len() >= 2, "malformed TrackDisplay::PRESETS row: {r:?}");
            (
                r[0].to_string(),
                unquote(r[1]).parse::<u32>().expect("height_px"),
            )
        })
        .collect();
    let expected_presets: Vec<(String, u32)> = TrackDisplay::PRESETS
        .iter()
        .map(|&(label, h)| (label.to_string(), h))
        .collect();
    assert_eq!(
        doc_presets, expected_presets,
        "docs/reference/project-settings.md TrackDisplay::PRESETS table drifted"
    );
}
