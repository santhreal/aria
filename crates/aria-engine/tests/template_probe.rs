//! An apply of a template probes its picks together.
//!
//! WHY: `ApplyTemplate` probed one pick after another with the engine held, so
//! a template of eight slots on a network drive kept the window and the
//! preview waiting for the sum of eight probes. The decoder here holds each
//! probe until every pick is being probed, so the probes are observed running
//! together, not timed; it runs on every platform, where the fill test above
//! needs the native encoder. Not caught: a file picked for two slots, which is
//! probed once per slot.

use std::path::Path;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use aria_core::{DecodeError, VideoDecoder};
use aria_decoder::{ExternalDecoder, MediaProbe, register_external_decoder};

use super::*;

/// Long enough that only probes run one after another miss it.
const TOGETHER: Duration = Duration::from_secs(5);

/// A decoder whose probes each wait until `picks` probes have started.
struct Gathering {
    picks: usize,
    /// Probes started, and how many of them saw every pick probing at once.
    state: Mutex<(usize, usize)>,
    started: Condvar,
}

impl ExternalDecoder for Gathering {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        let mut state = self.state.lock().expect("the probe count");
        state.0 += 1;
        self.started.notify_all();
        let deadline = Instant::now() + TOGETHER;
        while state.0 < self.picks {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            state = self
                .started
                .wait_timeout(state, left)
                .expect("the probe count")
                .0;
        }
        if state.0 >= self.picks {
            state.1 += 1;
        }
        Ok(MediaProbe {
            width: 640,
            height: 360,
            frame_rate: Rational::FPS_24,
            frame_count: 48,
            has_audio: false,
            is_image: false,
            video_codec: None,
            audio_codec: None,
        })
    }

    fn open_video(&self, _path: &Path) -> Result<Box<dyn VideoDecoder>, DecodeError> {
        Err(DecodeError::unsupported("this decoder only probes"))
    }
}

#[test]
fn an_apply_probes_its_picks_together_and_fills_each_slot_with_its_pick() {
    let mut a = authoring_engine();
    author_template_surface(&mut a);
    let dir = tempfile::tempdir().expect("tempdir");
    let template_path = dir.path().join("intro.ariat");
    a.engine
        .apply(Command::Project(ProjectCommand::SaveTemplate {
            path: template_path.clone(),
            meta: TemplateMeta::new("Trendy Intro"),
        }))
        .expect("save template");
    let slots = [a.slot_a, a.slot_b];
    let takes: Vec<_> = (0..slots.len())
        .map(|index| {
            let path = dir.path().join(format!("take-{index}.slotgather"));
            std::fs::write(&path, b"take").expect("write the take");
            dunce::canonicalize(&path).expect("the take exists")
        })
        .collect();
    let decoder = Arc::new(Gathering {
        picks: slots.len(),
        state: Mutex::new((0, 0)),
        started: Condvar::new(),
    });
    let _registered = register_external_decoder(
        "slotgather",
        &["slotgather".to_owned()],
        100,
        Arc::clone(&decoder) as Arc<dyn ExternalDecoder>,
    );

    let (_dir, mut engine) = common::temp_engine();
    let out = engine
        .apply(Command::Project(ProjectCommand::ApplyTemplate {
            path: template_path,
            picks: takes
                .iter()
                .map(|path| TemplatePick {
                    path: path.clone(),
                    source_in: None,
                })
                .collect(),
        }))
        .expect("apply template");

    assert!(matches!(out, ApplyOutcome::AppliedTemplate));
    let (probes, together) = *decoder.state.lock().expect("the probe count");
    assert_eq!(probes, slots.len(), "one probe per pick");
    assert_eq!(
        together,
        slots.len(),
        "every probe of the apply runs while the others are running"
    );
    for (slot, take) in slots.into_iter().zip(&takes) {
        match &engine.project().clip(slot).expect("the slot").content {
            ClipSource::Media { media, .. } => assert_eq!(
                engine.project().media(*media).expect("the pick").path(),
                take.as_path(),
                "each slot plays its own pick, in slot order"
            ),
            other => panic!("slot not filled: {other:?}"),
        }
    }
}
