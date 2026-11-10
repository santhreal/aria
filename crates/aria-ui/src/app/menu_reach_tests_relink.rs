//! Relink All, driven from its File menu row.
//!
//! WHY: the class closed is "Relink All reads the folder on the window's
//! thread". The walk and the probe of every same-named file ran inside the
//! pick's continuation, so a folder of card copies froze the window until the
//! last probe returned, and the bind then probed the chosen file again. The
//! cases cover the search running as a job, a plan arriving after another
//! project opened, and the probe count of a bound file. Not caught: a walk
//! that is cancelled midway, which reads the whole folder before it stops.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_core::{DecodeError, Rational, VideoDecoder};
use aria_decoder::{ExternalDecoder, MediaProbe, register_external_decoder};

use super::*;

/// Relink All searches the folder as a job: the row returns with the entry
/// still missing and the search on the board, and the bind lands when the job
/// hands its plan back. A search on the window's thread froze the window for
/// as long as the folder took to read and every same-named file took to probe.
#[test]
fn relink_all_searches_the_chosen_folder_off_the_window_thread() {
    let (state, _) = Answered::RelinkAll.state();
    assert_eq!(
        state.store.media.iter().filter(|item| item.missing).count(),
        1,
        "the dressing for a relink needs one missing file"
    );
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-relink-all");

    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.picks_asked(), 1);
        assert!(
            state.store.media.iter().any(|item| item.missing),
            "the folder was searched on the window's thread"
        );
        let searching: Vec<_> = state.job_view.iter().map(|job| job.label.clone()).collect();
        assert!(
            searching
                .iter()
                .any(|label| label.starts_with("Searching ")
                    && label.ends_with(" for 1 missing file")),
            "no search on the board: {searching:?}"
        );
    });

    run_until(&entity, &cx, "the relink", |state| {
        state.store.media.iter().all(|item| !item.missing)
    });
    run_until(&entity, &cx, "the search's row finishing", |state| {
        state.refresh_jobs();
        state.running_job_count() == 0
    });
    cx.update(|app| {
        let state = entity.read(app);
        assert_eq!(state.status_message.as_deref(), Some("Relinked 1 file"));
        let done: Vec<_> = state.job_view.iter().map(|job| job.label.clone()).collect();
        assert!(
            done.iter().any(|label| label.starts_with("Searched ")),
            "the finished search reads as still searching: {done:?}"
        );
    });
}
/// A plan names entries of the project that was open when the folder was
/// chosen. A project opened while the search ran gets none of it: no bind,
/// no note, and no count on the status line.
#[test]
fn relink_all_applies_nothing_to_a_project_opened_during_the_search() {
    let (state, _) = Answered::RelinkAll.state();
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);

    run_row(&entity, &cx, AppMenu::File, "file-relink-all");
    // The search finishes and hands back its plan before the window reads
    // it: the window's tasks do not run until the clock is moved on.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while cx.update(|app| {
        entity.update(app, |state, _| {
            state.refresh_jobs();
            state.running_job_count() > 0
        })
    }) {
        assert!(
            std::time::Instant::now() < deadline,
            "the search did not finish within 20 s"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    cx.update(|app| {
        entity.update(app, |state, _| {
            state.new_project().expect("open another project");
            state.status_message = None;
        })
    });
    cx.executor().advance_clock(crate::handoff::CHECK);
    cx.run_until_parked();

    cx.update(|app| {
        let state = entity.read(app);
        assert!(
            state.relink_notes.is_empty(),
            "the old project's plan was applied: {:?}",
            state.relink_notes
        );
        assert_eq!(state.status_message, None);
    });
}

/// A decoder that counts its probes, reporting a 640x360 clip.
#[derive(Default)]
struct CountingDecoder {
    probes: AtomicUsize,
}

impl ExternalDecoder for CountingDecoder {
    fn probe(&self, _path: &Path) -> Result<MediaProbe, DecodeError> {
        self.probes.fetch_add(1, Ordering::SeqCst);
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

/// The search probes a same-named file to choose it, and the bind adopts that
/// probe: the bound file is opened once.
#[test]
fn relink_all_probes_the_file_it_binds_once() {
    let decoder = Arc::new(CountingDecoder::default());
    let _registered = register_external_decoder(
        "relink-counted",
        &["relinkcounted".to_owned()],
        100,
        Arc::clone(&decoder) as Arc<dyn ExternalDecoder>,
    );
    let source = own_scratch("relink-counted").join("clip.relinkcounted");
    let moved = own_scratch("relink-counted-moved");
    std::fs::write(&source, b"counted").expect("stage a file to lose");
    let (mut state, _) = with_clip();
    state.import_media(source.clone()).expect("import it");
    std::fs::rename(&source, moved.join("clip.relinkcounted"))
        .expect("move it out from under the project");
    crate::projection::invalidate_media_statuses();
    state.rebuild_store();
    state.supply_pick(Answer::file(&moved));
    let cx = gpui::TestAppContext::single();
    let entity = entity_of(state, &cx);
    decoder.probes.store(0, Ordering::SeqCst);

    run_row(&entity, &cx, AppMenu::File, "file-relink-all");
    run_until(&entity, &cx, "the relink", |state| {
        state.store.media.iter().all(|item| !item.missing)
    });

    assert_eq!(
        decoder.probes.load(Ordering::SeqCst),
        1,
        "the bound file was probed by the search and again by the bind"
    );
}
