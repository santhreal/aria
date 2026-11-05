//! Cancelled-render scratch return contract: decoded CPU frames from earlier
//! layers in a cancelled multi-layer scene render return to their decoder slots
//! so the next render at the same source time reuses them without re-decoding.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_models::{MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::*;

const FPS_30: Rational = Rational::FPS_30;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-30fps-3s.mp4")
}

/// WHY: when a scene render was cancelled after decoding a media layer (for
/// example when the startup GPU probe or a rapid scrub superseded an
/// in-flight preview generation), `submit_scene_once` restored
/// `realize_scratch` without calling `return_scratch`, so the next render
/// at the same `source_time` cleared the decoded `Shown` frame in
/// `begin_frame()` and re-decoded the GOP from scratch. Closes the class of
/// cancelled renders dropping already-decoded CPU `Shown` frames instead of
/// returning them to their `DecoderSlot`.
#[test]
fn cancelled_render_returns_decoded_frames_to_their_slots_for_reuse() {
    struct CancelAfterDecode {
        inner: Box<dyn VideoDecoder>,
        calls: Arc<AtomicUsize>,
        cancel: aria_core::Cancel,
        trip_once: bool,
    }

    impl VideoDecoder for CancelAfterDecode {
        fn info(&self) -> &aria_core::SourceInfo {
            self.inner.info()
        }

        fn seek(&mut self, target: RationalTime) -> Result<(), aria_core::DecodeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.seek(target)
        }

        fn next_frame(&mut self) -> Result<Option<VideoFrame>, aria_core::DecodeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.inner.next_frame()
        }

        fn frame_at(
            &mut self,
            target: RationalTime,
        ) -> Result<Option<VideoFrame>, aria_core::DecodeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let out = self.inner.frame_at(target);
            if self.trip_once {
                self.trip_once = false;
                self.cancel.cancel();
            }
            out
        }

        fn frame_at_nearest(
            &mut self,
            target: RationalTime,
        ) -> Result<Option<VideoFrame>, aria_core::DecodeError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let out = self.inner.frame_at_nearest(target);
            if self.trip_once {
                self.trip_once = false;
                self.cancel.cancel();
            }
            out
        }
    }

    let mut project = Project::new("decode ahead", FPS_30);
    let media = project.add_media(MediaSource::new(fixture(), 640, 360, FPS_30, 90, false));
    let v1 = project.add_track(TrackKind::Video, "V1");
    let v2 = project.add_track(TrackKind::Video, "V2");
    let a = project
        .add_clip(
            v1,
            media,
            TimeRange::at_rate(0, 30, FPS_30),
            RationalTime::new(0, FPS_30),
        )
        .expect("v1 clip");
    project
        .add_clip(
            v2,
            media,
            TimeRange::at_rate(0, 30, FPS_30),
            RationalTime::new(0, FPS_30),
        )
        .expect("v2 clip");

    let Ok(mut renderer) = Renderer::new_headless() else {
        return;
    };
    let key = DecoderKey {
        media,
        clip: Some(a),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let cancel = aria_core::Cancel::new();
    let inner = aria_decoder::open_video_decoder(&fixture(), aria_decoder::OutputMode::Cpu)
        .unwrap_or_else(|error| panic!("fixture: {error}"));
    let decoder = AheadDecoder::new(Box::new(CancelAfterDecode {
        inner,
        calls: Arc::clone(&calls),
        cancel: cancel.clone(),
        trip_once: true,
    }));
    renderer
        .decoders
        .insert(key, DecoderSlot::new(Some(decoder), None, 0));
    let settled = |renderer: &Renderer| {
        if let Some(decoder) = &renderer.decoders[&key].decoder {
            decoder.idle();
        }
        calls.load(Ordering::SeqCst)
    };

    let mut sink = aria_compositor::ImageSink::default();
    renderer.cancel_next_frame(cancel);
    let cancelled = renderer.render_frame_fit_into_with(
        &project,
        RationalTime::new(0, FPS_30),
        160,
        90,
        crate::resolve::ResolveOverrides::default(),
        SeekPolicy::Exact,
        &mut sink,
    );
    assert!(
        matches!(cancelled, Err(crate::RenderError::Cancelled)),
        "expected Cancelled after decode, got {cancelled:?}"
    );
    assert_eq!(
        renderer.decoders[&key]
            .shown
            .as_ref()
            .map(|shown| shown.source_time),
        Some(RationalTime::new(0, FPS_30)),
        "a cancelled multi-layer render must return already-decoded frames to their slots instead of dropping them"
    );
    let after_cancelled = settled(&renderer);
    assert!(
        after_cancelled > 0,
        "the cancelled attempt decoded the frame"
    );

    renderer
        .render_frame_fit_into_with(
            &project,
            RationalTime::new(0, FPS_30),
            160,
            90,
            crate::resolve::ResolveOverrides::default(),
            SeekPolicy::Exact,
            &mut sink,
        )
        .expect("retry at same tick succeeds");
    assert_eq!(
        settled(&renderer),
        after_cancelled,
        "retry after a cancelled render must reuse the already-decoded Shown frame without re-decoding"
    );
}
