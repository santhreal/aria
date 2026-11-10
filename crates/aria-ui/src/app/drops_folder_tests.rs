//! A folder dropped on the window, on Home, or named on the command line.
//!
//! WHY: every dropped path went to the media import, and a folder failed there
//! with a decoder's error, so a card or a folder of footage had to be opened and
//! its files selected by hand. A folder now contributes every media file under
//! it, sorted by path, in its place among the dropped paths; metadata files a
//! card carries beside each clip, hidden folders, and files no import dialog
//! offers are skipped; and a project file inside a folder is not opened. A
//! folder with no media file is named on the error line; dropped alone, it fails
//! the drop, so a drop on Home or a folder named on the command line stays on
//! Home. A package waiting for a file type in the folder loads before the
//! folder is read, so a plugin decoder's files are placed. Not caught: a folder
//! deeper than the relink walk, whose files are not listed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aria_plugin::{DecoderSpec, ExternalDecoder, MediaProbe, PluginContext, Registration};
use parking_lot::Mutex;

use super::AppState;
use crate::types::UiTrackKind;

fn texture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A copy of the still fixture at `dir/rel`, with its folders.
fn still(dir: &Path, rel: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
    std::fs::copy(texture(), &path).expect("copy the still");
    path
}

/// `text` at `dir/rel`.
fn file(dir: &Path, rel: &str, text: &str) -> PathBuf {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("make the folder");
    std::fs::write(&path, text).expect("write the file");
    path
}

fn fresh(dir: &Path) -> AppState {
    let mut state = AppState::new(PluginContext::new(), dir.to_path_buf()).expect("a state");
    state.new_project().expect("a project");
    state
}

/// The file each picture clip plays, relative to `dir`, in timeline order, and
/// whether the clips sit end to end from zero.
fn placed(state: &AppState, dir: &Path) -> (Vec<String>, bool) {
    let root = dir.file_name().expect("a named scratch folder");
    let mut clips: Vec<_> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.kind == UiTrackKind::Video)
        .flat_map(|lane| lane.clips.iter())
        .collect();
    clips.sort_by_key(|clip| clip.timeline_start_ticks);
    let chained = clips
        .iter()
        .scan(0, |end, clip| {
            let next = clip.timeline_start_ticks == *end;
            *end = clip.timeline_end_ticks;
            Some(next)
        })
        .all(|next| next);
    let files = clips
        .iter()
        .map(|clip| {
            let media = state
                .store
                .media
                .iter()
                .find(|item| Some(item.id) == clip.media_id)
                .expect("a placed clip plays a pool entry");
            // The pool holds the canonical path, so the scratch folder is
            // found by name rather than by its spelling.
            let path = Path::new(media.path.as_ref());
            let at = path
                .components()
                .position(|part| part.as_os_str() == root)
                .expect("a file under the scratch folder");
            path.components()
                .skip(at + 1)
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    (files, chained)
}

#[test]
fn a_dropped_folder_places_its_media_in_path_order_and_skips_the_rest() {
    let dir = crate::scratch_pool::scratch("folder-drop");
    let card = dir.join("card");
    still(&card, "b.png");
    still(&card, "a.png");
    still(&card, "day2/c.png");
    still(&card, ".cache/hidden.png");
    file(&card, "._a.png", "apple double metadata");
    file(&card, "notes.txt", "shot list");
    file(&card, "cut.aria", "not a project");
    let loose = still(&dir, "loose.png");
    let first = still(&dir, "first.png");

    let mut state = fresh(&dir);
    state
        .handle_dropped_paths(vec![first, card, loose])
        .expect("the drop lands");

    assert_eq!(state.last_error, None, "nothing in the drop failed");
    let (files, chained) = placed(&state, &dir);
    assert_eq!(
        files,
        [
            "first.png",
            "card/a.png",
            "card/b.png",
            "card/day2/c.png",
            "loose.png"
        ],
        "the folder's media, sorted, in its place among the dropped paths"
    );
    assert!(chained, "one chain from zero: {files:?}");
    assert_eq!(state.store.media.len(), 5, "nothing else entered the pool");
}

/// The error line for a folder with no media file.
fn no_media(name: &str) -> String {
    format!("No media in {name}. Drop a folder that holds video, audio, or still files.")
}

#[test]
fn a_folder_with_no_media_is_named_and_changes_nothing() {
    let dir = crate::scratch_pool::scratch("folder-drop-empty");
    let docs = dir.join("paperwork");
    file(&docs, "notes.txt", "shot list");
    file(&docs, "._clip.mp4", "apple double metadata");
    still(&docs, ".thumbs/cover.png");
    std::fs::create_dir_all(dir.join("bare")).expect("make the folder");
    let loose = still(&dir, "loose.png");

    for name in ["paperwork", "bare"] {
        // Alone, the drop fails with the folder's name.
        let mut state = fresh(&dir);
        match state.handle_dropped_paths(vec![dir.join(name)]) {
            Err(aria_engine::EngineError::Refused(copy)) => assert_eq!(copy, no_media(name)),
            other => panic!("{name}: the drop returned {other:?}"),
        }
        assert!(
            state.store.media.is_empty(),
            "{name}: the pool is unchanged"
        );

        // Beside a file, the file lands and the error line names the folder.
        let mut state = fresh(&dir);
        state
            .handle_dropped_paths(vec![dir.join(name), loose.clone()])
            .expect("the still lands");
        assert_eq!(state.last_error, Some(no_media(name)), "{name}");
        assert_eq!(placed(&state, &dir).0, ["loose.png"], "{name}");

        // Named on the command line, it leaves the session on Home.
        let mut state = AppState::new(PluginContext::new(), dir.clone()).expect("a state");
        state.open_cli_paths(vec![dir.join(name)]);
        assert_eq!(state.route, crate::types::UiRoute::Home, "{name}");
        assert_eq!(state.last_error, Some(no_media(name)), "{name}");
    }
}

#[test]
fn a_folder_named_on_home_opens_a_project_with_its_media() {
    let dir = crate::scratch_pool::scratch("folder-drop-home");
    let card = dir.join("card");
    still(&card, "one.png");
    still(&card, "two.png");

    let mut state = AppState::new(PluginContext::new(), dir.clone()).expect("a state");
    state.open_cli_paths(vec![card]);

    assert_eq!(state.last_error, None);
    assert_eq!(state.route, crate::types::UiRoute::Editor);
    let (files, chained) = placed(&state, &dir);
    assert_eq!(files, ["card/one.png", "card/two.png"]);
    assert!(chained);
}

/// A decoder that probes every file as a two-second picture and opens none.
struct Clip;

impl ExternalDecoder for Clip {
    fn probe(&self, _: &Path) -> Result<MediaProbe, aria_core::DecodeError> {
        Ok(MediaProbe {
            width: 64,
            height: 36,
            frame_rate: aria_core::Rational::new(24, 1),
            frame_count: 48,
            has_audio: false,
            is_image: false,
            video_codec: None,
            audio_codec: None,
        })
    }

    fn open_video(
        &self,
        _: &Path,
    ) -> Result<Box<dyn aria_core::VideoDecoder>, aria_core::DecodeError> {
        Err(aria_core::DecodeError::unsupported("probes only"))
    }
}

#[test]
fn a_folder_of_a_type_a_waiting_package_decodes_loads_it_first() {
    let dir = crate::scratch_pool::scratch("folder-drop-lazy");
    let card = dir.join("card");
    file(&card, "b.lazyclip", "clip");
    file(&card, "a.LAZYCLIP", "clip");
    file(&card, "notes.txt", "shot list");
    let mut state = fresh(&dir);
    let package = crate::app::activation_tests::lazy_package(
        "com.test.lazyclip",
        "Lazy clips",
        "[activation]\non = [\"file:lazyclip\"]\n",
    );
    let held: Arc<Mutex<Vec<Registration>>> = Arc::default();
    let loads = Arc::new(AtomicUsize::new(0));
    let (keep, count) = (Arc::clone(&held), Arc::clone(&loads));
    state
        .ctx
        .defer_package(&package, move |ctx| {
            count.fetch_add(1, Ordering::SeqCst);
            let spec = DecoderSpec {
                id: "com.test.lazyclip.clip".into(),
                name: "Lazy clip".into(),
                extensions: vec!["lazyclip".into()],
                priority: 0,
            };
            let registration = ctx
                .register_decoder(spec, Clip)
                .map_err(|err| err.to_string())?;
            keep.lock().push(registration);
            Ok(())
        })
        .expect("the package defers");

    state
        .handle_dropped_paths(vec![card])
        .expect("the drop lands");

    assert_eq!(loads.load(Ordering::SeqCst), 1, "the package loaded once");
    assert!(!state.ctx.is_waiting("com.test.lazyclip"));
    assert_eq!(state.last_error, None);
    let (files, chained) = placed(&state, &dir);
    assert_eq!(files, ["card/a.LAZYCLIP", "card/b.lazyclip"]);
    assert!(chained, "one chain from zero: {files:?}");
    assert_eq!(held.lock().len(), 1);
}
