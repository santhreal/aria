//! Activation: a package that lists activation events stays unloaded until the
//! first of them.
//!
//! The host defers such a package with [`PluginContext::defer_package`]. While
//! it waits, the host holds stand-ins for what it declares: its WGSL effects,
//! generators and transitions, commands, menu rows, keys, settings section and
//! presets are registered on the view its manifest describes, so the browsers,
//! the palette, the menus and Settings list them under the package, and its
//! declared panels are listed by [`PluginContext::waiting_panels`]. The first
//! trigger that matches one of its events drops the stand-ins and loads the
//! package with the host's loader, once. A trigger that arrives while that
//! mount runs finds nothing waiting.
//!
//! | Event | Trigger |
//! | --- | --- |
//! | `command:<id>` | running the declared command |
//! | `file:<ext>` | [`PluginContext::activate_for_file`] |
//! | `effect:<id>`, `generator:<id>`, `transition:<id>` | [`PluginContext::project_activations`] |
//! | `panel:<id>` | opening the declared panel |
//! | `setting:<key>` | [`PluginContext::activate_setting`] |

use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aria_models::Project;
use parking_lot::Mutex;

use crate::commands::CommandError;
use crate::context::PluginContext;
use crate::package::{Package, register_declared};
use crate::panels::PanelSpec;
use crate::registration::Registration;

/// One event of a package's `[activation] on` list.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Activation {
    /// Load at start.
    Startup,
    /// Running the declared command with this id.
    Command(String),
    /// Opening, importing or dropping a file with this extension. Lowercase,
    /// without the dot.
    File(String),
    /// Opening a project that uses the effect with this id.
    Effect(String),
    /// Opening a project that uses the transition with this id.
    Transition(String),
    /// Opening a project that uses the generator with this id.
    Generator(String),
    /// Opening the declared panel with this id.
    Panel(String),
    /// Reading or writing the setting with this key.
    Setting(String),
}

/// The kinds of [`Activation`], for listing and matching every kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActivationKind {
    Startup,
    Command,
    File,
    Effect,
    Transition,
    Generator,
    Panel,
    Setting,
}

impl ActivationKind {
    /// Every kind, `Startup` first.
    pub const ALL: [Self; 8] = [
        Self::Startup,
        Self::Command,
        Self::File,
        Self::Effect,
        Self::Transition,
        Self::Generator,
        Self::Panel,
        Self::Setting,
    ];

    /// How `plugin.toml` spells the kind: `startup`, or the prefix its argument
    /// follows.
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::Command => "command:",
            Self::File => "file:",
            Self::Effect => "effect:",
            Self::Transition => "transition:",
            Self::Generator => "generator:",
            Self::Panel => "panel:",
            Self::Setting => "setting:",
        }
    }

    fn with(self, argument: String) -> Activation {
        match self {
            Self::Startup => Activation::Startup,
            Self::Command => Activation::Command(argument),
            Self::File => Activation::File(argument),
            Self::Effect => Activation::Effect(argument),
            Self::Transition => Activation::Transition(argument),
            Self::Generator => Activation::Generator(argument),
            Self::Panel => Activation::Panel(argument),
            Self::Setting => Activation::Setting(argument),
        }
    }

    /// Whether an event of this kind is matched against the open project.
    fn reads_project(self) -> bool {
        matches!(self, Self::Effect | Self::Transition | Self::Generator)
    }
}

impl Activation {
    /// Read one event as `plugin.toml` spells it. A file extension is kept
    /// lowercase without its dot, so `file:.CUBE` reads as `file:cube`.
    pub fn parse(event: &str) -> Result<Self, String> {
        if event == ActivationKind::Startup.spelling() {
            return Ok(Self::Startup);
        }
        for kind in ActivationKind::ALL {
            if kind == ActivationKind::Startup {
                continue;
            }
            let Some(argument) = event.strip_prefix(kind.spelling()) else {
                continue;
            };
            let argument = argument.trim();
            if argument.is_empty() {
                return Err(format!("activation event {event:?} names nothing"));
            }
            if kind == ActivationKind::File {
                return file_extension(argument).map(Self::File).ok_or_else(|| {
                    format!("activation event {event:?} names no file extension: write `file:cube`")
                });
            }
            return Ok(kind.with(argument.to_owned()));
        }
        let prefixes: Vec<&str> = ActivationKind::ALL
            .iter()
            .filter(|kind| **kind != ActivationKind::Startup)
            .map(|kind| kind.spelling())
            .collect();
        Err(format!(
            "activation event {event:?} is not `startup` or one of {}",
            prefixes.join(" ")
        ))
    }

    pub fn kind(&self) -> ActivationKind {
        match self {
            Self::Startup => ActivationKind::Startup,
            Self::Command(_) => ActivationKind::Command,
            Self::File(_) => ActivationKind::File,
            Self::Effect(_) => ActivationKind::Effect,
            Self::Transition(_) => ActivationKind::Transition,
            Self::Generator(_) => ActivationKind::Generator,
            Self::Panel(_) => ActivationKind::Panel,
            Self::Setting(_) => ActivationKind::Setting,
        }
    }

    /// The id, extension or key after the prefix; `None` for `startup`.
    pub fn argument(&self) -> Option<&str> {
        match self {
            Self::Startup => None,
            Self::Command(argument)
            | Self::File(argument)
            | Self::Effect(argument)
            | Self::Transition(argument)
            | Self::Generator(argument)
            | Self::Panel(argument)
            | Self::Setting(argument) => Some(argument),
        }
    }
}

impl fmt::Display for Activation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.kind().spelling())?;
        if let Some(argument) = self.argument() {
            f.write_str(argument)?;
        }
        Ok(())
    }
}

/// `cube` from `cube`, `.cube` or `.CUBE`; `None` for anything that is not one
/// extension.
fn file_extension(raw: &str) -> Option<String> {
    let extension = raw.strip_prefix('.').unwrap_or(raw);
    let plain = !extension.is_empty()
        && extension
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    plain.then(|| extension.to_ascii_lowercase())
}

/// A package waiting for its first activation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingPlugin {
    pub id: String,
    pub name: String,
    /// Its activation events, as declared.
    pub events: Vec<Activation>,
}

/// A panel a waiting package declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitingPanel {
    /// The declaring package's id.
    pub plugin: String,
    /// The declaring package's name.
    pub plugin_name: String,
    pub spec: Arc<PanelSpec>,
}

/// How one package's activation went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activated {
    pub id: String,
    pub name: String,
    /// `Err` holds why the package did not mount. The context's failure ledger
    /// holds it too.
    pub result: Result<(), String>,
}

/// What loads a deferred package: the host's own mount path for its tier.
type Loader = Arc<dyn Fn(&PluginContext) -> Result<(), String> + Send + Sync>;

struct Waiting {
    id: String,
    name: String,
    events: Vec<Activation>,
    /// The package ids it requires, which load before it when they also wait.
    requires: Vec<String>,
    panels: Vec<Arc<PanelSpec>>,
    /// The commands, menu rows, keys and settings registered in its place.
    /// Dropped before it mounts, so its own registrations of the same ids are
    /// not refused.
    stand_ins: Vec<Registration>,
    load: Loader,
}

/// The packages waiting on one context.
#[derive(Default)]
pub(crate) struct Activations {
    waiting: Mutex<Vec<Waiting>>,
    /// Moves whenever a package starts or stops waiting.
    revision: AtomicU64,
    /// The project revision and waiting revision at which
    /// [`PluginContext::project_activations`] last found nothing to load.
    settled: Mutex<Option<(u64, u64)>>,
}

impl Activations {
    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
    }
}

/// Take every waiting package an event in `matches` names, with the waiting
/// packages they require, requirements first.
fn take(waiting: &mut Vec<Waiting>, matches: &impl Fn(&Activation) -> bool) -> Vec<Waiting> {
    let mut wanted: Vec<String> = waiting
        .iter()
        .filter(|entry| entry.events.iter().any(matches))
        .map(|entry| entry.id.clone())
        .collect();
    let mut next = 0;
    while next < wanted.len() {
        if let Some(entry) = waiting.iter().find(|entry| entry.id == wanted[next]) {
            for required in &entry.requires {
                if !wanted.contains(required) && waiting.iter().any(|held| held.id == *required) {
                    wanted.push(required.clone());
                }
            }
        }
        next += 1;
    }
    let (mut taken, kept): (Vec<Waiting>, Vec<Waiting>) = std::mem::take(waiting)
        .into_iter()
        .partition(|entry| wanted.contains(&entry.id));
    *waiting = kept;
    // Each pass emits the packages whose taken requirements are already out. A
    // cycle emits the rest as they are; their mounts fail on the requirement.
    let mut ordered = Vec::with_capacity(taken.len());
    while !taken.is_empty() {
        let ready = taken.iter().position(|entry| {
            entry
                .requires
                .iter()
                .all(|required| !taken.iter().any(|other| other.id == *required))
        });
        ordered.push(taken.remove(ready.unwrap_or(0)));
    }
    ordered
}

/// Every effect, transition and generator event in `wanted` that `project`
/// uses, once each. An effect instance matches both `effect:` and
/// `generator:`, since a generator is an effect that draws its own picture.
fn used_in(project: &Project, wanted: &[Activation]) -> Vec<Activation> {
    let mut found: Vec<Activation> = Vec::new();
    let mut note = |kinds: &[ActivationKind], id: &str| {
        for event in wanted {
            if kinds.contains(&event.kind())
                && event.argument() == Some(id)
                && !found.contains(event)
            {
                found.push(event.clone());
            }
        }
    };
    let effect = [ActivationKind::Effect, ActivationKind::Generator];
    let transition = [ActivationKind::Transition];
    let timelines = std::iter::once(project.timeline())
        .chain(project.sequences().map(|sequence| &sequence.timeline));
    for timeline in timelines {
        for insert in timeline.master_audio_effects() {
            note(&effect, &insert.effect_id);
        }
        for track in timeline.tracks_ordered() {
            for insert in &track.audio_effects {
                note(&effect, &insert.effect_id);
            }
            for junction in track.transitions() {
                note(&transition, &junction.transition_id);
            }
            for clip in track.clips() {
                for instance in &clip.effects {
                    note(&effect, &instance.effect_id);
                }
                for insert in &clip.audio_effects {
                    note(&effect, &insert.effect_id);
                }
            }
        }
    }
    found
}

thread_local! {
    /// The declared command whose stand-in is running its package's command on
    /// this thread.
    static FORWARDING: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Run declared command `command` of `plugin` once its package has loaded.
///
/// A stand-in that another thread's activation took but has not dropped yet
/// still answers to `command`; that second entry fails instead of recursing.
fn run_loaded(ctx: &PluginContext, plugin: &str, command: &str) -> Result<(), String> {
    if FORWARDING.with_borrow(|forwarding| forwarding.as_deref() == Some(command)) {
        return Err(format!("{plugin} is still loading; run {command} again"));
    }
    for activated in ctx.activate(&Activation::Command(command.to_owned())) {
        activated
            .result
            .map_err(|reason| format!("{} did not load: {reason}", activated.name))?;
    }
    let outer = FORWARDING.replace(Some(command.to_owned()));
    let ran = ctx.run_command(command);
    FORWARDING.set(outer);
    match ran {
        Ok(()) => Ok(()),
        Err(CommandError::Unknown(_)) => Err(format!("{plugin} did not register {command}")),
        Err(err) => Err(err.to_string()),
    }
}

impl PluginContext {
    /// Hold `package` unloaded until the first of its activation events, then
    /// mount it with `load`.
    ///
    /// What it declares is registered in its place, on the view its manifest
    /// describes: its WGSL effects, generators and transitions, menu rows,
    /// keys, settings section and presets as they are, and each declared
    /// command as a stand-in that loads the package, then runs the command
    /// the package registered under that id. A package already waiting under
    /// the same id is replaced. Refused, with nothing held, when a
    /// registration is refused, while a plugin is mounting, or when the
    /// package [loads at start](crate::package::PackageManifest::loads_at_start).
    pub fn defer_package(
        &self,
        package: &Package,
        load: impl Fn(&PluginContext) -> Result<(), String> + Send + Sync + 'static,
    ) -> Result<(), String> {
        let manifest = &package.manifest;
        if manifest.loads_at_start() {
            return Err(format!(
                "{} was not deferred: it loads at start",
                manifest.id
            ));
        }
        if self.mounting() {
            return Err(format!(
                "{} was not deferred: another plugin is mounting",
                manifest.id
            ));
        }
        self.cancel_activation(&manifest.id);
        let view = self.view(&package.plugin_manifest());
        let mut stand_ins = register_declared(&view, package).map_err(|err| err.to_string())?;
        stand_ins.reserve(manifest.commands.len());
        for command in &manifest.commands {
            let spec = package.command_spec(command.id.clone(), command.title.clone());
            let id = command.id.clone();
            let name = manifest.name.clone();
            let held = view
                .register_command(spec, move |ctx: &PluginContext| run_loaded(ctx, &name, &id))
                .map_err(|err| err.to_string())?;
            stand_ins.push(held);
        }
        let panels: Vec<Arc<PanelSpec>> = manifest
            .panels
            .iter()
            .map(|panel| {
                Arc::new(PanelSpec {
                    id: panel.id.clone(),
                    title: Arc::from(panel.title.as_str()),
                    location: panel.location,
                })
            })
            .collect();
        let has_panels = !panels.is_empty();
        let registry = self.activation_registry();
        registry.waiting.lock().push(Waiting {
            id: manifest.id.clone(),
            name: manifest.name.clone(),
            events: manifest.activation.clone(),
            requires: manifest
                .requires
                .iter()
                .map(|requirement| requirement.id.clone())
                .collect(),
            panels,
            stand_ins,
            load: Arc::new(load),
        });
        self.waiting_changed(has_panels);
        Ok(())
    }

    /// Mount every package waiting for `event`, after the waiting packages it
    /// requires, and return how each went. Empty when none waits for it.
    pub fn activate(&self, event: &Activation) -> Vec<Activated> {
        self.activate_matching(|waiting| waiting == event)
    }

    /// [`activate`](Self::activate) for `file:` events naming `path`'s
    /// extension, compared without case. Call it before a file is opened,
    /// imported or classified as a drop, so a package that reads it is loaded.
    pub fn activate_for_file(&self, path: &Path) -> Vec<Activated> {
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            return Vec::new();
        };
        self.activate_matching(|event| {
            matches!(event, Activation::File(wanted) if wanted.eq_ignore_ascii_case(extension))
        })
    }

    /// [`activate`](Self::activate) for the `setting:` event naming `key`.
    /// The host's settings store calls it on every read and write of a key.
    pub fn activate_setting(&self, key: &str) -> Vec<Activated> {
        self.activate_matching(
            |event| matches!(event, Activation::Setting(wanted) if wanted == key),
        )
    }

    fn activate_matching(&self, matches: impl Fn(&Activation) -> bool) -> Vec<Activated> {
        let registry = self.activation_registry();
        let taken = {
            let mut waiting = registry.waiting.lock();
            if !waiting
                .iter()
                .any(|entry| entry.events.iter().any(&matches))
            {
                return Vec::new();
            }
            // A mount cannot nest, so the package keeps waiting for the next
            // trigger rather than failing for the session.
            if self.mounting() {
                return waiting
                    .iter()
                    .filter(|entry| entry.events.iter().any(&matches))
                    .map(|entry| Activated {
                        id: entry.id.clone(),
                        name: entry.name.clone(),
                        result: Err("another plugin is mounting; try again".to_owned()),
                    })
                    .collect();
            }
            take(&mut waiting, &matches)
        };
        self.waiting_changed(taken.iter().any(|entry| !entry.panels.is_empty()));
        let host = self.scoped(None);
        taken
            .into_iter()
            .map(|entry| {
                let Waiting {
                    id,
                    name,
                    stand_ins,
                    load,
                    ..
                } = entry;
                drop(stand_ins);
                let result = load(&host);
                Activated { id, name, result }
            })
            .collect()
    }

    /// The effect, transition and generator events of waiting packages that
    /// `project` uses, for the caller to [`activate`](Self::activate) once it
    /// no longer holds the project. Call it before rendering.
    ///
    /// `revision` is the caller's revision of `project`. A call at the
    /// revision of a call that found nothing, with no package deferred or
    /// loaded since, returns at once.
    pub fn project_activations(&self, project: &Project, revision: u64) -> Vec<Activation> {
        let registry = self.activation_registry();
        let key = (revision, registry.revision.load(Ordering::Relaxed));
        if *registry.settled.lock() == Some(key) {
            return Vec::new();
        }
        let wanted: Vec<Activation> = registry
            .waiting
            .lock()
            .iter()
            .flat_map(|entry| entry.events.iter())
            .filter(|event| event.kind().reads_project())
            .cloned()
            .collect();
        let used = if wanted.is_empty() {
            wanted
        } else {
            used_in(project, &wanted)
        };
        if used.is_empty() {
            *registry.settled.lock() = Some(key);
        }
        used
    }

    /// Stop waiting for the package with `id` and drop its stand-ins. Returns
    /// whether it was waiting.
    pub fn cancel_activation(&self, id: &str) -> bool {
        let registry = self.activation_registry();
        // Dropped once the lock is released: a stand-in's drop takes the
        // command and settings registries' locks.
        let removed: Vec<Waiting> = {
            let mut waiting = registry.waiting.lock();
            let (removed, kept) = std::mem::take(&mut *waiting)
                .into_iter()
                .partition(|entry| entry.id == id);
            *waiting = kept;
            removed
        };
        let found = !removed.is_empty();
        let had_panels = removed.iter().any(|entry| !entry.panels.is_empty());
        drop(removed);
        if found {
            self.waiting_changed(had_panels);
        }
        found
    }

    /// Whether the package with `id` is waiting for an activation event.
    pub fn is_waiting(&self, id: &str) -> bool {
        self.activation_registry()
            .waiting
            .lock()
            .iter()
            .any(|entry| entry.id == id)
    }

    /// Every waiting package, in the order it was deferred.
    pub fn waiting_plugins(&self) -> Vec<WaitingPlugin> {
        self.activation_registry()
            .waiting
            .lock()
            .iter()
            .map(|entry| WaitingPlugin {
                id: entry.id.clone(),
                name: entry.name.clone(),
                events: entry.events.clone(),
            })
            .collect()
    }

    /// The panels waiting packages declare, in the order they were deferred.
    pub fn waiting_panels(&self) -> Vec<WaitingPanel> {
        self.activation_registry()
            .waiting
            .lock()
            .iter()
            .flat_map(|entry| {
                entry.panels.iter().map(|spec| WaitingPanel {
                    plugin: entry.id.clone(),
                    plugin_name: entry.name.clone(),
                    spec: Arc::clone(spec),
                })
            })
            .collect()
    }

    /// Moves whenever a package starts or stops waiting. A host caches what it
    /// draws from the waiting packages against this.
    pub fn activation_revision(&self) -> u64 {
        self.activation_registry().revision.load(Ordering::Relaxed)
    }

    /// Count a change to the waiting packages. When `panels`, the waiting
    /// panels changed too, so the panel revision moves and the host's panel
    /// waker runs: a host draws waiting panels beside registered ones. Called
    /// with no lock held.
    fn waiting_changed(&self, panels: bool) {
        self.activation_registry().changed();
        if panels {
            self.panel_registry().changed();
        }
    }
}

#[cfg(test)]
#[path = "activation_tests.rs"]
mod tests;
