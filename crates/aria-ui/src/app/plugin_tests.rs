//! Contracts for the Plugins list.
//!
//! The class closed here is "the plugin host is invisible": a plugin that did not
//! mount was a log line, a mounted plugin could not be named, and nothing could be
//! turned off. Rows are enumerated from `crate::plugins::builtin()` at run time, so
//! a plugin added there is exercised without a second list here.

use std::path::Path;

use aria_plugin::PluginContext;

use super::AppState;
use crate::plugins::{builtin, builtin_plugin_ids, mount_all};

fn state() -> AppState {
    let ctx = PluginContext::new();
    // Storage and settings mount without a GPU; the rest may fail on a headless
    // machine, which is exactly the case the list has to describe.
    let dir = crate::scratch_pool::scratch("plugins");
    let _ = mount_all(&ctx, &dir);
    AppState::new(ctx, dir).expect("state")
}

#[test]
fn every_builtin_plugin_has_a_row_that_matches_the_host() {
    let state = state();
    let rows = state.plugin_rows();
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        builtin_plugin_ids(),
        "the list is not the built-in set, in mount order"
    );
    assert_eq!(rows.len(), builtin(&state.data_dir).len());

    let mounted = state.ctx.plugins();
    for row in &rows {
        assert!(!row.name.is_empty(), "{} has no name", row.id);
        assert!(!row.version.is_empty(), "{} has no version", row.id);
        assert_eq!(
            row.enabled,
            mounted.iter().any(|id| *row.id == **id),
            "{} disagrees with the host about being mounted",
            row.id
        );
        // A row is either running or it explains itself.
        if !row.enabled {
            assert!(
                row.error.is_some(),
                "{} is not mounted and gives no reason",
                row.id
            );
        }
        // Service names are short, not fully qualified paths.
        for service in &row.services {
            assert!(
                !service.contains("::"),
                "{} lists a raw type path: {service}",
                row.id
            );
        }
    }

    // Storage mounts anywhere and provides a service, so the list is never empty
    // of real content even on a headless machine.
    let storage = rows
        .iter()
        .find(|row| row.id == "aria.storage")
        .expect("storage row");
    assert!(
        storage.enabled,
        "storage did not mount: {:?}",
        storage.error
    );
    assert!(
        storage
            .services
            .iter()
            .any(|name| name.contains("StorageService")),
        "storage lists no service: {:?}",
        storage.services
    );
}

#[test]
fn a_plugin_can_be_disabled_and_re_enabled() {
    let mut state = state();
    // Pick a mounted, non-required plugin from the list rather than naming one.
    let target = state
        .plugin_rows()
        .into_iter()
        .find(|row| row.enabled && state.plugin_can_toggle(&row.id))
        .map(|row| row.id);
    let Some(id) = target else {
        // A machine where only the required plugins mounted has nothing to toggle;
        // the required ones must still refuse.
        assert!(!state.plugin_can_toggle("aria.storage"));
        return;
    };

    assert!(state.set_plugin_enabled(&id, false), "disable did nothing");
    let row = state
        .plugin_rows()
        .into_iter()
        .find(|row| row.id == id)
        .expect("row survives disabling");
    assert!(!row.enabled, "{id} still reads as mounted");
    assert!(
        row.services.is_empty(),
        "{id} still lists services while disabled"
    );
    assert!(!state.ctx.plugins().iter().any(|live| *id == **live));

    // Disabling twice is inert.
    assert!(!state.set_plugin_enabled(&id, false));

    assert!(state.set_plugin_enabled(&id, true), "enable did nothing");
    let row = state
        .plugin_rows()
        .into_iter()
        .find(|row| row.id == id)
        .expect("row after enabling");
    assert!(row.enabled, "{id} did not come back");
    assert!(row.error.is_none(), "{id} came back with an error");
}

#[test]
fn the_plugins_the_shell_needs_cannot_be_turned_off() {
    let mut state = state();
    for id in ["aria.storage", "aria.settings", "aria.engine"] {
        assert!(!state.plugin_can_toggle(id), "{id} is offered a toggle");
        assert!(!state.set_plugin_enabled(id, false), "{id} was turned off");
        assert!(
            state
                .status_message
                .as_deref()
                .unwrap_or_default()
                .contains(id),
            "the refusal did not name {id}: {:?}",
            state.status_message
        );
    }
    // An id that is not a built-in plugin at all is refused, not mounted.
    assert!(!state.set_plugin_enabled("not.a.plugin", true));
}

#[test]
fn a_failed_mount_is_named_with_its_reason_and_the_app_still_runs() {
    // A context with nothing mounted: every plugin with a dependency fails, and
    // `mount_all` must return those failures rather than stopping at the first.
    let ctx = PluginContext::new();
    let dir = crate::scratch_pool::scratch("plugin-fail");
    let failures = mount_all(&ctx, &dir);
    let state = AppState::new(ctx, dir).expect("state");

    // Whatever the machine, the app started and the list is complete.
    assert_eq!(state.plugin_rows().len(), builtin(&state.data_dir).len());
    for failure in &failures {
        let row = state
            .plugin_rows()
            .into_iter()
            .find(|row| row.id == failure.id)
            .expect("a failure names a plugin in the list");
        assert!(!row.enabled);
        assert_eq!(row.error.as_deref(), Some(failure.reason.as_str()));
        assert!(!failure.reason.is_empty(), "{} failed silently", failure.id);
    }
    // The ledger and the rows agree on how many are down.
    assert_eq!(
        state
            .plugin_rows()
            .iter()
            .filter(|row| row.error.is_some())
            .count(),
        failures.len()
    );
}

/// A fault the host recovered from shows on the plugin's row while it stays
/// mounted. Failing the plugin after mount turns the row into a failure with
/// the reason, and the fault goes with it.
#[test]
fn a_recovered_fault_and_a_failure_after_mount_show_on_the_row() {
    let state = state();
    let id = "aria.storage";
    let row = |state: &AppState| {
        state
            .plugin_rows()
            .into_iter()
            .find(|row| row.id == id)
            .expect("a row")
    };
    state
        .ctx
        .report_fault(id, "an audio effect missed its deadlines");
    let mounted = row(&state);
    assert!(mounted.enabled && mounted.error.is_none());
    assert_eq!(
        mounted.fault.as_deref(),
        Some("an audio effect missed its deadlines")
    );

    assert!(state.ctx.fail(id, "a callback panicked"));
    let failed = row(&state);
    assert!(!failed.enabled);
    assert_eq!(failed.error.as_deref(), Some("a callback panicked"));
    assert_eq!(failed.fault, None);
}

/// WHY: a plugin's log is read in the Plugins list. The row counts the lines a
/// plugin logged, opens its log by default once the plugin has an error or a
/// fault, keeps the Log toggle's choice over that default, and Copy holds every
/// line with its level. Not covered: the drawn colours.
#[test]
fn a_row_counts_its_log_and_opens_it_on_a_failure_until_toggled() {
    let mut state = state();
    let id = "aria.storage";
    let row = |state: &AppState| {
        state
            .plugin_rows()
            .into_iter()
            .find(|row| row.id == id)
            .expect("a row")
    };
    let quiet = row(&state);
    assert_eq!((quiet.log_lines, quiet.log_open), (0, false));

    state
        .ctx
        .log_for(id, aria_plugin::LogLevel::Info, "indexed 3 projects");
    state.ctx.report_fault(id, "the index is stale");
    let faulted = row(&state);
    assert_eq!((faulted.log_lines, faulted.log_open), (2, true));
    assert_eq!(
        state.plugin_log_text(id),
        "info indexed 3 projects\nwarn the index is stale\n"
    );

    state.toggle_plugin_log(id);
    assert!(
        !row(&state).log_open,
        "the toggle closes a log opened by default"
    );
    assert!(state.ctx.fail(id, "a callback panicked"));
    let failed = row(&state);
    assert_eq!(failed.log_lines, 3);
    assert!(
        !failed.log_open,
        "the toggle's choice outlasts a new failure"
    );
    state.toggle_plugin_log(id);
    assert!(row(&state).log_open);
}

/// A plugin that registers a value nobody reads is a plugin that does not own it.
/// These two services were exactly that, so the tests pin the consumer, not the
/// registration.
#[test]
fn the_analysis_run_is_tuned_by_the_plugin_not_by_the_call_site() {
    use crate::plugins::DetectorTuning;

    let ctx = PluginContext::new();
    let _ = mount_all(&ctx, &crate::scratch_pool::scratch("plugin-tuning"));
    // A tuning that is plainly not the default, so a request carrying the default
    // would fail rather than pass by coincidence.
    let tuning = DetectorTuning {
        shots: aria_analysis::ShotDetectionConfig::new(
            128,
            72,
            32,
            aria_analysis::ShotScoreWeights::default(),
            0.75,
            2.5,
        )
        .expect("a legal shot config"),
        ..DetectorTuning::default()
    };
    assert_ne!(tuning, DetectorTuning::default());
    let _reg = ctx.register(tuning);
    let mut state = AppState::new(ctx, crate::scratch_pool::scratch("tuning")).expect("state");
    state.new_project().expect("new project");
    let media = state
        .import_media(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/texture.png"),
        )
        .expect("import fixture");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place clip");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| aria_models::ClipId::from_raw(clip.id))
        .next()
        .expect("one clip");
    state.select_clip(clip);

    let request = state
        .analysis_precheck(super::analysis::AnalysisKind::Shots)
        .expect("a still can be analyzed for shots");
    assert_eq!(
        request.tuning, tuning,
        "the run would have used the crate defaults instead of the plugin's tuning"
    );
}

#[test]
fn the_assistant_limits_come_from_the_plugin_and_say_so_when_it_is_gone() {
    let mut state = state();
    let Some(host) = state.ctx.try_get::<crate::plugins::AiHost>() else {
        // The AI plugin needs the engine, which needs a GPU. Where it did not
        // mount, the row must say so rather than print the crate defaults.
        assert_eq!(state.agent_limits(), "The AI plugin is not mounted");
        return;
    };
    let config = host.agent_config().clone();
    let limits = state.agent_limits();
    assert!(
        limits.contains(&config.max_tool_calls.to_string())
            && limits.contains(&config.max_turns.to_string()),
        "the limits row does not report the plugin's caps: {limits}"
    );

    drop(host);
    assert!(state.set_plugin_enabled("aria.ai", false), "disable the AI");
    assert_eq!(state.agent_limits(), "The AI plugin is not mounted");
}

/// Catalogs are data a user can replace, so the app must be able to say which
/// set is in effect. Mounting reads every catalog, so the list is complete
/// before any panel has opened one, and on a clean machine none is refused.
#[test]
fn every_catalog_is_listed_with_its_source() {
    let state = state();
    let rows = state.catalog_rows();
    for name in [
        "transitions",
        "effects",
        "filters",
        "animations",
        "text_effects",
        "easings",
        "speed_presets",
    ] {
        let row = rows
            .iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("{name} is missing from the Catalogs list"));
        assert!(row.entries > 0, "{name} is listed as empty");
        assert_eq!(row.problem, None, "{name} was refused on a clean machine");
    }
    assert!(
        state.settings_section_visible("Catalogs"),
        "the section that carries the rows is hidden"
    );
}

/// A plugin reads only what its manifest declares, so a built-in that forgot a
/// declaration loses a capability in silence — the surface whose service is gone
/// just hides. The mount records every refusal, so the whole class is one
/// assertion: after a full mount, nothing was refused.
#[test]
fn no_builtin_is_refused_a_service_it_asks_for() {
    let state = state();
    let denials = state.ctx.denials();
    assert!(
        denials.is_empty(),
        "a built-in asks for a service its manifest does not declare: {denials:?}"
    );
    for row in state.plugin_rows() {
        assert!(
            row.denied.is_empty(),
            "{} was refused {:?}",
            row.id,
            row.denied
        );
    }
}

/// Mount order is derived from the declared dependencies, and it decides
/// event-handler order. It must be the order the built-in list gives, so a
/// dependency edit cannot silently reshuffle which plugin vetoes a command first.
#[test]
fn the_builtin_mount_order_is_the_declared_order() {
    let plugins = builtin(Path::new(""));
    let manifests: Vec<_> = plugins.iter().map(|p| p.manifest().clone()).collect();
    let order = aria_plugin::load_order(&manifests).expect("the built-ins are acyclic");
    assert_eq!(order, builtin_plugin_ids());
}

/// The palette runs whatever the registry holds, so Enter on a filtered query has
/// to reach the shipped command through the mounted host — not through the table
/// it was declared in.
#[test]
fn the_palette_runs_a_registered_command_from_its_query() {
    let mut state = state();
    // Home refuses every dialog but Settings, and a fresh state starts there.
    state.new_project().expect("a project to edit");
    assert_eq!(state.dialog, crate::app::DialogKind::None);
    state.palette_query = "Jobs".into();
    assert!(
        state.run_palette_highlight().is_none(),
        "a registered command runs without the entity context"
    );
    assert_eq!(
        state.dialog,
        crate::app::DialogKind::Jobs,
        "the palette did not reach the registered command"
    );
}

/// With no host mounted there is nothing to offer, and asking must be a no-op
/// rather than a panic: a headless `AppState` has no palette.
#[test]
fn with_no_command_host_the_palette_offers_nothing() {
    let mut state = AppState::new(
        PluginContext::new(),
        crate::scratch_pool::scratch("pal-bare"),
    )
    .expect("state");
    state.new_project().expect("a project to edit");
    state.palette_query = "Jobs".into();
    assert!(state.run_palette_highlight().is_none());
    assert_eq!(state.dialog, crate::app::DialogKind::None);
}

/// The viewer and Home are slots several entries share, so the predicate is what
/// decides which body draws. Two visible at once would stack the assistant over
/// the preview; none would leave the slot blank.
#[test]
fn a_body_slot_shows_exactly_the_entry_whose_predicate_holds() {
    use aria_plugin::PanelLocation;

    let mut state = state();
    let host =
        crate::plugins::panel_host::host_of(&state.ctx).expect("mount registers the panel host");

    assert_eq!(state.route, crate::types::UiRoute::Home);
    assert_eq!(
        host.visible_at(PanelLocation::Home, &state).len(),
        1,
        "the Home route must draw its body"
    );

    state.new_project().expect("a project to edit");
    assert_eq!(state.route, crate::types::UiRoute::Editor);
    assert!(
        host.visible_at(PanelLocation::Home, &state).is_empty(),
        "the Home body must not draw over the editor"
    );

    // The viewer is one rectangle with several claimants; whichever pane has
    // focus, exactly one body draws in it. Enumerated from `ActivePanel::ALL`
    // rather than a list here, so a pane added to the enum is covered by
    // construction — and one that claims the viewer without a panel row shows
    // up as zero bodies.
    assert_eq!(host.at(PanelLocation::Viewer).len(), 3);
    for panel in crate::types::ActivePanel::ALL {
        state.active_panel = panel;
        assert_eq!(
            host.visible_at(PanelLocation::Viewer, &state).len(),
            1,
            "{panel:?} must select exactly one viewer body"
        );
    }
}

/// WHY: `AppState::plugin_rows` must leave `frame_ms` and `block_ms` as `None`
/// when a mounted plugin has not rendered a frame or processed an audio block,
/// populate both mean and max fields from `PluginContext`'s cost ledger once
/// work is recorded without mixing plugins, and clear them when a plugin is
/// disabled and re-enabled.
#[test]
fn plugin_rows_carry_recorded_frame_and_block_costs_and_reset_on_disable() {
    use std::time::Duration;

    let mut state = state();
    let row_of = |state: &AppState, id: &str| {
        state
            .plugin_rows()
            .into_iter()
            .find(|row| row.id == id)
            .unwrap_or_else(|| panic!("missing row for {id}"))
    };

    let before = row_of(&state, "aria.storage");
    assert!(before.mount_ms.is_some());
    assert_eq!(before.frame_ms, None);
    assert_eq!(before.frame_max_ms, None);
    assert_eq!(before.block_ms, None);
    assert_eq!(before.block_max_ms, None);

    state
        .ctx
        .record_frame_cost("aria.storage", Duration::from_millis(2));
    state
        .ctx
        .record_frame_cost("aria.storage", Duration::from_millis(10));
    state
        .ctx
        .record_block_cost("aria.storage", Duration::from_micros(40));

    let after = row_of(&state, "aria.storage");
    let frame_ms = after.frame_ms.expect("frame_ms populated");
    let frame_max_ms = after.frame_max_ms.expect("frame_max_ms populated");
    let block_ms = after.block_ms.expect("block_ms populated");
    let block_max_ms = after.block_max_ms.expect("block_max_ms populated");
    assert!(
        (frame_ms - 3.0).abs() < 0.05,
        "expected smoothed frame_ms ~3.0 ms, got {frame_ms}"
    );
    assert!(
        (frame_max_ms - 10.0).abs() < 0.05,
        "expected frame_max_ms ~10.0 ms, got {frame_max_ms}"
    );
    assert!(
        (block_ms - 0.04).abs() < 0.005,
        "expected block_ms ~0.04 ms, got {block_ms}"
    );
    assert!(
        (block_max_ms - 0.04).abs() < 0.005,
        "expected block_max_ms ~0.04 ms, got {block_max_ms}"
    );

    // Unrelated mounted plugins remain untouched.
    let settings_row = row_of(&state, "aria.settings");
    assert_eq!(settings_row.frame_ms, None);
    assert_eq!(settings_row.block_ms, None);

    // If a toggleable built-in is mounted, disabling and re-enabling it clears
    // its recorded costs.
    if let Some(toggle_id) = state
        .plugin_rows()
        .into_iter()
        .find(|row| row.enabled && state.plugin_can_toggle(&row.id))
        .map(|row| row.id)
    {
        state
            .ctx
            .record_frame_cost(&toggle_id, Duration::from_millis(6));
        assert!(row_of(&state, &toggle_id).frame_ms.is_some());
        assert!(state.set_plugin_enabled(&toggle_id, false));
        assert_eq!(row_of(&state, &toggle_id).frame_ms, None);
        assert!(state.set_plugin_enabled(&toggle_id, true));
        assert_eq!(row_of(&state, &toggle_id).frame_ms, None);
    }
}
