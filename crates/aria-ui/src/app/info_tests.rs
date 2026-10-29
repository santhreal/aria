//! Contracts for the Info panel's readings.
//!
//! Two classes closed here. A colour shown for a pixel that is not the one
//! under the pointer: a frame rendered below canvas size read at canvas
//! coordinates, a point off the picture read at all, a GPU-only frame never
//! read, a reading that arrives after the pointer or the frame moved on shown
//! anyway, or frames read for a panel nobody has open. And a frame cost that is
//! paid and never stated: a stage of the engine's catalog with no row, a pass
//! count stated for a frame that drew none, a status line that disagrees with
//! the panel, or a playback verdict for a run that never ran. Not covered: the
//! drawn panel, which the shell tests open.

use std::sync::Arc;

use aria_core::{Cancel, RgbaImage};
use aria_models::RationalTime;
use gpui::GpuDevice;

use super::*;
use crate::app::canvas_overlay::Rect;
use crate::app::premiere_tests::with_clip;

/// A colour unique to texel `(x, y)` of any frame under 4096 pixels a side.
fn encoded(x: u32, y: u32) -> [u8; 4] {
    [
        (x % 256) as u8,
        (y % 256) as u8,
        ((x / 256) * 16 + y / 256) as u8,
        255,
    ]
}

/// A frame `size` texels across whose every texel is [`encoded`].
fn picture([width, height]: [u32; 2]) -> RgbaImage {
    let pixels = (0..height)
        .flat_map(|y| (0..width).flat_map(move |x| encoded(x, y)))
        .collect();
    RgbaImage::new(width, height, pixels)
}

/// A session with one selected clip of a 64x48 picture whose every texel
/// differs, so a reading of the wrong texel cannot match. The canvas is at
/// least that size.
fn gradient() -> (AppState, aria_models::ClipId) {
    let asset = crate::scratch_pool::scratch("info-probe").join("gradient.png");
    let png = aria_render::encode_png(&picture([64, 48])).expect("encode the picture");
    std::fs::write(&asset, png).expect("write the picture");
    crate::app::premiere_tests::with_media(asset)
}

/// Where the canvas is drawn in the window: offset, at half size.
fn drawn(canvas: [u32; 2]) -> Rect {
    Rect::new(40.0, 30.0, canvas[0] as f32 / 2.0, canvas[1] as f32 / 2.0)
}

/// The window point inside canvas pixel `at` when the canvas is drawn at
/// `rect`, at half size.
fn window_at(rect: Rect, at: [u32; 2]) -> [f32; 2] {
    [
        rect.x + (at[0] as f32 + 0.25) * 0.5,
        rect.y + (at[1] as f32 + 0.25) * 0.5,
    ]
}

/// A session showing a CPU frame a quarter of canvas size at tick 3, the
/// canvas drawn at [`drawn`]; the Info tab behind another.
fn shown() -> (AppState, [u32; 2], Rect) {
    let (mut state, _) = gradient();
    let canvas = state.canvas_size();
    let rect = drawn(canvas);
    state.overlay_frame.canvas = Some(rect);
    state.last_frame = Some(Arc::new(picture(canvas.map(|side| side / 4))));
    state.shown_tick = Some(3);
    // Info opens beside Effects & Presets, which stays in front.
    state.dock_tree.open(crate::dock::PANEL_INFO);
    state.dock_tree.activate(crate::dock::PANEL_EFFECTS);
    (state, canvas, rect)
}

#[test]
fn the_pointer_reads_the_texel_of_the_frame_on_screen_that_covers_it() {
    let (mut state, canvas, rect) = shown();
    state.focus_dock_panel(crate::dock::PANEL_INFO);
    assert!(state.dock_tree.is_active(crate::dock::PANEL_INFO));
    let [width, height] = canvas;
    for at in [
        [0, 0],
        [7, 5],
        [width / 3, height / 2],
        [width - 1, height - 1],
    ] {
        assert!(
            state.point_viewer(Some(window_at(rect, at))),
            "a new pixel under the pointer did not repaint Info"
        );
        let readout = state.info_readout();
        assert_eq!(readout.pointer, Some(at));
        // The frame is a quarter of canvas size: canvas pixel (x, y) is texel
        // (x / 4, y / 4).
        assert_eq!(
            readout.color,
            Some(encoded(at[0] / 4, at[1] / 4)),
            "color at canvas pixel {at:?}"
        );
        assert!(
            !state.point_viewer(Some(window_at(rect, at))),
            "the same pixel repainted Info"
        );
    }
}

#[test]
fn a_point_off_the_picture_reads_nothing() {
    let (mut state, canvas, rect) = shown();
    state.focus_dock_panel(crate::dock::PANEL_INFO);
    assert!(state.point_viewer(Some(window_at(rect, [1, 1]))));
    for outside in [
        [rect.x - 0.5, rect.y + 1.0],
        [rect.x + 1.0, rect.y - 0.5],
        [rect.x + rect.w, rect.y + 1.0],
        [rect.x + 1.0, rect.y + rect.h],
    ] {
        state.point_viewer(Some(window_at(rect, [1, 1])));
        assert!(
            state.point_viewer(Some(outside)),
            "{outside:?} kept a reading"
        );
        let readout = state.info_readout();
        assert_eq!(
            (readout.pointer, readout.color),
            (None, None),
            "{outside:?}"
        );
    }
    state.point_viewer(Some(window_at(rect, [canvas[0] / 2, 0])));
    assert!(
        state.point_viewer(None),
        "leaving the Viewer kept a reading"
    );
    assert_eq!(state.info_readout().pointer, None);
}

#[test]
fn nothing_is_read_while_info_is_not_shown() {
    let (mut state, _, rect) = shown();
    assert!(!state.dock_tree.is_active(crate::dock::PANEL_INFO));
    assert!(
        !state.point_viewer(Some(window_at(rect, [9, 9]))),
        "a panel behind another tab repainted"
    );
    assert_eq!(
        state.viewer_probe.color, None,
        "a panel behind another tab read a pixel"
    );
    // Bringing it forward reads on the next move.
    state.focus_dock_panel(crate::dock::PANEL_INFO);
    assert!(state.point_viewer(Some(window_at(rect, [10, 9]))));
    assert_eq!(state.info_readout().color, Some(encoded(2, 2)));
}

#[test]
fn a_reading_for_another_pixel_or_frame_is_dropped() {
    let (mut state, _, rect) = shown();
    state.focus_dock_panel(crate::dock::PANEL_INFO);
    // A GPU-only frame: the reading comes back from the worker.
    state.last_frame = None;
    state.point_viewer(Some(window_at(rect, [20, 12])));
    let rgba = [1, 2, 3, 255];
    for (at, tick) in [([21, 12], 3), ([20, 12], 2), ([20, 12], 4)] {
        assert!(
            !state.adopt_probe(Some(Probed { at, tick, rgba })),
            "kept a reading of {at:?} at tick {tick}"
        );
        assert_eq!(state.info_readout().color, None);
    }
    assert!(state.adopt_probe(Some(Probed {
        at: [20, 12],
        tick: 3,
        rgba
    })));
    assert_eq!(state.info_readout().color, Some(rgba));
    // A move drops the reading of the pixel left behind.
    state.point_viewer(Some(window_at(rect, [30, 12])));
    assert_eq!(state.info_readout().color, None);
}

#[test]
fn texel_of_maps_canvas_pixels_onto_any_frame_size() {
    let canvas = [1920, 1080];
    for (at, frame, texel) in [
        ([0, 0], [480, 270], [0, 0]),
        ([1919, 1079], [480, 270], [479, 269]),
        ([1919, 1079], [1920, 1080], [1919, 1079]),
        ([960, 540], [1280, 720], [640, 360]),
        ([5, 5], [1, 1], [0, 0]),
        ([1919, 1079], [3840, 2160], [3838, 2158]),
    ] {
        assert_eq!(texel_of(at, canvas, frame), texel, "{at:?} on {frame:?}");
    }
}

/// The reading of a GPU-only frame equals the CPU frame rendered from the same
/// time at the same size, and after a burst of moves it is the reading of the
/// last pixel the pointer reached.
#[gpui::test]
async fn a_gpu_frame_reads_what_the_cpu_frame_shows(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = gradient();
    let Some(gpu) = crate::test_support::mounted_gpu(&state.ctx) else {
        // The render plugin only logs why it mounted no device: bring-up is
        // tried again so the skip states the reason.
        let reason = crate::app::render_device::renderer_on(None, "the Info probe").err();
        eprintln!("skipping: no headless GPU available: {reason:?}");
        return;
    };
    let fit = Some((320, 180));
    let time = RationalTime::new(0, state.playhead.rate);
    // The CPU frame first: until the window's device is shared, the worker
    // reads every frame back. Scopes do not force a read-back, because a
    // texture frame is measured on the device.
    let on_cpu = state
        .preview_worker
        .frame(&executor, time, fit, Cancel::never())
        .await
        .expect("worker alive")
        .expect("frame rendered");
    assert!(
        on_cpu.texture.is_none() && !on_cpu.rgba.pixels.is_empty(),
        "a worker with no shared device presented a texture"
    );
    state.preview_worker.set_gpu_device(Some(GpuDevice::new((
        gpu.instance.clone(),
        gpu.adapter.clone(),
        gpu.device.clone(),
        gpu.queue.clone(),
    ))));
    let on_gpu = state
        .preview_worker
        .frame(&executor, time, fit, Cancel::never())
        .await
        .expect("worker alive")
        .expect("frame rendered");
    assert!(
        on_gpu.texture.is_some(),
        "the shared device rendered no texture"
    );

    state
        .preview_worker
        .set_shown_texture(on_gpu.texture.clone());
    state.last_frame = None;
    state.shown_tick = Some(on_gpu.tick);
    let canvas = state.canvas_size();
    let rect = drawn(canvas);
    state.overlay_frame.canvas = Some(rect);
    state.focus_dock_panel(crate::dock::PANEL_INFO);
    let frame = [on_cpu.rgba.width, on_cpu.rgba.height];
    let [width, height] = canvas;
    let points = [
        [width / 2, height / 2],
        [width / 5, height / 3],
        [width - 1, height - 1],
        [0, 0],
    ];
    for at in points {
        // Each point read alone.
        state.point_viewer(Some(window_at(rect, at)));
        assert_eq!(state.preview_worker.run(&executor, || ()).await, Some(()));
        state.take_preview_mail();
        let [x, y] = texel_of(at, canvas, frame);
        let want = on_cpu.rgba.pixel(x, y);
        let got = state
            .info_readout()
            .color
            .unwrap_or_else(|| panic!("no reading at {at:?}"));
        for channel in 0..4 {
            assert!(
                got[channel].abs_diff(want[channel]) <= 1,
                "GPU reading {got:?} at {at:?}, CPU frame {want:?}"
            );
        }
    }
    // A burst of moves before the worker runs: the reading kept is the last
    // pixel's.
    for at in points {
        state.point_viewer(Some(window_at(rect, at)));
    }
    assert_eq!(state.preview_worker.run(&executor, || ()).await, Some(()));
    state.take_preview_mail();
    let last = points[points.len() - 1];
    assert_eq!(state.info_readout().pointer, Some(last));
    let [x, y] = texel_of(last, canvas, frame);
    let want = on_cpu.rgba.pixel(x, y);
    let got = state.info_readout().color.expect("the last pixel was read");
    assert!(
        (0..4).all(|channel| got[channel].abs_diff(want[channel]) <= 1),
        "burst reading {got:?}, CPU frame {want:?}"
    );

    // Exposure measures the GPU-only frame as it measures the CPU copy.
    state.measure_exposure();
    let on_screen = state
        .media_reach
        .exposure
        .unwrap_or_else(|| panic!("no exposure: {:?}", state.status_message));
    state.last_frame = Some(Arc::clone(&on_cpu.rgba));
    state.measure_exposure();
    let copied = state
        .media_reach
        .exposure
        .expect("the CPU frame was measured");
    assert!(
        (on_screen.mean_luma - copied.mean_luma).abs() < 0.5,
        "GPU-only frame {on_screen:?}, CPU frame {copied:?}"
    );
}

#[test]
fn the_selected_clip_and_a_trim_read_back() {
    let (mut state, clip) = with_clip();
    let readout = state.info_readout();
    let ui = state.ui_clip(clip).expect("the clip");
    let rate = state.store.sequence.frame_rate;
    let row = |label: &str| {
        readout
            .clip
            .iter()
            .find(|row| row.label == label)
            .map(|row| row.value.clone())
            .unwrap_or_else(|| panic!("no {label} row in {:?}", readout.clip))
    };
    assert_eq!(row("Name"), ui.name.to_string());
    assert_eq!(
        row("In"),
        format_ticks(ui.timeline_start_ticks, rate.num, rate.den)
    );
    assert_eq!(
        row("Out"),
        format_ticks(ui.timeline_end_ticks, rate.num, rate.den)
    );
    assert_eq!(row("Duration"), ui.duration_label.to_string());
    let media = state
        .store
        .media
        .iter()
        .find(|item| Some(item.id) == ui.media_id)
        .expect("the clip's media");
    assert_eq!(row("Source"), media.name.to_string());
    assert!(readout.drag.is_empty(), "no drag, and a drag row");

    state.trim_feedback = Some(crate::types::TrimFeedback {
        tool_label: "Ripple".into(),
        clip_name: ui.name.clone(),
        delta_ticks: 12,
        delta_label: "+00:00:00:12".into(),
        detail_label: "".into(),
    });
    let drag = state.info_readout().drag;
    assert_eq!(
        drag.iter()
            .map(|row| (row.label, row.value.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("Tool", "Ripple"),
            ("Clip", ui.name.as_ref()),
            ("Change", "+00:00:00:12"),
        ]
    );

    state.clear_selection();
    assert!(state.info_readout().clip.is_empty());
}

#[test]
fn a_viewer_move_reads_its_offset_in_canvas_pixels() {
    let (mut state, _) = with_clip();
    let [width, height] = state.canvas_size().map(|side| side as f32);
    let start = crate::types::UiClipTransform::default();
    let mut draft = start;
    draft.position_x += 0.25;
    draft.position_y -= 0.1;
    state.overlay_drag = Some(OverlayDrag::Transform {
        handle: None,
        start,
        grab: [0.0, 0.0],
        draft,
    });
    let drag = state.info_readout().drag;
    assert_eq!(drag.len(), 1, "{drag:?}");
    assert_eq!(drag[0].label, "Move");
    assert_eq!(
        drag[0].value,
        format!("{:+.0} px, {:+.0} px", 0.25 * width, -0.1 * height)
    );
}

#[test]
fn a_failing_driver_reads_with_its_clip_and_property() {
    let (mut state, clip) = with_clip();
    state
        .ctx
        .mount(&aria_plugin::ExpressionsPlugin)
        .expect("mount Expressions");
    state
        .apply_command(aria_commands::Command::Edit(
            aria_commands::EditCommand::SetParamDriver {
                clip,
                param: aria_models::ClipParam::Rotation,
                code: Some(aria_models::DriverCode::new(
                    "aria.expressions",
                    "nothing + 1",
                )),
            },
        ))
        .expect("code that fails is kept until it is fixed");
    // The inspector row evaluates the driver, which records its failure.
    let fault = state
        .param_rows("transform")
        .into_iter()
        .find(|row| row.param == aria_models::ClipParam::Rotation)
        .and_then(|row| row.fault)
        .expect("the row reports the failure");
    let name = state.ui_clip(clip).expect("the clip").name.to_string();
    let faults = state.info_readout().faults;
    let shown = faults
        .iter()
        .find(|shown| shown.source.starts_with(&name))
        .unwrap_or_else(|| panic!("no driver fault in {faults:?}"));
    assert_eq!(shown.source, format!("{name} Rotation driver"));
    assert!(
        shown.message.contains("nothing is not defined")
            && fault.contains("nothing is not defined"),
        "{shown:?}"
    );
}

/// Shift+8 and Shift+9 bring Info and Audio Meters forward, opening each when
/// closed, and a second press leaves it open.
#[test]
fn shift_8_and_shift_9_bring_their_panels_forward() {
    for (id, panel) in [
        ("key-pane-info", crate::dock::PANEL_INFO),
        ("key-pane-audio-meters", crate::dock::PANEL_AUDIO_METERS),
    ] {
        let (mut state, _) = with_clip();
        let binding = crate::keys::bindings::SHIPPED_BINDINGS
            .iter()
            .find(|binding| binding.id == id)
            .unwrap_or_else(|| panic!("{id} is bound"));
        let crate::keys::KeyBody::Plain(run) = binding.run else {
            panic!("{id} runs on the state alone");
        };
        if state.dock_tree.is_open(panel) {
            state.toggle_dock_panel(panel);
        }
        run(&mut state);
        assert!(
            state.dock_tree.is_active(panel),
            "{id} left {panel} closed or behind"
        );
        run(&mut state);
        assert!(
            state.dock_tree.is_active(panel),
            "a second {id} put {panel} away"
        );
    }
}

/// Window > Info and Window > Align dock panels that draw, a second press
/// closes each, and the Info row draws the chord that brings it forward.
#[gpui::test]
fn the_info_and_align_panels_dock_from_the_window_menu(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "info-align-panels");
    for (panel, drawn, chord) in [
        (crate::dock::PANEL_INFO, "info-panel", Some("Shift+8")),
        (crate::dock::PANEL_ALIGN, "align-panel", None),
    ] {
        let row = format!("{}{panel}", crate::chrome::menu_window::PANEL_PREFIX);
        let shown = visual.read(|app| {
            let state = state.read(app);
            crate::chrome::menu_items::items_of(crate::types::AppMenu::Window, state)
                .into_iter()
                .find(|item| item.id == row)
                .and_then(|item| item.chord(state))
        });
        assert_eq!(shown.as_deref(), chord, "the chord beside Window > {panel}");
        let press = |visual: &mut gpui::VisualTestContext| {
            let found = visual.update(|_, app| {
                state.update(app, |state, cx| {
                    crate::chrome::menu_items::run_item(
                        crate::types::AppMenu::Window,
                        &row,
                        state,
                        cx,
                    )
                })
            });
            assert!(found, "Window has no row for {panel}");
            visual.run_until_parked();
            visual.read(|app| state.read(app).dock_tree.is_open(panel))
        };
        if visual.read(|app| state.read(app).dock_tree.is_open(panel)) {
            assert!(!press(visual), "{panel} did not close");
        }
        assert!(press(visual), "{panel} did not open");
        assert!(
            visual.debug_bounds(drawn).is_some(),
            "{panel} is open and not drawn"
        );
        assert!(!press(visual), "{panel} did not close");
        assert!(
            visual.debug_bounds(drawn).is_none(),
            "{panel} is closed and still drawn"
        );
    }
}

// --- what the last frame cost, and the rate playback held ------------------

/// A frame whose stages each cost a different amount, so a row that reads the
/// wrong field cannot match by coincidence.
fn measured(passes: u32, built: u32) -> aria_render::FrameStats {
    aria_render::FrameStats {
        decode_ms: 1.0,
        raster_ms: 2.0,
        composite_ms: 4.0,
        effect_passes: passes,
        effect_pass_bind_groups: built,
        snapped_frames: 0,
    }
}

/// Every stage of the engine's catalog, in pipeline order, then the whole
/// frame and the part a smaller preview buys back. Derived from the catalog,
/// so a stage added to it is red here until the Info panel states it.
#[test]
fn the_last_frame_states_every_stage_then_the_whole_frame() {
    let (mut state, _) = with_clip();
    let stats = measured(0, 0);
    state.frame_stats = stats;
    let ms = |value: f64| format!("{value:.2} ms");
    let mut expected: Vec<InfoRow> = frame_stages()
        .iter()
        .map(|stage| InfoRow::new(stage.label, ms((stage.read)(&stats))))
        .collect();
    expected.push(InfoRow::new("Frame", "7.00 ms"));
    expected.push(InfoRow::new("Scales with size", "6.00 ms"));
    assert_eq!(state.info_readout().frame, expected);
}

/// A zeroed split is no frame measured, not a frame that cost nothing: the
/// panel has no rows to draw and the status line says so.
#[test]
fn an_unmeasured_frame_has_no_rows_and_says_so() {
    let (mut state, _) = with_clip();
    state.frame_stats = aria_render::FrameStats::default();
    assert_eq!(state.info_readout().frame, Vec::new());
    state.report_render_cost();
    assert_eq!(
        state.status_message.as_deref(),
        Some("No frame has been rendered yet")
    );
}

/// The pass counts are counts, not stages: absent while the frame drew no
/// effect pass, the last two rows once it did.
#[test]
fn effect_passes_are_stated_only_once_a_pass_was_drawn() {
    let (mut state, _) = with_clip();
    state.frame_stats = measured(0, 0);
    let rows = state.info_readout().frame;
    assert!(
        rows.iter()
            .all(|row| !row.label.contains("pass") && !row.label.contains("Bind")),
        "a frame with no effect pass stated one: {rows:?}"
    );
    state.frame_stats = measured(24, 2);
    let rows = state.info_readout().frame;
    assert_eq!(
        rows[rows.len() - 2..],
        [
            InfoRow::new("Effect passes", "24"),
            InfoRow::new("Bind groups built", "2"),
        ]
    );
}

/// Last Frame Cost prints the rows the Info panel draws, in its order, so the
/// status line and the panel cannot disagree about a frame.
#[test]
fn last_frame_cost_prints_the_rows_the_info_panel_draws() {
    let (mut state, _) = with_clip();
    state.frame_stats = measured(24, 2);
    state.report_render_cost();
    assert_eq!(
        state.status_message.as_deref(),
        Some(
            "Decode 1.00 ms · Raster 2.00 ms · Composite 4.00 ms · Frame 7.00 ms · \
             Scales with size 6.00 ms · Effect passes 24 · Bind groups built 2"
        )
    );
}

/// A paused editor has no rate to miss; a run is stated once it delivers a
/// frame. It holds the rate while its frames come in under budget and are
/// shown on time, and misses it once frames that came due are dropped, even
/// with every render inside budget.
#[test]
fn playback_is_stated_once_it_delivers_a_frame_with_its_verdict() {
    let (mut state, _) = with_clip();
    assert_eq!(state.info_readout().playback, None);
    state.toggle_playback();
    let budget = state.playback.budget();
    for frame in 1..=19u32 {
        state.playback.frame(budget / 2);
        state.playback.present(budget * frame);
    }
    let stated = |state: &AppState| {
        state
            .info_readout()
            .playback
            .expect("a delivered frame is stated")
    };
    let late = InfoRow::new(
        "Late",
        format!(
            "0 of 19 frames, worst {:.1} ms",
            (budget / 2).as_secs_f64() * 1000.0
        ),
    );
    let held = stated(&state);
    assert!(held.holds_rate, "a run shown on time missed the rate");
    assert_eq!(
        held.rows,
        [
            InfoRow::new(
                "Rate",
                format!(
                    "{:.1} of {:.0} fps",
                    1.0 / budget.as_secs_f64(),
                    state.playhead.rate.as_f64()
                ),
            ),
            late.clone(),
        ]
    );
    let mut dropped = aria_render::DroppedFrames::default();
    dropped.record(aria_render::DropCause::DecodeLate, 2);
    state.playback.drop_frames(dropped);
    let missed = stated(&state);
    assert!(
        !missed.holds_rate,
        "a run that dropped two frames in twenty-one held the rate"
    );
    assert_eq!(missed.rows[1], late, "a drop is not a late render");
}
