//! An import or a relink through the host probes its file with the engine
//! unlocked, and the apply adopts that probe instead of probing again.
//!
//! WHY: the class closed is "a command's file probe runs under the engine
//! lock". A probe opens the file, which takes seconds on a network drive, and
//! a render job's import held the engine that long, stalling every gesture
//! frame and every preview frame. The decoder here waits inside its probe
//! until the test releases it, so the engine's state during the probe is
//! observed, not timed. The cases cover both commands that probe, and an
//! import of a file the pool holds, which probes nothing. Not caught: other
//! commands that read files under the lock (Open, Load, Export).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use aria_commands::{Command, ProjectCommand};
use aria_core::{DecodeError, Rational, VideoDecoder};
use aria_decoder::{ExternalDecoder, MediaProbe, register_external_decoder};
use aria_engine::ApplyOutcome;
use aria_models::{MediaId, MediaSource, Project};
use parking_lot::Mutex;

use super::{BOUND, Origin, ProjectHost, fixture};

/// A decoder whose first probe waits for the test, reporting a 640x360 clip.
struct HeldDecoder {
    calls: AtomicUsize,
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl ExternalDecoder for HeldDecoder {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            let _ = self.entered.lock().send(());
            let _ = self.release.lock().recv_timeout(BOUND);
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

/// A file with an extension of its own, read by a [`HeldDecoder`].
struct Held {
    _dir: tempfile::TempDir,
    file: PathBuf,
    decoder: Arc<HeldDecoder>,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    _registered: aria_decoder::ExternalDecoderHandle,
}

impl Held {
    fn new(extension: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(format!("clip.{extension}"));
        std::fs::write(&file, b"held").unwrap();
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        let decoder = Arc::new(HeldDecoder {
            calls: AtomicUsize::new(0),
            entered: Mutex::new(entered_tx),
            release: Mutex::new(release_rx),
        });
        let registered = register_external_decoder(
            extension,
            &[extension.to_owned()],
            100,
            Arc::clone(&decoder) as Arc<dyn ExternalDecoder>,
        );
        Self {
            _dir: dir,
            file,
            decoder,
            entered,
            release,
            _registered: registered,
        }
    }

    /// Apply `command` on another thread and report whether the engine was
    /// free while the probe ran, then the outcome.
    fn apply(&self, host: &Arc<ProjectHost>, command: Command) -> (bool, ApplyOutcome) {
        let applying = {
            let host = Arc::clone(host);
            std::thread::spawn(move || host.apply(&Origin::Editor, command))
        };
        self.entered
            .recv_timeout(BOUND)
            .expect("the probe must start within the bound");
        let free = host.engine().try_lock().is_some();
        self.release.send(()).unwrap();
        let outcome = applying.join().unwrap().expect("the command applies");
        (free, outcome)
    }

    fn calls(&self) -> usize {
        self.decoder.calls.load(Ordering::SeqCst)
    }
}

fn width(host: &ProjectHost, media: MediaId) -> u32 {
    host.snapshot().media(media).expect("the pool entry").width
}

#[test]
fn an_import_probes_with_the_engine_free_once_and_a_reimport_probes_nothing() {
    let (fixture, _policy) = fixture();
    let host = &fixture.host;
    let held = Held::new("heldimport");
    let import = || {
        Command::Project(ProjectCommand::Import {
            path: held.file.clone(),
        })
    };

    let (free, outcome) = held.apply(host, import());
    let ApplyOutcome::Imported { media } = outcome else {
        panic!("an import reports the pool entry, got {outcome:?}");
    };
    assert!(free, "the engine must be free while the import probes");
    assert_eq!(held.calls(), 1, "the apply adopts the probe taken ahead");
    assert_eq!(width(host, media), 640);

    let again = host.apply(&Origin::Editor, import()).unwrap();
    assert!(matches!(again, ApplyOutcome::Imported { media: same } if same == media));
    assert_eq!(held.calls(), 1, "a file the pool holds is not probed");
}

#[test]
fn a_relink_probes_with_the_engine_free_once() {
    let (fixture, _policy) = fixture();
    let host = &fixture.host;
    let held = Held::new("heldrelink");
    let mut project = Project::new("probe", Rational::FPS_24);
    let media = project.add_media(MediaSource::new(
        "/missing/clip.mp4",
        320,
        240,
        Rational::FPS_24,
        48,
        false,
    ));
    host.engine().lock().reset_project(project);

    let command = Command::Project(ProjectCommand::RelinkMedia {
        media,
        path: held.file.clone(),
    });
    let (free, outcome) = held.apply(host, command);
    assert!(matches!(outcome, ApplyOutcome::Relinked { media: relinked } if relinked == media));
    assert!(free, "the engine must be free while the relink probes");
    assert_eq!(held.calls(), 1, "the apply adopts the probe taken ahead");
    assert_eq!(width(host, media), 640);
}

#[test]
fn a_probe_of_another_file_is_not_adopted() {
    let (fixture, _policy) = fixture();
    let held = Held::new("heldother");
    held.release.send(()).unwrap();
    let other = held.file.with_file_name("other.heldother");
    std::fs::write(&other, b"other").unwrap();
    let import = |path: &Path| {
        Command::Project(ProjectCommand::Import {
            path: path.to_path_buf(),
        })
    };

    let probed = aria_engine::Probed::ahead(&import(&held.file), |_| false);
    let outcome = fixture
        .host
        .engine()
        .lock()
        .apply_probed(import(&other), probed)
        .unwrap();
    let ApplyOutcome::Imported { media } = outcome else {
        panic!("an import reports the pool entry, got {outcome:?}");
    };
    assert_eq!(held.calls(), 2, "the imported file is probed itself");
    assert_eq!(
        fixture.host.snapshot().media(media).unwrap().path(),
        other.canonicalize().unwrap()
    );
}
