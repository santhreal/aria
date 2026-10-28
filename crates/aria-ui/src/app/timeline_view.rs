//! The Timeline Display Settings: thumbnails, waveform scale, playhead
//! follow, what a zoom bar knob holds still, and the outline column's width.
//!
//! Each choice is a view preference saved with the UI settings, not an edit,
//! so none of them enters the undo history. The View menu rows and the
//! timeline header's display button call these setters, and Settings >
//! Timeline calls the zoom bar's; the timeline reads
//! [`crate::timeline_display::TimelineDisplay`] when it draws and when the
//! transport follows the playhead, and the zoom bar reads its knobs' setting
//! when a knob is pressed. The display buttons open their View submenu, this
//! one or the Viewer's, through [`AppState::toggle_display_menu`].

use super::AppState;
use crate::chrome::menu_items::{DropEntry, drop_entries};
use crate::timeline_display::{Follow, Thumbnails, WaveformScale, ZoomBarKnobs};
use crate::types::AppMenu;

impl AppState {
    /// Open the View menu's submenu `caption` under the button `owner`, the
    /// first choice lit so the arrows and Enter walk the choices, or close it
    /// when `owner` already holds it out. The Timeline header's and the
    /// Viewer's display buttons run this.
    pub fn toggle_display_menu(&mut self, owner: &'static str, caption: &'static str) {
        if self.menu_owner == Some(owner) {
            self.close_menu();
            return;
        }
        let entries = drop_entries(&self.menu_rows(AppMenu::View));
        let Some(at) = entries
            .iter()
            .position(|entry| matches!(entry, DropEntry::Sub { caption: c, .. } if *c == caption))
        else {
            return;
        };
        self.close_context();
        self.open_menu = Some(AppMenu::View);
        self.menu_row = Some(at);
        self.menu_flyout = Some((at, Some(0)));
        self.menu_owner = Some(owner);
    }

    /// The caption of the View submenu that is out, or `None` when none is.
    #[cfg(test)]
    pub fn display_menu(&self) -> Option<&'static str> {
        if self.open_menu != Some(AppMenu::View) {
            return None;
        }
        let (at, _) = self.menu_flyout?;
        match drop_entries(&self.menu_rows(AppMenu::View)).get(at) {
            Some(DropEntry::Sub { caption, .. }) => Some(caption),
            _ => None,
        }
    }

    /// Draw video clips with `thumbnails`.
    pub fn set_timeline_thumbnails(
        &mut self,
        thumbnails: Thumbnails,
        cx: &mut gpui::Context<Self>,
    ) {
        self.ui_settings.timeline_display.thumbnails = thumbnails;
        self.status_message = Some(format!("Timeline thumbnails: {}", thumbnails.label()));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Draw audio waveforms on `scale`.
    pub fn set_waveform_scale(&mut self, scale: WaveformScale, cx: &mut gpui::Context<Self>) {
        self.ui_settings.timeline_display.waveform = scale;
        self.status_message = Some(format!("Timeline waveforms: {}", scale.label()));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Follow the playhead during playback with `follow`.
    pub fn set_timeline_follow(&mut self, follow: Follow, cx: &mut gpui::Context<Self>) {
        self.ui_settings.timeline_display.follow = follow;
        // The next follow measures from wherever the view is now.
        self.follow_scroll = None;
        self.status_message = Some(format!("Timeline follow: {}", follow.label()));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Hold `knobs` still while a knob of the zoom bar is dragged.
    pub fn set_zoom_bar_knobs(&mut self, knobs: ZoomBarKnobs, cx: &mut gpui::Context<Self>) {
        self.ui_settings.zoom_bar_knobs = knobs;
        self.status_message = Some(format!("Zoom bar knobs: {}", knobs.label()));
        self.persist_settings_later(cx);
        cx.notify();
    }

    /// Draw the outline column `width` pixels wide, held to the settings range.
    /// A drag calls this per frame and saves the settings once, on release.
    pub fn set_outline_width(&mut self, width: f32) {
        self.ui_settings.outline_width = crate::settings::clamp_outline(width);
    }

    /// The outline column's width as drawn: the setting, narrowed so the
    /// Timeline frame keeps `OUTLINE_TIME_MIN` px of time area, and never
    /// under the settings minimum. Every reader of the column's width goes
    /// through here, so the header, the lanes and the pointer mapping agree.
    pub fn outline_drawn_width(&self) -> f32 {
        drawn_outline(
            self.ui_settings.outline_width,
            self.timeline_frame.map(|[_, _, w, _]| w),
        )
    }
}

/// The time area a narrow Timeline keeps beside the outline column.
pub const OUTLINE_TIME_MIN: f32 = 160.0;

/// The drawn outline width for the `setting` in a Timeline frame `panel_w` px
/// wide, or the setting before the frame is measured.
pub fn drawn_outline(setting: f32, panel_w: Option<f32>) -> f32 {
    let room = panel_w.map_or(setting, |w| w - OUTLINE_TIME_MIN);
    setting.min(room).max(crate::settings::OUTLINE_MIN)
}

#[cfg(test)]
#[path = "timeline_view_tests.rs"]
mod tests;
