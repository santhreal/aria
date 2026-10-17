//! Tests for declared panels.
//!
//! Closes the class "a panel's code runs where it should not, outlives its
//! plugin, or receives input its tree could not produce": every case goes
//! through `register_panel`, `panel`, `send_panel_event` and `refresh_panel`,
//! the entry points every host and plugin use. It does not cover how a host
//! draws a tree; the panel tests in `aria-ui` own that.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc;
use std::time::Duration;

use parking_lot::Mutex;

use super::*;
use crate::plugin::{Plugin, PluginManifest};
use crate::ui_tree::TreeError;

const SETTLE: Duration = Duration::from_secs(10);
const ID: &str = "test.panel.counter";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[default]
    Normal,
    PanicDraw,
    PanicEvent,
    Refuse,
    Fail,
}

/// What a test panel did, and how it behaves next.
#[derive(Default)]
struct Probe {
    count: AtomicU32,
    views: AtomicUsize,
    events: AtomicUsize,
    mode: Mutex<Mode>,
}

impl Probe {
    fn set(&self, mode: Mode) {
        *self.mode.lock() = mode;
    }
}

/// A counter: a line of text and a button that adds one.
struct Counter(Arc<Probe>);

impl PanelView for Counter {
    fn view(&self, _: &PluginContext) -> UiNode {
        self.0.views.fetch_add(1, SeqCst);
        match *self.0.mode.lock() {
            Mode::PanicDraw => panic!("no tree"),
            Mode::Refuse => {
                return UiNode::row([UiNode::button("same", "A"), UiNode::button("same", "B")]);
            }
            _ => {}
        }
        UiNode::column([
            UiNode::text(format!("count {}", self.0.count.load(SeqCst))),
            UiNode::button("add", "Add"),
        ])
    }

    fn event(&self, _: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        self.0.events.fetch_add(1, SeqCst);
        match *self.0.mode.lock() {
            Mode::PanicEvent => panic!("no event"),
            Mode::Fail => return Err("not now".into()),
            _ => {}
        }
        if let PanelEvent::Pressed { .. } = event {
            self.0.count.fetch_add(1, SeqCst);
        }
        Ok(())
    }
}

/// A plugin that docks one counter panel.
struct WithPanel {
    manifest: PluginManifest,
    panel: &'static str,
    probe: Arc<Probe>,
}

impl Plugin for WithPanel {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_panel(
            spec(self.panel, PanelLocation::Sidebar),
            Counter(Arc::clone(&self.probe)),
        )?;
        Ok(())
    }
}

fn spec(id: &str, location: PanelLocation) -> PanelSpec {
    PanelSpec {
        id: id.to_owned(),
        title: "Counter".into(),
        location,
    }
}

fn with_panel(plugin: &'static str, panel: &'static str) -> WithPanel {
    WithPanel {
        manifest: PluginManifest::new(plugin, plugin, "0.1.0", &[], &[]),
        panel,
        probe: Arc::default(),
    }
}

/// A context with the counter's plugin mounted and nothing read yet.
fn mounted() -> (PluginContext, Arc<Probe>) {
    let ctx = PluginContext::new();
    let plugin = with_panel("test.panel", ID);
    ctx.mount(&plugin).expect("mounts");
    (ctx, plugin.probe)
}

/// A context whose counter has drawn its first tree.
fn drawn() -> (PluginContext, Arc<Probe>) {
    let (ctx, probe) = mounted();
    let _ = ctx.panel(ID);
    settle(&ctx);
    assert!(state(&ctx).tree.is_some(), "the first build did not land");
    (ctx, probe)
}

fn settle(ctx: &PluginContext) {
    assert!(
        ctx.events().flush(SETTLE),
        "the delivery thread did not settle"
    );
}

fn state(ctx: &PluginContext) -> PanelState {
    ctx.panel(ID).expect("the counter is registered")
}

/// The counter's text line.
fn count(ctx: &PluginContext) -> String {
    let tree = state(ctx).tree.expect("a tree");
    match &*tree {
        UiNode::Column { children } => match children.first() {
            Some(UiNode::Text { text, .. }) => text.to_string(),
            other => panic!("the counter's first row is {other:?}"),
        },
        other => panic!("the counter draws {other:?}"),
    }
}

/// Park the delivery thread until the returned sender drops, so a test can
/// queue work behind it and observe what was queued.
fn hold(ctx: &PluginContext) -> mpsc::Sender<()> {
    let (release, wait) = mpsc::channel::<()>();
    let (took, taken) = mpsc::channel();
    ctx.events().defer(move || {
        let _ = took.send(());
        let _ = wait.recv();
    });
    taken
        .recv_timeout(SETTLE)
        .expect("the delivery thread took the hold");
    release
}

fn press() -> PanelEvent {
    PanelEvent::Pressed { id: "add".into() }
}

#[test]
fn the_first_read_queues_the_first_build_without_waiting() {
    let (ctx, probe) = mounted();
    settle(&ctx);
    assert_eq!(probe.views.load(SeqCst), 0, "a panel built before any read");

    let release = hold(&ctx);
    let first = state(&ctx);
    assert!(first.tree.is_none(), "the first read waited for the build");
    assert_eq!(&*first.spec.title, "Counter");
    drop(release);

    settle(&ctx);
    assert_eq!(count(&ctx), "count 0");
    assert_eq!(probe.views.load(SeqCst), 1);
}

#[test]
fn an_event_reaches_the_plugin_and_the_tree_is_built_again() {
    let (ctx, probe) = drawn();
    ctx.send_panel_event(ID, press()).expect("queued");
    ctx.send_panel_event(ID, press()).expect("queued");
    settle(&ctx);
    assert_eq!(count(&ctx), "count 2");
    assert_eq!(probe.events.load(SeqCst), 2);
}

#[test]
fn a_handlers_error_shows_until_it_accepts_the_next_event() {
    let (ctx, probe) = drawn();
    probe.set(Mode::Fail);
    ctx.send_panel_event(ID, press()).expect("queued");
    settle(&ctx);
    assert_eq!(state(&ctx).event_error.as_deref(), Some("not now"));
    assert_eq!(count(&ctx), "count 0");

    // A refresh is not an event: the error stays.
    ctx.refresh_panel(ID).expect("refreshes");
    settle(&ctx);
    assert_eq!(state(&ctx).event_error.as_deref(), Some("not now"));

    probe.set(Mode::Normal);
    ctx.send_panel_event(ID, press()).expect("queued");
    settle(&ctx);
    assert_eq!(state(&ctx).event_error, None);
    assert_eq!(count(&ctx), "count 1");
}

#[test]
fn events_the_tree_cannot_produce_never_reach_the_plugin() {
    let (ctx, probe) = drawn();
    let cases = [
        (
            PanelEvent::Pressed {
                id: "missing".into(),
            },
            EventMismatch::NoControl("missing".into()),
        ),
        (
            PanelEvent::Toggled {
                id: "add".into(),
                on: true,
            },
            EventMismatch::Wrong("add".into()),
        ),
        (
            PanelEvent::Chose {
                id: "add".into(),
                index: 0,
            },
            EventMismatch::Wrong("add".into()),
        ),
    ];
    for (event, mismatch) in cases {
        assert_eq!(
            ctx.send_panel_event(ID, event),
            Err(PanelError::Mismatch {
                panel: ID.into(),
                mismatch,
            })
        );
    }
    assert_eq!(
        ctx.send_panel_event("test.panel.none", press()),
        Err(PanelError::Unknown("test.panel.none".into()))
    );
    settle(&ctx);
    assert_eq!(probe.events.load(SeqCst), 0);
}

#[test]
fn an_event_before_the_first_tree_is_refused() {
    let (ctx, probe) = mounted();
    assert_eq!(
        ctx.send_panel_event(ID, press()),
        Err(PanelError::NotDrawn(ID.into()))
    );
    settle(&ctx);
    assert_eq!(probe.events.load(SeqCst), 0);
}

#[test]
fn refreshes_queued_before_a_build_starts_share_it() {
    let (ctx, probe) = drawn();
    let before = probe.views.load(SeqCst);
    let release = hold(&ctx);
    for _ in 0..3 {
        ctx.refresh_panel(ID).expect("refreshes");
    }
    drop(release);
    settle(&ctx);
    assert_eq!(probe.views.load(SeqCst), before + 1);

    // Once that build has started, the next refresh queues another.
    ctx.refresh_panel(ID).expect("refreshes");
    settle(&ctx);
    assert_eq!(probe.views.load(SeqCst), before + 2);
}

#[test]
fn a_plugin_drives_only_its_own_panels() {
    let ctx = PluginContext::new();
    let mine = with_panel("test.mine", "test.mine.panel");
    let theirs = with_panel("test.theirs", "test.theirs.panel");
    ctx.mount(&mine).expect("mounts");
    ctx.mount(&theirs).expect("mounts");
    let _ = ctx.panel("test.mine.panel");
    let _ = ctx.panel("test.theirs.panel");
    settle(&ctx);

    let view = ctx.view(&mine.manifest);
    let foreign = "test.theirs.panel";
    assert_eq!(
        view.refresh_panel(foreign),
        Err(PanelError::NotOwned(foreign.into()))
    );
    assert_eq!(
        view.send_panel_event(foreign, press()),
        Err(PanelError::NotOwned(foreign.into()))
    );
    assert_eq!(view.refresh_panel("test.mine.panel"), Ok(()));
    assert_eq!(view.send_panel_event("test.mine.panel", press()), Ok(()));
    // The host drives every panel.
    assert_eq!(ctx.send_panel_event(foreign, press()), Ok(()));
    settle(&ctx);
    assert_eq!(mine.probe.count.load(SeqCst), 1);
    assert_eq!(theirs.probe.count.load(SeqCst), 1);
}

#[test]
fn a_panel_that_panics_fails_its_plugin() {
    for (mode, doing, message) in [
        (Mode::PanicDraw, "drawing", "no tree"),
        (Mode::PanicEvent, "handling an event", "no event"),
    ] {
        let (ctx, probe) = drawn();
        probe.set(mode);
        if mode == Mode::PanicEvent {
            ctx.send_panel_event(ID, press()).expect("queued");
        } else {
            ctx.refresh_panel(ID).expect("refreshes");
        }
        settle(&ctx);
        assert!(!ctx.is_mounted("test.panel"), "{doing}");
        assert!(ctx.panels().is_empty(), "the failed plugin's panel stays");
        let failures = ctx.failures();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(
            failures[0].reason,
            format!("panel {ID} panicked while {doing}: {message}")
        );
    }
}

#[test]
fn a_host_panel_that_panics_shows_the_panic_and_stays() {
    let ctx = PluginContext::new();
    let probe = Arc::new(Probe::default());
    probe.set(Mode::PanicDraw);
    let _panel = ctx
        .register_panel(
            spec(ID, PanelLocation::Inspector),
            Counter(Arc::clone(&probe)),
        )
        .expect("registers");
    let _ = ctx.panel(ID);
    settle(&ctx);
    let shown = state(&ctx);
    assert!(shown.tree.is_none());
    assert_eq!(
        shown.refused.as_deref(),
        Some(format!("panel {ID} panicked while drawing: no tree").as_str())
    );
    assert!(ctx.failures().is_empty());

    // A later refresh that draws clears it.
    probe.set(Mode::Normal);
    ctx.refresh_panel(ID).expect("refreshes");
    settle(&ctx);
    assert_eq!(state(&ctx).refused, None);
    assert_eq!(count(&ctx), "count 0");
}

#[test]
fn a_refused_tree_keeps_the_last_good_one_and_records_a_fault() {
    let (ctx, probe) = drawn();
    probe.set(Mode::Refuse);
    ctx.refresh_panel(ID).expect("refreshes");
    settle(&ctx);

    let reason = TreeError::DuplicateId("same".into()).to_string();
    let shown = state(&ctx);
    assert_eq!(shown.refused.as_deref(), Some(reason.as_str()));
    assert_eq!(count(&ctx), "count 0", "the last good tree was dropped");
    assert!(
        ctx.is_mounted("test.panel"),
        "a refused tree failed its plugin"
    );
    let faults = ctx.faults();
    assert_eq!(faults.len(), 1, "{faults:?}");
    assert_eq!(
        faults[0].message,
        format!("panel {ID} returned a tree the host refused: {reason}")
    );

    probe.set(Mode::Normal);
    ctx.refresh_panel(ID).expect("refreshes");
    settle(&ctx);
    assert_eq!(state(&ctx).refused, None);
}

#[test]
fn a_panel_unregistered_before_its_build_runs_is_not_built() {
    let (ctx, probe) = mounted();
    let release = hold(&ctx);
    let _ = ctx.panel(ID);
    ctx.unmount("test.panel").expect("unmounts");
    drop(release);
    settle(&ctx);
    assert_eq!(probe.views.load(SeqCst), 0);
    assert!(ctx.panel(ID).is_none());
}

/// Every change a host would draw differently grows the revision once and
/// wakes the host once: registration, each build, and the panel leaving.
#[test]
fn the_host_is_woken_once_for_every_change_it_would_draw() {
    let ctx = PluginContext::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&wakes);
    ctx.set_panel_waker(move || {
        counted.fetch_add(1, SeqCst);
    });
    let start = ctx.panel_revision();

    let plugin = with_panel("test.panel", ID);
    ctx.mount(&plugin).expect("mounts");
    assert_eq!((wakes.load(SeqCst), ctx.panel_revision()), (1, start + 1));

    let _ = ctx.panel(ID);
    settle(&ctx);
    assert_eq!((wakes.load(SeqCst), ctx.panel_revision()), (2, start + 2));

    ctx.send_panel_event(ID, press()).expect("queued");
    settle(&ctx);
    assert_eq!((wakes.load(SeqCst), ctx.panel_revision()), (3, start + 3));

    ctx.unmount("test.panel").expect("unmounts");
    assert_eq!((wakes.load(SeqCst), ctx.panel_revision()), (4, start + 4));
    assert!(ctx.panels().is_empty());
}

/// Every location, so a new one turns this file red until someone decides
/// whether a declared panel may dock there.
const LOCATIONS: [PanelLocation; 6] = [
    PanelLocation::Inspector,
    PanelLocation::Sidebar,
    PanelLocation::Overlay,
    PanelLocation::Timeline,
    PanelLocation::Viewer,
    PanelLocation::Home,
];

/// Fails to compile when `PanelLocation` grows a variant `LOCATIONS` lacks.
fn listed(location: PanelLocation) -> bool {
    match location {
        PanelLocation::Inspector
        | PanelLocation::Sidebar
        | PanelLocation::Overlay
        | PanelLocation::Timeline
        | PanelLocation::Viewer
        | PanelLocation::Home => LOCATIONS.contains(&location),
    }
}

#[test]
fn a_panel_docks_only_in_a_stacked_pane() {
    for location in LOCATIONS {
        assert!(listed(location));
        let ctx = PluginContext::new();
        let result = ctx.register_panel(spec(ID, location), Counter(Arc::default()));
        if DOCKED_LOCATIONS.contains(&location) {
            assert!(result.is_ok(), "{location:?}");
            assert_eq!(ctx.panels().len(), 1);
        } else {
            let Err(PluginError::PanelRefused(reason)) = result else {
                panic!("{location:?} was not refused");
            };
            assert_eq!(
                reason,
                format!(
                    "{ID}: a declared panel docks in the sidebar, the inspector, or the timeline area, not {location:?}"
                )
            );
            assert!(ctx.panels().is_empty());
        }
    }
}

#[test]
fn malformed_and_duplicate_panels_are_refused() {
    let ctx = PluginContext::new();
    let cases = [
        (spec(" ", PanelLocation::Sidebar), "a panel needs an id"),
        (
            PanelSpec {
                id: ID.into(),
                title: "".into(),
                location: PanelLocation::Sidebar,
            },
            "test.panel.counter: a panel needs a title",
        ),
    ];
    for (spec, reason) in cases {
        match ctx.register_panel(spec, Counter(Arc::default())) {
            Err(PluginError::PanelRefused(got)) => assert_eq!(got, reason),
            other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
        }
    }
    let _first = ctx
        .register_panel(spec(ID, PanelLocation::Sidebar), Counter(Arc::default()))
        .expect("registers");
    match ctx.register_panel(spec(ID, PanelLocation::Inspector), Counter(Arc::default())) {
        Err(PluginError::PanelRefused(got)) => {
            assert_eq!(got, format!("a panel is registered as {ID}"));
        }
        other => panic!("expected a refusal, got {:?}", other.map(|_| ())),
    }
    assert_eq!(ctx.panels().len(), 1);
}
