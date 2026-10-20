//! Verifies `docs/reference/export-and-subtitles.md` against `VideoCodec`,
//! `aria_encoder` capabilities, `ExportOptions` ladders, `EmptyExport`,
//! `aria_interchange` formats, and `captions` constants.

use aria_core::{Rational, VideoCodec};
use aria_encoder::{AUDIO_CAPABILITY, capability};
use aria_interchange::{FORMATS, MAX_FILE_BYTES, SUBTITLE_FORMATS};
use aria_render::{
    EXPORT_AUDIO_CHANNELS, EXPORT_AUDIO_RATE, EmptyExport, MIN_EXPORT_DIM, PngSequenceEncoder,
};
use aria_transcription::WHISPER_SAMPLE_RATE;

use crate::app::captions::{
    CaptionCommit, MAX_PCM_FRAMES, THREAD_CHOICES, TRANSCRIPT_ANALYZER,
    TRANSCRIPT_ANALYZER_VERSION,
};
use crate::app::export_options::{
    ExportBitrate, ExportGop, ExportOptions, ExportRate, ExportScale,
};


fn unquote(cell: &str) -> &str {
    cell.trim().trim_start_matches('`').trim_end_matches('`')
}

fn section<'a>(doc: &'a str, heading: &str) -> &'a str {
    let start = doc
        .find(heading)
        .unwrap_or_else(|| panic!("docs/reference/export-and-subtitles.md missing heading {heading:?}"));
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

/// **Every export codec, capability, sheet ladder, interchange format, and caption option matches `docs/reference/export-and-subtitles.md`.**
///
/// Closes: undocumented or stale `VideoCodec`, `EncoderCapability`, `AUDIO_CAPABILITY`,
/// `ExportScale`, `ExportRate`, `ExportGop`, `ExportBitrate`, `EmptyExport`,
/// `aria_interchange::FORMATS`, `SUBTITLE_FORMATS`, or `CaptionCommit` member.
/// Not caught: prose explanations outside the reference tables.
#[test]
fn export_and_subtitles_doc_matches_registries() {
    let doc_buf = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/reference/export-and-subtitles.md"),
    )
    .expect("read docs/reference/export-and-subtitles.md");
    #[allow(non_snake_case)]
    let EXPORT_DOC = doc_buf.as_str();
    // 1. VideoCodec::ALL
    for codec in VideoCodec::ALL {
        match codec {
            VideoCodec::H264 | VideoCodec::H265 => {}
        }
    }
    let codec_rows = table_rows(section(EXPORT_DOC, "## Video codecs (`VideoCodec`)"));
    let doc_codecs: Vec<(String, String, String, String, bool)> = codec_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                unquote(r[1]).to_string(),
                r[2].replace('`', ""),
                r[3].to_string(),
                unquote(r[4]) == "yes",
            )
        })
        .collect();
    let expected_codecs: Vec<(String, String, String, String, bool)> = VideoCodec::ALL
        .iter()
        .map(|&c| {
            let alias = match c {
                VideoCodec::H264 => "h264, avc",
                VideoCodec::H265 => "h265, hevc",
            };
            for tok in alias.split(", ") {
                assert_eq!(VideoCodec::from_key(tok), Some(c));
            }
            (
                format!("{c:?}"),
                c.key().to_string(),
                alias.to_string(),
                c.label().to_string(),
                c == VideoCodec::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_codecs, expected_codecs,
        "docs/reference/export-and-subtitles.md VideoCodec table drifted from VideoCodec::ALL"
    );

    // 2. Platform video encoder capability on current target + AUDIO_CAPABILITY + export constants
    let cap_rows = table_rows(section(
        EXPORT_DOC,
        "### Platform video encoders (`aria_encoder::capability`)",
    ));
    assert_eq!(cap_rows.len(), 5, "expected 5 platform encoder rows");
    let host_cap = capability();
    let matching_row = cap_rows
        .iter()
        .find(|r| unquote(r[1]) == host_cap.backend)
        .expect("current host encoder backend must appear in Platform video encoders table");
    assert_eq!(unquote(matching_row[2]), host_cap.container);
    let expected_ext = if host_cap.extension.is_empty() {
        "(empty)"
    } else {
        host_cap.extension
    };
    assert_eq!(unquote(matching_row[3]), expected_ext);
    assert_eq!(unquote(matching_row[4]), host_cap.video);
    assert_eq!(unquote(matching_row[5]), host_cap.audio.unwrap_or("none"));
    assert_eq!(unquote(matching_row[6]), host_cap.available.to_string());
    let expected_codec_list = if host_cap.codecs.is_empty() {
        "none".to_string()
    } else {
        host_cap
            .codecs
            .iter()
            .map(|c| c.label())
            .collect::<Vec<_>>()
            .join(", ")
    };
    assert_eq!(unquote(matching_row[7]), expected_codec_list);
    assert_eq!(unquote(matching_row[8]), host_cap.bitrate.to_string());

    let const_rows = table_rows(section(
        EXPORT_DOC,
        "### Sound-only and frame-sequence sinks and export constants",
    ));
    let doc_consts: Vec<(&str, &str)> = const_rows
        .iter()
        .map(|r| (unquote(r[0]), unquote(r[1])))
        .collect();
    let png_choice = aria_core::EncoderChoice::only("png", Some(false));
    let _: fn(std::path::PathBuf) -> Result<PngSequenceEncoder, aria_core::EncodeError> =
        PngSequenceEncoder::new;
    let audio_label = AUDIO_CAPABILITY.label();
    let png_hw = format!("{:?}", png_choice.hardware);
    let rate_str = EXPORT_AUDIO_RATE.to_string();
    let ch_str = EXPORT_AUDIO_CHANNELS.to_string();
    let min_dim_str = MIN_EXPORT_DIM.to_string();
    let expected_consts: Vec<(&str, &str)> = vec![
        ("AUDIO_CAPABILITY.container", AUDIO_CAPABILITY.container),
        ("AUDIO_CAPABILITY.codec", AUDIO_CAPABILITY.codec),
        ("AUDIO_CAPABILITY.extension", AUDIO_CAPABILITY.extension),
        ("AUDIO_CAPABILITY.label()", audio_label.as_str()),
        ("PngSequenceEncoder::choice().encoder", png_choice.encoder.as_ref()),
        ("PngSequenceEncoder::choice().hardware", png_hw.as_str()),
        ("EXPORT_AUDIO_RATE", rate_str.as_str()),
        ("EXPORT_AUDIO_CHANNELS", ch_str.as_str()),
        ("MIN_EXPORT_DIM", min_dim_str.as_str()),
    ];
    assert_eq!(
        doc_consts, expected_consts,
        "docs/reference/export-and-subtitles.md export constants table drifted"
    );

    // 3. ExportOptions ladders
    let def_opts = ExportOptions::default();

    for scale in ExportScale::ALL {
        match scale {
            ExportScale::Full
            | ExportScale::Threequarter
            | ExportScale::Half
            | ExportScale::Quarter => {}
        }
    }
    let scale_rows = table_rows(section(EXPORT_DOC, "### Output scale (`ExportScale::ALL`)"));
    let doc_scales: Vec<(String, String, String, bool)> = scale_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_scales: Vec<(String, String, String, bool)> = ExportScale::ALL
        .iter()
        .map(|&s| {
            let (num, den) = s.ratio();
            (
                format!("{s:?}"),
                s.label().to_string(),
                format!("{num}/{den}"),
                s == def_opts.scale,
            )
        })
        .collect();
    assert_eq!(
        doc_scales, expected_scales,
        "docs/reference/export-and-subtitles.md ExportScale table drifted from ExportScale::ALL"
    );

    for rate in ExportRate::ALL {
        match rate {
            ExportRate::Project
            | ExportRate::Fps24
            | ExportRate::Fps25
            | ExportRate::Fps30
            | ExportRate::Fps50
            | ExportRate::Fps60 => {}
        }
    }
    let rate_rows = table_rows(section(
        EXPORT_DOC,
        "### Output frame rate (`ExportRate::ALL`)",
    ));
    let doc_rates: Vec<(String, String, String, bool)> = rate_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let sentinel = Rational::new(99, 1001);
    let expected_rates: Vec<(String, String, String, bool)> = ExportRate::ALL
        .iter()
        .map(|&r| {
            let resolved = r.resolve(sentinel);
            let resolved_str = if resolved == sentinel {
                "project".to_string()
            } else {
                format!("{}/{}", resolved.num, resolved.den)
            };
            (
                format!("{r:?}"),
                r.label().to_string(),
                resolved_str,
                r == def_opts.rate,
            )
        })
        .collect();
    assert_eq!(
        doc_rates, expected_rates,
        "docs/reference/export-and-subtitles.md ExportRate table drifted from ExportRate::ALL"
    );

    for gop in ExportGop::ALL {
        match gop {
            ExportGop::Codec | ExportGop::Frames(_) => {}
        }
    }
    let gop_rows = table_rows(section(
        EXPORT_DOC,
        "### Keyframe spacing (`ExportGop::ALL`)",
    ));
    let doc_gops: Vec<(String, String, String, bool)> = gop_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_gops: Vec<(String, String, String, bool)> = ExportGop::ALL
        .iter()
        .map(|&g| {
            let frames_str = g.frames().map_or_else(|| "none".to_string(), |f| f.to_string());
            (
                format!("{g:?}"),
                g.label(),
                frames_str,
                g == def_opts.gop,
            )
        })
        .collect();
    assert_eq!(
        doc_gops, expected_gops,
        "docs/reference/export-and-subtitles.md ExportGop table drifted from ExportGop::ALL"
    );

    for br in ExportBitrate::ALL {
        match br {
            ExportBitrate::Encoder | ExportBitrate::Kbps(_) => {}
        }
    }
    let br_rows = table_rows(section(
        EXPORT_DOC,
        "### Target video bitrate (`ExportBitrate::ALL`)",
    ));
    let doc_brs: Vec<(String, String, String, bool)> = br_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                r[1].to_string(),
                unquote(r[2]).to_string(),
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_brs: Vec<(String, String, String, bool)> = ExportBitrate::ALL
        .iter()
        .map(|&b| {
            let kbps_str = b.kbps().map_or_else(|| "none".to_string(), |k| k.to_string());
            (
                format!("{b:?}"),
                b.label(),
                kbps_str,
                b == def_opts.bitrate,
            )
        })
        .collect();
    assert_eq!(
        doc_brs, expected_brs,
        "docs/reference/export-and-subtitles.md ExportBitrate table drifted from ExportBitrate::ALL"
    );

    // 4. EmptyExport::ALL
    for empty in EmptyExport::ALL {
        match empty {
            EmptyExport::Sequence | EmptyExport::Range | EmptyExport::Silence => {}
        }
    }
    let empty_rows = table_rows(section(
        EXPORT_DOC,
        "### Empty export refusals (`EmptyExport::ALL`)",
    ));
    let doc_empty: Vec<String> = empty_rows
        .iter()
        .map(|r| unquote(r[0]).to_string())
        .collect();
    let expected_empty: Vec<String> = EmptyExport::ALL
        .iter()
        .map(|e| format!("{e:?}"))
        .collect();
    assert_eq!(
        doc_empty, expected_empty,
        "docs/reference/export-and-subtitles.md EmptyExport table drifted from EmptyExport::ALL"
    );

    // 5. Timeline interchange and subtitle formats
    assert!(
        EXPORT_DOC.contains(&format!("`{MAX_FILE_BYTES}`")),
        "docs/reference/export-and-subtitles.md must document MAX_FILE_BYTES = {MAX_FILE_BYTES}"
    );
    let fmt_rows = table_rows(section(
        EXPORT_DOC,
        "### Timeline interchange formats (`FORMATS`)",
    ));
    let doc_fmts: Vec<(&str, &str, &str, String)> = fmt_rows
        .iter()
        .map(|r| (unquote(r[0]), r[1], unquote(r[2]), r[3].replace('`', "")))
        .collect();
    let expected_fmts: Vec<(&str, &str, &str, String)> = FORMATS
        .iter()
        .map(|f| (f.id, f.name, f.extension, f.reads.join(", ")))
        .collect();
    assert_eq!(
        doc_fmts, expected_fmts,
        "docs/reference/export-and-subtitles.md FORMATS table drifted from aria_interchange::FORMATS"
    );

    let sub_rows = table_rows(section(
        EXPORT_DOC,
        "### Subtitle formats (`SUBTITLE_FORMATS`)",
    ));
    let doc_subs: Vec<(&str, &str, &str)> = sub_rows
        .iter()
        .map(|r| (unquote(r[0]), r[1], unquote(r[2])))
        .collect();
    let expected_subs: Vec<(&str, &str, &str)> = SUBTITLE_FORMATS
        .iter()
        .map(|s| (s.id, s.name, s.extension))
        .collect();
    assert_eq!(
        doc_subs, expected_subs,
        "docs/reference/export-and-subtitles.md SUBTITLE_FORMATS table drifted from aria_interchange::SUBTITLE_FORMATS"
    );

    // 6. CaptionCommit::ALL and transcription constants
    for (commit, _, _) in CaptionCommit::ALL {
        match commit {
            CaptionCommit::TextClips | CaptionCommit::Markers => {}
        }
    }
    let commit_rows = table_rows(section(
        EXPORT_DOC,
        "### Caption commit targets (`CaptionCommit::ALL`)",
    ));
    let doc_commits: Vec<(String, &str, &str, bool)> = commit_rows
        .iter()
        .map(|r| {
            (
                unquote(r[0]).to_string(),
                unquote(r[1]),
                r[2],
                unquote(r[3]) == "yes",
            )
        })
        .collect();
    let expected_commits: Vec<(String, &str, &str, bool)> = CaptionCommit::ALL
        .iter()
        .map(|&(variant, id, label)| {
            (
                format!("{variant:?}"),
                id,
                label,
                variant == CaptionCommit::default(),
            )
        })
        .collect();
    assert_eq!(
        doc_commits, expected_commits,
        "docs/reference/export-and-subtitles.md CaptionCommit table drifted from CaptionCommit::ALL"
    );

    let tx_rows = table_rows(section(EXPORT_DOC, "### Transcription constants"));
    let doc_tx: Vec<(&str, &str)> = tx_rows
        .iter()
        .map(|r| (unquote(r[0]), unquote(r[1])))
        .collect();
    let whisper_rate = WHISPER_SAMPLE_RATE.to_string();
    let max_pcm = MAX_PCM_FRAMES.to_string();
    let threads_str = THREAD_CHOICES
        .iter()
        .map( usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let ver_str = TRANSCRIPT_ANALYZER_VERSION.to_string();
    let expected_tx: Vec<(&str, &str)> = vec![
        ("WHISPER_SAMPLE_RATE", whisper_rate.as_str()),
        ("MAX_PCM_FRAMES", max_pcm.as_str()),
        ("THREAD_CHOICES", threads_str.as_str()),
        ("TRANSCRIPT_ANALYZER", TRANSCRIPT_ANALYZER),
        ("TRANSCRIPT_ANALYZER_VERSION", ver_str.as_str()),
    ];
    assert_eq!(
        doc_tx, expected_tx,
        "docs/reference/export-and-subtitles.md Transcription constants table drifted"
    );
}
