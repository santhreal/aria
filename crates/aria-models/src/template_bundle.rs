//! Distributable template bundles (`.ariab`).
//!
//! A raw `.ariat` references its sample media by **local absolute path**
//! and is therefore not distributable — on any other machine every slot is
//! missing media. A bundle is the shippable form: one uncompressed tar
//! archive (sample media is already-compressed mp4/jpg; the JSON is small)
//! containing
//!
//! ```text
//! bundle.json          manifest: name, min_schema_version, format version
//! template.ariat    the template, media paths rewritten to media/<n>.<ext>
//! media/0.mp4          sample media at relative paths
//! media/1.jpg
//! ```
//!
//! The manifest exists so a gallery (or an older app) can decide whether it
//! can open the bundle **without** parsing the full template document:
//! `min_schema_version` is the embedded project's schema version, refused
//! by builds older than it.
//!
//! [`write()`] rewrites pool paths to the relative `media/` form while
//! archiving; [`install`] extracts into a per-template directory and
//! rewrites them back to absolute, yielding a directory whose
//! `template.ariat` loads with the ordinary [`Template::load_from_file`].
//! Extraction validates entry names (the tar path-traversal classic) and
//! refuses anything outside `media/` and the two known files.

use std::fmt::Display;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ModelError;
use crate::schema::PROJECT_SCHEMA_VERSION;
use crate::template::Template;
use aria_core::{io_reason, unwritable_directory};

/// Recommended extension for template bundles.
pub const BUNDLE_FILE_EXTENSION: &str = "ariab";

/// Bumped only if the *archive layout* changes (not the template schema —
/// that is what `min_schema_version` tracks).
pub const BUNDLE_FORMAT_VERSION: u32 = 1;

/// The `bundle.json` manifest at the head of every bundle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
    /// Archive layout version ([`BUNDLE_FORMAT_VERSION`]).
    pub format_version: u32,
    /// Display name (mirrors the template's meta, for gallery listings).
    pub name: String,
    /// The embedded project-schema version; apps older than this must
    /// refuse the bundle instead of half-parsing it.
    pub min_schema_version: u32,
}

/// The fix for a file that is not a bundle this build can open.
const PICK_ANOTHER: &str = "Pick another .ariab file.";

/// The fix for a folder a bundle could not be written in.
const CHECK_THE_FOLDER: &str = "Check the folder and the free disk space.";

/// A refusal that states the bundle at `path`, what `verb` met, and the fix.
fn refused(verb: &str, path: &Path, reason: impl Display, fix: &str) -> ModelError {
    ModelError::TemplateBundle(format!(
        "Couldn't {verb} the template bundle {}: {reason}. {fix}",
        path.display()
    ))
}

/// Write `template` as a distributable bundle at `path`. Every pool media
/// file is read from its current (absolute) path and archived under
/// `media/`; the archived template references those relative paths.
///
/// Fails if any pool media file is missing on disk — a bundle with absent
/// sample media would be broken on every machine, so authoring refuses it
/// here rather than shipping the problem. The archive replaces `path` the way
/// a project save does: through a synced temporary renamed over it, so a
/// failed write leaves an existing bundle whole. A path the media pool plays
/// from, or a symbolic link, fails before anything is written.
pub fn write(template: &Template, path: &Path) -> Result<(), ModelError> {
    if crate::persist::reject_pool_media(template.project(), path, "bundle").is_err() {
        return Err(refused(
            "write",
            path,
            "it is media this bundle uses",
            "Write the bundle to another file.",
        ));
    }

    // Plan the rewrite first: media id -> (source path, archive name).
    let mut entries: Vec<(crate::ids::MediaId, PathBuf, String)> = Vec::new();
    for (index, media) in template.project().media_iter().enumerate() {
        let source = media.path().to_path_buf();
        if !source.is_file() {
            return Err(refused(
                "write",
                path,
                format_args!("the sample media {} is missing", source.display()),
                "Relink it, then write the bundle again.",
            ));
        }
        let ext = source
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("bin")
            .to_lowercase();
        entries.push((media.id, source, format!("media/{index}.{ext}")));
    }

    // The archived template carries the relative paths.
    let mut doc = template.clone();
    for (id, _, archive_name) in &entries {
        if let Some(media) = doc.project.media_mut(*id) {
            media.path = PathBuf::from(archive_name);
        }
    }

    let manifest = BundleManifest {
        format_version: BUNDLE_FORMAT_VERSION,
        name: template.meta().name.clone(),
        min_schema_version: PROJECT_SCHEMA_VERSION,
    };

    let unwritten = |e: &dyn Display| refused("write", path, e, CHECK_THE_FOLDER);
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| unwritten(&e))?;
    let template_json = serde_json::to_vec_pretty(&doc).map_err(|e| unwritten(&e))?;

    crate::persist::save_streamed(path, |out| {
        let mut builder = tar::Builder::new(out);
        append_bytes(&mut builder, "bundle.json", &manifest_json)?;
        append_bytes(&mut builder, "template.ariat", &template_json)?;
        for (_, source, archive_name) in &entries {
            builder.append_path_with_name(source, archive_name)?;
        }
        builder.into_inner()?;
        Ok(())
    })
    .map_err(|e| unwritten(&io_reason(&e)))
}

/// Open the bundle at `path` as a tar archive. A folder, a pipe or a device
/// is refused before any read.
fn open_archive(
    path: &Path,
) -> Result<tar::Archive<std::io::BufReader<std::fs::File>>, ModelError> {
    if let Some(what) = crate::persist::special_file(path) {
        return Err(refused(
            "open",
            path,
            format_args!("it is {what}"),
            PICK_ANOTHER,
        ));
    }
    let file = std::fs::File::open(path).map_err(|e| {
        let reason = match e.kind() {
            std::io::ErrorKind::NotFound => "no such file".to_owned(),
            _ => io_reason(&e),
        };
        refused("open", path, reason, PICK_ANOTHER)
    })?;
    Ok(tar::Archive::new(std::io::BufReader::new(file)))
}

/// The refusal for an archive that does not read as a tar file. The tar
/// reader's own message quotes the header bytes it could not parse, which for
/// a project or a template is the start of the document, so only a failure
/// of the file itself is stated as it is.
fn not_a_bundle(path: &Path, e: &std::io::Error) -> ModelError {
    let reason = if e.raw_os_error().is_some() {
        io_reason(e)
    } else {
        "not a tar archive".to_owned()
    };
    refused(
        "open",
        path,
        format_args!("it is not a template bundle ({reason})"),
        PICK_ANOTHER,
    )
}

/// Whether `bytes` start as a tar archive, as every bundle does: the `ustar`
/// magic of the first header, at byte 257.
pub(crate) fn is_archive(bytes: &[u8]) -> bool {
    bytes.get(257..262) == Some(b"ustar".as_slice())
}

/// Read just the manifest — the cheap compatibility check a gallery runs
/// before downloading nothing else or offering "requires a newer Aria".
pub fn read_manifest(path: &Path) -> Result<BundleManifest, ModelError> {
    let garbled = |e: std::io::Error| not_a_bundle(path, &e);
    let mut archive = open_archive(path)?;
    for entry in archive.entries().map_err(garbled)? {
        let mut entry = entry.map_err(garbled)?;
        if entry.path().map_err(garbled)?.as_ref() == Path::new("bundle.json") {
            let mut raw = String::new();
            entry.read_to_string(&mut raw).map_err(garbled)?;
            return serde_json::from_str(&raw).map_err(|e| {
                refused(
                    "open",
                    path,
                    format_args!("its bundle.json is not a bundle manifest ({e})"),
                    PICK_ANOTHER,
                )
            });
        }
    }
    Err(refused(
        "open",
        path,
        "it is not a template bundle (it has no bundle.json)",
        PICK_ANOTHER,
    ))
}

/// Extract a bundle into `dest_dir` (created; expected empty or absent) and
/// return the loaded, render-ready template whose media paths point into
/// `dest_dir/media/`. The rewritten `template.ariat` is left in
/// `dest_dir`, so subsequent sessions load it directly with
/// [`Template::load_from_file`] — install once, open forever.
///
/// Every entry name is checked before the first is extracted, so a bundle
/// with an entry outside the layout writes nothing.
pub fn install(path: &Path, dest_dir: &Path) -> Result<Template, ModelError> {
    // The media paths are rewritten onto this folder, so a relative one is
    // made absolute first; an empty one stays as given and fails to be made.
    let absolute = std::path::absolute(dest_dir);
    let dest_dir = absolute.as_deref().unwrap_or(dest_dir);
    let garbled = |e: std::io::Error| not_a_bundle(path, &e);
    let unwritten = |e: std::io::Error| {
        refused(
            "install",
            path,
            io_reason(&unwritable_directory(dest_dir, &e)),
            CHECK_THE_FOLDER,
        )
    };
    let newer = |what: &str, found: u32, read: u32| {
        refused(
            "install",
            path,
            format_args!(
                "it uses {what} version {found}, which is newer than this build reads ({read})"
            ),
            "Update Aria to install it.",
        )
    };

    let manifest = read_manifest(path)?;
    if manifest.format_version > BUNDLE_FORMAT_VERSION {
        return Err(newer(
            "bundle format",
            manifest.format_version,
            BUNDLE_FORMAT_VERSION,
        ));
    }
    if manifest.min_schema_version > PROJECT_SCHEMA_VERSION {
        return Err(newer(
            "project format",
            manifest.min_schema_version,
            PROJECT_SCHEMA_VERSION,
        ));
    }

    for entry in open_archive(path)?.entries_with_seek().map_err(garbled)? {
        let entry = entry.map_err(garbled)?;
        let name = entry.path().map_err(garbled)?;
        if !safe_entry_name(&name) {
            return Err(refused(
                "install",
                path,
                format_args!(
                    "it holds \"{}\", a path outside the bundle layout",
                    name.display()
                ),
                PICK_ANOTHER,
            ));
        }
    }

    std::fs::create_dir_all(dest_dir).map_err(unwritten)?;
    let mut archive = open_archive(path)?;
    for entry in archive.entries().map_err(garbled)? {
        let mut entry = entry.map_err(garbled)?;
        let dest = dest_dir.join(entry.path().map_err(garbled)?);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(unwritten)?;
        }
        entry.unpack(&dest).map_err(unwritten)?;
    }

    // Rewrite the extracted template's relative media paths to absolute.
    let template_path = dest_dir.join("template.ariat");
    let mut template = Template::load_from_file(&template_path)?;
    let ids: Vec<crate::ids::MediaId> = template.project().media_iter().map(|m| m.id).collect();
    for id in ids {
        if let Some(media) = template.project.media_mut(id)
            && media.path.is_relative()
        {
            media.path = dest_dir.join(&media.path);
        }
    }
    template.save_to_file(&template_path).map_err(unwritten)?;
    Ok(template)
}

/// Only the two known files and normal-component paths under `media/` may
/// unpack (no absolute paths, no `..`, no other roots).
fn safe_entry_name(name: &Path) -> bool {
    if name == Path::new("bundle.json") || name == Path::new("template.ariat") {
        return true;
    }
    let mut components = name.components();
    if components.next() != Some(Component::Normal("media".as_ref())) {
        return false;
    }
    components.all(|c| matches!(c, Component::Normal(_)))
}

fn append_bytes<W: std::io::Write>(
    builder: &mut tar::Builder<W>,
    name: &str,
    bytes: &[u8],
) -> std::io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append_data(&mut header, name, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::Replaceable;
    use crate::template::TemplateMeta;
    use crate::time::{Rational, RationalTime, TimeRange};
    use crate::track::TrackKind;
    use crate::{MediaSource, Project};

    const R24: Rational = Rational::FPS_24;

    /// A one-slot template whose sample media is a real file on disk.
    fn template_with_real_media(dir: &Path) -> Template {
        let sample_path = dir.join("sample.mp4");
        std::fs::write(&sample_path, b"not really an mp4, but bytes travel").unwrap();

        let mut project = Project::new("Bundled", R24);
        let media = project.add_media(MediaSource::new(&sample_path, 1920, 1080, R24, 240, true));
        let track = project.add_track(TrackKind::Video, "V1");
        let slot = project
            .add_clip(
                track,
                media,
                TimeRange::at_rate(0, 24, R24),
                RationalTime::new(0, R24),
            )
            .unwrap();
        project
            .set_replaceable(slot, Some(Replaceable::new(0)))
            .unwrap();
        Template::from_project(project, TemplateMeta::new("Bundled"))
    }

    #[test]
    fn bundle_roundtrips_and_installs_with_absolute_paths() {
        let dir = tempfile::tempdir().unwrap();
        let template = template_with_real_media(dir.path());
        let bundle = dir.path().join("bundled.ariab");
        write(&template, &bundle).unwrap();

        let manifest = read_manifest(&bundle).unwrap();
        assert_eq!(manifest.name, "Bundled");
        assert_eq!(manifest.min_schema_version, PROJECT_SCHEMA_VERSION);

        let install_dir = dir.path().join("installed/bundled");
        let installed = install(&bundle, &install_dir).unwrap();
        assert_eq!(installed.slot_count(), 1);
        for media in installed.project().media_iter() {
            assert!(media.path.is_absolute(), "rewritten: {:?}", media.path);
            assert!(media.path.is_file(), "extracted: {:?}", media.path);
            assert!(media.path.starts_with(&install_dir));
        }

        // Install once, open forever: the rewritten file loads directly.
        let reloaded = Template::load_from_file(&install_dir.join("template.ariat")).unwrap();
        assert_eq!(reloaded.meta().name, "Bundled");
        assert!(reloaded.project().media_iter().all(|m| m.path.is_file()));
    }

    /// WHY: an install into a relative folder kept the extracted media paths
    /// relative to the working directory, so the installed `template.ariat`
    /// and every project filled from it lost their media once the working
    /// directory changed. The installed template, and its file on disk, hold
    /// absolute paths for a relative folder too. Not caught: a folder reached
    /// through a symbolic link that later moves.
    #[test]
    fn an_install_into_a_relative_folder_holds_absolute_media_paths() {
        let dir = tempfile::tempdir().unwrap();
        let template = template_with_real_media(dir.path());
        let bundle = dir.path().join("bundled.ariab");
        write(&template, &bundle).unwrap();

        let cwd = std::env::current_dir().unwrap();
        let up: PathBuf = cwd
            .components()
            .skip(1)
            .map(|_| Component::ParentDir)
            .collect();
        let target = dir.path().join("installed");
        let relative = up.join(target.strip_prefix("/").unwrap());
        assert!(relative.is_relative(), "{relative:?}");

        let installed = install(&bundle, &relative).unwrap();
        let on_disk = Template::load_from_file(&target.join("template.ariat")).unwrap();
        for (what, template) in [("installed", &installed), ("on disk", &on_disk)] {
            assert_eq!(template.project().media_iter().count(), 1);
            for media in template.project().media_iter() {
                assert!(media.path.is_absolute(), "{what}: {:?}", media.path);
                assert!(media.path.is_file(), "{what}: {:?}", media.path);
            }
        }
    }

    /// Build a tar archive at `path` holding `entries` in order.
    fn tar_of(path: &Path, entries: &[(&str, &[u8])]) {
        let mut builder = tar::Builder::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            append_bytes(&mut builder, name, bytes).unwrap();
        }
        builder.into_inner().unwrap();
    }

    fn manifest_json(format_version: u32, min_schema_version: u32) -> Vec<u8> {
        let name = "Bundled".into();
        serde_json::to_vec(&BundleManifest {
            format_version,
            name,
            min_schema_version,
        })
        .unwrap()
    }

    /// WHY: every bundle failure read "Couldn't open this project: the
    /// project file is damaged or corrupted (...). Pick another .aria file.",
    /// writing one included, and a bundle with an entry outside its layout
    /// had the entries before that one extracted. Each refusal states the
    /// bundle, what it met and the fix, and a refused install writes nothing.
    /// Not caught: a refusal this table has no case for.
    #[test]
    fn every_bundle_refusal_states_the_bundle_and_its_fix() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str| dir.path().join(name);
        let mut project = Project::new("Broken", R24);
        let gone = "/definitely/not/here.mp4";
        project.add_media(MediaSource::new(gone, 1920, 1080, R24, 240, true));
        let broken = Template::from_project(project, TemplateMeta::new("Broken"));
        write(&template_with_real_media(dir.path()), &at("good.ariab")).unwrap();
        std::fs::write(at("taken"), b"a file").unwrap();
        std::fs::write(at("text.ariab"), b"not an archive").unwrap();
        tar_of(&at("plain.ariab"), &[("readme.txt", &b"hello"[..])]);
        tar_of(&at("garbled.ariab"), &[("bundle.json", &b"{"[..])]);
        let (format, schema) = (BUNDLE_FORMAT_VERSION, PROJECT_SCHEMA_VERSION);
        let manifests = [
            manifest_json(format + 1, 1),
            manifest_json(format, schema + 1),
            manifest_json(format, 1),
        ];
        tar_of(&at("format.ariab"), &[("bundle.json", &manifests[0][..])]);
        tar_of(&at("schema.ariab"), &[("bundle.json", &manifests[1][..])]);
        let evil = [
            ("bundle.json", &manifests[2][..]),
            ("media/0.mp4", &b"bytes"[..]),
            ("not-media/escape.txt", &b"pwned"[..]),
        ];
        tar_of(&at("evil.ariab"), &evil);

        let state = |verb: &str, name: &str, reason: &str, fix: &str| {
            let path = at(name);
            format!(
                "Couldn't {verb} the template bundle {}: {reason}. {fix}",
                path.display()
            )
        };
        let open = |name: &str, reason: &str| state("open", name, reason, PICK_ANOTHER);
        let newer = |name: &str, what: &str, found: u32, read: u32| {
            let reason =
                format!("it uses {what} version {found}, which is newer than this build reads");
            let reason = format!("{reason} ({read})");
            state("install", name, &reason, "Update Aria to install it.")
        };
        let installed = at("installed");
        let install_into = |name: &str, dest: &Path| match install(&at(name), dest) {
            Ok(_) => panic!("{name} installed"),
            Err(err) => err.to_string(),
        };
        let mut cases = vec![
            (
                write(&broken, &at("broken.ariab")).unwrap_err().to_string(),
                state(
                    "write",
                    "broken.ariab",
                    &format!("the sample media {gone} is missing"),
                    "Relink it, then write the bundle again.",
                ),
            ),
            (
                read_manifest(&at("absent.ariab")).unwrap_err().to_string(),
                open("absent.ariab", "no such file"),
            ),
            (
                read_manifest(&at("plain.ariab")).unwrap_err().to_string(),
                open(
                    "plain.ariab",
                    "it is not a template bundle (it has no bundle.json)",
                ),
            ),
            (
                install_into("format.ariab", &installed),
                newer("format.ariab", "bundle format", format + 1, format),
            ),
            (
                install_into("schema.ariab", &installed),
                newer("schema.ariab", "project format", schema + 1, schema),
            ),
            (
                install_into("evil.ariab", &installed),
                state(
                    "install",
                    "evil.ariab",
                    "it holds \"not-media/escape.txt\", a path outside the bundle layout",
                    PICK_ANOTHER,
                ),
            ),
        ];
        // A destination that is a file: `create_dir_all` fails with EEXIST.
        #[cfg(unix)]
        cases.push((
            install_into("good.ariab", &at("taken")),
            state(
                "install",
                "good.ariab",
                &format!(
                    "cannot write in \"{}\": {}",
                    at("taken").display(),
                    io_reason(&std::io::Error::from_raw_os_error(17))
                ),
                CHECK_THE_FOLDER,
            ),
        ));
        for (got, want) in cases {
            assert_eq!(got, want);
        }
        for (name, reason) in [
            ("text.ariab", "it is not a template bundle ("),
            (
                "garbled.ariab",
                "its bundle.json is not a bundle manifest (",
            ),
        ] {
            let got = read_manifest(&at(name)).unwrap_err().to_string();
            let head = state("open", name, reason, "");
            let head = head.trim_end_matches(". ");
            assert!(got.starts_with(head), "{got}");
            assert!(got.ends_with("). Pick another .ariab file."), "{got}");
        }
        assert!(!installed.exists(), "a refused install wrote files");
    }

    /// A bundle written over its own sample media fails and leaves the media
    /// as it was; written in place, the archive truncated the footage it was
    /// about to read.
    ///
    /// The media is read-only, so a writer that opens it in place fails at
    /// once instead of archiving a file it is growing, which never ends and
    /// fills the disk. Not bounded when the tests run with the privilege to
    /// write a read-only file.
    #[test]
    fn write_over_sample_media_fails_and_keeps_the_media() {
        let dir = tempfile::tempdir().unwrap();
        let template = template_with_real_media(dir.path());
        let sample = dir.path().join("sample.mp4");
        let before = std::fs::read(&sample).unwrap();
        let mut read_only = std::fs::metadata(&sample).unwrap().permissions();
        read_only.set_readonly(true);
        std::fs::set_permissions(&sample, read_only).unwrap();

        let err = write(&template, &sample).unwrap_err();

        assert_eq!(
            err.to_string(),
            format!(
                "Couldn't write the template bundle {}: it is media this bundle uses. Write the \
                 bundle to another file.",
                sample.display()
            )
        );
        assert_eq!(std::fs::read(&sample).unwrap(), before);
    }

    /// Writing a bundle replaces the file: a hard link to the previous bundle
    /// keeps its bytes, and the new one reads back whole.
    #[test]
    fn write_replaces_an_existing_bundle_instead_of_rewriting_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut template = template_with_real_media(dir.path());
        let bundle = dir.path().join("bundled.ariab");
        write(&template, &bundle).unwrap();
        let old = std::fs::read(&bundle).unwrap();
        let witness = dir.path().join("witness.ariab");
        std::fs::hard_link(&bundle, &witness).unwrap();

        template.meta.name = "Renamed".into();
        write(&template, &bundle).unwrap();

        assert_eq!(std::fs::read(&witness).unwrap(), old);
        assert_eq!(read_manifest(&bundle).unwrap().name, "Renamed");
    }

    #[test]
    fn entry_name_validator_rejects_traversal_and_absolutes() {
        assert!(safe_entry_name(Path::new("bundle.json")));
        assert!(safe_entry_name(Path::new("template.ariat")));
        assert!(safe_entry_name(Path::new("media/0.mp4")));
        assert!(safe_entry_name(Path::new("media/sub/1.jpg")));
        assert!(!safe_entry_name(Path::new("media/../escape.txt")));
        assert!(!safe_entry_name(Path::new("../escape.txt")));
        assert!(!safe_entry_name(Path::new("/etc/passwd")));
        assert!(!safe_entry_name(Path::new("other.txt")));
    }
}
