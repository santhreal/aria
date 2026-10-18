//! `AppState` commands: 14 methods moved out of `app.rs`.
//!
//! A pure move to stay under the file ceiling. These are inherent methods, so
//! they keep their existing paths — a method belongs to the type, not to the
//! file it is written in. No caller changes.

use super::*;

enum FilmstripInvalidation {
    Full,
    Spans(Vec<(i64, i64)>),
}

struct FilmstripImpact {
    full: bool,
    picture: bool,
    clips: Vec<aria_models::ClipId>,
    tracks: Vec<aria_models::TrackId>,
    media: Option<aria_models::MediaId>,
    before: Vec<(i64, i64)>,
    before_tracks: Vec<(i64, i64)>,
    wide_tracks: bool,
}

impl FilmstripImpact {
    fn capture(command: &Command, project: &aria_models::Project) -> Self {
        let mut impact = Self {
            full: false,
            picture: false,
            clips: Vec::new(),
            tracks: Vec::new(),
            media: None,
            before: Vec::new(),
            before_tracks: Vec::new(),
            wide_tracks: false,
        };
        match command {
            Command::Project(
                aria_commands::ProjectCommand::Open { .. }
                | aria_commands::ProjectCommand::Load { .. }
                | aria_commands::ProjectCommand::ApplyTemplate { .. },
            ) => impact.full = true,
            Command::Project(aria_commands::ProjectCommand::RelinkMedia { media, .. }) => {
                impact.picture = true;
                impact.media = Some(*media);
            }
            Command::Project(
                aria_commands::ProjectCommand::Import { .. }
                | aria_commands::ProjectCommand::RemoveMedia { .. }
                | aria_commands::ProjectCommand::Save { .. }
                | aria_commands::ProjectCommand::SetMediaLabel { .. }
                | aria_commands::ProjectCommand::Export { .. }
                | aria_commands::ProjectCommand::SaveTemplate { .. },
            ) => {}
            Command::Edit(edit) => {
                let targets = edit.targets();
                impact.picture =
                    !picture_neutral_edit(edit) && targets_touch_picture(project, &targets);
                if impact.picture {
                    impact.clips = targets.clips;
                    impact.tracks = targets.tracks;
                    impact.wide_tracks = matches!(
                        edit,
                        EditCommand::FreezeFrame { .. }
                            | EditCommand::RippleDelete { .. }
                            | EditCommand::ShiftClips { .. }
                            | EditCommand::RippleInsert { .. }
                            | EditCommand::FitToFill { ripple: true, .. }
                            | EditCommand::SourceEdit { ripple: true, .. }
                            | EditCommand::SetTrackSolo { .. }
                    );
                    if matches!(
                        edit,
                        EditCommand::FreezeFrame { .. } | EditCommand::RippleDelete { .. }
                    ) {
                        for clip in &impact.clips {
                            if let Some(track) = track_containing(project, *clip)
                                && !impact.tracks.contains(&track)
                            {
                                impact.tracks.push(track);
                            }
                        }
                    }
                    if let EditCommand::SetTrackSolo { track, .. } = edit
                        && let Some(kind) = project.timeline().track(*track).map(|track| track.kind)
                    {
                        impact.tracks = project
                            .timeline()
                            .tracks_ordered()
                            .filter(|track| track.kind == kind)
                            .map(|track| track.id)
                            .collect();
                    }
                }
            }
        }
        impact.before = impact.spans_in(project);
        impact.before_tracks = impact.track_spans_in(project);
        impact
    }

    fn finish(
        mut self,
        outcome: &ApplyOutcome,
        project: &aria_models::Project,
    ) -> FilmstripInvalidation {
        if self.full {
            return FilmstripInvalidation::Full;
        }
        if !self.picture {
            return FilmstripInvalidation::Spans(Vec::new());
        }
        let after = self.spans_in(project);
        self.before.extend(after);
        if self.wide_tracks {
            self.before.extend(&self.before_tracks);
            let after_tracks = self.track_spans_in(project);
            self.before.extend(after_tracks);
        }
        match outcome {
            ApplyOutcome::Opened
            | ApplyOutcome::Loaded
            | ApplyOutcome::AppliedTemplate
            | ApplyOutcome::Edited(
                aria_commands::EditOutcome::UpdatedCanvas
                | aria_commands::EditOutcome::UpdatedSequenceRate
                | aria_commands::EditOutcome::OpenedSequence,
            ) => return FilmstripInvalidation::Full,
            ApplyOutcome::Edited(aria_commands::EditOutcome::Created(clip))
            | ApplyOutcome::Edited(aria_commands::EditOutcome::Updated(clip)) => {
                push_clip_span(project, *clip, &mut self.before);
            }
            ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedTrack(track))
            | ApplyOutcome::Edited(aria_commands::EditOutcome::ShiftedTrack(track))
            | ApplyOutcome::Edited(aria_commands::EditOutcome::UpdatedTrack(track)) => {
                if !self.wide_tracks {
                    self.before.extend(&self.before_tracks);
                    let after_tracks = self.track_spans_in(project);
                    self.before.extend(after_tracks);
                }
                push_track_spans(project, *track, &mut self.before);
            }
            ApplyOutcome::Edited(aria_commands::EditOutcome::RemovedTrack(_)) => {
                self.before.extend(&self.before_tracks);
            }
            ApplyOutcome::Imported { .. }
            | ApplyOutcome::RemovedMedia { .. }
            | ApplyOutcome::Saved
            | ApplyOutcome::Relinked { .. }
            | ApplyOutcome::LabeledMedia { .. }
            | ApplyOutcome::Exported { .. }
            | ApplyOutcome::SavedTemplate
            | ApplyOutcome::Edited(
                aria_commands::EditOutcome::Removed(_)
                | aria_commands::EditOutcome::CreatedMarker(_)
                | aria_commands::EditOutcome::UpdatedMarker(_)
                | aria_commands::EditOutcome::RemovedMarker(_)
                | aria_commands::EditOutcome::UpdatedAudioLayout
                | aria_commands::EditOutcome::UpdatedTimeDisplay
                | aria_commands::EditOutcome::UpdatedMaster
                | aria_commands::EditOutcome::UpdatedProject
                | aria_commands::EditOutcome::CreatedBin(_)
                | aria_commands::EditOutcome::UpdatedPool
                | aria_commands::EditOutcome::UpdatedSourcePatch,
            ) => {}
        }
        self.before.retain(|(start, end)| start < end);
        self.before.sort_unstable();
        self.before.dedup();
        FilmstripInvalidation::Spans(self.before)
    }

    fn spans_in(&self, project: &aria_models::Project) -> Vec<(i64, i64)> {
        let mut spans = Vec::new();
        for clip in &self.clips {
            push_clip_span(project, *clip, &mut spans);
        }
        if let Some(media) = self.media {
            for track in project.timeline().tracks_ordered() {
                for clip in track.clips_ordered() {
                    if matches!(
                        &clip.content,
                        aria_models::ClipSource::Media { media: held, .. } if *held == media
                    ) {
                        spans.push((clip.timeline.start.value, clip.timeline.end_tick()));
                    }
                }
            }
        }
        spans
    }

    fn track_spans_in(&self, project: &aria_models::Project) -> Vec<(i64, i64)> {
        let mut spans = Vec::new();
        for track in &self.tracks {
            push_track_spans(project, *track, &mut spans);
        }
        spans
    }
}

fn track_containing(
    project: &aria_models::Project,
    clip: aria_models::ClipId,
) -> Option<aria_models::TrackId> {
    project
        .timeline()
        .tracks_ordered()
        .find(|track| track.clips_ordered().iter().any(|held| held.id == clip))
        .map(|track| track.id)
}

fn push_clip_span(
    project: &aria_models::Project,
    id: aria_models::ClipId,
    spans: &mut Vec<(i64, i64)>,
) {
    if let Some(clip) = project.clip(id) {
        spans.push((clip.timeline.start.value, clip.timeline.end_tick()));
    }
}

fn push_track_spans(
    project: &aria_models::Project,
    id: aria_models::TrackId,
    spans: &mut Vec<(i64, i64)>,
) {
    if let Some(track) = project.timeline().track(id) {
        spans.extend(
            track
                .clips_ordered()
                .into_iter()
                .map(|clip| (clip.timeline.start.value, clip.timeline.end_tick())),
        );
    }
}

fn targets_touch_picture(
    project: &aria_models::Project,
    targets: &aria_commands::EditTargets,
) -> bool {
    if targets.clips.is_empty() && targets.tracks.is_empty() {
        return true;
    }
    project.timeline().tracks_ordered().any(|track| {
        track.kind.is_visual()
            && (targets.tracks.contains(&track.id)
                || track
                    .clips_ordered()
                    .iter()
                    .any(|clip| targets.clips.contains(&clip.id)))
    })
}

fn picture_neutral_edit(edit: &EditCommand) -> bool {
    match edit {
        EditCommand::ExtractAudio { .. }
        | EditCommand::SetReplaceable { .. }
        | EditCommand::SetTextEditable { .. }
        | EditCommand::SetClipPitch { .. }
        | EditCommand::SetClipAudio { .. }
        | EditCommand::SetClipDenoise { .. }
        | EditCommand::SetClipLabel { .. }
        | EditCommand::SetClipType { .. }
        | EditCommand::SetAudioRole { .. }
        | EditCommand::SetTrackName { .. }
        | EditCommand::SetTrackMuted { .. }
        | EditCommand::SetTrackLocked { .. }
        | EditCommand::SetTrackDuckSource { .. }
        | EditCommand::SetTrackTargeted { .. }
        | EditCommand::SetTrackSyncLock { .. }
        | EditCommand::SetTrackDisplay { .. }
        | EditCommand::SetTrackGain { .. }
        | EditCommand::SetTrackPan { .. }
        | EditCommand::SetTrackAutomation { .. }
        | EditCommand::SetTrackOutput { .. }
        | EditCommand::AddSend { .. }
        | EditCommand::SetSend { .. }
        | EditCommand::RemoveSend { .. }
        | EditCommand::LinkClips { .. }
        | EditCommand::UnlinkClips { .. }
        | EditCommand::GroupClips { .. }
        | EditCommand::UngroupClips { .. }
        | EditCommand::SetClipBeats { .. }
        | EditCommand::ClearBeats { .. }
        | EditCommand::SetAudioLayout { .. }
        | EditCommand::SetTimeDisplay { .. }
        | EditCommand::SetMasterMix { .. }
        | EditCommand::SetSourcePatch { .. } => true,
        EditCommand::SetParamKeyframe { param, .. }
        | EditCommand::RemoveParamKeyframe { param, .. }
        | EditCommand::SetParamConstant { param, .. }
        | EditCommand::SetParamDriver { param, .. }
        | EditCommand::ApplyEasingPreset { param, .. }
        | EditCommand::SetKeyframeEase { param, .. }
        | EditCommand::SetKeyframeRoving { param, .. } => {
            param.scope() == aria_models::ParamScope::Audio
        }
        _ => false,
    }
}

#[cfg(test)]
mod filmstrip_impact_tests {
    use super::*;

    #[test]
    fn solo_invalidation_covers_every_picture_lane_of_the_same_kind() {
        let rate = aria_models::Rational::FPS_24;
        let mut project = aria_models::Project::new("cache", rate);
        let lower = project.add_track(aria_models::TrackKind::Text, "T1");
        let upper = project.add_track(aria_models::TrackKind::Text, "T2");
        let audio = project.add_track(aria_models::TrackKind::Audio, "A1");
        project
            .add_generated(
                lower,
                aria_models::Generator::text("Lower"),
                aria_models::TimeRange::at_rate(0, 24, rate),
            )
            .expect("lower title");
        project
            .add_generated(
                upper,
                aria_models::Generator::text("Upper"),
                aria_models::TimeRange::at_rate(48, 24, rate),
            )
            .expect("upper title");

        let visual = FilmstripImpact::capture(
            &Command::Edit(EditCommand::SetTrackSolo {
                track: upper,
                solo: true,
            }),
            &project,
        );
        let sound = FilmstripImpact::capture(
            &Command::Edit(EditCommand::SetTrackSolo {
                track: audio,
                solo: true,
            }),
            &project,
        );

        assert!(
            visual.picture,
            "soloing a picture lane changes the composite"
        );
        assert!(visual.before_tracks.contains(&(0, 24)));
        assert!(visual.before_tracks.contains(&(48, 72)));
        assert!(!sound.picture, "soloing an audio lane discarded filmstrips");
    }

    #[test]
    fn mixer_only_commands_have_no_picture_impact() {
        let project = aria_models::Project::new("cache", aria_models::Rational::FPS_24);
        let impact = FilmstripImpact::capture(
            &Command::Edit(EditCommand::SetAudioLayout {
                layout: aria_models::AudioLayout::Stereo,
            }),
            &project,
        );

        assert!(!impact.picture);
        assert!(!impact.full);
        assert!(impact.before.is_empty());
    }

    #[test]
    fn ripple_delete_invalidates_the_rest_of_its_picture_lane() {
        let rate = aria_models::Rational::FPS_24;
        let mut project = aria_models::Project::new("cache", rate);
        let track = project.add_track(aria_models::TrackKind::Text, "T1");
        let first = project
            .add_generated(
                track,
                aria_models::Generator::text("First"),
                aria_models::TimeRange::at_rate(0, 24, rate),
            )
            .expect("first title");
        project
            .add_generated(
                track,
                aria_models::Generator::text("Second"),
                aria_models::TimeRange::at_rate(48, 24, rate),
            )
            .expect("second title");

        let impact = FilmstripImpact::capture(
            &Command::Edit(EditCommand::RippleDelete { clip: first }),
            &project,
        );

        assert!(impact.picture);
        assert!(
            impact.wide_tracks,
            "ripple shifts reach the rest of the lane"
        );
        assert!(impact.before_tracks.contains(&(0, 24)));
        assert!(
            impact.before_tracks.contains(&(48, 72)),
            "ripple invalidation stopped at the deleted clip"
        );
    }
}

impl AppState {
    pub fn new(plugin_ctx: PluginContext, data_dir: PathBuf) -> Result<Self, EngineError> {
        // One owner for the profile root: every reader under it (the storage
        // layout, the settings file, the recents list) needs an absolute path,
        // and a relative override is resolved here rather than asserted there.
        let data_dir =
            crate::settings::absolute_data_dir(&data_dir).map_err(EngineError::Refused)?;
        // A session built on a bare context (tests, the headless check)
        // registers what the plugins would have, and keeps the registrations
        // for its own lifetime.
        let mut services = Vec::new();
        let engine = match plugin_ctx.try_get::<Mutex<Engine>>() {
            Some(shared) => shared,
            None => {
                let mut engine = Engine::new(EngineConfig::default())?;
                engine.new_session();
                let shared = Arc::new(Mutex::new(engine));
                services.push(plugin_ctx.register_arc(shared.clone()));
                shared
            }
        };
        // The engine plugin registers the host beside the engine; a session built
        // on a bare context (tests, the headless check) makes its own over the
        // same engine, so there is still exactly one edit path.
        let host = match plugin_ctx.try_get::<ProjectHost>() {
            Some(host) => host,
            None => {
                let host = Arc::new(ProjectHost::new(Arc::clone(&engine), &plugin_ctx));
                services.push(plugin_ctx.register_arc(Arc::clone(&host)));
                host
            }
        };
        let editor_view = match plugin_ctx.try_get::<aria_plugin::EditorView>() {
            Some(view) => view,
            None => {
                let view = Arc::new(aria_plugin::EditorView::new());
                services.push(plugin_ctx.register_arc(Arc::clone(&view)));
                view
            }
        };
        if plugin_ctx.analyzers().is_empty() && !plugin_ctx.is_mounted("aria.analysis") {
            let tuning = plugin_ctx
                .try_get::<crate::plugins::DetectorTuning>()
                .map(|t| *t)
                .unwrap_or_default();
            if let Ok(regs) = aria_plugin::register_shipped_analyzers(
                &plugin_ctx,
                tuning.beats,
                tuning.silence,
                tuning.loudness,
            ) {
                services.extend(regs);
            }
        }
        {
            let mut engine = engine.lock();
            if engine
                .project()
                .timeline()
                .tracks_ordered()
                .next()
                .is_none()
            {
                engine.new_session();
            }
        }
        let snapshot = {
            let mut engine = engine.lock();
            let start = RationalTime::new(0, engine.project().timeline().frame_rate);
            Snapshot::from_engine(&mut engine, start, &[], None)
        };
        let download_cache = storage::cache_for(&data_dir);
        // One registry per process: the plugin context already holds it, so a session
        // that built its own board would show an empty list while work ran elsewhere.
        let jobs = match plugin_ctx.try_get::<aria_jobs::JobManager>() {
            Some(shared) => (*shared).clone(),
            None => {
                let manager = aria_jobs::JobManager::new();
                crate::plugins::announce_jobs(&manager, plugin_ctx.event_bus());
                services.push(plugin_ctx.register(manager.clone()));
                manager
            }
        };
        let export_queue = Arc::new(
            crate::app::export_queue::ExportQueue::new_with_device_and_plugins(
                jobs.clone(),
                super::render_device::shared_device(&plugin_ctx),
                Some(plugin_ctx.clone()),
                Arc::clone(plugin_ctx.event_bus()),
            ),
        );
        // Home is the first route, so the list it draws is read once here rather
        // than on the first frame that draws it.
        let recents: Arc<[crate::app::recents::RecentView]> =
            crate::app::recents::visible_recents(&data_dir).into();
        let import_recents: Arc<[PathBuf]> = crate::app::import_recents::load(&data_dir).into();
        let (presets, broken_presets) = crate::app::presets::load_presets(&data_dir);
        let theme_files = crate::theme::file::scan(&data_dir, &plugin_ctx.themes());
        let preview_worker = crate::preview_worker::PreviewWorker::spawn(
            Arc::clone(&engine),
            plugin_ctx.clone(),
        )
        .map_err(|err| {
            EngineError::Refused(format!(
                "couldn't start the preview thread: {err}. Free system resources and reopen."
            ))
        })?;
        let render_files_handle = Arc::new(aria_render::RenderFiles::new(
            data_dir.join("render-files"),
            crate::app::render_files::render_files_environment(&plugin_ctx),
        ));
        let mut state = Self {
            ctx: Arc::new(plugin_ctx),
            engine,
            host,
            revision: snapshot.revision,
            playhead: snapshot.playhead,
            selection: Vec::new(),
            zoom: 1.0,
            scroll_x: 0.0,
            scroll_eased: false,
            scroll_fling: false,
            zoom_eased: false,
            active_panel: ActivePanel::Preview,
            presets,
            broken_presets,
            preset_draft: None,
            driver_draft: None,
            parent_whip: None,
            ui_settings: UiSettings::load(&data_dir),
            dock_tree: crate::dock::DockTree::editing(),
            active_workspace_id: crate::layouts::WORKSPACES[0].id.to_string(),
            workspace_transition_epoch: 0,
            plugin_installs: crate::app::plugin_install::PluginInstalls::open(&data_dir),
            theme_files,
            data_dir,
            recents,
            import_recents,
            store: std::sync::Arc::new(snapshot.store),
            // The first poll projects: a key built here would have to be kept
            // in step with the snapshot above by hand, and being wrong is worse
            // than one projection.
            projected: None,
            selected: snapshot.selected,
            facts: snapshot.facts,
            // Nothing decoded and nothing asked for yet: both fill in on the
            // first frame and the first time the report is opened.
            report: Default::default(),
            frame_stats: Default::default(),
            last_frame: None,
            last_render: None,
            scopes_open: false,
            scope_textures: None,
            shown_tick: None,
            retired_images: Vec::new(),
            preview_worker,
            playing: false,
            last_error: None,
            dialog: DialogKind::None,
            route: UiRoute::Home,
            project_path: None,
            status_message: None,
            relink_notes: std::collections::HashMap::new(),
            status_seen: None,
            inspector_tab: crate::app::DEFAULT_INSPECTOR_TAB.to_string(),
            sidebar_tab: SidebarTab::Media,
            context_menu: None,
            context_at: (240.0, 96.0),
            context_plugin_rows: Arc::from([]),
            palette_query: String::new(),
            palette_cache: Default::default(),
            palette_index: 0,
            palette_scroll: gpui::ScrollHandle::new(),
            rename: None,
            rename_draft: String::new(),
            rename_caret: Default::default(),
            clock_draft: None,
            clock_site: Default::default(),
            clock_display: gpui::SharedString::new_static("00:00:00:00"),
            pointer_playhead: None,
            prop_rows: Default::default(),
            marquee: None,
            preview_view: Default::default(),
            snap_guide: None,
            snap_index: crate::snap::SnapIndex::default(),
            lane_resize: None,
            expanded_clips: Default::default(),
            expanded_models: Default::default(),
            track_drag: None,
            track_drop_row: None,
            ramp_drag: None,
            ramp_frame: None,
            curve_drag: None,
            fader_drag: None,
            insert_picker: None,
            mixer_automation: Default::default(),
            open_insert: None,
            curve_frame: None,
            curve_param: None,
            curve_axis: 0,
            timeline_frame: None,
            dock_frame_rects: Default::default(),
            transcript: Default::default(),
            clipboard: None,
            copied_effect: None,
            last_effect: None,
            tool: EditorTool::Pointer,
            tool_session: Default::default(),
            step_hold: Default::default(),
            viewer_scrub: Default::default(),
            edit_mode: EditMode::Ripple,
            mark_in: None,
            mark_out: None,
            work_area_press: None,
            export_options: Default::default(),
            speed_duration: Default::default(),
            audio_gain: Default::default(),
            remove_attributes: Default::default(),
            keyframe_velocity: Default::default(),
            effects_browser: Default::default(),
            follow_scroll: None,
            snap_enabled: true,
            linked_selection: true,
            shuttle: crate::transport::ShuttleSpeed::ONE,
            looping: false,
            work_area_playback: false,
            k_held: false,
            k_crawl: 0,
            interaction: crate::interaction::InteractionGate::new(),
            save_generation: 0,
            open_menu: None,
            menu_row: None,
            menu_flyout: None,
            menu_owner: None,
            preview_generation: 0,
            preview_decoding: false,
            preview_wanted: false,
            still_stale: false,
            // Nothing is decoding, so nothing is cancellable.
            preview_cancel: aria_core::Cancel::never(),
            playback_generation: 0,
            playback_vsyncs: std::rc::Rc::new(()),
            playback_anchor_tick: 0,
            playback_started: None,
            playback_cadence: None,
            playback_shown: 0,
            last_present: None,
            dropped: aria_render::DroppedFrames::default(),
            auto_resolution: crate::resolution::AutoResolution::default(),
            resolution_list_open: false,
            // Empty until playback runs, at the rate the project opens with.
            playback: aria_render::PlaybackMeter::at_rate(snapshot.playhead.rate),
            audio: crate::audio::AudioHandle::new(),
            last_window_w: 1440.0,
            last_window_h: 900.0,
            download_cache,
            cloud_feeds: cloud_feeds::Feeds::new(),
            stock_query: String::new(),
            stock_search_focus: false,
            stock_kind: aria_cloud::dto::StockKind::Video,
            media_query: String::new(),
            media_search_focus: false,
            media_sort: crate::types::MediaSort::default(),
            media_sort_list_open: false,
            pool_selection: crate::app::pool_selection::PoolSelection::default(),
            current_bin: None,
            bin_rename: None,
            // Filled by the first reconcile, which every projection runs.
            bin_children: std::sync::Arc::from([]),
            bin_trail: std::sync::Arc::from([]),
            pool_preview: pool_preview::PoolPreviewStore::default(),
            pool_hover: None,
            sources: Default::default(),
            viewer_compare: Default::default(),
            viewer_guides: Default::default(),
            waveforms: crate::waveforms::WaveformStore::default(),
            filmstrips: crate::filmstrips::FilmstripStore::default(),
            text_presets: Vec::new().into(),
            jobs,
            export_queue,
            jobs_watched: false,
            edits_watch: None,
            editor_view,
            offline_announced: std::collections::HashSet::new(),
            _services: services,
            job_view: Vec::new().into(),
            generate: generate::GenerateForm::default(),
            generation_seq: 0,
            animation_preview: None,
            captions: captions::CaptionsForm::default(),
            analysis: analysis::AnalysisForm::default(),
            duck: duck::DuckForm::default(),
            media_reach: media_reach::MediaReachForm::default(),
            timeline_report: None,
            assistant: assistant::AssistantForm::default(),
            caches: caches::CacheForm::default(),
            settings_form: settings_form::SettingsForm::default(),
            key_form: super::keymaps::KeyForm::default(),
            marker_edit: Default::default(),
            marker_color_filter: None,
            ai_probe: ai_reach::AiProbe::default(),
            overlay_frame: canvas_overlay::FrameBox::default(),
            overlay_drag: None,
            viewer_probe: info::ViewerProbe::default(),
            align_to: align::AlignTo::default(),
            pending_intent: None,
            quit_requested: false,
            last_autosave: None,
            overlay_mode: crate::types::OverlayMode::default(),
            rebinding: None,
            meter: meter::MeterReading::default(),
            meter_at: None,
            strip_meters: Vec::new(),
            projections: 0,
            picks: picker::Picks::default(),
            plugin_panels: plugin_panels::PluginPanels::default(),
            plugin_overlays: crate::app::plugin_overlays::PluginOverlays::default(),
            landed: None,
            selected_edge: None,
            selected_gap: None,
            trim_feedback: None,
            timeline_av_split: 0.55,
            video_scroll_y: 0.0,
            audio_scroll_y: 0.0,
            timeline_paint_stats: crate::types::TimelinePaintStats::default(),
            render_bar: Arc::new(Vec::new()),
            render_files_handle,
            render_files_generation: Arc::new(std::sync::atomic::AtomicU64::new(0)),
            render_bar_state: crate::app::render_bar::RenderBarState::default(),
            value_scrubs: crate::app::value_scrub::ValueScrubs::default(),
            line_drag: None,
            grade_controls: crate::app::grade_controls::GradeControls::default(),
        };
        // The registry is the only thing that resolves a chord, so the keys the
        // operator rebound are installed before the first keystroke rather than
        // consulted at dispatch.
        state.apply_key_overrides();
        if let Some(ws_id) = state.ui_settings.active_workspace.clone() {
            state.active_workspace_id = ws_id;
        }
        if let Some(saved_tree) = state.ui_settings.dock_tree.clone() {
            let mut tree = saved_tree;
            tree.normalize();
            state.dock_tree = tree;
        } else if state.ui_settings.sidebar_collapsed || state.ui_settings.inspector_collapsed {
            state.dock_tree = state.panes().to_dock_tree();
        }
        // Before the first measurement, so it counts what the waveform showed
        // when the app last closed.
        state
            .preview_worker
            .set_waveform(state.ui_settings.waveform.trace());
        // A restored tree can hold the Scopes panel in front, which measures
        // from the first frame on.
        state.sync_scope_demand();
        {
            let mut engine = state.engine.lock();
            engine.set_preview_proxies(state.ui_settings.use_proxies);
            engine.set_preview_transparency(state.ui_settings.transparency_grid);
        }
        state.snap_index = state.build_snap_index();
        state.refresh_render_files();
        Ok(state)
    }

    pub fn engine(&self) -> &Arc<Mutex<Engine>> {
        &self.engine
    }

    /// The host every edit goes through, for gestures that group commands.
    pub(crate) fn host(&self) -> &Arc<ProjectHost> {
        &self.host
    }

    /// Run a multi-command gesture as one undo step: kept when `body` returns
    /// `Ok`, reverted when it fails or panics. A gesture that half-lands is
    /// worse than one that does not happen. The projection is refreshed either
    /// way.
    pub(crate) fn one_step<T>(
        &mut self,
        body: impl FnOnce(&mut Self) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        self.grouped(None, body)
    }

    /// [`Self::one_step`], with the undo step named `label` rather than after
    /// its first command: a duplicate that adds, writes and moves an effect is
    /// one duplicate in the history, not an add.
    pub(crate) fn one_step_as<T>(
        &mut self,
        label: &'static str,
        body: impl FnOnce(&mut Self) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        self.grouped(Some(label), body)
    }

    fn grouped<T>(
        &mut self,
        label: Option<&'static str>,
        body: impl FnOnce(&mut Self) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let group = self.host.group(&Origin::Editor)?;
        let result = body(self);
        match (&result, label) {
            (Ok(_), Some(label)) => {
                group.commit_as(label);
            }
            (Ok(_), None) => {
                group.commit();
            }
            (Err(_), _) => group.rollback(),
        }
        self.sync_from_engine();
        result
    }

    pub fn apply_command(&mut self, command: Command) -> Result<ApplyOutcome, EngineError> {
        let (outcome, (invalidation, revision)) = self.host.apply_observed(
            &Origin::Editor,
            command,
            FilmstripImpact::capture,
            |impact, outcome, engine| (impact.finish(outcome, engine.project()), engine.revision()),
        )?;
        match invalidation {
            FilmstripInvalidation::Full => self.filmstrips.sync_revision(revision),
            FilmstripInvalidation::Spans(spans) => {
                self.filmstrips.sync_revision_selective(revision, &spans);
            }
        }
        // Every edit funnels through here, so one line covers a drop, a paste, a
        // razor and an insert at the playhead rather than four.
        if let ApplyOutcome::Edited(aria_commands::EditOutcome::Created(clip)) = &outcome {
            self.landed = Some((*clip, std::time::Instant::now()));
        }
        self.sync_from_engine();
        // An error is stale the moment something works: a failure that has been
        // superseded must not keep the strip red.
        self.clear_error();
        self.republish_audio();
        Ok(outcome)
    }

    /// How long a new clip counts as new: the catalog duration of the animation
    /// drawn on the strength of it, so the window and the motion cannot drift.
    const LANDED_MS: u64 = gpui::motion::presets::RESIZE.duration_ms;

    /// The clip that has just been created, while it is still settling.
    ///
    /// The timeline asks once per draw and compares against each card, so the
    /// answer is the id rather than a per-clip predicate. Raw, because the store
    /// rows the timeline draws carry raw ids.
    pub fn landing_clip(&self) -> Option<u64> {
        self.landed
            .filter(|(_, at)| at.elapsed() < std::time::Duration::from_millis(Self::LANDED_MS))
            .map(|(clip, _)| clip.raw())
    }

    /// Undo one step and report it, or report why nothing was undone.
    pub fn undo(&mut self) -> bool {
        self.step_history(ProjectHost::undo, "Nothing to undo")
    }

    /// Redo one step and report it, or report why nothing was redone.
    pub fn redo(&mut self) -> bool {
        self.step_history(ProjectHost::redo, "Nothing to redo")
    }

    fn step_history(
        &mut self,
        step: fn(&ProjectHost) -> Result<bool, EngineError>,
        idle: &str,
    ) -> bool {
        match step(self.host.as_ref()) {
            Ok(true) => {
                self.sync_from_engine();
                self.clear_error();
                self.republish_audio();
                true
            }
            Ok(false) => {
                self.status_message = Some(idle.into());
                false
            }
            Err(err) => {
                self.report_error(&err);
                false
            }
        }
    }

    pub fn add_track(
        &mut self,
        kind: TrackKind,
        name: impl Into<String>,
    ) -> Result<ApplyOutcome, EngineError> {
        self.apply_command(Command::Edit(EditCommand::AddTrack {
            kind,
            name: name.into(),
            index: None,
            pinned: true,
        }))
    }

    pub fn select_clip(&mut self, id: ClipId) {
        let group = self.ui_clip_ref(id).and_then(|clip| clip.grouped_to);
        if let Some(group) = group {
            let mut selected = vec![id];
            for track in &self.store.sequence.tracks {
                if track.locked {
                    continue;
                }
                for clip in &track.clips {
                    if !clip.locked && clip.grouped_to == Some(group) {
                        let member = ClipId::from_raw(clip.id);
                        if !selected.contains(&member) {
                            selected.push(member);
                        }
                    }
                }
            }
            self.selection = selected;
        } else {
            self.selection = vec![id];
        }
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
        // The analysis rows belong to the selected clip's media, so they follow the
        // selection. A session that never analyzed anything reads nothing from disk.
        if let Err(err) = self.refresh_moments() {
            self.report_error(err);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
    }

    /// Every magnet in the sequence, sorted, as the drag path reads them.
    ///
    /// Rebuilt only when a projection is adopted: clip edges, beats, markers and
    /// the sequence bounds change with the project, not with the pointer. The
    /// shape lives in [`crate::snap::index_for`], so the bench that measures it
    /// builds the same index this does.
    pub(crate) fn build_snap_index(&self) -> crate::snap::SnapIndex {
        crate::snap::index_for(&self.store)
    }

    /// The magnets that move without a new projection: the playhead and the two
    /// marks. Handed to the index at query time rather than sorted into it.
    fn loose_magnets(&self) -> Vec<crate::snap::SnapCandidate> {
        use crate::snap::{SnapCandidate, SnapKind};
        let mut loose = vec![SnapCandidate::new(
            self.store.playhead_ticks,
            SnapKind::Playhead,
        )];
        loose.extend(
            self.mark_in
                .map(|tick| SnapCandidate::new(tick, SnapKind::MarkIn)),
        );
        loose.extend(
            self.mark_out
                .map(|tick| SnapCandidate::new(tick, SnapKind::MarkOut)),
        );
        loose
    }

    /// Every magnet, loose ones included, in one list. The readable form: the
    /// drag path reads the index instead.
    pub fn snap_candidates(&self) -> Vec<crate::snap::SnapCandidate> {
        let mut all = self.loose_magnets();
        all.extend(self.snap_index.rows().iter().copied());
        all
    }

    pub fn snap_at(
        &self,
        desired: i64,
        moving: Option<u64>,
        threshold_ticks: i64,
        suspended: bool,
    ) -> (i64, Option<crate::snap::SnapCandidate>) {
        if !self.snap_enabled || suspended {
            return (desired, None);
        }
        self.snap_index
            .nearest(desired, threshold_ticks, moving, &self.loose_magnets())
    }

    pub fn selected_clip(&self) -> Option<ClipId> {
        self.selection.first().copied()
    }

    pub fn split_at_playhead(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SplitClip {
            clip,
            at: self.playhead,
        }))?;
        Ok(())
    }

    /// Shift+Delete: remove the selected clips and close each gap, latest
    /// first so an earlier removal never moves a later clip out from under its
    /// own delete. A selected gap is closed.
    pub fn ripple_delete_selected(&mut self) -> Result<(), EngineError> {
        if let Some(gap) = self.selected_gap.take() {
            return self.close_gap(gap);
        }
        let mut clips: Vec<(i64, ClipId)> = self
            .selection
            .iter()
            .filter_map(|id| {
                self.ui_clip_ref(*id)
                    .map(|clip| (clip.timeline_start_ticks, *id))
            })
            .collect();
        if clips.is_empty() {
            return Ok(());
        }
        clips.sort_unstable_by_key(|(start, _)| std::cmp::Reverse(*start));
        self.one_step(|this| {
            for (_, clip) in clips {
                this.apply_command(Command::Edit(EditCommand::RippleDelete { clip }))?;
            }
            Ok(())
        })?;
        self.clear_selection();
        Ok(())
    }
}
