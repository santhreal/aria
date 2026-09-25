//! Contracts for the scope strip's state.
//!
//! The class closed here: colour work was done by eye, and the first version of
//! any scope surface measures on a timer or on open only, which shows one
//! frame's numbers under another frame's picture. These pin that a measurement
//! exists only while the strip is open, that it follows the frame, and that
//! closing the strip drops the textures instead of leaving a stale reading
//! parked in memory.
//!
//! Not covered: the drawn strip. `crate::scopes` tests assert the bitmaps.

use std::sync::Arc;

use aria_core::RgbaImage;

use super::premiere_tests::with_clip;

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

#[test]
fn the_strip_starts_closed_and_measures_nothing() {
    let (state, _) = with_clip();
    assert!(!state.scopes_open);
    assert!(state.scope_textures.is_none());
    assert_eq!(state.scope_summary(), "Scopes off");
}

#[test]
fn opening_the_strip_measures_the_frame_already_on_screen() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(64, 32, [200, 200, 200, 255]));
    state.toggle_scopes();
    assert!(state.scopes_open);
    let textures = state.scope_textures.as_ref().expect("measured on open");
    assert_eq!(textures.sampled, 64 * 32);
    assert!(state.scope_summary().contains("2048"));
}

#[test]
fn closing_the_strip_drops_the_measurement() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(16, 16, [10, 20, 30, 255]));
    state.toggle_scopes();
    assert!(state.scope_textures.is_some());
    state.toggle_scopes();
    assert!(!state.scopes_open);
    assert!(
        state.scope_textures.is_none(),
        "a closed strip must not hold a stale reading"
    );
}

#[test]
fn a_refresh_while_closed_measures_nothing() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(16, 16, [10, 20, 30, 255]));
    state.refresh_scopes();
    assert!(state.scope_textures.is_none());
}

#[test]
fn the_measurement_follows_the_frame() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(32, 16, [0, 0, 0, 255]));
    state.toggle_scopes();
    let first = state.scope_textures.as_ref().expect("first").sampled;
    assert_eq!(first, 32 * 16);

    state.last_frame = Some(frame(8, 8, [255, 255, 255, 255]));
    state.refresh_scopes();
    let second = state.scope_textures.as_ref().expect("second").sampled;
    assert_eq!(
        second, 64,
        "the strip reports the new frame, not the old one"
    );
}

#[test]
fn opening_the_strip_with_no_frame_says_so_rather_than_faking_one() {
    let (mut state, _) = with_clip();
    state.last_frame = None;
    state.toggle_scopes();
    assert!(state.scopes_open);
    assert!(state.scope_textures.is_none());
    assert_eq!(state.scope_summary(), "No frame");
}

#[test]
fn a_transparent_frame_measures_no_pixels_but_still_paints() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(8, 8, [0, 0, 0, 0]));
    state.toggle_scopes();
    let textures = state.scope_textures.as_ref().expect("painted");
    assert_eq!(
        textures.sampled, 0,
        "alpha-zero pixels are not part of the picture"
    );
}

#[test]
fn a_malformed_frame_leaves_the_previous_reading_alone() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(16, 16, [64, 64, 64, 255]));
    state.toggle_scopes();
    let before = state.scope_textures.as_ref().expect("first").sampled;

    // Dimensions that do not match the buffer: the frame cannot be viewed.
    state.last_frame = Some(Arc::new(RgbaImage::new(16, 16, vec![0; 8])));
    state.refresh_scopes();
    let after = state.scope_textures.as_ref().expect("kept").sampled;
    assert_eq!(before, after, "a bad frame must not blank the strip");
}

#[test]
fn the_strip_is_not_an_edit() {
    let (mut state, _) = with_clip();
    let depth = state.history_depth();
    state.last_frame = Some(frame(16, 16, [1, 2, 3, 255]));
    state.toggle_scopes();
    state.refresh_scopes();
    state.toggle_scopes();
    assert_eq!(state.history_depth(), depth, "viewing is not editing");
}

#[test]
fn the_palette_and_the_view_menu_both_reach_the_scopes() {
    let (state, _) = with_clip();
    assert!(
        crate::commands::BUILTIN_COMMANDS
            .iter()
            .any(|command| command.id == "pal-preview-scopes"),
        "the palette has no way to open the scopes"
    );
    let labels = crate::chrome::labels_for(crate::types::AppMenu::View, &state);
    assert!(
        labels.contains(&"Scopes"),
        "the View menu has no scopes row: {labels:?}"
    );
}

#[test]
fn consecutive_refreshes_on_same_frame_reuse_retained_textures() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(32, 16, [100, 150, 200, 255]));
    state.toggle_scopes();
    assert!(state.scopes_open);
    let first = state.scope_textures.as_ref().expect("first textures");
    let first_waveform = Arc::clone(&first.waveform);
    let first_vectorscope = Arc::clone(&first.vectorscope);
    let first_histogram = Arc::clone(&first.histogram);

    // Refresh again with the exact same frame: textures must be reused directly
    // without reallocating RenderImage instances.
    state.refresh_scopes();
    let second = state.scope_textures.as_ref().expect("second textures");
    assert!(
        Arc::ptr_eq(&first_waveform, &second.waveform),
        "waveform texture was reallocated on warm frame refresh"
    );
    assert!(
        Arc::ptr_eq(&first_vectorscope, &second.vectorscope),
        "vectorscope texture was reallocated on warm frame refresh"
    );
    assert!(
        Arc::ptr_eq(&first_histogram, &second.histogram),
        "histogram texture was reallocated on warm frame refresh"
    );
}

#[test]
fn consecutive_same_dimension_refreshes_reuse_retained_scope_image_buffers() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(16, 16, [10, 20, 30, 255]));
    state.toggle_scopes();
    let images = state.scope_images.as_ref().expect("scope images");
    let waveform_ptr = images.waveform.pixels.as_ptr();
    let vectorscope_ptr = images.vectorscope.pixels.as_ptr();
    let histogram_ptr = images.histogram.pixels.as_ptr();
    let waveform_cap = images.waveform.pixels.capacity();
    let vectorscope_cap = images.vectorscope.pixels.capacity();
    let histogram_cap = images.histogram.pixels.capacity();

    // Replace with a new frame of the same dimensions: CPU pixel buffers must be
    // reused in place rather than reallocated.
    state.last_frame = Some(frame(16, 16, [50, 60, 70, 255]));
    state.refresh_scopes();

    let updated = state.scope_images.as_ref().expect("retained images");
    assert_eq!(
        waveform_ptr,
        updated.waveform.pixels.as_ptr(),
        "waveform pixel buffer was reallocated for same-dimension frame"
    );
    assert_eq!(
        vectorscope_ptr,
        updated.vectorscope.pixels.as_ptr(),
        "vectorscope pixel buffer was reallocated for same-dimension frame"
    );
    assert_eq!(
        histogram_ptr,
        updated.histogram.pixels.as_ptr(),
        "histogram pixel buffer was reallocated for same-dimension frame"
    );
    assert_eq!(waveform_cap, updated.waveform.pixels.capacity());
    assert_eq!(vectorscope_cap, updated.vectorscope.pixels.capacity());
    assert_eq!(histogram_cap, updated.histogram.pixels.capacity());
}

#[test]
fn input_dimension_changes_repaint_fixed_scope_buffers_safely() {
    let (mut state, _) = with_clip();
    state.last_frame = Some(frame(32, 16, [100, 100, 100, 255]));
    state.toggle_scopes();
    let before = state.scope_images.as_ref().expect("first images");
    let waveform_ptr = before.waveform.pixels.as_ptr();
    let vectorscope_ptr = before.vectorscope.pixels.as_ptr();
    let histogram_ptr = before.histogram.pixels.as_ptr();
    assert_eq!(
        state.scope_textures.as_ref().expect("first").sampled,
        32 * 16
    );

    // Change to a new frame with different dimensions.
    state.last_frame = Some(frame(64, 32, [200, 200, 200, 255]));
    state.refresh_scopes();
    let textures = state.scope_textures.as_ref().expect("resized textures");
    assert_eq!(textures.sampled, 64 * 32);
    let images = state.scope_images.as_ref().expect("resized images");
    assert_eq!(images.waveform.width, crate::scopes::HISTOGRAM_W);
    assert!(images.waveform.is_well_formed());
    assert!(images.vectorscope.is_well_formed());
    assert!(images.histogram.is_well_formed());
    assert_eq!(waveform_ptr, images.waveform.pixels.as_ptr());
    assert_eq!(vectorscope_ptr, images.vectorscope.pixels.as_ptr());
    assert_eq!(histogram_ptr, images.histogram.pixels.as_ptr());
}

#[test]
fn refresh_scopes_borrows_last_frame_without_cloning_arc() {
    let (mut state, _) = with_clip();
    let test_frame = frame(16, 16, [10, 20, 30, 255]);
    state.last_frame = Some(Arc::clone(&test_frame));
    assert_eq!(Arc::strong_count(&test_frame), 2);

    state.toggle_scopes();
    assert_eq!(
        Arc::strong_count(&test_frame),
        2,
        "toggle_scopes must not clone or retain additional frame Arc references"
    );

    state.refresh_scopes();
    assert_eq!(
        Arc::strong_count(&test_frame),
        2,
        "refresh_scopes must borrow last_frame without cloning the Arc"
    );
}
