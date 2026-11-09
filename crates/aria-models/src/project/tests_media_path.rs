//! `Project::find_media_by_path` and `Project::plays_file`: which pool entry
//! plays a file, and what one lookup costs.
//!
//! WHY: the class closed is "a pool lookup resolves every source on disk".
//! Import, relink and timeline interchange each look the pool up once per
//! file, under the engine lock, and resolving each source path costs a
//! round trip per path component on a network drive. The cases cover every
//! spelling of a pool file (canonical, `.`, `..`, a linked folder, a linked
//! file), a source spelled through a linked folder, a moved file, and a file
//! the pool does not hold; the cost case holds the lookup to a small share of
//! resolving the pool's sources once. Not caught: a pool source that is
//! itself a link with a file name other than its target's, which the lookup
//! does not resolve.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

/// A folder of media with every spelling the lookup has to accept.
struct Disk {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Disk {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("media")).unwrap();
        std::fs::write(root.join("media/clip.mp4"), b"clip").unwrap();
        std::fs::write(root.join("media/other.mp4"), b"other").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("media"), root.join("via")).unwrap();
            std::os::unix::fs::symlink(root.join("media/clip.mp4"), root.join("media/alias.mp4"))
                .unwrap();
        }
        Self { _dir: dir, root }
    }

    fn at(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }
}

fn source(path: PathBuf) -> MediaSource {
    MediaSource::new(path, 1920, 1080, R24, 48, true)
}

#[test]
fn every_spelling_of_a_pool_file_finds_its_entry_and_no_other_file_does() {
    let disk = Disk::new();
    let mut project = Project::new("test", R24);
    let clip = project.add_media(source(disk.at("media/clip.mp4")));
    let moved = project.add_media(source(disk.at("gone/moved.mp4")));
    #[cfg(unix)]
    let other = project.add_media(source(disk.at("via/other.mp4")));
    #[cfg(not(unix))]
    let other = project.add_media(source(disk.at("media/other.mp4")));

    let mut cases: Vec<(&str, Option<MediaId>, bool)> = vec![
        ("media/clip.mp4", Some(clip), true),
        ("media/./clip.mp4", Some(clip), true),
        ("media/../media/clip.mp4", Some(clip), true),
        ("media/other.mp4", Some(other), true),
        ("gone/moved.mp4", Some(moved), false),
        ("gone/./moved.mp4", Some(moved), false),
        ("gone/../gone/moved.mp4", None, false),
        ("media/absent.mp4", None, false),
        ("media", None, false),
    ];
    if cfg!(unix) {
        cases.extend([
            ("via/clip.mp4", Some(clip), true),
            ("media/alias.mp4", Some(clip), true),
            ("via/other.mp4", Some(other), true),
        ]);
    }
    for (path, entry, plays) in cases {
        let path = disk.at(path);
        assert_eq!(
            project.find_media_by_path(&path),
            entry,
            "{}",
            path.display()
        );
        assert_eq!(project.plays_file(&path), plays, "{}", path.display());
    }
}

/// The fastest of five runs of `work`.
fn fastest(mut work: impl FnMut()) -> Duration {
    (0..5)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .min()
        .unwrap()
}

#[test]
fn a_lookup_in_a_large_pool_resolves_the_asked_path_and_not_every_source() {
    const POOL: usize = 4000;
    let disk = Disk::new();
    let mut deep = disk.at("media");
    for level in 0..24 {
        deep.push(format!("level-{level}"));
    }
    std::fs::create_dir_all(&deep).unwrap();
    let mut project = Project::new("test", R24);
    for index in 0..POOL {
        project.add_media(source(deep.join(format!("take-{index}.mp4"))));
    }
    let asked = disk.at("media/./clip.mp4");
    let sources: Vec<&Path> = project.media_iter().map(|media| media.path()).collect();

    let resolve_each = fastest(|| {
        for source in &sources {
            std::hint::black_box(source.canonicalize().is_ok());
        }
    });
    let find = fastest(|| {
        assert_eq!(
            project.find_media_by_path(std::hint::black_box(&asked)),
            None
        );
    });
    let plays = fastest(|| {
        assert!(!project.plays_file(std::hint::black_box(&asked)));
    });
    for (name, took) in [("find_media_by_path", find), ("plays_file", plays)] {
        println!("{name}: {took:?} for {POOL} sources; resolving each source: {resolve_each:?}");
        assert!(
            took * 8 < resolve_each,
            "{name} took {took:?} over {POOL} sources, against {resolve_each:?} to resolve \
             each source once: the lookup resolves the sources on disk"
        );
    }
}
