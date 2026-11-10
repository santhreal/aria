//! Audio branch and MP4 edit-list trimming for the GStreamer backend.
//!
//! Every AAC encoder prepends a fixed lookahead (priming) window of silence
//! before the first input sample (`avenc_aac`: 1024 samples, `fdkaacenc`: 2048
//! samples, `voaacenc`: 1600 samples) and emits 1024-sample access units through
//! the end of the stream. Without trimming, the muxed `.mp4` plays its audio
//! late by the priming delay and extends past the video sequence by the priming
//! plus the final AAC frame's padding.
//!
//! This module normalizes the `aacparse` access-unit timestamps, flushes the
//! lookahead of encoders that do not drain on EOS (`voaacenc`), bounds the
//! final access unit's duration to the remaining input + priming samples, and
//! rewrites the `soun` track's `elst`, `tkhd`, and `mvhd` atoms once the file
//! closes so both ISO-BMFF edit-list readers (`ffmpeg`, QuickTime) and
//! GStreamer `qtdemux` decode sample 0 at timestamp 0 and stop at the exact
//! pushed sample count.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aria_core::{AudioEncoderConfig, EncodeError, RationalTime};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;

use super::{AAC_BIT_RATE, AUDIO_QUEUE_NS, Element, NS_PER_SEC, make, mux_queue};

/// Samples per AAC-LC access unit.
const AAC_FRAME_SAMPLES: u64 = 1024;

/// Priming delay (encoder lookahead in PCM samples per channel) that each
/// GStreamer AAC encoder prepends before the first sample of input.
fn aac_priming_samples(name: &str) -> Result<u64, EncodeError> {
    match name {
        "avenc_aac" => Ok(1024),
        "fdkaacenc" => Ok(2048),
        "voaacenc" => Ok(1600),
        other => Err(EncodeError::unsupported(format!(
            "AAC encoder {other} has no configured priming delay."
        ))),
    }
}

#[derive(Debug)]
struct AudioPacketGate {
    sample_rate: u32,
    emitted_packets: AtomicU64,
    max_packets: AtomicU64,
    last_packet_samples: AtomicU64,
}

/// The active audio branch of a [`super::GstEncoder`].
#[derive(Debug)]
pub(super) struct AudioBranch {
    pub(super) src: gst_app::AppSrc,
    aac_name: &'static str,
    config: AudioEncoderConfig,
    priming_samples: u64,
    pushed_samples: u64,
    gate: Arc<AudioPacketGate>,
}

impl AudioBranch {
    pub(super) fn add(
        pipeline: &gst::Pipeline,
        mux: &gst::Element,
        config: AudioEncoderConfig,
        aac_override: Option<&str>,
    ) -> Result<Self, EncodeError> {
        let aac_name: &'static str = match aac_override {
            Some(name) => {
                let Some(&known) = super::GstEncoder::AAC_ENCODERS
                    .iter()
                    .find(|&&candidate| candidate == name)
                else {
                    return Err(EncodeError::unsupported(format!(
                        "AAC encoder {name} is not in GstEncoder::AAC_ENCODERS."
                    )));
                };
                if gst::ElementFactory::find(known).is_none() {
                    return Err(EncodeError::unsupported(format!(
                        "AAC encoder plugin {known} is not installed."
                    )));
                }
                known
            }
            None => super::GstEncoder::AAC_ENCODERS
                .iter()
                .copied()
                .find(|n| gst::ElementFactory::find(n).is_some())
                .ok_or_else(|| {
                    EncodeError::unsupported(
                        "no AAC encoder plugin found. Install gstreamer1.0-libav or gstreamer1.0-plugins-bad.",
                    )
                })?,
        };
        let priming_samples = aac_priming_samples(aac_name)?;

        let src = gst_app::AppSrc::builder()
            .format(gst::Format::Time)
            .caps(&audio_caps(config)?)
            .stream_type(gst_app::AppStreamType::Stream)
            .is_live(false)
            .block(true)
            .build();
        let convert = make(Element::AudioConvert)?;
        let resample = make(Element::AudioResample)?;
        let encoder = gst::ElementFactory::make(aac_name)
            .build()
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        if encoder.find_property("bitrate").is_some() {
            encoder.set_property("bitrate", AAC_BIT_RATE);
        }
        let parse = make(Element::AacParse)?;
        let queue = mux_queue(AUDIO_QUEUE_NS)?;

        let gate = Arc::new(AudioPacketGate {
            sample_rate: config.sample_rate.max(1),
            emitted_packets: AtomicU64::new(0),
            max_packets: AtomicU64::new(u64::MAX),
            last_packet_samples: AtomicU64::new(AAC_FRAME_SAMPLES),
        });
        attach_aacparse_probe(&parse, Arc::clone(&gate))?;

        pipeline
            .add_many([
                src.upcast_ref(),
                &convert,
                &resample,
                &encoder,
                &parse,
                &queue,
            ])
            .map_err(|e| EncodeError::Start(e.to_string()))?;
        gst::Element::link_many([
            src.upcast_ref(),
            &convert,
            &resample,
            &encoder,
            &parse,
            &queue,
        ])
        .map_err(|e| EncodeError::Start(e.to_string()))?;
        queue
            .link(mux)
            .map_err(|e| EncodeError::Start(e.to_string()))?;

        Ok(Self {
            src,
            aac_name,
            config,
            priming_samples,
            pushed_samples: 0,
            gate,
        })
    }

    pub(super) fn push_samples(
        &mut self,
        samples: &[f32],
        pts: RationalTime,
    ) -> Result<(), gst::FlowError> {
        let channels = usize::from(self.config.channels.max(1));
        let frames_in_buf = (samples.len() / channels) as u64;
        let bytes = bytemuck::cast_slice::<f32, u8>(samples).to_vec();
        let mut buffer = gst::Buffer::from_mut_slice(bytes);
        if let Some(buf) = buffer.get_mut() {
            buf.set_pts(gst::ClockTime::from_nseconds(
                pts.in_units(NS_PER_SEC) as u64
            ));
        }
        self.src.push_buffer(buffer)?;
        self.pushed_samples = self.pushed_samples.saturating_add(frames_in_buf);
        Ok(())
    }

    /// Set the packet ceiling from the total pushed samples, flush any encoder
    /// lookahead that does not drain on EOS (`voaacenc`), and send EOS.
    pub(super) fn finish_stream(&mut self) -> Result<(), gst::FlowError> {
        if self.pushed_samples > 0 {
            let total_media_samples = self.pushed_samples + self.priming_samples;
            let total_packets = total_media_samples.div_ceil(AAC_FRAME_SAMPLES);
            let last_samples = total_media_samples - (total_packets - 1) * AAC_FRAME_SAMPLES;
            self.gate
                .last_packet_samples
                .store(last_samples, Ordering::Release);
            self.gate
                .max_packets
                .store(total_packets, Ordering::Release);

            if self.aac_name == "voaacenc" {
                let needed_input = total_packets * AAC_FRAME_SAMPLES;
                if needed_input > self.pushed_samples {
                    let pad_frames = (needed_input - self.pushed_samples) as usize;
                    let channels = usize::from(self.config.channels.max(1));
                    let silence = vec![0u8; pad_frames * channels * std::mem::size_of::<f32>()];
                    let mut buffer = gst::Buffer::from_mut_slice(silence);
                    let sr = u64::from(self.config.sample_rate.max(1));
                    let pts_ns = (u128::from(self.pushed_samples) * (NS_PER_SEC as u128)
                        / u128::from(sr)) as u64;
                    if let Some(buf) = buffer.get_mut() {
                        buf.set_pts(gst::ClockTime::from_nseconds(pts_ns));
                    }
                    self.src.push_buffer(buffer)?;
                }
            }
        }
        self.src.end_of_stream().map(|_| ())
    }

    /// Rewrite the closed MP4 file's `moov` box so the `soun` edit list skips
    /// `priming_samples` and stops after `pushed_samples`.
    pub(super) fn patch_mp4_edit_list(&self, path: &Path) -> Result<(), EncodeError> {
        if self.pushed_samples == 0 {
            return Ok(());
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                EncodeError::Encode(format!("couldn't reopen MP4 to trim AAC priming: {e}"))
            })?;
        let file_len = file
            .metadata()
            .map_err(|e| EncodeError::Encode(format!("couldn't stat MP4 output: {e}")))?
            .len();

        let mut pos = 0u64;
        while pos + 8 <= file_len {
            file.seek(SeekFrom::Start(pos))
                .map_err(|e| EncodeError::Encode(format!("MP4 seek failed: {e}")))?;
            let mut hdr = [0u8; 16];
            file.read_exact(&mut hdr[..8])
                .map_err(|e| EncodeError::Encode(format!("MP4 header read failed: {e}")))?;
            let size32 = u32::from_be_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as u64;
            let (box_size, hdr_len) = if size32 == 1 {
                if pos + 16 > file_len {
                    break;
                }
                file.read_exact(&mut hdr[8..16]).map_err(|e| {
                    EncodeError::Encode(format!("MP4 extended header read failed: {e}"))
                })?;
                let size64 = u64::from_be_bytes([
                    hdr[8], hdr[9], hdr[10], hdr[11], hdr[12], hdr[13], hdr[14], hdr[15],
                ]);
                (size64, 16u64)
            } else if size32 == 0 {
                (file_len - pos, 8u64)
            } else {
                (size32, 8u64)
            };
            if box_size < hdr_len || pos + box_size > file_len {
                break;
            }
            if &hdr[4..8] == b"moov" {
                let Ok(moov_len) = usize::try_from(box_size) else {
                    break;
                };
                file.seek(SeekFrom::Start(pos))
                    .map_err(|e| EncodeError::Encode(format!("MP4 moov seek failed: {e}")))?;
                let mut moov = vec![0u8; moov_len];
                file.read_exact(&mut moov)
                    .map_err(|e| EncodeError::Encode(format!("MP4 moov read failed: {e}")))?;
                let sr = u64::from(self.config.sample_rate.max(1));
                if patch_moov_bytes(
                    &mut moov,
                    hdr_len as usize,
                    self.pushed_samples,
                    self.priming_samples,
                    sr,
                ) {
                    file.seek(SeekFrom::Start(pos)).map_err(|e| {
                        EncodeError::Encode(format!("MP4 moov write seek failed: {e}"))
                    })?;
                    file.write_all(&moov)
                        .map_err(|e| EncodeError::Encode(format!("MP4 moov write failed: {e}")))?;
                }
                break;
            }
            pos += box_size;
        }
        Ok(())
    }
}

fn attach_aacparse_probe(
    parse: &gst::Element,
    gate: Arc<AudioPacketGate>,
) -> Result<(), EncodeError> {
    let pad = parse
        .static_pad("src")
        .ok_or_else(|| EncodeError::Start("aacparse has no src pad.".into()))?;
    pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
        let idx = gate.emitted_packets.fetch_add(1, Ordering::AcqRel);
        let max_packets = gate.max_packets.load(Ordering::Acquire);
        if idx >= max_packets {
            return gst::PadProbeReturn::Drop;
        }
        let pkt_samples = if idx + 1 == max_packets {
            gate.last_packet_samples.load(Ordering::Acquire)
        } else {
            AAC_FRAME_SAMPLES
        };
        if let Some(gst::PadProbeData::Buffer(ref mut buffer)) = info.data {
            let buf = buffer.make_mut();
            let sr = u128::from(gate.sample_rate);
            let pts_ns = (u128::from(idx * AAC_FRAME_SAMPLES) * (NS_PER_SEC as u128) + sr / 2) / sr;
            let dur_ns = (u128::from(pkt_samples) * (NS_PER_SEC as u128) + sr / 2) / sr;
            let pts = gst::ClockTime::from_nseconds(pts_ns as u64);
            buf.set_pts(pts);
            buf.set_dts(pts);
            buf.set_duration(gst::ClockTime::from_nseconds(dur_ns as u64));
        }
        gst::PadProbeReturn::Ok
    });
    Ok(())
}

fn audio_caps(audio: AudioEncoderConfig) -> Result<gst::Caps, EncodeError> {
    Ok(gst::Caps::builder("audio/x-raw")
        .field("format", gst_audio::AUDIO_FORMAT_F32.to_str())
        .field("layout", "interleaved")
        .field("rate", audio.sample_rate as i32)
        .field("channels", i32::from(audio.channels))
        .build())
}

fn next_box(buf: &[u8], pos: usize, end: usize) -> Option<([u8; 4], usize, usize, usize)> {
    if pos + 8 > end || end > buf.len() {
        return None;
    }
    let size32 = u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as usize;
    let fourcc = [buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]];
    let (box_size, hdr_len) = if size32 == 1 {
        if pos + 16 > end {
            return None;
        }
        let size64 = u64::from_be_bytes([
            buf[pos + 8],
            buf[pos + 9],
            buf[pos + 10],
            buf[pos + 11],
            buf[pos + 12],
            buf[pos + 13],
            buf[pos + 14],
            buf[pos + 15],
        ]);
        let Ok(sz) = usize::try_from(size64) else {
            return None;
        };
        (sz, 16)
    } else if size32 == 0 {
        (end - pos, 8)
    } else {
        (size32, 8)
    };
    if box_size < hdr_len || pos + box_size > end {
        return None;
    }
    Some((fourcc, pos, pos + hdr_len, pos + box_size))
}

fn patch_moov_bytes(
    moov: &mut [u8],
    moov_payload_start: usize,
    pushed_samples: u64,
    priming_samples: u64,
    sample_rate: u64,
) -> bool {
    let moov_end = moov.len();
    let mut mvhd_loc: Option<(u8, usize, u64)> = None;
    let mut traks: Vec<(usize, usize)> = Vec::new();

    let mut p = moov_payload_start;
    while let Some((fourcc, _bstart, payload, bend)) = next_box(moov, p, moov_end) {
        if &fourcc == b"mvhd" && payload + 20 <= bend {
            let v = moov[payload];
            if v == 0 {
                let ts = u32::from_be_bytes([
                    moov[payload + 12],
                    moov[payload + 13],
                    moov[payload + 14],
                    moov[payload + 15],
                ]) as u64;
                mvhd_loc = Some((0, payload + 16, ts));
            } else if v == 1 && payload + 28 <= bend {
                let ts = u32::from_be_bytes([
                    moov[payload + 20],
                    moov[payload + 21],
                    moov[payload + 22],
                    moov[payload + 23],
                ]) as u64;
                mvhd_loc = Some((1, payload + 24, ts));
            }
        } else if &fourcc == b"trak" {
            traks.push((payload, bend));
        }
        p = bend;
    }

    let Some((mvhd_v, mvhd_dur_off, mvhd_ts)) = mvhd_loc else {
        return false;
    };
    if mvhd_ts == 0 {
        return false;
    }

    let target_audio_mvhd_dur = (pushed_samples * mvhd_ts + sample_rate / 2) / sample_rate;
    let mut max_trak_dur = 0u64;
    let mut patched_soun = false;

    for (t_payload, t_end) in traks {
        let mut tkhd_loc: Option<(u8, usize)> = None;
        let mut elst_loc: Option<(u8, usize)> = None;
        let mut mdhd_ts: Option<u64> = None;
        let mut is_soun = false;

        let mut tp = t_payload;
        while let Some((fourcc, _, c_payload, c_end)) = next_box(moov, tp, t_end) {
            if &fourcc == b"tkhd" && c_payload + 24 <= c_end {
                let v = moov[c_payload];
                if v == 0 {
                    tkhd_loc = Some((0, c_payload + 20));
                } else if v == 1 && c_payload + 36 <= c_end {
                    tkhd_loc = Some((1, c_payload + 28));
                }
            } else if &fourcc == b"edts" {
                let mut ep = c_payload;
                while let Some((efourcc, _, e_payload, e_end)) = next_box(moov, ep, c_end) {
                    if &efourcc == b"elst" && e_payload + 8 <= e_end {
                        let v = moov[e_payload];
                        let count = u32::from_be_bytes([
                            moov[e_payload + 4],
                            moov[e_payload + 5],
                            moov[e_payload + 6],
                            moov[e_payload + 7],
                        ]);
                        if count >= 1 {
                            elst_loc = Some((v, e_payload + 8));
                        }
                    }
                    ep = e_end;
                }
            } else if &fourcc == b"mdia" {
                let mut mp = c_payload;
                while let Some((mfourcc, _, m_payload, m_end)) = next_box(moov, mp, c_end) {
                    if &mfourcc == b"mdhd" && m_payload + 16 <= m_end {
                        let v = moov[m_payload];
                        if v == 0 {
                            mdhd_ts = Some(u32::from_be_bytes([
                                moov[m_payload + 12],
                                moov[m_payload + 13],
                                moov[m_payload + 14],
                                moov[m_payload + 15],
                            ]) as u64);
                        } else if v == 1 && m_payload + 24 <= m_end {
                            mdhd_ts = Some(u32::from_be_bytes([
                                moov[m_payload + 20],
                                moov[m_payload + 21],
                                moov[m_payload + 22],
                                moov[m_payload + 23],
                            ]) as u64);
                        }
                    } else if &mfourcc == b"hdlr" && m_payload + 12 <= m_end {
                        is_soun = &moov[m_payload + 8..m_payload + 12] == b"soun";
                    }
                    mp = m_end;
                }
            }
            tp = c_end;
        }

        if is_soun {
            if let Some((tkhd_v, tkhd_dur_off)) = tkhd_loc {
                write_u32_or_u64(moov, tkhd_v, tkhd_dur_off, target_audio_mvhd_dur);
            }
            let track_ts = mdhd_ts.unwrap_or(sample_rate).max(1);
            let priming_media_time = (priming_samples * track_ts + sample_rate / 2) / sample_rate;
            if let Some((elst_v, entry_off)) = elst_loc {
                if elst_v == 0 && entry_off + 8 <= moov_end {
                    moov[entry_off..entry_off + 4]
                        .copy_from_slice(&(target_audio_mvhd_dur as u32).to_be_bytes());
                    moov[entry_off + 4..entry_off + 8]
                        .copy_from_slice(&(priming_media_time as i32).to_be_bytes());
                    patched_soun = true;
                } else if elst_v == 1 && entry_off + 16 <= moov_end {
                    moov[entry_off..entry_off + 8]
                        .copy_from_slice(&target_audio_mvhd_dur.to_be_bytes());
                    moov[entry_off + 8..entry_off + 16]
                        .copy_from_slice(&(priming_media_time as i64).to_be_bytes());
                    patched_soun = true;
                }
            }
        }

        if let Some((tkhd_v, tkhd_dur_off)) = tkhd_loc {
            let dur = read_u32_or_u64(moov, tkhd_v, tkhd_dur_off);
            max_trak_dur = max_trak_dur.max(dur);
        }
    }

    if patched_soun && max_trak_dur > 0 {
        write_u32_or_u64(moov, mvhd_v, mvhd_dur_off, max_trak_dur);
    }
    patched_soun
}

fn read_u32_or_u64(buf: &[u8], version: u8, off: usize) -> u64 {
    if version == 0 && off + 4 <= buf.len() {
        u32::from_be_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]]) as u64
    } else if version == 1 && off + 8 <= buf.len() {
        u64::from_be_bytes([
            buf[off],
            buf[off + 1],
            buf[off + 2],
            buf[off + 3],
            buf[off + 4],
            buf[off + 5],
            buf[off + 6],
            buf[off + 7],
        ])
    } else {
        0
    }
}

fn write_u32_or_u64(buf: &mut [u8], version: u8, off: usize, val: u64) {
    if version == 0 && off + 4 <= buf.len() {
        buf[off..off + 4].copy_from_slice(&(val as u32).to_be_bytes());
    } else if version == 1 && off + 8 <= buf.len() {
        buf[off..off + 8].copy_from_slice(&val.to_be_bytes());
    }
}
