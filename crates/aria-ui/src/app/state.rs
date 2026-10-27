//! The [`AppState`] struct: every field of the running editor.
//!
//! Moved out of `app.rs` to stay under the file ceiling. The definition is
//! unchanged except that the seven fields that were private to `app` are now
//! `pub(super)` — the same visibility by a different spelling. Private-to-`app`
//! already meant visible in `app` and every descendant, which is exactly what
//! `pub(super)` from this child grants, so no field is reachable from anywhere
//! it was not reachable before.
//!
//! Behaviour lives in the siblings `edits.rs`, `selection.rs` and
//! `engine_sync.rs`. Methods keep their paths there because a method hangs off
//! the type, not the module.

use super::*;

/// The inspector tab a session opens on.
///
/// The selected tab is state, so which one it starts as is the app's decision
/// rather than the panel's; the inspector's tab host asserts that a tab with
/// this id is registered, so the two cannot disagree.
pub const DEFAULT_INSPECTOR_TAB: &str = "transform";

pub struct AppState {
    pub ctx: Arc<PluginContext>,
    pub(super) engine: Arc<Mutex<Engine>>,
    /// The one edit path: every command, group, undo, and redo goes through it,
    /// so plugins see the editor's edits and the editor sees theirs.
    pub(super) host: Arc<ProjectHost>,
    pub revision: u64,
    pub playhead: RationalTime,
    pub selection: Vec<ClipId>,
    /// Selected edit point on a clip edge for trimming, if any.
    pub selected_edge: Option<crate::types::SelectedEdge>,
    /// Selected empty gap between clips on a lane, if any.
    pub selected_gap: Option<crate::types::SelectedGap>,
    /// Live on-screen trim HUD feedback while a trim gesture is in flight.
    pub trim_feedback: Option<crate::types::TrimFeedback>,
    /// Height fraction `[0.15, 0.85]` of the lane stack given to video lanes
    /// above the A/V divider.
    pub timeline_av_split: f32,
    /// Vertical scroll offset (px) of the video lane group above the divider.
    pub video_scroll_y: f32,
    /// Vertical scroll offset (px) of the audio lane group below the divider.
    pub audio_scroll_y: f32,
    /// Virtualization and paint counters from the most recent timeline frame.
    pub timeline_paint_stats: crate::types::TimelinePaintStats,
    pub zoom: f32,
    pub scroll_x: f32,
    /// Whether the drawn scroll offset should ease toward [`Self::scroll_x`].
    /// Set by whoever moved the view: false for a hand on a wheel or a drag,
    /// true for a fit or a playhead the transport is following.
    pub scroll_eased: bool,
    /// Whether the drawn scroll offset is settling from a released pan. It rides
    /// the one spec that overshoots, so a flick passes where it was going and
    /// comes back; every other scroll is [`Self::scroll_eased`]'s business.
    pub scroll_fling: bool,
    /// Whether the drawn zoom should ease toward [`Self::zoom`]. Same rule as
    /// [`Self::scroll_eased`]: a pointer-anchored wheel notch has to land under
    /// the pointer that anchored it, a fit or a keyed step arrives.
    pub zoom_eased: bool,
    /// The clip an edit has just created, and when. The timeline reads it to
    /// settle that one card instead of having it appear: an element id cannot
    /// answer this, because a card scrolled back into view is new to the tree
    /// and old to the eye.
    pub(super) landed: Option<(ClipId, std::time::Instant)>,
    pub active_panel: ActivePanel,
    pub ui_settings: UiSettings,
    /// Dockable frame/tab/split/floating/maximize tree.
    pub dock_tree: crate::dock::DockTree,
    /// Currently active workspace id (`"win-workspace-editing"` by default).
    pub active_workspace_id: String,
    /// Incremented when a workspace is applied or reset so the shell animates
    /// frame rectangles over 180 ms.
    pub workspace_transition_epoch: u64,
    pub data_dir: PathBuf,
    /// Theme files under `<data_dir>/themes`, read when Settings opens.
    pub theme_files: crate::theme::file::ThemeFiles,
    /// User animation presets loaded from `<data_dir>/presets/*.ariapreset`.
    pub presets: Vec<aria_models::Preset>,
    /// Any `.ariapreset` files under `<data_dir>/presets/` that failed to load.
    pub broken_presets: Vec<crate::app::presets::BrokenPreset>,
    /// The open "Save as preset" draft (name and part toggles), if any.
    pub preset_draft: Option<crate::app::presets::PresetDraft>,
    /// Driver source being edited in the inspector, if any.
    pub driver_draft: Option<crate::app::drivers::DriverDraft>,
    /// The clip the Parent row's pick whip is armed for: the next clip pressed
    /// on the timeline becomes its parent.
    pub(crate) parent_whip: Option<aria_models::ClipId>,
    /// Recent projects as Home draws them.
    ///
    /// Held rather than read per draw: the list lives in a file and each row's
    /// "missing" flag is a stat of its path, and Home is animated, so drawing it
    /// from disk cost a read plus a dozen stats every frame. Re-read when the
    /// list changes and when Home is entered, which is when it can have changed
    /// under the editor.
    pub(super) recents: Arc<[crate::app::recents::RecentView]>,
    /// The Home body's scroll position, shared by every rail's pane. Choosing
    /// a rail puts it back at the top.
    pub(crate) home_scroll: gpui::ScrollHandle,
    /// File > Import Recent, newest first. Read once at startup and written
    /// through on every change; see `app::import_recents`.
    pub(super) import_recents: Arc<[PathBuf]>,
    /// The projection every panel draws from.
    ///
    /// Shared rather than owned: three panels need it for the length of a frame
    /// and cannot hold a borrow of the state across the listeners they attach,
    /// so each used to deep-copy the whole sequence — every track, every clip,
    /// every keyframe — once per render. A write takes it back through
    /// `AppState::store_mut`, which copies only while a frame still holds the
    /// old one.
    pub store: Arc<UiEditorStore>,
    /// What [`Self::store`] was projected from, or `None` before the first
    /// projection. A poll whose inputs match this key skips the projection
    /// entirely; see [`ProjectionKey`](crate::app::engine_sync::ProjectionKey).
    pub(crate) projected: Option<crate::app::engine_sync::ProjectionKey>,
    /// The selected clip as the model holds it, projected beside
    /// [`Self::store`] and refreshed by the same poll.
    ///
    /// The inspector's values — a parameter at the playhead, its keyframes, the
    /// ramp, a title's generator, the overlay quad — are not in the store, and
    /// answering them from the session took a lock per element per frame while a
    /// preview composite held the same lock for its whole decode. See
    /// [`selected_detail`](crate::app::selected_detail).
    pub(crate) selected: Option<crate::app::selected_detail::SelectedDetail>,
    /// The session facts a frame reads that the store does not carry: the
    /// canvas, the size it renders at, which pool items are in use, the undo
    /// list. See [`project_facts`](crate::app::project_facts).
    pub(crate) facts: crate::app::project_facts::ProjectFacts,
    /// What the project report counts, measured when it is asked for rather
    /// than once per repaint. See [`project_report`](crate::app::project_report).
    pub(crate) report: crate::app::project_report::ProjectReport,
    /// Stage timings of the last frame the editor decoded, recorded where the
    /// frame lands so a readout never asks the session for them.
    pub(crate) frame_stats: aria_render::FrameStats,
    pub last_frame: Option<Arc<RgbaImage>>,
    pub last_render: Option<Arc<RenderImage>>,
    /// Whether the preview shows the scope strip. A scope costs a read of the
    /// frame, so it is measured only while it is being looked at.
    pub scopes_open: bool,
    /// The scopes measured from `last_frame`, or `None` when the strip is
    /// closed or no reading has arrived yet.
    pub scope_textures: Option<scope_view::ScopeTextures>,
    /// The sequence tick `last_frame` shows, or `None` before the first frame.
    pub(crate) shown_tick: Option<i64>,
    /// Viewer and scope textures no longer on screen, waiting to leave the
    /// window atlas. See [`AppState::release_retired_images`].
    pub(crate) retired_images: Vec<Arc<RenderImage>>,
    /// The session's preview render thread: Viewer frames, render-ahead,
    /// scopes, and pool pictures. See [`crate::preview_worker`].
    pub(crate) preview_worker: crate::preview_worker::PreviewWorker,
    pub playing: bool,
    pub last_error: Option<String>,
    pub dialog: DialogKind,
    pub route: UiRoute,
    pub project_path: Option<PathBuf>,
    pub status_message: Option<String>,
    /// Why a missing pool entry was not relinked, by media id, from the last
    /// Relink All. Held here rather than on the pool row because the row is
    /// reprojected from the project every frame and the project has no place
    /// for a UI verdict. Cleared when a relink runs again.
    pub relink_notes: std::collections::HashMap<u64, String>,
    /// The status text currently on the strip and when it first appeared, used
    /// to age it out. Not written by the forty-odd places that post a status:
    /// `age_status` notices the text changed and starts the clock itself.
    pub status_seen: Option<(String, std::time::Instant)>,
    /// The picked inspector tab, by id. A string because the tab
    /// list is data: a tab has no enum variant to be.
    pub inspector_tab: String,
    pub sidebar_tab: SidebarTab,
    pub context_menu: Option<ContextTarget>,
    pub context_at: (f32, f32),
    /// The rows plugins added to the menu last opened, in mount order. Kept
    /// after the menu closes so its fade out draws the same rows.
    pub(crate) context_plugin_rows: Arc<[super::ContextPluginRow]>,
    pub palette_query: String,
    pub(crate) palette_cache: std::cell::RefCell<crate::app::palette::PaletteCache>,
    /// Which of the palette's current hits is highlighted. Clamped on read, so a
    /// query that shortens the list cannot leave it pointing past the end.
    pub palette_index: usize,
    /// The palette list's scroll position. A move of the highlight scrolls the
    /// list through it, so the highlighted row is drawn in view.
    pub palette_scroll: gpui::ScrollHandle,
    /// In-progress inline rename, if any.
    pub rename: Option<crate::types::RenameTarget>,
    pub rename_draft: String,
    /// The caret and selection in `rename_draft`.
    pub(crate) rename_caret: super::rename::RenameCaret,
    /// The clock's draft while a playhead field holds focus. `None` means the
    /// fields are showing the playhead.
    pub clock_draft: Option<String>,
    /// The playhead field the draft is typed into: the Viewer's or the
    /// Timeline's.
    pub(crate) clock_site: crate::app::clock::ClockSite,
    /// Cached playhead timecode for the unfocused playhead field.
    pub(crate) clock_display: gpui::SharedString,
    /// The tick a pointer last put the playhead on: the ruler, the Viewer's
    /// scrub strip, a property row's strip. `None` once anything else moves it.
    pub(crate) pointer_playhead: Option<i64>,
    /// Timeline property rows: reveal modes, keyframe selection and clipboard,
    /// the graph editor, and in-flight keyframe drags.
    pub(crate) prop_rows: super::property_rows::PropertyRowsState,
    /// The live rubber band on the timeline, while one is being dragged.
    pub marquee: Option<crate::app::marquee::Marquee>,
    /// How the preview is being looked at: guides, zoom, pan, fullscreen.
    pub preview_view: crate::app::preview_view::PreviewView,
    /// What the magnet last locked onto, while a drag is in flight. Carries the
    /// reason, not just the tick, so the guide can name itself.
    pub snap_guide: Option<crate::snap::SnapCandidate>,
    /// Every magnet in the sequence, sorted by tick.
    ///
    /// Rebuilt when a projection is adopted, which is when a clip edge, a beat,
    /// a marker or the sequence length can have moved. A drag frame reads it
    /// instead of walking every clip and every beat to build the same list again
    /// once per mouse move.
    pub(super) snap_index: std::sync::OnceLock<crate::snap::SnapIndex>,
    /// The lane being resized and its previewed height, while the drag is in
    /// flight. Held here rather than written through the engine so one gesture
    /// costs one undo entry. Read through [`AppState::lane_preview`]: a panel
    /// asks which height to draw, never which field holds it.
    pub(super) lane_resize: Option<(u64, f32)>,
    /// The clips drawn with their keyframe lane open.
    ///
    /// Behind a handle because the timeline reads the whole set once per frame
    /// and a frame cannot hold a read borrow while it builds listeners: the draw
    /// clones the handle, and a toggle takes the set back with copy-on-write.
    pub expanded_clips: std::sync::Arc<Vec<u64>>,
    /// The model of each clip in `expanded_clips`, and whether it sits on an
    /// audio lane, projected when the set or the project changes so the
    /// property rows draw without taking the session lock.
    pub(crate) expanded_models: std::collections::HashMap<u64, (aria_models::Clip, bool)>,
    /// The lane being dragged by its header, and the row the drop would land on.
    pub track_drag: Option<u64>,
    pub track_drop_row: Option<usize>,
    /// The ramp point under the pointer, and the last measured graph rectangle
    /// (x, y, w, h in window pixels) the pointer is read against.
    pub ramp_drag: Option<clip_speed::RampDrag>,
    pub ramp_frame: Option<[f32; 4]>,
    /// The curve-editor keyframe or bezier handle under the pointer, and the
    /// last measured graph rectangle (x, y, w, h in window pixels) it is read
    /// against. Same shape as the ramp above, for the same reason: a drag is a
    /// draft until the release, so one gesture is one undo entry.
    pub curve_drag: Option<curve_drag::CurveDragDraft>,
    pub curve_frame: Option<[f32; 4]>,
    /// The mixer fader under the pointer and the gain it is at. Same shape and
    /// the same reason as the two drags above: riding a fader is a draft until
    /// the release, so one gesture is one undo entry.
    pub fader_drag: Option<mixer::FaderDrag>,
    /// Active playback automation pass state (`Write`, `Touch`, `Latch`).
    pub mixer_automation: mixer::AutomationPassState,
    /// The insert chain whose Add list is open, if any.
    pub insert_picker: Option<aria_models::AudioInsertTarget>,
    /// The insert whose parameters are open, and the chain it is in.
    pub open_insert: Option<(aria_models::AudioInsertTarget, usize)>,
    /// The property the curve editor graphs, and which axis of it when the
    /// property has two. `None` graphs the first animatable property the
    /// selection offers, so the block is never empty while something is
    /// keyframable; a property that stops being reachable falls back the same
    /// way rather than graphing nothing.
    pub curve_param: Option<aria_models::ClipParam>,
    pub curve_axis: u8,
    /// Where the timeline panel was last drawn (x, y, w, h in window pixels).
    ///
    /// A pointer position arrives in window coordinates, and the timeline is not
    /// at the window's left edge: the media rail and its splitter are to the
    /// left of it, and both are resizable and collapsible. Every conversion from
    /// a pointer to a tick reads this through
    /// `AppState::timeline_content_left`, the same way the ramp graph reads
    /// [`Self::ramp_frame`] and the source monitor reads its scrub bar.
    /// `None` before the panel has been drawn once, which is before any pointer
    /// can be over it.
    pub timeline_frame: Option<[f32; 4]>,
    /// Where the last build of the timeline body put the playhead's head, its
    /// lines and its timecode, which the shell paints from on every frame.
    pub(crate) timeline_playhead: crate::timeline_playhead::Layer,
    /// Last painted window bounds `[x, y, w, h]` of each docked frame.
    pub(crate) dock_frame_rects: std::sync::Arc<
        parking_lot::Mutex<std::collections::HashMap<crate::dock::FrameId, [f32; 4]>>,
    >,
    /// The composer, the conversation, and the turn in flight.
    pub assistant: assistant::AssistantForm,
    /// The conversation as the panel draws it.
    ///
    /// Behind a handle for the same reason as [`Self::expanded_clips`], and it
    /// matters more here: this is the one list that grows while a model streams,
    /// so a frame that copied it would copy the whole answer per token.
    pub transcript: std::sync::Arc<Vec<UiAgentEntry>>,
    pub clipboard: Option<ClipId>,
    /// The effect an effect header's Copy took, with every parameter and
    /// keyframe, which a paste adds to a clip's chain. A value rather than a
    /// reference: the source can change or go after the copy.
    pub copied_effect: Option<aria_models::EffectInstance>,
    /// The catalog id of the effect added to a clip last this session, which
    /// the Effect menu offers to apply again.
    pub last_effect: Option<String>,
    pub tool: EditorTool,
    pub tool_session: crate::app::clip_text::tool_hold::ToolSession,
    /// The Viewer's held step button, which repeats while it is down.
    pub step_hold: crate::app::step_hold::StepHold,
    /// A press on the Viewer's playhead field or mini ruler, and where the
    /// ruler was last painted.
    pub viewer_scrub: crate::app::viewer_scrub::ViewerScrub,
    pub edit_mode: EditMode,
    pub snap_enabled: bool,
    /// Sequence > Linked Selection: a click selects a clip with its linked
    /// companions, and a move or trim carries them along. Off, each clip of a
    /// linked pair selects, moves, and trims alone.
    pub linked_selection: bool,
    pub shuttle: crate::transport::ShuttleSpeed,
    pub looping: bool,
    pub(super) work_area_playback: bool,
    pub(crate) k_held: bool,
    pub(super) k_crawl: i32,
    pub mark_in: Option<i64>,
    pub mark_out: Option<i64>,
    /// A press on the timeline's work area bar, until it is released.
    pub(crate) work_area_press: Option<crate::app::work_area::WorkAreaPress>,
    /// A press on the timeline's zoom scroll bar, until it is released.
    pub(crate) zoom_bar_press: Option<crate::app::zoom_bar::ZoomBarPress>,
    /// The export sheet's own choices: resolution, rate, keyframes, audio, and
    /// whether to render only the marked range. Not part of the project.
    pub export_options: crate::app::export_options::ExportOptions,
    /// Whether the export sheet waits on the codec probe, so the window
    /// redraws once it answers.
    pub(crate) codec_probe_pending: bool,
    /// The Speed/Duration dialog's form state.
    pub speed_duration: clip_ops::SpeedDurationForm,
    /// The Audio Gain dialog's form state.
    pub audio_gain: clip_ops::AudioGainForm,
    /// The Remove Attributes dialog's form state.
    pub remove_attributes: crate::app::remove_attributes::RemoveAttributesForm,
    /// The scroll offset the last playhead follow left, so a later offset
    /// that differs was set by another writer and holds the follow off. See
    /// `AppState::follow_playhead`.
    pub follow_scroll: Option<f32>,
    /// The Keyframe Velocity dialog's form state.
    pub keyframe_velocity: crate::app::animation_verbs::KeyframeVelocityForm,
    /// The Effects panel's search, open folders and picked row.
    pub effects_browser: crate::app::effects_browser::EffectsBrowser,
    pub open_menu: Option<AppMenu>,
    /// The row of the open drop the keyboard lit, which Enter runs. `None` for a
    /// drop the pointer opened. See [`crate::app::menu_keys`].
    pub menu_row: Option<usize>,
    /// The submenu whose flyout is open, as the drop entry that holds it, and
    /// the row of the flyout the keyboard lit. `None` while no flyout is open.
    pub menu_flyout: Option<(usize, Option<usize>)>,
    /// The button the open drop was opened from, as its element id, when a
    /// button rather than a title opened it. The button then draws the open
    /// flyout under itself, alone, and the title stays shut. `None` for a drop
    /// opened at its title or from the keyboard.
    pub menu_owner: Option<&'static str>,
    pub interaction: std::sync::Arc<crate::interaction::InteractionGate>,
    pub(super) save_generation: u64,
    pub(super) preview_generation: u64,
    /// Whether a preview decode is running. One runs at a time: a decode holds
    /// the engine lock for its whole length, so a second would only wait for the
    /// first and then land a frame the playhead has already left.
    pub(super) preview_decoding: bool,
    /// Whether another frame was asked for while a decode was running. The
    /// decode in flight starts it when it lands, which is how a scrub of forty
    /// mouse moves costs two decodes instead of forty.
    pub(super) preview_wanted: bool,
    /// Whether the project changed or the playhead moved while paused since the
    /// last frame was asked for, so the frame on screen is not the one the
    /// Viewer should show. The shell's next draw asks for the frame again; any
    /// request clears it.
    pub(super) still_stale: bool,
    /// The stop for the decode in flight. A newer frame cancels it, so the
    /// decode drops the layers the playhead has already passed instead of
    /// finishing a picture nobody will see. `Cancel::never()` while no decode is
    /// running.
    pub(super) preview_cancel: aria_core::Cancel,
    pub(super) playback_generation: u64,
    /// One clone per playback vsync callback the window holds: its strong
    /// count less one is the number of presenting loops still armed.
    pub(crate) playback_vsyncs: std::rc::Rc<()>,
    pub(super) playback_anchor_tick: i64,
    pub(super) playback_started: Option<Instant>,
    /// Which ticks the run shows, while one is presenting.
    pub(super) playback_cadence: Option<crate::preview_worker::Cadence>,
    /// The newest slot of the run whose turn has come, shown or dropped.
    pub(super) playback_shown: i64,
    /// When the run's last vsync was handled.
    pub(super) last_present: Option<Instant>,
    /// Frames dropped since the indicator was last reset, across runs.
    pub(super) dropped: aria_render::DroppedFrames,
    /// The Auto playback resolution level and the drops it steps on.
    pub(super) auto_resolution: crate::resolution::AutoResolution,
    /// Whether the Viewer's resolution list is open.
    pub(crate) resolution_list_open: bool,
    /// What playback is actually managing, against the sequence's own rate.
    ///
    /// One counter for the session's current run: it is re-made at the rate
    /// playback starts at, so a report never mixes two rates, and the panel
    /// reads the same numbers the render gate measures.
    pub(super) playback: aria_render::PlaybackMeter,
    pub audio: crate::audio::AudioHandle,
    pub last_window_w: f32,
    pub last_window_h: f32,
    /// The session's download cache. One instance, because the current root and the
    /// pinned paths are session state.
    pub(super) download_cache: aria_cloud::cache::DownloadCache,
    /// Per-feed cloud load state. Session-scoped: a catalog is re-fetched on the
    /// next launch rather than trusted from a previous one.
    pub cloud_feeds: cloud_feeds::Feeds,
    /// The stock search box.
    pub stock_query: String,
    /// Whether the stock search box has keyboard focus. Typing is captured only while
    /// it does, so the single-letter tool shortcuts keep working with the tab open.
    pub stock_search_focus: bool,
    /// Which media kind stock search asks for.
    pub stock_kind: aria_cloud::dto::StockKind,
    /// The bin's filter text. Session state: a filter is a way of looking at the
    /// pool, not a property of the project, so it is never saved.
    pub media_query: String,
    /// Whether the bin filter box has keyboard focus.
    pub media_search_focus: bool,
    /// How the bin orders what it shows.
    pub media_sort: crate::types::MediaSort,
    /// Whether the bin's order menu is open.
    pub(crate) media_sort_list_open: bool,
    /// The picked pool items, in pick order. Its focus is the
    /// last one acted on, which is what a single-item row means; the whole set
    /// is what a multicam group is built from.
    pub pool_selection: crate::app::pool_selection::PoolSelection,
    /// The pool folder the bin is showing, or `None` for the pool root. Session
    /// state: which folder is open is a way of looking at the pool.
    pub current_bin: Option<u64>,
    /// A folder rename in flight, if any.
    pub bin_rename: Option<crate::app::bins::BinRename>,
    /// The folders directly inside the open one, in draw order.
    ///
    /// A projection beside the store, replaced when the pool's folders or the
    /// open one change: the strip clones a handle instead of walking the pool
    /// and copying every folder it keeps, twice, per frame. One owner rebuilds
    /// both, `AppState::refresh_bins`.
    pub bin_children: std::sync::Arc<[crate::types::UiBin]>,
    /// The open folder and everything above it, root first: the breadcrumb.
    pub bin_trail: std::sync::Arc<[crate::types::UiBin]>,
    /// Rendered pool thumbnails, keyed by source and source tick.
    pub pool_preview: crate::app::pool_preview::PoolPreviewStore,
    /// The pool row under the pointer and the source tick it is scrubbed to.
    pub pool_hover: Option<(u64, i64)>,
    /// The Viewer's Source tabs and which tab has the keys.
    pub sources: crate::app::viewer_tabs::SourceTabs,
    /// What View > Compare holds beside the Program frame, and how it is drawn.
    pub viewer_compare: crate::app::viewer_compare::ViewerCompare,
    /// Rulers and guides over the Program view.
    pub viewer_guides: crate::app::viewer_guides::ViewerGuides,
    /// Decoded audio peaks per media entry, filled off-thread and read by the
    /// timeline. Session-scoped: peaks describe a file, not the project.
    pub waveforms: crate::waveforms::WaveformStore,
    /// Rendered filmstrip cells, keyed by tick and invalidated by picture span.
    pub filmstrips: crate::filmstrips::FilmstripStore,
    /// The text-preset pack currently loaded from the cloud, if any. Session-scoped:
    /// a preset is baked onto the clip when applied, so nothing here is saved.
    /// A pack is replaced whole rather than edited, so this is a shared slice
    /// rather than a shared vector: the panel clones the handle, and one preset
    /// is copied when a row is clicked.
    pub text_presets: std::sync::Arc<[aria_cloud::dto::TextPreset]>,
    /// The background-job registry. Cloned from the plugin context so every producer
    /// in the process reports into the one board.
    pub(crate) jobs: aria_jobs::JobManager,
    /// Sequential render queue for background exports.
    pub(crate) export_queue: std::sync::Arc<crate::app::export_queue::ExportQueue>,
    /// Whether the job subscriber is installed. One per session: a second subscriber
    /// would wake the entity twice per transition.
    pub(crate) jobs_watched: bool,
    /// The subscription that re-projects after edits from plugins and the
    /// assistant. One per session, dropped with it.
    pub(crate) edits_watch: Option<aria_plugin::Registration>,
    /// The view plugins read through `EditorView`, recorded once per drawn
    /// frame by [`Self::publish_view`].
    pub(crate) editor_view: Arc<aria_plugin::EditorView>,
    /// Pool items already announced as `MediaOffline`, so an item that stays
    /// missing is announced once and one that returns can be again.
    pub(crate) offline_announced: std::collections::HashSet<u64>,
    /// Services this session registered on a bare context, for its lifetime.
    pub(crate) _services: Vec<aria_plugin::Registration>,
    /// The board, as every surface that draws pending work reads it.
    ///
    /// A projection, like the sequence: rebuilt when the registry publishes a
    /// transition or the export queue changes, and read from a handle by the
    /// pending strip, the jobs board, the export sheet and the status line. A
    /// frame takes no registry lock and copies no job.
    pub job_view: std::sync::Arc<[jobs::UiJob]>,
    /// The Generate tab's prompt, kind, and focus.
    pub generate: generate::GenerateForm,
    /// Distinguishes two generations of the same prompt, so the provider does not
    /// dedupe the second one against the first through the idempotency key.
    pub(crate) generation_seq: u64,
    /// The catalog entry currently previewed by hover, mirrored so a repeat hover over
    /// the same entry costs no frame. The engine holds the authoritative override.
    pub(crate) animation_preview: Option<(
        aria_models::ClipId,
        aria_models::AnimationSlot,
        Option<aria_models::AnimationRef>,
    )>,
    /// The Captions panel's language, granularity, commit target, and last transcript.
    pub captions: captions::CaptionsForm,
    /// The Analysis panel's rows, filters, contour, and beat ticks.
    pub analysis: analysis::AnalysisForm,
    /// How deep and how fast the next duck dips.
    pub duck: duck::DuckForm,
    /// The Media Support dialog's report, normalise target, and exposure readout.
    pub media_reach: media_reach::MediaReachForm,
    /// What the last timeline import or export placed or wrote the nearest way
    /// it could, while the Timeline File dialog shows it.
    pub timeline_report: Option<timeline_files::TimelineReport>,
    /// Measured cache usage, held between renders because measuring walks the disk.
    pub caches: caches::CacheForm,
    /// The Settings dialog's focused field, draft text, and section filter.
    pub settings_form: settings_form::SettingsForm,
    /// The Keys section's named sets, its keyboard image modifiers, and the
    /// move a taken chord is asking about. Session state.
    pub(crate) key_form: super::keymaps::KeyForm,
    /// The Marker Edit dialog's draft fields, focus, and validation error.
    pub marker_edit: super::MarkerEditForm,
    /// Active colour filter in the Markers panel (`None` shows all colours).
    pub marker_color_filter: Option<aria_models::MarkerColor>,
    /// What the AI endpoint last answered: its model list, and the connection
    /// test's result line. Session state — a model list is a fact about a
    /// server, not about the project.
    pub ai_probe: ai_reach::AiProbe,
    /// The rectangles the preview last painted: the canvas and the selected clip's
    /// quad, in window pixels. Recorded during paint because a pointer event carries
    /// a window position and nothing else.
    pub(crate) overlay_frame: canvas_overlay::FrameBox,
    /// The on-canvas handle being dragged, with the geometry the gesture has built
    /// so far. Held here rather than committed per move, so one drag is one edit.
    pub(crate) overlay_drag: Option<canvas_overlay::OverlayDrag>,
    /// The Viewer pixel under the pointer and the colour read there, for the
    /// Info panel.
    pub(crate) viewer_probe: info::ViewerProbe,
    /// What the Align panel lines the selection up against.
    pub(crate) align_to: align::AlignTo,
    /// The action requested while the project had unsaved edits, held until
    /// the prompt is answered. Only ever set while the unsaved-changes
    /// dialog is up.
    pub(crate) pending_intent: Option<unsaved::PendingIntent>,
    /// Set once leaving the app has been answered, or needed no answer. The
    /// shell quits on the next frame it draws.
    pub(crate) quit_requested: bool,
    /// The recovery copy: when the last one started, and the one still being
    /// written.
    pub(crate) autosave: recovery::Autosave,
    /// Which on-canvas handles the preview offers. Session state: how you are
    /// looking at the picture, not part of the project.
    pub(crate) overlay_mode: crate::types::OverlayMode,
    /// The binding whose next keystroke is its new chord, while Settings is
    /// waiting for one. Session state: an armed capture that survived a restart
    /// would eat the first key of the next session.
    pub(crate) rebinding: Option<&'static str>,
    /// The level the meter is drawing, after ballistics. Held rather than read
    /// straight from the audio path so the fall rate is the repaint clock's.
    pub(crate) meter: meter::MeterReading,
    /// When the meter was last advanced, for the fall rate.
    pub(crate) meter_at: Option<Instant>,
    /// What each mixer lane's bars are drawing, after the same ballistics the
    /// programme meter uses. Keyed by lane, so a strip reads its own level and a
    /// lane the mix is not carrying reads none.
    pub(crate) strip_meters: Vec<(aria_models::TrackId, [f32; 2])>,
    /// How many times the store has been rebuilt from the engine. A scrub must
    /// not move it: the projection carries the playhead as a value and derives
    /// nothing from it.
    pub(crate) projections: u64,
    /// The picks the session has prompted for, and the answers a test supplied
    /// in their place. One owner: `picker` is the only way a dialog
    /// opens.
    pub(crate) picks: picker::Picks,
    /// Plugin-declared panels as last read, and the one draft and drag a
    /// declared control keeps on this side.
    pub plugin_panels: crate::app::plugin_panels::PluginPanels,
    /// Plugin viewer overlays as last read, the handle the pointer holds, and
    /// their tessellated shapes.
    pub plugin_overlays: crate::app::plugin_overlays::PluginOverlays,
    /// Installed plugin packages, and the archives waiting for the install
    /// prompt.
    pub plugin_installs: crate::app::plugin_install::PluginInstalls,
    /// Latest timeline render-bar segments computed off the UI thread.
    pub render_bar: std::sync::Arc<Vec<crate::app::render_files::RenderSegment>>,
    pub(crate) render_files_handle: std::sync::Arc<aria_render::RenderFiles>,
    /// The mounted-plugin revision the render-files environment was formatted
    /// at; an edit reformats it only when the revision has moved since.
    pub(crate) render_files_mounts: u64,
    pub(crate) render_files_generation: std::sync::Arc<std::sync::atomic::AtomicU64>,
    pub(crate) render_bar_state: crate::app::render_bar::RenderBarState,
    /// The readout a press holds, and the one open as text.
    pub(crate) value_scrubs: crate::app::value_scrub::ValueScrubs,
    /// The press held on a clip bar's line.
    pub(crate) line_drag: Option<crate::app::clip_line::LineDrag>,
    /// The colour wheel a press holds, and the Color panel's open sections.
    pub(crate) grade_controls: crate::app::grade_controls::GradeControls,
}
