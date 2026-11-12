//! The MP4 rewrite that trims AAC priming, run on files built box by box.

use std::io::Cursor;

use super::*;

const SAMPLE_RATE: u64 = 48_000;
/// Three seconds and 100 samples of sound.
const PUSHED: u64 = 3 * 48_000 + 100;
const PRIMING: u64 = 1024;
const MOVIE_SCALE: u32 = 1000;
/// The sound track's own timescale, not the sample rate.
const SOUND_SCALE: u32 = 44_100;
/// The video track's duration in the movie timescale, longer than the sound.
const VIDEO: u64 = 3050;

/// The fields the rewrite sets, in the units the file states them in.
#[derive(Debug, Clone, Copy)]
struct Times {
    /// `mvhd` duration, movie timescale.
    movie: u64,
    /// Sound `tkhd` duration and `elst` segment duration, movie timescale.
    sound: u64,
    /// Sound `elst` media time, sound timescale.
    skip: u64,
}

/// As the muxer closes the file: the sound runs on by the priming and the
/// last packet's padding, and the movie by as much.
const MUXED: Times = Times {
    movie: 3120,
    sound: 3045,
    skip: 0,
};

/// After the rewrite: the sound lasts `PUSHED` samples (3002.08 ms, stated
/// as 3002) and starts `PRIMING` samples in (940.8 ticks at 44.1 kHz, stated
/// as 941), and the movie lasts as long as its longest track, the video.
const TRIMMED: Times = Times {
    movie: VIDEO,
    sound: 3002,
    skip: 941,
};

/// How the boxes are written. ISO/IEC 14496-12 lets a box state its size in
/// 64 bits (`size` 1, then `largesize`) and lets the last box run to the end
/// of the file (`size` 0); version 1 of `mvhd`, `tkhd`, `mdhd` and `elst`
/// states times in 64 bits.
#[derive(Debug, Clone, Copy)]
struct Layout {
    version: u8,
    large_mdat: bool,
    large_trak: bool,
    open_moov: bool,
}

fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + body.len()).unwrap();
    [&size.to_be_bytes()[..], kind, body].concat()
}

fn large(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let size = 16 + body.len() as u64;
    [&1u32.to_be_bytes()[..], kind, &size.to_be_bytes(), body].concat()
}

/// A box of a container that `layout` may write with a 64-bit size.
fn sized(large_size: bool, kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    if large_size {
        large(kind, body)
    } else {
        boxed(kind, body)
    }
}

/// A full box: version, zero flags, then `fields`.
fn full(kind: &[u8; 4], version: u8, fields: &[&[u8]]) -> Vec<u8> {
    boxed(kind, &[&[version, 0, 0, 0][..], &fields.concat()].concat())
}

/// A time field, 32 bits in version 0 and 64 bits in version 1.
fn time(version: u8, value: u64) -> Vec<u8> {
    match version {
        0 => u32::try_from(value).unwrap().to_be_bytes().to_vec(),
        _ => value.to_be_bytes().to_vec(),
    }
}

fn trak(layout: Layout, id: u32, handler: &[u8; 4], scale: u32, length: u64, skip: u64) -> Vec<u8> {
    let v = layout.version;
    let zero = time(v, 0);
    let tkhd = full(
        b"tkhd",
        v,
        &[
            &zero,
            &zero,
            &id.to_be_bytes(),
            &[0; 4],
            &time(v, length),
            &[0; 60],
        ],
    );
    let elst = full(
        b"elst",
        v,
        &[
            &1u32.to_be_bytes(),
            &time(v, length),
            &time(v, skip),
            &0x0001_0000u32.to_be_bytes(),
        ],
    );
    let mdhd = full(
        b"mdhd",
        v,
        &[&zero, &zero, &scale.to_be_bytes(), &time(v, 0), &[0; 4]],
    );
    let hdlr = full(b"hdlr", 0, &[&[0; 4], handler, &[0; 12], b"\0"]);
    let body = [
        tkhd,
        boxed(b"edts", &elst),
        boxed(b"mdia", &[mdhd, hdlr].concat()),
    ]
    .concat();
    sized(layout.large_trak, b"trak", &body)
}

/// An MP4 file with a video and a sound track, `mdat` before `moov` as the
/// muxer writes it.
fn mp4(layout: Layout, times: Times, sound: bool) -> Vec<u8> {
    let v = layout.version;
    let zero = time(v, 0);
    let mvhd = full(
        b"mvhd",
        v,
        &[
            &zero,
            &zero,
            &MOVIE_SCALE.to_be_bytes(),
            &time(v, times.movie),
            &[0; 80],
        ],
    );
    let mut moov_body = [mvhd, trak(layout, 1, b"vide", 30_000, VIDEO, 0)].concat();
    if sound {
        moov_body.extend(trak(
            layout,
            2,
            b"soun",
            SOUND_SCALE,
            times.sound,
            times.skip,
        ));
    }
    let mut moov = boxed(b"moov", &moov_body);
    if layout.open_moov {
        moov[..4].fill(0);
    }
    [
        boxed(b"ftyp", b"isom\0\0\0\0isom"),
        sized(layout.large_mdat, b"mdat", &[0xAA; 64]),
        moov,
    ]
    .concat()
}

fn patched(mut file: Vec<u8>) -> Vec<u8> {
    let length = file.len() as u64;
    let mut cursor = Cursor::new(&mut file);
    patch_edit_list(&mut cursor, length, PUSHED, PRIMING, SAMPLE_RATE).unwrap();
    file
}

/// WHY: the rewrite that keeps AAC priming out of an exported MP4 ran only on
/// the small version 0 files the export tests write. An export past 4 GiB
/// states its `mdat` size in 64 bits, a long or finely timed one states its
/// times in version 1 boxes, and a box may run to the end of the file. A
/// misread offset in any of those leaves the sound late by the priming and
/// long by the last packet, or writes the times into the wrong bytes. Each
/// layout must come out byte for byte as the same layout written with the
/// trimmed times: the sound's `tkhd` and `elst` set, the movie as long as its
/// longest track, and nothing else changed.
///
/// Not covered: the files the muxer writes, covered by the export tests.
#[test]
fn every_box_layout_trims_the_sound_and_changes_nothing_else() {
    let plain = Layout {
        version: 0,
        large_mdat: false,
        large_trak: false,
        open_moov: false,
    };
    let layouts = [
        ("version 0", plain),
        (
            "version 1",
            Layout {
                version: 1,
                ..plain
            },
        ),
        (
            "64-bit mdat size",
            Layout {
                large_mdat: true,
                ..plain
            },
        ),
        (
            "64-bit trak size",
            Layout {
                large_trak: true,
                ..plain
            },
        ),
        (
            "moov runs to the end",
            Layout {
                open_moov: true,
                ..plain
            },
        ),
        (
            "all of them",
            Layout {
                version: 1,
                large_mdat: true,
                large_trak: true,
                open_moov: true,
            },
        ),
    ];
    for (name, layout) in layouts {
        let muxed = mp4(layout, MUXED, true);
        assert_eq!(patched(muxed), mp4(layout, TRIMMED, true), "{name}");
    }
}

/// WHY: a file the rewrite cannot read must reach the caller as written, not
/// half rewritten. Cases: no sound track, a `moov` that states more bytes
/// than the file holds, and a 64-bit size cut off by the end of the file.
#[test]
fn a_file_without_a_readable_sound_track_is_left_as_written() {
    let plain = Layout {
        version: 0,
        large_mdat: false,
        large_trak: false,
        open_moov: false,
    };
    let silent = mp4(plain, MUXED, false);
    let mut cut = mp4(plain, MUXED, true);
    cut.truncate(cut.len() - 1);
    let large_cut = [
        boxed(b"ftyp", b"isom"),
        1u32.to_be_bytes().to_vec(),
        b"mdat\0\0\0".to_vec(),
    ]
    .concat();
    for (name, file) in [
        ("no sound track", silent),
        ("moov cut short", cut),
        ("largesize cut short", large_cut),
    ] {
        assert_eq!(patched(file.clone()), file, "{name}");
    }
}
