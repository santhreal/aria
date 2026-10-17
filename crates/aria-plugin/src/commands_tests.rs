//! Tests for plugin commands.
//!
//! Closes the class "a palette command outlives, escapes, or deadlocks its
//! plugin": every case runs through `register_command` and `run_command`, the
//! two entry points every command passes. It does not cover how a host lists
//! the commands; the palette tests in `aria-ui` own that.

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::plugin::Plugin;

struct Clock(u32);

/// A plugin that registers one command built by `make`.
struct WithCommand {
    manifest: PluginManifest,
    make: fn() -> (
        CommandSpec,
        Box<dyn Fn(&PluginContext) -> Result<(), String> + Send + Sync>,
    ),
}

impl Plugin for WithCommand {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        let (spec, run) = (self.make)();
        ctx.register_command(spec, move |ctx: &PluginContext| run(ctx))?;
        Ok(())
    }
}

fn spec(id: &str, title: &str) -> CommandSpec {
    CommandSpec::new(id, title)
}

/// Reads the `Clock` service and fails with what it saw.
fn reads_clock() -> (
    CommandSpec,
    Box<dyn Fn(&PluginContext) -> Result<(), String> + Send + Sync>,
) {
    (
        spec("test.clock.read", "Read the clock"),
        Box::new(|ctx: &PluginContext| match ctx.try_get::<Clock>() {
            Some(clock) => Err(format!("clock reads {}", clock.0)),
            None => Err("no clock".to_owned()),
        }),
    )
}

fn panics() -> (
    CommandSpec,
    Box<dyn Fn(&PluginContext) -> Result<(), String> + Send + Sync>,
) {
    (
        spec("test.boom", "Boom"),
        Box::new(|_: &PluginContext| panic!("boom")),
    )
}

fn plugin(
    id: &'static str,
    uses: &'static [&'static str],
    make: fn() -> (
        CommandSpec,
        Box<dyn Fn(&PluginContext) -> Result<(), String> + Send + Sync>,
    ),
) -> WithCommand {
    WithCommand {
        manifest: PluginManifest::new(id, id, "0.1.0", &[], uses),
        make,
    }
}

#[test]
fn a_command_runs_through_its_plugins_view() {
    let ctx = PluginContext::new();
    let _clock = ctx.register(Clock(7));

    let declared = plugin("test.declared", &["Clock"], reads_clock);
    ctx.mount(&declared).expect("mounts");
    assert_eq!(
        ctx.commands()
            .iter()
            .map(|spec| (spec.id.as_str(), spec.title.as_str()))
            .collect::<Vec<_>>(),
        [("test.clock.read", "Read the clock")]
    );
    assert_eq!(
        ctx.run_command("test.clock.read"),
        Err(CommandError::Failed("clock reads 7".into()))
    );
    ctx.unmount("test.declared").expect("unmounts");

    // The same command from a plugin that did not declare the service sees
    // none, though the host's context has one.
    let undeclared = plugin("test.undeclared", &[], reads_clock);
    ctx.mount(&undeclared).expect("mounts");
    assert_eq!(
        ctx.run_command("test.clock.read"),
        Err(CommandError::Failed("no clock".into()))
    );
    assert!(
        ctx.denials()
            .iter()
            .any(|denial| denial.plugin == "test.undeclared"),
        "{:?}",
        ctx.denials()
    );
}

#[test]
fn a_command_leaves_with_its_plugin() {
    let ctx = PluginContext::new();
    let declared = plugin("test.leaving", &[], reads_clock);
    ctx.mount(&declared).expect("mounts");
    assert_eq!(ctx.commands().len(), 1);

    ctx.unmount("test.leaving").expect("unmounts");
    assert!(ctx.commands().is_empty());
    assert_eq!(
        ctx.run_command("test.clock.read"),
        Err(CommandError::Unknown("test.clock.read".into()))
    );

    // Mounted again, it registers again: the id was released.
    ctx.mount(&declared).expect("mounts again");
    assert_eq!(ctx.commands().len(), 1);
}

#[test]
fn a_panicking_command_fails_its_plugin() {
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.panicky", &[], panics))
        .expect("mounts");

    assert_eq!(
        ctx.run_command("test.boom"),
        Err(CommandError::Panicked("boom".into()))
    );
    assert!(!ctx.is_mounted("test.panicky"));
    assert!(
        ctx.commands().is_empty(),
        "the failed plugin's command stays"
    );
    let failures = ctx.failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0].id, "test.panicky");
    assert_eq!(failures[0].reason, "command test.boom panicked: boom");
}

#[test]
fn a_host_command_panics_without_failing_a_plugin() {
    let ctx = PluginContext::new();
    let _held = ctx
        .register_command(
            spec("host.boom", "Boom"),
            |_: &PluginContext| -> Result<(), String> { panic!("host boom") },
        )
        .expect("registers");
    assert_eq!(
        ctx.run_command("host.boom"),
        Err(CommandError::Panicked("host boom".into()))
    );
    assert!(ctx.failures().is_empty());
    assert_eq!(ctx.commands().len(), 1, "a host command stays registered");
}

#[test]
fn malformed_and_duplicate_commands_are_refused() {
    let ctx = PluginContext::new();
    let ok = |_: &PluginContext| -> Result<(), String> { Ok(()) };
    let _first = ctx
        .register_command(spec("host.one", "One"), ok)
        .expect("registers");
    for (spec, want) in [
        (
            spec("", "Untitled"),
            "command refused: a command needs an id",
        ),
        (
            spec("  ", "Untitled"),
            "command refused: a command needs an id",
        ),
        (
            spec("host.two", ""),
            "command refused: host.two: a command needs a title",
        ),
        (
            spec("host.one", "Again"),
            "command refused: a command is registered as host.one",
        ),
    ] {
        let err = ctx.register_command(spec.clone(), ok).expect_err("refused");
        assert_eq!(err.to_string(), want, "{spec:?}");
    }
    assert_eq!(ctx.commands().len(), 1);
    assert_eq!(ctx.run_command("host.one"), Ok(()));
}

/// A refused command inside a mount fails the mount and releases what the
/// plugin registered before it.
#[test]
fn a_refused_command_fails_the_mount() {
    let ctx = PluginContext::new();
    let _taken = ctx
        .register_command(spec("test.clock.read", "Taken"), |_: &PluginContext| Ok(()))
        .expect("registers");
    let plugin = plugin("test.late", &[], reads_clock);
    assert!(ctx.mount(&plugin).is_err());
    assert!(!ctx.is_mounted("test.late"));
    assert_eq!(
        ctx.commands()
            .iter()
            .map(|spec| spec.title.as_str())
            .collect::<Vec<_>>(),
        ["Taken"]
    );
}

/// A command runs with no registry lock held: one that registers another
/// command and runs it completes, rather than deadlocking on the lock its
/// own lookup took.
#[test]
fn a_command_may_register_and_run_commands() {
    let ctx = PluginContext::new();
    let _outer = ctx
        .register_command(spec("host.outer", "Outer"), |ctx: &PluginContext| {
            let inner = ctx
                .register_command(spec("host.inner", "Inner"), |_: &PluginContext| {
                    Err("inner ran".to_owned())
                })
                .map_err(|err| err.to_string())?;
            let ran = ctx.run_command("host.inner");
            drop(inner);
            match ran {
                Err(CommandError::Failed(reason)) => Err(reason),
                other => Err(format!("unexpected {other:?}")),
            }
        })
        .expect("registers");

    let (sent, received) = mpsc::channel();
    let runner = ctx.clone();
    std::thread::spawn(move || {
        let _ = sent.send(runner.run_command("host.outer"));
    });
    let ran = received
        .recv_timeout(Duration::from_secs(10))
        .expect("the command finished rather than deadlocking");
    assert_eq!(ran, Err(CommandError::Failed("inner ran".into())));
    assert_eq!(ctx.commands().len(), 1, "the inner registration dropped");
}

fn place(command: &str, path: &str) -> crate::menus::MenuCommand {
    crate::menus::MenuCommand {
        command: command.to_owned(),
        path: crate::menus::MenuPath::parse(path).expect("a menu path"),
    }
}

fn key(command: &str, chord: crate::keys::Chord, route: crate::keys::KeyRoute) -> KeyDefault {
    KeyDefault {
        command: command.to_owned(),
        chord,
        route,
    }
}

/// Menu rows and key defaults are listed in registration order, move the
/// revision a host caches against on every change, and leave when their
/// registration drops. A dropped row removes only itself, not an equal row
/// registered since.
#[test]
fn menu_rows_and_keys_list_in_order_and_leave_with_their_registration() {
    use crate::keys::{Chord, KeyRoute};
    let ctx = PluginContext::new();
    let start = ctx.command_revision();
    let first = ctx
        .register_menu_entry(place("host.one", "Sequence/Tools"))
        .expect("registers");
    let second = ctx
        .register_menu_entry(place("host.one", "Clip"))
        .expect("the same command in another place");
    let chord = ctx
        .register_key_default(key("host.one", Chord::ctrl_alt("l"), KeyRoute::Editor))
        .expect("registers");
    let home = ctx
        .register_key_default(key("host.one", Chord::ctrl_alt("l"), KeyRoute::Home))
        .expect("another route is another default");
    assert_eq!(ctx.command_revision(), start + 4);
    assert_eq!(
        ctx.menu_entries()
            .iter()
            .map(|entry| entry.path.to_string())
            .collect::<Vec<_>>(),
        ["Sequence/Tools", "Clip"]
    );
    assert_eq!(
        first.contribution().map(ToString::to_string).as_deref(),
        Some("menu item host.one in Sequence/Tools")
    );
    assert_eq!(
        chord.contribution().map(ToString::to_string).as_deref(),
        Some("key Ctrl+Alt+L for host.one")
    );
    drop(first);
    let again = ctx
        .register_menu_entry(place("host.one", "Sequence/Tools"))
        .expect("free again once dropped");
    drop(second);
    assert_eq!(
        ctx.menu_entries()
            .iter()
            .map(|entry| entry.path.to_string())
            .collect::<Vec<_>>(),
        ["Sequence/Tools"],
        "dropping the Clip row removed the row registered since"
    );
    drop((again, chord));
    assert_eq!(
        ctx.key_defaults()
            .iter()
            .map(|key| key.route)
            .collect::<Vec<_>>(),
        [KeyRoute::Home]
    );
    drop(home);
    assert!(ctx.menu_entries().is_empty() && ctx.key_defaults().is_empty());
    assert_eq!(ctx.command_revision(), start + 10);
}

#[test]
fn malformed_and_duplicate_menu_rows_and_keys_are_refused() {
    use crate::keys::{Chord, KeyRoute};
    let ctx = PluginContext::new();
    let _row = ctx
        .register_menu_entry(place("host.one", "File"))
        .expect("registers");
    let _key = ctx
        .register_key_default(key("host.one", Chord::ctrl("k"), KeyRoute::Editor))
        .expect("registers");
    let refusals = [
        (
            ctx.register_menu_entry(place(" ", "File")),
            "command refused: a menu entry in File names no command",
        ),
        (
            ctx.register_menu_entry(place("host.one", "file")),
            "command refused: host.one is already in File",
        ),
        (
            ctx.register_key_default(key("", Chord::plain("k"), KeyRoute::Editor)),
            "command refused: the key K names no command",
        ),
        (
            ctx.register_key_default(key("host.one", Chord::plain("j"), KeyRoute::Editor)),
            "command refused: host.one already has the Editor key Ctrl+K",
        ),
    ];
    for (result, want) in refusals {
        assert_eq!(
            result.map(drop).map_err(|err| err.to_string()),
            Err(want.to_owned())
        );
    }
    assert_eq!(ctx.menu_entries().len(), 1);
    assert_eq!(ctx.key_defaults().len(), 1);
}

/// Closes the class of a command that runs while its enablement condition is
/// false, or whose condition reads keys other than the ones last published.
#[test]
fn a_disabled_command_is_refused_until_its_condition_holds() {
    use crate::ContextKey::{Playing, Selection};

    let ctx = PluginContext::new();
    let runs = Arc::new(AtomicU32::new(0));
    let counted = Arc::clone(&runs);
    let mut gated = spec("host.gated", "Gated");
    gated.enablement = Some(Enablement::parse("selection && !playing").expect("parses"));
    let _held = ctx
        .register_command(gated, move |_: &PluginContext| {
            counted.fetch_add(1, Ordering::Relaxed);
            Ok(())
        })
        .expect("registers");

    assert_eq!(
        ctx.context_keys(),
        ContextKeys::NONE,
        "nothing is published yet"
    );
    let selected = ContextKeys::NONE.with(Selection, true);
    let cases = [
        (ContextKeys::NONE, false),
        (selected, true),
        (selected.with(Playing, true), false),
        (ContextKeys::NONE.with(Playing, true), false),
        (selected, true),
    ];
    for (keys, enabled) in cases {
        ctx.set_context_keys(keys);
        assert_eq!(ctx.context_keys(), keys);
        assert_eq!(ctx.commands()[0].enabled(keys), enabled, "{keys:?}");
        let before = runs.load(Ordering::Relaxed);
        let result = ctx.run_command("host.gated");
        if enabled {
            assert_eq!(result, Ok(()), "{keys:?}");
            assert_eq!(runs.load(Ordering::Relaxed), before + 1, "{keys:?}");
        } else {
            assert_eq!(
                result,
                Err(CommandError::Disabled("selection && !playing".into())),
                "{keys:?}"
            );
            assert_eq!(
                runs.load(Ordering::Relaxed),
                before,
                "a refused command ran"
            );
        }
    }
    assert_eq!(
        CommandError::Disabled("selection".into()).to_string(),
        "needs selection"
    );
}

/// A plugin's view of the context reads the keys the host published on it.
#[test]
fn a_plugins_view_reads_the_published_keys() {
    fn reads_keys() -> (
        CommandSpec,
        Box<dyn Fn(&PluginContext) -> Result<(), String> + Send + Sync>,
    ) {
        (
            spec("test.keys.read", "Read the keys"),
            Box::new(|ctx: &PluginContext| {
                Err(format!(
                    "in-out {}",
                    ctx.context_keys().has(crate::ContextKey::InOut)
                ))
            }),
        )
    }
    let ctx = PluginContext::new();
    ctx.mount(&plugin("test.keys", &[], reads_keys))
        .expect("mounts");
    assert_eq!(
        ctx.run_command("test.keys.read"),
        Err(CommandError::Failed("in-out false".into()))
    );
    ctx.set_context_keys(ContextKeys::NONE.with(crate::ContextKey::InOut, true));
    assert_eq!(
        ctx.run_command("test.keys.read"),
        Err(CommandError::Failed("in-out true".into()))
    );
}
