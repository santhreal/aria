//! Verifies `docs/reference/scopes.md` against `WaveformMode`, `WaveformTrace`,
//! `WaveformChannel`, `ScopeConfig`, and `aria_ui::scopes` graticule constants.

use aria_analysis::scopes::luma709;
use aria_analysis::{
    MAX_SCOPE_SAMPLES, MAX_VECTORSCOPE_SIZE, MAX_WAVEFORM_COLUMNS, SCOPE_BINS, ScopeConfig,
    WaveformChannel, WaveformTrace, chroma_codes709, vectorscope_cell,
};

use crate::scopes::{
    HISTOGRAM_H, HISTOGRAM_W, SCOPE_PX, SKIN_TONE_DEGREES, TARGET_COLOURS, TARGET_LEVELS,
    WaveformMode, channel_colour,
};

fn unquote(cell: &str) -> &str {
    cell.trim().trim_start_matches('`').trim_end_matches('`')
}

fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("docs/reference/scopes.md missing heading {heading:?}"));
    let body = &doc[start + heading.len()..];
    let body = body.strip_prefix('\n').unwrap_or(body);
    let level = heading.bytes().take_while(|&b| b == b'#').count();
    let mut offset = 0usize;
    for line in body.lines() {
        let hashes = line.bytes().take_while(|&b| b == b'#').count();
        if hashes > 0 && hashes <= level && line.as_bytes().get(hashes) == Some(&b' ') {
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

/// **Every scope mode, waveform trace, channel, configuration bound, and graticule constant matches `docs/reference/scopes.md`.**
///
/// Closes: undocumented or stale `WaveformMode`, `WaveformTrace`, `WaveformChannel`,
/// `ScopeConfig` bound or default, or vectorscope graticule constant / target colour.
/// Not caught: prose explanations outside the tables.
#[test]
fn scopes_doc_matches_registries() {
    let doc_buf = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/reference/scopes.md"),
    )
    .expect("read docs/reference/scopes.md");
    #[allow(non_snake_case)]
    let SCOPES_DOC = doc_buf.as_str();
    // 1. WaveformMode::ALL & WaveformTrace::ALL
    for mode in WaveformMode::ALL {
        match mode {
            WaveformMode::Luma | WaveformMode::RgbParade | WaveformMode::YcbcrParade => {}
        }
    }
    for trace in WaveformTrace::ALL {
        match trace {
            WaveformTrace::Luma | WaveformTrace::RgbParade | WaveformTrace::YcbcrParade => {}
        }
    }
    assert_eq!(WaveformMode::ALL.len(), WaveformTrace::ALL.len());

    let mode_rows = table_rows(section(
        SCOPES_DOC,
        "## Waveform modes and traces (`WaveformMode` and `WaveformTrace`)",
    ));
    let doc_modes: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
    )> = mode_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].to_string(),
                r[3].to_string(),
                unquote(r[4]).to_string(),
                unquote(r[5]).to_string(),
                unquote(r[6]).to_string(),
                unquote(r[7]).to_string(),
                unquote(r[8]) == "yes",
            )
        })
        .collect();

    let expected_modes: Vec<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        bool,
    )> = WaveformMode::ALL
        .iter()
        .zip(WaveformTrace::ALL.iter())
        .map(|(&mode, &trace)| {
            assert_eq!(mode.trace(), trace);
            let serde_val = serde_json::to_value(mode).expect("serialize WaveformMode");
            let serde_name = serde_val.as_str().expect("string").to_string();
            let channels_str = trace
                .channels()
                .iter()
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            (
                format!("{mode:?}"),
                serde_name,
                mode.label().to_string(),
                mode.short_label().to_string(),
                mode.id().to_string(),
                mode.menu_id().to_string(),
                format!("{trace:?}"),
                channels_str,
                mode == WaveformMode::default() && trace == WaveformTrace::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_modes, expected_modes,
        "docs/reference/scopes.md WaveformMode/WaveformTrace table drifted"
    );

    // 2. WaveformChannel
    let all_channels = [
        WaveformChannel::Luma,
        WaveformChannel::Red,
        WaveformChannel::Green,
        WaveformChannel::Blue,
        WaveformChannel::Cb,
        WaveformChannel::Cr,
    ];
    for ch in all_channels {
        match ch {
            WaveformChannel::Luma
            | WaveformChannel::Red
            | WaveformChannel::Green
            | WaveformChannel::Blue
            | WaveformChannel::Cb
            | WaveformChannel::Cr => {}
        }
    }
    let ch_rows = table_rows(section(
        SCOPES_DOC,
        "## Waveform channels (`WaveformChannel`)",
    ));
    let doc_channels: Vec<(String, String, u8)> = ch_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                unquote(r[2]).parse::<u8>().expect("gray code"),
            )
        })
        .collect();
    let (grey_cb, grey_cr) = chroma_codes709(128, 128, 128);
    let expected_channels: Vec<(String, String, u8)> = all_channels
        .iter()
        .map(|&ch| {
            let grey = match ch {
                WaveformChannel::Luma => luma709(128, 128, 128),
                WaveformChannel::Red | WaveformChannel::Green | WaveformChannel::Blue => 128,
                WaveformChannel::Cb => grey_cb,
                WaveformChannel::Cr => grey_cr,
            };
            (format!("{ch:?}"), format!("{:?}", channel_colour(ch)), grey)
        })
        .collect();
    assert_eq!(
        doc_channels, expected_channels,
        "docs/reference/scopes.md WaveformChannel table drifted"
    );

    // 3. ScopeConfig bounds and defaults
    let cfg_rows = table_rows(section(
        SCOPES_DOC,
        "## Scope configuration and bounds (`ScopeConfig`)",
    ));
    let doc_cfg: Vec<(&str, &str)> = cfg_rows
        .iter()
        .map(|r| (unquote(r[0]), unquote(r[1])))
        .collect();
    let def_cfg = ScopeConfig::default();
    let bins_s = SCOPE_BINS.to_string();
    let max_cols_s = MAX_WAVEFORM_COLUMNS.to_string();
    let max_vec_s = MAX_VECTORSCOPE_SIZE.to_string();
    let max_samp_s = MAX_SCOPE_SAMPLES.to_string();
    let def_cols_s = def_cfg.waveform_columns().to_string();
    let def_vec_s = def_cfg.vectorscope_size().to_string();
    let def_samp_s = def_cfg.maximum_samples().to_string();
    let def_trace_s = format!("{:?}", def_cfg.waveform_trace());
    let expected_cfg: Vec<(&str, &str)> = vec![
        ("SCOPE_BINS", bins_s.as_str()),
        ("MAX_WAVEFORM_COLUMNS", max_cols_s.as_str()),
        ("MAX_VECTORSCOPE_SIZE", max_vec_s.as_str()),
        ("MAX_SCOPE_SAMPLES", max_samp_s.as_str()),
        (
            "ScopeConfig::default().waveform_columns()",
            def_cols_s.as_str(),
        ),
        (
            "ScopeConfig::default().vectorscope_size()",
            def_vec_s.as_str(),
        ),
        (
            "ScopeConfig::default().maximum_samples()",
            def_samp_s.as_str(),
        ),
        (
            "ScopeConfig::default().waveform_trace()",
            def_trace_s.as_str(),
        ),
    ];
    assert_eq!(
        doc_cfg, expected_cfg,
        "docs/reference/scopes.md ScopeConfig table drifted"
    );

    // 4. Bitmap and graticule constants + TARGET_COLOURS
    let const_rows = table_rows(section(SCOPES_DOC, "### Bitmap and graticule constants"));
    let doc_consts: Vec<(&str, &str)> = const_rows
        .iter()
        .map(|r| (unquote(r[0]), unquote(r[1])))
        .collect();
    let scope_px_s = SCOPE_PX.to_string();
    let hist_w_s = HISTOGRAM_W.to_string();
    let hist_h_s = HISTOGRAM_H.to_string();
    let skin_s = format!("{SKIN_TONE_DEGREES:.1}");
    let levels_s = TARGET_LEVELS
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let expected_consts: Vec<(&str, &str)> = vec![
        ("SCOPE_PX", scope_px_s.as_str()),
        ("HISTOGRAM_W", hist_w_s.as_str()),
        ("HISTOGRAM_H", hist_h_s.as_str()),
        ("SKIN_TONE_DEGREES", skin_s.as_str()),
        ("TARGET_LEVELS", levels_s.as_str()),
    ];
    assert_eq!(
        doc_consts, expected_consts,
        "docs/reference/scopes.md Bitmap and graticule constants table drifted"
    );

    let target_rows = table_rows(section(
        SCOPES_DOC,
        "### Vectorscope color targets (`TARGET_COLOURS`)",
    ));
    let doc_targets: Vec<(usize, String, String, String)> = target_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).parse::<usize>().expect("index"),
                unquote(r[2]).to_string(),
                unquote(r[3]).to_string(),
                unquote(r[4]).to_string(),
            )
        })
        .collect();
    let grid_size = ScopeConfig::default().vectorscope_size();
    let expected_targets: Vec<(usize, String, String, String)> = TARGET_COLOURS
        .iter()
        .enumerate()
        .map(|(idx, &[r, g, b])| {
            let scale = |code: u8, level: u8| if code == 0 { 0 } else { level };
            let cell_75 = vectorscope_cell(
                scale(r, TARGET_LEVELS[0]),
                scale(g, TARGET_LEVELS[0]),
                scale(b, TARGET_LEVELS[0]),
                grid_size,
            );
            let cell_100 = vectorscope_cell(
                scale(r, TARGET_LEVELS[1]),
                scale(g, TARGET_LEVELS[1]),
                scale(b, TARGET_LEVELS[1]),
                grid_size,
            );
            (
                idx,
                format!("{:?}", [r, g, b]),
                format!("{cell_75:?}"),
                format!("{cell_100:?}"),
            )
        })
        .collect();
    assert_eq!(
        doc_targets, expected_targets,
        "docs/reference/scopes.md TARGET_COLOURS table drifted"
    );
}
