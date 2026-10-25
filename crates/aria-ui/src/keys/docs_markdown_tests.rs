//! Markdown checks over every file under `docs/`: relative links and
//! `#anchor` fragments resolve, fenced code blocks close, tables end at a
//! blank line, and prose stays in the documentation register, is spelled in
//! US English, and counts no catalog members.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::test_support::us_spelling::british_words;

pub(super) fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"));
    let mut paths: Vec<PathBuf> = entries.map(|e| e.unwrap().path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            collect_md_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn gfm_slug(heading: &str) -> String {
    let mut stripped = String::new();
    let mut chars = heading.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '`' {
            continue;
        }
        if ch == '[' {
            let mut label = String::new();
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
                label.push(c);
            }
            if chars.peek() == Some(&'(') {
                chars.next();
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            stripped.push_str(&label);
            continue;
        }
        stripped.push(ch);
    }
    let mut slug = String::new();
    for ch in stripped.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            slug.push(ch);
        } else if ch == ' ' {
            slug.push('-');
        }
    }
    slug
}

fn markdown_headings(md_path: &Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(md_path)
        .unwrap_or_else(|e| panic!("failed to read {md_path:?}: {e}"));
    let mut in_fence = false;
    let mut slugs = BTreeSet::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) {
            let after = &trimmed[hashes..];
            if let Some(title) = after.strip_prefix(' ') {
                let base = gfm_slug(title);
                let count = counts.entry(base.clone()).or_insert(0);
                let slug = if *count == 0 {
                    base
                } else {
                    format!("{base}-{count}")
                };
                *count += 1;
                slugs.insert(slug);
            }
        }
    }
    slugs
}

pub(super) fn strip_inline_code(line: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for ch in line.chars() {
        if ch == '`' {
            in_code = !in_code;
        } else if !in_code {
            out.push(ch);
        }
    }
    out
}

pub(super) fn extract_markdown_links(line: &str) -> Vec<String> {
    let cleaned = strip_inline_code(line);
    let bytes = cleaned.as_bytes();
    let mut links = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b']' {
                j += 1;
            }
            if j + 1 < bytes.len() && bytes[j] == b']' && bytes[j + 1] == b'(' {
                let mut k = j + 2;
                while k < bytes.len() && bytes[k] != b')' {
                    k += 1;
                }
                if k < bytes.len() && bytes[k] == b')' {
                    let target = cleaned[j + 2..k].trim();
                    if !target.is_empty() {
                        links.push(target.to_string());
                    }
                    i = k + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    links
}

pub(super) fn check_markdown_links_in_files(md_files: &[PathBuf]) -> (usize, Vec<String>) {
    let mut heading_cache: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut broken: Vec<String> = Vec::new();
    let mut checked_links = 0usize;

    for md_file in md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let mut in_fence = false;
        for (idx, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                continue;
            }
            if in_fence {
                continue;
            }
            for target in extract_markdown_links(line) {
                if target.starts_with("http://")
                    || target.starts_with("https://")
                    || target.starts_with("mailto:")
                {
                    continue;
                }
                checked_links += 1;
                let (path_part, anchor) = match target.split_once('#') {
                    Some((p, a)) => (p, Some(a)),
                    None => (target.as_str(), None),
                };
                let target_path = if path_part.is_empty() {
                    md_file.clone()
                } else {
                    md_file.parent().unwrap().join(path_part)
                };
                let Ok(canonical) = std::fs::canonicalize(&target_path) else {
                    broken.push(format!(
                        "{}:{}: broken link `{target}` (target file does not exist)",
                        md_file.display(),
                        idx + 1
                    ));
                    continue;
                };
                if let Some(anchor_slug) = anchor
                    && canonical.extension().and_then(|s| s.to_str()) == Some("md")
                {
                    let slugs = heading_cache
                        .entry(canonical.clone())
                        .or_insert_with(|| markdown_headings(&canonical));
                    if !slugs.contains(anchor_slug) {
                        broken.push(format!(
                            "{}:{}: broken anchor `#{anchor_slug}` in `{target}`",
                            md_file.display(),
                            idx + 1
                        ));
                    }
                }
            }
        }
    }

    (checked_links, broken)
}

/// Closes the class where a documentation edit introduces a broken relative
/// file link or a broken `#anchor` heading fragment anywhere under `docs/`.
/// Does not check reachability of external `http://` or `https://` URLs.
#[test]
fn all_markdown_links_and_anchors_in_docs_resolve() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut md_files = Vec::new();
    collect_md_files(&docs_root, &mut md_files);
    assert!(
        md_files.len() >= 20,
        "expected at least 20 markdown files under docs/, found {}",
        md_files.len()
    );

    let (checked_links, broken) = check_markdown_links_in_files(&md_files);

    assert!(
        checked_links >= 200,
        "expected at least 200 internal links across docs/, checked {checked_links}"
    );
    assert!(
        broken.is_empty(),
        "broken relative links or #anchors in docs/:\n{}",
        broken.join("\n")
    );
}

/// WHY: a fenced code block closes only on a fence line with no info string.
/// A block left open swallows every following line, so a ```` ```rust ````
/// meant to open the next example renders as code text, the prose between
/// the two examples renders as code, and the link check above, which toggles
/// on every fence line, reads code as prose and prose as code. This walks each
/// file under `docs/` with the closing rule a renderer uses and fails on a
/// fence that opens inside an open block or a block still open at the end of
/// the file. Not caught: a bare fence meant to open a block that instead
/// closes the one before it.
#[test]
fn every_fenced_code_block_in_docs_closes() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut md_files = Vec::new();
    collect_md_files(&docs_root, &mut md_files);
    let mut fences = 0usize;
    let mut unclosed: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let mut open: Option<(usize, char, usize)> = None;
        for (idx, line) in text.lines().enumerate() {
            let trimmed = line.trim_start_matches(' ');
            if line.len() - trimmed.len() > 3 {
                continue;
            }
            let Some(mark) = trimmed.chars().next().filter(|c| *c == '`' || *c == '~') else {
                continue;
            };
            let run = trimmed.chars().take_while(|c| *c == mark).count();
            if run < 3 {
                continue;
            }
            let info = trimmed[run..].trim();
            match open {
                None => {
                    open = Some((idx + 1, mark, run));
                    fences += 1;
                }
                Some((_, open_mark, open_run))
                    if mark == open_mark && run >= open_run && info.is_empty() =>
                {
                    open = None;
                }
                Some((opened, _, _)) if !info.is_empty() => unclosed.push(format!(
                    "{}:{}: the fence line {:?} opens inside the block opened at line {opened}",
                    md_file.display(),
                    idx + 1,
                    trimmed.trim_end()
                )),
                Some(_) => {}
            }
        }
        if let Some((opened, _, _)) = open {
            unclosed.push(format!(
                "{}:{opened}: fenced block never closes",
                md_file.display()
            ));
        }
    }
    assert!(
        fences >= 100,
        "expected at least 100 fenced blocks across docs/, found {fences}"
    );
    assert!(
        unclosed.is_empty(),
        "unbalanced code fences in docs/:\n{}",
        unclosed.join("\n")
    );
}

/// WHY: a pipe table continues to the first blank line or block start, so a
/// paragraph written on the line after the last row renders as one more row
/// of the table, its text in the first column. This walks each file under
/// `docs/` outside fenced blocks and fails on a table followed by a line that
/// is neither a row, a blank line, nor the start of another block. Not caught:
/// a row whose cell count differs from the header's.
#[test]
fn every_table_in_docs_ends_at_a_blank_line() {
    let docs_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut md_files = Vec::new();
    collect_md_files(&docs_root, &mut md_files);
    let starts_block = |line: &str| {
        let t = line.trim_start();
        let ordered = t
            .split_once(". ")
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        ["#", "```", "~~~", ">", "- ", "* ", "+ ", "<"]
            .iter()
            .any(|p| t.starts_with(p))
            || ordered
    };
    let mut tables = 0usize;
    let mut runons: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let lines: Vec<&str> = text.lines().collect();
        let mut in_fence = false;
        let mut in_table = false;
        for (idx, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
                in_table = false;
                continue;
            }
            if in_fence {
                continue;
            }
            let row = line.trim_start().starts_with('|');
            if in_table {
                if row {
                    continue;
                }
                in_table = false;
                if !line.trim().is_empty() && !starts_block(line) {
                    runons.push(format!(
                        "{}:{}: {:?} continues the table above it",
                        md_file.display(),
                        idx + 1,
                        line.trim()
                    ));
                }
                continue;
            }
            let delimiter = lines.get(idx + 1).is_some_and(|next| {
                let next = next.trim();
                next.starts_with('|')
                    && next.contains('-')
                    && next.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
            });
            if row && delimiter {
                in_table = true;
                tables += 1;
            }
        }
    }
    assert!(
        tables >= 100,
        "expected at least 100 tables across docs/, found {tables}"
    );
    assert!(
        runons.is_empty(),
        "table rows run into the next paragraph in docs/:\n{}",
        runons.join("\n")
    );
}

/// The lines of `text` outside fenced code blocks, with their 1-based numbers.
fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fence: Option<&'static str> = None;
    let mut lines = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        match fence {
            Some(mark) if trimmed.starts_with(mark) => fence = None,
            Some(_) => {}
            None => match ["```", "~~~"].into_iter().find(|m| trimmed.starts_with(m)) {
                Some(mark) => fence = Some(mark),
                None => lines.push((idx + 1, line)),
            },
        }
    }
    lines
}

/// WHY: the documentation register states the control, setting, or file
/// that acts, in flat sentences, so an em dash and the persona nouns "the
/// user" and "the operator" are out of the register. Rewriting the instances
/// leaves the class open, so this walks the repository README and every file
/// under `docs/` outside fenced blocks and inline code spans and fails on an
/// em dash or one of those phrases. Not caught: the other banned filler and
/// metaphor words, and a persona phrased another way, such as "users".
#[test]
fn docs_prose_has_no_em_dash_or_persona_noun() {
    const BANNED: [&str; 4] = ["\u{2014}", "the user", "the operator", "an operator"];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut md_files = vec![root.join("README.md")];
    collect_md_files(&root.join("docs"), &mut md_files);
    let mut read = 0usize;
    let mut found: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        for (line_no, line) in prose_lines(&text) {
            read += 1;
            let prose = strip_inline_code(line).to_lowercase();
            for banned in BANNED.iter().filter(|b| prose.contains(*b)) {
                found.push(format!(
                    "{}:{line_no}: {banned:?} in {:?}",
                    md_file.display(),
                    line.trim()
                ));
            }
        }
    }
    assert!(
        read >= 10_000,
        "expected at least 10000 prose lines across docs/ and README.md, read {read}"
    );
    assert!(
        found.is_empty(),
        "docs prose outside the register:\n{}",
        found.join("\n")
    );
}

/// WHY: the documentation is spelled in US English, as the editor's labels
/// are, so a chapter does not write `colour` beside the Color panel it
/// documents. This walks the repository README, the changelog, every file
/// under `docs/` and each crate README outside fenced blocks and inline code
/// spans, where identifiers and manifest keys keep their published spelling,
/// and fails on a word [`british_words`] reports. Not caught: a British
/// spelling that matcher does not hold, and the Rust doc comments outside the
/// plugin ABI.
#[test]
fn docs_prose_is_spelled_in_us_english() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut md_files = vec![root.join("README.md"), root.join("CHANGELOG.md")];
    collect_md_files(&root.join("docs"), &mut md_files);
    let crates = std::fs::read_dir(root.join("crates")).expect("read crates/");
    let mut readmes: Vec<PathBuf> = crates
        .map(|entry| {
            entry
                .expect("read a crates/ entry")
                .path()
                .join("README.md")
        })
        .filter(|readme| readme.is_file())
        .collect();
    readmes.sort();
    md_files.extend(readmes);
    let mut read = 0usize;
    let mut found: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        for (line_no, line) in prose_lines(&text) {
            read += 1;
            for (word, us) in british_words(&strip_inline_code(line)) {
                found.push(format!(
                    "{}:{line_no}: `{word}`, US `{us}`",
                    md_file.display()
                ));
            }
        }
    }
    assert!(
        read >= 10_000,
        "expected at least 10000 prose lines across the READMEs and docs/, read {read}"
    );
    assert!(
        found.is_empty(),
        "British spellings in docs prose; write the US spelling:\n{}",
        found.join("\n")
    );
}

/// Plural nouns that name catalog members, registry rows, or shell elements.
/// A number before one of them counts entries a chapter already lists.
const CATALOG_NOUNS: [&str; 37] = [
    "animations",
    "bindings",
    "codecs",
    "commands",
    "controls",
    "crossfades",
    "curves",
    "dialogs",
    "easings",
    "effects",
    "encoders",
    "entries",
    "fields",
    "formats",
    "generators",
    "keys",
    "kinds",
    "menus",
    "modes",
    "panels",
    "parameters",
    "presets",
    "properties",
    "rows",
    "sections",
    "shapes",
    "shortcuts",
    "sliders",
    "stickers",
    "surfaces",
    "tabs",
    "tools",
    "transitions",
    "variants",
    "views",
    "wheels",
    "workspaces",
];

/// Lines whose count `docs_claims_tests` derives from a registry, so the
/// count cannot go stale: the file, by its path under the repository root,
/// and the start of the line.
const PINNED_COUNT_LINES: [(&str, &str); 1] = [("docs/architecture.md", "| Menu rows, all ")];

/// The phrases of `prose` that count catalog members: an integer word
/// followed, within four words of the same clause, by a word in
/// `CATALOG_NOUNS`. A word ending in `,`, `.`, `;`, `:`, or `)` ends the
/// clause. `prose` has its inline code removed.
fn catalog_counts(prose: &str) -> Vec<String> {
    let words: Vec<&str> = prose.split_whitespace().collect();
    let mut counts = Vec::new();
    for (at, word) in words.iter().enumerate() {
        let digits = word.trim_start_matches(['(', '*']);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        for (ahead, next) in words.iter().enumerate().skip(at + 1).take(4) {
            let bare = next
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .to_lowercase();
            if CATALOG_NOUNS.contains(&bare.as_str()) {
                counts.push(words[at..=ahead].join(" "));
                break;
            }
            if next.ends_with([',', '.', ';', ':', ')']) {
                break;
            }
        }
    }
    counts
}

#[test]
fn catalog_counts_finds_member_counts_and_skips_measures() {
    let cases: [(&str, &[&str]); 10] = [
        (
            "| 24 Panels, 15 Project tabs |",
            &["24 Panels,", "15 Project tabs"],
        ),
        (
            "Aria ships 8 video transition entries in",
            &["8 video transition entries"],
        ),
        (
            "| 17 animations, 6 text style presets |",
            &["17 animations,", "6 text style presets"],
        ),
        ("expose 149 state commands (", &["149 state commands"]),
        ("lists (128 entries) here", &["(128 entries)"]),
        ("the 15  variants in order", &["15 variants"]),
        ("a 40 ms audio grain at each position", &[]),
        ("8 parallel lowpass-feedback comb filters", &[]),
        ("move 12 frames, then the presets", &[]),
        ("3 more steps across the five presets", &[]),
    ];
    for (prose, want) in cases {
        assert_eq!(catalog_counts(prose), want, "catalog_counts({prose:?})");
    }
}

/// WHY: a number that counts catalog members in a manual page goes stale
/// when the catalog grows, and no registry test reads it: the reference
/// landing page once counted 17 animations and 16 colour filter presets
/// against catalogs of 31 and 10. Each chapter lists its members, so prose
/// and table cells state no count of them. This walks the repository README
/// and every file under `docs/` outside fenced blocks and inline code spans
/// and fails on a phrase `catalog_counts` reports. `docs/spec/` and
/// `docs/performance.md` are skipped: they state measured workloads and pixel
/// sizes, where the number is the fact, and so are the lines in
/// `PINNED_COUNT_LINES`. Not caught: a count spelled as a word, or a count of
/// a noun outside `CATALOG_NOUNS`.
#[test]
fn docs_prose_counts_no_catalog_members() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut md_files = vec![root.join("README.md")];
    collect_md_files(&root.join("docs"), &mut md_files);
    let skipped = [root.join("docs/spec"), root.join("docs/performance.md")];
    md_files.retain(|path| !skipped.iter().any(|skip| path.starts_with(skip)));
    let mut read = 0usize;
    let mut found: Vec<String> = Vec::new();
    for md_file in &md_files {
        let text =
            std::fs::read_to_string(md_file).unwrap_or_else(|e| panic!("read {md_file:?}: {e}"));
        let pinned: Vec<&str> = PINNED_COUNT_LINES
            .iter()
            .filter(|(path, _)| md_file.ends_with(path))
            .map(|(_, start)| *start)
            .collect();
        for (line_no, line) in prose_lines(&text) {
            read += 1;
            if pinned.iter().any(|start| line.starts_with(start)) {
                continue;
            }
            for count in catalog_counts(&strip_inline_code(line)) {
                found.push(format!("{}:{line_no}: {count:?}", md_file.display()));
            }
        }
    }
    assert!(
        read >= 10_000,
        "expected at least 10000 prose lines across docs/ and README.md, read {read}"
    );
    assert!(
        found.is_empty(),
        "docs count catalog members a chapter lists; drop the number:\n{}",
        found.join("\n")
    );
}
