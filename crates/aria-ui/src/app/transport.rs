//! Transport: the playhead, playback, shuttle, and the frame clock.
//!
//! Split out of `app.rs` unchanged. `spawn_preview` and `start_playback` live
//! here too, so the whole clock is one file.

use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_core::RgbaImage;
use aria_engine::EngineError;
use aria_models::RationalTime;
use gpui::{App, Entity};

use crate::frame::rgba_to_render;
use crate::projection::project_to_ui;
use crate::types::UiEditorStore;

use super::{AppState, FrameReady};

impl AppState {
    pub fn set_playhead(&mut self, ticks: i64) {
        let rate = self.playhead.rate;
        let duration = self.store.sequence.duration_ticks.max(0);
        self.playhead = RationalTime::new(ticks.clamp(0, duration), rate);
        let tick = self.playhead.value;
        self.store_mut().playhead_ticks = tick;
        self.refresh_clock_text();
        self.interaction.touch();
    }

    pub fn toggle_playback(&mut self) {
        self.playing = !self.playing;
        self.shuttle = 1;
        self.interaction.set_playing(self.playing);
        if self.playing {
            self.playback_generation = self.playback_generation.saturating_add(1);
            self.playback_anchor_tick = self.playhead.value;
            self.playback_started = Some(Instant::now());
            self.playback = aria_render::PlaybackMeter::at_rate(self.shuttle_rate());
            self.republish_audio();
        } else {
            self.playback_started = None;
            self.audio.publish_stop();
        }
    }

    pub fn set_shuttle(&mut self, speed: i32) {
        if speed == 0 {
            if self.playing {
                self.playing = false;
                self.interaction.set_playing(false);
                self.playback_started = None;
                self.audio.publish_stop();
            }
            self.shuttle = 1;
            return;
        }
        // The ladder climbs in the direction of the press, not from the rate
        // that happens to be showing: L at 2x is 4x, because the operator is
        // asking for more of what is already playing. Matching the exact rate
        // instead made every second press fall back to 1x, so L toggled 1x and
        // 2x forever and 4x was unreachable from the keyboard.
        if self.playing && self.shuttle.signum() == speed.signum() {
            self.shuttle = self.shuttle.saturating_mul(2).clamp(-8, 8);
        } else {
            self.shuttle = speed.clamp(-8, 8);
        }
        self.playing = true;
        self.interaction.set_playing(true);
        self.playback_generation = self.playback_generation.saturating_add(1);
        self.playback_anchor_tick = self.playhead.value;
        self.playback_started = Some(Instant::now());
        // A new ladder step is a new rate: a report that mixed 1x and 4x frames
        // would say playback missed a target nobody asked for.
        self.playback = aria_render::PlaybackMeter::at_rate(self.shuttle_rate());
        self.republish_audio();
    }

    /// The rate playback is trying to hold: the sequence's rate times the
    /// shuttle step, in either direction.
    ///
    /// Reverse is the same rate as forward — 4x back needs four frames per frame
    /// interval just as 4x forward does — so the sign is dropped rather than
    /// making the target negative.
    pub(crate) fn shuttle_rate(&self) -> aria_core::Rational {
        let rate = self.playhead.rate;
        let step = self.shuttle.unsigned_abs().max(1) as i32;
        aria_core::Rational::new(rate.num.saturating_mul(step), rate.den)
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
        self.shuttle == 1
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
                self.shuttle,
                1,
            )
        };
        let duration = self.store.sequence.duration_ticks.max(0);
        if duration == 0 || tick >= duration || tick <= 0 && self.shuttle < 0 {
            let end = if self.shuttle < 0 { 0 } else { duration };
            self.set_playhead(end);
            self.follow_playhead();
            self.playing = false;
            self.playback_started = None;
            self.interaction.set_playing(false);
            self.audio.publish_stop();
            // A stopped transport is at 1x, the same rule K applies: the rate is
            // what playback is doing, so a rate left standing over a stopped
            // playhead would keep the badge on screen and mute the next press of
            // space until something set it back.
            self.shuttle = 1;
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

/// Decode the frame at the playhead now, then hand the turn to whatever was
/// asked for while it ran.
fn decode_preview(state: Entity<AppState>, cx: &mut App) {
    let engine = state.read(cx).engine().clone();
    let playhead = state.read(cx).playhead;
    let generation = state.read(cx).preview_generation;
    let cancel = state.update(cx, |state, _| state.arm_preview_cancel());
    cx.spawn(async move |cx| {
        // Wall clock around the whole frame, because that is what the operator
        // waits for: the thread hand-off and the commit are as much of a late
        // frame as the decode is.
        let started = Instant::now();
        // The decode gets its own thread so a codec that panics cannot take the
        // window with it, and the wait for that thread is as long as the decode:
        // waiting on the foreground executor would stop every animation, and the
        // splash, for the length of a frame.
        let (rendered, stats) = crate::handoff::on_a_thread(
            cx.background_executor(),
            "aria-preview-decode",
            move || {
                // Token and frame under one lock: a token set while another caller
                // held the engine would be spent by that caller's frame.
                let mut engine = engine.lock();
                engine.cancel_next_frame(cancel);
                let frame = engine.get_frame(playhead);
                // The split belongs to this frame, so it is taken under the lock
                // that produced it and travels with it. Nothing on the drawing
                // side ever has to ask the session what the last frame cost.
                (frame, engine.last_frame_stats())
            },
        )
        .await
        .unwrap_or_else(|| {
            (
                Err(EngineError::Export("preview thread panicked".into())),
                aria_render::FrameStats::default(),
            )
        });
        let delivered = rendered.is_ok();
        let again = state.update(cx, |state, cx| {
            let again = state.release_preview();
            if state.preview_generation == generation {
                commit_preview(state, cx, rendered, stats);
                // Only a frame playback asked for and got counts. A scrub's
                // decodes are not playback, and a frame that failed or was
                // cancelled was never delivered, so charging playback for it
                // would report a rate nobody saw.
                if state.playing && delivered {
                    state.playback.frame(started.elapsed());
                }
            }
            again
        });
        if again {
            let _ = cx.update(|app| spawn_preview(state, app));
        }
    })
    .detach();
}

/// Put a decoded frame on screen, or say why there is none.
///
/// A cancelled frame is neither: the operator moved the playhead before it
/// arrived, the picture on screen is the one from the previous stop, and the
/// decode the cancellation made room for is already starting. Reporting it
/// would put "cancelled" on the strip once per mouse move of a scrub.
fn commit_preview(
    state: &mut AppState,
    cx: &mut gpui::Context<AppState>,
    rendered: Result<RgbaImage, EngineError>,
    stats: aria_render::FrameStats,
) {
    match rendered {
        Ok(frame) => {
            state.last_render = rgba_to_render(&frame);
            state.last_frame = Some(Arc::new(frame));
            state.last_error = None;
            // What the frame cost is recorded with the frame. Every readout of
            // the split — the render-cost panel, the report's stage table, the
            // status line — reads this field, so none of them takes the session
            // lock the decode was holding a moment ago.
            state.frame_stats = stats;
            // The scopes describe the frame on screen, so they are remeasured
            // with it rather than on a timer.
            state.refresh_scopes();
            cx.emit(FrameReady);
            cx.notify();
        }
        Err(err) if err.is_cancelled() => {}
        Err(err) => {
            state.report_error(&err);
            cx.notify();
        }
    }
}

/// Drive the transport from a frame timer for as long as the state says it is
/// playing.
///
/// Safe to call whenever something may have started playback, including while a
/// clock is already running: each call takes the transport's generation, so an
/// earlier loop sees a generation that is no longer its own and exits on its
/// next tick. One clock advances the playhead, never two.
pub fn start_playback(state: Entity<AppState>, cx: &mut App) {
    if !state.read(cx).playing {
        return;
    }
    let generation = state.update(cx, |state, _| {
        state.playback_generation = state.playback_generation.saturating_add(1);
        state.playback_generation
    });
    let period = state.read(cx).frame_period();
    spawn_preview(state.clone(), cx);
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(period).await;
            let keep = state.update(cx, |state, cx| {
                if !state.playing || state.playback_generation != generation {
                    return false;
                }
                let advanced = state.advance_playhead();
                cx.notify();
                advanced
            });
            if !keep {
                break;
            }
            cx.update(|app| {
                spawn_preview(state.clone(), app);
            });
        }
    })
    .detach();
}

/// Empty-project helper used by unit tests that do not need a GPU renderer.
pub fn project_store(project: &aria_models::Project) -> UiEditorStore {
    project_to_ui(
        project,
        RationalTime::new(0, project.timeline().frame_rate),
        &[],
    )
}
