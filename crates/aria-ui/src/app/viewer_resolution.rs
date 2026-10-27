//! The Viewer's playback and paused resolution: the size each frame is
//! rendered at, and the settings that choose it. See [`crate::resolution`].

use crate::resolution::{PausedResolution, PlaybackResolution, Resolution};

use super::{AppState, spawn_preview};

impl AppState {
    /// The level playback renders at: the fixed setting, or Auto's level.
    pub fn playback_resolution_level(&self) -> Resolution {
        self.ui_settings
            .playback_resolution
            .fixed()
            .unwrap_or_else(|| self.auto_resolution.level())
    }

    /// The playback resolution as the status strip and the Viewer show it:
    /// `1/2`, or `Auto 1/2` with the level Auto is at.
    pub fn playback_resolution_text(&self) -> String {
        match self.ui_settings.playback_resolution {
            PlaybackResolution::Auto => format!("Auto {}", self.auto_resolution.level().label()),
            fixed => fixed.label().to_owned(),
        }
    }

    /// The size playback frames fit within; `None` renders at canvas size.
    pub(crate) fn playback_fit(&self) -> Option<(u32, u32)> {
        self.playback_resolution_level()
            .fit(self.canvas_size_tuple())
    }

    /// The size a Viewer frame fits within now: the playback resolution while
    /// playing, the paused resolution otherwise.
    pub(crate) fn preview_fit(&self) -> Option<(u32, u32)> {
        if self.playback_cadence.is_some() {
            self.playback_fit()
        } else {
            self.ui_settings
                .paused_resolution
                .resolution()
                .fit(self.canvas_size_tuple())
        }
    }

    /// Set the playback resolution and save it. A run in progress renders
    /// ahead at the new size; frames already rendered at the old size are
    /// still shown.
    pub fn set_playback_resolution(
        &mut self,
        choice: PlaybackResolution,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_settings.playback_resolution == choice {
            return;
        }
        self.ui_settings.playback_resolution = choice;
        self.persist_settings_later(cx);
        if let Some(cadence) = self.playback_cadence {
            self.preview_worker
                .render_ahead(cadence, self.playback_shown, self.playback_fit());
        }
        self.status_message = Some(format!("Playback resolution: {}", choice.label()));
        cx.notify();
    }

    /// Set the paused resolution, save it, and render the frame on screen
    /// again at it when playback is stopped.
    pub fn set_paused_resolution(
        &mut self,
        choice: PausedResolution,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.ui_settings.paused_resolution == choice {
            return;
        }
        self.ui_settings.paused_resolution = choice;
        self.persist_settings_later(cx);
        if self.playback_cadence.is_none() {
            let state = cx.entity();
            cx.defer(move |app| spawn_preview(state, app));
        }
        self.status_message = Some(format!("Paused resolution: {}", choice.label()));
        cx.notify();
    }

    /// The next playback resolution in [`PlaybackResolution::ALL`], wrapping.
    pub fn cycle_playback_resolution(&mut self, cx: &mut gpui::Context<Self>) {
        let all = PlaybackResolution::ALL;
        let at = all
            .iter()
            .position(|choice| *choice == self.ui_settings.playback_resolution)
            .unwrap_or(0);
        self.set_playback_resolution(all[(at + 1) % all.len()], cx);
    }

    /// Switch the paused resolution between Full and 1/2.
    pub fn toggle_paused_resolution(&mut self, cx: &mut gpui::Context<Self>) {
        let next = match self.ui_settings.paused_resolution {
            PausedResolution::Full => PausedResolution::Half,
            PausedResolution::Half => PausedResolution::Full,
        };
        self.set_paused_resolution(next, cx);
    }
}
