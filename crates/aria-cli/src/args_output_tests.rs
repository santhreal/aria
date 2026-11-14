//! What a verb's output name may be: the extension of the one format the verb
//! writes, and a file in a directory that exists.

use super::*;

/// WHY: `export p.aria out.webm` wrote MP4 into `out.webm`, and `frame p.aria
/// still.jpg` wrote a PNG named `.jpg`: each verb writes one format whatever
/// the output is named. Every verb in `Verb::ALL` that writes a file refuses
/// a name with no extension or with another verb's, and accepts its own in
/// either case, with any dots in the directory; a default output name is in
/// the verb's format. `Verb::writes` is an exhaustive match, so a new verb
/// does not compile until it states what it writes.
///
/// Not caught: `plugin pack --out`, whose archive name is a flag value.
#[test]
fn a_verb_refuses_an_output_named_for_another_format() {
    let writers: Vec<(Verb, usize, &str)> = Verb::ALL
        .into_iter()
        .filter_map(|verb| {
            verb.writes()
                .map(|(index, extension)| (verb, index, extension))
        })
        .collect();
    assert!(writers.len() >= 4, "only {writers:?} write a file");
    let line = |verb: Verb, index: usize, out: &str| {
        let mut words = vec![verb.key().to_string()];
        words.extend((0..index).map(|_| "p.aria".to_string()));
        words.push(out.to_string());
        Args::parse(words).expect("parse")
    };
    for &(verb, index, extension) in &writers {
        let others = writers
            .iter()
            .map(|(_, _, other)| *other)
            .filter(|other| *other != extension)
            .chain(["webm"]);
        for wrong in std::iter::once(String::new()).chain(others.map(|other| format!(".{other}"))) {
            let out = format!("cut{wrong}");
            let err = line(verb, index, &out)
                .reject_misnamed_output()
                .expect_err("an output named for another format was accepted");
            assert_eq!(
                err,
                ArgsError::Usage(format!(
                    "{} writes a .{extension} file; name the output \"cut.{extension}\" rather than \"{out}\"",
                    verb.key()
                )),
                "{} {out}",
                verb.key()
            );
        }
        // An empty name has no extension to replace: the refusal states it
        // names no file rather than suggesting "" back.
        assert_eq!(
            line(verb, index, "").reject_misnamed_output(),
            Err(ArgsError::Usage(format!(
                "{} writes a .{extension} file, and \"\" names no file",
                verb.key()
            ))),
            "{} with an empty output",
            verb.key()
        );
        for right in [
            format!("cut.{extension}"),
            format!("cut.{}", extension.to_uppercase()),
            format!("v1.2/cut.{extension}"),
        ] {
            assert_eq!(
                line(verb, index, &right).reject_misnamed_output(),
                Ok(()),
                "{} {right}",
                verb.key()
            );
        }
    }
    for (verb, default) in [
        (Verb::Frame, crate::frame::DEFAULT_OUT),
        (Verb::Demo, crate::demo::DEFAULT_OUT),
    ] {
        let (_, extension) = verb.writes().expect("the verb writes a file");
        assert_eq!(
            std::path::Path::new(default).extension(),
            Some(std::ffi::OsStr::new(extension)),
            "{} writes {default} by default",
            verb.key()
        );
    }
}

/// WHY: `aria frame p.aria nodir/still.png` rendered and then failed with
/// "i/o error: No such file or directory (os error 2)", naming no path, and
/// an output that is a directory or a symbolic link failed as an i/o error
/// with exit code 1. Every verb in `Verb::ALL` that writes a file refuses an
/// output that is not a file, or whose directory is missing or is a file, and
/// accepts a file, a new name in a directory that exists, or a bare name.
///
/// Not caught: a directory that exists but cannot be written; the write
/// states that directory.
#[test]
fn a_verb_refuses_an_output_it_cannot_write_as_a_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("plain");
    std::fs::write(&file, b"a file").expect("write a file");
    let mut not_files = vec![(dir.path().join("folder.png"), "a directory")];
    std::fs::create_dir(&not_files[0].0).expect("make a directory");
    #[cfg(unix)]
    {
        let link = dir.path().join("link.mp4");
        std::os::unix::fs::symlink(&file, &link).expect("link to a file");
        not_files.push((link, "a symbolic link"));
    }
    let mut writers = 0;
    for verb in Verb::ALL {
        let Some((index, extension)) = verb.writes() else {
            continue;
        };
        writers += 1;
        let line = |out: &std::path::Path| {
            let mut words = vec![verb.key().to_string()];
            words.extend((0..index).map(|_| "p.aria".to_string()));
            words.push(out.display().to_string());
            Args::parse(words).expect("parse")
        };
        let name = format!("cut.{extension}");
        for missing in [dir.path().join("nodir"), file.clone()] {
            let out = missing.join(&name);
            assert_eq!(
                line(&out).reject_unwritable_output(),
                Err(ArgsError::Usage(format!(
                    "there is no directory \"{}\" to write \"{}\" in",
                    missing.display(),
                    out.display()
                ))),
                "{} {}",
                verb.key(),
                out.display()
            );
        }
        for (out, what) in &not_files {
            assert_eq!(
                line(out).reject_unwritable_output(),
                Err(ArgsError::Usage(format!(
                    "\"{}\" is {what}; name a file to write",
                    out.display()
                )))
            );
        }
        for out in [dir.path().join(&name), name.clone().into(), file.clone()] {
            assert_eq!(
                line(&out).reject_unwritable_output(),
                Ok(()),
                "{} {}",
                verb.key(),
                out.display()
            );
        }
    }
    assert!(writers >= 4, "only {writers} verbs write a file");
}
