//! A plugin panel as the window drives it: read, clicked, typed into, dragged,
//! and gone.
//!
//! Each case drives the host's own calls (`sync_plugin_panels`, `panel_event`,
//! the text field row, the slider grab, ride and release) against a panel
//! registered on the session's real context, and asserts what reached the
//! plugin and the tree the window read back. Drawing is not covered here: the
//! gpui elements in `panels/plugin_panel.rs` call these same methods.

use std::sync::Arc;
use std::time::Duration;

use aria_plugin::{
    MAX_TEXT_BYTES, PanelEvent, PanelLocation, PanelSpec, PanelView, PluginContext, Registration,
    UiNode,
};
use parking_lot::Mutex;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::app::plugin_panels::{PluginPanels, image_of};
use crate::keys::fields;

const PANEL: &str = "test.probe";

/// What the probe panel holds, and every event it received.
#[derive(Default)]
struct Held {
    events: Vec<PanelEvent>,
    presses: u32,
    on: bool,
    level: f64,
    name: String,
    mode: usize,
    pixels: Option<Arc<[u8]>>,
}

#[derive(Clone, Default)]
struct Probe(Arc<Mutex<Held>>);

impl PanelView for Probe {
    fn view(&self, _: &PluginContext) -> UiNode {
        let held = self.0.lock();
        let mut nodes = vec![
            UiNode::heading("Probe"),
            UiNode::text(format!("presses {}", held.presses)),
            UiNode::button("press", "Press"),
            UiNode::Button {
                id: "off".into(),
                label: "Off".into(),
                disabled: true,
            },
            UiNode::toggle("live", "Live", held.on),
            UiNode::Slider {
                id: "level".into(),
                label: "Level".into(),
                value: held.level,
                min: 0.0,
                max: 10.0,
                step: 0.5,
            },
            UiNode::input("name", held.name.as_str(), "Name"),
            UiNode::choice("mode", "Mode", ["a", "b", "c"], held.mode),
        ];
        if let Some(pixels) = &held.pixels {
            nodes.push(UiNode::Image {
                width: 2,
                height: 1,
                rgba: Arc::clone(pixels),
            });
        }
        UiNode::column(nodes)
    }

    fn event(&self, _: &PluginContext, event: &PanelEvent) -> Result<(), String> {
        let mut held = self.0.lock();
        held.events.push(event.clone());
        match event {
            PanelEvent::Pressed { .. } => held.presses += 1,
            PanelEvent::Toggled { on, .. } => held.on = *on,
            PanelEvent::Slid { value, .. } => held.level = *value,
            PanelEvent::Submitted { text, .. } if &**text == "bad" => {
                return Err("bad name".into());
            }
            PanelEvent::Submitted { text, .. } => held.name = text.to_string(),
            PanelEvent::Chose { index, .. } => held.mode = *index,
        }
        Ok(())
    }
}

fn register(state: &AppState, probe: &Probe) -> Registration {
    state
        .ctx
        .register_panel(
            PanelSpec {
                id: PANEL.into(),
                title: "Probe".into(),
                location: PanelLocation::Sidebar,
            },
            probe.clone(),
        )
        .expect("the probe panel registers")
}

/// Read until the panels stop moving: a read can queue a build, and a build
/// moves the revision when it lands. Bounded, so a read that never settles
/// fails here rather than hanging the suite.
fn settle(state: &mut AppState) {
    for _ in 0..16 {
        assert!(
            state.ctx.events().flush(Duration::from_secs(5)),
            "the delivery thread did not drain"
        );
        if state.sync_plugin_panels().is_none() {
            return;
        }
    }
    panic!("the panels never stopped moving");
}

/// A session with the probe mounted for its lifetime, read once it settled.
fn mounted() -> (AppState, Probe) {
    let (mut state, _) = with_clip();
    let probe = Probe::default();
    let registration = register(&state, &probe);
    state._services.push(registration);
    settle(&mut state);
    (state, probe)
}

fn tree(state: &AppState) -> Arc<UiNode> {
    state
        .plugin_panels
        .shown()
        .iter()
        .find(|panel| panel.spec.id == PANEL)
        .and_then(|panel| panel.tree.clone())
        .expect("the probe has drawn")
}

fn control(state: &AppState, id: &str) -> UiNode {
    tree(state).find(id).expect("the control is drawn").clone()
}

fn events(probe: &Probe) -> Vec<PanelEvent> {
    probe.0.lock().events.clone()
}

/// Mounts the probe and puts the keyboard in its empty text box, for the
/// paste sweep in `field_paste_tests`.
pub(super) fn focus_probe_input(state: &mut AppState) {
    let probe = Probe::default();
    let registration = register(state, &probe);
    state._services.push(registration);
    settle(state);
    assert!(state.focus_panel_input(PANEL, &Arc::from("name")));
}

#[test]
fn a_panel_shows_loading_until_its_first_build_lands_and_docks_where_declared() {
    let (mut state, _) = with_clip();
    let probe = Probe::default();
    let _registration = register(&state, &probe);
    assert!(state.sync_plugin_panels().is_some());
    let first = state.plugin_panels.shown();
    assert_eq!(first.len(), 1);
    assert_eq!(&*first[0].spec.title, "Probe");
    assert!(
        first[0].tree.is_none(),
        "the first read cannot wait for a build"
    );

    settle(&mut state);
    assert_eq!(
        state.plugin_panels.at(PanelLocation::Sidebar).count(),
        1,
        "docked where it was declared"
    );
    assert_eq!(state.plugin_panels.at(PanelLocation::Inspector).count(), 0);
    assert_eq!(control(&state, "press"), UiNode::button("press", "Press"));
    // Settled means a further read finds nothing new.
    assert!(state.sync_plugin_panels().is_none());
}

#[test]
fn a_press_reaches_the_plugin_and_the_rebuilt_tree_is_read() {
    let (mut state, probe) = mounted();
    state.panel_event(PANEL, PanelEvent::Pressed { id: "press".into() });
    settle(&mut state);
    assert_eq!(events(&probe), [PanelEvent::Pressed { id: "press".into() }]);
    let drawn = tree(&state);
    let UiNode::Column { children } = &*drawn else {
        panic!("the probe draws a column");
    };
    assert_eq!(children[1], UiNode::text("presses 1"));
    assert_eq!(state.last_error, None);
}

#[test]
fn toggles_and_choices_send_the_value_clicked() {
    let (mut state, probe) = mounted();
    state.panel_event(
        PANEL,
        PanelEvent::Toggled {
            id: "live".into(),
            on: true,
        },
    );
    state.panel_event(
        PANEL,
        PanelEvent::Chose {
            id: "mode".into(),
            index: 2,
        },
    );
    settle(&mut state);
    assert_eq!(
        control(&state, "live"),
        UiNode::toggle("live", "Live", true)
    );
    assert_eq!(
        control(&state, "mode"),
        UiNode::choice("mode", "Mode", ["a", "b", "c"], 2)
    );
    assert_eq!(events(&probe).len(), 2);
}

/// The host holds an event to the tree on screen: the plugin never receives a
/// press on a disabled button, a control that is not drawn, or an option that
/// is not offered, and the status line states why.
#[test]
fn an_event_the_tree_cannot_produce_is_refused_before_the_plugin() {
    let cases = [
        (
            PanelEvent::Pressed { id: "off".into() },
            "panel test.probe: control \"off\" is disabled",
        ),
        (
            PanelEvent::Pressed { id: "gone".into() },
            "panel test.probe: the panel has no control \"gone\"",
        ),
        (
            PanelEvent::Chose {
                id: "mode".into(),
                index: 3,
            },
            "panel test.probe: control \"mode\" does not produce this event or value",
        ),
        (
            PanelEvent::Toggled {
                id: "press".into(),
                on: true,
            },
            "panel test.probe: control \"press\" does not produce this event or value",
        ),
    ];
    for (event, expect) in cases {
        let (mut state, probe) = mounted();
        state.panel_event(PANEL, event);
        settle(&mut state);
        assert_eq!(state.last_error.as_deref(), Some(expect));
        assert!(events(&probe).is_empty(), "{expect}");
    }
    let (mut state, _) = mounted();
    state.panel_event("test.missing", PanelEvent::Pressed { id: "press".into() });
    assert_eq!(
        state.last_error.as_deref(),
        Some("no panel is registered as test.missing")
    );
}

#[test]
fn a_slider_follows_the_pointer_and_sends_one_snapped_value_on_release() {
    let (mut state, probe) = mounted();
    let level: Arc<str> = Arc::from("level");
    // Not painted yet: a press has nothing to measure against.
    assert!(!state.grab_panel_slider(PANEL, &level, 150.0));

    let key = PluginPanels::slider_key(PANEL, "level");
    assert!(state.plugin_panels.record_track(key, [100.0, 200.0]));
    assert!(
        !state.plugin_panels.record_track(key, [100.0, 200.0]),
        "the same rectangle asks for no second frame"
    );
    assert!(state.grab_panel_slider(PANEL, &level, 100.0));
    let held = |state: &AppState| state.plugin_panels.slide.as_ref().map(|slide| slide.value);
    assert_eq!(held(&state), Some(0.0));
    // 51 of 200 pixels is 2.55, snapped to the 0.5 step.
    assert!(state.ride_panel_slider(151.0));
    assert_eq!(held(&state), Some(2.5));
    assert!(!state.ride_panel_slider(152.0), "2.6 snaps to the same 2.5");
    assert!(state.ride_panel_slider(400.0));
    assert_eq!(held(&state), Some(10.0), "past the track clamps to max");
    assert!(state.ride_panel_slider(-50.0));
    assert_eq!(held(&state), Some(0.0), "before the track clamps to min");
    assert!(state.ride_panel_slider(200.0));
    assert!(
        events(&probe).is_empty(),
        "a drag sends nothing until it ends"
    );

    assert!(state.release_panel_slider());
    assert!(state.plugin_panels.slide.is_none());
    settle(&mut state);
    assert_eq!(
        events(&probe),
        [PanelEvent::Slid {
            id: "level".into(),
            value: 5.0,
        }]
    );
    let UiNode::Slider { value, .. } = control(&state, "level") else {
        panic!("level is a slider");
    };
    assert_eq!(value, 5.0);

    // Letting go where the tree already stands sends nothing.
    assert!(state.grab_panel_slider(PANEL, &level, 200.0));
    assert!(state.release_panel_slider());
    settle(&mut state);
    assert_eq!(events(&probe).len(), 1);
    assert!(!state.release_panel_slider(), "nothing is held");
    assert!(!state.ride_panel_slider(10.0), "nothing is held");
}

#[test]
fn a_text_box_takes_typing_and_sends_its_draft_on_enter() {
    let (mut state, probe) = mounted();
    let name: Arc<str> = Arc::from("name");
    assert!(
        !state.focus_panel_input(PANEL, &Arc::from("press")),
        "not a text box"
    );
    assert!(state.focus_panel_input(PANEL, &name));
    let field = fields::focused(&state).expect("the panel box holds the keyboard");
    assert_eq!(field.id, "aria.field.plugin-panel");

    assert!(fields::insert(&mut state, "Ada\n"));
    assert_eq!((field.text)(&state), "Ada");
    (field.backspace)(&mut state);
    assert_eq!((field.text)(&state), "Ad");
    state.submit_panel_input();
    assert!(fields::focused(&state).is_none(), "Enter leaves the box");
    settle(&mut state);
    assert_eq!(
        events(&probe),
        [PanelEvent::Submitted {
            id: "name".into(),
            text: "Ad".into(),
        }]
    );
    assert_eq!(control(&state, "name"), UiNode::input("name", "Ad", "Name"));

    // Focus starts from the tree's value; Escape sends nothing.
    assert!(state.focus_panel_input(PANEL, &name));
    assert_eq!((field.text)(&state), "Ad");
    fields::insert(&mut state, "x");
    (field.escape)(&mut state);
    assert!(fields::focused(&state).is_none());
    settle(&mut state);
    assert_eq!(events(&probe).len(), 1);
}

/// A handler's `Err` is shown on the panel and the tree keeps what it had; the
/// next accepted event clears it.
#[test]
fn a_refused_submission_is_shown_on_the_panel_until_the_next_accepted_event() {
    let (mut state, _) = mounted();
    let name: Arc<str> = Arc::from("name");
    assert!(state.focus_panel_input(PANEL, &name));
    fields::insert(&mut state, "bad");
    state.submit_panel_input();
    settle(&mut state);
    let error = |state: &AppState| state.plugin_panels.shown()[0].event_error.clone();
    assert_eq!(error(&state).as_deref(), Some("bad name"));
    assert_eq!(control(&state, "name"), UiNode::input("name", "", "Name"));

    state.panel_event(PANEL, PanelEvent::Pressed { id: "press".into() });
    settle(&mut state);
    assert_eq!(error(&state), None);
}

#[test]
fn a_draft_stops_at_the_longest_text_an_event_carries() {
    let (mut state, _) = mounted();
    assert!(state.focus_panel_input(PANEL, &Arc::from("name")));
    fields::insert(&mut state, &"é".repeat(MAX_TEXT_BYTES));
    let draft = state.panel_input_text();
    assert!(draft.len() <= MAX_TEXT_BYTES, "{} bytes", draft.len());
    assert!(draft.len() > MAX_TEXT_BYTES - 2, "{} bytes", draft.len());
    assert!(draft.chars().all(|ch| ch == 'é'));
}

/// A panel unregistered mid-gesture leaves no draft holding the keyboard and
/// no drag holding the pointer.
#[test]
fn a_panel_that_goes_away_takes_its_draft_and_drag_with_it() {
    let (mut state, _) = with_clip();
    let probe = Probe::default();
    let registration = register(&state, &probe);
    settle(&mut state);
    let key = PluginPanels::slider_key(PANEL, "level");
    state.plugin_panels.record_track(key, [0.0, 100.0]);
    assert!(state.focus_panel_input(PANEL, &Arc::from("name")));
    assert!(state.grab_panel_slider(PANEL, &Arc::from("level"), 50.0));

    drop(registration);
    settle(&mut state);
    assert!(state.plugin_panels.shown().is_empty());
    assert!(fields::focused(&state).is_none());
    assert!(state.plugin_panels.slide.is_none());
    assert!(!state.release_panel_slider());
}

/// A panel a plugin registers is a Window row with the panel's title, which
/// opens and closes it the way a built-in panel's row does and is checked while
/// it is open; the row leaves with the registration.
#[test]
fn a_plugin_panel_is_a_window_row_until_its_registration_drops() {
    use gpui::AppContext as _;
    let (mut state, _) = with_clip();
    let probe = Probe::default();
    let registration = register(&state, &probe);
    settle(&mut state);
    let row = format!("{}{PANEL}", crate::chrome::menu_window::PANEL_PREFIX);
    let find = |state: &AppState| {
        crate::chrome::menu_items::items_of(crate::types::AppMenu::Window, state)
            .into_iter()
            .find(|item| item.id == row)
    };
    let item = find(&state).expect("the plugin panel has no Window row");
    assert_eq!(item.label, "Probe");
    let checked = item.checked.expect("a panel row is checkable");
    let was_open = state.dock_tree.is_open(PANEL);
    let cx = gpui::TestAppContext::single();
    let entity = cx.update(|app| app.new(|_| state));
    let press = || {
        cx.update(|app| {
            entity.update(app, |state: &mut AppState, cx| {
                (item.run)(item.id, state, cx);
                (state.dock_tree.is_open(PANEL), checked(item.id, state))
            })
        })
    };
    assert_eq!(press(), (!was_open, !was_open), "the first press");
    assert_eq!(press(), (was_open, was_open), "the second press");

    drop(registration);
    let gone = cx.update(|app| {
        entity.update(app, |state: &mut AppState, _| {
            settle(state);
            find(state).is_none()
        })
    });
    assert!(gone, "the Window row outlived the registration");
}

#[test]
fn an_image_is_decoded_once_and_released_when_no_tree_shows_it() {
    let (mut state, probe) = mounted();
    let pixels: Arc<[u8]> = Arc::from([255u8, 0, 0, 255, 0, 0, 255, 255].as_slice());
    probe.0.lock().pixels = Some(Arc::clone(&pixels));
    state.panel_event(PANEL, PanelEvent::Pressed { id: "press".into() });
    settle(&mut state);
    let first = image_of(state.plugin_panels.images(), 2, 1, &pixels).expect("decoded");
    assert!(
        image_of(state.plugin_panels.images(), 1, 2, &pixels).is_none(),
        "the same bytes at another size are another image"
    );

    // A rebuild that shows the same pixels keeps the same texture.
    state.panel_event(PANEL, PanelEvent::Pressed { id: "press".into() });
    assert!(state.ctx.events().flush(Duration::from_secs(5)));
    let evicted = state.sync_plugin_panels().expect("the rebuild moved");
    assert!(evicted.is_empty());
    let again = image_of(state.plugin_panels.images(), 2, 1, &pixels).expect("kept");
    assert!(Arc::ptr_eq(&first, &again), "decoded again");

    // A rebuild without it hands the texture back for release.
    probe.0.lock().pixels = None;
    state.panel_event(PANEL, PanelEvent::Pressed { id: "press".into() });
    assert!(state.ctx.events().flush(Duration::from_secs(5)));
    let evicted = state.sync_plugin_panels().expect("the rebuild moved");
    assert_eq!(evicted.len(), 1);
    assert!(Arc::ptr_eq(&evicted[0], &first));
    assert!(state.plugin_panels.images().is_empty());
}

/// A package that waits for `panel:<id>` and declares that panel in the
/// sidebar. Loading it registers the probe under the id when `registers`.
fn defer_panel_package(
    state: &AppState,
    id: &str,
    name: &str,
    panel: &str,
    registers: bool,
) -> (
    Arc<Mutex<Vec<Registration>>>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let package = super::activation_tests::lazy_package(
        id,
        name,
        &format!(
            "[activation]\non = [\"panel:{panel}\"]\n\
             [[panels]]\nid = {panel:?}\ntitle = \"Later\"\nlocation = \"sidebar\"\n"
        ),
    );
    let held: Arc<Mutex<Vec<Registration>>> = Arc::default();
    let loads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (slot, count, panel) = (Arc::clone(&held), Arc::clone(&loads), panel.to_owned());
    state
        .ctx
        .defer_package(&package, move |ctx| {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if registers {
                let spec = PanelSpec {
                    id: panel.clone(),
                    title: "Later".into(),
                    location: PanelLocation::Sidebar,
                };
                let registration = ctx
                    .register_panel(spec, Probe::default())
                    .map_err(|err| err.to_string())?;
                slot.lock().push(registration);
            }
            Ok(())
        })
        .expect("the package defers");
    (held, loads)
}

/// A waiting package's declared panel is held where it docks beside the
/// registered ones, opening it loads the package once and the panel it
/// registers replaces the waiting one, and a package that loads without
/// registering the panel is reported by name.
#[test]
fn a_waiting_panel_docks_where_declared_and_opening_it_loads_its_package() {
    let (mut state, _) = with_clip();
    let (_held, loads) =
        defer_panel_package(&state, "com.test.later", "Later", "test.later-panel", true);
    settle(&mut state);
    let waiting = state.plugin_panels.waiting();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].spec.id, "test.later-panel");
    assert_eq!(waiting[0].plugin_name, "Later");
    assert!(state.plugin_panels.any_at(PanelLocation::Sidebar));
    assert!(!state.plugin_panels.any_at(PanelLocation::Inspector));
    assert_eq!(state.plugin_panels.at(PanelLocation::Sidebar).count(), 0);
    assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 0);

    state.open_waiting_panel("test.later-panel");
    assert_eq!(loads.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(state.last_error, None);
    settle(&mut state);
    assert!(state.plugin_panels.waiting().is_empty());
    assert!(
        state
            .plugin_panels
            .at(PanelLocation::Sidebar)
            .any(|panel| panel.spec.id == "test.later-panel")
    );
    state.open_waiting_panel("test.later-panel");
    assert_eq!(
        loads.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "an open panel is not waiting"
    );

    let (_held, silent) = defer_panel_package(
        &state,
        "com.test.silent",
        "Silent",
        "test.silent-panel",
        false,
    );
    settle(&mut state);
    state.open_waiting_panel("test.silent-panel");
    assert_eq!(silent.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        state.last_error.as_deref(),
        Some("Silent did not register panel test.silent-panel")
    );
}
