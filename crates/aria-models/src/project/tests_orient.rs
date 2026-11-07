//! `Project::orient_media`: a save from before the probe applied the
//! container's display rotation states a phone video's sideways size, and the
//! upright picture then draws squashed into it.
//!
//! WHY: the class closed is "a persisted size the decoder no longer agrees
//! with is served". The cases cover each outcome the repair has (transposed,
//! already right, square, unrelated, missing), the entries it must not probe
//! (already upright, stills, sound files), one probe per file however many
//! subclips share it, and a pool large enough to spread across every probe
//! thread. Not caught: a probe that reads the wrong upright size, which the
//! decoder's rotation tests own.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;

/// A probe that answers from `sizes` and counts the calls per path.
struct FakeProbe {
    sizes: HashMap<PathBuf, (u32, u32)>,
    calls: Mutex<HashMap<PathBuf, usize>>,
}

impl FakeProbe {
    fn new(sizes: &[(&str, (u32, u32))]) -> Self {
        Self {
            sizes: sizes
                .iter()
                .map(|&(path, size)| (PathBuf::from(path), size))
                .collect(),
            calls: Mutex::new(HashMap::new()),
        }
    }

    fn probe(&self, path: &Path) -> Option<(u32, u32)> {
        *self
            .calls
            .lock()
            .expect("the call count")
            .entry(path.to_path_buf())
            .or_default() += 1;
        self.sizes.get(path).copied()
    }

    fn calls(&self) -> HashMap<PathBuf, usize> {
        self.calls.lock().expect("the call count").clone()
    }
}

/// A video at `path` stored at `width`×`height` by a save that predates the
/// flag.
fn stale(path: &str, width: u32, height: u32) -> MediaSource {
    let mut media = MediaSource::new(path, width, height, R24, 48, false);
    media.upright = false;
    media
}

fn size_and_flag(project: &Project, id: MediaId) -> ((u32, u32), bool) {
    let media = project.media(id).expect("the entry");
    ((media.width, media.height), media.upright)
}

#[test]
fn each_stale_entry_takes_the_outcome_its_probe_states() {
    let probe = FakeProbe::new(&[
        ("/v/phone.mp4", (1080, 1920)),
        ("/v/level.mp4", (1920, 1080)),
        ("/v/square.mp4", (720, 720)),
        ("/v/other.mp4", (640, 480)),
    ]);
    let mut project = Project::new("orient", R24);
    let phone = project.add_media(stale("/v/phone.mp4", 1920, 1080));
    let level = project.add_media(stale("/v/level.mp4", 1920, 1080));
    let square = project.add_media(stale("/v/square.mp4", 720, 720));
    let other = project.add_media(stale("/v/other.mp4", 1920, 1080));
    let missing = project.add_media(stale("/v/missing.mp4", 1920, 1080));

    let changed = project.orient_media(|path| probe.probe(path));

    assert_eq!(changed, 1, "only the transposed entry changes size");
    let cases = [
        ("transposed", phone, ((1080, 1920), true)),
        ("already right", level, ((1920, 1080), true)),
        ("square", square, ((720, 720), true)),
        ("unrelated size", other, ((1920, 1080), false)),
        ("missing file", missing, ((1920, 1080), false)),
    ];
    for (what, id, expected) in cases {
        assert_eq!(size_and_flag(&project, id), expected, "{what}");
    }
}

#[test]
fn entries_that_need_no_probe_are_not_probed() {
    let probe = FakeProbe::new(&[
        ("/v/upright.mp4", (1080, 1920)),
        ("/v/still.png", (300, 400)),
        ("/v/sound.wav", (0, 0)),
    ]);
    let mut project = Project::new("orient", R24);
    let upright = project.add_media(MediaSource::new(
        "/v/upright.mp4",
        1920,
        1080,
        R24,
        48,
        false,
    ));
    let mut still = MediaSource::image("/v/still.png", 400, 300);
    still.upright = false;
    let still = project.add_media(still);
    let mut sound = MediaSource::new("/v/sound.wav", 0, 0, R24, 48, true);
    sound.upright = false;
    let sound = project.add_media(sound);

    assert_eq!(project.orient_media(|path| probe.probe(path)), 0);
    assert!(probe.calls().is_empty(), "probed {:?}", probe.calls());
    assert_eq!(size_and_flag(&project, upright), ((1920, 1080), true));
    assert_eq!(size_and_flag(&project, still), ((400, 300), false));
    assert_eq!(size_and_flag(&project, sound), ((0, 0), false));
}

#[test]
fn a_file_shared_by_subclips_is_probed_once_and_corrects_each() {
    let probe = FakeProbe::new(&[("/v/phone.mp4", (1080, 1920))]);
    let mut project = Project::new("orient", R24);
    let whole = stale("/v/phone.mp4", 1920, 1080);
    let parts: Vec<MediaSource> = (0..3)
        .map(|n| {
            MediaSource::subclip(&whole, format!("part {n}"), tr(n * 8, 8)).expect("a subclip")
        })
        .collect();
    let mut ids = vec![project.add_media(whole)];
    ids.extend(parts.into_iter().map(|part| project.add_media(part)));

    assert_eq!(project.orient_media(|path| probe.probe(path)), 4);
    assert_eq!(
        probe.calls(),
        HashMap::from([(PathBuf::from("/v/phone.mp4"), 1)])
    );
    for id in ids {
        assert_eq!(size_and_flag(&project, id), ((1080, 1920), true));
    }
}

/// More files than this host has cores, so every probe thread takes a share,
/// and a second pass over the repaired pool probes nothing.
#[test]
fn a_pool_wider_than_the_probe_threads_is_repaired_once() {
    let count = std::thread::available_parallelism().map_or(1, usize::from) * 3 + 1;
    let names: Vec<String> = (0..count).map(|n| format!("/v/clip-{n}.mp4")).collect();
    let probe = FakeProbe::new(
        &names
            .iter()
            .enumerate()
            .map(|(n, name)| {
                let short = 100 + n as u32;
                (name.as_str(), (short, short * 2))
            })
            .collect::<Vec<_>>(),
    );
    let mut project = Project::new("orient", R24);
    let ids: Vec<MediaId> = names
        .iter()
        .enumerate()
        .map(|(n, name)| {
            let short = 100 + n as u32;
            let (width, height) = if n % 2 == 0 {
                (short * 2, short)
            } else {
                (short, short * 2)
            };
            project.add_media(stale(name, width, height))
        })
        .collect();

    assert_eq!(
        project.orient_media(|path| probe.probe(path)),
        count.div_ceil(2)
    );
    for (n, id) in ids.into_iter().enumerate() {
        let short = 100 + n as u32;
        assert_eq!(
            size_and_flag(&project, id),
            ((short, short * 2), true),
            "entry {n}"
        );
    }
    let calls = probe.calls();
    assert_eq!(calls.len(), count);
    assert!(calls.values().all(|&n| n == 1), "{calls:?}");

    assert_eq!(project.orient_media(|path| probe.probe(path)), 0);
    assert_eq!(probe.calls(), calls, "a repaired pool was probed again");
}
