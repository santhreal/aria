//! A scope's picture fills the inside of the frame drawn around it.
//!
//! The class closed here: a scope picture laid out at its frame's full height
//! inside the frame's 1px border overran the padding box, which clips, so the
//! bottom rows were never drawn. On the waveform those rows are code 0, and a
//! black floor, the most common reading in a graded frame, showed nothing.
//! Each scope in the Scopes panel is measured against its frame: the waveform
//! and histogram through `scope_image`, the vectorscope through
//! `square_scope_image`. The strip draws through the same two functions.
//!
//! Not covered: plugin scopes, whose ids are hashed and carry no selector, and
//! the texture's pixels, which the test platform does not raster on Linux.

use aria_analysis::WaveformTrace;
use aria_core::RgbaImage;
use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::scope_view::{ScopeBuffers, measure};
use crate::dock::PANEL_SCOPES;
use crate::test_support::drawn_shell_at;

/// The width of a scope frame's border.
const BORDER: f32 = 1.0;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// Where `selector` was drawn.
fn drawn(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("the Scopes panel drew no {selector}"))
}

/// The edges of `at` as `(side, position)`, moved toward its center by `inset`.
fn edges(at: Bounds<Pixels>, inset: f32) -> [(&'static str, f32); 4] {
    [
        ("top", f32::from(at.top()) + inset),
        ("right", f32::from(at.right()) - inset),
        ("bottom", f32::from(at.bottom()) - inset),
        ("left", f32::from(at.left()) + inset),
    ]
}

#[gpui::test]
fn every_scope_picture_fills_the_inside_of_its_frame(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(1920.), px(1080.)));
    let (state, mut visual) = drawn_shell_at(cx, "scopes-frame", at);
    let visual = &mut visual;
    // The readings fade up; reduced motion lands them at once.
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_dock_panel(PANEL_SCOPES);
            let frame = RgbaImage::new(16, 16, vec![128; 16 * 16 * 4]);
            let mut buffers = ScopeBuffers::default();
            let reading = measure(&frame, 0, WaveformTrace::Luma, &mut buffers);
            state.scope_textures = Some(reading.expect("a well-formed frame measures"));
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());

    let scopes = [
        ("scopes-panel-waveform", "scopes-panel-waveform-image"),
        ("scopes-panel-vector", "scopes-panel-vector-image"),
        ("scopes-panel-histogram", "scopes-panel-histogram-image"),
    ];
    for (frame, picture) in scopes {
        let inside = edges(drawn(visual, frame), BORDER);
        let image = edges(drawn(visual, picture), 0.0);
        for ((side, want), (_, edge)) in inside.into_iter().zip(image) {
            assert!(
                (edge - want).abs() <= EPS,
                "{picture} draws its {side} edge at {edge}, the inside of {frame} at {want}"
            );
        }
    }
}
