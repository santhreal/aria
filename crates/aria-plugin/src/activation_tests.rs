//! Tests for activation events.
//!
//! Closes the class "a deferred package loads too early, twice, never, or in
//! the wrong order, or its stand-ins outlive or collide with what it loads":
//! every case defers a real package read from disk through
//! [`PluginContext::defer_package`] and fires the trigger a host fires. The
//! per-kind test matches every [`ActivationKind`], so a new kind does not
//! compile until it has a trigger here. It does not cover where a host calls
//! the triggers; the `aria-ui` tests own that.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

use aria_models::{
    AudioEffectInstance, Clip, EffectInstance, MediaId, Preset, Rational, TimeRange, Track,
    TrackKind, Transition,
};

use super::*;
use crate::commands::CommandSpec;
use crate::error::PluginError;
use crate::package::{COMPONENT_FILE, MANIFEST_FILE, PRESETS_DIR, register_declared};
use crate::plugin::{Plugin, PluginManifest};
use crate::service::PanelLocation;
use crate::settings_schema::SettingsSchema;

/// What a deferred package's code does when it loads: what its tier does
/// (register what it declares), then register `command` as the real handler.
struct Code {
    manifest: PluginManifest,
    package: Package,
    command: Option<&'static str>,
    log: Arc<Log>,
}

/// What every loaded package did, in order.
#[derive(Default)]
struct Log {
    loads: Mutex<Vec<String>>,
    runs: AtomicUsize,
}

impl Log {
    fn loads(&self) -> Vec<String> {
        self.loads.lock().clone()
    }
}

impl Plugin for Code {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        self.log.loads.lock().push(self.manifest.id.to_string());
        register_declared(ctx, &self.package)?;
        if let Some(command) = self.command {
            let log = Arc::clone(&self.log);
            ctx.register_command(
                CommandSpec::new(command, "Real"),
                move |_: &PluginContext| {
                    log.runs.fetch_add(1, AtomicOrdering::SeqCst);
                    Ok(())
                },
            )?;
        }
        Ok(())
    }
}

/// A host context with a Settings schema, and the packages' scratch root.
struct Host {
    ctx: PluginContext,
    schema: Arc<SettingsSchema>,
    log: Arc<Log>,
    root: tempfile::TempDir,
    _schema: Registration,
}

impl Host {
    fn new() -> Self {
        let ctx = PluginContext::new();
        let schema = Arc::new(SettingsSchema::default());
        let held = ctx.register_arc(Arc::clone(&schema));
        Self {
            ctx,
            schema,
            log: Arc::default(),
            root: tempfile::tempdir().expect("scratch dir"),
            _schema: held,
        }
    }

    /// The code package `id`, named `name`, with `rest` after `[plugin]`.
    fn package(&self, id: &str, name: &str, rest: &str) -> Package {
        let dir = self.root.path().join(id);
        write(
            &dir,
            MANIFEST_FILE,
            &format!(
                "[plugin]\nid = {id:?}\nname = {name:?}\nversion = \"1.0.0\"\napi = 1\n{rest}"
            ),
        );
        write(&dir, COMPONENT_FILE, "\0asm");
        Package::open(&dir).expect("opens")
    }

    /// Defer `package`; loading it mounts [`Code`] registering `command`.
    fn defer(&self, package: &Package, command: Option<&'static str>) {
        let code = Arc::new(Code {
            manifest: package.plugin_manifest(),
            package: package.clone(),
            command,
            log: Arc::clone(&self.log),
        });
        self.ctx
            .defer_package(package, move |ctx| {
                ctx.mount(&*code).map_err(|err| err.to_string())
            })
            .expect("defers");
    }

    fn command_titles(&self) -> Vec<(String, String)> {
        self.ctx
            .commands()
            .iter()
            .map(|spec| (spec.id.clone(), spec.title.clone()))
            .collect()
    }
}

fn write(dir: &Path, relative: &str, contents: &str) {
    let path = dir.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("create parent");
    std::fs::write(path, contents).expect("write");
}

/// A package that declares a command offered in a menu and on a key, a
/// setting, a panel and a preset, and waits for `on`.
const DECLARES_EVERYTHING: &str = r#"
[[commands]]
id = "lazy.run"
title = "Run lazily"
menu = "Edit"
key = "Ctrl+Alt+J"

[[settings]]
key = "mode"
label = "Mode"

[[panels]]
id = "lazy.panel"
title = "Lazy"
location = "inspector"
"#;

fn lazy(host: &Host, on: &str) -> Package {
    let dir = host.root.path().join("com.test.lazy");
    let preset = Preset::new(
        "Warm",
        vec![EffectInstance::new("vignette")],
        Vec::new(),
        Vec::new(),
    );
    write(
        &dir,
        &format!("{PRESETS_DIR}/warm.ariapreset"),
        &preset.to_json().expect("serializes"),
    );
    host.package(
        "com.test.lazy",
        "Lazy",
        &format!("[activation]\non = [{on:?}]\n{DECLARES_EVERYTHING}"),
    )
}

#[test]
fn every_event_kind_reads_back_as_written() {
    for kind in ActivationKind::ALL {
        let written = match kind {
            ActivationKind::Startup => "startup".to_owned(),
            _ => format!("{}lut-2", kind.spelling()),
        };
        let event = Activation::parse(&written).expect(&written);
        assert_eq!(event.kind(), kind, "{written}");
        assert_eq!(event.to_string(), written);
        assert_eq!(
            event.argument(),
            (kind != ActivationKind::Startup).then_some("lut-2")
        );
    }
    for written in ["file:cube", "file:.cube", "file:.CUBE", "file: Cube "] {
        assert_eq!(
            Activation::parse(written),
            Ok(Activation::File("cube".into())),
            "{written}"
        );
    }
    let prefixes = "command: file: effect: transition: generator: panel: setting:";
    let refused = [
        (
            "boot",
            format!("activation event \"boot\" is not `startup` or one of {prefixes}"),
        ),
        (
            "Startup",
            format!("activation event \"Startup\" is not `startup` or one of {prefixes}"),
        ),
        (
            "service:com.example.Clock",
            format!(
                "activation event \"service:com.example.Clock\" is not `startup` or one of \
                 {prefixes}"
            ),
        ),
        (
            "command:",
            "activation event \"command:\" names nothing".to_owned(),
        ),
        (
            "panel:  ",
            "activation event \"panel:  \" names nothing".to_owned(),
        ),
        (
            "file:.",
            "activation event \"file:.\" names no file extension: write `file:cube`".to_owned(),
        ),
        (
            "file:a.b",
            "activation event \"file:a.b\" names no file extension: write `file:cube`".to_owned(),
        ),
    ];
    for (written, reason) in refused {
        assert_eq!(Activation::parse(written), Err(reason), "{written}");
    }
}

/// Before the package loads, everything it declares is listed under it. The
/// first run loads it once and runs the handler it registered; the stand-ins
/// are gone, so its own registrations of the same ids are not refused.
#[test]
fn a_waiting_command_is_listed_and_its_first_run_loads_the_package_once() {
    let host = Host::new();
    let package = lazy(&host, "command:lazy.run");
    let before = host.ctx.activation_revision();
    host.defer(&package, Some("lazy.run"));

    assert!(host.ctx.activation_revision() > before);
    assert_eq!(
        host.ctx.waiting_plugins(),
        vec![WaitingPlugin {
            id: "com.test.lazy".into(),
            name: "Lazy".into(),
            events: vec![Activation::Command("lazy.run".into())],
        }]
    );
    assert_eq!(
        host.command_titles(),
        vec![("lazy.run".into(), "Run lazily".into())]
    );
    assert_eq!(host.ctx.menu_entries().len(), 1);
    assert_eq!(host.ctx.key_defaults().len(), 1);
    let section = host.schema.section_of("com.test.lazy").expect("a section");
    assert_eq!(section.fields[0].key, "com.test.lazy.mode");
    let presets = host.ctx.presets();
    assert_eq!(presets.len(), 1);
    assert_eq!(presets[0].plugin.as_ref(), "Lazy");
    assert!(host.log.loads().is_empty(), "nothing loads before the run");
    assert!(!host.ctx.is_mounted("com.test.lazy"));

    host.ctx.run_command("lazy.run").expect("runs");
    assert_eq!(host.log.loads(), vec!["com.test.lazy"]);
    assert_eq!(host.log.runs.load(AtomicOrdering::SeqCst), 1);
    assert!(host.ctx.is_mounted("com.test.lazy"));
    assert!(!host.ctx.is_waiting("com.test.lazy"));
    assert!(host.ctx.waiting_plugins().is_empty());
    assert!(host.ctx.waiting_panels().is_empty());
    assert_eq!(
        host.command_titles(),
        vec![("lazy.run".into(), "Real".into())]
    );
    assert_eq!(host.ctx.menu_entries().len(), 1);
    assert_eq!(host.ctx.key_defaults().len(), 1);
    assert!(host.schema.section_of("com.test.lazy").is_some());
    assert_eq!(host.ctx.presets().len(), 1);

    host.ctx.run_command("lazy.run").expect("runs again");
    assert_eq!(
        host.log.loads().len(),
        1,
        "a second run does not load again"
    );
    assert_eq!(host.log.runs.load(AtomicOrdering::SeqCst), 2);
}

#[test]
fn a_run_fails_naming_the_package_when_it_registers_no_such_command() {
    let host = Host::new();
    let package = lazy(&host, "command:lazy.run");
    host.defer(&package, None);

    assert_eq!(
        host.ctx.run_command("lazy.run"),
        Err(CommandError::Failed(
            "Lazy did not register lazy.run".into()
        ))
    );
    assert!(host.ctx.is_mounted("com.test.lazy"));
    assert_eq!(
        host.ctx.run_command("lazy.run"),
        Err(CommandError::Unknown("lazy.run".into()))
    );
}

#[test]
fn a_run_fails_with_the_load_error_and_nothing_stays_waiting() {
    let host = Host::new();
    let package = lazy(&host, "command:lazy.run");
    host.ctx
        .defer_package(&package, |_| Err("the component is not valid".to_owned()))
        .expect("defers");

    assert_eq!(
        host.ctx.run_command("lazy.run"),
        Err(CommandError::Failed(
            "Lazy did not load: the component is not valid".into()
        ))
    );
    assert!(!host.ctx.is_waiting("com.test.lazy"));
    assert!(host.ctx.commands().is_empty(), "the stand-ins dropped");
    assert!(host.schema.section_of("com.test.lazy").is_none());
}

#[test]
fn a_file_event_matches_the_extension_without_case() {
    let host = Host::new();
    let package = host.package(
        "com.test.cube",
        "Cube",
        "[activation]\non = [\"file:.CUBE\"]",
    );
    assert_eq!(
        package.manifest.activation,
        vec![Activation::File("cube".into())]
    );
    host.defer(&package, None);

    for unrelated in ["look.lut", "cube", "look.cube.zip", ""] {
        assert!(
            host.ctx.activate_for_file(Path::new(unrelated)).is_empty(),
            "{unrelated}"
        );
    }
    assert!(host.ctx.is_waiting("com.test.cube"));
    let activated = host.ctx.activate_for_file(Path::new("grades/Look.Cube"));
    assert_eq!(
        activated,
        vec![Activated {
            id: "com.test.cube".into(),
            name: "Cube".into(),
            result: Ok(()),
        }]
    );
    assert!(
        host.ctx
            .activate_for_file(Path::new("other.cube"))
            .is_empty()
    );
    assert_eq!(host.log.loads(), vec!["com.test.cube"]);
}

/// A project with two abutting video clips joined by a transition, one with
/// an effect and an audio insert, the other with a generator's effect; an
/// audio lane with an insert; and a nested sequence whose clip has an effect.
/// Each id is `com.test.<what>`.
fn project_using_everything() -> Project {
    let rate = Rational::FPS_24;
    let mut project = Project::new("uses", rate);
    let timeline = project.timeline_mut();
    let video = timeline.add_track(Track::new(TrackKind::Video, "V1"));
    let mut left = Clip::from_media(
        MediaId::next(),
        TimeRange::at_rate(0, 24, rate),
        TimeRange::at_rate(0, 24, rate),
    );
    left.effects.push(EffectInstance::new("com.test.glow"));
    left.audio_effects
        .push(AudioEffectInstance::new("com.test.clip-gain"));
    let mut right = Clip::from_media(
        MediaId::next(),
        TimeRange::at_rate(0, 24, rate),
        TimeRange::at_rate(24, 24, rate),
    );
    right.effects.push(EffectInstance::new("com.test.noise"));
    let left = timeline.add_clip(video, left).expect("left clip");
    let right = timeline.add_clip(video, right).expect("right clip");
    let mut lane = Track::new(TrackKind::Audio, "A1");
    lane.audio_effects
        .push(AudioEffectInstance::new("com.test.lane-eq"));
    timeline.add_track(lane);
    project.restore_transitions(vec![(
        video,
        vec![Transition::new(left, right, "com.test.wipe", 12)],
    )]);
    let nest = project.create_sequence("Nest", rate);
    let nested = &mut project.sequence_mut(nest).expect("the nest").timeline;
    let nested_video = nested.add_track(Track::new(TrackKind::Video, "V1"));
    let mut inner = Clip::from_media(
        MediaId::next(),
        TimeRange::at_rate(0, 24, rate),
        TimeRange::at_rate(0, 24, rate),
    );
    inner.effects.push(EffectInstance::new("com.test.nested"));
    nested.add_clip(nested_video, inner).expect("nested clip");
    project
}

fn sorted(mut events: Vec<Activation>) -> Vec<String> {
    let mut written: Vec<String> = events.drain(..).map(|event| event.to_string()).collect();
    written.sort();
    written
}

#[test]
fn a_project_names_each_waiting_effect_transition_and_generator_it_uses() {
    let host = Host::new();
    let events = [
        "effect:com.test.glow",
        "effect:com.test.clip-gain",
        "generator:com.test.noise",
        "effect:com.test.lane-eq",
        "transition:com.test.wipe",
        "effect:com.test.nested",
        "effect:com.test.unused",
        "transition:com.test.glow",
        "file:glow",
    ];
    for (index, event) in events.iter().enumerate() {
        let package = host.package(
            &format!("com.test.p{index}"),
            &format!("P{index}"),
            &format!("[activation]\non = [{event:?}]"),
        );
        host.defer(&package, None);
    }
    let project = project_using_everything();

    let used = host.ctx.project_activations(&project, 1);
    assert_eq!(
        sorted(used.clone()),
        vec![
            "effect:com.test.clip-gain",
            "effect:com.test.glow",
            "effect:com.test.lane-eq",
            "effect:com.test.nested",
            "generator:com.test.noise",
            "transition:com.test.wipe",
        ]
    );
    for event in &used {
        assert_eq!(host.ctx.activate(event).len(), 1, "{event}");
    }
    assert_eq!(host.log.loads().len(), 6);
    assert!(
        host.ctx.project_activations(&project, 1).is_empty(),
        "what loaded is no longer waiting"
    );
    let waiting: Vec<String> = host
        .ctx
        .waiting_plugins()
        .into_iter()
        .map(|plugin| plugin.id)
        .collect();
    assert_eq!(waiting, vec!["com.test.p6", "com.test.p7", "com.test.p8"]);
}

/// A call that found nothing is not repeated at the same project revision
/// until a package starts or stops waiting.
#[test]
fn a_settled_project_revision_is_not_walked_again() {
    let host = Host::new();
    let package = host.package(
        "com.test.late",
        "Late",
        "[activation]\non = [\"effect:com.test.late\"]",
    );
    host.defer(&package, None);
    let mut project = project_using_everything();
    assert!(host.ctx.project_activations(&project, 7).is_empty());

    let clip = project
        .timeline()
        .tracks_ordered()
        .find_map(|track| track.clips().next().map(|clip| clip.id))
        .expect("a clip");
    project
        .timeline_mut()
        .clip_mut(clip)
        .expect("the clip")
        .effects
        .push(EffectInstance::new("com.test.late"));
    assert!(
        host.ctx.project_activations(&project, 7).is_empty(),
        "the same revision is settled"
    );
    assert_eq!(
        host.ctx.project_activations(&project, 8),
        vec![Activation::Effect("com.test.late".into())],
        "a new revision is walked"
    );

    // Revision 8 settles once nothing waits, and a package that starts
    // waiting again unsettles it.
    assert!(host.ctx.cancel_activation("com.test.late"));
    assert!(host.ctx.project_activations(&project, 8).is_empty());
    host.defer(&package, None);
    assert_eq!(
        host.ctx.project_activations(&project, 8),
        vec![Activation::Effect("com.test.late".into())]
    );
}

#[test]
fn waiting_requirements_load_first() {
    let host = Host::new();
    let top = host.package(
        "com.test.top",
        "Top",
        "requires = [\"com.test.base\"]\n[activation]\non = [\"file:top\"]",
    );
    let base = host.package(
        "com.test.base",
        "Base",
        "[activation]\non = [\"file:base\"]",
    );
    // Deferred in the wrong order: the requirement still loads first.
    host.defer(&top, None);
    host.defer(&base, None);

    let activated = host.ctx.activate_for_file(Path::new("a.top"));
    let ids: Vec<&str> = activated.iter().map(|done| done.id.as_str()).collect();
    assert_eq!(ids, vec!["com.test.base", "com.test.top"]);
    assert!(
        activated.iter().all(|done| done.result.is_ok()),
        "{activated:?}"
    );
    assert_eq!(host.log.loads(), vec!["com.test.base", "com.test.top"]);
    assert!(host.ctx.is_mounted("com.test.top"));
    assert!(host.ctx.activate_for_file(Path::new("a.base")).is_empty());
}

#[test]
fn cancelling_drops_every_stand_in_and_deferring_again_replaces_them() {
    let host = Host::new();
    let package = lazy(&host, "command:lazy.run");
    host.defer(&package, Some("lazy.run"));
    host.defer(&package, Some("lazy.run"));
    assert_eq!(host.ctx.waiting_plugins().len(), 1);
    assert_eq!(host.ctx.commands().len(), 1);
    assert_eq!(host.ctx.waiting_panels().len(), 1);

    let before = host.ctx.activation_revision();
    assert!(host.ctx.cancel_activation("com.test.lazy"));
    assert!(host.ctx.activation_revision() > before);
    assert!(host.ctx.commands().is_empty());
    assert!(host.ctx.menu_entries().is_empty());
    assert!(host.ctx.key_defaults().is_empty());
    assert!(host.schema.section_of("com.test.lazy").is_none());
    assert!(host.ctx.presets().is_empty());
    assert!(host.ctx.waiting_panels().is_empty());
    assert!(host.ctx.waiting_plugins().is_empty());
    assert!(!host.ctx.cancel_activation("com.test.lazy"));
    assert_eq!(
        host.ctx.run_command("lazy.run"),
        Err(CommandError::Unknown("lazy.run".into()))
    );
    assert!(host.log.loads().is_empty());
}

/// A host draws waiting panels beside registered ones and redraws on the panel
/// waker, so deferring, cancelling and loading a package that declares a
/// panel each move the panel revision and run the waker.
#[test]
fn a_waiting_panel_change_moves_the_panel_revision_and_wakes_the_host() {
    let host = Host::new();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    host.ctx.set_panel_waker(move || {
        counter.fetch_add(1, AtomicOrdering::SeqCst);
    });
    let package = lazy(&host, "panel:lazy.panel");
    let mut seen = (host.ctx.panel_revision(), 0);
    let mut moved = |what: &str| {
        let now = (
            host.ctx.panel_revision(),
            wakes.load(AtomicOrdering::SeqCst),
        );
        assert!(
            now.0 > seen.0 && now.1 > seen.1,
            "{what} left the panel revision at {:?}",
            now
        );
        seen = now;
    };
    host.defer(&package, None);
    assert_eq!(host.ctx.waiting_panels().len(), 1);
    moved("deferring");
    assert!(host.ctx.cancel_activation("com.test.lazy"));
    moved("cancelling");
    host.defer(&package, None);
    moved("deferring again");
    let activated = host.ctx.activate(&Activation::Panel("lazy.panel".into()));
    assert_eq!(activated.len(), 1);
    assert_eq!(activated[0].result, Ok(()));
    assert!(host.ctx.waiting_panels().is_empty());
    moved("loading");
}

/// Mounts the plugin `com.test.mounter`, which runs the waiting command and
/// tries to defer `package` from inside its mount.
struct Mounter {
    manifest: PluginManifest,
    package: Package,
    seen: Mutex<Vec<String>>,
}

impl Plugin for Mounter {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let ran = ctx.run_command("lazy.run");
        self.seen.lock().push(format!("{ran:?}"));
        let deferred = ctx.defer_package(&self.package, |_| Ok(()));
        self.seen.lock().push(format!("{deferred:?}"));
        Ok(())
    }
}

#[test]
fn an_event_during_a_mount_keeps_the_package_waiting_for_the_next() {
    let host = Host::new();
    let package = lazy(&host, "command:lazy.run");
    host.defer(&package, Some("lazy.run"));
    let mounter = Mounter {
        manifest: PluginManifest::new("com.test.mounter", "Mounter", "1.0.0", &[], &[]),
        package,
        seen: Mutex::default(),
    };

    host.ctx.mount(&mounter).expect("mounts");
    assert_eq!(
        *mounter.seen.lock(),
        vec![
            format!(
                "{:?}",
                Err::<(), _>(CommandError::Failed(
                    "Lazy did not load: another plugin is mounting; try again".into()
                ))
            ),
            format!(
                "{:?}",
                Err::<(), _>(
                    "com.test.lazy was not deferred: another plugin is mounting".to_owned()
                )
            ),
        ]
    );
    assert!(host.ctx.is_waiting("com.test.lazy"));
    assert!(host.log.loads().is_empty());

    host.ctx
        .run_command("lazy.run")
        .expect("runs after the mount");
    assert_eq!(host.log.loads(), vec!["com.test.lazy"]);
    assert_eq!(host.log.runs.load(AtomicOrdering::SeqCst), 1);
}

/// A stand-in another thread's activation took but has not dropped yet
/// answers its command once more; that run fails instead of recursing until
/// the stack overflows.
#[test]
fn a_stand_in_that_outlives_its_waiting_entry_fails_instead_of_recursing() {
    let host = Host::new();
    let package = lazy(&host, "command:lazy.run");
    host.defer(&package, Some("lazy.run"));
    let held = {
        let mut waiting = host.ctx.activation_registry().waiting.lock();
        let entry = waiting.pop().expect("waiting");
        entry.stand_ins
    };

    assert_eq!(
        host.ctx.run_command("lazy.run"),
        Err(CommandError::Failed(
            "Lazy is still loading; run lazy.run again".into()
        ))
    );
    assert!(
        FORWARDING.with_borrow(Option::is_none),
        "the marker is cleared"
    );
    drop(held);
    assert!(host.ctx.commands().is_empty());
}

/// Every kind has the trigger a host fires for it, and that trigger loads a
/// package waiting for it. A kind added without a trigger does not compile.
#[test]
fn every_kind_loads_on_its_trigger() {
    for kind in ActivationKind::ALL {
        let host = Host::new();
        let event = match kind {
            ActivationKind::Startup => "startup",
            ActivationKind::Command => "command:lazy.run",
            ActivationKind::File => "file:lazy",
            ActivationKind::Effect => "effect:com.test.glow",
            ActivationKind::Transition => "transition:com.test.wipe",
            ActivationKind::Generator => "generator:com.test.noise",
            ActivationKind::Panel => "panel:lazy.panel",
            ActivationKind::Setting => "setting:mode",
        };
        let package = lazy(&host, event);
        assert_eq!(
            package
                .manifest
                .activation
                .iter()
                .map(Activation::kind)
                .collect::<Vec<_>>(),
            vec![kind]
        );
        if kind == ActivationKind::Startup {
            assert!(package.manifest.loads_at_start());
            assert_eq!(
                host.ctx.defer_package(&package, |_| Ok(())),
                Err("com.test.lazy was not deferred: it loads at start".to_owned())
            );
            continue;
        }
        assert!(!package.manifest.loads_at_start(), "{event}");
        host.defer(&package, Some("lazy.run"));
        assert!(host.log.loads().is_empty(), "{event}");
        let activated = match kind {
            ActivationKind::Startup => unreachable!("handled above"),
            ActivationKind::Command => {
                host.ctx.run_command("lazy.run").expect("runs");
                Vec::new()
            }
            ActivationKind::File => host.ctx.activate_for_file(Path::new("x.LAZY")),
            ActivationKind::Effect | ActivationKind::Transition | ActivationKind::Generator => {
                let used = host.ctx.project_activations(&project_using_everything(), 1);
                assert_eq!(used.len(), 1, "{event}");
                host.ctx.activate(&used[0])
            }
            ActivationKind::Panel => {
                let panels = host.ctx.waiting_panels();
                assert_eq!(
                    panels,
                    vec![WaitingPanel {
                        plugin: "com.test.lazy".into(),
                        plugin_name: "Lazy".into(),
                        spec: Arc::new(PanelSpec {
                            id: "lazy.panel".into(),
                            title: Arc::from("Lazy"),
                            location: PanelLocation::Inspector,
                        }),
                    }]
                );
                host.ctx
                    .activate(&Activation::Panel(panels[0].spec.id.clone()))
            }
            ActivationKind::Setting => host.ctx.activate_setting("com.test.lazy.mode"),
        };
        if kind != ActivationKind::Command {
            assert_eq!(activated.len(), 1, "{event}");
            assert_eq!(activated[0].result, Ok(()), "{event}");
        }
        assert_eq!(host.log.loads(), vec!["com.test.lazy"], "{event}");
        assert!(!host.ctx.is_waiting("com.test.lazy"), "{event}");
    }
}

#[test]
fn a_command_panel_or_setting_event_must_name_a_declared_row() {
    let host = Host::new();
    for (event, what) in [
        ("command:lazy.other", "[[commands]]"),
        ("panel:lazy.other", "[[panels]]"),
        ("setting:other", "[[settings]]"),
    ] {
        let dir = host.root.path().join("refused");
        write(
            &dir,
            MANIFEST_FILE,
            &format!(
                "[plugin]\nid = \"com.test.lazy\"\nname = \"Lazy\"\nversion = \"1.0.0\"\napi = 1\n\
                 [activation]\non = [{event:?}]\n{DECLARES_EVERYTHING}"
            ),
        );
        let refused = Package::open(&dir).expect_err(event);
        assert_eq!(
            refused.reason,
            format!(
                "com.test.lazy: activation event {event:?} names no {what} entry of this package"
            )
        );
    }
}
