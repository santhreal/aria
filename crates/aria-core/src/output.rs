//! The checks on the name of a file a command is about to write, run before
//! any work: the CLI's writers and the Python binding's writers state the
//! same problem in the same words.

use std::path::Path;

/// Why `out` cannot be written as a new file, or `None` when nothing is in
/// its place: a directory, a symbolic link, or a special file at `out` is
/// stated as what it is.
pub fn not_a_file(out: &str) -> Option<String> {
    let meta = std::fs::symlink_metadata(out).ok()?;
    let what = if meta.is_file() {
        return None;
    } else if meta.is_dir() {
        "a directory"
    } else if meta.is_symlink() {
        "a symbolic link"
    } else {
        "a special file"
    };
    Some(format!("\"{out}\" is {what}; name a file to write"))
}

/// Why `out` cannot name the `.{extension}` file that `verb` writes, or
/// `None` when it can. The writer writes one format whatever the name says,
/// so a name for another format states the name to use. A path that names
/// no file (empty, `.`, `..`, or a root) and anything [`not_a_file`] states
/// are stated before the extension, since no extension makes either one a
/// file.
pub fn output_name_problem(verb: &str, out: &str, extension: &str) -> Option<String> {
    if let Some(problem) = not_a_file(out) {
        return Some(problem);
    }
    let path = Path::new(out);
    if path.file_name().is_none() {
        return Some(format!(
            "{verb} writes a .{extension} file, and \"{out}\" names no file"
        ));
    }
    if path
        .extension()
        .is_some_and(|named| named.eq_ignore_ascii_case(extension))
    {
        return None;
    }
    Some(format!(
        "{verb} writes a .{extension} file; name the output \"{}\" rather than \"{out}\"",
        path.with_extension(extension).display()
    ))
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
