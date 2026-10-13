//! `AppState` media: the import verbs, and the formats an import accepts.
//!
//! The format list is here rather than on a panel because three surfaces offer
//! the same import — the media bin's button, the inspector's relink, a template
//! asking for a slot's media — and the drop claim has to accept exactly what
//! those file dialogs offer. A list owned by whichever panel happened to draw
//! the first dialog is a list the other two copy.

use super::*;

/// The extensions an import dialog offers, and a drop claim accepts.
pub const MEDIA_IMPORT_EXTENSIONS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "wav", "mp3", "png", "jpg", "jpeg", "webp",
];

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

    pub fn import_dropped_paths(&mut self, paths: Vec<PathBuf>) -> Result<(), EngineError> {
        let mut video = Vec::new();
        let mut audio = Vec::new();
        for path in paths {
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
        self.place_chain(TrackKind::Video, &video)?;
        self.place_chain(TrackKind::Audio, &audio)?;
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
        self.one_step(|state| {
            let clip = state.place_generated(Generator::SolidColor { rgba: [0; 4] }, track, at)?;
            state.apply_command(Command::Edit(EditCommand::AddEffect {
                clip,
                effect_id: spec.id.clone(),
            }))?;
            state.apply_command(Command::Edit(EditCommand::SetClipName {
                clip,
                name: Some(spec.label.clone()),
            }))?;
            Ok(())
        })
    }

    /// Place `generator` on `track`, or on the first lane of its kind, at `at`,
    /// and return the new clip.
    fn place_generated(
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
        let track = match track.filter(|id| {
            self.store
                .sequence
                .tracks
                .iter()
                .any(|lane| lane.id == id.raw() && lane.kind == ui_kind)
        }) {
            Some(id) => id,
            None => self.ensure_kind_track(kind)?,
        };
        let rate = self.playhead.rate;
        let duration = (5 * i64::from(rate.num.max(1))) / i64::from(rate.den.max(1));
        match self.apply_command(Command::Edit(EditCommand::AddGenerated {
            track,
            generator,
            timeline: TimeRange::at_rate(at.max(0), duration.max(1), rate),
        }))? {
            ApplyOutcome::Edited(aria_commands::EditOutcome::Created(clip)) => Ok(clip),
            other => Err(EngineError::Refused(format!(
                "placing a generated clip produced {other:?}, not a clip"
            ))),
        }
    }

    pub(super) fn place_chain(
        &mut self,
        kind: TrackKind,
        items: &[MediaId],
    ) -> Result<(), EngineError> {
        if items.is_empty() {
            return Ok(());
        }
        let track = self.ensure_kind_track(kind)?;
        for media in items {
            let spans = self.spans_on(track);
            let start =
                crate::os_drop::plan_sequential_starts(&spans, self.playhead.value, &[1])[0];
            self.drop_media_on_timeline(*media, Some(track), start)?;
        }
        Ok(())
    }
}
