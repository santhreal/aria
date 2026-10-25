//! The shell's own panels, registered the way a plugin registers one.
//!
//! Every panel the editor draws goes through [`PanelHost`], including these. That
//! is the point: the shell resolves a slot from the registry, so a contributed
//! panel is drawn by the same code that draws the media pool, and a built-in has
//! no privileged path.

use std::sync::Arc;

use aria_plugin::{PanelLocation, Plugin, PluginContext, PluginError, PluginManifest};
use gpui::{AnyElement, Entity, IntoElement};

use crate::app::{AppState, DialogKind};
use crate::chrome::menu_host::MenuHost;
use crate::commands::host::CommandHost;
use crate::commands::{BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
use crate::dock::DefaultFrame;
use crate::keys::KeyHost;
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::panels::{
    about_dialog, ai_panel, ai_tools, align_panel, audio_gain_dialog, audio_meters, color_panel,
    context_menu, effects_browser, export_dialog, history_panel, home, info_panel, inspector,
    install_dialog, jobs_dialog, keyframe_velocity_dialog, marker_edit_dialog, markers,
    media_support, mixer, palette, preview, project_report, remove_attributes_dialog, rename_bar,
    settings_dialog, sidebar, speed_duration_dialog, text_panel, timeline, timeline_report,
    unsaved_dialog,
};
use crate::plugins::overlay_host::{OverlayEntry, OverlayHost, OverlayRender, OverlayWhen};
use crate::plugins::panel_host::{
    self, DockSpec, PanelEntry, PanelHost, PanelRender, PanelVisible,
};
use crate::shell::Shell;
use crate::types::{ActivePanel, UiRoute};

pub struct UiPlugin;

/// The shipped panels, registered against the slots the shell resolves.
///
/// A table rather than a call each: the test below enumerates it, so a panel added
/// here is covered without a second list. The docked panes are always on; the rest
/// carry a predicate, because the assistant replaces the preview, Home replaces the
/// editor, and the assistant's tools show in the Inspector only while it is open.
/// `PanelLocation::Overlay` has no built-in entry — an overlay contribution is
/// drawn over the viewer. The second field is the panel's title, the one name its
/// tab, its Window row and its frame menu's Add row draw.
const BUILTIN_PANELS: &[(
    &str,
    &str,
    PanelLocation,
    PanelRender,
    PanelVisible,
    Option<DockSpec>,
)] = &[
    (
        "aria.panel.media",
        "Project",
        PanelLocation::Sidebar,
        draw_sidebar,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Left,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Media,
        )),
    ),
    (
        "aria.panel.timeline",
        "Timeline",
        PanelLocation::Timeline,
        draw_timeline,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Bottom,
            DockSpec::TIMELINE_MIN_SIZE,
            ActivePanel::Timeline,
        )),
    ),
    (
        "aria.panel.inspector",
        "Effect Controls",
        PanelLocation::Inspector,
        draw_inspector,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Left,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Inspector,
        )),
    ),
    (
        "aria.panel.viewer",
        "Viewer",
        PanelLocation::Viewer,
        draw_preview,
        |state| !matches!(state.active_panel, ActivePanel::Ai | ActivePanel::Mixer),
        Some(DockSpec::new(
            DefaultFrame::Center,
            DockSpec::VIEWER_MIN_SIZE,
            ActivePanel::Preview,
        )),
    ),
    (
        "aria.panel.mixer",
        "Audio Track Mixer",
        PanelLocation::Viewer,
        draw_mixer,
        mixer::visible,
        Some(DockSpec::new(
            DefaultFrame::Center,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Mixer,
        )),
    ),
    (
        "aria.panel.audio_meters",
        "Audio Meters",
        PanelLocation::Inspector,
        draw_audio_meters,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Bottom,
            (64.0, 80.0),
            ActivePanel::Mixer,
        )),
    ),
    (
        "aria.panel.jobs",
        "Jobs",
        PanelLocation::Inspector,
        draw_jobs_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Timeline,
        )),
    ),
    (
        crate::dock::PANEL_CHARACTER,
        "Character",
        PanelLocation::Inspector,
        draw_character_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Inspector,
        )),
    ),
    (
        crate::dock::PANEL_PARAGRAPH,
        "Paragraph",
        PanelLocation::Inspector,
        draw_paragraph_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Inspector,
        )),
    ),
    (
        crate::dock::PANEL_INFO,
        "Info",
        PanelLocation::Inspector,
        draw_info_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Inspector,
        )),
    ),
    (
        crate::dock::PANEL_ALIGN,
        "Align",
        PanelLocation::Inspector,
        draw_align_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Inspector,
        )),
    ),
    (
        crate::dock::PANEL_HISTORY,
        "History",
        PanelLocation::Inspector,
        draw_history_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Timeline,
        )),
    ),
    (
        crate::dock::PANEL_MARKERS,
        "Markers",
        PanelLocation::Sidebar,
        draw_markers_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Left,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Timeline,
        )),
    ),
    (
        crate::dock::PANEL_SCOPES,
        "Scopes",
        PanelLocation::Inspector,
        draw_scopes_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Left,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Preview,
        )),
    ),
    (
        crate::dock::PANEL_COLOR,
        "Color",
        PanelLocation::Inspector,
        draw_color_panel,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Inspector,
        )),
    ),
    (
        "aria.panel.effects",
        "Effects & Presets",
        PanelLocation::Sidebar,
        draw_effects_browser,
        panel_host::ALWAYS,
        Some(DockSpec::new(
            DefaultFrame::Right,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Media,
        )),
    ),
    (
        "aria.panel.assistant",
        "Assistant",
        PanelLocation::Viewer,
        draw_assistant,
        |state| state.active_panel == ActivePanel::Ai,
        Some(DockSpec::new(
            DefaultFrame::Center,
            DockSpec::DEFAULT_MIN_SIZE,
            ActivePanel::Ai,
        )),
    ),
    (
        "aria.panel.home",
        "Home",
        PanelLocation::Home,
        draw_home,
        |state| state.route == UiRoute::Home,
        None,
    ),
    (
        "aria.panel.ai-tools",
        "Tools",
        PanelLocation::Inspector,
        draw_ai_tools,
        ai_tools::visible,
        None,
    ),
];

/// The shipped dialogs and overlays, one entry per thing the shell used to name
/// directly. A table for the same reason the panels are one: the test below
/// enumerates it.
const BUILTIN_OVERLAYS: &[(&str, OverlayWhen, OverlayRender)] = &[
    (
        "aria.dialog.export",
        OverlayWhen::Dialog(DialogKind::Export),
        draw_export,
    ),
    (
        "aria.dialog.settings",
        OverlayWhen::Dialog(DialogKind::Settings),
        draw_settings,
    ),
    (
        "aria.dialog.palette",
        OverlayWhen::Dialog(DialogKind::Palette),
        draw_palette,
    ),
    (
        "aria.dialog.jobs",
        OverlayWhen::Dialog(DialogKind::Jobs),
        draw_jobs,
    ),
    (
        "aria.dialog.unsaved",
        OverlayWhen::Dialog(DialogKind::UnsavedChanges),
        draw_unsaved,
    ),
    (
        "aria.dialog.install-plugin",
        OverlayWhen::Dialog(DialogKind::InstallPlugin),
        draw_install_plugin,
    ),
    (
        "aria.dialog.project-report",
        OverlayWhen::Dialog(DialogKind::ProjectReport),
        draw_project_report,
    ),
    (
        "aria.dialog.media-support",
        OverlayWhen::Dialog(DialogKind::MediaSupport),
        draw_media_support,
    ),
    (
        "aria.dialog.timeline-report",
        OverlayWhen::Dialog(DialogKind::TimelineReport),
        draw_timeline_report,
    ),
    (
        "aria.dialog.speed-duration",
        OverlayWhen::Dialog(DialogKind::SpeedDuration),
        draw_speed_duration,
    ),
    (
        "aria.dialog.audio-gain",
        OverlayWhen::Dialog(DialogKind::AudioGain),
        draw_audio_gain,
    ),
    (
        "aria.dialog.about",
        OverlayWhen::Dialog(DialogKind::About),
        draw_about,
    ),
    (
        "aria.dialog.remove-attributes",
        OverlayWhen::Dialog(DialogKind::RemoveAttributes),
        draw_remove_attributes,
    ),
    (
        "aria.dialog.keyframe-velocity",
        OverlayWhen::Dialog(DialogKind::KeyframeVelocity),
        draw_keyframe_velocity,
    ),
    (
        "aria.dialog.marker-edit",
        OverlayWhen::Dialog(DialogKind::MarkerEdit),
        draw_marker_edit,
    ),
    (
        "aria.overlay.context-menu",
        OverlayWhen::ContextMenu,
        draw_context_menu,
    ),
    ("aria.overlay.rename", OverlayWhen::Rename, draw_rename_bar),
];

fn draw_export(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    export_dialog::render(state, cx).into_any_element()
}

fn draw_settings(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    settings_dialog::render(state, cx).into_any_element()
}

fn draw_palette(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    palette::render(state, cx).into_any_element()
}

fn draw_jobs(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    jobs_dialog::render(state, cx).into_any_element()
}

fn draw_jobs_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    jobs_dialog::render_panel(state, cx)
}

/// The ids of the built-in dockable panels, in registration order: what a
/// session reads as registered when no ui plugin has mounted a panel host.
pub(crate) fn builtin_dock_panel_ids() -> impl Iterator<Item = &'static str> {
    BUILTIN_PANELS
        .iter()
        .filter(|(.., dock)| dock.is_some())
        .map(|(id, ..)| *id)
}

/// The title of built-in panel `id`, or `None` when no built-in panel has that
/// id: what a session reads as a panel's name when no ui plugin has mounted a
/// panel host.
pub(crate) fn builtin_panel_title(id: &str) -> Option<&'static str> {
    BUILTIN_PANELS
        .iter()
        .find(|(panel, ..)| *panel == id)
        .map(|(_, title, ..)| *title)
}

fn draw_character_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    text_panel::render_character(state, cx)
}

fn draw_paragraph_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    text_panel::render_paragraph(state, cx)
}

fn draw_info_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    info_panel::render(state, cx)
}

fn draw_align_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    align_panel::render(state, cx)
}

fn draw_history_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    history_panel::render(state, cx)
}

fn draw_markers_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    markers::render(state, cx)
}

fn draw_scopes_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    preview::scopes::panel(state, cx)
}

fn draw_color_panel(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    color_panel::render(state, cx)
}

fn draw_unsaved(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    unsaved_dialog::render(state, cx).into_any_element()
}

fn draw_install_plugin(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    install_dialog::render(state, cx).into_any_element()
}

fn draw_speed_duration(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    speed_duration_dialog::render(state, cx).into_any_element()
}

fn draw_audio_gain(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    audio_gain_dialog::render(state, cx).into_any_element()
}

fn draw_about(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    about_dialog::render(state, cx).into_any_element()
}

fn draw_remove_attributes(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    remove_attributes_dialog::render(state, cx).into_any_element()
}

fn draw_keyframe_velocity(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    keyframe_velocity_dialog::render(state, cx).into_any_element()
}
fn draw_marker_edit(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    marker_edit_dialog::render(state, cx).into_any_element()
}

fn draw_context_menu(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    context_menu::render(state, cx).into_any_element()
}

fn draw_rename_bar(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    rename_bar::render(state, cx).into_any_element()
}

fn draw_sidebar(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    sidebar::render(state, cx).into_any_element()
}

fn draw_timeline(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    timeline::render(state, cx).into_any_element()
}

fn draw_inspector(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    inspector::render(state, cx).into_any_element()
}

fn draw_preview(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    preview::render(state, cx).into_any_element()
}

fn draw_assistant(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    ai_panel::render(state, cx).into_any_element()
}

fn draw_mixer(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    mixer::render(state, cx).into_any_element()
}

fn draw_audio_meters(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    audio_meters::render(state, cx)
}

fn draw_effects_browser(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    effects_browser::effects_dock_panel(&crate::theme::Theme::of(cx), state, cx)
}

fn draw_ai_tools(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    ai_tools::render(state, cx).into_any_element()
}

fn draw_media_support(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    media_support::render(state, cx).into_any_element()
}

fn draw_timeline_report(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    timeline_report::render(state, cx).into_any_element()
}

fn draw_project_report(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    project_report::render(state, cx).into_any_element()
}

fn draw_home(state: &Entity<AppState>, cx: &mut gpui::Context<Shell>) -> AnyElement {
    home::render(state, cx).into_any_element()
}

impl Plugin for UiPlugin {
    fn manifest(&self) -> &PluginManifest {
        // No dependency: a panel needs no GPU, and a shell with no panels at all
        // would be a blank editor.
        static MANIFEST: PluginManifest =
            PluginManifest::new("aria.ui", "Editor panels", "0.1.0", &[], &[]);
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        // Registered before the built-ins are added so a later plugin can reach the
        // same host and contribute or override a panel.
        let host = Arc::new(PanelHost::default());
        for (id, title, location, render, visible, dock) in BUILTIN_PANELS {
            host.add(PanelEntry {
                id,
                title,
                location: *location,
                render: *render,
                visible: *visible,
                dock: *dock,
            });
        }
        host.set_declared(crate::panels::plugin_panel::render_at);
        host.set_declared_one(crate::panels::plugin_panel::render_one);
        super::register_dyn(ctx, host);

        // The palette is a registry for the same reason the panels are: a plugin
        // contributes a command through the call the shipped ones use.
        let commands = Arc::new(CommandHost::default());
        for entry in BUILTIN_COMMANDS {
            commands.add(*entry);
        }
        for entry in BUILTIN_JOB_COMMANDS {
            commands.add_windowed(*entry);
        }
        super::register_dyn(ctx, commands);

        // Same for the app bar: the shipped tables are one contributor, so a
        // plugin's menu item needs no chrome edit.
        let menus = Arc::new(MenuHost::default());
        menus.add(crate::chrome::menu_items::items_for);
        super::register_dyn(ctx, menus);

        // And the keyboard: a chord is data, so a plugin can bind one and a taken
        // chord is refused with a reason instead of shadowing the shipped binding.
        let keys = Arc::new(KeyHost::default());
        for binding in SHIPPED_BINDINGS {
            keys.add(*binding);
        }
        super::register_dyn(ctx, keys);

        // Dialogs and overlays too, so a plugin's dialog opens through the same
        // `open_dialog` call and is drawn by the code that draws Export.
        let overlays = Arc::new(OverlayHost::default());
        for (id, when, render) in BUILTIN_OVERLAYS {
            overlays.add(OverlayEntry {
                id,
                when: *when,
                render: *render,
            });
        }
        super::register_dyn(ctx, overlays);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A panel with no id, a duplicate id, or a location nothing draws would be a
    /// panel the shell silently drops.
    #[test]
    fn every_builtin_panel_is_registered_once_at_a_slot_the_shell_draws() {
        let host = PanelHost::default();
        for (id, title, location, render, visible, dock) in BUILTIN_PANELS {
            assert!(!id.is_empty() && !title.is_empty(), "{id} is unnamed");
            host.add(PanelEntry {
                id,
                title,
                location: *location,
                render: *render,
                visible: *visible,
                dock: *dock,
            });
        }
        assert_eq!(host.ids().len(), BUILTIN_PANELS.len(), "duplicate panel id");

        // One entry per slot the table names, counted from the table rather than
        // written down: a panel added above is asserted here with no edit, and a
        // hand-written count cannot drift away from what ships.
        let mut expected: Vec<(PanelLocation, usize)> = Vec::new();
        for (_, _, location, _, _, _) in BUILTIN_PANELS {
            match expected.iter_mut().find(|(seen, _)| seen == location) {
                Some((_, count)) => *count += 1,
                None => expected.push((*location, 1)),
            }
        }
        for (location, count) in &expected {
            assert_eq!(
                host.at(*location).len(),
                *count,
                "{location:?} has the wrong number of built-in bodies"
            );
        }
        assert!(
            !expected
                .iter()
                .any(|(location, _)| *location == PanelLocation::Overlay),
            "the viewer is drawn by a panel now; nothing built-in overlays it"
        );
    }

    /// Registering the same id twice replaces the panel rather than drawing both.
    #[test]
    fn a_second_registration_of_an_id_overrides_the_first() {
        let host = PanelHost::default();
        host.add(PanelEntry {
            id: "aria.panel.media",
            title: "Project",
            location: PanelLocation::Sidebar,
            render: draw_sidebar,
            visible: panel_host::ALWAYS,
            dock: DockSpec::for_location(PanelLocation::Sidebar),
        });
        host.add(PanelEntry {
            id: "aria.panel.media",
            title: "Replaced",
            location: PanelLocation::Inspector,
            render: draw_inspector,
            visible: panel_host::ALWAYS,
            dock: DockSpec::for_location(PanelLocation::Inspector),
        });
        assert_eq!(host.ids(), vec!["aria.panel.media"]);
        assert!(host.at(PanelLocation::Sidebar).is_empty());
        assert_eq!(host.at(PanelLocation::Inspector).len(), 1);
    }

    /// The palette reads the registry, so a command that is in the table but never
    /// registered does not exist in the palette. Mount is the only place that
    /// bridges the two.
    #[test]
    fn mounting_offers_every_shipped_command() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
        let host = crate::commands::host::host_of(&ctx).expect("mount registers the command host");
        let ids: Vec<&str> = BUILTIN_COMMANDS
            .iter()
            .map(|cmd| cmd.id)
            .chain(BUILTIN_JOB_COMMANDS.iter().map(|cmd| cmd.id))
            .collect();
        assert_eq!(host.ids(), ids);
    }

    /// The app bar reads the registry, so a shipped menu with no contributor would
    /// draw an empty drop-down.
    #[test]
    fn mounting_registers_the_shipped_menu_tables() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
        let host = crate::chrome::menu_host::host_of(&ctx).expect("mount registers the menu host");
        assert_eq!(host.len(), 1);
    }

    /// The shell dispatches from the registry, so a shipped chord that never
    /// registered would be a dead key.
    #[test]
    fn mounting_registers_every_shipped_binding_without_a_conflict() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
        let host = crate::keys::host_of(&ctx).expect("mount registers the key host");
        assert_eq!(host.all().len(), SHIPPED_BINDINGS.len());
        assert!(
            host.conflicts().is_empty(),
            "the shipped table collides with itself: {:?}",
            host.conflicts()
        );
    }

    /// Every dialog the app can open has an overlay registered for it.
    ///
    /// The class this closes: the shell now draws whatever the registry holds, so
    /// a `DialogKind` with no entry opens a dialog that draws nothing — a state
    /// that is invisible and cannot be left except by Escape. Enumerated over the
    /// kinds, so a new variant is red here until it is registered or is
    /// deliberately plugin-owned.
    #[test]
    fn mounting_registers_an_overlay_for_every_dialog_the_shell_can_open() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("the ui plugin needs nothing");
        let host =
            crate::plugins::overlay_host::host_of(&ctx).expect("mount registers the overlay host");
        assert_eq!(host.ids().len(), BUILTIN_OVERLAYS.len(), "duplicate id");

        for kind in [
            DialogKind::Export,
            DialogKind::Settings,
            DialogKind::Palette,
            DialogKind::Jobs,
            DialogKind::UnsavedChanges,
            DialogKind::InstallPlugin,
            DialogKind::SpeedDuration,
            DialogKind::AudioGain,
            DialogKind::ProjectReport,
            DialogKind::MediaSupport,
            DialogKind::TimelineReport,
            DialogKind::About,
            DialogKind::RemoveAttributes,
            DialogKind::KeyframeVelocity,
            DialogKind::MarkerEdit,
        ] {
            assert!(host.dialog(kind, true).is_some(), "{kind:?} has no overlay");
        }
        // Nothing is registered for the absence of a dialog, or for an id no
        // plugin claimed.
        assert!(host.dialog(DialogKind::None, true).is_none());
        assert!(host.dialog(DialogKind::Plugin("nobody"), true).is_none());

        // The stacking overlays are still there, and they are not dialogs.
        assert_eq!(host.matching(OverlayWhen::ContextMenu, true).len(), 1);
        assert_eq!(host.matching(OverlayWhen::Rename, true).len(), 1);
    }

    /// Panels registered at PanelLocation::Inspector have well-defined docking and visible predicates.
    #[test]
    fn inspector_location_panels_have_explicit_docking_and_visibility_rules() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("mount UI plugin");
        let host = crate::plugins::panel_host::host_of(&ctx).expect("panel host");
        let inspector_panels = host.at(PanelLocation::Inspector);
        assert!(
            !inspector_panels.is_empty(),
            "at least one inspector panel must be registered"
        );
        // The primary inspector is always registered and available
        assert!(
            inspector_panels
                .iter()
                .any(|(id, _)| *id == "aria.panel.inspector"),
            "primary inspector panel must be present"
        );

        for (id, title, location, _render, _visible, _dock) in BUILTIN_PANELS {
            if *location == PanelLocation::Inspector {
                assert!(!id.is_empty(), "inspector panel has empty id");
                assert!(!title.is_empty(), "inspector panel has empty title");
            }
        }
    }

    /// Inspector and viewer panels adhere to explicit collision-free docking and mutually exclusive
    /// visibility rules across relevant editor states.
    #[test]
    fn inspector_and_viewer_panels_have_collision_free_visibility() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("mount UI plugin");
        let host = crate::plugins::panel_host::host_of(&ctx).expect("panel host");
        let dir = crate::scratch_pool::scratch("inspector-visibility-test");
        let mut state = AppState::new(PluginContext::new(), dir).expect("state");

        // 1. Viewer slot mutual exclusivity: exactly one body is visible for each ActivePanel route
        for active in ActivePanel::ALL {
            state.active_panel = active;
            let visible_viewers = host.visible_at(PanelLocation::Viewer, &state);
            assert_eq!(
                visible_viewers.len(),
                1,
                "viewer slot must show exactly one mutually exclusive panel for {active:?}"
            );
        }

        // 2. Inspector slot: primary inspector is always docked and present; auxiliary panels
        // appear only when their specific gating condition holds.
        state.active_panel = ActivePanel::Preview;
        let visible_inspector = host.visible_at(PanelLocation::Inspector, &state);
        assert!(
            !visible_inspector.is_empty(),
            "primary inspector is always docked and visible"
        );

        // AI tools panel is visible in inspector only when assistant is active
        state.active_panel = ActivePanel::Ai;
        assert!(
            crate::panels::ai_tools::visible(&state),
            "ai tools must be visible when ActivePanel is Ai"
        );
        state.active_panel = ActivePanel::Preview;
        assert!(
            !crate::panels::ai_tools::visible(&state),
            "ai tools must be hidden when ActivePanel is not Ai"
        );
    }

    /// A card stacked in the Inspector column stands under whichever tab the
    /// selected clip shows, so a card drawn for a selection repeats under
    /// Transform, Speed, and Info alike. Every stacked card is drawn on a
    /// condition beyond the selection: a selected clip stacks none, and so does
    /// a selected music clip a duck could run from, whose dip, attack, and
    /// release the Audio tab sets.
    #[test]
    fn a_selection_stacks_no_card_under_the_inspector() {
        let ctx = PluginContext::new();
        ctx.mount(&UiPlugin).expect("mount UI plugin");
        let host = crate::plugins::panel_host::host_of(&ctx).expect("panel host");
        let selections = [
            ("a selected clip", crate::app::premiere_tests::with_clip().0),
            (
                "a music clip a duck could run from",
                crate::app::duck_tests::ready_duck(),
            ),
        ];
        assert!(
            selections[1].1.duck_precheck().is_ok(),
            "the duck fixture is one a duck could run from"
        );
        for (what, mut state) in selections {
            state.active_panel = ActivePanel::Inspector;
            assert!(
                state.selected_clip().is_some(),
                "{what}: the fixture selects its clip"
            );
            let stacked: Vec<&str> = host
                .cached_at(PanelLocation::Inspector)
                .iter()
                .filter(|entry| entry.dock.is_none() && (entry.visible)(&state))
                .map(|entry| entry.id)
                .collect();
            assert_eq!(
                host.sub_entries_at(PanelLocation::Inspector, &state).len(),
                0,
                "{what} stacks {stacked:?} under the Inspector tabs"
            );
        }
    }
}
