//! CapCut-style templates.
//!
//! A [`Template`] is a *finished* [`Project`] — every transition, effect,
//! animation, text layer, music track, and beat-synced cut baked in and
//! locked — in which the author has marked specific clips as user-replaceable
//! ([`Replaceable`](crate::Replaceable)). This mirrors CapCut's "set
//! replaceable material clips" + "use template" flow:
//!
//! - A replaceable clip keeps its **sample media**, so the template previews
//!   exactly like the author's video.
//! - [`apply`](Template::apply) fills the slots **in order** (the order in
//!   which the editor or an agent picks media) by swapping each slot's media while its locked
//!   timeline duration, transform, effects, transitions, and animations are
//!   preserved.
//! - The customization surface ([`slots`](Template::slots),
//!   [`editable_texts`](Template::editable_texts), [`music`](Template::music))
//!   is *derived* by scanning the markers — there is no second manifest to keep
//!   in sync with the timeline.
//!
//! The result of `apply` is an ordinary [`Project`] that the existing
//! compositor and exporter render unchanged.

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::clip::{Clip, Generator, SlotMedia};
use crate::error::{ModelError, ProjectLoadError};
use crate::ids::{ProjectId, TemplateId};
use crate::media::MediaSource;
use crate::project::Project;
use crate::schema::{PROJECT_SCHEMA_VERSION, ProjectSchema};
use crate::time::{Rational, RationalTime, TimeRange, resample};

/// Stable format family for a Aria template document.
pub const TEMPLATE_SCHEMA_KIND: &str = "aria.template";

/// Recommended extension for Aria template files.
pub const TEMPLATE_FILE_EXTENSION: &str = "ariat";

/// Coarse browse category for a template, mirroring the CapCut gallery tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCategory {
    #[default]
    Trending,
    Travel,
    Lifestyle,
    Birthday,
    Wedding,
    Slideshow,
    Vlog,
    Gaming,
    Business,
    Education,
    Other,
}

/// User-facing metadata for a [`Template`] (everything the gallery shows that
/// is not the timeline itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TemplateMeta {
    /// Display name.
    pub name: String,
    /// Optional creator label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Browse category.
    #[serde(default)]
    pub category: TemplateCategory,
    /// Free-form search tags.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Timeline tick used as the cover/preview frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<RationalTime>,
    /// Optional longer description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl TemplateMeta {
    /// Metadata with just a name; everything else defaulted.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            author: None,
            category: TemplateCategory::default(),
            tags: Vec::new(),
            cover: None,
            description: None,
        }
    }

    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn with_category(mut self, category: TemplateCategory) -> Self {
        self.category = category;
        self
    }

    pub fn with_tags(mut self, tags: impl IntoIterator<Item = String>) -> Self {
        self.tags = tags.into_iter().collect();
        self
    }

    pub fn with_cover(mut self, cover: RationalTime) -> Self {
        self.cover = Some(cover);
        self
    }
}

/// One media choice supplied to [`Template::apply`], in slot order.
///
/// `source_in` is the in-point into the chosen media (at its native rate);
/// `None` starts from the beginning, matching CapCut ("takes from the start of
/// the clip"). The slot's locked timeline duration determines how much source
/// is drawn.
#[derive(Debug, Clone)]
pub struct Pick {
    pub media: MediaSource,
    pub source_in: Option<RationalTime>,
}

impl Pick {
    /// Fill a slot from the start of `media`.
    pub fn new(media: MediaSource) -> Self {
        Self {
            media,
            source_in: None,
        }
    }

    /// Fill a slot from a specific in-point in `media`.
    pub fn at(media: MediaSource, source_in: RationalTime) -> Self {
        Self {
            media,
            source_in: Some(source_in),
        }
    }
}

/// A CapCut-style template: a finished [`Project`] plus the markers that say
/// which clips a user may fill or edit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    /// Template document schema (kind [`TEMPLATE_SCHEMA_KIND`]).
    pub schema: ProjectSchema,
    pub id: TemplateId,
    pub meta: TemplateMeta,
    /// The finished timeline, with sample media in its pool and replaceable
    /// clips marked.
    pub project: Project,
}

impl Template {
    /// Author a template from a finished [`Project`]. Mark the slots first with
    /// [`Project::set_replaceable`] / [`Project::set_text_editable`].
    pub fn from_project(project: Project, meta: TemplateMeta) -> Self {
        Self {
            schema: Self::current_schema(),
            id: TemplateId::next(),
            meta,
            project,
        }
    }

    /// The finished, sample-filled project (what a preview renders).
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn meta(&self) -> &TemplateMeta {
        &self.meta
    }

    /// Total timeline length.
    pub fn duration(&self) -> RationalTime {
        self.project.timeline().duration()
    }

    /// The visual fill slots in fill order (replaceable clips that take video
    /// or image media). Excludes the music slot, which is swapped separately.
    pub fn slots(&self) -> Vec<&Clip> {
        let mut slots: Vec<&Clip> = self
            .replaceable_clips()
            .filter(|clip| {
                clip.replaceable
                    .as_ref()
                    .is_some_and(|r| r.accepts != SlotMedia::AudioOnly)
            })
            .collect();
        slots.sort_by_key(|clip| {
            let order = clip.replaceable.as_ref().map_or(u32::MAX, |r| r.order);
            (order, clip.id.raw())
        });
        slots
    }

    /// How many visual slots a user must fill.
    pub fn slot_count(&self) -> usize {
        // One definition of "slot": whatever `slots()` lists (cold path, so
        // the sort it performs costs nothing that matters).
        self.slots().len()
    }

    /// The swappable music/soundtrack clip, if the author marked one
    /// (`accepts == AudioOnly`). Swap it post-apply with
    /// [`Project::set_clip_media`].
    pub fn music(&self) -> Option<&Clip> {
        self.replaceable_clips()
            .filter(|clip| {
                clip.replaceable
                    .as_ref()
                    .is_some_and(|r| r.accepts == SlotMedia::AudioOnly)
            })
            .min_by_key(|clip| clip.replaceable.as_ref().map_or(u32::MAX, |r| r.order))
    }

    /// Text clips a user may re-word (the text keeps its style and animation),
    /// ordered by timeline position.
    pub fn editable_texts(&self) -> Vec<&Clip> {
        let mut texts: Vec<&Clip> = self
            .project
            .timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips())
            .filter(|clip| {
                clip.text_editable
                    && matches!(
                        clip.content,
                        crate::ClipSource::Generated(Generator::Text { .. })
                    )
            })
            .collect();
        texts.sort_by_key(|clip| (clip.timeline.start.value, clip.id.raw()));
        texts
    }

    /// Fill the visual slots in order, returning a normal, render-ready
    /// [`Project`]. Each pick (in slot order) swaps its slot's media while the
    /// slot's locked duration, transform, effects, transitions, and animations
    /// are preserved.
    ///
    /// Supplying fewer picks than slots is allowed: the remaining slots keep
    /// their sample media, exactly like a CapCut template preview. Supplying
    /// *more* picks than slots is refused ([`ModelError::TooManyPicks`]) —
    /// silently dropping extras would hide an off-by-one in the caller (the
    /// AI agent fills templates too). Each pick is validated against the
    /// slot's media-type restriction and — for non-image media — that enough
    /// source exists to cover the slot's locked duration from the chosen
    /// in-point.
    ///
    /// The output keeps its `replaceable` / `text_editable` markers, matching
    /// CapCut: a filled template stays re-editable (re-pick a slot, re-word a
    /// title), and the markers are render-inert metadata. Saved results are
    /// therefore ordinary projects that still carry their template chrome.
    pub fn apply(&self, picks: &[Pick]) -> Result<Project, ModelError> {
        let slots = self.slots();
        if picks.len() > slots.len() {
            return Err(ModelError::TooManyPicks {
                given: picks.len(),
                slots: slots.len(),
            });
        }

        let mut out = self.project.clone();
        out.id = ProjectId::next();
        let tl_rate = out.timeline().frame_rate;

        // Resolve every slot up front (immutable borrow of the template) before
        // mutating the cloned output, so the slot ids and geometry are stable.
        for (slot, pick) in slots.into_iter().zip(picks.iter()) {
            let accepts = slot
                .replaceable
                .as_ref()
                .map_or(SlotMedia::Any, |r| r.accepts);
            let kind = pick.media.kind();
            if !accepts.accepts(kind) {
                return Err(ModelError::SlotMediaMismatch {
                    slot: slot.id,
                    accepts,
                    found: kind,
                });
            }
            let source = slot_source(slot, tl_rate, pick.source_in, &pick.media)?;
            let media_id = out.add_media(pick.media.clone());
            out.set_clip_media(slot.id, media_id, source)?;
        }
        Ok(out)
    }

    fn replaceable_clips(&self) -> impl Iterator<Item = &Clip> {
        self.project
            .timeline()
            .tracks_ordered()
            .flat_map(|track| track.clips())
            .filter(|clip| clip.replaceable.is_some())
    }

    fn current_schema() -> ProjectSchema {
        ProjectSchema {
            version: PROJECT_SCHEMA_VERSION,
            kind: TEMPLATE_SCHEMA_KIND.into(),
            extensions: Vec::new(),
        }
    }

    /// Serialize this template to a `.ariat` JSON file.
    ///
    /// The file is written the way a project is: through a synced temporary
    /// beside `path` that replaces it atomically, so a failed save leaves the
    /// old template whole and a hard link to it keeps the old bytes. An
    /// existing file's permissions carry over. A symbolic-link destination,
    /// or a path the template's media pool plays from, fails with
    /// [`io::ErrorKind::InvalidInput`] before anything is written.
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        crate::persist::reject_pool_media(&self.project, path, "template")?;
        let mut doc = self.clone();
        doc.schema = Self::current_schema();
        crate::persist::save_document(path, &doc)
    }

    /// Deserialize a template from a `.ariat` JSON file.
    ///
    /// Mirrors the project loader (see `persist.rs`) and fails with
    /// [`ModelError::TemplateLoad`] in the same classes: a missing or
    /// unreadable file, a file that is not a template, a newer format, and
    /// a damaged field. The schema is read and validated *before* the strict
    /// typed parse, so a future-version file is never half-parsed; the
    /// embedded project document is migrated up to the current shape first
    /// (`.ariat` versions in lockstep with the project schema), and the
    /// embedded project passes the layout and reference checks a project
    /// file passes.
    pub fn load_from_file(path: &Path) -> Result<Template, ModelError> {
        Self::load_document(path).map_err(|err| match err {
            ModelError::ProjectLoad(err) => ModelError::TemplateLoad(err),
            other => other,
        })
    }

    fn load_document(path: &Path) -> Result<Template, ModelError> {
        let at = Some(path);
        let bytes = std::fs::read(path).map_err(|err| ProjectLoadError::from_io(path, &err))?;
        let not_a_template = |reason| ProjectLoadError::not_a_project(at, reason);
        let text = std::str::from_utf8(&bytes).map_err(|_| not_a_template("not JSON"))?;
        let mut doc: serde_json::Value =
            serde_json::from_str(text).map_err(|_| not_a_template("not JSON"))?;
        if !doc.is_object() {
            return Err(not_a_template("not a JSON object").into());
        }

        let schema = crate::persist::read_schema(&doc, at)?;
        if schema.kind != TEMPLATE_SCHEMA_KIND
            || schema.version < 1
            || schema.version > PROJECT_SCHEMA_VERSION
        {
            return Err(ProjectLoadError::from_schema(at, &schema, &Self::current_schema()).into());
        }
        // The saver stamps the whole document (outer schema) with the version
        // that describes the embedded project's shape, so migrate that
        // document from the outer version before the strict parse.
        if let Some(project) = doc.get_mut("project") {
            crate::persist::migrate_document(project, schema.version);
        }

        let mut template = match Template::deserialize(&doc) {
            Ok(template) => template,
            Err(_) => {
                // Reparse the pretty text so the refusal states the field and
                // its JSON path, as the project loader does.
                let pretty = serde_json::to_string_pretty(&doc).unwrap_or_else(|_| text.to_owned());
                let err = serde_json::from_str::<Template>(&pretty)
                    .expect_err("document rejected as a value must fail as text");
                return Err(ProjectLoadError::from_serde(at, &pretty, &err).into());
            }
        };
        // Keep the file's original schema as provenance; the writer re-stamps
        // the current version on save (mirroring the project loader).
        template.schema = schema;
        // Mirror the project loader: pre-main-track files re-derive the
        // lane-zone / main-track invariants on entry.
        template.project.normalize_all_lanes();
        crate::persist::check_layout(&template.project, at, "project.")?;
        Ok(template)
    }
}

/// The source window a fill draws for `slot`: the slot's *locked* timeline
/// duration, scaled by the slot's base speed and converted into the chosen
/// media's rate, starting at `source_in` (default 0). For non-image media the
/// window must lie within the source; images repeat one frame for any length.
///
/// The duration conversion **rounds up** (one exact rational ceiling, not
/// truncate-then-round-nearest): a fill must never under-cover its slot, or
/// the last timeline frame would read past the source window. Slots that are
/// speed-ramped or reversed are refused — sizing their window needs the
/// curve integral, and v1 rejects the pick rather than mis-windowing it.
fn slot_source(
    slot: &Clip,
    tl_rate: Rational,
    source_in: Option<RationalTime>,
    media: &MediaSource,
) -> Result<TimeRange, ModelError> {
    if slot.reversed || slot.has_speed_curve() {
        return Err(ModelError::SlotRetimeUnsupported { slot: slot.id });
    }

    // source_ticks = tl_dur ticks × (tl.den/tl.num) s/tick × speed × media
    // rate — assembled as a single i128 fraction and ceiled.
    let tl_dur = i128::from(slot.timeline.duration.value);
    let speed = slot.speed;
    let numer =
        tl_dur * i128::from(tl_rate.den) * i128::from(speed.num) * i128::from(media.frame_rate.num);
    let denom = i128::from(tl_rate.num) * i128::from(speed.den) * i128::from(media.frame_rate.den);
    if denom == 0 {
        return Err(ModelError::InvalidParam(
            "zero rate or speed denominator".into(),
        ));
    }
    // Positive by construction: durations, speeds, and rates are validated
    // positive at their entry points.
    let src_dur =
        (numer.div_euclid(denom) + i128::from(numer.rem_euclid(denom) != 0)).max(1) as i64;

    let start = match source_in {
        Some(rt) => resample(rt, media.frame_rate).value,
        None => 0,
    };
    let end = start
        .checked_add(src_dur)
        .ok_or(ModelError::SlotDurationUnmet { slot: slot.id })?;
    if start < 0 || (!media.is_image && end > media.duration.value) {
        return Err(ModelError::SlotDurationUnmet { slot: slot.id });
    }
    Ok(TimeRange::at_rate(start, src_dur, media.frame_rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Replaceable;
    use crate::ids::ClipId;
    use crate::time::Rational;
    use crate::track::TrackKind;
    use crate::{Generator, MediaSource, Project};

    const R24: Rational = Rational::FPS_24;

    /// Build a tiny 2-slot template: two 1s video slots on a video track plus a
    /// locked title on a text track.
    fn sample_template() -> (Template, ClipId, ClipId) {
        let mut project = Project::new("Trendy Intro", R24);
        let sample = project.add_media(MediaSource::new("sample.mp4", 1920, 1080, R24, 240, true));
        let video = project.add_track(TrackKind::Video, "V1");
        let text = project.add_track(TrackKind::Text, "T1");

        let slot_a = project
            .add_clip(
                video,
                sample,
                TimeRange::at_rate(0, 24, R24),
                RationalTime::new(0, R24),
            )
            .unwrap();
        let slot_b = project
            .add_clip(
                video,
                sample,
                TimeRange::at_rate(0, 24, R24),
                RationalTime::new(24, R24),
            )
            .unwrap();
        let title = project
            .add_generated(
                text,
                Generator::text("Your Name"),
                TimeRange::at_rate(0, 48, R24),
            )
            .unwrap();

        project
            .set_replaceable(slot_b, Some(Replaceable::new(1)))
            .unwrap();
        project
            .set_replaceable(slot_a, Some(Replaceable::new(0)))
            .unwrap();
        project.set_text_editable(title, true).unwrap();

        (
            Template::from_project(project, TemplateMeta::new("Trendy Intro")),
            slot_a,
            slot_b,
        )
    }

    #[test]
    fn slots_are_listed_in_fill_order() {
        let (template, slot_a, slot_b) = sample_template();
        let slots: Vec<ClipId> = template.slots().iter().map(|c| c.id).collect();
        assert_eq!(template.slot_count(), 2);
        assert_eq!(slots, vec![slot_a, slot_b]);
        assert_eq!(template.editable_texts().len(), 1);
    }

    #[test]
    fn apply_fills_slots_at_locked_durations() {
        let (template, slot_a, slot_b) = sample_template();
        let picks = vec![
            Pick::new(MediaSource::new("a.mp4", 1920, 1080, R24, 120, true)),
            Pick::new(MediaSource::new("b.mp4", 1280, 720, R24, 90, true)),
        ];
        let out = template.apply(&picks).unwrap();

        // The slots now point at the picked media, windowed to the locked 1s.
        for (slot, name) in [(slot_a, "a.mp4"), (slot_b, "b.mp4")] {
            let clip = out.clip(slot).unwrap();
            match &clip.content {
                crate::ClipSource::Media { media, source } => {
                    assert_eq!(source.duration.value, 24, "locked 1s slot");
                    assert_eq!(out.media(*media).unwrap().path().to_str().unwrap(), name);
                }
                other => panic!("slot not filled: {other:?}"),
            }
        }
    }

    #[test]
    fn fewer_picks_keep_sample_media() {
        let (template, slot_a, slot_b) = sample_template();
        let sample_media = match &template.project().clip(slot_a).unwrap().content {
            crate::ClipSource::Media { media, .. } => *media,
            _ => unreachable!(),
        };
        let out = template
            .apply(&[Pick::new(MediaSource::new(
                "a.mp4", 1920, 1080, R24, 120, true,
            ))])
            .unwrap();
        // slot_b was not picked: it keeps the author's sample media.
        match &out.clip(slot_b).unwrap().content {
            crate::ClipSource::Media { media, .. } => assert_eq!(*media, sample_media),
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn apply_rejects_wrong_media_kind() {
        let mut project = Project::new("Photo Slot", R24);
        let sample = project.add_media(MediaSource::image("sample.png", 800, 600));
        let track = project.add_track(TrackKind::Video, "V1");
        let slot = project
            .add_clip(
                track,
                sample,
                TimeRange::at_rate(0, STILL_TICKS, crate::media::STILL_TICK_RATE),
                RationalTime::new(0, R24),
            )
            .unwrap();
        project
            .set_replaceable(
                slot,
                Some(Replaceable::new(0).with_accepts(SlotMedia::ImageOnly)),
            )
            .unwrap();
        let template = Template::from_project(project, TemplateMeta::new("Photo Slot"));

        let video = Pick::new(MediaSource::new("clip.mp4", 1920, 1080, R24, 120, true));
        assert!(matches!(
            template.apply(&[video]),
            Err(ModelError::SlotMediaMismatch { .. })
        ));
    }

    #[test]
    fn apply_rejects_media_too_short_for_slot() {
        let (template, _, _) = sample_template();
        // A 1s slot needs 24 frames; this clip only has 10.
        let tiny = Pick::new(MediaSource::new("tiny.mp4", 1920, 1080, R24, 10, true));
        assert!(matches!(
            template.apply(&[tiny]),
            Err(ModelError::SlotDurationUnmet { .. })
        ));
    }

    #[test]
    fn template_file_roundtrips() {
        let (template, _, _) = sample_template();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("intro.ariat");
        template.save_to_file(&path).unwrap();
        let loaded = Template::load_from_file(&path).unwrap();
        assert_eq!(loaded.schema.kind, TEMPLATE_SCHEMA_KIND);
        assert_eq!(loaded.slot_count(), template.slot_count());
        assert_eq!(loaded.meta.name, "Trendy Intro");
    }

    const STILL_TICKS: i64 = crate::media::STILL_DEFAULT_DURATION_TICKS;

    /// The class closed here is "a template that fails to load is refused
    /// as a project": a missing, unreadable, foreign, newer or damaged
    /// `.ariat` file is stated as a template with its path and the `.ariat`
    /// fix, in the classes a project file is, and a template whose project
    /// plays media its pool does not hold is refused the way a project file
    /// is rather than applied. Every [`ProjectLoadErrorKind`] has a case; a
    /// new kind does not compile here until it has one.
    #[test]
    fn every_template_load_refusal_states_the_template_and_its_fix() {
        use crate::error::ProjectLoadErrorKind;
        use std::path::PathBuf;

        #[cfg(unix)]
        fn locked(path: PathBuf) -> Vec<(PathBuf, String)> {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
            let msg = format!(
                "Couldn't read template at {} (permission denied). Check the file permissions and try again.",
                path.display()
            );
            vec![(path, msg)]
        }
        #[cfg(not(unix))]
        fn locked(_: PathBuf) -> Vec<(PathBuf, String)> {
            Vec::new()
        }

        let dir = tempfile::tempdir().unwrap();
        let refusal = |path: &Path| match Template::load_from_file(path) {
            Err(ModelError::TemplateLoad(err)) => {
                (err.kind(), ModelError::TemplateLoad(err).to_string())
            }
            other => panic!("{} loaded as {other:?}", path.display()),
        };
        let saved = |name: &str, edit: &dyn Fn(&mut serde_json::Value)| {
            let path = dir.path().join(name);
            sample_template().0.save_to_file(&path).unwrap();
            let mut doc: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            edit(&mut doc);
            std::fs::write(&path, doc.to_string()).unwrap();
            path
        };
        let v = PROJECT_SCHEMA_VERSION;
        for kind in ProjectLoadErrorKind::ALL {
            let cases: Vec<(PathBuf, String)> = match kind {
                ProjectLoadErrorKind::NotFound => {
                    let path = dir.path().join("missing.ariat");
                    let msg = format!(
                        "No template at {}. Check the path or pick another .ariat file.",
                        path.display()
                    );
                    vec![(path, msg)]
                }
                ProjectLoadErrorKind::Unreadable => {
                    locked(saved("locked.ariat", &|_: &mut serde_json::Value| {}))
                }
                ProjectLoadErrorKind::NotAProject => {
                    let project = dir.path().join("plain.ariat");
                    Project::new("plain", R24).save_to_file(&project).unwrap();
                    let text = dir.path().join("notes.ariat");
                    std::fs::write(&text, "not json at all").unwrap();
                    let folder = dir.path().join("folder.ariat");
                    std::fs::create_dir(&folder).unwrap();
                    let not = |path: &Path, reason: &str| {
                        format!(
                            "Couldn't open {}: it is not an Aria template file ({reason}). Pick a valid .ariat template file.",
                            path.display()
                        )
                    };
                    vec![
                        (
                            project.clone(),
                            not(
                                &project,
                                &format!(
                                    "its format is 'aria.project' version {v}, and this build reads 'aria.template' versions 1 to {v}"
                                ),
                            ),
                        ),
                        (text.clone(), not(&text, "not JSON")),
                        (folder.clone(), not(&folder, "a directory")),
                    ]
                }
                ProjectLoadErrorKind::NewerSchema => {
                    // The body is garbage a strict parse would reject; a
                    // future-version file is refused by the schema check,
                    // never half-parsed.
                    let path = dir.path().join("future.ariat");
                    std::fs::write(
                        &path,
                        r#"{"schema":{"version":99,"kind":"aria.template"},"project":5}"#,
                    )
                    .unwrap();
                    let msg = format!(
                        "Couldn't open {}: it uses template format version 99, which is newer than this build reads ('aria.template' versions 1 to {v}). Update Aria to open it.",
                        path.display()
                    );
                    vec![(path, msg)]
                }
                ProjectLoadErrorKind::Damaged => {
                    let rate = saved("rate.ariat", &|doc: &mut serde_json::Value| {
                        doc["project"]["timeline"]["frame_rate"]["num"] = "bad".into();
                    });
                    let pool = saved("pool.ariat", &|doc: &mut serde_json::Value| {
                        doc["project"]["media"] = serde_json::json!([]);
                    });
                    let damaged = |path: &Path, at: &str| {
                        format!(
                            "Couldn't open {}: the template file is damaged or corrupted at {at}",
                            path.display()
                        )
                    };
                    vec![
                        (
                            rate.clone(),
                            damaged(&rate, "field 'num' in 'project.timeline.frame_rate.num'"),
                        ),
                        (
                            pool.clone(),
                            damaged(&pool, "field 'tracks' in 'project.timeline.tracks'"),
                        ),
                    ]
                }
            };
            for (path, expected) in cases {
                let (found, msg) = refusal(&path);
                assert_eq!(found, kind, "{msg}");
                match kind {
                    ProjectLoadErrorKind::Damaged => {
                        assert!(
                            msg.starts_with(&expected)
                                && msg.ends_with("Pick another .ariat file."),
                            "{msg}"
                        );
                        assert!(!msg.contains("at line "), "{msg}");
                    }
                    _ => assert_eq!(msg, expected),
                }
            }
        }
    }

    #[test]
    fn load_migrates_older_template_versions() {
        let (template, _, _) = sample_template();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("v1.ariat");
        template.save_to_file(&path).unwrap();
        // Rewind the document to v1: the loader must walk the project
        // migration chain over the embedded document (v1 -> v2 is
        // shape-identical) and keep the file's version as provenance.
        let mut doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        doc["schema"]["version"] = serde_json::json!(1);
        std::fs::write(&path, serde_json::to_string(&doc).unwrap()).unwrap();

        let loaded = Template::load_from_file(&path).unwrap();
        assert_eq!(loaded.schema.version, 1);
        assert_eq!(loaded.slot_count(), template.slot_count());
    }

    /// A template save replaces the file rather than rewriting it in place:
    /// another name for the old file keeps the old bytes, and a save aimed at
    /// pool media or through a symbolic link leaves the target untouched.
    #[test]
    fn template_save_replaces_the_file_and_leaves_a_hard_link_on_the_old_bytes() {
        let (mut template, _, _) = sample_template();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("intro.ariat");
        template.save_to_file(&path).unwrap();
        let old = std::fs::read(&path).unwrap();
        let witness = dir.path().join("witness.ariat");
        std::fs::hard_link(&path, &witness).unwrap();

        template.meta.name = "Renamed Intro".into();
        template.save_to_file(&path).unwrap();

        assert_eq!(std::fs::read(&witness).unwrap(), old);
        let loaded = Template::load_from_file(&path).unwrap();
        assert_eq!(loaded.meta.name, "Renamed Intro");
    }

    #[test]
    fn template_save_over_pool_media_fails_and_keeps_the_media() {
        let (mut template, _, _) = sample_template();
        let dir = tempfile::tempdir().unwrap();
        let footage = dir.path().join("footage.mp4");
        std::fs::write(&footage, b"footage bytes").unwrap();
        let footage = footage.canonicalize().unwrap();
        template.project.add_media(MediaSource::new(
            footage.clone(),
            1920,
            1080,
            R24,
            240,
            true,
        ));

        let error = template.save_to_file(&footage).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            error.to_string().contains("media this template uses"),
            "{error}"
        );
        assert_eq!(std::fs::read(&footage).unwrap(), b"footage bytes");
        // Another spelling of the same file is the same file.
        let dotted = dir.path().join(".").join("footage.mp4");
        assert_eq!(
            template.save_to_file(&dotted).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(std::fs::read(&footage).unwrap(), b"footage bytes");
    }

    #[cfg(unix)]
    #[test]
    fn template_save_rejects_a_symbolic_link_destination() {
        let (template, _, _) = sample_template();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.ariat");
        std::fs::write(&target, b"keep").unwrap();
        let link = dir.path().join("link.ariat");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = template.save_to_file(&link).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read(&target).unwrap(), b"keep");
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[cfg(unix)]
    #[test]
    fn template_save_gives_a_new_file_the_umask_default_and_keeps_an_existing_mode() {
        use std::os::unix::fs::PermissionsExt;
        let (template, _, _) = sample_template();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shared.ariat");
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let reference = dir.path().join("reference");
        std::fs::File::create(&reference).unwrap();
        template.save_to_file(&path).unwrap();
        assert_eq!(mode(&path), mode(&reference));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        template.save_to_file(&path).unwrap();

        assert_eq!(mode(&path), 0o640);
    }

    #[test]
    fn slot_source_overflow_and_bounds() {
        let slot = Clip::from_media(
            crate::ids::MediaId::from_raw(1),
            TimeRange::at_rate(0, 24, R24),
            TimeRange::at_rate(0, 24, R24),
        );
        let media = MediaSource::new("test.mp4", 1920, 1080, R24, 100, true);
        // Out of bounds start
        let res = slot_source(&slot, R24, Some(RationalTime::new(90, R24)), &media);
        assert!(matches!(res, Err(ModelError::SlotDurationUnmet { .. })));

        // Overflow start
        let res_overflow = slot_source(
            &slot,
            R24,
            Some(RationalTime::new(i64::MAX - 10, R24)),
            &media,
        );
        assert!(matches!(
            res_overflow,
            Err(ModelError::SlotDurationUnmet { .. })
        ));
    }
}
