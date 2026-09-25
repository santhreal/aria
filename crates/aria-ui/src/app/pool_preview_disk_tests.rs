//! The poster-frame cache: what survives a relaunch, and what is refused.
//!
//! Declared from `pool_preview_disk.rs` with `#[path]`, so `use super::*` still
//! means that module.
//!
//! Every case here is filesystem-only: no engine, no GPU, no window. The
//! wiring — that a tile reads the cache before it decodes and writes it after —
//! is asserted in `pool_preview_tests.rs`, which has the store.

use super::*;
use crate::scratch_pool::scratch;

fn key(byte: u8) -> MediaContentKey {
    MediaContentKey::from_bytes([byte; 32])
}

fn picture(width: u32, height: u32) -> RgbaImage {
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    // A recognisable first pixel, so a round trip proves the bytes and not just
    // the dimensions.
    pixels[0] = 200;
    pixels[3] = 255;
    RgbaImage::new(width, height, pixels)
}

#[test]
fn a_poster_frame_round_trips_through_the_cache() {
    let root = scratch("poster-roundtrip");
    let path = poster_path(&root, key(1), 120, 68);
    assert!(read(&path).is_none(), "nothing is cached yet");

    let written = picture(120, 68);
    write(&path, &written).expect("write the poster frame");
    let read_back = read(&path).expect("the poster frame comes back");

    assert_eq!((read_back.width, read_back.height), (120, 68));
    assert_eq!(
        read_back.pixels, written.pixels,
        "the bytes are the picture"
    );
}

#[test]
fn one_source_at_one_size_is_one_file() {
    let root = scratch("poster-one-file");
    let path = poster_path(&root, key(2), 120, 68);
    write(&path, &picture(120, 68)).expect("write once");
    write(&path, &picture(120, 68)).expect("write again");

    let entries: Vec<String> = std::fs::read_dir(&root)
        .expect("the cache directory exists")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        entries,
        vec![
            path.file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        ],
        "a second write replaced the file instead of adding one, and left no part file"
    );
}

#[test]
fn a_different_source_or_size_is_a_different_file() {
    let root = scratch("poster-distinct");
    let a = poster_path(&root, key(3), 120, 68);
    let b = poster_path(&root, key(4), 120, 68);
    let big = poster_path(&root, key(3), 1280, 720);
    assert_ne!(a, b, "the content key is in the name");
    assert_ne!(a, big, "the size is in the name");

    write(&a, &picture(120, 68)).expect("write a");
    assert!(
        read(&b).is_none(),
        "another source is not this one's picture"
    );
    assert!(
        read(&big).is_none(),
        "another size is not this one's picture"
    );
}

/// The refusals, one per way a file can be wrong. Each must read as a miss: a
/// decode is cheap and a wrong picture is not.
#[test]
fn a_file_that_is_not_this_container_is_a_miss() {
    let root = scratch("poster-refusals");
    let good = poster_path(&root, key(5), 4, 4);
    write(&good, &picture(4, 4)).expect("write a good file");
    let bytes = std::fs::read(&good).expect("read it back raw");

    let cases: [(&str, Vec<u8>); 5] = [
        ("empty", Vec::new()),
        ("header only", bytes[..HEADER].to_vec()),
        ("truncated body", bytes[..bytes.len() - 4].to_vec()),
        ("trailing bytes", [bytes.as_slice(), &[0u8; 4]].concat()),
        ("another container's magic", {
            let mut wrong = bytes.to_vec();
            wrong[..MAGIC.len()].copy_from_slice(b"ARIATHM\x00");
            wrong
        }),
    ];
    for (what, content) in cases {
        let path = root.join(format!("wrong-{}.athumb", what.replace(' ', "-")));
        std::fs::write(&path, &content).expect("write the bad file");
        assert!(read(&path).is_none(), "{what} was served as a picture");
    }
}

/// A version bump must retire every file written under the old one, which is the
/// half of "version anything persisted" that is easy to skip: the magic carries
/// the version, so a file written by an older container reads as a miss above.
#[test]
fn the_container_version_is_the_last_byte_of_the_magic() {
    assert_eq!(&MAGIC[..7], b"ARIATHM");
    assert_eq!(MAGIC[7], 1, "the shipped container version");
    assert_eq!(HEADER, 16, "magic, width, height");
}

#[test]
fn a_malformed_picture_is_never_written() {
    let root = scratch("poster-malformed");
    let path = poster_path(&root, key(6), 2, 2);
    let short = RgbaImage::new(2, 2, vec![1, 2, 3]);
    assert!(write(&path, &short).is_err(), "a short buffer was written");
    assert!(!path.exists(), "the refusal left a file behind");

    let empty = RgbaImage::transparent(0, 8);
    assert!(
        write(&path, &empty).is_err(),
        "an empty picture was written"
    );
    assert!(!path.exists());
}
