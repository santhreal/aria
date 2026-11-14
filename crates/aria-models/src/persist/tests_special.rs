//! Opening an Aria document path that names something other than a regular
//! file.

use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::{Template, template_bundle};

/// One reader of an Aria document file: how it reads `path` (with `scratch`
/// as a folder it may write to), and the refusal it states for `path` being
/// `what`.
struct Reader {
    name: &'static str,
    read: fn(&Path, &Path) -> Result<(), ModelError>,
    refusal: fn(&Path, &str) -> String,
}

/// Every entry point that reads a project, template or bundle file.
fn readers() -> [Reader; 4] {
    let bundle = |path: &Path, what: &str| {
        format!(
            "Couldn't open the template bundle {}: it is {what}. Pick another .ariab file.",
            path.display()
        )
    };
    [
        Reader {
            name: "Project::load_from_file",
            read: |path, _| Project::load_from_file(path).map(drop),
            refusal: |path, what| {
                format!(
                    "Couldn't open {}: it is not an Aria project file ({what}). \
                     Pick a valid .aria project file.",
                    path.display()
                )
            },
        },
        Reader {
            name: "Template::load_from_file",
            read: |path, _| Template::load_from_file(path).map(drop),
            refusal: |path, what| {
                format!(
                    "Couldn't open {}: it is not an Aria template file ({what}). \
                     Pick a valid .ariat template file.",
                    path.display()
                )
            },
        },
        Reader {
            name: "template_bundle::read_manifest",
            read: |path, _| template_bundle::read_manifest(path).map(drop),
            refusal: bundle,
        },
        Reader {
            name: "template_bundle::install",
            read: |path, scratch| {
                template_bundle::install(path, &scratch.join("installed")).map(drop)
            },
            refusal: bundle,
        },
    ]
}

/// Run `read` on its own thread and fail if it has not returned in 10 s: a
/// read that waits forever fails the test instead of hanging the suite.
fn bounded(reader: &Reader, path: &Path, scratch: &Path) -> Result<(), ModelError> {
    let (done, result) = mpsc::channel();
    let (read, path_owned, scratch_owned) =
        (reader.read, path.to_path_buf(), scratch.to_path_buf());
    std::thread::spawn(move || {
        let _ = done.send(read(&path_owned, &scratch_owned));
    });
    result
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| {
            panic!(
                "{} of {} did not return within 10 s",
                reader.name,
                path.display()
            )
        })
}

/// WHY: a project, template or bundle path that named a pipe made every
/// reader wait forever for a writer (`aria inspect`, `aria.Project.load` and
/// the editor's Open hung), and one that named a device such as `/dev/zero`
/// read until memory ran out and then reported "out of memory. Check the
/// file permissions". A bundle that was a folder reported a tar error. Each
/// reader now states what the path is before it reads a byte. Covers a pipe,
/// a symbolic link to a pipe, a character device and a folder for every
/// reader in [`readers`], and that a regular file and a missing path are not
/// stated as special. Not caught: a path that turns into a pipe between the
/// check and the read.
#[test]
fn every_document_reader_states_a_path_that_is_not_a_file_before_reading_it() {
    let dir = tempfile::tempdir().unwrap();
    // The read end of a pipe whose write end stays open, named through
    // `/dev/fd`: a read waits for a writer forever, as on a named pipe, and
    // no `mkfifo` process starts.
    use std::os::fd::AsRawFd;
    let (pipe, _writer) = std::io::pipe().expect("a pipe");
    let fifo = std::path::PathBuf::from(format!("/dev/fd/{}", pipe.as_raw_fd()));
    let link = dir.path().join("link.aria");
    std::os::unix::fs::symlink(&fifo, &link).unwrap();
    let folder = dir.path().join("folder.aria");
    std::fs::create_dir(&folder).unwrap();
    let device = Path::new("/dev/null");

    let pipe = "a pipe, socket or device";
    let cases = [
        (fifo.as_path(), pipe),
        (link.as_path(), pipe),
        (device, pipe),
        (folder.as_path(), "a directory"),
    ];
    for reader in readers() {
        for (path, what) in cases {
            let scratch = tempfile::tempdir().unwrap();
            let err = bounded(&reader, path, scratch.path())
                .expect_err("a path that is not a file is refused");
            assert_eq!(
                err.to_string(),
                (reader.refusal)(path, what),
                "{} of {}",
                reader.name,
                path.display()
            );
            assert_eq!(
                std::fs::read_dir(scratch.path()).unwrap().count(),
                0,
                "{} of {} wrote nothing",
                reader.name,
                path.display()
            );
        }
    }

    let regular = dir.path().join("regular.aria");
    Project::new("regular", crate::time::Rational::FPS_24)
        .save_to_file(&regular)
        .unwrap();
    assert_eq!(special_file(&regular), None);
    assert_eq!(special_file(&dir.path().join("missing.aria")), None);
    Project::load_from_file(&regular).expect("a regular project file opens");
}

/// WHY: a template bundle opened as a project or a template read "not JSON",
/// and a project or a template installed as a bundle quoted the tar reader's
/// header error, which holds the first bytes of the document across several
/// lines. Each reader in [`readers`] opens its own kind of document and
/// states a document of another kind in one line: a bundle as "a template
/// bundle", a project or a template as "not a tar archive". Not caught: a tar
/// archive that is not a bundle, which the bundle readers state by the
/// `bundle.json` it lacks.
#[test]
fn every_document_reader_states_a_document_of_another_kind() {
    let dir = tempfile::tempdir().unwrap();
    let model = Project::new("cut", crate::time::Rational::FPS_24);
    let project = dir.path().join("cut.aria");
    model.save_to_file(&project).unwrap();
    let template = Template::from_project(model, crate::TemplateMeta::new("cut"));
    let template_file = dir.path().join("cut.ariat");
    template.save_to_file(&template_file).unwrap();
    let bundle = dir.path().join("cut.ariab");
    template_bundle::write(&template, &bundle).unwrap();

    let documents = [
        ("project", project.as_path()),
        ("template", template_file.as_path()),
        ("bundle", bundle.as_path()),
    ];
    for reader in readers() {
        let own = match reader.name.split("::").next() {
            Some("Project") => "project",
            Some("Template") => "template",
            Some("template_bundle") => "bundle",
            _ => panic!("{} reads no kind of document this test knows", reader.name),
        };
        for (kind, path) in documents {
            let scratch = tempfile::tempdir().unwrap();
            let outcome = bounded(&reader, path, scratch.path());
            let read = format!("{} of a {kind}", reader.name);
            if kind == own {
                outcome.unwrap_or_else(|err| panic!("{read} failed: {err}"));
                continue;
            }
            let message = outcome.expect_err(&read).to_string();
            assert!(!message.contains('\n'), "{read} spans lines: {message}");
            let stated = match (own, kind) {
                (_, "bundle") => Some("a template bundle"),
                ("bundle", _) => Some("not a template bundle (not a tar archive)"),
                _ => None,
            };
            if let Some(what) = stated {
                assert_eq!(message, (reader.refusal)(path, what), "{read}");
            }
            assert_eq!(
                std::fs::read_dir(scratch.path()).unwrap().count(),
                0,
                "{read} wrote nothing"
            );
        }
    }
}
