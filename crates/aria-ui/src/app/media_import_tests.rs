//! A drop of several files probes them at the same time, and each import
//! adopts its file's probe instead of probing again.
//!
//! WHY: the class closed is "a multi-file import probes one file after
//! another on the window thread". A probe opens the file, which takes up to
//! seconds on a network drive, so a drop of a folder froze the window for the
//! sum of its probes. The decoder here holds each probe until every file of
//! the drop is being probed, so the probes are observed running together, not
//! timed. Not caught: the placement after the imports, which
//! `source_drop_tests` covers.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aria_core::{DecodeError, Rational, VideoDecoder};
use aria_decoder::{ExternalDecoder, MediaProbe, register_external_decoder};
use aria_plugin::PluginContext;
use parking_lot::{Condvar, Mutex};

use super::AppState;

/// Long enough that only probes run one after another miss it.
const TOGETHER: Duration = Duration::from_secs(5);

/// A decoder whose probes each wait until `files` probes have started.
struct GatheringDecoder {
    files: usize,
    /// Probes started, and how many of them saw every file probing at once.
    state: Mutex<(usize, usize)>,
    started: Condvar,
}

impl ExternalDecoder for GatheringDecoder {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        let mut state = self.state.lock();
        state.0 += 1;
        self.started.notify_all();
        let deadline = std::time::Instant::now() + TOGETHER;
        while state.0 < self.files {
            if self.started.wait_until(&mut state, deadline).timed_out() {
                break;
            }
        }
        if state.0 >= self.files {
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
fn a_drop_of_several_files_probes_them_together_once_each() {
    const FILES: usize = 4;
    let dir = crate::scratch_pool::scratch("parallel-probe");
    let paths: Vec<PathBuf> = (0..FILES)
        .map(|index| {
            let path = dir.join(format!("take-{index}.gathered"));
            std::fs::write(&path, b"take").unwrap();
            path
        })
        .collect();
    let decoder = Arc::new(GatheringDecoder {
        files: FILES,
        state: Mutex::new((0, 0)),
        started: Condvar::new(),
    });
    let _registered = register_external_decoder(
        "gathered",
        &["gathered".to_owned()],
        100,
        Arc::clone(&decoder) as Arc<dyn ExternalDecoder>,
    );
    let mut state = AppState::new(PluginContext::new(), dir).unwrap();
    state.new_project().unwrap();

    let arrived = state.import_dropped_paths(paths.clone()).unwrap();

    assert_eq!(arrived, paths, "every file of the drop imports");
    assert_eq!(state.store.media.len(), FILES);
    let (probes, together) = *decoder.state.lock();
    assert_eq!(probes, FILES, "each import adopts the probe taken ahead");
    assert_eq!(
        together, FILES,
        "every probe of the drop runs while the others are running"
    );
}
