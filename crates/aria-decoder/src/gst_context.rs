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
//! [`warm_up`] opens those devices on a thread of its own when the session
//! starts, so the first decoder finds its context kept: on the RTX 5090 the
//! first `nvh264dec` open takes about 1.7 s cold and 50 to 65 ms after the
//! warm-up. A decoder open waits for a warm-up still running rather than
//! opening a second device beside it.

use std::sync::{Condvar, Mutex, PoisonError};

use gstreamer as gst;
use gstreamer::prelude::*;

/// Contexts kept so far, one per context type, in the order they arrived.
static KEPT: Mutex<Vec<gst::Context>> = Mutex::new(Vec::new());

/// Set every kept context on `pipeline`, and keep any new context type one of
/// its elements creates. Call before the first state change.
pub(crate) fn share(pipeline: &gst::Pipeline) {
    for context in KEPT.lock().unwrap_or_else(PoisonError::into_inner).iter() {
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
    let mut kept = store.lock().unwrap_or_else(PoisonError::into_inner);
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
    let kept: Vec<gst::Context> = store.lock().unwrap_or_else(PoisonError::into_inner).clone();
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

/// How long one device warm-up may take before it is abandoned.
const DEVICE_WAIT: gst::ClockTime = gst::ClockTime::from_seconds(10);

/// Where a [`Once`] job is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Stage {
    Idle,
    Running,
    Finished,
}

/// A job that runs at most once per process, on a thread of its own, and can
/// be waited for.
pub(crate) struct Once {
    stage: Mutex<Stage>,
    finished: Condvar,
}

impl Once {
    pub(crate) const fn new() -> Self {
        Self {
            stage: Mutex::new(Stage::Idle),
            finished: Condvar::new(),
        }
    }

    /// Run `job` on a thread named `name` unless a job was started before.
    /// Returns whether this call started it.
    pub(crate) fn start(&'static self, name: &str, job: impl FnOnce() + Send + 'static) -> bool {
        {
            let mut stage = self.stage.lock().unwrap_or_else(PoisonError::into_inner);
            if *stage != Stage::Idle {
                return false;
            }
            *stage = Stage::Running;
        }
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                // Finishes even when `job` panics, so no waiter blocks forever.
                let _finish = Finish(self);
                job();
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "the device warm-up thread did not start; decoders open their own devices");
            self.finish();
        }
        true
    }

    fn finish(&self) {
        *self.stage.lock().unwrap_or_else(PoisonError::into_inner) = Stage::Finished;
        self.finished.notify_all();
    }

    /// Block until a started job finishes; return at once when none runs.
    pub(crate) fn wait(&self) {
        let stage = self.stage.lock().unwrap_or_else(PoisonError::into_inner);
        let _finished = self
            .finished
            .wait_while(stage, |stage| *stage == Stage::Running)
            .unwrap_or_else(PoisonError::into_inner);
    }

    #[cfg(test)]
    pub(crate) fn stage(&self) -> Stage {
        *self.stage.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Marks its [`Once`] finished when dropped.
struct Finish(&'static Once);

impl Drop for Finish {
    fn drop(&mut self) {
        self.0.finish();
    }
}

static WARM_UP: Once = Once::new();

/// Open this host's hardware decode devices on a thread of their own and keep
/// their contexts for every later decode pipeline. Returns at once; only the
/// first call in a process starts the warm-up.
pub fn warm_up() {
    if let Err(err) = crate::gst::ensure_init() {
        tracing::warn!(%err, "GStreamer did not initialise; the decode device warm-up is skipped");
        return;
    }
    WARM_UP.start("aria-decode-warm-up", warm_devices);
}

/// Block until a running [`warm_up`] finishes. Returns at once when none was
/// started.
pub fn await_warm_up() {
    WARM_UP.wait();
}

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
        tracing::warn!(%err, "GStreamer did not initialise; the decode device warm-up is skipped");
        return;
    }
    let records = crate::gst_unloadable::user_records();
    for (element, launch) in DEVICES {
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
        .map_err(|err| err.to_string())
        .and_then(|_| pipeline.state(DEVICE_WAIT).0.map_err(|err| err.to_string()));
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
        .unwrap_or_else(PoisonError::into_inner)
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
        crate::gst::ensure_init().expect("GStreamer initialises");
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

    /// WHY: a decoder opened while the device warm-up runs must wait for it
    /// rather than open a second device beside it, and must not wait forever
    /// when the warm-up job panics. Covers start-once, the wait, and the panic
    /// path; does not cover the CUDA device itself, which the build host lacks.
    #[test]
    fn a_waiter_blocks_until_the_one_job_finishes_even_when_it_panics() {
        use std::sync::mpsc;
        use std::time::Duration;

        static JOB: Once = Once::new();
        JOB.wait();
        assert_eq!(JOB.stage(), Stage::Idle, "waiting started nothing");

        let (release, released) = mpsc::channel::<()>();
        let (ran, runs) = mpsc::channel::<()>();
        let started = JOB.start("aria-test-once", move || {
            let _ = ran.send(());
            let _ = released.recv();
            panic!("the job panics after its release");
        });
        assert!(started, "the first start did not run the job");
        assert!(
            !JOB.start("aria-test-once", || unreachable!("a second job ran")),
            "a second start was accepted"
        );
        runs.recv_timeout(Duration::from_secs(5))
            .expect("the job runs");

        let (done, waited) = mpsc::channel::<()>();
        let waiter = std::thread::spawn(move || {
            JOB.wait();
            let _ = done.send(());
        });
        assert!(
            waited.recv_timeout(Duration::from_millis(200)).is_err(),
            "the wait returned while the job ran"
        );
        release.send(()).expect("release the job");
        waited
            .recv_timeout(Duration::from_secs(5))
            .expect("the wait ends once the job panics");
        waiter.join().expect("waiter");
        assert_eq!(JOB.stage(), Stage::Finished);
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

        crate::gst::ensure_init().expect("GStreamer initialises");
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
        crate::gst::ensure_init().expect("GStreamer initialises");
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
            .unwrap_or_else(PoisonError::into_inner)
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
        crate::gst::ensure_init().expect("GStreamer initialises");
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
        let kept = store.lock().unwrap_or_else(PoisonError::into_inner).clone();
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
