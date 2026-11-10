//! The video and audio containers the platform decoder reads, in the order
//! the import dialogs offer them. Still formats are listed by the image
//! catalog ([`crate::image_extensions`]).

/// The containers every platform decoder reads.
pub const PORTABLE_CONTAINERS: &[&str] = &["mp4", "mov", "mkv", "webm", "wav", "mp3"];

/// The containers the GStreamer decoder reads besides
/// [`PORTABLE_CONTAINERS`], on Linux.
pub const GSTREAMER_CONTAINERS: &[&str] = &[
    "m4v", "avi", "mxf", "3gp", "m4a", "flac", "ogg", "opus", "mts", "m2ts", "aac",
];

/// The containers this build's platform decoder reads:
/// [`PORTABLE_CONTAINERS`], then on Linux [`GSTREAMER_CONTAINERS`].
pub fn container_extensions() -> Vec<&'static str> {
    let mut all = PORTABLE_CONTAINERS.to_vec();
    if cfg!(target_os = "linux") {
        all.extend_from_slice(GSTREAMER_CONTAINERS);
    }
    all
}

#[cfg(all(test, target_os = "linux"))]
#[path = "containers_tests.rs"]
mod tests;
