//! The export sheet's rows are one grid, and every choice on it is a chip that
//! sets that choice.
//!
//! Two classes are closed here:
//!
//! - Two row shapes on one sheet. Every row, a chip row or a fact, stands one
//!   chip tall at one pitch, its label where the title starts, and a chip's
//!   label on the same text edge as a fact's value.
//! - A choice the sheet draws but cannot reach, or a chip that sets another
//!   choice or another row's field. Each row's chips are enumerated from the
//!   option's own `ALL`, so a choice added later is pressed without an edit
//!   here, and each press is checked against the field it sets.
//!
//! Every row the sheet draws is found by its label, so a row added to the
//! sheet joins the grid check without an edit here. Not covered: which chip is
//! lit, which `components::chip` draws from the flag `chip_line` passes; and the
//! in-flight render rows, which are drawn by the loaders and only while a job
//! runs.

use gpui::{TestAppContext, VisualTestContext, px};

use crate::app::export_options::{
    AUDIO_CHOICES, ExportBitrate, ExportGop, ExportRate, ExportScale, WHOLE_TIMELINE,
};
use crate::app::{AppState, DialogKind};
use crate::interned::intern;
use crate::test_support::drawn_shell;

const LABELS: [&str; 11] = [
    "Encoder",
    "Canvas",
    "Format",
    "Scale",
    "Frame rate",
    "Codec",
    "Bitrate",
    "Keyframes",
    "Audio",
    "Range",
    "Destination",
];

const EPS: f32 = 0.51;

/// Open the export sheet once the codec probe has answered, so the Codec row
/// is drawn in its settled shape.
fn open_sheet(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) {
    aria_encoder::available_codecs();
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Export);
            cx.notify();
        });
    });
    visual.run_until_parked();
}

/// Press the chip drawn under `selector`.
fn press(visual: &mut VisualTestContext, selector: &str) {
    let bounds = visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the export sheet draws no {selector}"));
    visual.simulate_click(bounds.center(), gpui::Modifiers::none());
    visual.run_until_parked();
}

#[gpui::test]
fn every_export_row_stands_on_one_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "export-rows");
    open_sheet(&state, visual);
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the export sheet draws a card");
    let chip = visual
        .debug_bounds(intern(&format!(
            "export-scale-{}",
            ExportScale::Full.label()
        )))
        .expect("the Scale row draws its chips");
    let mut lines: Vec<(&str, gpui::Bounds<gpui::Pixels>)> = LABELS
        .iter()
        .filter_map(|&label| {
            visual
                .debug_bounds(intern(&format!("export-line-{label}")))
                .map(|bounds| (label, bounds))
        })
        .collect();
    assert!(
        lines.len() >= 9,
        "the sheet draws its rows, found {:?}",
        lines.iter().map(|(label, _)| *label).collect::<Vec<_>>()
    );
    lines.sort_by(|a, b| a.1.top().partial_cmp(&b.1.top()).unwrap());
    let (first_label, first) = lines[0];
    for (label, line) in &lines {
        assert!(
            (line.size.height - chip.size.height).abs() < px(EPS),
            "{label} is {:?} tall, a chip {:?}",
            line.size.height,
            chip.size.height
        );
        assert!(
            (line.left() - first.left()).abs() < px(EPS)
                && (line.right() - first.right()).abs() < px(EPS),
            "{label} spans {:?}..{:?}, {first_label} {:?}..{:?}",
            line.left(),
            line.right(),
            first.left(),
            first.right()
        );
    }
    let pitch = lines[1].1.top() - lines[0].1.top();
    for pair in lines.windows(2) {
        let step = pair[1].1.top() - pair[0].1.top();
        assert!(
            (step - pitch).abs() < px(EPS),
            "{} follows {} by {step:?}, the sheet's pitch is {pitch:?}",
            pair[1].0,
            pair[0].0
        );
    }
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let inside = |run: &&gpui::TextRunLayout| card.contains(&run.bounds.center());
    let run = |text: &str| {
        runs.iter()
            .filter(inside)
            .filter(|run| run.text.as_ref() == text)
            .min_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap())
            .unwrap_or_else(|| panic!("the sheet draws {text}"))
            .clone()
    };
    // The labels start where the sheet's title starts.
    let title = run("Export");
    for (label, _) in &lines {
        let label_run = run(label);
        assert!(
            (label_run.bounds.left() - title.bounds.left()).abs() < px(EPS),
            "{label} starts at {:?}, the title at {:?}",
            label_run.bounds.left(),
            title.bounds.left()
        );
    }
    // A chip's label stands on the text edge a fact's value starts at.
    let fact = visual
        .debug_bounds("export-row-Canvas")
        .expect("the Canvas row is a fact");
    let chip_text = run(ExportScale::Full.label());
    assert!(
        (chip_text.bounds.left() - fact.left()).abs() < px(EPS),
        "the first chip's label starts at {:?}, a fact's value at {:?}",
        chip_text.bounds.left(),
        fact.left()
    );
    // Each row's value stands on its label's line, a fact's as a chip's.
    let encoder = visual.read(|app| state.read(app).export_encoder_label());
    for (label, value) in [
        ("Encoder", encoder.as_str()),
        ("Scale", ExportScale::Full.label()),
    ] {
        let (label_run, value_run) = (run(label), run(value));
        assert!(
            (label_run.bounds.top() - value_run.bounds.top()).abs() < px(EPS),
            "{label} stands at {:?}, its value {value} at {:?}",
            label_run.bounds.top(),
            value_run.bounds.top()
        );
    }
}

/// Each chip of each always-drawn row sets its own choice, pressed in reverse
/// order so the first press moves the field off its default.
#[gpui::test]
fn every_chip_sets_the_choice_it_names(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "export-chips");
    open_sheet(&state, visual);
    let options =
        |visual: &mut VisualTestContext| visual.read(|app| state.read(app).export_options.clone());

    for scale in ExportScale::ALL.into_iter().rev() {
        press(visual, &format!("export-scale-{}", scale.label()));
        assert_eq!(options(visual).scale, scale, "{}", scale.label());
    }
    for rate in ExportRate::ALL.into_iter().rev() {
        press(visual, &format!("export-rate-{}", rate.label()));
        assert_eq!(options(visual).rate, rate, "{}", rate.label());
    }
    for gop in ExportGop::ALL.into_iter().rev() {
        press(visual, &format!("export-gop-{}", gop.label()));
        assert_eq!(options(visual).gop, gop, "{}", gop.label());
    }
    for (audio, label) in AUDIO_CHOICES.into_iter().rev() {
        press(visual, &format!("export-audio-{label}"));
        assert_eq!(options(visual).audio, audio, "{label}");
    }

    let encoder = aria_encoder::capability();
    let bitrate_chips = visual
        .debug_bounds(intern(&format!(
            "export-bitrate-{}",
            ExportBitrate::Encoder.label()
        )))
        .is_some();
    assert_eq!(
        bitrate_chips, encoder.bitrate,
        "the Bitrate row offers the ladder exactly where the encoder honors one"
    );
    if encoder.bitrate {
        for bitrate in ExportBitrate::ALL.into_iter().rev() {
            press(visual, &format!("export-bitrate-{}", bitrate.label()));
            assert_eq!(options(visual).bitrate, bitrate, "{}", bitrate.label());
        }
    }

    // The Codec row offers what the probe found and the codec in force, and
    // nothing else; with one of them it states it instead.
    let probed = aria_encoder::probed_codecs().expect("the probe answered");
    let chosen = options(visual).codec;
    let offered: Vec<aria_core::VideoCodec> = aria_core::VideoCodec::ALL
        .into_iter()
        .filter(|codec| probed.contains(codec) || *codec == chosen)
        .collect();
    for codec in aria_core::VideoCodec::ALL {
        let drawn = visual
            .debug_bounds(intern(&format!("export-codec-{}", codec.label())))
            .is_some();
        assert_eq!(
            drawn,
            offered.len() > 1 && offered.contains(&codec),
            "{} chip on a machine that writes {probed:?}, {} chosen",
            codec.label(),
            chosen.label()
        );
    }
    // A press on a codec the probe found keeps every other found one offered;
    // the chosen codec the probe did not find leaves the row once another is
    // chosen, so it is not pressed.
    if offered.len() > 1 {
        for &codec in offered.iter().rev().filter(|codec| probed.contains(codec)) {
            press(visual, &format!("export-codec-{}", codec.label()));
            assert_eq!(options(visual).codec, codec, "{}", codec.label());
        }
    }

    assert!(
        !options(visual).marked_range,
        "no chip above the Range row sets the range"
    );
    assert_eq!(
        visual.read(|app| state.read(app).dialog),
        DialogKind::Export
    );
}

/// The Range row: the marked choice is refused with a sentence until a range
/// is marked, then names the span it renders and sets it.
#[gpui::test]
fn the_range_chips_render_the_marked_span_once_one_is_marked(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "export-range");
    open_sheet(&state, visual);
    let marked = |visual: &mut VisualTestContext| {
        visual.read(|app| state.read(app).export_options.marked_range)
    };

    press(visual, "export-range-Marked range");
    assert!(!marked(visual));
    assert!(
        visual
            .read(|app| state.read(app).last_error.clone())
            .is_some_and(|err| err.contains("No range is marked")),
        "pressing the marked range with nothing marked says how to mark one"
    );

    let label = visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.mark_in = Some(10);
            state.mark_out = Some(40);
            cx.notify();
            state.marked_range_label()
        })
    });
    visual.run_until_parked();
    assert_ne!(label, "Marked range");
    press(visual, &format!("export-range-{label}"));
    assert!(marked(visual));
    assert_eq!(
        visual.read(|app| state.read(app).export_range_label()),
        label
    );
    press(visual, &format!("export-range-{WHOLE_TIMELINE}"));
    assert!(!marked(visual));
}

/// The Format row lists the platform encoder and each mounted plugin encoder
/// as chips; a press selects that encoder, and the rows it governs follow.
#[gpui::test]
fn the_format_chips_select_the_encoder(cx: &mut TestAppContext) {
    use crate::test_support::NullEncoder;
    use aria_plugin::EncoderSpec;

    let (state, visual) = drawn_shell(cx, "export-format");
    let _gif = visual.update(|_, app| {
        state.update(app, |state, _| {
            state
                .ctx
                .register_encoder(
                    EncoderSpec {
                        id: "anim.gif".into(),
                        name: "Animated GIF".into(),
                        extension: "gif".into(),
                        audio: false,
                    },
                    NullEncoder,
                )
                .expect("register gif")
        })
    });
    open_sheet(&state, visual);
    let platform = visual.read(|app| state.read(app).export_format_label());
    assert!(
        visual.debug_bounds("export-row-Format").is_none(),
        "with two encoders the Format row is chips, not a fact"
    );

    press(visual, "export-format-Animated GIF (.gif)");
    assert_eq!(
        visual.read(|app| state.read(app).export_options.plugin_encoder.clone()),
        Some("anim.gif".to_string())
    );
    for gone in ["Codec", "Bitrate", "Audio"] {
        assert!(
            visual
                .debug_bounds(intern(&format!("export-line-{gone}")))
                .is_none(),
            "the {gone} row stays drawn for an encoder it does not govern"
        );
    }

    press(visual, &format!("export-format-{platform}"));
    assert_eq!(
        visual.read(|app| state.read(app).export_options.plugin_encoder.clone()),
        None
    );
    assert!(
        visual.debug_bounds("export-line-Codec").is_some(),
        "the platform encoder's Codec row returns"
    );
}
