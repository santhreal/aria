//! Session state shared by every GPUI panel.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aria_commands::{Command, EditCommand};
use aria_core::RgbaImage;
use aria_engine::{ApplyOutcome, Engine, EngineConfig, EngineError};
use aria_models::{ClipId, RationalTime, TimeRange, TrackId, TrackKind};
use aria_plugin::{Origin, PluginContext, ProjectHost};
use gpui::{Context, EventEmitter, RenderImage};
use parking_lot::Mutex;

use crate::settings::{SAVE_DEBOUNCE_MS, UiSettings};
use crate::types::{
    ActivePanel, AppMenu, ContextTarget, EditMode, EditorTool, SidebarTab, UiAgentEntry, UiClip,
    UiClipTransform, UiEditorStore, UiRoute,
};

#[path = "app/adjustment.rs"]
pub mod adjustment;
#[path = "app/ai_reach.rs"]
pub mod ai_reach;
#[path = "app/align.rs"]
pub mod align;
#[path = "app/analysis.rs"]
pub mod analysis;
pub mod animation_verbs;
#[path = "app/assistant.rs"]
pub mod assistant;
#[path = "app/assistant_host.rs"]
pub mod assistant_host;
#[path = "app/assistant_store.rs"]
pub mod assistant_store;
#[path = "app/audio_channels.rs"]
pub mod audio_channels;
#[path = "app/audio_inserts.rs"]
pub mod audio_inserts;
#[path = "app/bake.rs"]
pub(crate) mod bake;
#[path = "app/bins.rs"]
pub mod bins;
#[path = "app/caches.rs"]
pub mod caches;
#[path = "app/canvas_overlay.rs"]
pub mod canvas_overlay;
#[path = "app/captions.rs"]
pub mod captions;
#[path = "app/catalog_drag.rs"]
mod catalog_drag;
#[path = "app/clip_anim.rs"]
pub(crate) mod clip_anim;
#[path = "app/clip_audio.rs"]
pub(crate) mod clip_audio;
#[path = "app/clip_effect_layers.rs"]
pub(crate) mod clip_effect_layers;
#[path = "app/clip_effects.rs"]
pub(crate) mod clip_effects;
#[path = "app/clip_frame.rs"]
pub(crate) mod clip_frame;
#[path = "app/clip_keys.rs"]
pub(crate) mod clip_keys;
#[path = "app/clip_line.rs"]
pub(crate) mod clip_line;
#[path = "app/clip_look.rs"]
pub(crate) mod clip_look;
#[path = "app/clip_mask.rs"]
pub(crate) mod clip_mask;
#[path = "app/clip_ops.rs"]
pub mod clip_ops;
#[path = "app/clip_params.rs"]
pub(crate) mod clip_params;
#[path = "app/clip_parent.rs"]
pub mod clip_parent;
#[path = "app/clip_speed.rs"]
pub(crate) mod clip_speed;
#[path = "app/clip_styles.rs"]
pub(crate) mod clip_styles;
#[path = "app/clip_template.rs"]
pub(crate) mod clip_template;
#[path = "app/clip_text.rs"]
pub(crate) mod clip_text;
#[path = "app/clip_transitions.rs"]
pub(crate) mod clip_transitions;
#[path = "app/clipboard.rs"]
mod clipboard;
#[path = "app/clock.rs"]
pub mod clock;
#[path = "app/cloud_feeds.rs"]
pub(crate) mod cloud_feeds;
#[path = "app/curve_drag.rs"]
pub mod curve_drag;
#[path = "app/curve_geometry.rs"]
pub mod curve_geometry;
#[path = "app/curve_stroke.rs"]
pub mod curve_stroke;
#[path = "app/decline.rs"]
pub mod decline;
#[path = "app/dialog_primary.rs"]
pub mod dialog_primary;
#[path = "app/drivers.rs"]
pub mod drivers;
#[path = "app/drops.rs"]
pub(crate) mod drops;
#[path = "app/duck.rs"]
pub mod duck;
#[path = "app/edits.rs"]
mod edits;
#[path = "app/effect_header.rs"]
pub(crate) mod effect_header;
#[path = "app/effects_browser.rs"]
pub mod effects_browser;
#[path = "app/error_copy.rs"]
mod error_copy;
#[path = "app/export_options.rs"]
pub mod export_options;
#[path = "app/export_queue.rs"]
pub mod export_queue;
#[path = "app/export_request.rs"]
pub mod export_request;
#[path = "app/foreign_edits.rs"]
mod foreign_edits;
#[path = "app/generate.rs"]
pub mod generate;
#[path = "app/grade_controls.rs"]
pub mod grade_controls;
#[path = "app/history.rs"]
mod history;
#[path = "app/import_recents.rs"]
pub mod import_recents;
#[path = "app/info.rs"]
pub mod info;
#[path = "app/jobs.rs"]
pub mod jobs;
#[path = "app/key_strip.rs"]
pub mod key_strip;
#[path = "app/key_verbs.rs"]
mod key_verbs;
#[path = "app/keymaps.rs"]
pub mod keymaps;
#[path = "app/labels.rs"]
mod labels;
#[path = "app/lanes.rs"]
mod lanes;
#[path = "app/launch.rs"]
pub mod launch;
#[path = "app/layouts.rs"]
pub(crate) mod layouts;
#[path = "app/marquee.rs"]
pub mod marquee;
#[path = "app/mask_path.rs"]
pub mod mask_path;
#[path = "app/mask_path_keys.rs"]
pub(crate) mod mask_path_keys;
#[path = "app/mask_track.rs"]
pub(crate) mod mask_track;
#[path = "app/media.rs"]
pub(crate) mod media;
#[path = "app/media_reach.rs"]
pub mod media_reach;
#[path = "app/menu_keys.rs"]
pub mod menu_keys;
pub mod menu_verbs;
#[path = "app/meter.rs"]
pub mod meter;
#[path = "app/mixer.rs"]
pub mod mixer;
#[path = "app/models.rs"]
pub mod models;
#[path = "app/multicam.rs"]
pub mod multicam;
#[path = "app/multicam_sync.rs"]
pub mod multicam_sync;
#[path = "app/nested.rs"]
pub mod nested;
#[path = "app/palette.rs"]
pub mod palette;
#[path = "app/picker.rs"]
pub mod picker;
#[path = "app/plugin_commands.rs"]
mod plugin_commands;
#[path = "app/plugin_install.rs"]
pub mod plugin_install;
#[path = "app/plugin_list.rs"]
pub(crate) mod plugin_list;
#[path = "app/plugin_overlays.rs"]
pub mod plugin_overlays;
#[path = "app/plugin_panels.rs"]
pub mod plugin_panels;
#[path = "app/pool_preview.rs"]
pub mod pool_preview;
#[path = "app/pool_rows.rs"]
pub mod pool_rows;
#[path = "app/pool_selection.rs"]
pub mod pool_selection;
#[path = "app/presets.rs"]
pub(crate) mod presets;
#[path = "app/preview_view.rs"]
pub mod preview_view;
#[path = "app/project_facts.rs"]
pub mod project_facts;
#[path = "app/project_reach.rs"]
pub mod project_reach;
#[path = "app/project_report.rs"]
pub mod project_report;
#[path = "app/project_settings.rs"]
pub(crate) mod project_settings;
#[path = "app/property_rows.rs"]
pub mod property_rows;
#[path = "app/proxy.rs"]
pub(crate) mod proxy;
#[path = "app/rebind.rs"]
mod rebind;
#[path = "app/recents.rs"]
pub mod recents;
#[path = "app/recovery.rs"]
pub mod recovery;
#[path = "app/relink.rs"]
pub(crate) mod relink;
pub mod remove_attributes;
#[path = "app/rename.rs"]
mod rename;
#[path = "app/render_bar.rs"]
mod render_bar;
#[path = "app/render_device.rs"]
mod render_device;
#[path = "app/render_files.rs"]
pub mod render_files;
#[path = "app/render_reach.rs"]
pub mod render_reach;
#[path = "app/scope_view.rs"]
pub mod scope_view;
#[path = "app/selected_detail.rs"]
pub mod selected_detail;
#[path = "app/session.rs"]
mod session;
#[path = "app/settings_form.rs"]
pub mod settings_form;
#[path = "app/shared_edits.rs"]
pub(crate) mod shared_edits;
#[path = "app/skills.rs"]
pub mod skills;
#[path = "app/source_patch.rs"]
mod source_patch;
#[path = "app/source_transport.rs"]
pub mod source_transport;
#[path = "app/status.rs"]
pub mod status;
#[path = "app/step_hold.rs"]
pub mod step_hold;
#[path = "app/storage.rs"]
pub(crate) mod storage;
#[path = "app/subclips.rs"]
pub(crate) mod subclips;
#[path = "app/templates.rs"]
pub(crate) mod templates;
#[path = "app/text_reach.rs"]
pub mod text_reach;
#[path = "app/three_point.rs"]
mod three_point;
#[path = "app/timeline_files.rs"]
pub mod timeline_files;
#[path = "app/timeline_view.rs"]
pub(crate) mod timeline_view;
#[path = "app/tracks.rs"]
pub(crate) mod tracks;
#[path = "app/transport.rs"]
mod transport;
#[path = "app/trim_gestures.rs"]
pub mod trim_gestures;
#[path = "app/unsaved.rs"]
pub mod unsaved;
#[path = "app/value_scrub.rs"]
pub mod value_scrub;
#[path = "app/view_events.rs"]
mod view_events;
#[path = "app/viewer_compare.rs"]
pub mod viewer_compare;
#[path = "app/viewer_guides.rs"]
pub mod viewer_guides;
#[path = "app/viewer_match.rs"]
pub mod viewer_match;
#[path = "app/viewer_resolution.rs"]
mod viewer_resolution;
#[path = "app/viewer_scrub.rs"]
pub mod viewer_scrub;
#[path = "app/viewer_tabs.rs"]
pub mod viewer_tabs;
#[path = "app/work_area.rs"]
pub mod work_area;
#[path = "app/zoom.rs"]
mod zoom;

pub use edits::{MarkerEditField, MarkerEditForm};
pub(crate) use error_copy::{describe_engine_error, describe_export_refusal};
pub use render_bar::compute_render_segments;
pub use render_files::{RenderQuality, RenderSegment};
#[cfg(test)]
pub(crate) use transport::Mail;
pub use transport::{project_store, spawn_preview, start_playback};
pub(crate) use view_events::ContextPluginRow;

/// Raised when the projected editor store changes.
#[derive(Clone, Debug)]
pub struct StoreChanged;

/// Raised when a new preview frame is on screen, or a scope or probe reading
/// of the one on screen arrives. Each window redraws the panel bodies that
/// draw the frame or its readings.
#[derive(Clone, Debug)]
pub struct FrameReady;

/// Raised when a playback vsync moves the playhead. Each window redraws the
/// panel bodies that draw the playhead, and those that draw the timeline view
/// when the move scrolled it.
#[derive(Clone, Debug)]
pub struct PlayheadMoved {
    /// Whether the timeline scrolled to follow the playhead.
    pub followed: bool,
}

/// Raised when a media tile's scrub position or picture changes and nothing
/// else on screen does. Each window redraws the panel bodies that draw tiles.
#[derive(Clone, Debug)]
pub struct PoolTileChanged;

/// Raised when a timeline filmstrip cell finishes rendering. Each window
/// redraws the panel bodies that draw filmstrips.
#[derive(Clone, Debug)]
pub struct FilmstripChanged;

/// Which dialog is on screen. One at a time: a dialog owns the keyboard while it
/// is up, so a second one would take input from the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DialogKind {
    #[default]
    None,
    Export,
    Settings,
    Palette,
    Jobs,
    /// What the sequence carries. A dialog rather than a docked pane: it
    /// answers an occasional question and reads at length, so it should not
    /// spend inspector width permanently.
    ProjectReport,
    /// What this build decodes and what the selection is made of, a dialog as
    /// the Project Report is. Opening it builds the report.
    MediaSupport,
    /// What the last timeline import or export could not carry as it was.
    /// The run that met such an item opens it, after an install prompt that is
    /// up, and closing it drops the report.
    TimelineReport,
    /// The prompt standing between unsaved edits and an action that would
    /// replace the session.
    UnsavedChanges,
    /// The prompt that shows a plugin package and what it asks for before it
    /// is installed.
    InstallPlugin,
    /// The Speed/Duration dialog (`Ctrl+R`).
    SpeedDuration,
    /// The Audio Gain dialog (`G`).
    AudioGain,
    /// The build's version, plugin ABI and data folder (Help > About).
    About,
    /// Edit > Remove Attributes: the groups to return to their defaults.
    RemoveAttributes,
    /// Animation > Keyframe Velocity: the speed and influence of the selected
    /// keyframes' handles.
    KeyframeVelocity,
    /// The Edit Marker dialog (`M` on an existing marker, or Markers > Edit Marker…).
    MarkerEdit,
    /// A dialog a plugin registered, by the id it registered it under. The shell
    /// finds the drawing in `plugins::overlay_host`, so a plugin's dialog opens
    /// through the same call a shipped one does.
    Plugin(&'static str),
}

struct Snapshot {
    revision: u64,
    playhead: RationalTime,
    store: UiEditorStore,
    /// The selected clip as the model holds it, for the values the projection
    /// does not carry. Built here because the engine is already in hand: the
    /// inspector asking per element would be a lock per element per frame.
    selected: Option<selected_detail::SelectedDetail>,
    /// The session facts a frame reads that the store does not carry, for the
    /// same reason: asking per element is a lock per element per frame.
    facts: project_facts::ProjectFacts,
    /// What `store` was built from, for the next projection to compare against.
    basis: crate::projection::LaneBasis,
}

impl Snapshot {
    /// Project the engine's state. A lane that did not move since `previous`
    /// was built keeps the clips `previous` holds for it.
    fn from_engine(
        engine: &mut Engine,
        playhead: RationalTime,
        selection: &[ClipId],
        previous: Option<crate::projection::Previous<'_>>,
    ) -> Self {
        let rate = engine.project().timeline().frame_rate;
        let at = RationalTime::new(playhead.value, rate);
        let (mut store, basis) = crate::projection::project_to_ui_from(
            engine.project(),
            at,
            selection,
            engine.is_dirty(),
            engine.can_undo(),
            engine.can_redo(),
            previous,
        );
        // Which items decode through a proxy is the renderer's state, not the
        // project's — it is never saved — so the projection cannot read it and
        // this is the one pass that can: the engine is already in hand, and a
        // pool row asking per frame would be a lock per row per frame.
        for item in &mut store.media {
            item.proxied = engine
                .media_proxy(aria_models::MediaId::from_raw(item.id))
                .is_some();
        }
        let facts = project_facts::ProjectFacts::of_with_basis(engine, Some(&basis));
        Self {
            revision: engine.revision(),
            playhead: at,
            store,
            selected: selected_detail::SelectedDetail::of(engine, selection, at),
            facts,
            basis,
        }
    }
}

impl EventEmitter<StoreChanged> for AppState {}
impl EventEmitter<FrameReady> for AppState {}
impl EventEmitter<PlayheadMoved> for AppState {}
impl EventEmitter<PoolTileChanged> for AppState {}
impl EventEmitter<FilmstripChanged> for AppState {}

#[cfg(test)]
#[path = "app/error_copy_tests.rs"]
mod error_copy_tests;

#[cfg(test)]
#[path = "app/first_run_tests.rs"]
mod first_run_tests;

// The one place a test gets a project with a clip on the timeline. `pub(crate)`
// because the frame-cost tests live beside the motion catalog they measure, not
// beside the state they load.
#[cfg(test)]
#[path = "app/premiere_tests.rs"]
pub(crate) mod premiere_tests;

// The dressed states every census and every reach test presses against. Declared
// here rather than inside one census: a menu row and an inspector chip want the
// same clip with sound, and a fixture that only one census can see grows a second
// copy in the next one. `pub(crate)` for the drawn menu tests, which dress the
// state a window already holds.
#[cfg(test)]
#[path = "app/drawn_census_fixtures.rs"]
pub(crate) mod fixtures;

#[cfg(test)]
#[path = "app/reach_tests.rs"]
mod reach_tests;

#[cfg(test)]
#[path = "app/menu_reach_tests.rs"]
mod menu_reach_tests;

#[cfg(test)]
#[path = "app/linked_selection_tests.rs"]
mod linked_selection_tests;

#[cfg(test)]
#[path = "app/animation_verbs_tests.rs"]
mod animation_verbs_tests;

#[cfg(test)]
#[path = "app/operator_surface_tests.rs"]
mod operator_surface_tests;

#[cfg(test)]
#[path = "app/drawn_census.rs"]
mod drawn_census;

#[cfg(test)]
#[path = "app/sent_census.rs"]
mod sent_census;

#[cfg(test)]
#[path = "app/windowed_reach_tests.rs"]
mod windowed_reach_tests;

#[cfg(test)]
#[path = "app/look_tests.rs"]
mod look_tests;

#[cfg(test)]
#[path = "app/frame_tests.rs"]
mod frame_tests;

#[cfg(test)]
#[path = "app/mask_tests.rs"]
mod mask_tests;

#[cfg(test)]
#[path = "app/styles_tests.rs"]
mod styles_tests;

#[cfg(test)]
#[path = "app/anim_tests.rs"]
mod anim_tests;

#[cfg(test)]
#[path = "app/effect_tests.rs"]
mod effect_tests;

#[cfg(test)]
#[path = "app/transition_tests.rs"]
mod transition_tests;

#[cfg(test)]
#[path = "app/template_tests.rs"]
mod template_tests;

#[cfg(test)]
#[path = "app/keyframe_tests.rs"]
pub(crate) mod keyframe_tests;

#[cfg(test)]
#[path = "app/clip_name_tests.rs"]
mod clip_name_tests;

#[cfg(test)]
#[path = "app/fade_drag_tests.rs"]
mod fade_drag_tests;

#[cfg(test)]
#[path = "app/snap_feedback_tests.rs"]
mod snap_feedback_tests;

#[cfg(test)]
#[path = "app/clock_entry_tests.rs"]
mod clock_entry_tests;
#[cfg(test)]
#[path = "app/clock_tests.rs"]
mod clock_tests;

#[cfg(test)]
#[path = "app/marquee_tests.rs"]
mod marquee_tests;

#[cfg(test)]
#[path = "app/preview_view_tests.rs"]
mod preview_view_tests;

#[cfg(test)]
#[path = "app/scope_view_tests.rs"]
mod scope_view_tests;

#[cfg(test)]
#[path = "app/source_patch_tests.rs"]
mod source_patch_tests;

#[cfg(test)]
#[path = "app/source_drop_tests.rs"]
mod source_drop_tests;

#[cfg(test)]
#[path = "app/adjustment_tests.rs"]
mod adjustment_tests;

#[cfg(test)]
#[path = "app/palette_tests.rs"]
mod palette_tests;

#[cfg(test)]
#[path = "app/speed_tests.rs"]
mod speed_tests;

#[cfg(test)]
#[path = "app/marker_tests.rs"]
mod marker_tests;

#[cfg(test)]
#[path = "app/export_options_tests.rs"]
mod export_options_tests;

#[cfg(test)]
#[path = "app/export_queue_tests.rs"]
mod export_queue_tests;

#[cfg(test)]
#[path = "app/export_request_tests.rs"]
mod export_request_tests;

#[cfg(test)]
#[path = "app/status_tests.rs"]
mod status_tests;

#[cfg(test)]
#[path = "app/track_tests.rs"]
mod track_tests;

#[cfg(test)]
#[path = "app/lane_display_tests.rs"]
mod lane_display_tests;

#[cfg(test)]
#[path = "app/project_tests.rs"]
mod project_tests;

#[cfg(test)]
#[path = "app/template_file_tests.rs"]
mod template_file_tests;

#[cfg(test)]
#[path = "app/text_tests.rs"]
mod text_tests;

#[cfg(test)]
#[path = "app/catalog_drag_tests.rs"]
mod catalog_drag_tests;

#[cfg(test)]
#[path = "app/storage_tests.rs"]
mod storage_tests;

#[cfg(test)]
#[path = "app/cloud_tests.rs"]
mod cloud_tests;

#[cfg(test)]
#[path = "app/jobs_tests.rs"]
mod jobs_tests;

#[cfg(test)]
#[path = "app/dock_panels_tests.rs"]
mod dock_panels_tests;

#[cfg(test)]
#[path = "app/empty_state_tests.rs"]
mod empty_state_tests;

#[cfg(test)]
#[path = "app/plugin_events_tests.rs"]
mod plugin_events_tests;

#[cfg(test)]
#[path = "app/captions_tests.rs"]
mod captions_tests;
#[cfg(test)]
#[path = "app/generate_tests.rs"]
mod generate_tests;

#[cfg(test)]
#[path = "app/grade_controls_tests.rs"]
mod grade_controls_tests;

#[cfg(test)]
#[path = "app/effect_bypass_tests.rs"]
mod effect_bypass_tests;

#[cfg(test)]
#[path = "app/badge_menu_tests.rs"]
mod badge_menu_tests;
#[cfg(test)]
#[path = "app/effect_header_tests.rs"]
mod effect_header_tests;
#[cfg(test)]
#[path = "app/key_strip_tests.rs"]
mod key_strip_tests;
#[cfg(test)]
#[path = "app/menu_dialog_tests.rs"]
mod menu_dialog_tests;
#[cfg(test)]
#[path = "app/menu_press_tests.rs"]
mod menu_press_tests;
#[cfg(test)]
#[path = "app/outline_column_tests.rs"]
mod outline_column_tests;
#[cfg(test)]
#[path = "app/panel_focus_tests.rs"]
mod panel_focus_tests;
#[cfg(test)]
#[path = "app/pointer_playhead_tests.rs"]
mod pointer_playhead_tests;
#[cfg(test)]
#[path = "app/release_outside_tests.rs"]
mod release_outside_tests;
#[cfg(test)]
#[path = "app/shared_edits_tests.rs"]
mod shared_edits_tests;
#[cfg(test)]
#[path = "app/text_fit_tests.rs"]
mod text_fit_tests;
#[cfg(test)]
#[path = "app/timeline_header_tests.rs"]
mod timeline_header_tests;

#[cfg(test)]
#[path = "app/sidebar_groups_tests.rs"]
mod sidebar_groups_tests;

#[cfg(test)]
#[path = "app/viewer_row_tests.rs"]
mod viewer_row_tests;

#[cfg(test)]
#[path = "app/activation_tests.rs"]
mod activation_tests;
#[cfg(test)]
#[path = "app/analysis_tests.rs"]
mod analysis_tests;
#[cfg(test)]
#[path = "app/bin_tests.rs"]
mod bin_tests;
#[cfg(test)]
#[path = "app/clipboard_tests.rs"]
mod clipboard_tests;
#[cfg(test)]
#[path = "app/duck_tests.rs"]
mod duck_tests;
#[cfg(test)]
#[path = "app/effect_preset_tests.rs"]
mod effect_preset_tests;
#[cfg(test)]
#[path = "app/engine_lock_tests.rs"]
mod engine_lock_tests;
#[cfg(test)]
#[path = "app/history_tests.rs"]
mod history_tests;
#[cfg(test)]
#[path = "app/hook_tests.rs"]
mod hook_tests;
#[cfg(test)]
#[path = "app/label_tests.rs"]
mod label_tests;
#[cfg(test)]
#[path = "app/overlay_tests.rs"]
mod overlay_tests;
#[cfg(test)]
#[path = "app/plugin_overlays_tests.rs"]
mod plugin_overlays_tests;
#[cfg(test)]
#[path = "app/plugin_panels_tests.rs"]
mod plugin_panels_tests;
#[cfg(test)]
#[path = "app/plugin_tests.rs"]
mod plugin_tests;
#[cfg(test)]
#[path = "app/settings_tests.rs"]
mod settings_tests;

#[cfg(test)]
#[path = "app/unsaved_tests.rs"]
mod unsaved_tests;

#[cfg(test)]
#[path = "app/recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "app/media_drop_tests.rs"]
mod media_drop_tests;

#[cfg(test)]
#[path = "app/transform_handle_tests.rs"]
mod transform_handle_tests;

#[cfg(test)]
#[path = "app/meter_tests.rs"]
mod meter_tests;

#[cfg(test)]
#[path = "app/shuttle_tests.rs"]
mod shuttle_tests;

#[cfg(test)]
#[path = "app/scrub_stale_tests.rs"]
mod scrub_stale_tests;

#[cfg(test)]
#[path = "app/layer_quad_tests.rs"]
mod layer_quad_tests;

#[cfg(test)]
#[path = "app/playback_report_tests.rs"]
mod playback_report_tests;

#[cfg(test)]
#[path = "app/playback_present_tests.rs"]
pub(crate) mod playback_present_tests;

#[cfg(test)]
#[path = "app/status_strip_tests.rs"]
mod status_strip_tests;

#[cfg(test)]
#[path = "app/tool_groups_tests.rs"]
mod tool_groups_tests;

#[cfg(test)]
#[path = "app/float_close_tests.rs"]
mod float_close_tests;

#[cfg(test)]
#[path = "app/assistant_host_tests.rs"]
mod assistant_host_tests;
#[cfg(test)]
#[path = "app/assistant_store_tests.rs"]
mod assistant_store_tests;
#[cfg(test)]
#[path = "app/assistant_tests.rs"]
mod assistant_tests;
#[cfg(test)]
#[path = "app/bin_folder_tests.rs"]
mod bin_folder_tests;
#[cfg(test)]
#[path = "app/codec_tests.rs"]
mod codec_tests;
#[cfg(test)]
#[path = "app/trim_gesture_tests.rs"]
mod trim_gesture_tests;

#[cfg(test)]
#[path = "app/param_rows_tests.rs"]
mod param_rows_tests;

#[cfg(test)]
#[path = "app/field_paste_tests.rs"]
mod field_paste_tests;

#[cfg(test)]
#[path = "app/picker_tests.rs"]
mod picker_tests;

#[cfg(test)]
#[path = "app/zoom_tests.rs"]
mod zoom_tests;

#[cfg(test)]
#[path = "app/viewer_resolution_tests.rs"]
mod viewer_resolution_tests;

#[cfg(test)]
#[path = "app/value_scrub_tests.rs"]
pub(crate) mod value_scrub_tests;

#[cfg(test)]
#[path = "app/layouts_tests.rs"]
mod layouts_tests;

#[cfg(test)]
#[path = "app/recents_tests.rs"]
mod recents_tests;

#[cfg(test)]
#[path = "app/preview_queue_tests.rs"]
mod preview_queue_tests;

#[cfg(test)]
#[path = "app/still_frame_tests.rs"]
mod still_frame_tests;

#[cfg(test)]
#[path = "app/playhead_frame_tests.rs"]
mod playhead_frame_tests;

#[cfg(test)]
#[path = "app/rename_caret_tests.rs"]
mod rename_caret_tests;

#[cfg(test)]
#[path = "app/pool_rows_tests.rs"]
mod pool_rows_tests;
#[cfg(test)]
#[path = "app/pool_selection_tests.rs"]
mod pool_selection_tests;

// `AppState` is defined in `state.rs`, its methods in `commands.rs`,
// `selection.rs` and `engine_sync.rs`. Re-exported so every existing path
// still resolves.
#[path = "app/state.rs"]
mod state;
pub use state::*;
#[path = "app/commands.rs"]
mod commands;
#[path = "app/engine_sync.rs"]
mod engine_sync;
#[path = "app/selection.rs"]
mod selection;
