//! Device contexts shared across every decode pipeline in the process.
//!
//! A hardware decoder opens its device through a `gst::Context`
//! (`gst.cuda.context` for NVDEC, a VA display for VA-API). With nothing set on
//! the pipeline, each new pipeline creates its own: on an RTX 5090 with driver
//! 580 that is about 1.5 s for the first `nvh264dec` in a process and 130 ms
//! for each one after it. The first context an element posts with
//! `HAVE_CONTEXT` is kept here and set on every later pipeline before it
//! leaves `NULL`, so a `GstBin` hands it to the decoder `decodebin` plugs, and
//! the device opens once per process.
//!
//! [`warm_up`] opens those devices on a thread of its own before the editor's
//! window opens, so the first decoder finds its context kept: on the RTX 5090 the
//! first `nvh264dec` open takes about 1.7 s cold and 50 to 65 ms after the
//! warm-up. A decoder open waits for a warm-up still running rather than
//! opening a second device beside it.
//!
//! A process that ends through `exit` stops the warm-up at its next step and
//! waits for the step in progress, so no plugin loads while the C library
//! runs the destructors of the process's libraries: the warm-up is a
//! [`OnceJob`].

use parking_lot::Mutex;

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::gst_job::OnceJob;

/// Contexts kept so far, one per context type, in the order they arrived.
static KEPT: Mutex<Vec<gst::Context>> = Mutex::new(Vec::new());

/// Set every kept context on `pipeline`, and keep any new context type one of
/// its elements creates. Call before the first state change.
pub(crate) fn share(pipeline: &gst::Pipeline) {
    for context in KEPT.lock().iter() {
        pipeline.set_context(context);
    }
    if let Some(bus) = pipeline.bus() {
        bus.set_sync_handler(|_, message| {
            if let gst::MessageView::HaveContext(have) = message.view() {
                keep(have.context());
            }
            gst::BusSyncReply::Pass
        });
    }
}

/// Keep `context` unless one of its type is already kept. Returns whether it
/// was kept.
fn keep(context: gst::Context) -> bool {
    keep_in(&KEPT, context)
}

/// Keep `context` in `store` unless one of its type is already there.
fn keep_in(store: &Mutex<Vec<gst::Context>>, context: gst::Context) -> bool {
    let mut kept = store.lock();
    if kept
        .iter()
        .any(|held| held.context_type() == context.context_type())
    {
        return false;
    }
    kept.push(context);
    true
}

/// Moves `element`, a lone element outside any pipeline, to `READY` on the
/// device contexts this process keeps, and returns whether it got there.
///
/// A lone element has no bus to post `NEED_CONTEXT` on, so `share` cannot
/// reach it: each kept context is set on it before it leaves `NULL`, and each
/// context type it holds once open that is not kept yet is kept for every
/// later pipeline. A warm-up still running is waited for first, as a decoder
/// open waits, so the element does not open a second device beside it. On an
/// RTX 5090 `nvh264enc` opens in about 5 ms on the kept CUDA context and in
/// about 68 ms on one of its own.
pub fn open_on_kept_contexts(element: &gst::Element) -> bool {
    WARM_UP.wait();
    open_on(element, &KEPT)
}

/// [`open_on_kept_contexts`] over the contexts in `store`.
fn open_on(element: &gst::Element, store: &Mutex<Vec<gst::Context>>) -> bool {
    let kept: Vec<gst::Context> = store.lock().clone();
    for context in &kept {
        element.set_context(context);
    }
    if element.set_state(gst::State::Ready).is_err() {
        return false;
    }
    for context in element.contexts() {
        keep_in(store, context);
    }
    true
}

/// The pipeline that opens one hardware decode device and nothing else, for
/// each element that opens it.
const DEVICES: &[(&str, &str)] = &[(
    "cudaupload",
    "videotestsrc num-buffers=1 ! cudaupload ! fakesink",
)];

/// How long one device warm-up may take before it is abandoned, in seconds.
pub(crate) const DEVICE_WAIT_SECS: u64 = 10;

/// How long one device warm-up may take before it is abandoned.
const DEVICE_WAIT: gst::ClockTime = gst::ClockTime::from_seconds(DEVICE_WAIT_SECS);

static WARM_UP: OnceJob = OnceJob::new();

/// Open this host's hardware decode devices on a thread of their own and keep
/// their contexts for every later decode pipeline. Returns at once; only the
/// first call in a process starts the warm-up.
pub fn warm_up() {
    if let Err(err) = crate::gst::ensure_init() {
        tracing::warn!(%err, "GStreamer did not initialize; the decode device warm-up is skipped");
        return;
    }
    WARM_UP.start("aria-decode-warm-up", warm_devices);
}

/// Block until a running [`warm_up`] finishes. Returns at once when none was
/// started.
pub fn await_warm_up() {
    WARM_UP.wait();
}

/// Demote any device plugins recorded as unloadable on this boot before
/// autoplugging or scanning the registry for hardware decoders.
pub(crate) fn demote_recorded_unloadable() {
    static DEMOTED: std::sync::Once = std::sync::Once::new();
    DEMOTED.call_once(|| {
        let records = crate::gst_unloadable::user_records();
        for (element, _) in DEVICES {
            if let Some(factory) = gst::ElementFactory::find(element)
                && let Some(stamp) = crate::gst_unloadable::Stamp::of(&factory)
                && crate::gst_unloadable::failed_before(&records, &stamp)
            {
                crate::gst_unloadable::demote(&stamp);
            }
        }
    });
}

/// Core decode pipeline elements pre-loaded during [`warm_up`] so the first
/// file open does not block on plugin `.so` mapping, GType class init, or
/// `libavcodec` decoder registration.
const CORE_DECODE_FACTORIES: &[&str] = &[
    "uridecodebin",
    "decodebin",
    "filesrc",
    "typefind",
    "qtdemux",
    "multiqueue",
    "h264parse",
    "h265parse",
    "avdec_h264",
    "avdec_h265",
    "avdec_prores",
    "videoconvert",
    "videoscale",
    "capsfilter",
    "appsink",
];

/// Open each device in [`DEVICES`] whose element this host has and can load.
///
/// The registry can list an element whose plugin no longer loads (a CUDA
/// plugin cached before the driver went away). Launching it would build the
/// pipeline from an unloaded factory and log GStreamer criticals, so the
/// factory is loaded first and the device skipped with the cause. The failure
/// is recorded (see [`crate::gst_unloadable`]): a later process on the same
/// boot loads neither the plugin nor, through autoplugging, its decoders.
fn warm_devices() {
    if let Err(err) = crate::gst::ensure_init() {
        tracing::warn!(%err, "GStreamer did not initialize; the decode device warm-up is skipped");
        return;
    }
    let records = crate::gst_unloadable::user_records();
    for (element, launch) in DEVICES {
        if WARM_UP.stopping() {
            return;
        }
        let Some(factory) = gst::ElementFactory::find(element) else {
            continue;
        };
        let stamp = crate::gst_unloadable::Stamp::of(&factory);
        if let Some(stamp) = &stamp
            && crate::gst_unloadable::failed_before(&records, stamp)
        {
            crate::gst_unloadable::demote(stamp);
            tracing::info!(
                element,
                "the decode device plugin failed to load earlier on this boot; no device warm-up"
            );
            continue;
        }
        let loaded = factory.load();
        if let Some(stamp) = &stamp {
            if loaded.is_err() {
                crate::gst_unloadable::demote(stamp);
            }
            if let Err(err) = crate::gst_unloadable::record(&records, stamp, loaded.is_err()) {
                tracing::debug!(%err, path = %records.display(), "the unloadable plugin record was not written");
            }
        }
        if let Err(err) = loaded {
            tracing::info!(element, %err, "the decode device element is registered but does not load; no device warm-up");
            continue;
        }
        if let Err(cause) = warm_device(launch) {
            tracing::warn!(element, %cause, "the decode device warm-up failed; the first decoder opens its own device");
        }
    }
    if WARM_UP.stopping() {
        return;
    }
    let _ = crate::gst_dmabuf_watch::registry_has_dmabuf_video_decoder();
    for name in CORE_DECODE_FACTORIES {
        if WARM_UP.stopping() {
            return;
        }
        let _ = gst::ElementFactory::make(name).build();
    }
}

/// Run `launch` to `PAUSED` with context sharing, then stop it.
fn warm_device(launch: &str) -> Result<(), String> {
    let pipeline = gst::parse::launch(launch)
        .map_err(|err| err.to_string())?
        .downcast::<gst::Pipeline>()
        .map_err(|_| "the warm-up description is not a pipeline".to_owned())?;
    share(&pipeline);
    let changed = pipeline
        .set_state(gst::State::Paused)
        .and_then(|_| pipeline.state(DEVICE_WAIT).0)
        .map_err(|err| {
            crate::gst::posted_error(&pipeline)
                .map_or_else(|| err.to_string(), |posted| posted.to_string())
        });
    let _ = pipeline.set_state(gst::State::Null);
    match changed {
        Ok(gst::StateChangeSuccess::Async) => {
            Err(format!("not prerolled after {} s", DEVICE_WAIT.seconds()))
        }
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    }
}

/// The context types kept so far.
#[cfg(test)]
pub(crate) fn kept_types() -> Vec<String> {
    KEPT.lock()
        .iter()
        .map(|context| context.context_type().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A context posted on one pipeline's bus is set on the next pipeline,
    /// and a second context of the same type does not replace it. Closes the
    /// class where every decode pipeline opened its own hardware device. Does
    /// not prove a hardware decoder reads the context; that needs the device.
    #[test]
    fn a_posted_context_reaches_the_next_pipeline() {
        crate::gst::ensure_init().expect("GStreamer initializes");
        let ty = "aria.test.shared-context";

        let first = gst::Pipeline::new();
        share(&first);
        let context = gst::Context::new(ty, true);
        let element = gst::ElementFactory::make("fakesink")
            .build()
            .expect("fakesink");
        first.add(&element).expect("add fakesink");
        let posted = gst::message::HaveContext::builder(context)
            .src(&element)
            .build();
        element.post_message(posted).expect("post HAVE_CONTEXT");
        assert!(
            kept_types().iter().any(|kept| kept == ty),
            "the posted context was not kept: {:?}",
            kept_types()
        );

        let second = gst::Pipeline::new();
        share(&second);
        assert!(
            second.context(ty).is_some(),
            "the kept context was not set on the next pipeline"
        );

        assert!(
            !keep(gst::Context::new(ty, true)),
            "a second context of a kept type replaced the first"
        );
    }

    /// WHY: the warm-up runs in every session, before any decoder, on hosts
    /// whose registry lists a device element that no longer loads (a CUDA
    /// plugin cached before the driver went away). Launching that element
    /// logged six GStreamer and GObject criticals per process. Holds on every
    /// host that the warm-up logs no GStreamer critical, whether the device
    /// element is absent, unloadable, or opens a device; it goes red only on a
    /// host with an unloadable entry, such as the build server.
    #[test]
    fn the_device_warm_up_logs_no_gstreamer_critical() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};

        crate::gst::ensure_init().expect("GStreamer initializes");
        let criticals = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&criticals);
        let handler = gst::glib::log_set_handler(
            Some("GStreamer"),
            gst::glib::LogLevels::LEVEL_CRITICAL,
            false,
            false,
            move |_, _, _| {
                count.fetch_add(1, Ordering::Relaxed);
            },
        );
        warm_devices();
        gst::glib::log_remove_handler(Some("GStreamer"), handler);
        assert_eq!(criticals.load(Ordering::Relaxed), 0);
    }

    /// A lone element opens on every context in the store, and the store
    /// keeps each context type the element holds once open that it lacked,
    /// without replacing one it has. Runs on every host: a `fakesink` holds
    /// the contexts set on it as a device element holds the one it creates.
    #[test]
    fn a_lone_element_opens_on_the_store_and_lends_what_it_holds() {
        crate::gst::ensure_init().expect("GStreamer initializes");
        let shared = gst::Context::new("aria.test.lone-shared", true);
        let own = gst::Context::new("aria.test.lone-own", true);
        let store = Mutex::new(vec![shared]);
        let element = gst::ElementFactory::make("fakesink")
            .build()
            .expect("fakesink");
        element.set_context(&own);
        element.set_context(&gst::Context::new("aria.test.lone-shared", false));

        assert!(open_on(&element, &store), "fakesink did not reach READY");
        let held: Vec<String> = element
            .contexts()
            .iter()
            .map(|c| format!("{} {}", c.context_type(), c.is_persistent()))
            .collect();
        let _ = element.set_state(gst::State::Null);
        assert!(
            held.iter().any(|c| c == "aria.test.lone-shared true"),
            "the element did not open on the kept context: {held:?}"
        );
        let kept: Vec<String> = store
            .lock()
            .iter()
            .map(|c| format!("{} {}", c.context_type(), c.is_persistent()))
            .collect();
        assert_eq!(
            kept,
            ["aria.test.lone-shared true", "aria.test.lone-own true"]
        );
    }

    /// WHY: an encoder opens alone, outside any pipeline, so it had no bus
    /// to ask for a context on and made a CUDA context of its own: about
    /// 68 ms of each NVENC open on an RTX 5090 against 5 ms on a kept one,
    /// and a second device context in the process. Holds that a lone element
    /// opened with nothing kept lends each context it holds to the store, and
    /// that the next one opens on those contexts. Checks nothing on a host
    /// with no NVENC element that loads and opens.
    #[test]
    fn a_lone_element_opens_on_the_kept_context_and_lends_its_own() {
        crate::gst::ensure_init().expect("GStreamer initializes");
        let Some(name) = ["nvh264enc", "nvh265enc"].into_iter().find(|name| {
            gst::ElementFactory::find(name).is_some_and(|factory| factory.load().is_ok())
        }) else {
            return;
        };
        let lone = || {
            gst::ElementFactory::make(name)
                .build()
                .expect("build the encoder")
        };
        let store = Mutex::new(Vec::new());
        let first = lone();
        let opened = open_on(&first, &store);
        let held = first.contexts();
        let _ = first.set_state(gst::State::Null);
        if !opened {
            return;
        }
        let kept = store.lock().clone();
        assert!(!held.is_empty(), "{name} opened on no context");
        assert_eq!(
            kept.iter().map(|c| c.context_type()).collect::<Vec<_>>(),
            held.iter().map(|c| c.context_type()).collect::<Vec<_>>(),
            "the store did not keep what {name} held"
        );

        let second = lone();
        assert!(open_on(&second, &store), "{name} did not open again");
        let reused = second.contexts();
        let _ = second.set_state(gst::State::Null);
        for context in &kept {
            assert!(
                reused
                    .iter()
                    .any(|again| again.structure() == context.structure()),
                "{name} opened on a new {} context",
                context.context_type()
            );
        }
    }
}
