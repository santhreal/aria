//! A count the command line prints agrees with its noun: `1 frame`, `3 frames`.
//!
//! WHY: `aria export --at 9 --to 9.04` printed `wrote 1 frames`, `inspect` of a
//! one-tick sequence `(1 ticks)`, and `analyze` of a file one loudness window
//! long `1 windows`: each wrote a count beside a fixed plural instead of through
//! `aria_core::count`. This reads the binary's source on each run and fails on
//! a placeholder followed by a plural that a `counted` call anywhere under
//! `crates/` or `apps/` writes, outside test code.
//!
//! Not caught: a plural no `counted` call writes; a count and its noun in two
//! placeholders, such as `"{} {}"`; and test code, which is a file named for
//! tests or an inline test module.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, skipping build output and hidden directories.
pub(super) fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

/// Add the plural of every `counted(count, one, many)` call in `text`: the
/// last two string literals among the call's own arguments, when they differ.
fn plurals_counted(text: &str, into: &mut BTreeSet<String>) {
    for (at, _) in text.match_indices("counted(") {
        let mut depth = 0_usize;
        let mut literals = Vec::new();
        let mut chars = text[at + "counted".len()..].chars();
        while let Some(c) = chars.next() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                '"' => {
                    let mut literal = String::new();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => {
                                chars.next();
                            }
                            '"' => break,
                            c => literal.push(c),
                        }
                    }
                    if depth == 1 {
                        literals.push(literal);
                    }
                }
                _ => {}
            }
        }
        if let [.., one, many] = literals.as_slice()
            && one != many
        {
            into.insert(many.clone());
        }
    }
}

/// `text` up to its inline test module: a `#[cfg(test)]` line whose next line
/// that is not an attribute opens a `mod` block.
pub(super) fn outside_tests(text: &str) -> &str {
    let lines: Vec<(usize, &str)> = text
        .split_inclusive('\n')
        .scan(0, |at, line| {
            let start = *at;
            *at += line.len();
            Some((start, line.trim()))
        })
        .collect();
    for (index, &(start, line)) in lines.iter().enumerate() {
        if line != "#[cfg(test)]" {
            continue;
        }
        let item = lines[index + 1..]
            .iter()
            .map(|&(_, line)| line)
            .find(|line| !line.starts_with("#["));
        if item.is_some_and(|item| item.contains("mod ") && item.ends_with('{')) {
            return &text[..start];
        }
    }
    text
}

/// Each placeholder in `code` followed by a space and a word in `plurals`: the
/// line it is on, counted from 1, and the word. Comment lines are skipped, and
/// an escaped `{{` opens no placeholder.
fn bare_counts(code: &str, plurals: &BTreeSet<String>) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in code.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for (close, _) in line.match_indices("} ") {
            let Some(open) = line[..close].rfind('{') else {
                continue;
            };
            let inner = &line[open + 1..close];
            if line[..open].ends_with('{')
                || inner.contains(|c: char| c.is_whitespace() || c == '"')
            {
                continue;
            }
            let rest = &line[close + 2..];
            for plural in plurals {
                if rest.starts_with(plural.as_str())
                    && !rest[plural.len()..].starts_with(char::is_alphanumeric)
                {
                    found.push((index + 1, plural.clone()));
                }
            }
        }
    }
    found
}

#[test]
fn every_count_the_command_line_prints_agrees_with_its_noun() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = crate_dir.join("../..");
    let mut everywhere = Vec::new();
    for dir in ["crates", "apps"] {
        rust_files(&root.join(dir), &mut everywhere);
    }
    let mut plurals = BTreeSet::new();
    for file in &everywhere {
        if let Ok(text) = fs::read_to_string(file) {
            plurals_counted(&text, &mut plurals);
        }
    }
    assert!(
        ["frames", "markers", "clips"]
            .iter()
            .all(|plural| plurals.contains(*plural)),
        "the counted calls read gave {plurals:?}"
    );

    let src = crate_dir.join("src");
    let mut ours = Vec::new();
    rust_files(&src, &mut ours);
    ours.retain(|file| {
        !file
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("test"))
    });
    assert!(
        ours.iter().any(|file| file.ends_with("export.rs")),
        "no source read under {}",
        src.display()
    );
    let mut bare = Vec::new();
    for file in &ours {
        let text = fs::read_to_string(file).expect("read source");
        for (line, plural) in bare_counts(outside_tests(&text), &plurals) {
            let shown = file.strip_prefix(&src).unwrap_or(file).display();
            bare.push(format!("{shown}:{line} writes a count before \"{plural}\""));
        }
    }
    assert!(
        bare.is_empty(),
        "write these through aria_core::count::counted:\n{}",
        bare.join("\n")
    );
}

/// The matchers at their boundaries: each placeholder form, an escaped brace, a
/// longer word, a singular, a comment, a multi-word plural; a call over lines
/// with a nested call and a trailing comma, and one whose words are the same;
/// and a test module against a test-only item.
#[test]
fn the_matchers_find_a_count_beside_a_plural_and_nothing_else() {
    let plurals: BTreeSet<String> = ["frames", "loudness windows"]
        .iter()
        .map(|plural| (*plural).to_owned())
        .collect();
    let lines = [
        ("\"wrote {} frames\"", true),
        ("\"wrote {count} frames of\"", true),
        ("\"{:>5} frames\"", true),
        ("\"{n} loudness windows\"", true),
        ("\"{{}} frames\"", false),
        ("\"{} framesets\"", false),
        ("\"{} frame\"", false),
        ("\"{a b} frames\"", false),
        ("if x { a } frames", false),
        ("// \"{} frames\"", false),
    ];
    for (line, hit) in lines {
        assert_eq!(!bare_counts(line, &plurals).is_empty(), hit, "{line}");
    }

    let mut counted = BTreeSet::new();
    plurals_counted(
        "counted(\n    list.len(),\n    \"clip\",\n    \"clips\",\n)\ncounted(n, \"sheep\", \"sheep\")",
        &mut counted,
    );
    assert_eq!(counted, BTreeSet::from(["clips".to_owned()]));

    let module = "fn a() {}\n#[cfg(test)]\nmod tests {\n    \"{} frames\"\n}\n";
    assert_eq!(outside_tests(module), "fn a() {}\n");
    let item = "#[cfg(test)]\nconst A: u8 = 1;\nfn b() {}\n";
    assert_eq!(outside_tests(item), item);
}
