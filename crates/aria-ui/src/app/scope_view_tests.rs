//! Contracts for the scope strip.
//!
//! The class closed here: scopes measured on the window's thread, on a timer or
//! on open only, which stalls the window for a pass over every frame and shows
//! one frame's numbers under another frame's picture. These pin that the pass
//! runs on the preview worker, that a reading is shown only while the strip is
//! open and only under the frame it was measured from, that closing the strip
//! releases its textures, and that a repaint reuses the scope bitmaps.
//!
//! Plugin scopes are pinned the same way: measured from the same frame as the
//! built-in three, on the worker, with the list following registrations and
//! their textures released with the reading they belong to.
//!
//! The waveform mode is pinned at each place it travels: a reading counted in
//! another mode is never shown, each View row and each chip over the waveform
//! reaches the worker, and a restart measures in the mode saved.
//!
//! Not covered: the strip's layout. `crate::scopes` tests assert the bitmaps.

use std::sync::Arc;

use aria_analysis::WaveformTrace;
use aria_core::RgbaImage;
use aria_plugin::{PluginContext, Registration, ScopeFrame, ScopeImage, ScopeSpec};
use gpui::AppContext;

use super::premiere_tests::with_clip;
use super::scope_view::{ScopeBuffers, ScopeTextures, measure};
use super::viewer_resolution_tests::run_view_row;
use super::{AppState, Mail};
use crate::interned::intern;
use crate::scopes::WaveformMode;
use crate::settings::UiSettings;

/// A frame of one colour, straight alpha.
fn frame(width: u32, height: u32, rgba: [u8; 4]) -> Arc<RgbaImage> {
    let pixels = rgba
        .iter()
        .copied()
        .cycle()
        .take((width * height * 4) as usize)
        .collect();
    Arc::new(RgbaImage::new(width, height, pixels))
}

/// A reading of `frame` for `tick`, as the worker makes one.
fn reading(frame: &RgbaImage, tick: i64) -> ScopeTextures {
    measure(
        frame,
        tick,
        WaveformTrace::Luma,
        &mut ScopeBuffers::default(),
    )
    .expect("a well-formed frame measures")
}

/// Register a 2 by 1 host scope `id` in `ctx` that paints its first pixel
/// in the frame's top-left colour and reads that red value back.
fn first_pixel_scope(ctx: &PluginContext, id: &str) -> Registration {
    let spec = ScopeSpec {
        id: id.to_owned(),
        name: "First pixel".to_owned(),
        width: 2,
        height: 1,
    };
    ctx.register_scope(
        spec,
        |_: &PluginContext, frame: &ScopeFrame<'_>, image: &mut ScopeImage| {
            let rgba = frame.pixel(0, 0).ok_or("an empty frame")?;
            image.put(0, 0, rgba);
            image.set_readout(format!("red {}", rgba[0]));
            Ok(())
        },
    )
    .expect("a valid scope registers")
}

#[test]
fn the_strip_starts_closed_and_measures_nothing() {
    let (state, _) = with_clip();
    assert!(!state.scopes_open);
    assert!(state.scope_textures.is_none());
    assert_eq!(state.scope_summary(), "Scopes off");
}

#[test]
fn a_frame_measures_every_opaque_pixel_and_says_which_frame() {
    let measured = reading(&frame(64, 32, [200, 200, 200, 255]), 7);
    assert_eq!(measured.sampled, 64 * 32);
    assert_eq!(measured.tick, 7);
}

#[test]
fn a_transparent_frame_measures_no_pixels_but_still_paints() {
    let measured = reading(&frame(8, 8, [0, 0, 0, 0]), 0);
    assert_eq!(
        measured.sampled, 0,
        "alpha-zero pixels are not part of the picture"
    );
}

#[test]
fn a_malformed_frame_measures_nothing() {
    // Dimensions that do not match the buffer: the frame cannot be viewed.
    let bad = RgbaImage::new(16, 16, vec![0; 8]);
    assert!(measure(&bad, 0, WaveformTrace::Luma, &mut ScopeBuffers::default()).is_none());
    let empty = RgbaImage::new(0, 0, Vec::new());
    assert!(measure(&empty, 0, WaveformTrace::Luma, &mut ScopeBuffers::default()).is_none());
}

/// A reading is shown only while the strip is open and only under the frame
/// it was measured from: one that arrives after the picture moved on, or after
/// the strip closed, is dropped.
#[test]
fn a_reading_is_shown_only_under_its_own_frame() {
    let (mut state, _) = with_clip();
    let picture = frame(16, 16, [10, 20, 30, 255]);
    state.last_frame = Some(Arc::clone(&picture));
    state.shown_tick = Some(4);

    assert!(
        !state.adopt_scopes(Some(reading(&picture, 4))),
        "a closed strip took a reading"
    );
    state.toggle_scopes();
    assert!(state.scopes_open);
    assert_eq!(state.scope_summary(), "No frame");
    assert!(
        !state.adopt_scopes(Some(reading(&picture, 3))),
        "a reading for another frame was shown"
    );
    assert!(state.scope_textures.is_none());
    assert!(state.adopt_scopes(Some(reading(&picture, 4))));
    assert_eq!(state.scope_summary(), "256 px sampled");
}

/// Closing the strip, and a newer reading, put the textures they replace in
/// line to leave the window atlas.
#[test]
fn a_replaced_or_closed_reading_releases_its_textures() {
    let (mut state, _) = with_clip();
    let picture = frame(16, 16, [10, 20, 30, 255]);
    state.last_frame = Some(Arc::clone(&picture));
    state.shown_tick = Some(0);
    state.toggle_scopes();
    let first = reading(&picture, 0);
    let waveform = Arc::clone(&first.waveform);
    assert!(state.adopt_scopes(Some(first)));
    assert!(state.retired_images.is_empty());

    assert!(state.adopt_scopes(Some(reading(&picture, 0))));
    assert_eq!(
        state.retired_images.len(),
        3,
        "the replaced reading was kept"
    );
    assert!(
        state
            .retired_images
            .iter()
            .any(|image| Arc::ptr_eq(image, &waveform))
    );

    state.toggle_scopes();
    assert!(!state.scopes_open);
    assert!(
        state.scope_textures.is_none(),
        "a closed strip must not hold a stale reading"
    );
    assert_eq!(state.retired_images.len(), 6);
}

#[test]
fn the_strip_is_not_an_edit() {
    let (mut state, _) = with_clip();
    let depth = state.history_depth();
    state.last_frame = Some(frame(16, 16, [1, 2, 3, 255]));
    state.shown_tick = Some(0);
    state.toggle_scopes();
    state.toggle_scopes();
    assert_eq!(state.history_depth(), depth, "viewing is not editing");
}

/// The strip is reached from the palette and View, and the Scopes panel from
/// Window, where every dock panel has its row.
#[test]
fn the_palette_and_the_menus_reach_the_strip_and_the_panel() {
    let (state, _) = with_clip();
    assert!(
        crate::commands::BUILTIN_COMMANDS
            .iter()
            .any(|command| command.id == "pal-preview-scopes"),
        "the palette has no way to open the scope strip"
    );
    let view = crate::chrome::labels_for(crate::types::AppMenu::View, &state);
    assert!(
        view.contains(&"Scope Strip"),
        "the View menu has no strip row: {view:?}"
    );
    let window = crate::chrome::labels_for(crate::types::AppMenu::Window, &state);
    assert!(
        window.contains(&"Scopes"),
        "the Window menu has no Scopes row: {window:?}"
    );
}

/// The worker repaints the scope bitmaps in place: a frame of the same size,
/// and a frame of another size, reuse the same pixel buffers.
#[test]
fn a_repaint_reuses_the_scope_bitmaps() {
    let mut images = ScopeBuffers::default();
    let _ = measure(
        &frame(16, 16, [10, 20, 30, 255]),
        0,
        WaveformTrace::Luma,
        &mut images,
    );
    let buffers = |images: &ScopeBuffers| {
        [
            images.builtin.waveform.pixels.as_ptr(),
            images.builtin.vectorscope.pixels.as_ptr(),
            images.builtin.histogram.pixels.as_ptr(),
        ]
    };
    let before = buffers(&images);
    let _ = measure(
        &frame(16, 16, [50, 60, 70, 255]),
        1,
        WaveformTrace::Luma,
        &mut images,
    );
    assert_eq!(buffers(&images), before, "a same-size frame reallocated");
    let resized = measure(
        &frame(64, 32, [200, 200, 200, 255]),
        2,
        WaveformTrace::Luma,
        &mut images,
    );
    assert_eq!(resized.map(|textures| textures.sampled), Some(64 * 32));
    assert_eq!(buffers(&images), before, "a resized frame reallocated");
    let images = &images.builtin;
    assert_eq!(images.waveform.width, crate::scopes::HISTOGRAM_W);
    assert!(images.waveform.is_well_formed());
    assert!(images.vectorscope.is_well_formed());
    assert!(images.histogram.is_well_formed());
}

/// A plugin scope reads the frame the built-in scopes read, draws at its
/// declared size, and its readout belongs to that frame.
#[test]
fn a_plugin_scope_measures_the_same_frame_as_the_builtins() {
    let ctx = PluginContext::new();
    let _scope = first_pixel_scope(&ctx, "test.first");
    let mut buffers = ScopeBuffers::with_plugins(ctx);
    for red in [10, 200] {
        let measured = measure(
            &frame(4, 4, [red, 20, 30, 255]),
            0,
            WaveformTrace::Luma,
            &mut buffers,
        )
        .expect("a well-formed frame measures");
        let [reading] = measured.plugins.as_slice() else {
            panic!("expected one plugin reading: {:?}", measured.plugins);
        };
        assert_eq!(&*reading.scope.spec.id, "test.first");
        assert_eq!(reading.readout, format!("red {red}"));
        let image = reading.drawn.as_ref().expect("the scope drew");
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (2, 1));
        // The texture is BGRA; the second pixel was never painted.
        assert_eq!(
            image.as_bytes(0),
            Some(&[30, 20, red, 255, 0, 0, 0, 0][..]),
            "the image is not this frame's drawing"
        );
    }
}

/// The plugin readings follow the registry: a scope registered after the
/// first frame shows on the next one, and one unregistered leaves.
#[test]
fn plugin_readings_follow_registrations() {
    let ctx = PluginContext::new();
    let mut buffers = ScopeBuffers::with_plugins(ctx.clone());
    let picture = frame(4, 4, [9, 9, 9, 255]);
    let ids = |buffers: &mut ScopeBuffers| {
        measure(&picture, 0, WaveformTrace::Luma, buffers)
            .expect("a well-formed frame measures")
            .plugins
            .iter()
            .map(|reading| reading.scope.spec.id.clone())
            .collect::<Vec<_>>()
    };
    assert!(ids(&mut buffers).is_empty());
    let first = first_pixel_scope(&ctx, "test.a");
    let second = first_pixel_scope(&ctx, "test.b");
    assert_eq!(ids(&mut buffers), ["test.a", "test.b"]);
    drop(first);
    assert_eq!(ids(&mut buffers), ["test.b"]);
    drop(second);
    assert!(ids(&mut buffers).is_empty());
}

/// A scope that fails shows its reason in place of an image, and the
/// built-in scopes still measure.
#[test]
fn a_failing_plugin_scope_reports_its_reason() {
    let ctx = PluginContext::new();
    let spec = ScopeSpec {
        id: "test.broken".to_owned(),
        name: "Broken".to_owned(),
        width: 8,
        height: 8,
    };
    let _scope = ctx
        .register_scope(
            spec,
            |_: &PluginContext, _: &ScopeFrame<'_>, _: &mut ScopeImage| Err("no signal".to_owned()),
        )
        .expect("a valid scope registers");
    let measured = measure(
        &frame(4, 4, [1, 2, 3, 255]),
        0,
        WaveformTrace::Luma,
        &mut ScopeBuffers::with_plugins(ctx),
    )
    .expect("the built-in scopes still measure");
    assert_eq!(measured.sampled, 16);
    let [reading] = measured.plugins.as_slice() else {
        panic!("expected one plugin reading: {:?}", measured.plugins);
    };
    assert_eq!(
        reading.drawn.as_ref().err().map(String::as_str),
        Some("no signal")
    );
}

/// A replaced reading releases its plugin textures with the built-in ones.
#[test]
fn a_replaced_reading_releases_its_plugin_textures() {
    let (mut state, _) = with_clip();
    let _scope = first_pixel_scope(&state.ctx, "test.release");
    let mut buffers = ScopeBuffers::with_plugins((*state.ctx).clone());
    let picture = frame(4, 4, [5, 6, 7, 255]);
    state.last_frame = Some(Arc::clone(&picture));
    state.shown_tick = Some(0);
    state.toggle_scopes();
    let first = measure(&picture, 0, WaveformTrace::Luma, &mut buffers).expect("measures");
    let plugin = Arc::clone(first.plugins[0].drawn.as_ref().expect("drew"));
    assert!(state.adopt_scopes(Some(first)));
    let next = measure(&picture, 0, WaveformTrace::Luma, &mut buffers).expect("measures");
    assert!(state.adopt_scopes(Some(next)));
    assert_eq!(
        state.retired_images.len(),
        4,
        "three built-in and one plugin"
    );
    assert!(
        state
            .retired_images
            .iter()
            .any(|image| Arc::ptr_eq(image, &plugin))
    );
}

/// Opening the strip over a frame already on screen measures that frame on the
/// preview worker, never on the calling thread, and the reading arrives
/// through the mailbox.
#[gpui::test]
async fn opening_the_strip_measures_the_frame_on_screen_on_the_worker(
    cx: &mut gpui::TestAppContext,
) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    let wakes = state.preview_worker.wakes();
    state.last_frame = Some(frame(64, 32, [200, 200, 200, 255]));
    state.shown_tick = Some(9);
    state.toggle_scopes();
    assert!(
        state.scope_textures.is_none(),
        "the scopes were measured on the calling thread"
    );
    assert_eq!(wakes.next_from_a_thread(&executor).await, Some(()));
    assert_eq!(
        state.take_preview_mail(),
        Mail::Reading,
        "the reading did not arrive"
    );
    let textures = state.scope_textures.as_ref().expect("measured on open");
    assert_eq!(textures.sampled, 64 * 32);
    assert_eq!(textures.tick, 9);
    assert!(state.scope_summary().contains("2048"));
}

/// While the strip is open a Viewer frame arrives with its scopes, measured
/// where it was rendered.
#[gpui::test]
async fn an_open_strip_gets_scopes_with_every_viewer_frame(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    state.toggle_scopes();
    let at = aria_models::RationalTime::new(2, state.playhead.rate);
    let rendered = state
        .preview_worker
        .frame(&executor, at, None, aria_core::Cancel::never())
        .await;
    let Some(Ok(frame)) = rendered else {
        panic!("the frame did not render: {rendered:?}");
    };
    let scopes = frame
        .scopes
        .as_ref()
        .expect("the frame came without scopes");
    assert_eq!(scopes.tick, 2);
    assert_eq!(
        scopes.sampled,
        reading(&frame.rgba, 2).sampled,
        "the scopes were not measured from this frame"
    );
}

/// The worker measures the session's plugin scopes with the Viewer frame it
/// rendered, so a scope registered by a plugin reaches the strip.
#[gpui::test]
async fn the_worker_measures_the_sessions_plugin_scopes(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    let _scope = first_pixel_scope(&state.ctx, "test.worker");
    state.toggle_scopes();
    let at = aria_models::RationalTime::new(2, state.playhead.rate);
    let rendered = state
        .preview_worker
        .frame(&executor, at, None, aria_core::Cancel::never())
        .await;
    let Some(Ok(frame)) = rendered else {
        panic!("the frame did not render: {rendered:?}");
    };
    let scopes = frame
        .scopes
        .as_ref()
        .expect("the frame came without scopes");
    let [reading] = scopes.plugins.as_slice() else {
        panic!("expected one plugin reading: {:?}", scopes.plugins);
    };
    let red = frame.rgba.pixels[0];
    assert_eq!(reading.readout, format!("red {red}"));
}

/// The worker's next wake, failing after 30 s of wall time: a measurement that
/// is never asked for fails here instead of spinning the test scheduler's
/// timer for ever.
async fn next_wake(
    wakes: &crate::handoff::Wakes<()>,
    executor: &gpui::BackgroundExecutor,
    what: &str,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while wakes.try_next().is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "{what}: the worker never woke"
        );
        executor.timer(std::time::Duration::from_millis(1)).await;
    }
}

/// Choosing a waveform mode measures the frame on screen again on the worker,
/// counted in that mode, though the reading on screen is for the same frame.
#[gpui::test]
async fn a_new_waveform_mode_remeasures_the_frame_on_screen(cx: &mut gpui::TestAppContext) {
    let executor = cx.executor();
    executor.allow_parking();
    let (mut state, _) = with_clip();
    let wakes = state.preview_worker.wakes();
    state.last_frame = Some(frame(64, 32, [200, 40, 30, 255]));
    state.shown_tick = Some(9);
    state.toggle_scopes();
    next_wake(&wakes, &executor, "opening the strip").await;
    assert_eq!(
        state.take_preview_mail(),
        Mail::Reading,
        "the luma reading did not arrive"
    );
    for mode in [
        WaveformMode::YcbcrParade,
        WaveformMode::RgbParade,
        WaveformMode::Luma,
    ] {
        assert!(state.choose_waveform_mode(mode));
        // A wake says mail is waiting, not that it is this reading: one the
        // worker counted in the mode just left wakes the window as well.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            next_wake(&wakes, &executor, mode.label()).await;
            state.take_preview_mail();
            let shown = state
                .scope_textures
                .as_ref()
                .map(|textures| (textures.trace, textures.tick));
            if shown == Some((mode.trace(), 9)) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the {mode:?} reading did not arrive; the strip shows {shown:?}"
            );
        }
    }
}

/// A reading the worker counted before a new waveform mode reached it is
/// dropped, and the one on screen stays until a reading in the new mode
/// replaces it, so the waveform never flashes empty or shows the old trace
/// under the new chip.
#[test]
fn a_reading_counted_in_another_waveform_mode_is_dropped() {
    let (mut state, _) = with_clip();
    let picture = frame(16, 16, [200, 40, 30, 255]);
    state.last_frame = Some(Arc::clone(&picture));
    state.shown_tick = Some(0);
    state.toggle_scopes();
    assert_eq!(state.waveform_mode(), WaveformMode::Luma);
    assert!(state.adopt_scopes(Some(reading(&picture, 0))));

    assert!(state.choose_waveform_mode(WaveformMode::RgbParade));
    assert!(
        !state.choose_waveform_mode(WaveformMode::RgbParade),
        "choosing the mode shown counted as a change"
    );
    assert_eq!(state.preview_worker.waveform(), WaveformTrace::RgbParade);
    assert!(
        !state.adopt_scopes(Some(reading(&picture, 0))),
        "a luma reading was shown under the RGB parade"
    );
    let shown = |state: &AppState| state.scope_textures.as_ref().map(|read| read.trace);
    assert_eq!(
        shown(&state),
        Some(WaveformTrace::Luma),
        "the reading on screen was dropped before its replacement arrived"
    );
    let parade = measure(
        &picture,
        0,
        WaveformTrace::RgbParade,
        &mut ScopeBuffers::default(),
    );
    assert!(state.adopt_scopes(parade));
    assert_eq!(shown(&state), Some(WaveformTrace::RgbParade));
}

/// Each View row shows its mode, checks only itself, and sends the mode to
/// the worker; the last one chosen is saved and a new session measures in it.
#[test]
fn each_waveform_row_reaches_the_worker_and_a_restart_reads_it_back() {
    let cx = gpui::TestAppContext::single();
    let (state, _) = with_clip();
    let data_dir = state.data_dir.clone();
    let state = cx.update(|app| app.new(|_| state));
    // Luma first, which is already shown, and a parade last, so the saved
    // mode cannot be mistaken for the default.
    let order = [
        WaveformMode::Luma,
        WaveformMode::YcbcrParade,
        WaveformMode::RgbParade,
    ];
    assert!(
        WaveformMode::ALL.iter().all(|mode| order.contains(mode)),
        "a waveform mode has no row in this walk"
    );
    for mode in order {
        let (caption, checked) = run_view_row(&cx, &state, mode.menu_id());
        assert_eq!((caption.as_str(), checked), (mode.label(), true));
        let (shown, counted, lit) = cx.read(|app| {
            let state = state.read(app);
            let lit: Vec<_> =
                crate::chrome::menu_items::items_of(crate::types::AppMenu::View, state)
                    .into_iter()
                    .filter(|item| {
                        WaveformMode::ALL
                            .iter()
                            .any(|mode| mode.menu_id() == item.id)
                            && item.is_checked(state)
                    })
                    .map(|item| item.id)
                    .collect();
            (state.waveform_mode(), state.preview_worker.waveform(), lit)
        });
        assert_eq!((shown, counted), (mode, mode.trace()));
        assert_eq!(lit, [mode.menu_id()], "the View menu checks another mode");
    }
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(
            crate::settings::SAVE_DEBOUNCE_MS + 50,
        ));
    cx.run_until_parked();
    assert_eq!(
        UiSettings::load(&data_dir).waveform,
        WaveformMode::RgbParade
    );
    let reopened = AppState::new(PluginContext::new(), data_dir).expect("reopen");
    assert_eq!(reopened.waveform_mode(), WaveformMode::RgbParade);
    assert_eq!(
        reopened.preview_worker.waveform(),
        WaveformTrace::RgbParade,
        "a new session counts luma under the saved parade"
    );
}

/// The chips over the waveform choose its mode, in the strip and in the
/// Scopes panel alike.
#[gpui::test]
fn the_chips_over_the_waveform_choose_its_mode(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "scope-waveform-chips");
    let picture = frame(16, 16, [200, 40, 30, 255]);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.last_frame = Some(Arc::clone(&picture));
            state.shown_tick = Some(0);
            state.toggle_scopes();
            assert!(state.adopt_scopes(Some(reading(&picture, 0))));
            cx.notify();
        })
    });
    visual.run_until_parked();
    for (place, mode) in [
        ("preview-scopes", WaveformMode::YcbcrParade),
        ("preview-scopes", WaveformMode::Luma),
        ("preview-scopes", WaveformMode::RgbParade),
    ] {
        let selector = format!("{place}-{}", mode.id());
        let bounds = visual
            .debug_bounds(intern(&selector))
            .unwrap_or_else(|| panic!("the strip draws no {selector} chip"));
        visual.simulate_click(bounds.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(
            visual.read(|app| state.read(app).waveform_mode()),
            mode,
            "{selector} chose another mode"
        );
    }
}

/// Mail that holds only a reading redraws where the frame on screen is drawn,
/// through `FrameReady`, and notifies no observer of the state; a failure or a
/// finished render bar notifies the state, which redraws the window.
///
/// The class closed here is mail routed to the wrong redraw: a reading that
/// notified the state would build every panel body again at the scope rate of
/// a run, and a render bar that only raised `FrameReady` would leave the
/// timeline ruler replaying the bar it drew before. Not caught: a reading
/// drawn by a panel the frame route leaves alone.
#[gpui::test]
fn each_kind_of_mail_redraws_what_it_changed(cx: &mut gpui::TestAppContext) {
    use std::cell::Cell;
    use std::rc::Rc;

    cx.executor().allow_parking();
    let (state, _) = with_clip();
    let state = cx.new(|_| state);
    let notified = Rc::new(Cell::new(0u32));
    let framed = Rc::new(Cell::new(0u32));
    let _observing = cx.update(|app| {
        let notified = notified.clone();
        app.observe(&state, move |_, _| notified.set(notified.get() + 1))
    });
    let _subscribed = cx.update(|app| {
        let framed = framed.clone();
        app.subscribe(&state, move |_, _: &crate::app::FrameReady, _| {
            framed.set(framed.get() + 1)
        })
    });
    for (mail, notifies, frames) in [
        (Mail::Nothing, 0, 0),
        (Mail::Reading, 0, 1),
        (Mail::Window, 1, 0),
    ] {
        notified.set(0);
        framed.set(0);
        state.update(cx, |state, cx| state.land_mail(mail, cx));
        cx.run_until_parked();
        assert_eq!(
            (notified.get(), framed.get()),
            (notifies, frames),
            "the notifications and FrameReady events {mail:?} raised"
        );
    }
}
