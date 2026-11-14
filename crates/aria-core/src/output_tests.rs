//! WHY: a writer's name check suggested the name it was given back to it:
//! `aria export cut.aria ""` printed `name the output "" rather than ""`,
//! and the Python `export(".")` printed `name the output "." rather than
//! "."`. The class is a refusal whose suggestion the same check refuses.
//! Not caught: a suggestion in a folder that cannot be written, which the
//! writer states when it opens the file.

use super::*;

const EXTENSIONS: [&str; 4] = ["mp4", "png", "aria", "ariaplugin"];

/// Every name a writer is given and refuses with a suggestion is followed
/// by a suggestion the check accepts, and every refusal without one states
/// why no name of that form can be a file.
#[test]
fn a_suggested_name_is_one_the_check_accepts() {
    let root = tempfile::tempdir().expect("scratch");
    let at = |name: &str| root.path().join(name).to_string_lossy().into_owned();
    std::fs::create_dir(at("folder.mp4")).expect("folder");
    let mut given: Vec<String> = [
        "", ".", "..", "/", "cut", "cut.", "cut.json", ".mp4", "v1.2/cut",
    ]
    .map(str::to_owned)
    .into();
    given.extend(
        [
            "",
            "cut",
            "cut.webm",
            "folder.mp4",
            "missing/..",
            "missing/",
        ]
        .map(at),
    );
    for extension in EXTENSIONS {
        for out in &given {
            let Some(problem) = output_name_problem("export", out, extension) else {
                panic!("{out:?} passed as a .{extension} file");
            };
            let Some((_, named)) = problem.split_once("name the output \"") else {
                assert!(
                    problem.ends_with("names no file")
                        || problem.ends_with("; name a file to write"),
                    "{out:?}: {problem}"
                );
                continue;
            };
            let suggested = named
                .strip_suffix(&format!("\" rather than \"{out}\""))
                .unwrap_or_else(|| panic!("{out:?}: {problem}"));
            assert_eq!(
                output_name_problem("export", suggested, extension),
                None,
                "{out:?} as .{extension} suggested {suggested:?}"
            );
        }
    }
}

/// Each problem in the words the CLI and the binding print.
#[test]
fn each_problem_is_stated_in_its_own_words() {
    let root = tempfile::tempdir().expect("scratch");
    let at = |name: &str| root.path().join(name).to_string_lossy().into_owned();
    std::fs::create_dir(at("folder.mp4")).expect("folder");
    std::fs::write(at("cut.mp4"), b"").expect("file");
    let mut rows = vec![
        (
            String::new(),
            "export writes a .mp4 file, and \"\" names no file".to_owned(),
        ),
        (
            at("missing/.."),
            format!(
                "export writes a .mp4 file, and \"{}\" names no file",
                at("missing/..")
            ),
        ),
        (
            ".".to_owned(),
            "\".\" is a directory; name a file to write".to_owned(),
        ),
        (
            "/".to_owned(),
            "\"/\" is a directory; name a file to write".to_owned(),
        ),
        (
            at("folder.mp4"),
            format!(
                "\"{}\" is a directory; name a file to write",
                at("folder.mp4")
            ),
        ),
        (
            "cut".to_owned(),
            "export writes a .mp4 file; name the output \"cut.mp4\" rather than \"cut\"".to_owned(),
        ),
        (
            "cut.json".to_owned(),
            "export writes a .mp4 file; name the output \"cut.mp4\" rather than \"cut.json\""
                .to_owned(),
        ),
        (
            "missing/".to_owned(),
            "export writes a .mp4 file; name the output \"missing.mp4\" rather than \"missing/\""
                .to_owned(),
        ),
    ];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(at("cut.mp4"), at("link.mp4")).expect("symlink");
        rows.push((
            at("link.mp4"),
            format!(
                "\"{}\" is a symbolic link; name a file to write",
                at("link.mp4")
            ),
        ));
        if std::path::Path::new("/dev/null").exists() {
            rows.push((
                "/dev/null".to_owned(),
                "\"/dev/null\" is a special file; name a file to write".to_owned(),
            ));
        }
    }
    for (out, stated) in rows {
        assert_eq!(
            output_name_problem("export", &out, "mp4"),
            Some(stated),
            "{out:?}"
        );
    }
    for out in [
        at("cut.mp4"),
        at("cut.MP4"),
        "v1.2/cut.mp4".to_owned(),
        at("new.mp4"),
    ] {
        assert_eq!(output_name_problem("export", &out, "mp4"), None, "{out:?}");
    }
}
