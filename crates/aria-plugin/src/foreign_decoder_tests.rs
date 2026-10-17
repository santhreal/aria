//! The checks every WASM and C ABI decoder passes through. Each test drives a
//! scripted [`DecoderCalls`] and asserts what [`ForeignDecoder`] hands the
//! renderer: refused streams, refused planes, error kinds by call, the
//! roll-forward path, audio positions, and instance closes. A runtime that
//! converts its values wrongly before they reach the adapter is covered by
//! that runtime's own tests, not these.

use std::path::Path;
use std::sync::Arc;

use aria_core::{ColorSpace, DecodeError, PixelFormat, Plane, Rational, RationalTime, Rect};
use aria_decoder::{ExternalDecoder, MediaProbe};
use parking_lot::Mutex;

use super::{
    CallError, DecodedPlanes, DecoderCalls, ForeignDecoder, MAX_FOREIGN_FRAME_SIDE, StreamInfo,
};

const RATE: Rational = Rational::new(30, 1);
const VIDEO: u64 = 7;
const AUDIO: u64 = 9;

fn stream() -> StreamInfo {
    StreamInfo {
        width: 64,
        height: 36,
        format: PixelFormat::Nv12,
        color: ColorSpace::BT601,
        frame_rate: RATE,
        time_base: RATE,
        duration: Some(90),
    }
}

/// Tightly packed planes for `stream`, each filled with its index.
fn tight(stream: &StreamInfo) -> Vec<Plane> {
    (0..stream.format.plane_count())
        .map(|index| {
            let (row_bytes, rows) = stream.plane_shape(index);
            Plane::new(vec![index as u8; row_bytes * rows], row_bytes, rows)
        })
        .collect()
}

#[derive(Default)]
struct State {
    stream: Mutex<Option<StreamInfo>>,
    log: Mutex<Vec<String>>,
    /// The frame index `next_frame` returns next, at one frame per tick.
    next: Mutex<i64>,
    planes: Mutex<Option<Vec<Plane>>>,
    /// Fails the call at this index in the log.
    fail: Mutex<Option<(usize, CallError)>>,
    written: Mutex<Option<usize>>,
}

impl State {
    fn record(&self, entry: String) -> Result<(), CallError> {
        let mut log = self.log.lock();
        log.push(entry);
        let mut fail = self.fail.lock();
        match fail.take() {
            Some((at, error)) if at + 1 == log.len() => Err(error),
            armed => {
                *fail = armed;
                Ok(())
            }
        }
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().clone()
    }

    fn count(&self, prefix: &str) -> usize {
        self.log
            .lock()
            .iter()
            .filter(|entry| entry.starts_with(prefix))
            .count()
    }
}

struct Script(Arc<State>);

impl DecoderCalls for Script {
    fn probe(&self, path: &Path) -> Result<MediaProbe, CallError> {
        self.0.record(format!("probe {}", path.display()))?;
        Ok(MediaProbe {
            width: 64,
            height: 36,
            frame_rate: RATE,
            frame_count: 90,
            has_audio: true,
            is_image: false,
            video_codec: Some("scripted".into()),
            audio_codec: None,
        })
    }

    fn open_video(&self, _path: &Path) -> Result<(u64, StreamInfo), CallError> {
        self.0.record("open_video".into())?;
        Ok((VIDEO, self.0.stream.lock().unwrap_or_else(stream)))
    }

    fn seek(&self, _path: &Path, instance: u64, pts: i64) -> Result<(), CallError> {
        self.0.record(format!("seek {instance} {pts}"))?;
        *self.0.next.lock() = pts;
        Ok(())
    }

    fn next_frame(
        &self,
        _path: &Path,
        _instance: u64,
        stream: &StreamInfo,
    ) -> Result<Option<DecodedPlanes>, CallError> {
        self.0.record("next".into())?;
        let mut next = self.0.next.lock();
        if *next >= 90 {
            return Ok(None);
        }
        let pts = *next;
        *next += 1;
        let planes = self.0.planes.lock().take().unwrap_or_else(|| tight(stream));
        Ok(Some(DecodedPlanes { pts, planes }))
    }

    fn open_audio(&self, _path: &Path, rate: u32, channels: u16) -> Result<u64, CallError> {
        self.0.record(format!("open_audio {rate} {channels}"))?;
        Ok(AUDIO)
    }

    fn read_audio(&self, _path: &Path, instance: u64, out: &mut [f32]) -> Result<usize, CallError> {
        self.0.record(format!("read {instance} {}", out.len()))?;
        out.fill(0.25);
        Ok(self.0.written.lock().take().unwrap_or(out.len()))
    }

    fn seek_audio(&self, _path: &Path, instance: u64, frame: i64) -> Result<(), CallError> {
        self.0.record(format!("seek_audio {instance} {frame}"))
    }

    fn close(&self, _path: &Path, instance: u64) {
        self.0.log.lock().push(format!("close {instance}"));
    }
}

fn decoder() -> (ForeignDecoder<Script>, Arc<State>) {
    let state = Arc::new(State::default());
    (ForeignDecoder::new(Script(Arc::clone(&state))), state)
}

fn path() -> &'static Path {
    Path::new("/media/clip.scripted")
}

#[test]
fn streams_the_host_cannot_present_are_refused_and_closed() {
    let cases: [(fn(&mut StreamInfo), &str); 8] = [
        (|s| s.format = PixelFormat::Rgba8, "Rgba8"),
        (|s| s.format = PixelFormat::P010, "P010"),
        (|s| s.width = 63, "63x36"),
        (|s| s.height = 0, "64x0"),
        (|s| s.width = MAX_FOREIGN_FRAME_SIDE + 2, "16386x36"),
        (|s| s.time_base = Rational::new(0, 1), "time base 0/1"),
        (|s| s.frame_rate = Rational::new(30, 0), "frame rate 30/0"),
        (
            |s| s.time_base = Rational::new(-90_000, 1),
            "time base -90000/1",
        ),
    ];
    for (bend, fragment) in cases {
        let (decoder, state) = decoder();
        let mut bent = stream();
        bend(&mut bent);
        *state.stream.lock() = Some(bent);
        match decoder.open_video(path()) {
            Err(DecodeError::Open(reason)) => {
                assert!(reason.contains(fragment), "{fragment}: {reason}");
            }
            Err(other) => panic!("{fragment}: expected an open refusal, got {other:?}"),
            Ok(_) => panic!("{fragment}: the stream opened"),
        }
        assert_eq!(state.log(), ["open_video", "close 7"], "{fragment}");
    }
}

#[test]
fn every_supported_format_and_size_opens() {
    for format in [PixelFormat::Nv12, PixelFormat::Yuv420p] {
        for (width, height) in [(2, 2), (64, 36), (MAX_FOREIGN_FRAME_SIDE, 2)] {
            let (decoder, state) = decoder();
            *state.stream.lock() = Some(StreamInfo {
                width,
                height,
                format,
                ..stream()
            });
            let mut video = decoder.open_video(path()).expect("opens");
            let info = video.info();
            assert_eq!(
                (info.coded_size, info.display_size, info.pixel_format),
                ((width, height), (width, height), format)
            );
            let frame = video.next_frame().expect("decodes").expect("a frame");
            assert_eq!(frame.cpu().expect("cpu").planes.len(), format.plane_count());
        }
    }
}

#[test]
fn frames_carry_the_stream_time_base_size_and_color() {
    let (decoder, _state) = decoder();
    let mut video = decoder.open_video(path()).expect("opens");
    assert_eq!(video.info().duration, Some(RationalTime::new(90, RATE)));
    video.seek(RationalTime::new(12, RATE)).expect("seeks");
    let frame = video.next_frame().expect("decodes").expect("a frame");
    assert_eq!(frame.pts, RationalTime::new(12, RATE));
    assert_eq!(frame.format, PixelFormat::Nv12);
    assert_eq!(frame.color, ColorSpace::BT601);
    assert_eq!(frame.coded_size, (64, 36));
    assert_eq!(frame.visible, Rect::new(0, 0, 64, 36));
    let planes = &frame.cpu().expect("cpu").planes;
    assert_eq!(
        planes
            .iter()
            .map(|p| (p.stride, p.rows, p.data.len()))
            .collect::<Vec<_>>(),
        [(64, 36, 64 * 36), (64, 18, 64 * 18)]
    );
}

#[test]
fn planes_that_do_not_fit_the_stream_fail_the_frame() {
    let y = || Plane::new(vec![0; 64 * 36], 64, 36);
    let cases: [(Vec<Plane>, &str); 5] = [
        (vec![y()], "has 2 planes, the decoder returned 1"),
        (
            vec![y(), Plane::new(vec![0; 64 * 18], 64, 18), y()],
            "returned 3",
        ),
        (
            vec![Plane::new(vec![0; 63 * 36], 63, 36), y()],
            "plane 0 holds 2268 bytes at stride 63",
        ),
        (
            vec![y(), Plane::new(vec![0; 64 * 17], 64, 18)],
            "plane 1 holds 1088 bytes",
        ),
        (
            vec![y(), Plane::new(vec![0; 64 * 17], 64, 17)],
            "over 17 rows; it needs a stride of at least 64 over 18 rows",
        ),
    ];
    for (planes, fragment) in cases {
        let (decoder, state) = decoder();
        let mut video = decoder.open_video(path()).expect("opens");
        *state.planes.lock() = Some(planes);
        match video.next_frame() {
            Err(DecodeError::Decode(reason)) => {
                assert!(reason.contains(fragment), "{fragment}: {reason}");
            }
            other => panic!("{fragment}: expected a decode error, got {other:?}"),
        }
    }
}

#[test]
fn call_errors_keep_unsupported_and_take_the_failed_kind_of_their_call() {
    type Call = fn(&ForeignDecoder<Script>) -> Result<(), DecodeError>;
    let open_video: Call = |d| d.open_video(path()).map(drop);
    let seek: Call = |d| {
        let mut video = d.open_video(path())?;
        video.seek(RationalTime::new(3, RATE))
    };
    let next: Call = |d| d.open_video(path())?.next_frame().map(drop);
    let open_audio: Call = |d| d.open_audio(path(), 48_000, 2).map(drop);
    let read: Call = |d| {
        d.open_audio(path(), 48_000, 2)?
            .read(&mut [0.0; 8])
            .map(drop)
    };
    let seek_audio: Call = |d| d.open_audio(path(), 48_000, 2)?.seek_to_frame(40);
    let probe: Call = |d| d.probe(path()).map(drop);
    // Calls before the failing one, and the kind a plain failure becomes.
    let cases: [(&str, Call, usize, fn(String) -> DecodeError); 7] = [
        ("probe", probe, 0, DecodeError::Open),
        ("open_video", open_video, 0, DecodeError::Open),
        ("seek", seek, 1, DecodeError::Seek),
        ("next_frame", next, 1, DecodeError::Decode),
        ("open_audio", open_audio, 0, DecodeError::Open),
        ("read", read, 1, DecodeError::Decode),
        ("seek_audio", seek_audio, 1, DecodeError::Seek),
    ];
    for (name, call, before, failed) in cases {
        for (error, expected) in [
            (
                CallError::Unsupported(format!("{name} not mine")),
                DecodeError::Unsupported(format!("{name} not mine")),
            ),
            (
                CallError::Failed(format!("{name} broke")),
                failed(format!("{name} broke")),
            ),
        ] {
            let (decoder, state) = decoder();
            *state.fail.lock() = Some((before, error));
            assert_eq!(
                call(&decoder).map_err(|err| err.to_string()),
                Err(expected.to_string()),
                "{name}"
            );
        }
    }
}

#[test]
fn sequential_frame_at_rolls_forward_and_a_backward_target_seeks() {
    let (decoder, state) = decoder();
    let mut video = decoder.open_video(path()).expect("opens");
    for tick in 0..10 {
        let frame = video
            .frame_at(RationalTime::new(tick, RATE))
            .expect("decodes")
            .expect("a frame");
        assert_eq!(frame.pts, RationalTime::new(tick, RATE));
    }
    assert_eq!(state.count("seek"), 1, "{:?}", state.log());
    let back = video
        .frame_at(RationalTime::new(4, RATE))
        .expect("decodes")
        .expect("a frame");
    assert_eq!(back.pts.value, 4);
    assert_eq!(state.count("seek"), 2, "{:?}", state.log());
    // Past the end, the nearest frame is the last one.
    let tail = video
        .frame_at_nearest(RationalTime::new(95, RATE))
        .expect("decodes")
        .expect("the last frame");
    assert_eq!(tail.pts.value, 89);
}

#[test]
fn audio_position_counts_whole_frames_and_follows_seeks() {
    let (decoder, state) = decoder();
    let mut audio = decoder.open_audio(path(), 48_000, 2).expect("opens");
    assert_eq!(audio.position(), Some(0));
    let mut block = [0.0; 8];
    assert_eq!(audio.read(&mut block).expect("reads"), 4);
    assert_eq!(block, [0.25; 8]);
    assert_eq!(audio.position(), Some(4));
    audio.seek_to_frame(4).expect("already there");
    assert_eq!(state.count("seek_audio"), 0);
    audio.seek_to_frame(100).expect("seeks");
    assert_eq!(audio.position(), Some(100));
    *state.written.lock() = Some(2);
    assert_eq!(audio.read(&mut block).expect("a short read"), 1);
    assert_eq!(audio.position(), Some(101));
    for (written, fragment) in [
        (3, "wrote 3 samples into a block of 8"),
        (10, "wrote 10 samples into a block of 8"),
    ] {
        *state.written.lock() = Some(written);
        match audio.read(&mut block) {
            Err(DecodeError::Decode(reason)) => assert!(reason.contains(fragment), "{reason}"),
            other => panic!("{written}: expected a decode error, got {other:?}"),
        }
        assert_eq!(audio.position(), Some(101));
    }
    assert_eq!(
        state.log()[..4],
        [
            "open_audio 48000 2",
            "read 9 8",
            "seek_audio 9 100",
            "read 9 8"
        ]
    );
}

#[test]
fn dropping_a_stream_closes_its_instance_once() {
    let (decoder, state) = decoder();
    let probe = decoder.probe(path()).expect("probes");
    assert_eq!(probe.video_codec.as_deref(), Some("scripted"));
    let video = decoder.open_video(path()).expect("opens");
    let audio = decoder.open_audio(path(), 48_000, 1).expect("opens");
    assert_eq!(state.count("close"), 0);
    drop(video);
    assert_eq!(state.count("close 7"), 1);
    drop(audio);
    drop(decoder);
    assert_eq!(
        state.log(),
        [
            "probe /media/clip.scripted",
            "open_video",
            "open_audio 48000 1",
            "close 7",
            "close 9"
        ]
    );
}
