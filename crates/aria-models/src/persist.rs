//! On-disk project files (`.aria` JSON).

use std::fs::{File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::error::{ModelError, ProjectLoadError};
use crate::project::Project;
use crate::schema::{PROJECT_SCHEMA_VERSION, ProjectSchema};

mod pretty;

/// Recommended extension for Aria project files.
pub const PROJECT_FILE_EXTENSION: &str = "aria";

const UNIQUE_PATH_ATTEMPTS: usize = 128;
static UNIQUE_PATH_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Project {
    /// Serialize this project to a `.aria` JSON file.
    ///
    /// The document is stamped with this build's schema version regardless
    /// of what was loaded: the writer defines the format. (A project opened
    /// from a v1 file may now hold v2-only data like keyframes; persisting
    /// it as "v1" would lie to older readers.)
    ///
    /// A path the media pool plays from fails with
    /// [`io::ErrorKind::InvalidInput`] before anything is written: the save
    /// would replace that footage with the project, which would then point at
    /// itself.
    pub fn save_to_file(&self, path: &Path) -> io::Result<()> {
        reject_pool_media(self, path, "project")?;
        let mut stale = None;
        let doc = self.stamped_for_save(&mut stale);
        save_document(path, doc)
    }

    /// The saved document: tab-indented pretty JSON with a trailing newline,
    /// stamped with this build's schema version and kind, written by
    /// [`pretty::to_vec`]. A reader parses any JSON whitespace, so a document
    /// with the two-space indent of earlier builds loads the same.
    ///
    /// A project whose schema is already current is written as it is; only an
    /// older one is copied to be restamped, since the copy of a large timeline
    /// costs as much as writing it.
    fn serialize_current(&self) -> serde_json::Result<Vec<u8>> {
        let mut stale = None;
        let doc = self.stamped_for_save(&mut stale);
        let mut json = pretty::to_vec(doc)?;
        json.push(b'\n');
        Ok(json)
    }

    fn stamped_for_save<'a>(&'a self, stale: &'a mut Option<Project>) -> &'a Project {
        if self.schema.version == PROJECT_SCHEMA_VERSION
            && self.schema.kind == crate::schema::PROJECT_SCHEMA_KIND
        {
            self
        } else {
            let mut copy = self.clone();
            copy.schema.version = PROJECT_SCHEMA_VERSION;
            copy.schema.kind = crate::schema::PROJECT_SCHEMA_KIND.into();
            stale.insert(copy)
        }
    }

    /// Deserialize a project from a `.aria` JSON file.
    ///
    /// The document's schema version is read and validated *before* the
    /// typed parse, and the private `migrate_document` helper rewrites older shapes up to the
    /// current one — so the strict parse below only ever sees the current
    /// format. Files newer than this build are refused, never half-parsed.
    pub fn load_from_file(path: &Path) -> Result<Project, ModelError> {
        let bytes = std::fs::read(path)
            .map_err(|err| ModelError::ProjectLoad(ProjectLoadError::from_io(path, &err)))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            ModelError::ProjectLoad(ProjectLoadError::not_a_project(Some(path), "not JSON"))
        })?;
        Self::load_from_string_at(text, Some(path))
    }

    /// Serialize this project to a JSON string stamped with the current schema version.
    pub fn save_to_string(&self) -> Result<String, ModelError> {
        let json = self
            .serialize_current()
            .map_err(|e| ModelError::InvalidProjectFile(e.to_string()))?;
        String::from_utf8(json).map_err(|e| ModelError::InvalidProjectFile(e.to_string()))
    }

    /// Deserialize a project from a JSON string, validating and migrating its schema.
    pub fn load_from_string(text: &str) -> Result<Project, ModelError> {
        Self::load_from_string_at(text, None)
    }

    fn load_from_string_at(text: &str, path: Option<&Path>) -> Result<Project, ModelError> {
        let bytes = text.as_bytes();
        let prefix = &bytes[..bytes.len().min(STREAM_BUF_CAPACITY)];
        if has_leading_current_schema(prefix)
            && let Ok(mut project) = serde_json::from_str::<Project>(text)
        {
            project.normalize_all_lanes();
            check_layout(&project, path)?;
            return Ok(project);
        }
        Self::load_from_value_str_at(text, path)
    }

    fn load_from_value_str_at(text: &str, path: Option<&Path>) -> Result<Project, ModelError> {
        #[cfg(test)]
        note_value_fallback();
        let bytes = text.as_bytes();
        let prefix = &bytes[..bytes.len().min(STREAM_BUF_CAPACITY)];
        let mut doc: serde_json::Value = match serde_json::from_str(text) {
            Ok(doc) => doc,
            Err(err) => {
                if has_leading_current_schema(prefix) {
                    return Err(ModelError::ProjectLoad(ProjectLoadError::from_serde(
                        path, text, &err,
                    )));
                }
                return Err(ModelError::ProjectLoad(ProjectLoadError::not_a_project(
                    path, "not JSON",
                )));
            }
        };
        if !doc.is_object() {
            return Err(ModelError::ProjectLoad(ProjectLoadError::not_a_project(
                path,
                "not a JSON object",
            )));
        }

        unwrap_legacy_envelope(&mut doc);
        let mut schema = read_schema_at(&doc, path)?;
        normalize_legacy_schema(&mut schema);
        validate_schema_at(&schema, path)?;
        migrate_document(&mut doc, schema.version);

        match serde_json::from_value::<Project>(doc.clone()) {
            Ok(mut project) => {
                // Keep the file's original (normalized) schema as provenance; the
                // writer re-stamps the current version on save.
                project.schema = schema;
                // Files written before the main-track flag (or edited externally)
                // re-derive the lane-zone / main-track invariants on entry.
                project.normalize_all_lanes();
                check_layout(&project, path)?;
                Ok(project)
            }
            Err(_) => {
                let pretty = serde_json::to_string_pretty(&doc).unwrap_or_else(|_| text.to_owned());
                let err = serde_json::from_str::<Project>(&pretty)
                    .expect_err("document rejected by from_value must fail from_str");
                Err(ModelError::ProjectLoad(ProjectLoadError::from_serde(
                    path, &pretty, &err,
                )))
            }
        }
    }
}

/// Refuse a project a sequence of which breaks a placement rule
/// ([`crate::Timeline::layout_fault`]) or holds a clip playing something the
/// project does not hold or a sequence containing itself
/// ([`Project::reference_fault`]), or a multicam group of which is invalid
/// ([`Project::multicam_fault`]), naming the field and the clip or group.
/// Both parse paths read this, so no edit or render reads a clip
/// [`crate::Timeline::add_clip`] or [`Project::add_clip`] refuses.
fn check_layout(project: &Project, path: Option<&Path>) -> Result<(), ModelError> {
    let timeline_fault = |timeline: &crate::Timeline, holder: Option<crate::SequenceId>| {
        timeline.layout_fault().or_else(|| {
            project
                .reference_fault(timeline, holder)
                .map(|reason| ("tracks", reason))
        })
    };
    let fault = timeline_fault(project.root_timeline(), None)
        .map(|(field, reason)| (field, format!("timeline.{field}"), reason))
        .or_else(|| {
            project.sequences().find_map(|sequence| {
                timeline_fault(&sequence.timeline, Some(sequence.id)).map(|(field, reason)| {
                    (
                        field,
                        format!("sequences.timeline.{field}"),
                        format!("in sequence '{}', {reason}", sequence.name),
                    )
                })
            })
        })
        .or_else(|| {
            let field = "multicam_groups";
            project
                .multicam_fault()
                .map(|reason| (field, field.to_owned(), reason))
        });
    match fault {
        Some((field, json_path, reason)) => {
            Err(ProjectLoadError::damaged(path, field, json_path, reason).into())
        }
        None => Ok(()),
    }
}

trait PersistenceFs {
    fn create_new(&self, path: &Path) -> io::Result<File>;
    fn write_all(&self, file: &mut File, contents: &[u8]) -> io::Result<()>;
    fn flush(&self, file: &mut File) -> io::Result<()>;
    fn set_permissions(&self, file: &File, permissions: Permissions) -> io::Result<()>;
    fn sync_all(&self, file: &File) -> io::Result<()>;
    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata>;
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;
    fn create_dir(&self, path: &Path) -> io::Result<()>;
    fn remove_file(&self, path: &Path) -> io::Result<()>;
    fn remove_dir(&self, path: &Path) -> io::Result<()>;
}

struct StdPersistenceFs;

impl PersistenceFs for StdPersistenceFs {
    fn create_new(&self, path: &Path) -> io::Result<File> {
        open_new_temp(path)
    }

    fn write_all(&self, file: &mut File, contents: &[u8]) -> io::Result<()> {
        file.write_all(contents)
    }

    fn flush(&self, file: &mut File) -> io::Result<()> {
        file.flush()
    }

    fn set_permissions(&self, file: &File, permissions: Permissions) -> io::Result<()> {
        file.set_permissions(permissions)
    }

    fn sync_all(&self, file: &File) -> io::Result<()> {
        file.sync_all()
    }

    fn symlink_metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        std::fs::symlink_metadata(path)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        create_private_dir(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn remove_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir(path)
    }
}

const STREAM_BUF_CAPACITY: usize = 64 * 1024;

#[cfg(test)]
std::thread_local! {
    static STREAM_WRITE_STATS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[cfg(test)]
fn reset_stream_write_stats() {
    STREAM_WRITE_STATS.with(|stats| stats.set((0, 0)));
}

#[cfg(test)]
fn stream_write_stats() -> (usize, usize) {
    STREAM_WRITE_STATS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn note_stream_write(len: usize) {
    STREAM_WRITE_STATS.with(|stats| {
        let (calls, max_len) = stats.get();
        stats.set((calls + 1, max_len.max(len)));
    });
}

#[cfg(test)]
std::thread_local! {
    static VALUE_FALLBACK_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn reset_value_fallback_count() {
    VALUE_FALLBACK_COUNT.with(|count| count.set(0));
}

#[cfg(test)]
fn value_fallback_count() -> usize {
    VALUE_FALLBACK_COUNT.with(std::cell::Cell::get)
}

#[cfg(test)]
fn note_value_fallback() {
    VALUE_FALLBACK_COUNT.with(|count| count.set(count.get() + 1));
}

fn has_leading_current_schema(buf: &[u8]) -> bool {
    let rest = buf.trim_ascii_start();
    let Some(rest) = rest.strip_prefix(b"{") else {
        return false;
    };
    let rest = rest.trim_ascii_start();
    let Some(rest) = rest.strip_prefix(b"\"schema\"") else {
        return false;
    };
    let rest = rest.trim_ascii_start();
    let Some(rest) = rest.strip_prefix(b":") else {
        return false;
    };
    let mut stream = serde_json::Deserializer::from_slice(rest).into_iter::<ProjectSchema>();
    let Some(Ok(schema)) = stream.next() else {
        return false;
    };
    let after = rest[stream.byte_offset()..].trim_ascii_start();
    matches!(after.first(), Some(b',' | b'}'))
        && schema.version == PROJECT_SCHEMA_VERSION
        && schema.kind == crate::schema::PROJECT_SCHEMA_KIND
}

struct FsWriter<'a, F> {
    file: &'a mut File,
    fs: &'a F,
}

impl<F: PersistenceFs> Write for FsWriter<'_, F> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(test)]
        note_stream_write(buf.len());
        self.fs
            .write_all(self.file, buf)
            .map_err(|error| contextual_error("could not write the temporary project", error))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Fail with [`io::ErrorKind::InvalidInput`] when `destination` is a file the
/// media pool of `project` plays from: a save would replace that footage with
/// a `document` that references it.
pub(crate) fn reject_pool_media(
    project: &Project,
    destination: &Path,
    document: &str,
) -> io::Result<()> {
    if project.plays_file(destination) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} is media this {document} uses; save the {document} to another file",
                destination.display()
            ),
        ));
    }
    Ok(())
}

/// Write `doc` as the saved JSON document at `destination`, as
/// [`save_streamed`] does.
pub(crate) fn save_document<T: Serialize + ?Sized>(destination: &Path, doc: &T) -> io::Result<()> {
    persist_streaming(destination, &StdPersistenceFs, |writer| {
        pretty::to_writer(&mut *writer, doc).map_err(io::Error::other)?;
        writer.write_all(b"\n")
    })
}

/// Replace `destination` with the bytes `write` produces, through a synced
/// sibling temporary renamed over it. A failed write leaves the old file
/// whole, and a hard link to it keeps the old bytes. A symbolic-link or
/// non-regular destination fails with [`io::ErrorKind::InvalidInput`]; an
/// existing file's permissions carry over to the replacement.
pub(crate) fn save_streamed(
    destination: &Path,
    write: impl FnOnce(&mut dyn Write) -> io::Result<()>,
) -> io::Result<()> {
    persist_streaming(destination, &StdPersistenceFs, |writer| write(writer))
}

fn persist_streaming<F: PersistenceFs>(
    destination: &Path,
    fs: &F,
    write: impl FnOnce(&mut io::BufWriter<FsWriter<'_, F>>) -> io::Result<()>,
) -> io::Result<()> {
    let permissions = destination_permissions(destination, fs)?;
    let temporary = write_temp(destination, permissions, fs, |file| {
        let mut writer = io::BufWriter::with_capacity(STREAM_BUF_CAPACITY, FsWriter { file, fs });
        write(&mut writer)?;
        writer.into_inner().map_err(|err| err.into_error())?;
        Ok(())
    })?;
    install_temp_with_ops(destination, &temporary, fs)
}

/// Create a unique temporary beside `destination`, fill it with `write`, then
/// flush it, give it `permissions`, and sync it. The temporary is removed when
/// any step fails.
fn write_temp(
    destination: &Path,
    permissions: Option<Permissions>,
    fs: &impl PersistenceFs,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<PathBuf> {
    let (temporary, mut file) = create_unique_temp(destination, fs)?;
    let result = (|| {
        write(&mut file)?;
        fs.flush(&mut file)
            .map_err(|error| contextual_error("could not flush the temporary project", error))?;
        if let Some(permissions) = permissions {
            fs.set_permissions(&file, permissions).map_err(|error| {
                contextual_error("could not preserve project file permissions", error)
            })?;
        }
        fs.sync_all(&file)
            .map_err(|error| contextual_error("could not sync the temporary project", error))
    })();
    drop(file);

    match result {
        Ok(()) => Ok(temporary),
        Err(error) => Err(cleanup_temp_after_error(fs, &temporary, error)),
    }
}

#[cfg(test)]
fn persist_serialized(
    destination: &Path,
    contents: &[u8],
    fs: &impl PersistenceFs,
) -> io::Result<()> {
    let permissions = destination_permissions(destination, fs)?;
    let temporary = write_synced_temp(destination, contents, permissions, fs)?;
    install_temp_with_ops(destination, &temporary, fs)
}

fn destination_permissions(
    destination: &Path,
    fs: &impl PersistenceFs,
) -> io::Result<Option<Permissions>> {
    match fs.symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "refusing to save through a symbolic-link project destination",
        )),
        Ok(metadata) if !metadata.file_type().is_file() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project destination is not a regular file",
        )),
        Ok(metadata) => Ok(Some(metadata.permissions())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(contextual_error(
            "could not inspect the project destination",
            error,
        )),
    }
}

#[cfg(test)]
fn write_synced_temp(
    destination: &Path,
    contents: &[u8],
    permissions: Option<Permissions>,
    fs: &impl PersistenceFs,
) -> io::Result<PathBuf> {
    write_temp(destination, permissions, fs, |file| {
        fs.write_all(file, contents)
            .map_err(|error| contextual_error("could not write the temporary project", error))
    })
}

fn create_unique_temp(destination: &Path, fs: &impl PersistenceFs) -> io::Result<(PathBuf, File)> {
    for _ in 0..UNIQUE_PATH_ATTEMPTS {
        let candidate = unique_sibling_path(destination, "tmp")?;
        match fs.create_new(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(contextual_error(
                    "could not create a temporary project file",
                    error,
                ));
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique temporary project file",
    ))
}

fn open_new_temp(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[derive(Debug)]
struct BackupPaths {
    container: PathBuf,
    project: PathBuf,
}

fn create_unique_backup(destination: &Path, fs: &impl PersistenceFs) -> io::Result<BackupPaths> {
    for _ in 0..UNIQUE_PATH_ATTEMPTS {
        let container = unique_sibling_path(destination, "backup")?;
        match fs.create_dir(&container) {
            Ok(()) => {
                let project = container.join("original");
                return Ok(BackupPaths { container, project });
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => {
                return Err(contextual_error(
                    "could not create a rollback backup for the project",
                    error,
                ));
            }
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique rollback backup for the project",
    ))
}

/// Create the directory `path`, readable and writable by its owner alone on
/// Unix (mode `0700`).
#[cfg(unix)]
fn create_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

/// Create the directory `path`. Windows directories inherit their parent's
/// access control list.
#[cfg(not(unix))]
fn create_private_dir(path: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new().create(path)
}

fn install_temp_with_ops(
    destination: &Path,
    temporary: &Path,
    fs: &impl PersistenceFs,
) -> io::Result<()> {
    // Recheck after the potentially long write/sync phase. This is not a
    // platform-specific openat lock, but it fails closed for every symlink
    // destination observed by the portable persistence path.
    if let Err(error) = destination_permissions(destination, fs) {
        return Err(cleanup_temp_after_error(fs, temporary, error));
    }

    let atomic_error = match fs.rename(temporary, destination) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };

    match destination_permissions(destination, fs) {
        Ok(Some(_)) => {}
        Ok(None) => {
            let error =
                contextual_error("could not atomically install the new project", atomic_error);
            return Err(cleanup_temp_after_error(fs, temporary, error));
        }
        Err(error) => return Err(cleanup_temp_after_error(fs, temporary, error)),
    }

    // Some platforms cannot rename over an existing file. Reserve a private,
    // unique sibling directory first, then move the old file into it. The
    // original is always either at the destination or in this backup; it is
    // never deleted before the new file reaches the commit point.
    let backup = match create_unique_backup(destination, fs) {
        Ok(backup) => backup,
        Err(error) => {
            let error = io::Error::new(
                error.kind(),
                format!(
                    "atomic project replacement failed ({atomic_error}); \
                     rollback backup creation also failed: {error}"
                ),
            );
            return Err(cleanup_temp_after_error(fs, temporary, error));
        }
    };

    if let Err(backup_error) = fs.rename(destination, &backup.project) {
        let error = io::Error::new(
            backup_error.kind(),
            format!(
                "atomic project replacement failed ({atomic_error}); moving the existing project \
                 to a rollback backup also failed: {backup_error}; the existing project was left \
                 in place"
            ),
        );
        let error = cleanup_empty_backup_after_error(fs, &backup.container, error);
        return Err(cleanup_temp_after_error(fs, temporary, error));
    }

    match fs.rename(temporary, destination) {
        Ok(()) => {
            // The new destination is committed. Cleanup is deliberately
            // best-effort: a stale backup must not turn a successful save into
            // an error after callers can already observe the new project.
            let _ = fs.remove_file(&backup.project);
            let _ = fs.remove_dir(&backup.container);
            Ok(())
        }
        Err(install_error) => match fs.rename(&backup.project, destination) {
            Ok(()) => {
                let error = io::Error::new(
                    install_error.kind(),
                    format!(
                        "could not install the new project after creating a rollback backup: \
                         {install_error}; the original project was restored"
                    ),
                );
                let error = cleanup_empty_backup_after_error(fs, &backup.container, error);
                Err(cleanup_temp_after_error(fs, temporary, error))
            }
            Err(rollback_error) => {
                let error = io::Error::new(
                    install_error.kind(),
                    format!(
                        "could not install the new project after creating a rollback backup: \
                         {install_error}; restoring the original project failed: {rollback_error}; \
                         the original project was retained in a sibling backup"
                    ),
                );
                Err(cleanup_temp_after_error(fs, temporary, error))
            }
        },
    }
}

fn unique_sibling_path(destination: &Path, role: &str) -> io::Result<PathBuf> {
    let parent = destination_parent(destination)?;
    if destination.file_name().is_none() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "project destination has no file name",
        ));
    }

    let nonce = UNIQUE_PATH_COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".aria-project-{role}-{}-{nonce}",
        std::process::id()
    )))
}

fn destination_parent(destination: &Path) -> io::Result<&Path> {
    let parent = destination.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "project destination has no parent directory",
        )
    })?;
    if parent.as_os_str().is_empty() {
        Ok(Path::new("."))
    } else {
        Ok(parent)
    }
}

fn cleanup_temp_after_error(
    fs: &impl PersistenceFs,
    temporary: &Path,
    primary_error: io::Error,
) -> io::Error {
    match fs.remove_file(temporary) {
        Ok(()) => primary_error,
        Err(error) if error.kind() == io::ErrorKind::NotFound => primary_error,
        Err(error) => io::Error::new(
            primary_error.kind(),
            format!(
                "{primary_error}; temporary project cleanup also failed: {error}; \
                 a temporary sibling may remain"
            ),
        ),
    }
}

fn cleanup_empty_backup_after_error(
    fs: &impl PersistenceFs,
    backup_container: &Path,
    primary_error: io::Error,
) -> io::Error {
    match fs.remove_dir(backup_container) {
        Ok(()) => primary_error,
        Err(error) if error.kind() == io::ErrorKind::NotFound => primary_error,
        Err(error) => io::Error::new(
            primary_error.kind(),
            format!(
                "{primary_error}; empty rollback-backup cleanup also failed: {error}; \
                 an empty sibling directory may remain"
            ),
        ),
    }
}

fn contextual_error(context: &'static str, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{context}: {error}"))
}

/// Unwrap the legacy `{ "version", "project" }` envelope (early v1 saves)
/// to the bare project document, pushing the envelope version into the
/// inner schema when the project carries none (or a placeholder version 0).
fn unwrap_legacy_envelope(doc: &mut serde_json::Value) {
    let Some(obj) = doc.as_object_mut() else {
        return;
    };
    // A real project document has a schema (or its legacy alias); only the
    // envelope nests the project under "project". Unknown root fields named
    // "project"/"version" on a current file must not trip this.
    let looks_like_envelope = obj.contains_key("project")
        && obj.contains_key("version")
        && !obj.contains_key("schema")
        && !obj.contains_key("schema_version");
    if !looks_like_envelope {
        return;
    }
    let version = obj.get("version").and_then(serde_json::Value::as_u64);
    let Some(mut project) = obj.remove("project") else {
        return;
    };
    if let (Some(version), Some(inner)) = (version, project.as_object_mut()) {
        let placeholder = inner
            .get("schema")
            .and_then(|s| s.get("version"))
            .and_then(serde_json::Value::as_u64)
            == Some(0);
        let missing = !inner.contains_key("schema") && !inner.contains_key("schema_version");
        if missing || placeholder {
            inner.insert("schema".into(), serde_json::json!(version));
        }
    }
    *doc = project;
}

/// Read the document's schema (object or legacy bare-integer form) without
/// deserializing the whole project. Shared with the template loader, which
/// wraps a project document and versions in lockstep with it.
pub(crate) fn read_schema(doc: &serde_json::Value) -> Result<ProjectSchema, ModelError> {
    read_schema_at(doc, None)
}

fn read_schema_at(
    doc: &serde_json::Value,
    path: Option<&Path>,
) -> Result<ProjectSchema, ModelError> {
    #[derive(Deserialize)]
    struct Holder(#[serde(deserialize_with = "crate::schema::deserialize")] ProjectSchema);

    let err = |reason| ModelError::ProjectLoad(ProjectLoadError::not_a_project(path, reason));
    let raw = doc
        .get("schema")
        .or_else(|| doc.get("schema_version"))
        .ok_or_else(|| err("missing project schema"))?;
    serde_json::from_value::<Holder>(raw.clone())
        .map(|holder| holder.0)
        .map_err(|_| err("invalid project schema"))
}

/// Rewrite a raw project document from `from`'s shape to the current
/// schema's, one version step at a time, before the typed parse.
///
/// This is the format versioning policy (v1 roadmap M0):
///
/// - **Additive optional fields don't bump the version.** New fields ship
///   with `#[serde(default)]` (+ skip-if-default on save); readers of the
///   same version ignore fields they don't know — and drop them on resave.
///   That tolerance is the compatibility contract *within* a version.
/// - **Shape changes bump [`PROJECT_SCHEMA_VERSION`]** and add a
///   `migrate_vN_to_vN1` step here rewriting the previous shape into the
///   new one.
/// - **Newer documents are refused** before this runs
///   ([`ModelError::UnsupportedProjectSchema`] from `validate_schema`) —
///   guessing at a future format risks silent data loss.
///
/// The template loader reuses this on its embedded project document
/// (`.ariat` files version in lockstep with [`PROJECT_SCHEMA_VERSION`]).
pub(crate) fn migrate_document(doc: &mut serde_json::Value, from: u32) {
    for step in from..PROJECT_SCHEMA_VERSION {
        match step {
            1 => migrate_v1_to_v2(doc),
            2 => migrate_v2_to_v3(doc),
            3 => migrate_v3_to_v4(doc),
            // `validate_schema` bounds `from` to supported versions, so a
            // missing arm is a bug: a version bump landed without its step.
            _ => unreachable!("no migration step for schema v{step} -> v{}", step + 1),
        }
    }
}

/// v1 → v2 (M2 animatable params): transform properties gained the
/// `{"kf": [...]}` curve shape, but constants kept the bare v1 value shape,
/// so every v1 document is already a valid v2 document — nothing to
/// rewrite. The step exists to anchor the chain.
fn migrate_v1_to_v2(_doc: &mut serde_json::Value) {}

/// v2 → v3 (mask stacks): a clip's one `mask` becomes a one-mask `masks`
/// stack, and a driver on a look-axis mask property (`mask_feather`, …) moves
/// to the same property of mask 0.
///
/// Clips sit in tracks, in nested sequences' timelines and inside a bake's
/// put-aside copy, so the walk visits every value and rewrites each object
/// shaped like a clip: the only one carrying both `content` and `timeline`.
fn migrate_v2_to_v3(doc: &mut serde_json::Value) {
    match doc {
        serde_json::Value::Array(items) => items.iter_mut().for_each(migrate_v2_to_v3),
        serde_json::Value::Object(fields) => {
            if fields.contains_key("content") && fields.contains_key("timeline") {
                migrate_v2_clip(fields);
            }
            fields.values_mut().for_each(migrate_v2_to_v3);
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

fn migrate_v2_clip(clip: &mut serde_json::Map<String, serde_json::Value>) {
    match clip.remove("mask") {
        None | Some(serde_json::Value::Null) => {}
        Some(mask) => {
            clip.insert("masks".into(), serde_json::Value::Array(vec![mask]));
        }
    }
    let Some(serde_json::Value::Array(drivers)) = clip.get_mut("drivers") else {
        return;
    };
    for driver in drivers {
        let Some(param) = driver.get_mut("param") else {
            continue;
        };
        let Some(name) = param
            .pointer("/look/param")
            .and_then(serde_json::Value::as_str)
            .and_then(|name| name.strip_prefix("mask_"))
        else {
            continue;
        };
        let migrated = serde_json::json!({ "mask": { "mask": 0, "param": name } });
        *param = migrated;
    }
}

/// v3 → v4 (per-project plugin storage): `Project` gained `plugin_data`
/// (`BTreeMap<String, BTreeMap<String, Vec<u8>>>`), which defaults to an
/// empty map when absent and is omitted on save when empty. If a v3 document
/// carries `plugin_data: null`, strip the null so `#[serde(default)]`
/// initializes an empty map.
fn migrate_v3_to_v4(doc: &mut serde_json::Value) {
    if let Some(obj) = doc.as_object_mut()
        && obj
            .get("plugin_data")
            .is_some_and(serde_json::Value::is_null)
    {
        obj.remove("plugin_data");
    }
}

fn normalize_legacy_schema(schema: &mut ProjectSchema) {
    if schema.kind.is_empty() {
        schema.kind = crate::schema::PROJECT_SCHEMA_KIND.into();
    }
}
#[cfg(test)]
fn validate_schema(found: &ProjectSchema) -> Result<(), ModelError> {
    validate_schema_at(found, None)
}

fn validate_schema_at(found: &ProjectSchema, path: Option<&Path>) -> Result<(), ModelError> {
    let expected = ProjectSchema::current();
    if !found.is_supported() {
        return Err(ModelError::ProjectLoad(ProjectLoadError::from_schema(
            path, found, &expected,
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
#[cfg(test)]
#[path = "persist/tests_keys.rs"]
mod tests_keys;
#[cfg(test)]
#[path = "persist/tests_layout.rs"]
mod tests_layout;
#[cfg(test)]
#[path = "persist/tests_mask_stack.rs"]
mod tests_mask_stack;
#[cfg(test)]
#[path = "persist/tests_refs.rs"]
mod tests_refs;
