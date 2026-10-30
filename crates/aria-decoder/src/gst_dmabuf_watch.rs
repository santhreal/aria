//! The video decoders a DMABuf attempt plugs, and the early stop for one that
//! cannot output DMABuf.
//!
//! `uridecodebin` plugs no converter after a decoder, so a decoder whose source
//! templates offer no `memory:DMABuf` decides the attempt before it opens. Such
//! a decoder is starved ([`starve`]): its sink pad drops the caps event and
//! every buffer, so it never creates a hardware session, and the attempt stops
//! at the next state poll. On an RTX 5090 the failed attempt took 265 ms with
//! `nvh264dec` opened and torn down, and 20 ms starved. A decoder that fails to
//! reach `READY` gets no caps and is not marked, so `decodebin` can still plug
//! the next one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use gstreamer as gst;
use gstreamer::prelude::*;
use parking_lot::Mutex;

pub(crate) const DMABUF_MEMORY: &str = "memory:DMABuf";

/// Cached result of [`registry_has_dmabuf_video_decoder`]: `0` = uncomputed,
/// `1` = false, `2` = true. Reset by [`invalidate_dmabuf_decoder_cache`] when
/// [`crate::gst_unloadable::demote`] takes a plugin out of autoplugging.
static HAS_DMABUF_DECODER: AtomicU8 = AtomicU8::new(0);

pub(crate) fn invalidate_dmabuf_decoder_cache() {
    HAS_DMABUF_DECODER.store(0, Ordering::Release);
}

/// What `uridecodebin` plugged into one DMABuf attempt.
#[derive(Default)]
pub(crate) struct DmabufWatch {
    /// The video decoders plugged, in the order they were plugged.
    plugged: Mutex<Vec<gst::ElementFactory>>,
    /// A starved decoder received its caps: the attempt cannot produce DMABuf.
    stopped: AtomicBool,
}

impl DmabufWatch {
    /// Whether a plugged decoder with no DMABuf output was starved.
    pub(crate) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    /// Each plugged video decoder and the caps its source pads can produce.
    pub(crate) fn decoders(&self) -> Vec<(String, gst::Caps)> {
        self.plugged
            .lock()
            .iter()
            .map(|factory| (factory.name().to_string(), src_caps(factory)))
            .collect()
    }
}

/// Record each video decoder `bin` plugs into `watch`, and when
/// `starve_non_dmabuf` holds, starve each one that cannot output DMABuf.
pub(crate) fn watch_video_decoders(
    bin: &gst::Bin,
    watch: &Arc<DmabufWatch>,
    starve_non_dmabuf: bool,
) {
    let watch = Arc::clone(watch);
    bin.connect_deep_element_added(move |_, _, element| {
        let Some(factory) = element.factory() else {
            return;
        };
        if !crate::gst::is_video_decoder(&factory) {
            return;
        }
        if starve_non_dmabuf && !offers_dmabuf(&src_caps(&factory)) {
            starve(element, &watch);
        }
        watch.plugged.lock().push(factory);
    });
}

/// Whether `factory` is a ranked video decoder whose static source pad
/// templates explicitly advertise `memory:DMABuf`.
///
/// Wrapper bins with `ANY` static source caps (such as `vp8alphadecodebin`) do
/// not count: when a host has a DMABuf-capable video decoder, that decoder's
/// own factory is registered and lists `memory:DMABuf`.
pub(crate) fn factory_advertises_dmabuf(factory: &gst::ElementFactory) -> bool {
    crate::gst::is_video_decoder(factory)
        && factory.rank() != gst::Rank::NONE
        && memory_kinds(&src_caps(factory))
            .iter()
            .any(|kind| kind == DMABUF_MEMORY)
}

/// Whether any ranked video decoder in the GStreamer registry advertises
/// `memory:DMABuf` output on its static source pad templates.
pub(crate) fn registry_has_dmabuf_video_decoder() -> bool {
    crate::gst_context::demote_recorded_unloadable();
    match HAS_DMABUF_DECODER.load(Ordering::Acquire) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    let has = gst::Registry::get()
        .features(gst::ElementFactory::static_type())
        .into_iter()
        .filter_map(|feature| feature.downcast::<gst::ElementFactory>().ok())
        .any(|factory| factory_advertises_dmabuf(&factory));
    HAS_DMABUF_DECODER.store(if has { 2 } else { 1 }, Ordering::Release);
    has
}

/// Consume the caps event and every buffer before `decoder`'s sink pad, and
/// mark `watch` stopped when the first of them arrives. Other events pass, so
/// the stream stays well formed.
///
/// The probe returns `Handled`, not `Drop`: gstreamer-rs takes the item out of
/// the probe info before calling the closure, so on `Drop` GStreamer unrefs a
/// null item and logs a `gst_mini_object_unref` critical for each one.
pub(crate) fn starve(decoder: &gst::Element, watch: &Arc<DmabufWatch>) {
    let Some(pad) = decoder.static_pad("sink") else {
        return;
    };
    let watch = Arc::clone(watch);
    pad.add_probe(gst::PadProbeType::DATA_DOWNSTREAM, move |_, info| {
        if let Some(gst::PadProbeData::Event(event)) = &info.data
            && event.type_() != gst::EventType::Caps
        {
            return gst::PadProbeReturn::Ok;
        }
        watch.stopped.store(true, Ordering::Release);
        gst::PadProbeReturn::Handled
    });
}

/// Whether caps allow `memory:DMABuf`, directly or as any memory.
pub(crate) fn offers_dmabuf(caps: &gst::CapsRef) -> bool {
    memory_kinds(caps)
        .iter()
        .any(|kind| kind == DMABUF_MEMORY || kind == "any memory")
}

/// Why no DMABuf reached the sink: each video decoder plugged and the memory
/// its output can be in.
pub(crate) fn dmabuf_cause(decoders: &[(String, gst::Caps)]) -> String {
    if decoders.is_empty() {
        return "no video decoder was plugged".to_owned();
    }
    let causes: Vec<String> = decoders
        .iter()
        .map(|(name, caps)| {
            let memory = memory_kinds(caps);
            if memory.iter().any(|kind| kind == DMABUF_MEMORY) {
                format!("{name} can output {DMABUF_MEMORY}, but none reached the sink")
            } else {
                format!(
                    "{name} outputs {} and no {DMABUF_MEMORY}",
                    memory.join(", ")
                )
            }
        })
        .collect();
    causes.join("; ")
}

/// The memory kinds `caps` allow, in order, each once.
fn memory_kinds(caps: &gst::CapsRef) -> Vec<String> {
    if caps.is_any() {
        return vec!["any memory".to_owned()];
    }
    let mut kinds: Vec<String> = Vec::new();
    for (_, features) in caps.iter_with_features() {
        let kind = if features.is_any() {
            "any memory".to_owned()
        } else if features.is_empty() || features.contains("memory:SystemMemory") {
            "system memory".to_owned()
        } else {
            features.to_string()
        };
        if !kinds.contains(&kind) {
            kinds.push(kind);
        }
    }
    kinds
}

/// Every caps `factory`'s source pads can produce.
fn src_caps(factory: &gst::ElementFactory) -> gst::Caps {
    let mut caps = gst::Caps::new_empty();
    let all = caps.make_mut();
    for template in factory.static_pad_templates().iter() {
        if template.direction() == gst::PadDirection::Src {
            all.append(template.caps());
        }
    }
    caps
}

#[cfg(test)]
#[path = "gst_dmabuf_watch_tests.rs"]
mod tests;
