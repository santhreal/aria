//! WHY: every install sent anonymous catalog, stock and update requests to a
//! built-in host that was not run by this project, so whoever held the domain
//! could serve templates, assistant skill packs, LUTs and Lottie files to every
//! editor. The class is a network host written into shipped code that nobody
//! decided on. This census reads every shipped `.rs` file of every crate in the
//! workspace, takes the host of each `http://` and `https://` string literal, and
//! requires it to be listed below with the reason it is contacted. A new host
//! turns this red until it is listed; a listed host no longer in the source turns
//! it red too, so the list cannot go stale.
//!
//! Shipped means: not a test file (`tests.rs`, `*_tests*.rs`, `tests_*.rs`, a
//! `tests/` or `test_support` path) and above the first `#[cfg(test)]` line of
//! its file. Comment lines are skipped.
//!
//! Not covered: a host assembled at run time from parts, a host in a data file
//! (TOML, JSON) rather than a `.rs` literal, and a host entered into a
//! setting.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Every host shipped code may contact, with the reason.
const KNOWN_HOSTS: &[(&str, &str)] = &[
    (
        "api.github.com",
        "the update check reads this repository's releases",
    ),
    (
        "github.com",
        "the prefix the workspace repository URL is parsed against, which is never fetched",
    ),
    ("api.pexels.com", "direct stock search with a Pexels key"),
    ("pixabay.com", "direct stock search with a Pixabay key"),
    ("queue.fal.run", "BYOK fal.ai generation"),
    ("openrouter.ai", "the OpenRouter AI provider"),
    ("huggingface.co", "Whisper model downloads"),
    ("raw.githubusercontent.com", "Whisper model checksums"),
    (
        "www.w3.org",
        "the SVG XML namespace, an identifier that is never fetched",
    ),
];

/// Loopback hosts: a server on the same machine.
const LOOPBACK: &[&str] = &["localhost", "127.0.0.1", "[::1]"];

fn is_test_file(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('\\', "/");
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    name == "tests.rs"
        || name.starts_with("tests_")
        || name.contains("_tests")
        || text.contains("/tests/")
        || text.contains("test_support")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// The host of every URL literal in the shipped part of `source`.
fn literal_hosts(source: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        for scheme in ["\"https://", "\"http://"] {
            for (at, _) in line.match_indices(scheme) {
                let rest = &line[at + scheme.len()..];
                let end = rest
                    .find(|c: char| {
                        matches!(c, '/' | ':' | '"' | '?' | '#' | '\\') || c.is_whitespace()
                    })
                    .unwrap_or(rest.len());
                let host = &rest[..end];
                // `"https://"` alone is a scheme check, and `{` is a format hole.
                if !host.is_empty() && !host.contains('{') {
                    hosts.push(host.to_string());
                }
            }
        }
    }
    hosts
}

#[test]
fn literal_hosts_reads_the_host_of_each_shipped_url_literal() {
    let source = "let a = \"https://one.example/x\";\n\
                  let b = \"http://127.0.0.1:8080\";\n\
                  // let c = \"https://comment.example\";\n\
                  let d = s.strip_prefix(\"https://\");\n\
                  let e = format!(\"https://{host}/v1\");\n\
                  let f = \"https://two.example\";\n\
                  #[cfg(test)]\n\
                  let g = \"https://test.example\";\n";
    assert_eq!(
        literal_hosts(source),
        ["one.example", "127.0.0.1", "two.example"]
    );
}

#[test]
fn every_shipped_url_host_is_a_known_host() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("aria-cloud is in the crates directory");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(crates)
        .expect("read the crates directory")
        .flatten()
    {
        rs_files(&entry.path().join("src"), &mut files);
    }
    let scanned: Vec<_> = files.into_iter().filter(|p| !is_test_file(p)).collect();
    assert!(
        scanned.len() > 500,
        "the census read {} shipped files; the crates directory moved",
        scanned.len()
    );

    let known: BTreeSet<&str> = KNOWN_HOSTS.iter().map(|(host, _)| *host).collect();
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for path in &scanned {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for host in literal_hosts(&source) {
            let file = path
                .strip_prefix(crates)
                .unwrap_or(path)
                .display()
                .to_string();
            found.entry(host).or_default().push(file);
        }
    }

    let unknown: Vec<_> = found
        .iter()
        .filter(|(host, _)| !known.contains(host.as_str()) && !LOOPBACK.contains(&host.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "shipped code contacts hosts not in KNOWN_HOSTS; list each with its reason or remove it: {unknown:?}"
    );
    let stale: Vec<_> = known
        .iter()
        .filter(|host| !found.contains_key(**host))
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_HOSTS lists hosts no shipped file names; delete them: {stale:?}"
    );
}
