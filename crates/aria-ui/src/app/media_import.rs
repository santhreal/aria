//! `AppState` media: the import verbs, and the formats an import accepts.
//!
//! The format list is here rather than on a panel because several file dialogs
//! offer the same media — a missing item's Relink, in the pool and in the
//! inspector, and a template asking for a slot's media — and the drop claim has
//! to accept exactly what those dialogs offer. A list owned by whichever panel
//! happened to draw the first dialog is a list the others copy.

use super::*;
use crate::app::picker::{Ask, FileSpec};

/// The extensions the platform decoders read, offered by every import dialog.
pub(crate) const MEDIA_IMPORT_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "wav", "mp3", "png", "jpg", "jpeg", "webp",
];

/// The extensions an import dialog offers and a drop claim accepts:
/// [`MEDIA_IMPORT_EXTENSIONS`], then every extension a decoder plugin claims.
pub(crate) fn media_import_extensions() -> Vec<String> {
    let mut all: Vec<String> = MEDIA_IMPORT_EXTENSIONS
        .iter()
        .map(|ext| (*ext).to_owned())
        .collect();
    for ext in aria_decoder::external_decoder_extensions() {
        if !all.contains(&ext) {
            all.push(ext);
        }
    }
    all
}

impl AppState {
    pub fn import_media(&mut self, path: PathBuf) -> Result<MediaId, EngineError> {
        match self.apply_command(Command::Project(ProjectCommand::Import { path }))? {
            ApplyOutcome::Imported { media } => {
                self.status_message = Some("Imported".into());
                Ok(media)
            }
            other => Err(EngineError::Import(format!(
                "unexpected import outcome: {other:?}"
            ))),
        }
    }

    pub fn relink_media(&mut self, media: MediaId, path: PathBuf) -> Result<(), EngineError> {
        crate::projection::invalidate_media_statuses();
        self.apply_command(Command::Project(ProjectCommand::RelinkMedia {
            media,
            path,
        }))?;
        self.status_message = Some("Relinked".into());
        Ok(())
    }

    /// Ask for the file a missing item now lives at, then relink it: the Relink
    /// button a missing item draws in the pool and in the inspector.
    pub fn pick_and_relink(&mut self, media: MediaId) {
        let spec = FileSpec::new("Relink Media").filter("Media", &media_import_extensions());
        self.request_pick(Ask::OpenFile(spec), move |state, answer| {
            let Some(path) = answer.one() else {
                return;
            };
            if let Err(err) = state.relink_media(media, path) {
                state.report_error(&err);
            }
        });
    }

    pub fn import_dropped_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), EngineError> {
        let mut video = Vec::new();
        let mut audio = Vec::new();
        for path in paths {
            self.load_plugins_for_file(&path);
            let media = self.import_media(path)?;
            let Some(item) = self.store.media.iter().find(|item| item.id == media.raw()) else {
                continue;
            };
            if item.kind == crate::types::UiClipKind::Audio {
                audio.push(media);
            } else {
                video.push(media);
            }
        }
        // Every placement and every lane it needs is one undo step, named
        // after the first edit: one drop, one `Source edit`.
        self.one_step_named(
            |label: &Option<&'static str>| *label,
            |state| {
                let pictures = state.place_chain(TrackKind::Video, &video)?;
                Ok(pictures.or(state.place_chain(TrackKind::Audio, &audio)?))
            },
        )?;
        Ok(())
    }

    pub fn add_generated(&mut self, generator: Generator) -> Result<(), EngineError> {
        self.drop_generated_on_timeline(generator, None, self.playhead.value.max(0))
    }

    pub fn drop_generated_on_timeline(
        &mut self,
        generator: Generator,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<(), EngineError> {
        self.place_generated(generator, track, at).map(|_| ())
    }

    /// Place the plugin generator `effect_id` at the playhead.
    pub fn add_effect_generator(&mut self, effect_id: &str) -> Result<(), EngineError> {
        self.drop_effect_generator_on_timeline(effect_id, None, self.playhead.value.max(0))
    }

    /// Place the plugin generator `effect_id` on `track` at `at`: a transparent
    /// solid named after the effect, with the effect first in its chain, as one
    /// undo step. Refused when no registered effect with that id is a
    /// generator.
    pub fn drop_effect_generator_on_timeline(
        &mut self,
        effect_id: &str,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<(), EngineError> {
        let spec = aria_models::effect_spec(effect_id)
            .filter(|spec| spec.generator)
            .ok_or_else(|| {
                aria_models::ModelError::InvalidParam(format!("no generator `{effect_id}`"))
            })?;
        self.place_effect_generator(&spec.id, &spec.label, None, track, at)
    }

    /// Place the plugin clip type `type_id` at the playhead.
    pub fn add_clip_type(&mut self, type_id: &str) -> Result<(), EngineError> {
        self.drop_clip_type_on_timeline(type_id, None, self.playhead.value.max(0))
    }

    /// Place the plugin clip type `type_id` on `track` at `at`: its generator,
    /// placed as [`Self::drop_effect_generator_on_timeline`] places one, named
    /// after the type and holding the type's id, as one undo step. Refused when
    /// no plugin lists the type or its generator cannot draw it.
    pub fn drop_clip_type_on_timeline(
        &mut self,
        type_id: &str,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<(), EngineError> {
        let spec = aria_models::clip_type_spec(type_id).ok_or_else(|| {
            aria_models::ModelError::InvalidParam(format!("no clip type `{type_id}`"))
        })?;
        if let Some(reason) = spec.unavailable() {
            return Err(aria_models::ModelError::InvalidParam(format!(
                "{} cannot be placed: {reason}",
                spec.label
            ))
            .into());
        }
        self.place_effect_generator(&spec.generator, &spec.label, Some(&spec.id), track, at)
    }

    /// A transparent solid on `track` at `at`, with `effect_id` first in its
    /// chain, named `name`, and holding `clip_type` when one is given, as one
    /// undo step.
    fn place_effect_generator(
        &mut self,
        effect_id: &str,
        name: &str,
        clip_type: Option<&str>,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<(), EngineError> {
        self.one_step(|state| {
            let clip = state.place_generated(Generator::SolidColor { rgba: [0; 4] }, track, at)?;
            state.apply_command(Command::Edit(EditCommand::AddEffect {
                clip,
                effect_id: effect_id.to_owned(),
            }))?;
            state.apply_command(Command::Edit(EditCommand::SetClipName {
                clip,
                name: Some(name.to_owned()),
            }))?;
            if let Some(clip_type) = clip_type {
                state.apply_command(Command::Edit(EditCommand::SetClipType {
                    clip,
                    clip_type: Some(clip_type.to_owned()),
                }))?;
            }
            Ok(())
        })
    }

    /// Detach the selected clip from its clip type: the clip keeps its chain
    /// and its name, and stops taking the type's colour, icon, and handles.
    /// Inert with no selection or on a clip that holds no type.
    pub fn detach_selected_clip_type(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let typed = self
            .engine
            .lock()
            .project()
            .clip(clip)
            .is_some_and(|clip| clip.clip_type.is_some());
        if !typed {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetClipType {
            clip,
            clip_type: None,
        }))?;
        Ok(())
    }

    /// Place `generator` on `track`, or on the first lane of its kind free at
    /// `at` (a new lane when none is), and return the new clip.
    pub(crate) fn place_generated(
        &mut self,
        generator: Generator,
        track: Option<TrackId>,
        at: i64,
    ) -> Result<aria_models::ClipId, EngineError> {
        // The generator's own row names the one lane that accepts it. A match
        // here defaulted every non-title generator to the sticker lane, so a new
        // generator landed there whatever its row said.
        let kind = generator.spec().lane;
        let ui_kind = crate::types::UiTrackKind::from_model(kind);
        let rate = self.playhead.rate;
        let duration = ((5 * i64::from(rate.num.max(1))) / i64::from(rate.den.max(1))).max(1);
        let at = at.max(0);
        let track = match track.filter(|id| {
            self.store
                .sequence
                .tracks
                .iter()
                .any(|lane| lane.id == id.raw() && lane.kind == ui_kind)
        }) {
            Some(id) => id,
            None => self.free_kind_track(kind, at, at + duration)?,
        };
        match self.apply_command(Command::Edit(EditCommand::AddGenerated {
            track,
            generator,
            timeline: TimeRange::at_rate(at, duration, rate),
        }))? {
            ApplyOutcome::Edited(aria_commands::EditOutcome::Created(clip)) => Ok(clip),
            other => Err(EngineError::Refused(format!(
                "placing a generated clip produced {other:?}, not a clip"
            ))),
        }
    }

    /// Place `items` end to end from the playhead on the first lane of
    /// `kind`. Each lands at the first tick from where the one before it ends
    /// that leaves its whole span free on every lane it places a clip on, so
    /// nothing already on the timeline moves or is covered, in either edit
    /// mode. Returns the history label of the first edit it applied.
    pub(super) fn place_chain(
        &mut self,
        kind: TrackKind,
        items: &[MediaId],
    ) -> Result<Option<&'static str>, EngineError> {
        if items.is_empty() {
            return Ok(None);
        }
        let track = self.ensure_kind_track(kind)?;
        let rate = self.playhead.rate;
        let mut cursor = self.playhead.value;
        let mut first = None;
        for &media in items {
            let Some(source) = self
                .engine
                .lock()
                .project()
                .media(media)
                .map(|item| item.full_range())
            else {
                continue;
            };
            let Some(lanes) = self.source_lanes(media, Some(track))? else {
                continue;
            };
            // The span the engine places the source at, in the sequence clock.
            let span = aria_models::resample(source.duration, rate).value.max(1);
            let mut taken: Vec<(i64, i64)> =
                lanes.lanes().flat_map(|lane| self.spans_on(lane)).collect();
            taken.sort_unstable();
            let at = crate::os_drop::plan_sequential_starts(&taken, cursor, &[span])[0];
            let label = self.place_source_on(
                crate::app::source_patch::SourcePlacement {
                    media,
                    source,
                    at: aria_models::RationalTime::new(at, rate),
                    fill: None,
                    ripple: false,
                    aimed: None,
                },
                lanes,
            )?;
            first.get_or_insert(label);
            cursor = at + span;
        }
        Ok(first)
    }
}
