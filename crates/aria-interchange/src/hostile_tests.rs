//! Every reader in [`FORMATS`] and [`SUBTITLE_FORMATS`], handed damaged text,
//! returns a value or an error and does not panic.
//!
//! Each reader starts from what its own writer produced, for a cut with every
//! part a cut holds or for cues with every kind of markup, so the damage lands
//! in text the reader otherwise reads whole. The text is then read cut short at
//! evenly spaced characters, and again after seeded edits: a character
//! replaced with one a format treats as syntax, a line dropped, repeated or
//! swapped with another, and a run of up to 20 nines inserted into a field. The
//! formats come from the registries, so a format added to either is read here
//! with no edit to this file. A failure names the format, the seed or the cut
//! point, and the text read.
//!
//! Not caught: a reader that loops forever (the test hangs rather than fails),
//! and a wrong value read from damaged text.

use std::panic::{AssertUnwindSafe, catch_unwind};

use aria_models::{MarkerColor, Rational};

use crate::model::{Event, LaneKind};
use crate::subtitles::Cue;
use crate::test_support::{R24, at, cut, event, lane, mark, written};
use crate::{FORMATS, SUBTITLE_FORMATS};

/// Characters a format reads as syntax, or that a reader may index past.
const HOSTILE: &[char] = &[
    '0', '9', ':', ';', '.', ',', '-', '+', '<', '>', '/', '&', '{', '}', '[', ']', '"', '\\', '#',
    '\n', '\r', ' ', '\t', 'e', 'x', '\0', 'é', '\u{feff}',
];

/// Seeded edits per format.
const SEEDS: u64 = 1_000;
/// Cut points per format, spread over the text.
const CUTS: usize = 400;

/// A cut with picture and sound lanes, a reversed shot at double speed
/// dissolving in, music counted in milliseconds, and markers with notes.
fn every_part() -> crate::model::Cut {
    let interview = Event {
        sound: true,
        ..event("Interview", 0, 48, "/media/A001.mov", at(24, R24))
    };
    let broll = Event {
        speed: Rational::new(2, 1),
        reversed: true,
        dissolve: Some(12),
        ..event("Broll", 48, 24, "/media/B 002.mov", at(0, R24))
    };
    let logo = event("Logo", 12, 24, "/media/logo.mov", at(0, R24));
    let music = Event {
        sound: true,
        ..event(
            "music",
            0,
            72,
            "/media/music.wav",
            at(0, Rational::new(1000, 1)),
        )
    };
    cut(
        "Promo",
        R24,
        vec![
            lane(LaneKind::Video, "V1", vec![interview, broll]),
            lane(LaneKind::Video, "V2", vec![logo]),
            lane(LaneKind::Audio, "A1", vec![music]),
        ],
        vec![
            mark(24, "Check", "Focus on the left", Some(MarkerColor::Green)),
            mark(60, "Out", "", None),
        ],
    )
}

/// Cues with styling tags, an override block, entities, a blank-line gap,
/// and two lines of text.
fn every_cue() -> Vec<Cue> {
    let cue = |start_ms, end_ms, text: &str| Cue {
        start_ms,
        end_ms,
        text: text.to_owned(),
    };
    vec![
        cue(0, 1_500, "<i>Hello</i> &amp; welcome"),
        cue(1_500, 3_250, "{\\an8}Up top\nand a second line"),
        cue(3_600_000, 3_601_001, "a < b > c & d"),
    ]
}

/// SplitMix64: a failing case is named by its seed.
struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// `text` after one to three seeded edits.
fn damaged(text: &str, seed: u64) -> String {
    let mut rng = Seeded(seed);
    let mut chars: Vec<char> = text.chars().collect();
    for _ in 0..=rng.below(3) {
        match rng.below(5) {
            0 if !chars.is_empty() => {
                let at = rng.below(chars.len());
                chars[at] = HOSTILE[rng.below(HOSTILE.len())];
            }
            1 => {
                // Half the runs land on a number, where a field is read.
                let mut at = rng.below(chars.len() + 1);
                if rng.below(2) == 0 {
                    at += chars[at..]
                        .iter()
                        .position(|c| c.is_ascii_digit())
                        .unwrap_or(0);
                }
                let nines = 1 + rng.below(20);
                chars.splice(at..at, std::iter::repeat_n('9', nines));
            }
            edit => {
                let text: String = chars.iter().collect();
                let mut lines: Vec<&str> = text.split('\n').collect();
                let at = rng.below(lines.len());
                match edit {
                    2 => {
                        lines.remove(at);
                    }
                    3 => lines.insert(at, lines[at]),
                    _ => {
                        let other = rng.below(lines.len());
                        lines.swap(at, other);
                    }
                }
                chars = lines.join("\n").chars().collect();
            }
        }
    }
    chars.into_iter().collect()
}

/// `text` cut short at `CUTS` character boundaries spread over it.
fn cut_points(text: &str) -> impl Iterator<Item = &str> {
    let bounds: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    let step = (bounds.len() / CUTS).max(1);
    bounds.into_iter().step_by(step).map(move |at| &text[..at])
}

/// Read every damaged and cut-short form of `whole` with `read`, and the
/// cases that panicked, each named by `name` and how the text was damaged.
fn panics<T>(name: &str, whole: &str, read: impl Fn(&str) -> T) -> Vec<String> {
    let mut found = Vec::new();
    let mut try_read = |how: String, text: &str| {
        if catch_unwind(AssertUnwindSafe(|| read(text))).is_err() {
            found.push(format!("{name} {how}: {text:?}"));
        }
    };
    for text in cut_points(whole) {
        try_read(format!("cut at byte {}", text.len()), text);
    }
    for seed in 0..SEEDS {
        try_read(format!("seed {seed}"), &damaged(whole, seed));
    }
    found
}

fn assert_none(found: Vec<String>) {
    assert!(
        found.is_empty(),
        "{} damaged texts panicked a reader:\n  {}",
        found.len(),
        found
            .iter()
            .take(8)
            .map(|case| case.chars().take(600).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn no_timeline_reader_panics_on_damaged_text() {
    let cut = every_part();
    let mut found = Vec::new();
    for format in &FORMATS {
        let (whole, _) = written(format.write, &cut);
        for rate in [R24, Rational::FPS_29_97] {
            assert!(
                (format.read)(&whole, rate, "Hostile").is_ok(),
                "{} does not read what its own writer wrote",
                format.id
            );
            found.extend(panics(format.id, &whole, |text| {
                (format.read)(text, rate, "Hostile")
            }));
        }
    }
    assert_none(found);
}

#[test]
fn no_subtitle_reader_panics_on_damaged_text() {
    let cues = every_cue();
    let mut found = Vec::new();
    for format in &SUBTITLE_FORMATS {
        let mut bytes = Vec::new();
        (format.write)(&cues, &mut bytes).expect("write cues to memory");
        let whole = String::from_utf8(bytes).expect("a subtitle writer writes UTF-8");
        assert!(
            (format.read)(&whole).is_ok(),
            "{} does not read what its own writer wrote",
            format.id
        );
        found.extend(panics(format.id, &whole, format.read));
    }
    assert_none(found);
}
