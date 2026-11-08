//! `aria inspect` prints each media pool entry as its kind has it.
//!
//! WHY: a sound printed as `0x0 1000/1 0:00:02.000`, the size and tick rate
//! its probe fills in for no picture, and a still printed the rate and length
//! a placement starts at. `inspect::tests` holds the line of each kind; this
//! holds that the binary prints those lines for a project `aria new` built,
//! and that `aria new` ends with a `wrote` line in the CLI's separators: it
//! printed an em dash after the path. Both commands print the rate in frames
//! a second, where they printed `24/1`, and `inspect` and `aria frame` count
//! frames, where they printed ticks. Does not cover a video, whose line
//! `inspect::tests` holds.

mod support;

use std::ffi::OsStr;
use std::path::Path;

use support::{aria, scratch};

#[test]
fn inspect_prints_a_sound_and_a_still_as_their_kinds() {
    let dir = scratch("inspect-pool");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .canonicalize()
        .expect("the fixtures directory");
    let sound = fixtures.join("fixture-sine-48k-2s.wav");
    let still = fixtures.join("texture.png");
    let project = dir.join("pool.aria");
    let new = aria([
        OsStr::new("new"),
        project.as_os_str(),
        sound.as_os_str(),
        still.as_os_str(),
    ]);
    assert!(
        new.status.success(),
        "aria new: {}",
        String::from_utf8_lossy(&new.stderr)
    );
    let made = String::from_utf8_lossy(&new.stdout);
    let wrote = made.lines().last().unwrap_or_default();
    let rest = wrote.strip_prefix(&format!("wrote {}: ", project.display()));
    let (length, rate) = rest
        .and_then(|rest| rest.strip_suffix(", 2 files"))
        .and_then(|rest| rest.split_once(" at "))
        .unwrap_or_else(|| panic!("aria new ended with {wrote:?}"));
    assert!(
        length.split(':').count() == 3 && rate == "24 fps" && !wrote.contains('—'),
        "aria new ended with {wrote:?}"
    );
    let shown = aria([OsStr::new("inspect"), project.as_os_str()]);
    assert!(
        shown.status.success(),
        "aria inspect: {}",
        String::from_utf8_lossy(&shown.stderr)
    );
    let stdout = String::from_utf8_lossy(&shown.stdout);
    assert!(
        stdout.lines().any(|line| line == "rate      24 fps"),
        "no rate line in\n{stdout}"
    );
    let sound_line = format!("  audio 0:00:02.000 {}", sound.display());
    assert!(
        stdout.lines().any(|line| line == sound_line),
        "{sound_line:?} is not a line of\n{stdout}"
    );
    let still_tail = format!(" still {}", still.display());
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("  ") && line.ends_with(&still_tail)),
        "no line ends with {still_tail:?} in\n{stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("duration  ") && line.ends_with(" frames)")),
        "no duration line counting frames in\n{stdout}"
    );
    let png = dir.join("first.png");
    let rendered = aria([OsStr::new("frame"), project.as_os_str(), png.as_os_str()]);
    let rendered = String::from_utf8_lossy(&rendered.stdout);
    let tail = format!(" at 0:00:00.000 (frame 0) to {}", png.display());
    assert!(
        rendered.lines().any(|line| line.ends_with(&tail)),
        "aria frame printed {rendered:?}"
    );
}
