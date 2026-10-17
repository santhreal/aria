//! A YUV4MPEG2 (`.y4m`) decoder: 8-bit 4:2:0 frames, every frame a sync
//! frame, read through `fs-read-at` one frame per call.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use crate::{
    DecodeError, DecodedFrame, DecoderSpec, FrameColor, FrameFormat, Instance, MediaProbe,
    NEXT_INSTANCE, OpenedVideo, VideoInfo, fs_len, fs_read_at, register_decoder,
};

pub(crate) const DECODER_ID: &str = "example.y4m";

/// Longest stream header read, comments included.
const MAX_HEADER: u32 = 64 * 1024;

/// One file's layout, parsed from its stream header and first frame header.
#[derive(Clone, Copy)]
struct Layout {
    width: u32,
    height: u32,
    rate_num: i32,
    rate_den: i32,
    /// Bytes before the first frame.
    header: u64,
    /// Bytes of one frame header and its planes.
    frame: u64,
    /// The `FRAME` line, newline included.
    frame_header: u64,
    frames: i64,
    /// The header carries `XARIA=TRAP`: decoding a frame traps, for tests of
    /// how the host fails a plugin.
    trap: bool,
}

impl Layout {
    fn luma(self) -> usize {
        self.width as usize * self.height as usize
    }

    fn chroma(self) -> usize {
        self.luma() / 4
    }

    fn color(self) -> FrameColor {
        if self.height >= 720 {
            FrameColor::Bt709
        } else {
            FrameColor::Bt601
        }
    }
}

struct Stream {
    path: String,
    layout: Layout,
    next: i64,
}

static STREAMS: Mutex<Option<HashMap<u64, Stream>>> = Mutex::new(None);

pub(crate) fn register() -> Result<(), String> {
    register_decoder(&DecoderSpec {
        id: DECODER_ID.to_owned(),
        name: "YUV4MPEG2".to_owned(),
        extensions: vec!["y4m".to_owned()],
        priority: 0,
    })
}

fn unsupported(reason: impl Into<String>) -> DecodeError {
    DecodeError::Unsupported(reason.into())
}

fn layout(path: &str) -> Result<Layout, DecodeError> {
    let head = fs_read_at(path, 0, MAX_HEADER).map_err(DecodeError::Failed)?;
    let Some(rest) = head.strip_prefix(b"YUV4MPEG2") else {
        return Err(unsupported("not a YUV4MPEG2 file"));
    };
    let end = rest
        .iter()
        .position(|&byte| byte == b'\n')
        .ok_or_else(|| unsupported("the stream header has no end within 64 KiB"))?;
    let line = std::str::from_utf8(&rest[..end])
        .map_err(|_| unsupported("the stream header is not text"))?;
    let (mut width, mut height, mut rate, mut trap) = (0u32, 0u32, None, false);
    for token in line.split_ascii_whitespace() {
        let Some((tag, value)) = token.split_at_checked(1) else {
            continue;
        };
        match tag {
            "W" => width = value.parse().map_err(|_| unsupported("bad width"))?,
            "H" => height = value.parse().map_err(|_| unsupported("bad height"))?,
            "F" => {
                let (num, den) = value
                    .split_once(':')
                    .ok_or_else(|| unsupported("bad frame rate"))?;
                let num: i32 = num.parse().map_err(|_| unsupported("bad frame rate"))?;
                let den: i32 = den.parse().map_err(|_| unsupported("bad frame rate"))?;
                if num <= 0 || den <= 0 {
                    return Err(unsupported("bad frame rate"));
                }
                rate = Some((num, den));
            }
            "C" if !matches!(value, "420" | "420jpeg" | "420paldv" | "420mpeg2") => {
                return Err(unsupported(format!(
                    "color space C{value} is not 8-bit 4:2:0"
                )));
            }
            "I" if value != "p" && value != "?" => {
                return Err(unsupported("interlaced streams are not read"));
            }
            "X" if value == "ARIA=TRAP" => trap = true,
            _ => {}
        }
    }
    let (rate_num, rate_den) = rate.ok_or_else(|| unsupported("the stream has no frame rate"))?;
    if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 {
        return Err(unsupported(format!(
            "{width}x{height} is not an even frame size"
        )));
    }
    let header = "YUV4MPEG2".len() as u64 + end as u64 + 1;
    let frame_line = fs_read_at(path, header, 256).map_err(DecodeError::Failed)?;
    let frame_header = match frame_line.iter().position(|&byte| byte == b'\n') {
        Some(newline) if frame_line.starts_with(b"FRAME") => newline as u64 + 1,
        _ if frame_line.is_empty() => "FRAME\n".len() as u64,
        _ => {
            return Err(DecodeError::Failed(
                "the first frame has no FRAME line".into(),
            ));
        }
    };
    let planes = u64::from(width) * u64::from(height) * 3 / 2;
    let frame = frame_header + planes;
    let length = fs_len(path).map_err(DecodeError::Failed)?;
    let frames = i64::try_from(length.saturating_sub(header) / frame)
        .map_err(|_| DecodeError::Failed("the file is too long".into()))?;
    Ok(Layout {
        width,
        height,
        rate_num,
        rate_den,
        header,
        frame,
        frame_header,
        frames,
        trap,
    })
}

pub(crate) fn probe(path: &str) -> Result<MediaProbe, DecodeError> {
    let layout = layout(path)?;
    Ok(MediaProbe {
        width: layout.width,
        height: layout.height,
        frame_rate_num: layout.rate_num,
        frame_rate_den: layout.rate_den,
        frame_count: layout.frames,
        has_audio: false,
        is_image: false,
        video_codec: Some("yuv4mpeg2".to_owned()),
        audio_codec: None,
    })
}

pub(crate) fn open(path: &str) -> Result<OpenedVideo, DecodeError> {
    let layout = layout(path)?;
    let instance = NEXT_INSTANCE.fetch_add(1, Ordering::Relaxed);
    STREAMS
        .lock()
        .map_err(|_| DecodeError::Failed("y4m stream lock poisoned".into()))?
        .get_or_insert_with(HashMap::new)
        .insert(
            instance,
            Stream {
                path: path.to_owned(),
                layout,
                next: 0,
            },
        );
    Ok(OpenedVideo {
        instance,
        info: VideoInfo {
            width: layout.width,
            height: layout.height,
            format: FrameFormat::Yuv420p,
            color: layout.color(),
            frame_rate_num: layout.rate_num,
            frame_rate_den: layout.rate_den,
            // One tick per frame: a pts is a frame index.
            time_base_num: layout.rate_num,
            time_base_den: layout.rate_den,
            duration: Some(layout.frames),
        },
    })
}

fn with_stream<T>(
    instance: Instance,
    f: impl FnOnce(&mut Stream) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = STREAMS
        .lock()
        .map_err(|_| "y4m stream lock poisoned".to_owned())?;
    let stream = guard
        .get_or_insert_with(HashMap::new)
        .get_mut(&instance)
        .ok_or_else(|| format!("unknown video stream: {instance}"))?;
    f(stream)
}

pub(crate) fn seek(instance: Instance, pts: i64) -> Result<(), String> {
    with_stream(instance, |stream| {
        stream.next = pts.clamp(0, stream.layout.frames);
        Ok(())
    })
}

pub(crate) fn next_frame(instance: Instance) -> Result<Option<DecodedFrame>, String> {
    let (path, layout, index) = with_stream(instance, |stream| {
        let index = stream.next;
        if index < stream.layout.frames {
            stream.next += 1;
        }
        Ok((stream.path.clone(), stream.layout, index))
    })?;
    if index >= layout.frames {
        return Ok(None);
    }
    assert!(
        !layout.trap,
        "y4m decoder trap requested by the file header"
    );
    let offset = layout.header + index as u64 * layout.frame;
    let len = u32::try_from(layout.frame).map_err(|_| "a frame is over 4 GiB".to_owned())?;
    let mut bytes = fs_read_at(&path, offset, len)?;
    if bytes.len() as u64 != layout.frame || !bytes.starts_with(b"FRAME") {
        return Err(format!(
            "frame {index} is not where a file of same-length FRAME lines puts it"
        ));
    }
    let skip = layout.frame_header as usize;
    let v = bytes.split_off(skip + layout.luma() + layout.chroma());
    let u = bytes.split_off(skip + layout.luma());
    let y = bytes.split_off(skip);
    Ok(Some(DecodedFrame {
        pts: index,
        planes: vec![y, u, v],
    }))
}

pub(crate) fn close(instance: Instance) {
    if let Ok(mut guard) = STREAMS.lock()
        && let Some(streams) = guard.as_mut()
    {
        streams.remove(&instance);
    }
}
