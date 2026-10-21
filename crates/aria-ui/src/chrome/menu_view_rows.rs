//! The View menu: zoom, viewer resolution and display, comparison, the scope
//! strip, canvas handles, playback, and the timeline display settings.
//!
//! [`super::menu_items::items_for`] dispatches here, so the bar, the palette
//! and the tests reach these rows through the one registry.

use std::borrow::Cow;

use crate::app::AppState;
use crate::app::viewer_compare::{CompareLayout, Reference};
use crate::scopes::WaveformMode;
use crate::timeline_display::{Follow, Thumbnails, WaveformScale};
use crate::types::OverlayMode;

use super::menu_items::{MenuItem, MenuItems};

const COMPARE: &str = "Compare";
const CANVAS_HANDLES: &str = "Canvas Handles";

static VIEW_ITEMS: &[MenuItem] = &[
    MenuItem::new("seq-in", "Zoom In", |_, s, _| {
        s.set_zoom(s.zoom * crate::transport::ZOOM_STEP, true);
    })
    .key("key-zoom-in"),
    MenuItem::new("seq-out", "Zoom Out", |_, s, _| {
        s.set_zoom(s.zoom / crate::transport::ZOOM_STEP, true);
    })
    .key("key-zoom-out"),
    MenuItem::new("seq-fit", "Fit", |_, s, _| s.zoom_fit_sequence()).key("key-fit"),
    MenuItem::new("seq-fit-selection", "Fit Selection", |_, s, _| {
        s.zoom_fit_selection()
    })
    .key("key-fit-selection")
    .when(super::menu_guards::selection),
    MenuItem::new(
        "view-playback-resolution",
        "Playback Resolution",
        |_, s, cx| s.cycle_playback_resolution(cx),
    )
    .under("Resolution"),
    MenuItem::new("view-paused-resolution", "Paused Resolution", |_, s, cx| {
        s.toggle_paused_resolution(cx)
    })
    .under("Resolution"),
    MenuItem::new("view-split", "Split Viewer", |_, s, _| {
        s.toggle_viewer_split()
    })
    .checked(|_, s| s.sources.split())
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-rulers", "Show Rulers", |_, s, _| s.toggle_rulers())
        .key("key-rulers")
        .checked(|_, s| s.viewer_guides.rulers)
        .under(VIEWER_DISPLAY),
    MenuItem::new("view-guides-show", "Show Guides", |_, s, _| {
        s.toggle_guides_shown()
    })
    .checked(|_, s| !s.viewer_guides.hidden)
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-guide-snap", "Snap to Guides", |_, s, _| {
        s.toggle_guide_snap()
    })
    .checked(|_, s| s.viewer_guides.snap)
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-guides-clear", "Clear Guides", |_, s, _| {
        s.clear_guides()
    })
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-title-safe", "Title Safe", |_, s, _| {
        s.toggle_title_safe()
    })
    .checked(|_, s| s.preview_view.title_safe)
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-action-safe", "Action Safe", |_, s, _| {
        s.toggle_action_safe()
    })
    .checked(|_, s| s.preview_view.action_safe)
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-transparency-grid", "Transparency Grid", |_, s, cx| {
        s.toggle_transparency_grid(cx)
    })
    .checked(|_, s| s.transparency_grid_on())
    .under(VIEWER_DISPLAY),
    MenuItem::new("view-grid", "Grid", |_, s, _| s.toggle_preview_grid())
        .checked(|_, s| s.preview_view.grid)
        .under(VIEWER_DISPLAY),
    MenuItem::new("view-proxies", "Proxies", |_, s, cx| s.toggle_proxies(cx))
        .checked(|_, s| s.proxies_on()),
    MenuItem::new("view-compare-frame", "Current Frame", |_, s, cx| {
        s.compare_with_frame(cx)
    })
    .checked(|_, s| matches!(s.viewer_compare.reference(), Some(Reference::Frame { .. })))
    .under(COMPARE),
    MenuItem::new("view-compare-still", "Still…", |_, s, cx| {
        s.pick_compare_still(cx)
    })
    .checked(|_, s| matches!(s.viewer_compare.reference(), Some(Reference::Still { .. })))
    .under(COMPARE),
    MenuItem::new("view-compare-source", "Source Tab", |_, s, _| {
        s.compare_with_source()
    })
    .when(super::menu_guards::source)
    .checked(|_, s| s.viewer_compare.reference() == Some(&Reference::Source))
    .under(COMPARE),
    MenuItem::new("view-compare-vertical", "Vertical Wipe", |_, s, _| {
        s.set_compare_layout(CompareLayout::Vertical)
    })
    .checked(|_, s| s.viewer_compare.layout() == CompareLayout::Vertical)
    .under(COMPARE),
    MenuItem::new("view-compare-horizontal", "Horizontal Wipe", |_, s, _| {
        s.set_compare_layout(CompareLayout::Horizontal)
    })
    .checked(|_, s| s.viewer_compare.layout() == CompareLayout::Horizontal)
    .under(COMPARE),
    MenuItem::new("view-compare-side", "Side by Side", |_, s, _| {
        s.set_compare_layout(CompareLayout::SideBySide)
    })
    .checked(|_, s| s.viewer_compare.layout() == CompareLayout::SideBySide)
    .under(COMPARE),
    MenuItem::new("view-compare-stop", "Stop Comparing", |_, s, _| {
        s.stop_comparing()
    })
    .when(super::menu_guards::comparing)
    .under(COMPARE),
    MenuItem::new("view-scopes", "Scope Strip", |_, s, _| s.toggle_scopes())
        .checked(|_, s| s.scopes_open),
    waveform_row(WaveformMode::Luma),
    waveform_row(WaveformMode::RgbParade),
    waveform_row(WaveformMode::YcbcrParade),
    MenuItem::new("view-zoom-fit", "Fit Preview", |_, s, _| {
        s.preview_zoom_fit()
    }),
    MenuItem::new("view-zoom-actual", "Preview 100%", |_, s, _| {
        s.preview_zoom_actual()
    }),
    MenuItem::new("view-handles-transform", "Transform", |_, s, _| {
        s.set_overlay_mode(OverlayMode::Transform)
    })
    .checked(|_, s| s.overlay_mode() == OverlayMode::Transform)
    .under(CANVAS_HANDLES),
    MenuItem::new("view-handles-crop", "Crop", |_, s, _| {
        s.set_overlay_mode(OverlayMode::Crop)
    })
    .checked(|_, s| s.overlay_mode() == OverlayMode::Crop)
    .under(CANVAS_HANDLES),
    MenuItem::new("view-handles-mask", "Mask", |_, s, _| {
        s.set_overlay_mode(OverlayMode::Mask)
    })
    .checked(|_, s| s.overlay_mode() == OverlayMode::Mask)
    .under(CANVAS_HANDLES),
    MenuItem::new("view-fullscreen", "Fullscreen Preview", |_, s, _| {
        s.toggle_preview_fullscreen()
    })
    .checked(|_, s| s.preview_view.fullscreen),
    MenuItem::new("view-loop", "Loop Playback", |_, s, _| s.toggle_loop())
        .key("key-loop")
        .checked(|_, s| s.looping),
    MenuItem::new("view-thumbs-head", "Head Thumbnails", |_, s, cx| {
        s.set_timeline_thumbnails(Thumbnails::Head, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.thumbnails == Thumbnails::Head)
    .under(TIMELINE_DISPLAY),
    MenuItem::new(
        "view-thumbs-head-tail",
        "Head and Tail Thumbnails",
        |_, s, cx| s.set_timeline_thumbnails(Thumbnails::HeadAndTail, cx),
    )
    .checked(|_, s| s.ui_settings.timeline_display.thumbnails == Thumbnails::HeadAndTail)
    .under(TIMELINE_DISPLAY),
    MenuItem::new(
        "view-thumbs-continuous",
        "Continuous Thumbnails",
        |_, s, cx| s.set_timeline_thumbnails(Thumbnails::Continuous, cx),
    )
    .checked(|_, s| s.ui_settings.timeline_display.thumbnails == Thumbnails::Continuous)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-waveform-log", "Logarithmic Waveforms", |_, s, cx| {
        s.set_waveform_scale(WaveformScale::Logarithmic, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.waveform == WaveformScale::Logarithmic)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-waveform-linear", "Linear Waveforms", |_, s, cx| {
        s.set_waveform_scale(WaveformScale::Linear, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.waveform == WaveformScale::Linear)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-waveform-hidden", "Hide Waveforms", |_, s, cx| {
        s.set_waveform_scale(WaveformScale::Hidden, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.waveform == WaveformScale::Hidden)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-follow-page", "Page Scroll", |_, s, cx| {
        s.set_timeline_follow(Follow::Page, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.follow == Follow::Page)
    .under(TIMELINE_DISPLAY),
    MenuItem::new("view-follow-smooth", "Smooth Scroll", |_, s, cx| {
        s.set_timeline_follow(Follow::Smooth, cx)
    })
    .checked(|_, s| s.ui_settings.timeline_display.follow == Follow::Smooth)
    .under(TIMELINE_DISPLAY),
];

/// The View submenu holding the timeline's thumbnail, waveform, and follow
/// choices.
pub(crate) const TIMELINE_DISPLAY: &str = "Timeline Display Settings";

/// The View submenu holding the Viewer's split, rulers, guides, safe margins
/// and grids.
pub(crate) const VIEWER_DISPLAY: &str = "Viewer Display Settings";

/// The View submenu choosing what the scopes' waveform monitor shows.
pub(crate) const SCOPE_WAVEFORM: &str = "Scope Waveform";

/// The View row showing `mode` in the scopes' waveform monitor.
const fn waveform_row(mode: WaveformMode) -> MenuItem {
    MenuItem::new(mode.menu_id(), mode.label(), run_waveform_row)
        .checked(waveform_row_checked)
        .under(SCOPE_WAVEFORM)
}

/// Show the waveform mode whose row ran.
fn run_waveform_row(id: &str, state: &mut AppState, cx: &mut gpui::Context<AppState>) {
    if let Some(mode) = WaveformMode::ALL
        .into_iter()
        .find(|mode| mode.menu_id() == id)
    {
        state.set_waveform_mode(mode, cx);
    }
}

/// Whether the row is the waveform mode shown.
fn waveform_row_checked(id: &str, state: &AppState) -> bool {
    state.waveform_mode().menu_id() == id
}

pub(super) fn view_items(_state: &AppState) -> MenuItems {
    Cow::Borrowed(VIEW_ITEMS)
}
