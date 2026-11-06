use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::clip::SlotMedia;
use crate::ids::{BinId, ClipId, MarkerId, MediaId, SequenceId, TrackId};
use crate::media::MediaKind;
use crate::schema::ProjectSchema;
use crate::time::{Rational, TimeError};
use crate::track::TrackKind;

/// Category of failure when loading a `.aria` project file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProjectLoadErrorKind {
    /// Path does not exist on disk.
    NotFound,
    /// File exists on disk but cannot be read (such as permission denied).
    Unreadable,
    /// File is not JSON, is not a JSON object, or has a non-project schema kind.
    NotAProject,
    /// File has a project schema version newer than this build supports.
    NewerSchema,
    /// File has a supported project schema but a field inside it is missing or invalid.
    Damaged,
}

impl ProjectLoadErrorKind {
    /// Every project-load failure kind in canonical order.
    pub const ALL: [Self; 5] = [
        Self::NotFound,
        Self::Unreadable,
        Self::NotAProject,
        Self::NewerSchema,
        Self::Damaged,
    ];
}

/// Structured project-load failure with classification and refusal wording.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectLoadError {
    NotFound {
        path: PathBuf,
    },
    Unreadable {
        path: PathBuf,
        reason: String,
    },
    NotAProject {
        path: Option<PathBuf>,
        reason: String,
    },
    NewerSchema {
        path: Option<PathBuf>,
        found: ProjectSchema,
        expected: ProjectSchema,
    },
    Damaged {
        path: Option<PathBuf>,
        field: String,
        json_path: String,
        reason: String,
    },
}

impl ProjectLoadError {
    /// Classify an OS file-read error on `path`.
    pub fn from_io(path: &Path, err: &io::Error) -> Self {
        if err.kind() == io::ErrorKind::NotFound {
            Self::NotFound {
                path: path.to_path_buf(),
            }
        } else {
            let reason = if err.kind() == io::ErrorKind::PermissionDenied {
                "permission denied".to_owned()
            } else {
                sanitize_load_reason(&err.to_string())
            };
            Self::Unreadable {
                path: path.to_path_buf(),
                reason,
            }
        }
    }

    /// Construct a [`ProjectLoadErrorKind::NotAProject`] error.
    pub fn not_a_project(path: Option<&Path>, reason: impl Into<String>) -> Self {
        Self::NotAProject {
            path: path.map(Path::to_path_buf),
            reason: sanitize_load_reason(&reason.into()),
        }
    }

    /// Construct a [`ProjectLoadErrorKind::NewerSchema`] or wrong-kind
    /// [`ProjectLoadErrorKind::NotAProject`] error from a schema mismatch.
    pub fn from_schema(
        path: Option<&Path>,
        found: &ProjectSchema,
        expected: &ProjectSchema,
    ) -> Self {
        if found.kind == expected.kind && found.version > expected.version {
            Self::NewerSchema {
                path: path.map(Path::to_path_buf),
                found: found.clone(),
                expected: expected.clone(),
            }
        } else {
            let supported = format!("'{}' versions 1 to {}", expected.kind, expected.version);
            Self::NotAProject {
                path: path.map(Path::to_path_buf),
                reason: format!(
                    "its format is '{}' version {}, and this build reads {supported}",
                    found.kind, found.version
                ),
            }
        }
    }

    /// Construct a [`ProjectLoadErrorKind::Damaged`] error naming the field and
    /// JSON path inside the file.
    pub fn damaged(
        path: Option<&Path>,
        field: impl Into<String>,
        json_path: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::Damaged {
            path: path.map(Path::to_path_buf),
            field: field.into(),
            json_path: json_path.into(),
            reason: sanitize_load_reason(&reason.into()),
        }
    }

    /// Classify a `serde_json` parse error against `json` into a [`ProjectLoadErrorKind::Damaged`] error.
    pub fn from_serde(path: Option<&Path>, json: &str, err: &serde_json::Error) -> Self {
        let raw_msg = err.to_string();
        let stripped = strip_serde_line_col(&raw_msg);
        let named_field = extract_named_field(stripped);
        let bytes = json.as_bytes();
        let mut target = byte_offset_for_line_col(json, err.line(), err.column());
        if named_field.is_some() && target > 0 && bytes.get(target - 1) == Some(&b'}') {
            target -= 1;
        }

        let mut stack: Vec<JsonFrame> = Vec::new();
        let mut i = 0usize;
        while i < target && i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    i += 1;
                    let mut s = Vec::new();
                    while i < bytes.len() {
                        match bytes[i] {
                            b'\\' => {
                                i += 1;
                                if i < bytes.len() {
                                    s.push(bytes[i]);
                                    i += 1;
                                }
                            }
                            b'"' => {
                                i += 1;
                                break;
                            }
                            b => {
                                s.push(b);
                                i += 1;
                            }
                        }
                    }
                    if let Some(JsonFrame::Object {
                        key,
                        after_colon: false,
                    }) = stack.last_mut()
                    {
                        *key = Some(String::from_utf8_lossy(&s).into_owned());
                    }
                    continue;
                }
                b'{' => stack.push(JsonFrame::Object {
                    key: None,
                    after_colon: false,
                }),
                b'}' | b']' => {
                    stack.pop();
                }
                b'[' => stack.push(JsonFrame::Array { index: 0 }),
                b':' => {
                    if let Some(JsonFrame::Object { after_colon, .. }) = stack.last_mut() {
                        *after_colon = true;
                    }
                }
                b',' => match stack.last_mut() {
                    Some(JsonFrame::Object { key, after_colon }) => {
                        *key = None;
                        *after_colon = false;
                    }
                    Some(JsonFrame::Array { index }) => *index += 1,
                    None => {}
                },
                _ => {}
            }
            i += 1;
        }

        if let Some(named) = named_field {
            if let Some(JsonFrame::Object { key, .. }) = stack.last_mut() {
                *key = Some(named.to_owned());
            } else {
                stack.push(JsonFrame::Object {
                    key: Some(named.to_owned()),
                    after_colon: true,
                });
            }
        }

        let mut json_path = String::new();
        let mut last_field = None;
        for frame in &stack {
            match frame {
                JsonFrame::Object { key: Some(key), .. } => {
                    if !json_path.is_empty() {
                        json_path.push('.');
                    }
                    json_path.push_str(key);
                    last_field = Some(key.clone());
                }
                JsonFrame::Object { key: None, .. } => {}
                JsonFrame::Array { index } => {
                    json_path.push_str(&format!("[{index}]"));
                }
            }
        }
        let field = last_field.unwrap_or_else(|| "root".to_owned());
        if json_path.is_empty() {
            json_path = field.clone();
        }
        Self::damaged(path, field, json_path, stripped)
    }
    pub fn kind(&self) -> ProjectLoadErrorKind {
        match self {
            Self::NotFound { .. } => ProjectLoadErrorKind::NotFound,
            Self::Unreadable { .. } => ProjectLoadErrorKind::Unreadable,
            Self::NotAProject { .. } => ProjectLoadErrorKind::NotAProject,
            Self::NewerSchema { .. } => ProjectLoadErrorKind::NewerSchema,
            Self::Damaged { .. } => ProjectLoadErrorKind::Damaged,
        }
    }

    /// Format the refusal sentence pair for the status strip and CLI.
    pub fn describe(&self) -> String {
        match self {
            Self::NotFound { path } => format!(
                "No project at {}. Check the path or pick another .aria file.",
                path.display()
            ),
            Self::Unreadable { path, reason } => format!(
                "Could not read project at {} ({reason}). Check the file permissions and try again.",
                path.display()
            ),
            Self::NotAProject { path, reason } => match path {
                Some(path) => format!(
                    "Couldn't open {}: it is not an Aria project file ({reason}). Pick a valid .aria project file.",
                    path.display()
                ),
                None => format!(
                    "Couldn't open this project: it is not an Aria project file ({reason}). Pick a valid .aria project file."
                ),
            },
            Self::NewerSchema {
                path,
                found,
                expected,
            } => {
                let supported = format!("'{}' versions 1 to {}", expected.kind, expected.version);
                match path {
                    Some(path) => format!(
                        "Couldn't open {}: it uses project format version {}, which is newer than this build reads ({supported}). Update Aria to open it.",
                        path.display(),
                        found.version
                    ),
                    None => format!(
                        "Couldn't open this project: it uses project format version {}, which is newer than this build reads ({supported}). Update Aria to open it.",
                        found.version
                    ),
                }
            }
            Self::Damaged {
                path,
                field,
                json_path,
                reason,
            } => match (path, field.is_empty()) {
                (Some(path), false) => format!(
                    "Couldn't open {}: the project file is damaged or corrupted at field '{field}' in '{json_path}' ({reason}). Pick another .aria file.",
                    path.display()
                ),
                (Some(path), true) => format!(
                    "Couldn't open {}: the project file is damaged or corrupted ({reason}). Pick another .aria file.",
                    path.display()
                ),
                (None, false) => format!(
                    "Couldn't open this project: the project file is damaged or corrupted at field '{field}' in '{json_path}' ({reason}). Pick another .aria file."
                ),
                (None, true) => format!(
                    "Couldn't open this project: the project file is damaged or corrupted ({reason}). Pick another .aria file."
                ),
            },
        }
    }
}

impl fmt::Display for ProjectLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.describe())
    }
}

impl From<ProjectLoadError> for ModelError {
    fn from(err: ProjectLoadError) -> Self {
        ModelError::ProjectLoad(err)
    }
}

fn sanitize_load_reason(raw: &str) -> String {
    let without_pos = strip_serde_line_col(raw.trim());
    without_pos
        .replace('`', "'")
        .replace("struct Rational", "a rational rate")
        .replace("Rational", "rate")
        .replace(['#', '{', '}'], "")
}

pub(crate) fn strip_serde_line_col(msg: &str) -> &str {
    if let Some((head, tail)) = msg.rsplit_once(" at line ")
        && let Some((line_part, col_part)) = tail.split_once(" column ")
        && !line_part.is_empty()
        && line_part.bytes().all(|b| b.is_ascii_digit())
        && !col_part.is_empty()
        && col_part.bytes().all(|b| b.is_ascii_digit())
    {
        return head;
    }
    msg
}

enum JsonFrame {
    Object {
        key: Option<String>,
        after_colon: bool,
    },
    Array {
        index: usize,
    },
}

fn extract_named_field(msg: &str) -> Option<&str> {
    for prefix in ["missing field `", "unknown field `", "duplicate field `"] {
        if let Some(rest) = msg.strip_prefix(prefix)
            && let Some(field) = rest.split('`').next()
        {
            return Some(field);
        }
    }
    None
}

fn byte_offset_for_line_col(text: &str, line: usize, col: usize) -> usize {
    let bytes = text.as_bytes();
    let mut cur_line = 1usize;
    let mut cur_col = 1usize;
    for (idx, &b) in bytes.iter().enumerate() {
        if cur_line == line && cur_col >= col {
            return idx;
        }
        if b == b'\n' {
            cur_line += 1;
            cur_col = 1;
        } else {
            cur_col += 1;
        }
    }
    bytes.len()
}

fn format_unsupported_project_schema(found: &ProjectSchema, expected: &ProjectSchema) -> String {
    ProjectLoadError::from_schema(None, found, expected).describe()
}

/// Errors from model mutations that would violate a referential or layout
/// invariant.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("{}", .0.describe())]
    ProjectLoad(ProjectLoadError),

    #[error("{}", format_unsupported_project_schema(found, expected))]
    UnsupportedProjectSchema {
        found: ProjectSchema,
        expected: ProjectSchema,
    },

    #[error("{}", ProjectLoadError::damaged(None, "", "", .0).describe())]
    InvalidProjectFile(String),
    #[error("unknown track: {0}")]
    UnknownTrack(TrackId),

    #[error("unknown media: {0}")]
    UnknownMedia(MediaId),

    #[error("unknown clip: {0}")]
    UnknownClip(ClipId),

    #[error("clip id {0} already exists on the timeline")]
    DuplicateClip(ClipId),

    #[error("unknown marker: {0}")]
    UnknownMarker(MarkerId),

    #[error("unknown bin: {0}")]
    UnknownBin(BinId),

    #[error("bin {0} cannot be moved inside itself or one of its own bins")]
    BinCycle(BinId),
    #[error("unknown sequence: {0}")]
    UnknownSequence(SequenceId),

    #[error("sequence {0} cannot contain itself, directly or transitively")]
    SequenceCycle(SequenceId),

    #[error("clip {0} cannot be parented to itself or to a clip parented to it")]
    ParentCycle(ClipId),

    #[error("the parent clip {parent} is not on the same sequence as clip {child}")]
    ParentElsewhere { child: ClipId, parent: ClipId },

    #[error("the parent clip {parent} does not overlap clip {child} in time")]
    ParentApart { child: ClipId, parent: ClipId },

    #[error("clip {0} is not on a picture lane, so it has no placement to parent")]
    ParentOffPicture(ClipId),

    #[error("the parent clip {0} is scaled to zero, so its child cannot keep its place")]
    ParentFlattened(ClipId),

    #[error("clip {0} would sit under more than {max} parents", max = crate::clip::MAX_PARENT_DEPTH)]
    ParentTooDeep(ClipId),

    #[error("clip overlaps an existing clip on {0}")]
    Overlap(TrackId),

    #[error("track {track} ({kind:?}) cannot hold this clip")]
    IncompatibleTrackKind { track: TrackId, kind: TrackKind },

    #[error("media {0} is still referenced by one or more clips")]
    MediaReferenced(MediaId),

    #[error("source range is outside the media bounds")]
    SourceOutOfBounds,

    #[error("invalid time range (negative or zero duration where positive required)")]
    InvalidRange,

    #[error("invalid transform: {0}")]
    InvalidTransform(String),

    #[error("invalid parameter: {0}")]
    InvalidParam(String),

    #[error("rate mismatch: expected {expected:?}, got {got:?}")]
    RateMismatch { expected: Rational, got: Rational },

    #[error("time arithmetic overflow")]
    TimeOverflow,

    // --- templates --------------------------------------------------------
    #[error("template slot {slot} accepts {accepts:?} but {found:?} media was supplied")]
    SlotMediaMismatch {
        slot: ClipId,
        accepts: SlotMedia,
        found: MediaKind,
    },

    #[error("media supplied for template slot {slot} cannot cover its locked duration")]
    SlotDurationUnmet { slot: ClipId },

    #[error("template slot {slot} is speed-ramped or reversed; filling it is not supported")]
    SlotRetimeUnsupported { slot: ClipId },

    #[error("{given} picks supplied for a template with {slots} fill slots")]
    TooManyPicks { given: usize, slots: usize },

    // --- multicam ---------------------------------------------------------
    #[error("unknown multicam group: {0}")]
    UnknownMulticamGroup(crate::ids::MulticamGroupId),

    #[error("multicam group must have at least one angle")]
    EmptyMulticamGroup,

    #[error("duplicate media {0} in multicam group angles")]
    DuplicateMulticamMedia(MediaId),

    #[error("multicam angle index {index} out of bounds (group has {total} angles)")]
    MulticamAngleOutOfBounds { index: usize, total: usize },

    #[error("multicam switches must be sorted and unique by tick")]
    InvalidMulticamCuts,

    #[error("multicam angle offset is not finite: {0}")]
    InvalidMulticamOffset(String),

    // --- audio routing ----------------------------------------------------
    #[error("track {0} is not a bus, so nothing can be routed into it")]
    NotABus(TrackId),

    #[error("track {0} carries no sound, so it has nothing to route")]
    LaneCarriesNoSound(TrackId),

    #[error("routing track {0} there would feed it back into itself")]
    RoutingCycle(TrackId),

    #[error("track {track} already sends to bus {to}")]
    DuplicateSend { track: TrackId, to: TrackId },

    #[error("track {track} has no send {index}")]
    UnknownSend { track: TrackId, index: usize },

    // --- audio inserts ----------------------------------------------------
    #[error("unknown audio effect: {0}")]
    UnknownAudioEffect(String),

    #[error("clip {0} carries no sound, so it has nothing to process")]
    ClipCarriesNoSound(ClipId),

    #[error("insert {index} is past the end of a chain of {len}")]
    UnknownInsert { index: usize, len: usize },

    #[error("a lane or the master holds at most {0} inserts")]
    InsertsFull(usize),
}

/// Bridge the shared `aria-core` time errors into [`ModelError`] so model
/// methods that propagate rational-time arithmetic with `?` keep returning a
/// single error type.
impl From<TimeError> for ModelError {
    fn from(err: TimeError) -> Self {
        match err {
            TimeError::RateMismatch { expected, got } => ModelError::RateMismatch { expected, got },
            TimeError::Overflow => ModelError::TimeOverflow,
        }
    }
}

impl ModelError {
    /// Classify a project-load error into its [`ProjectLoadErrorKind`].
    pub fn load_error_kind(&self) -> Option<ProjectLoadErrorKind> {
        match self {
            Self::ProjectLoad(err) => Some(err.kind()),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_error_display_formatting() {
        assert_eq!(
            ModelError::InvalidProjectFile("corrupted".into()).to_string(),
            "Couldn't open this project: the project file is damaged or corrupted (corrupted). Pick another .aria file."
        );
        assert_eq!(
            ModelError::UnknownTrack(TrackId::from_raw(5)).to_string(),
            "unknown track: TrackId#5"
        );
        assert_eq!(
            ModelError::UnknownClip(ClipId::from_raw(12)).to_string(),
            "unknown clip: ClipId#12"
        );
        assert_eq!(
            ModelError::SourceOutOfBounds.to_string(),
            "source range is outside the media bounds"
        );
        assert_eq!(
            ModelError::InvalidRange.to_string(),
            "invalid time range (negative or zero duration where positive required)"
        );
    }

    #[test]
    fn every_project_load_error_kind_round_trips_through_model_error() {
        let expected = ProjectSchema::current();
        for kind in ProjectLoadErrorKind::ALL {
            let load_err = match kind {
                ProjectLoadErrorKind::NotFound => ProjectLoadError::NotFound {
                    path: "missing.aria".into(),
                },
                ProjectLoadErrorKind::Unreadable => ProjectLoadError::Unreadable {
                    path: "locked.aria".into(),
                    reason: "No project at misleading prefix".into(),
                },
                ProjectLoadErrorKind::NotAProject => {
                    ProjectLoadError::not_a_project(Some(Path::new("plain.aria")), "not JSON")
                }
                ProjectLoadErrorKind::NewerSchema => ProjectLoadError::from_schema(
                    Some(Path::new("newer.aria")),
                    &ProjectSchema {
                        kind: crate::schema::PROJECT_SCHEMA_KIND.into(),
                        version: crate::schema::PROJECT_SCHEMA_VERSION + 1,
                        extensions: Vec::new(),
                    },
                    &expected,
                ),
                ProjectLoadErrorKind::Damaged => ProjectLoadError::damaged(
                    Some(Path::new("damaged.aria")),
                    "num",
                    "timeline.frame_rate.num",
                    "No project at misleading prefix",
                ),
            };
            assert_eq!(load_err.kind(), kind);
            let model_err = ModelError::ProjectLoad(load_err.clone());
            assert_eq!(
                model_err.load_error_kind(),
                Some(kind),
                "{kind:?} must round-trip its kind through ModelError::ProjectLoad without prefix matching"
            );
            assert_eq!(model_err.to_string(), load_err.describe());
        }
        assert_eq!(
            ModelError::InvalidProjectFile("No project at non-load".into()).load_error_kind(),
            None
        );
    }

    /// WHY: Project load failures previously funneled I/O, non-JSON, and field
    /// parse errors into raw `invalid project file: ...` strings (such as OS
    /// error numbers or serde line/column positions). This test derives the
    /// variant list from `pub enum ProjectLoadErrorKind` in source, builds an
    /// on-disk file case for every kind, and verifies classification and
    /// refusal wording at the `Project::load_from_file` choke point.
    #[test]
    fn every_project_load_error_kind_has_a_file_case_and_user_facing_wording() {
        let src = include_str!("error.rs");
        let (_, tail) = src
            .split_once("pub enum ProjectLoadErrorKind {")
            .expect("ProjectLoadErrorKind declaration present");
        let (body, _) = tail.split_once('}').expect("closing brace present");
        let declared: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with("//"))
            .map(|line| line.trim_end_matches(',').trim())
            .collect();
        let all_names: Vec<String> = ProjectLoadErrorKind::ALL
            .iter()
            .map(|kind| format!("{kind:?}"))
            .collect();
        assert_eq!(
            declared,
            all_names.iter().map(String::as_str).collect::<Vec<_>>(),
            "ProjectLoadErrorKind::ALL must list every declared variant in order"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        for kind in ProjectLoadErrorKind::ALL {
            match kind {
                ProjectLoadErrorKind::NotFound => {
                    let path = dir.path().join("missing.aria");
                    let err = crate::Project::load_from_file(&path).unwrap_err();
                    assert_eq!(err.load_error_kind(), Some(kind));
                    let msg = err.to_string();
                    assert!(
                        msg.starts_with(&format!("No project at {}", path.display())),
                        "NotFound wording must start with 'No project at <path>', got: {msg}"
                    );
                    assert!(!msg.contains("invalid project file"));
                    assert!(!msg.contains("os error"));
                }
                ProjectLoadErrorKind::Unreadable => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let path = dir.path().join("locked.aria");
                        std::fs::write(&path, "{}").expect("write locked file");
                        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000))
                            .expect("chmod 000");
                        let res = crate::Project::load_from_file(&path);
                        let _ =
                            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                        let err = res.expect_err("unreadable file must fail to load");
                        assert_eq!(err.load_error_kind(), Some(kind));
                        let msg = err.to_string();
                        assert!(
                            msg.contains(&path.display().to_string())
                                && msg.contains("permission denied"),
                            "Unreadable wording must name path and permission denied, got: {msg}"
                        );
                        assert!(!msg.contains("invalid project file"));
                        assert!(!msg.contains("os error"));
                    }
                }
                ProjectLoadErrorKind::NotAProject => {
                    let plain = dir.path().join("plain.aria");
                    std::fs::write(&plain, "not json at all").expect("write plain file");
                    let err = crate::Project::load_from_file(&plain).unwrap_err();
                    assert_eq!(err.load_error_kind(), Some(kind));
                    let msg = err.to_string();
                    assert!(
                        msg.contains(&plain.display().to_string())
                            && msg.contains("is not an Aria project file"),
                        "non-JSON NotAProject wording must name path and cause, got: {msg}"
                    );
                    assert!(!msg.contains("at line "));
                    assert!(!msg.contains("invalid project file"));

                    let wrong_kind = dir.path().join("wrong-kind.aria");
                    std::fs::write(
                        &wrong_kind,
                        r#"{"schema":{"kind":"other.document","version":1}}"#,
                    )
                    .expect("write wrong-kind file");
                    let err_kind = crate::Project::load_from_file(&wrong_kind).unwrap_err();
                    assert_eq!(err_kind.load_error_kind(), Some(kind));
                    let msg_kind = err_kind.to_string();
                    assert!(
                        msg_kind.contains("is not an Aria project file")
                            && msg_kind.contains("'other.document'"),
                        "wrong-schema-kind NotAProject wording must name format kind, got: {msg_kind}"
                    );
                }
                ProjectLoadErrorKind::NewerSchema => {
                    let newer = dir.path().join("newer.aria");
                    std::fs::write(
                        &newer,
                        r#"{"schema":{"version":99,"kind":"aria.project"},"id":1,"name":"future","metadata":{},"media":[],"timeline":{"frame_rate":{"num":24,"den":1},"tracks":[],"order":[],"clip_index":[]}}"#,
                    )
                    .expect("write newer-schema file");
                    let err = crate::Project::load_from_file(&newer).unwrap_err();
                    assert_eq!(err.load_error_kind(), Some(kind));
                    let msg = err.to_string();
                    let expected_supported = format!(
                        "'{}' versions 1 to {}",
                        crate::schema::PROJECT_SCHEMA_KIND,
                        crate::schema::PROJECT_SCHEMA_VERSION
                    );
                    assert!(
                        msg.contains("version 99")
                            && msg.contains("newer than this build reads")
                            && msg.contains(&expected_supported),
                        "NewerSchema wording must name both versions, got: {msg}"
                    );
                }
                ProjectLoadErrorKind::Damaged => {
                    let damaged = dir.path().join("damaged.aria");
                    std::fs::write(
                        &damaged,
                        r#"{"schema":{"version":4,"kind":"aria.project"},"id":1,"name":"broken","metadata":{},"media":[],"timeline":{"frame_rate":{"num":"bad","den":1},"tracks":[],"order":[],"clip_index":[]}}"#,
                    )
                    .expect("write damaged file");
                    let err = crate::Project::load_from_file(&damaged).unwrap_err();
                    assert_eq!(err.load_error_kind(), Some(kind));
                    let msg = err.to_string();
                    assert!(
                        msg.contains(&damaged.display().to_string())
                            && msg.contains("damaged or corrupted")
                            && msg.contains("field 'num'")
                            && msg.contains("in 'timeline.frame_rate.num'"),
                        "Damaged wording must name path, field, and JSON path in file, got: {msg}"
                    );
                    assert!(!msg.contains("at line "));
                    assert!(!msg.contains("invalid project file"));
                }
            }
        }
    }
}
