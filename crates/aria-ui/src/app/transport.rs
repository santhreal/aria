//! Transport: the playhead, playback, shuttle, and the frame clock.
//!
//! `spawn_preview` and `start_playback` live here too, so the whole clock is
//! one file.
//!
//! Playback runs on the display's vsync. Each vsync reads the master clock
//! (the audio device while the mixer follows the transport, the monotonic
//! clock otherwise), moves the playhead to it, and puts the newest due frame
//! the preview worker has rendered ahead on screen. A frame that was due and
//! not ready is dropped and counted with its cause; the previous frame stays
//! on screen.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_engine::EngineError;
use aria_models::RationalTime;
use aria_render::DroppedFrames;
use gpui::{App, Entity, WeakEntity, Window};

use crate::preview_worker::{Cadence, PreviewFrame};
use crate::projection::project_to_ui;
use crate::types::UiEditorStore;

use super::{AppState, FrameReady};
use crate::transport::ShuttleSpeed;

/// A gap between two vsyncs longer than this is a window that was not drawn,
/// hidden or minimized, rather than a run of drops: the slots that came due in
/// it are not counted.
const PRESENT_GAP: Duration = Duration::from_millis(250);

impl AppState {
    pub fn set_playhead(&mut self, ticks: i64) {
        let rate = self.playhead.rate;
        let duration = self.store.sequence.duration_ticks.max(0);
        let before = self.playhead.value;
        self.playhead = RationalTime::new(ticks.clamp(0, duration), rate);
        let tick = self.playhead.value;
        // Paused, the frame on screen is the one at the old playhead. The
        // shell's next draw asks for the frame here, whichever key, menu row,
        // palette row or plugin moved it; a control that asks for the frame
        // itself clears the mark with its request. Playback shows its own
        // frames.
        if tick != before && !self.playing {
            self.still_stale = true;
        }
        self.store_mut().playhead_ticks = tick;
        self.pointer_playhead = None;
        self.refresh_clock_text();
        self.interaction.touch();
        self.on_playback_tick_automation(tick);
        // The selected clip's driven values and its canvas quad are sampled at
        // the playhead. Nothing else polls between two moves, so the poll runs
        // here; a move with nothing else changed takes the playhead-only path,
        // one resolve and no re-projection.
        self.sync_from_engine();
    }

    pub fn scrub_playhead(&mut self, ticks: i64) {
        self.set_playhead(ticks);
        self.queue_audio_scrub();
    }

    /// Scrub to where a pointer is: the ruler, the Viewer's scrub strip, a
    /// property row's strip. The timeline draws a playhead the pointer placed
    /// at its target with no glide, so it stays under the hand; a keyed step
    /// still eases.
    pub fn scrub_playhead_to_pointer(&mut self, ticks: i64) {
        self.scrub_playhead(ticks);
        self.pointer_playhead = Some(self.playhead.value);
    }

    /// Whether the playhead stands where a pointer last put it.
    pub(crate) fn playhead_under_pointer(&self) -> bool {
        self.pointer_playhead == Some(self.playhead.value)
    }

    pub fn queue_audio_scrub(&mut self) {
        if self.playing || !self.ui_settings.audio_scrubbing {
            return;
        }
        let mixer = if self.audio.pending_scrub_grains() == 0 {
            let engine = self.engine.lock();
            let built = aria_render::ExportAudioMixer::for_project(engine.project())
                .map_err(|err| format!("this project's audio routing has no mix: {err}"));
            Some(Arc::new(parking_lot::Mutex::new(Some(built))))
        } else {
            None
        };
        self.audio.queue_scrub(crate::audio::ScrubGrain {
            mixer,
            tick: self.playhead.value,
            fps_num: self.playhead.rate.num,
            fps_den: self.playhead.rate.den,
        });
    }

    /// Active work area `[start, end]` in sequence ticks: `mark_in` to
    /// `mark_out`, falling back to the sequence bounds when unset.
    pub fn work_area(&self) -> (i64, i64) {
        let duration = self.store.sequence.duration_ticks.max(0);
        let start = self.mark_in.unwrap_or(0).clamp(0, duration);
        let end = self
            .mark_out
            .filter(|&out| out > start)
            .unwrap_or(duration)
            .clamp(start, duration);
        (start, end)
    }

    pub fn toggle_loop(&mut self) {
        self.looping = !self.looping;
        let end = self.active_playback_end();
        if let Some(cadence) = self.playback_cadence.as_mut() {
            cadence.end = end;
        }
    }

    pub(super) fn active_playback_end(&self) -> i64 {
        if self.work_area_playback || self.looping {
            self.work_area().1
        } else {
            self.store.sequence.duration_ticks.max(0)
        }
    }

    pub fn toggle_playback(&mut self) {
        self.playing = !self.playing;
        self.work_area_playback = false;
        self.k_crawl = 0;
        self.shuttle = ShuttleSpeed::ONE;
        self.interaction.set_playing(self.playing);
        if self.playing {
            self.clear_mixer_automation_pass();
            self.playback_generation = self.playback_generation.saturating_add(1);
            self.playback_anchor_tick = self.playhead.value;
            self.playback_started = Some(Instant::now());
            self.playback = aria_render::PlaybackMeter::at_rate(self.shuttle_rate());
            self.republish_audio();
        } else {
            self.clear_mixer_automation_pass();
            self.playback_started = None;
            self.audio.publish_stop();
            self.stop_presenting();
        }
    }

    /// Play the work area (`mark_in` to `mark_out`, or the whole sequence when
    /// unset) from its start at 1x.
    pub fn play_work_area(&mut self) {
        let (start, end) = self.work_area();
        self.set_playhead(start);
        self.k_crawl = 0;
        self.shuttle = ShuttleSpeed::ONE;
        if end <= start {
            self.playing = false;
            self.work_area_playback = false;
            self.interaction.set_playing(false);
            self.playback_started = None;
            self.audio.publish_stop();
            self.stop_presenting();
            return;
        }
        self.work_area_playback = true;
        self.playing = true;
        self.interaction.set_playing(true);
        self.playback_generation = self.playback_generation.saturating_add(1);
        self.clear_mixer_automation_pass();
        self.playback_anchor_tick = start;
        self.playback_started = Some(Instant::now());
        self.playback = aria_render::PlaybackMeter::at_rate(self.shuttle_rate());
        self.republish_audio();
        if self.playback_cadence.is_some() {
            self.arm_presenting();
        }
    }

    fn stop_transport(&mut self) {
        if self.playing {
            self.clear_mixer_automation_pass();
            self.playing = false;
            self.interaction.set_playing(false);
            self.playback_started = None;
            self.audio.publish_stop();
            self.stop_presenting();
        }
        self.work_area_playback = false;
        self.k_crawl = 0;
        self.shuttle = ShuttleSpeed::ONE;
    }

    fn apply_shuttle_speed(&mut self, speed: ShuttleSpeed) {
        let speed = speed.clamp_range();
        if speed.num == 0 {
            self.stop_transport();
            return;
        }
        self.shuttle = speed;
        self.playing = true;
        self.interaction.set_playing(true);
        self.playback_generation = self.playback_generation.saturating_add(1);
        self.playback_anchor_tick = self.playhead.value;
        self.playback_started = Some(Instant::now());
        self.playback = aria_render::PlaybackMeter::at_rate(self.shuttle_rate());
        self.republish_audio();
        if self.playback_cadence.is_some() {
            self.arm_presenting();
        }
    }

    pub fn set_shuttle(&mut self, speed: i32) {
        if speed == 0 {
            self.k_held = false;
            self.stop_transport();
            return;
        }
        self.k_crawl = 0;
        let next = if self.playing && self.shuttle.signum() == speed.signum() {
            let doubled = self.shuttle.as_hundredths().saturating_mul(2);
            let min_step = speed.clamp(-8, 8).saturating_mul(100);
            let raw = if doubled.abs() < min_step.abs() {
                min_step
            } else {
                doubled
            };
            ShuttleSpeed::hundredths(raw).clamp_range()
        } else {
            ShuttleSpeed::steps(speed.clamp(-8, 8)).clamp_range()
        };
        self.apply_shuttle_speed(next);
    }

    pub fn set_shuttle_speed(&mut self, speed: ShuttleSpeed) {
        self.k_crawl = 0;
        self.apply_shuttle_speed(speed);
    }

    /// Step shuttle speed by 10% in the sign of `delta_sign` (`Shift+J` / `Shift+L`),
    /// crossing zero directly (`0.1x <-> -0.1x`) and clamping to `[-8x, 8x]`.
    pub fn step_shuttle_fine(&mut self, delta_sign: i32) {
        if delta_sign == 0 {
            return;
        }
        if self.source_has_keys() {
            self.shuttle_source(delta_sign);
            return;
        }
        self.k_crawl = 0;
        let next = self.shuttle.step_ten_percent(delta_sign);
        self.apply_shuttle_speed(next);
    }

    pub fn k_down(&mut self, is_held: bool) {
        self.k_held = true;
        if self.source_has_keys() {
            self.stop_source_playback();
            return;
        }
        if !is_held && self.k_crawl == 0 {
            self.stop_transport();
        }
    }

    pub fn k_up(&mut self) {
        self.k_held = false;
        if self.k_crawl != 0 {
            self.stop_transport();
        }
    }

    pub fn shuttle_key_down(&mut self, dir: i32, is_held: bool) {
        let sign = dir.signum();
        if sign == 0 {
            return;
        }
        if self.source_has_keys() {
            // K held with J or L steps a frame, as on the sequence; a held J or
            // L repeats nothing, since the source clock has no crawl speed.
            if !is_held {
                if self.k_held {
                    self.step_source(i64::from(sign));
                } else {
                    self.shuttle_source(sign);
                }
            }
            return;
        }
        if self.k_held {
            if !is_held {
                if self.playing {
                    self.stop_transport();
                }
                self.scrub_playhead(self.playhead.value + i64::from(sign));
            } else if self.k_crawl != sign || !self.playing {
                self.k_crawl = sign;
                let slow = if sign < 0 {
                    ShuttleSpeed::REVERSE_QUARTER
                } else {
                    ShuttleSpeed::QUARTER
                };
                self.apply_shuttle_speed(slow);
            }
        } else if !is_held {
            self.set_shuttle(sign);
        }
    }

    pub fn shuttle_key_up(&mut self, dir: i32) {
        if self.k_crawl != 0 && (dir == 0 || self.k_crawl == dir.signum()) {
            let k = self.k_held;
            self.stop_transport();
            self.k_held = k;
        }
    }

    /// The rate playback is trying to hold: the sequence's rate, at every
    /// shuttle step and in either direction.
    ///
    /// The shuttle changes which frames are shown, not how many: at 4x each
    /// frame period shows the frame four ticks on (see [`Cadence::step`]), so
    /// the display never has to present more frames per second than the
    /// sequence has.
    pub(crate) fn shuttle_rate(&self) -> aria_core::Rational {
        self.playhead.rate
    }

    /// The frames this run shows: from the playhead, `shuttle` ticks apart.
    pub(crate) fn playback_cadence(&self) -> Cadence {
        Cadence {
            anchor: self.playhead.value,
            speed_num: i64::from(self.shuttle.num),
            speed_den: i64::from(self.shuttle.den),
            end: self.active_playback_end(),
            rate: self.playhead.rate,
        }
    }

    /// Start counting a run's frames from the frame on screen. Called by
    /// [`start_playback`]; the worker drops what it rendered for any earlier
    /// run and starts rendering ahead of this one.
    pub(crate) fn arm_presenting(&mut self) -> Cadence {
        let cadence = self.playback_cadence();
        self.playback_cadence = Some(cadence);
        self.playback_shown = 0;
        self.last_present = None;
        self.auto_resolution.start(Instant::now());
        self.preview_worker.clear();
        self.preview_worker
            .render_ahead(cadence, 0, self.playback_fit());
        cadence
    }

    /// The project changed: drop the frames rendered from the old one and, under
    /// playback, render ahead again from where the run is, so the next frame
    /// shown has the edit in it. Paused, the frame on screen is marked stale
    /// for the shell's next draw to ask for again, whichever control made the
    /// edit.
    pub(crate) fn picture_changed(&mut self) {
        self.preview_worker.clear();
        self.request_render_bar_refresh();
        let end = self.active_playback_end();
        if let Some(cadence) = self.playback_cadence.as_mut() {
            cadence.end = end;
            let cadence = *cadence;
            self.preview_worker
                .render_ahead(cadence, self.playback_shown, self.playback_fit());
        } else {
            self.still_stale = true;
        }
    }

    /// Whether the editor shows a frame from before an edit or a playhead move
    /// made while paused, with no frame asked for since. Home draws no frame,
    /// so the mark waits for the editor.
    pub(crate) fn still_frame_stale(&self) -> bool {
        self.still_stale && self.route != crate::types::UiRoute::Home
    }

    /// Install the task that applies what the preview worker leaves in its
    /// mailbox. It ends when the session, and with it the worker, is gone.
    pub fn watch_preview_worker(&mut self, cx: &mut gpui::Context<Self>) {
        let wakes = self.preview_worker.wakes();
        cx.spawn(async move |this, cx| {
            while let Some(()) = wakes.next_from_a_thread(cx.background_executor()).await {
                while wakes.try_next().is_some() {}
                let updated = this.update(cx, |state, cx| {
                    if state.take_preview_mail() {
                        state.release_retired_images(None, cx);
                        cx.notify();
                    }
                });
                if updated.is_err() {
                    return;
                }
            }
        })
        .detach();
    }

    /// Stop rendering ahead and drop the frames rendered for this run.
    pub(crate) fn stop_presenting(&mut self) {
        if self.playback_cadence.take().is_some() {
            self.preview_worker.clear();
            self.measure_shown();
        }
        self.last_present = None;
    }

    /// Frames dropped since the indicator was last reset.
    pub fn dropped_frames(&self) -> DroppedFrames {
        self.dropped
    }

    /// Clear the dropped-frame indicator. The run's own report keeps its count.
    pub fn reset_dropped_frames(&mut self) {
        self.dropped = DroppedFrames::default();
    }

    /// One vsync of playback: move the playhead to the clock, show the newest
    /// due frame, count the slots that came due without one, and keep the next
    /// frames rendering. False once playback has stopped.
    pub(crate) fn present_playback(&mut self, cx: &mut gpui::Context<AppState>) -> bool {
        let before = self.playhead.value;
        if !self.advance_playhead() {
            self.stop_presenting();
            cx.notify();
            return false;
        }
        // Most vsyncs at a display rate above the sequence's land on the tick
        // the last one did, and change nothing worth a redraw.
        let mut changed = self.playhead.value != before;
        let Some(cadence) = self.playback_cadence else {
            if changed {
                cx.notify();
            }
            return true;
        };
        let now = cadence.slot_at(self.playhead.value);
        let at = Instant::now();
        // A window that was not drawn for a while did not miss its vsyncs; it
        // had none. Only the slot due now is counted.
        let mut shown = self.playback_shown;
        if self
            .last_present
            .is_some_and(|last| at.duration_since(last) > PRESENT_GAP)
        {
            shown = shown.max(now.saturating_sub(1));
        }
        self.last_present = Some(at);
        let presented = self.preview_worker.present(cadence, shown, now);
        self.playback_shown = self.playback_shown.max(now);
        let auto = self.ui_settings.playback_resolution.fixed().is_none();
        if !presented.dropped.is_empty() {
            self.count_drops(presented.dropped);
            if auto {
                self.auto_resolution.dropped(presented.dropped.total(), at);
            }
            changed = true;
        } else if auto && presented.frame.is_some() && self.auto_resolution.presented(at) {
            changed = true;
        }
        if let Some(frame) = presented.frame {
            self.playback.frame(frame.took);
            if let Some(started) = self.playback_started {
                self.playback.present(started.elapsed());
            }
            self.show_frame(frame, cx);
            changed = true;
        }
        self.preview_worker
            .render_ahead(cadence, now, self.playback_fit());
        if changed {
            cx.notify();
        }
        true
    }

    /// Charge `dropped` to the run and to the indicator, and write each cause
    /// to the log when Settings asks for it.
    fn count_drops(&mut self, dropped: DroppedFrames) {
        self.playback.drop_frames(dropped);
        self.dropped.add(dropped);
        if self.ui_settings.log_dropped_frames {
            for cause in aria_render::DropCause::ALL {
                let frames = dropped.count(cause);
                if frames > 0 {
                    tracing::info!(
                        target: "aria::playback",
                        tick = self.playhead.value,
                        frames,
                        "dropped {frames} frame(s) at tick {}: {cause}",
                        self.playhead.value
                    );
                }
            }
        }
    }

    /// Put a rendered Viewer frame on screen, with its scopes when it carries
    /// them.
    pub(crate) fn show_frame(&mut self, frame: PreviewFrame, cx: &mut gpui::Context<AppState>) {
        self.preview_worker.set_shown_texture(frame.texture);
        if let Some(replaced) = std::mem::replace(&mut self.last_render, frame.image) {
            self.retired_images.push(replaced);
        }
        self.last_frame = (!frame.rgba.pixels.is_empty()).then_some(frame.rgba);
        self.shown_tick = Some(frame.tick);
        self.probe_viewer();
        // What the frame cost is recorded with the frame. Every readout of the
        // split reads this field, so none of them takes the session lock.
        self.frame_stats = frame.stats;
        match frame.scopes {
            Some(scopes) => {
                self.adopt_scopes(Some(scopes));
            }
            // Playback measures at most `SCOPE_RATE_HZ` frames a second; the
            // scopes hold the last reading until the next one, and the frame
            // playback stops on is measured then.
            None if self.playback_cadence.is_some() => {}
            // The scopes were shown after this frame was rendered: measure it
            // now, on the worker.
            None => self.measure_shown(),
        }
        cx.emit(FrameReady);
    }

    /// Release the textures no longer on screen from the atlas of every window.
    ///
    /// `window` is the window being updated, when there is one: gpui holds it
    /// out of the app's window list for the length of the update, so a release
    /// that left it out would miss the window the Viewer draws in. Call it where
    /// no window is being updated, or with the one that is.
    pub(crate) fn release_retired_images(&mut self, mut window: Option<&mut Window>, cx: &mut App) {
        for image in self.retired_images.drain(..) {
            cx.drop_image(image, window.as_deref_mut());
        }
    }

    /// Apply what the preview worker left in its mailbox: a scope reading for
    /// the frame on screen, the colour under the pointer, and the reason a
    /// render-ahead frame failed. True when anything changed.
    pub(crate) fn take_preview_mail(&mut self) -> bool {
        let scopes = self.adopt_scopes(self.preview_worker.take_measured());
        let probed = self.adopt_probe(self.preview_worker.take_probed());
        let failed = match self.preview_worker.take_failure() {
            Some(reason) => {
                self.report_error(reason);
                true
            }
            None => false,
        };
        let render_bar = self.poll_render_bar();
        scopes || probed || failed || render_bar
    }

    /// What playback is managing against the rate it is trying to hold.
    ///
    /// Empty until a frame is delivered under playback: a scrub's decodes are
    /// not playback, and reporting them would call a paused editor slow.
    pub fn playback_report(&self) -> aria_render::PlaybackReport {
        self.playback.report()
    }

    /// Whether the mixer can honour the transport as it stands.
    ///
    /// The mixer plays a project forward at the sequence rate: it neither
    /// resamples nor reads backwards, and its own clock only counts up. At any
    /// other shuttle speed there is no audio that matches the picture, so the
    /// audio path goes quiet and the frame clock drives the playhead — which is
    /// what makes reverse move at all, since a running audio clock would pull
    /// the playhead forward again on the next frame.
    ///
    /// One reader for the whole transport: every site that republishes during
    /// playback asks this rather than deciding for itself.
    pub(crate) const fn audio_follows(&self) -> bool {
        self.shuttle.is_audible()
    }

    /// Hand the audio path the transport as it now stands: the snapshot while
    /// the mixer can follow it, silence while it cannot.
    ///
    /// A stop is idempotent on the audio side but bumps the stream generation,
    /// so a stopped transport is left alone rather than told again.
    pub(crate) fn republish_audio(&self) {
        if !self.playing {
            return;
        }
        if self.audio_follows() {
            self.audio.publish_play(self.playback_snapshot());
        } else {
            self.audio.publish_stop();
        }
    }

    pub fn advance_playhead(&mut self) -> bool {
        if !self.playing {
            return false;
        }
        // The audio clock is read only while the mixer is the one following the
        // transport. A stream is torn down on the shell's next pump, not the
        // moment it is told to stop, so a shuttle that trusted whatever tick was
        // in the clock would be dragged forward at 1x for as long as that took.
        let audio_tick = self
            .audio_follows()
            .then(|| self.audio.current_tick())
            .flatten();
        let tick = if let Some(audio_tick) = audio_tick {
            audio_tick
        } else {
            let Some(started) = self.playback_started else {
                return false;
            };
            crate::transport::playback_tick_scaled(
                self.playback_anchor_tick,
                started.elapsed().as_millis() as i64,
                self.playhead.rate.num,
                self.playhead.rate.den,
                self.shuttle.num,
                self.shuttle.den,
            )
        };
        let duration = self.store.sequence.duration_ticks.max(0);
        let (work_start, work_end) = self.work_area();
        let (bound_start, bound_end) = if self.work_area_playback || self.looping {
            (work_start, work_end)
        } else {
            (0, duration)
        };
        let hit_end = bound_end <= bound_start
            || (self.shuttle.num > 0 && tick >= bound_end)
            || (self.shuttle.num < 0 && tick <= bound_start);
        if hit_end {
            if self.looping && bound_end > bound_start {
                let wrap = if self.shuttle.num < 0 {
                    bound_end
                } else {
                    bound_start
                };
                self.set_playhead(wrap);
                self.follow_playhead();
                self.playback_anchor_tick = wrap;
                self.playback_started = Some(Instant::now());
                self.republish_audio();
                if self.playback_cadence.is_some() {
                    self.arm_presenting();
                }
                return true;
            }
            let end = if self.shuttle.num < 0 {
                bound_start
            } else {
                bound_end
            };
            self.set_playhead(end);
            self.follow_playhead();
            self.playing = false;
            self.work_area_playback = false;
            self.k_crawl = 0;
            self.playback_started = None;
            self.interaction.set_playing(false);
            self.audio.publish_stop();
            // A stopped transport is at 1x, the same rule K applies: the rate is
            // what playback is doing, so a rate left standing over a stopped
            // playhead would keep the badge on screen and mute the next press of
            // space until something set it back.
            self.shuttle = ShuttleSpeed::ONE;
            return false;
        }
        self.set_playhead(tick);
        self.follow_playhead();
        true
    }

    /// What the audio thread needs to mix from here.
    ///
    /// Only the audible spans and resolved bus graph cross the engine boundary.
    /// Republishing during playback therefore avoids copying the project AST.
    pub(crate) fn playback_snapshot(&self) -> crate::audio::PlaybackSnapshot {
        let mixer = {
            let engine = self.engine.lock();
            aria_render::ExportAudioMixer::for_project(engine.project())
                .map_err(|err| format!("this project's audio routing has no mix: {err}"))
        };
        crate::audio::PlaybackSnapshot {
            mixer: std::sync::Arc::new(parking_lot::Mutex::new(Some(mixer))),
            start_tick: self.playhead.value,
            fps_num: self.playhead.rate.num,
            fps_den: self.playhead.rate.den,
            speed_num: self.shuttle.num,
            speed_den: self.shuttle.den,
        }
    }

    pub fn frame_period(&self) -> Duration {
        let num = self.playhead.rate.num.max(1) as u64;
        let den = self.playhead.rate.den.max(1) as u64;
        Duration::from_nanos((1_000_000_000u64 * den) / num)
    }

    /// Ask for a preview decode. True when this request is the one that starts
    /// it; false when a decode is already running, in which case the request is
    /// remembered and the running decode takes it on when it lands.
    ///
    /// The generation advances on every request, running or not, so a decode
    /// that finishes after a newer request knows its frame is no longer wanted.
    /// A running decode is also cancelled here: knowing the frame is unwanted is
    /// worth little if the operator still waits for it, because the next decode
    /// cannot start until the lock comes back.
    pub(crate) fn claim_preview(&mut self) -> bool {
        // The frame asked for here, or the one a running decode starts when it
        // lands, is rendered from the project as it is now.
        self.still_stale = false;
        self.preview_generation = self.preview_generation.saturating_add(1);
        if self.preview_decoding {
            self.preview_wanted = true;
            self.preview_cancel.cancel();
            return false;
        }
        self.preview_decoding = true;
        true
    }

    /// The token the decode that is starting should honour. Minted here, so the
    /// one that cancels it and the one the frame reads are the same object.
    pub(crate) fn arm_preview_cancel(&mut self) -> aria_core::Cancel {
        self.preview_cancel = aria_core::Cancel::new();
        self.preview_cancel.clone()
    }

    /// A decode has landed. True when a frame was asked for while it ran, which
    /// is the caller's turn to start that one.
    pub(crate) fn release_preview(&mut self) -> bool {
        self.preview_decoding = false;
        // Nothing is in flight, so nothing is cancellable: a stale live token
        // would let a request cancel a decode that has already landed.
        self.preview_cancel = aria_core::Cancel::never();
        std::mem::take(&mut self.preview_wanted)
    }
}

/// Ask for the frame at the playhead.
///
/// One decode runs at a time. A request that arrives while one is running does
/// not start a second: it marks that another frame is wanted, and the decode in
/// flight starts it when it lands. A scrub calls this once per mouse move, and
/// every decode takes the engine lock for its whole length, so queueing them
/// meant the operator waited for a dozen frames nobody would see before the one
/// under the pointer arrived.
pub fn spawn_preview(state: Entity<AppState>, cx: &mut App) {
    if state.update(cx, |state, _| state.claim_preview()) {
        decode_preview(state, cx);
    }
}

/// Render the frame at the playhead now, then hand the turn to whatever was
/// asked for while it ran.
fn decode_preview(state: Entity<AppState>, cx: &mut App) {
    let (playhead, fit) = {
        let state = state.read(cx);
        (state.playhead, state.preview_fit())
    };
    let generation = state.read(cx).preview_generation;
    let cancel = state.update(cx, |state, _| state.arm_preview_cancel());
    // The frame renders on the session's preview worker, so a codec that
    // panics costs the frame rather than the window, and no thread is started
    // for it. The texture and, while the strip is open, the scopes are made
    // there too, so the window only swaps pointers.
    let rendered =
        state
            .read(cx)
            .preview_worker
            .frame(cx.background_executor(), playhead, fit, cancel);
    // Weak, so a frame still rendering when the window closes does not keep
    // the session alive.
    let state = state.downgrade();
    cx.spawn(async move |cx| {
        let rendered = rendered.await.unwrap_or_else(|| {
            Err(EngineError::Export(
                "the preview frame panicked while rendering; the log has the panic".into(),
            ))
        });
        let Ok(again) = state.update(cx, |state, cx| {
            let again = state.release_preview();
            if state.preview_generation == generation {
                commit_preview(state, cx, rendered);
            }
            again
        }) else {
            return;
        };
        if again && let Some(state) = state.upgrade() {
            cx.update(|app| spawn_preview(state, app));
        }
    })
    .detach();
}

/// Put a rendered frame on screen, or say why there is none.
///
/// A cancelled frame is neither: the operator moved the playhead before it
/// arrived, the picture on screen is the one from the previous stop, and the
/// render the cancellation made room for is already starting. Reporting it
/// would put "cancelled" on the strip once per mouse move of a scrub.
fn commit_preview(
    state: &mut AppState,
    cx: &mut gpui::Context<AppState>,
    rendered: Result<PreviewFrame, EngineError>,
) {
    match rendered {
        Ok(frame) => {
            state.show_frame(frame, cx);
            state.release_retired_images(None, cx);
            state.last_error = None;
            cx.notify();
        }
        Err(err) if err.is_cancelled() => {}
        Err(err) => {
            state.report_error(&err);
            cx.notify();
        }
    }
}

/// Drive the transport from the display's vsync for as long as the state says
/// it is playing.
///
/// Safe to call whenever something may have started playback, including
/// while a run is already presenting: each call takes the transport's
/// generation, so an earlier loop sees a generation that is no longer its own
/// and ends on its next vsync. One clock advances the playhead, never two.
///
/// The loop holds a weak state handle between vsyncs: a vsync still pending
/// when the app shuts down retains no entity.
pub fn start_playback(state: Entity<AppState>, window: &Window, cx: &mut App) {
    if !state.read(cx).playing {
        if let Some(token) = state.update(cx, |state, _| state.sources.drive()) {
            play_source_on_vsync(state.downgrade(), token, window);
        }
        return;
    }
    let (generation, armed) = state.update(cx, |state, _| {
        state.playback_generation = state.playback_generation.saturating_add(1);
        state.arm_presenting();
        (state.playback_generation, state.playback_vsyncs.clone())
    });
    let weak = state.downgrade();
    spawn_preview(state, cx);
    present_on_vsync(weak, generation, armed, window);
}

/// Present one playback frame after the next frame the window draws, and
/// schedule the one after that while the run is current. `armed` travels with
/// the pending callback and drops when the loop ends.
#[allow(
    clippy::only_used_in_recursion,
    reason = "`armed` moves into each pending callback so that it drops when the loop ends"
)]
fn present_on_vsync(
    state: WeakEntity<AppState>,
    generation: u64,
    armed: std::rc::Rc<()>,
    window: &Window,
) {
    window.on_next_frame(move |window, cx| {
        let keep = state
            .update(cx, |state, cx| {
                let keep = state.playback_generation == generation && state.present_playback(cx);
                state.release_retired_images(Some(&mut *window), cx);
                keep
            })
            .unwrap_or(false);
        if keep {
            present_on_vsync(state, generation, armed, window);
        }
    });
}

/// Move the playing Source tab to its clock after each frame the window draws,
/// while the loop's `token` is the tabs' driver. Starting playback again hands
/// the tab to a new loop, and this one ends on its next vsync.
fn play_source_on_vsync(state: WeakEntity<AppState>, token: u64, window: &Window) {
    window.on_next_frame(move |window, cx| {
        let keep = state
            .update(cx, |state, cx| {
                if !state.sources.driven_by(token) {
                    return false;
                }
                let before = state.sources.focused().map(|view| view.tick);
                let keep = state.advance_source_playback(Instant::now());
                // A vsync that lands on the frame already shown redraws nothing.
                if !keep || state.sources.focused().map(|view| view.tick) != before {
                    cx.notify();
                }
                keep
            })
            .unwrap_or(false);
        if keep {
            play_source_on_vsync(state, token, window);
        }
    });
}

/// Empty-project helper used by unit tests that do not need a GPU renderer.
pub fn project_store(project: &aria_models::Project) -> UiEditorStore {
    project_to_ui(
        project,
        RationalTime::new(0, project.timeline().frame_rate),
        &[],
    )
}
