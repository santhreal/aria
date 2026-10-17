//! A view whose content a plugin builds off the drawing thread: the machinery
//! declared panels and viewer overlays share.
//!
//! The plugin's code never runs on the thread that draws. Its content is built
//! on the event delivery thread when a host first reads the view, after each
//! event the view receives, and after its plugin asks for a refresh. A host
//! reads the last built content without waiting, and the waker it sets runs
//! whenever new content lands. A panic in the code fails its plugin for the
//! session, as a command's does.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::{Mutex, RwLock};

use crate::context::PluginContext;
use crate::events::panic_message;
use crate::plugin::PluginManifest;

/// One kind of live view: its spec, its code, what the code builds, and the
/// events the host sends back.
pub(crate) trait LiveKind: 'static {
    type Spec: Send + Sync + 'static;
    type View: ?Sized + Send + Sync + 'static;
    type Content: Send + Sync + 'static;
    type Event: Send + 'static;

    /// What one view is called in a fault: `panel`.
    const NOUN: &'static str;
    /// What its code returns, in a fault: `a tree`.
    const CONTENT: &'static str;

    fn id(spec: &Self::Spec) -> &str;
    fn build(view: &Self::View, ctx: &PluginContext) -> Self::Content;
    fn handle(view: &Self::View, ctx: &PluginContext, event: &Self::Event) -> Result<(), String>;
    /// Why the host will not draw `content`, if it will not.
    fn check(content: &Self::Content) -> Result<(), String>;
}

/// What a host draws for one view.
pub(crate) struct Snapshot<K: LiveKind> {
    pub(crate) spec: Arc<K::Spec>,
    pub(crate) content: Option<Arc<K::Content>>,
    pub(crate) refused: Option<Arc<str>>,
    pub(crate) event_error: Option<Arc<str>>,
}

/// An event waiting on the delivery thread. A later event that merges with it
/// replaces it while it waits, so a drag queues one move, not one per pointer
/// step.
type Slot<E> = Arc<Mutex<Option<E>>>;

/// The part of an entry the delivery thread writes.
struct Shown<K: LiveKind> {
    content: Option<Arc<K::Content>>,
    refused: Option<Arc<str>>,
    event_error: Option<Arc<str>>,
    /// A build is queued that has not started.
    queued: bool,
    /// A build has been queued at least once.
    started: bool,
    /// The slot of the last job queued, while it waits and may still merge.
    tail: Option<Slot<K::Event>>,
}

impl<K: LiveKind> Default for Shown<K> {
    fn default() -> Self {
        Self {
            content: None,
            refused: None,
            event_error: None,
            queued: false,
            started: false,
            tail: None,
        }
    }
}

/// One registered view.
pub(crate) struct Entry<K: LiveKind> {
    spec: Arc<K::Spec>,
    /// The registering plugin's manifest, or `None` for the host.
    scope: Option<Arc<PluginManifest>>,
    view: Arc<K::View>,
    shown: Mutex<Shown<K>>,
}

impl<K: LiveKind> Entry<K> {
    /// The content the view shows now, or `None` before its first build.
    pub(crate) fn content(&self) -> Option<Arc<K::Content>> {
        self.shown.lock().content.clone()
    }

    fn snapshot(&self, shown: &Shown<K>) -> Snapshot<K> {
        Snapshot {
            spec: Arc::clone(&self.spec),
            content: shown.content.clone(),
            refused: shown.refused.clone(),
            event_error: shown.event_error.clone(),
        }
    }
}

/// Why a call did not reach a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    Unknown,
    NotOwned,
}

/// The views of one kind registered on one context, in registration order.
pub(crate) struct Live<K: LiveKind> {
    entries: RwLock<Vec<Arc<Entry<K>>>>,
    /// Counts every change a host would draw differently.
    revision: AtomicU64,
    waker: RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl<K: LiveKind> Default for Live<K> {
    fn default() -> Self {
        Self {
            entries: RwLock::default(),
            revision: AtomicU64::new(0),
            waker: RwLock::default(),
        }
    }
}

/// Removes one view when its plugin's registration drops.
pub(crate) struct Unregister<K: LiveKind> {
    registry: Weak<Live<K>>,
    id: String,
}

impl<K: LiveKind> Drop for Unregister<K> {
    fn drop(&mut self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        // The view drops after the lock is released, so its drop code may read
        // the registry.
        let removed: Vec<_> = registry
            .entries
            .write()
            .extract_if(.., |entry| K::id(&entry.spec) == self.id)
            .collect();
        let any = !removed.is_empty();
        drop(removed);
        if any {
            registry.changed();
        }
    }
}

impl<K: LiveKind> Live<K> {
    fn find(&self, id: &str) -> Option<Arc<Entry<K>>> {
        self.entries
            .read()
            .iter()
            .find(|entry| K::id(&entry.spec) == id)
            .cloned()
    }

    /// Count a change and run the host's waker. Called with no lock held, so
    /// the waker may read the registry.
    pub(crate) fn changed(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
        let waker = self.waker.read().clone();
        if let Some(waker) = waker {
            waker();
        }
    }

    /// Add a view under `ctx`'s scope, returning what removes it, or `Err`
    /// when the id is taken. Nothing of `view` runs until a host first reads it.
    pub(crate) fn insert(
        self: &Arc<Self>,
        ctx: &PluginContext,
        spec: K::Spec,
        view: Arc<K::View>,
    ) -> Result<Unregister<K>, String> {
        let id = K::id(&spec).to_owned();
        {
            let mut entries = self.entries.write();
            if entries.iter().any(|entry| K::id(&entry.spec) == id) {
                return Err(format!("a {} is registered as {id}", K::NOUN));
            }
            entries.push(Arc::new(Entry {
                spec: Arc::new(spec),
                scope: ctx.scope(),
                view,
                shown: Mutex::default(),
            }));
        }
        self.changed();
        Ok(Unregister {
            registry: Arc::downgrade(self),
            id,
        })
    }

    /// Every registered spec, in registration order.
    pub(crate) fn specs(&self) -> Vec<Arc<K::Spec>> {
        self.entries
            .read()
            .iter()
            .map(|entry| Arc::clone(&entry.spec))
            .collect()
    }

    /// What `id` shows now. Never waits: the first read queues the first build,
    /// and until it lands the content is `None`.
    pub(crate) fn read(self: &Arc<Self>, ctx: &PluginContext, id: &str) -> Option<Snapshot<K>> {
        let entry = self.find(id)?;
        let (snapshot, first) = {
            let mut shown = entry.shown.lock();
            let first = !shown.started;
            if first {
                shown.started = true;
                shown.queued = true;
            }
            (entry.snapshot(&shown), first)
        };
        if first {
            self.queue(ctx, &entry, None);
        }
        Some(snapshot)
    }

    /// The view `id`, when `ctx` may drive it: the host's context drives every
    /// view, a plugin's view only its own.
    pub(crate) fn owned(&self, ctx: &PluginContext, id: &str) -> Result<Arc<Entry<K>>, Access> {
        let entry = self.find(id).ok_or(Access::Unknown)?;
        if let Some(caller) = ctx.plugin_scope() {
            let owner = entry.scope.as_ref().map(|manifest| &manifest.id);
            if owner != Some(&caller) {
                return Err(Access::NotOwned);
            }
        }
        Ok(entry)
    }

    /// Deliver `event` to `entry`, then build it again. With `merge`, an event
    /// still waiting from the last send that `merge` accepts is replaced
    /// instead of a second one queued.
    pub(crate) fn send(
        self: &Arc<Self>,
        ctx: &PluginContext,
        entry: &Arc<Entry<K>>,
        event: K::Event,
        merge: impl Fn(&K::Event) -> bool,
    ) {
        let slot = {
            let mut shown = entry.shown.lock();
            if let Some(tail) = &shown.tail {
                let mut waiting = tail.lock();
                if waiting.as_ref().is_some_and(&merge) {
                    *waiting = Some(event);
                    return;
                }
            }
            let slot: Slot<K::Event> = Arc::new(Mutex::new(Some(event)));
            shown.tail = Some(Arc::clone(&slot));
            slot
        };
        self.queue(ctx, entry, Some(slot));
    }

    /// Build `entry` again, for state that changed outside an event. Refreshes
    /// queued before the build starts share one build.
    pub(crate) fn refresh(self: &Arc<Self>, ctx: &PluginContext, entry: &Arc<Entry<K>>) {
        let queue = {
            let mut shown = entry.shown.lock();
            let queue = !shown.queued;
            shown.queued = true;
            shown.started = true;
            queue
        };
        if queue {
            self.queue(ctx, entry, None);
        }
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub(crate) fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        *self.waker.write() = Some(waker);
    }

    /// Run the event in `slot`, when there is one, and then build, on the
    /// delivery thread after every notification already posted.
    ///
    /// The job holds the entry weakly: a view unregistered before its job runs
    /// is not built.
    fn queue(
        self: &Arc<Self>,
        ctx: &PluginContext,
        entry: &Arc<Entry<K>>,
        slot: Option<Slot<K::Event>>,
    ) {
        let weak = Arc::downgrade(entry);
        let registry = Arc::downgrade(self);
        let host = ctx.scoped(None);
        ctx.events().defer(move || {
            if let (Some(entry), Some(registry)) = (weak.upgrade(), registry.upgrade()) {
                registry.run(&host, &entry, slot);
            }
        });
    }

    fn run(&self, host: &PluginContext, entry: &Entry<K>, slot: Option<Slot<K::Event>>) {
        let view = host.scoped(entry.scope.clone());
        let event = slot.and_then(|slot| {
            let mut shown = entry.shown.lock();
            if shown
                .tail
                .as_ref()
                .is_some_and(|tail| Arc::ptr_eq(tail, &slot))
            {
                shown.tail = None;
            }
            drop(shown);
            slot.lock().take()
        });
        if let Some(event) = event {
            let handled = catch_unwind(AssertUnwindSafe(|| K::handle(&entry.view, &view, &event)));
            match handled {
                Ok(result) => {
                    entry.shown.lock().event_error = result.err().map(Arc::from);
                }
                Err(payload) => {
                    let message = panic_message(payload.as_ref());
                    self.panicked(host, entry, "handling an event", &message);
                    return;
                }
            }
        }
        entry.shown.lock().queued = false;
        let built = catch_unwind(AssertUnwindSafe(|| K::build(&entry.view, &view)));
        let content = match built {
            Ok(content) => content,
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                self.panicked(host, entry, "drawing", &message);
                return;
            }
        };
        let checked = K::check(&content);
        {
            let mut shown = entry.shown.lock();
            match &checked {
                Ok(()) => {
                    shown.content = Some(Arc::new(content));
                    shown.refused = None;
                }
                Err(err) => shown.refused = Some(Arc::from(err.as_str())),
            }
        }
        if let (Err(err), Some(manifest)) = (&checked, &entry.scope) {
            host.report_fault(
                &manifest.id,
                format!(
                    "{} {} returned {} the host refused: {err}",
                    K::NOUN,
                    K::id(&entry.spec),
                    K::CONTENT
                ),
            );
        }
        self.changed();
    }

    /// A view's code panicked: fail its plugin, or, for a view the host
    /// registered or one whose plugin is not mounted yet, show the panic in
    /// the view.
    fn panicked(&self, host: &PluginContext, entry: &Entry<K>, doing: &str, message: &str) {
        let reason = format!(
            "{} {} panicked while {doing}: {message}",
            K::NOUN,
            K::id(&entry.spec)
        );
        let failed = entry
            .scope
            .as_ref()
            .is_some_and(|manifest| host.fail(&manifest.id, reason.clone()));
        if !failed {
            let mut shown = entry.shown.lock();
            shown.queued = false;
            shown.refused = Some(Arc::from(reason));
        }
        self.changed();
    }
}
