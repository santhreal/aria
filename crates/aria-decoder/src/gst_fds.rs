//! The open-file limit of a decoding process.
//!
//! An open GStreamer decode pipeline holds about a dozen file descriptors: the
//! source file, the socket pairs behind its bus and queue polls, and event
//! fds. A desktop session starts a process with a soft limit of 1024, which a
//! few dozen layers on screen, each with its own decoder, reach; past it every
//! open fails with "Element failed to change its state". The first GStreamer
//! initialisation in a process raises the soft limit to the hard limit, at
//! most [`OPEN_FILES`].

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

/// The highest soft limit asked for. The hard limit caps it lower.
pub(crate) const OPEN_FILES: u64 = 65_536;

/// The limit to set in place of `limit`, or `None` when its soft limit is
/// already at or above the target (`None` in a field is unlimited).
fn raised(limit: Rlimit) -> Option<Rlimit> {
    let want = limit
        .maximum
        .map_or(OPEN_FILES, |hard| hard.min(OPEN_FILES));
    let soft = limit.current?;
    (soft < want).then_some(Rlimit {
        current: Some(want),
        maximum: limit.maximum,
    })
}

/// Raise the soft open-file limit of this process to the hard limit, at most
/// [`OPEN_FILES`]. A higher soft limit is kept.
pub(crate) fn raise_open_file_limit() {
    let limit = getrlimit(Resource::Nofile);
    let Some(new) = raised(limit) else {
        return;
    };
    if let Err(err) = setrlimit(Resource::Nofile, new) {
        tracing::warn!(
            %err,
            soft = limit.current,
            asked = new.current,
            "the open-file limit was not raised; with many decoders open, a later open can fail"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WHY: a decoding process kept at the 1024 soft limit of a desktop session
    /// fails every open once a few dozen decoders are open. Covers the target
    /// below, at, and above the cap, an unlimited hard or soft limit, and the
    /// choke point: every decode path initialises GStreamer through
    /// `ensure_init`, and after it the soft limit is raised. Does not catch a
    /// pipeline that leaks descriptors; that runs out at any limit.
    #[test]
    fn gstreamer_initialisation_raises_the_open_file_limit() {
        let limit = |current: Option<u64>, maximum: Option<u64>| Rlimit { current, maximum };
        let cases = [
            (
                limit(Some(1024), Some(1_048_576)),
                Some(limit(Some(OPEN_FILES), Some(1_048_576))),
            ),
            (
                limit(Some(1024), Some(4096)),
                Some(limit(Some(4096), Some(4096))),
            ),
            (limit(Some(1024), None), Some(limit(Some(OPEN_FILES), None))),
            (limit(Some(4096), Some(4096)), None),
            (limit(Some(OPEN_FILES), Some(1_048_576)), None),
            (limit(Some(1_048_576), Some(1_048_576)), None),
            (limit(None, None), None),
        ];
        for (given, expected) in cases {
            assert_eq!(raised(given), expected, "raising {given:?}");
        }

        crate::gst::ensure_init().expect("GStreamer initializes");
        let now = getrlimit(Resource::Nofile);
        let want = now.maximum.map_or(OPEN_FILES, |hard| hard.min(OPEN_FILES));
        assert!(
            now.current.is_none_or(|soft| soft >= want),
            "after GStreamer initialized the soft open-file limit is {:?}, below {want}",
            now.current
        );
    }
}
