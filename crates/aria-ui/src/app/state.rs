//! The [`AppState`] struct: every field of the running editor.
//!
//! Moved out of `app.rs` to stay under the file ceiling. The definition is
//! unchanged except that the seven fields that were private to `app` are now
//! `pub(super)` — the same visibility by a different spelling. Private-to-`app`
//! already meant visible in `app` and every descendant, which is exactly what
//! `pub(super)` from this child grants, so no field is reachable from anywhere
//! it was not reachable before.
//!
//! Behaviour lives in the siblings `edits.rs` and `selection.rs`. Methods keep
//! their paths there because a method hangs off the type, not the module.

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
    pub revision: u64,
    pub playhead: RationalTime,
    pub selection: Vec<ClipId>,
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
    pub data_dir: PathBuf,
    /// Saved effect presets loaded from the profile data directory.
    pub effect_presets: Vec<aria_models::EffectPreset>,
    /// The name being typed for a new effect preset, when the field is open.
    ///
    /// A draft rather than a flag plus a string: the field is open exactly when
    /// there is something to type into, which is what the key router reads.
    pub effect_preset_draft: Option<String>,
    /// Recent projects as Home draws them.
    ///
    /// Held rather than read per draw: the list lives in a file and each row's
    /// "missing" flag is a stat of its path, and Home is animated, so drawing it
    /// from disk cost a read plus a dozen stats every frame. Re-read when the
    /// list changes and when Home is entered, which is when it can have changed
    /// under the editor.
    pub(super) recents: Arc<[crate::app::recents::RecentView]>,
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
    /// entirely; see [`ProjectionKey`](crate::app::selection::ProjectionKey).
    pub(crate) projected: Option<crate::app::selection::ProjectionKey>,
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
    /// The scopes painted from `last_frame`, or `None` when the strip is closed
    /// or no frame has arrived yet.
    pub scope_textures: Option<scope_view::ScopeTextures>,
    /// Reused CPU pixel storage for scope refreshes.
    pub(crate) scope_images: Option<crate::scopes::ScopeImages>,
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
    /// The inspector tab the operator picked, by id. A string because the tab
    /// list is data: a tab has no enum variant to be.
    pub inspector_tab: String,
    pub sidebar_tab: SidebarTab,
    pub context_menu: Option<ContextTarget>,
    pub context_at: (f32, f32),
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
    /// The clock's draft while the tool-strip timecode holds focus. `None` means
    /// the readout is showing the playhead.
    pub clock_draft: Option<String>,
    /// Cached playhead timecode for the unfocused toolbar clock.
    pub(crate) clock_display: gpui::SharedString,
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
    pub(super) snap_index: crate::snap::SnapIndex,
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
    /// The composer, the conversation, and the turn in flight.
    pub assistant: assistant::AssistantForm,
    /// The conversation as the panel draws it.
    ///
    /// Behind a handle for the same reason as [`Self::expanded_clips`], and it
    /// matters more here: this is the one list that grows while a model streams,
    /// so a frame that copied it would copy the whole answer per token.
    pub transcript: std::sync::Arc<Vec<UiAgentEntry>>,
    pub clipboard: Option<ClipId>,
    pub tool: EditorTool,
    pub edit_mode: EditMode,
    pub snap_enabled: bool,
    pub shuttle: i32,
    pub mark_in: Option<i64>,
    pub mark_out: Option<i64>,
    /// The export sheet's own choices: resolution, rate, keyframes, audio, and
    /// whether to render only the marked range. Not part of the project.
    pub export_options: crate::app::export_options::ExportOptions,
    pub open_menu: Option<AppMenu>,
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
    /// The stop for the decode in flight. A newer frame cancels it, so the
    /// decode drops the layers the playhead has already passed instead of
    /// finishing a picture nobody will see. `Cancel::never()` while no decode is
    /// running.
    pub(super) preview_cancel: aria_core::Cancel,
    pub(super) playback_generation: u64,
    pub(super) playback_anchor_tick: i64,
    pub(super) playback_started: Option<Instant>,
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
    /// The pool items the operator has picked, in pick order. Its focus is the
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
    /// The source monitor's contents, when it is open over the sequence.
    pub source_view: Option<crate::app::pool_preview::SourceView>,
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
    /// The Media support panel's report, normalise target, and exposure readout.
    pub media_reach: media_reach::MediaReachForm,
    /// Measured cache usage, held between renders because measuring walks the disk.
    pub caches: caches::CacheForm,
    /// The Settings dialog's focused field, draft text, and section filter.
    pub settings_form: settings_form::SettingsForm,
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
    /// What the operator asked for while the project had unsaved edits, held
    /// until they answer the prompt. Only ever set while the unsaved-changes
    /// dialog is up.
    pub(crate) pending_intent: Option<unsaved::PendingIntent>,
    /// When the last recovery copy was written. `None` means none this session,
    /// so the first dirty tick writes one.
    pub(crate) last_autosave: Option<Instant>,
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
    /// The picks the session has asked the operator for, and the answers a test
    /// supplied in their place. One owner: `picker` is the only way a dialog
    /// opens.
    pub(crate) picks: picker::Picks,
}
