//! A renderer that adopts another's media sources decodes each media id from
//! the file the other decodes it from.
//!
//! WHY: filmstrip cells render on a renderer of their own, which adopts the
//! session renderer's media sources before every cell. A change of the file
//! behind a media id that left the generation where it was would keep the
//! cell renderer's decoder open on the old file, so cells would show the
//! original after the session switched to a proxy, relinked the media, or
//! opened another project. Closes that class for every writer of the
//! media-source state in `api.rs`: the census fails on a writer with no row
//! in [`changes`]. The other direction is pinned too: sources naming the same
//! files keep the adopter's decoder, so a cell does not reopen it per frame.
//!
//! Not covered: the frames an adopter decodes from a proxy, which belong to
//! the decoder's own tests, and writers outside `api.rs`.

use std::path::PathBuf;

use aria_models::{MediaId, MediaSource, Project, Rational, RationalTime, TimeRange, TrackKind};

use super::Renderer;

const FPS: Rational = Rational::FPS_30;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// A file on disk that stands in as a proxy.
fn proxy() -> PathBuf {
    fixture("fixture-640x360-24fps-3s.mp4")
}

/// One clip of the 30 fps fixture.
fn project() -> (Project, MediaId) {
    let mut project = Project::new("media sources", FPS);
    let media = project.add_media(MediaSource::new(
        fixture("fixture-640x360-30fps-3s.mp4"),
        640,
        360,
        FPS,
        90,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 90, FPS),
            RationalTime::new(0, FPS),
        )
        .expect("a clip of the fixture");
    (project, media)
}

/// A renderer that adopted `session`'s sources and decoded a frame from them.
fn adopter(session: &Renderer, project: &Project) -> Renderer {
    let mut adopter = Renderer::with_gpu(session.gpu().share());
    adopter.adopt_media_sources(&session.media_sources());
    adopter
        .render_frame_fit(project, RationalTime::new(10, FPS), 32, 18)
        .unwrap_or_else(|error| panic!("the adopter's frame: {error}"));
    assert_eq!(adopter.decoders.len(), 1, "the adopter opened its decoder");
    adopter
}

type Step = fn(&mut Renderer, MediaId);

/// Every writer of the media-source state: its name, what the session holds
/// before the write, and the write.
fn changes() -> [(&'static str, Step, Step); 7] {
    [
        ("set_proxy", |_, _| {}, |r, m| r.set_proxy(m, proxy())),
        (
            "clear_proxy",
            |r, m| r.set_proxy(m, proxy()),
            |r, m| r.clear_proxy(m),
        ),
        (
            "clear_proxies",
            |r, m| r.set_proxy(m, proxy()),
            |r, _| r.clear_proxies(),
        ),
        (
            "reset_media_sources",
            |_, _| {},
            |r, _| r.reset_media_sources(),
        ),
        (
            "invalidate_media_source",
            |_, _| {},
            |r, m| r.invalidate_media_source(m),
        ),
        (
            "set_use_proxies",
            |r, m| r.set_proxy(m, proxy()),
            |r, _| r.set_use_proxies(false),
        ),
        (
            "set_use_proxies",
            |r, m| {
                r.set_proxy(m, proxy());
                r.set_use_proxies(false);
            },
            |r, _| r.set_use_proxies(true),
        ),
    ]
}

#[test]
fn every_change_of_the_file_behind_a_media_id_reaches_an_adopter() {
    let (project, media) = project();
    for (name, before, change) in changes() {
        let Ok(mut session) = Renderer::new_headless() else {
            return;
        };
        before(&mut session, media);
        let mut cells = adopter(&session, &project);
        change(&mut session, media);
        cells.adopt_media_sources(&session.media_sources());
        assert!(
            cells.decoders.is_empty(),
            "after {name}, the adopter kept a decoder open on the file it read before"
        );
        assert_eq!(
            *cells.proxies, *session.proxies,
            "after {name}, the adopter decodes from other proxies than the session"
        );
        assert_eq!(
            cells.use_proxies, session.use_proxies,
            "after {name}, the adopter uses proxies where the session does not, or the reverse"
        );
    }
}

#[test]
fn sources_naming_the_same_files_keep_the_adopters_decoder() {
    let (project, media) = project();
    let same: [(&str, Step); 4] = [
        ("the same sources again", |_, _| {}),
        ("clear_proxy of a media with no proxy", |r, m| {
            r.clear_proxy(m)
        }),
        ("clear_proxies with none registered", |r, _| {
            r.clear_proxies()
        }),
        ("set_use_proxies to the state in force", |r, _| {
            r.set_use_proxies(true)
        }),
    ];
    for (name, call) in same {
        let Ok(mut session) = Renderer::new_headless() else {
            return;
        };
        let mut cells = adopter(&session, &project);
        call(&mut session, media);
        session
            .render_frame_fit(&project, RationalTime::new(40, FPS), 32, 18)
            .unwrap_or_else(|error| panic!("the session's frame: {error}"));
        cells.adopt_media_sources(&session.media_sources());
        assert_eq!(
            cells.decoders.len(),
            1,
            "after {name} and a session frame, the adopter dropped its decoder"
        );
    }
}

/// Text that writes the proxies, whether they are used, or the still bitmaps.
const WRITES: &[&str] = &[
    "make_mut(&mut self.proxies)",
    "self.proxies =",
    "take(&mut self.proxies)",
    "self.use_proxies =",
    "self.stills.clear()",
    "self.stills.remove(",
];

/// The functions of `api.rs` whose bodies contain one of [`WRITES`].
fn writers() -> Vec<&'static str> {
    let mut writers: Vec<&str> = Vec::new();
    let mut current = None;
    for line in include_str!("api.rs").lines() {
        let line = line.trim_start();
        // `pub`, `pub(crate)`, `pub(super)` and `const` before `fn`.
        let head = if line.starts_with("pub") {
            line.split_once(' ').map_or(line, |(_, rest)| rest)
        } else {
            line
        };
        let head = head.strip_prefix("const ").unwrap_or(head);
        if let Some(rest) = head.strip_prefix("fn ") {
            current = rest.split(['(', '<']).next();
        }
        if WRITES.iter().any(|write| line.contains(write))
            && let Some(name) = current
            && !writers.contains(&name)
        {
            writers.push(name);
        }
    }
    writers
}

#[test]
fn every_writer_of_the_media_sources_has_a_row() {
    let rows: Vec<&str> = changes().iter().map(|(name, ..)| *name).collect();
    let writers = writers();
    let unlisted: Vec<&&str> = writers
        .iter()
        .filter(|name| !rows.contains(name) && **name != "adopt_media_sources")
        .collect();
    assert!(
        unlisted.is_empty(),
        "{unlisted:?} change the files media ids decode from with no row in `changes`: \
         add one, and move the media generation in the writer"
    );
    let stale: Vec<&&str> = rows.iter().filter(|row| !writers.contains(row)).collect();
    assert!(
        stale.is_empty(),
        "{stale:?} no longer write the media sources: drop their rows"
    );
}
