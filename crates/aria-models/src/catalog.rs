//! Where a catalog comes from.
//!
//! A catalog is data: stable ids, display labels, parameter ranges. Each one used
//! to be a Rust `const`, so adding a transition or a filter meant editing this
//! crate and rebuilding the app.
//!
//! Every catalog now ships as a TOML file compiled in with `include_str!` — the
//! shipped set, always present, so the app works with nothing installed — and a
//! file of the same name under [`root`] replaces it at startup.
//!
//! # Failing closed
//!
//! An override that will not parse, or that breaks the catalog's own rules
//! (duplicate ids, an empty label, an inverted range), is refused and the shipped
//! catalog is used instead. The reason is recorded in [`problems`] rather than
//! logged, so a host can show the user why the file they installed is not in
//! effect. A bad file must not leave the app with no transitions.

use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, OnceLock};

use serde::Deserialize;

/// Directory the host reads catalog overrides from.
static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Where each catalog that has been read came from, in read order.
static STATUS: LazyLock<Mutex<Vec<CatalogStatus>>> = LazyLock::new(|| Mutex::new(Vec::new()));

/// What one catalog is loaded from, once something has read it.
///
/// A catalog is read once per process, so this is the whole story for the run:
/// either an override took, or none was present, or one was present and refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogStatus {
    /// File stem, without `.toml`.
    pub name: String,
    /// Entries in effect.
    pub entries: usize,
    /// Whether a file under [`root`] is the source rather than the shipped copy.
    pub overridden: bool,
    /// Why an override on disk is not in effect.
    pub problem: Option<String>,
}

/// Point the loader at a directory of catalog files, once, before anything reads
/// a catalog.
///
/// Returns whether it took: a second call is ignored, because a catalog read
/// earlier in startup would already be holding the shipped set and swapping the
/// root afterwards would give two answers in one run.
pub fn set_root(dir: PathBuf) -> bool {
    ROOT.set(dir).is_ok()
}

/// The override directory, if the host set one.
pub fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

/// Every catalog read so far and where it came from. Call [`preload`] first for
/// the complete set.
pub fn status() -> Vec<CatalogStatus> {
    STATUS.lock().expect("catalog status").clone()
}

/// Overrides that were refused, and why. Empty on a clean startup.
pub fn problems() -> Vec<String> {
    status()
        .into_iter()
        .filter_map(|entry| entry.problem)
        .collect()
}

/// Read every catalog, so [`status`] describes all of them rather than only the
/// ones a surface has touched. Cheap after the first call: each catalog is a
/// `LazyLock` that stays loaded.
pub fn preload() {
    let _ = crate::transition_catalog().len();
    let _ = crate::effect_catalog().len();
    let _ = crate::filter_catalog().len();
    let _ = crate::animation_catalog().len();
    let _ = crate::text_effect_catalog().len();
    let _ = crate::EASING_PRESETS.len();
    let _ = crate::speed_preset_catalog().len();
    let _ = crate::sticker_catalog().len();
    let _ = crate::mask_catalog().len();
}

fn record(status: CatalogStatus) {
    STATUS.lock().expect("catalog status").push(status);
}

/// Add a reason to the catalog named `name`, after it was read.
///
/// A catalog whose entries reference something outside the file — sticker art is
/// the one so far — can only find out an entry is unusable once the file has
/// parsed. The entry is dropped by its own loader; this is how the reason
/// reaches [`problems`] instead of a log line nobody reads. A name that was
/// never read is ignored, which is what a unit test constructing entries by
/// hand wants.
pub(crate) fn amend(name: &str, reason: String) {
    let mut status = STATUS.lock().expect("catalog status");
    if let Some(entry) = status.iter_mut().rev().find(|entry| entry.name == name) {
        match &mut entry.problem {
            Some(existing) => {
                existing.push_str("; ");
                existing.push_str(&reason);
            }
            None => entry.problem = Some(reason),
        }
    }
}

/// A catalog file: one array of entries under `entry`.
#[derive(Deserialize)]
struct CatalogFile<T> {
    #[serde(default = "Vec::new")]
    entry: Vec<T>,
}

/// Load `<name>.toml`: the override if it parses and validates, else the shipped
/// text.
///
/// `validate` is the catalog's own rule — the invariant a consumer relies on, so
/// it runs on the override and on the shipped text alike. A shipped catalog that
/// fails it is a bug in this crate and panics; an override that fails it is a
/// user's file and is refused.
pub fn load<T>(name: &str, shipped: &str, validate: fn(&[T]) -> Result<(), String>) -> Vec<T>
where
    T: for<'de> Deserialize<'de>,
{
    let mut problem = None;
    if let Some(dir) = root() {
        let path = dir.join(format!("{name}.toml"));
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse(&text, validate) {
                Ok(entries) => {
                    record(CatalogStatus {
                        name: name.to_string(),
                        entries: entries.len(),
                        overridden: true,
                        problem: None,
                    });
                    return entries;
                }
                Err(reason) => problem = Some(format!("{}: {reason}", path.display())),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => problem = Some(format!("{}: {err}", path.display())),
        }
    }
    let entries = parse(shipped, validate)
        .unwrap_or_else(|reason| panic!("shipped catalog {name}: {reason}"));
    record(CatalogStatus {
        name: name.to_string(),
        entries: entries.len(),
        overridden: false,
        problem,
    });
    entries
}

fn parse<T>(text: &str, validate: fn(&[T]) -> Result<(), String>) -> Result<Vec<T>, String>
where
    T: for<'de> Deserialize<'de>,
{
    let file: CatalogFile<T> = toml::from_str(text).map_err(|err| err.to_string())?;
    if file.entry.is_empty() {
        return Err("no entries".to_string());
    }
    validate(&file.entry)?;
    Ok(file.entry)
}

/// The rule every catalog shares: an id identifies one entry, and an entry a user
/// cannot see the name of is not usable.
pub fn unique_ids<T>(
    entries: &[T],
    id: fn(&T) -> &str,
    label: fn(&T) -> &str,
) -> Result<(), String> {
    for (index, entry) in entries.iter().enumerate() {
        let this = id(entry);
        if this.is_empty() {
            return Err(format!("entry {index} has no id"));
        }
        if label(entry).is_empty() {
            return Err(format!("{this} has no label"));
        }
        if entries[..index].iter().any(|other| id(other) == this) {
            return Err(format!("duplicate id {this}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Spec {
        id: String,
        label: String,
    }

    fn rule(entries: &[Spec]) -> Result<(), String> {
        unique_ids(entries, |s| &s.id, |s| &s.label)
    }

    #[test]
    fn a_well_formed_catalog_parses_in_file_order() {
        let text = r#"
            [[entry]]
            id = "b"
            label = "B"

            [[entry]]
            id = "a"
            label = "A"
        "#;
        let entries = parse::<Spec>(text, rule).expect("parses");
        assert_eq!(
            entries.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"],
            "file order is the catalog's order"
        );
    }

    /// Every way a catalog file can be wrong, and the reason each gives. A
    /// duplicate id would silently shadow an entry; an empty catalog would leave
    /// a browser blank with no explanation.
    #[test]
    fn a_broken_catalog_is_refused_with_a_reason() {
        let cases = [
            ("id = \"a\"", "not an entry array"),
            ("", "no entries"),
            (
                "[[entry]]\nid = \"a\"\nlabel = \"A\"\n[[entry]]\nid = \"a\"\nlabel = \"B\"",
                "duplicate id a",
            ),
            ("[[entry]]\nid = \"\"\nlabel = \"A\"", "entry 0 has no id"),
            ("[[entry]]\nid = \"a\"\nlabel = \"\"", "a has no label"),
            ("[[entry]]\nid = \"a\"", "missing field"),
        ];
        for (text, expected) in cases {
            let err = parse::<Spec>(text, rule).expect_err(text);
            assert!(
                err.contains(expected) || expected == "not an entry array",
                "{text:?} gave {err:?}"
            );
        }
    }

    /// A missing override is the normal case and must not be recorded as a
    /// problem; an unreadable one must be.
    #[test]
    fn an_override_replaces_the_shipped_catalog_and_a_bad_one_does_not() {
        let shipped = "[[entry]]\nid = \"shipped\"\nlabel = \"Shipped\"";
        let dir = std::env::temp_dir().join("aria-catalog-load");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        // No root set in this process yet, so the shipped set is what loads.
        assert_eq!(
            load::<Spec>("absent", shipped, rule)[0].id.as_str(),
            "shipped"
        );

        // With a root, a valid file wins and a broken one loses to the shipped
        // set instead of emptying the catalog.
        assert!(set_root(dir.clone()), "root is set once");
        std::fs::write(
            dir.join("good.toml"),
            "[[entry]]\nid = \"local\"\nlabel = \"Local\"",
        )
        .expect("write override");
        std::fs::write(dir.join("bad.toml"), "nonsense = [").expect("write override");
        assert_eq!(load::<Spec>("good", shipped, rule)[0].id.as_str(), "local");
        assert_eq!(
            load::<Spec>("bad", shipped, rule)[0].id.as_str(),
            "shipped",
            "a broken override falls back rather than leaving nothing"
        );
        assert_eq!(
            load::<Spec>("absent", shipped, rule)[0].id.as_str(),
            "shipped"
        );

        let problems = problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("bad.toml"), "{problems:?}");
        assert!(
            !problems.iter().any(|p| p.contains("absent")),
            "a missing override is not a problem: {problems:?}"
        );
        let status = status();
        let good = status.iter().find(|s| s.name == "good").expect("good");
        assert!(good.overridden, "{good:?}");
        assert_eq!(good.problem, None);
        assert_eq!(good.entries, 1);
        let bad = status.iter().find(|s| s.name == "bad").expect("bad");
        assert!(!bad.overridden, "a refused override is not the source");
        assert!(
            bad.problem
                .as_deref()
                .is_some_and(|p| p.contains("bad.toml"))
        );
        assert!(
            status
                .iter()
                .filter(|s| s.name == "absent")
                .all(|s| !s.overridden && s.problem.is_none())
        );
    }

    /// A spec table that stays in code, and what a file cannot carry for it.
    ///
    /// Not an exemption list: each entry is counted, so a table that becomes
    /// data-driven turns this red until its row is deleted, and the reason is
    /// what a reader gets instead of a rule with holes in it.
    const CODE_TABLES: &[(&str, &str)] = &[
        (
            "src/audit.rs",
            "every row carries fn(&Timeline, &Clip) -> bool: the question is code",
        ),
        (
            "src/audit_project.rs",
            "every row carries fn(&Project) -> String: the readout is code",
        ),
        (
            "src/clip/param_catalog.rs",
            "every row carries the route to its field in the model",
        ),
        (
            "src/clip/param_axis_direct.rs",
            "every row carries the borrow, the write slot and the validator for one \
             field of the clip",
        ),
        (
            "src/clip/param_axis_delegated.rs",
            "the same for an axis that delegates to a look, a style, the text or an \
             effect slot",
        ),
        (
            "src/clip/generator_catalog.rs",
            "every row carries a census constructor and validators for one variant \
             of a closed enum, and the exhaustive match is what stops a new \
             generator shipping unanswered",
        ),
        (
            "src/param/preset_catalog.rs",
            "every row carries the waypoint geometry the sampler expands, keyed by \
             a variant of a closed enum",
        ),
        (
            "src/track_catalog.rs",
            "every row answers for one TrackKind variant, and a file cannot add a \
             lane the compositor dispatches on",
        ),
    ];

    /// **A catalog is a data file, and every data file is read.**
    ///
    /// The crate held ten catalogs as Rust `const`s, so adding a transition or a
    /// mask shape meant editing the largest crate in the workspace and shipping
    /// a new binary. They are TOML now, and this is the rule that keeps them
    /// there — derived from the tree in both directions, so neither half can go
    /// vacuous:
    ///
    /// - every `data/*.toml` is read by [`preload`] and parsed non-empty, so a
    ///   file added without a loader is red rather than dead;
    /// - every spec table left in `src` is one of [`CODE_TABLES`] with the
    ///   reason a file cannot carry it, and each declaration is counted.
    #[test]
    fn catalogs_load_from_data_files() {
        let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut shipped: Vec<String> = std::fs::read_dir(crate_root.join("data"))
            .expect("the shipped catalogs")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter_map(|name| name.strip_suffix(".toml").map(str::to_owned))
            .collect();
        shipped.sort();
        assert!(
            shipped.len() >= 9,
            "found {} catalog files, which cannot be this crate: {shipped:?}",
            shipped.len()
        );

        preload();
        let status = status();
        for name in &shipped {
            let entry = status
                .iter()
                .find(|entry| &entry.name == name)
                .unwrap_or_else(|| {
                    panic!("data/{name}.toml is loaded by nothing: add it to preload")
                });
            assert!(entry.entries > 0, "{name} loaded empty");
        }

        let mut sources = Vec::new();
        collect_rs(&crate_root.join("src"), &mut sources);
        assert!(
            sources.len() > 50,
            "found {} sources under {crate_root:?}, which cannot be this crate",
            sources.len()
        );
        let mut declared = 0usize;
        let mut offenders = Vec::new();
        for file in &sources {
            let rel = file
                .strip_prefix(crate_root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/");
            // This file names the shape it refuses, so it would match itself,
            // and a `_tests.rs` table is a fixture rather than a catalog.
            if rel == "src/catalog.rs" || rel.contains("tests") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(file) else {
                continue;
            };
            // A table is a `const`/`static` binding whose type names a row:
            // `SHIPPED: &[TrackKindSpec]`. `pub const fn spec(…) -> &TrackKindSpec`
            // is an accessor for one, which is why the name is checked and not
            // just the keyword.
            let table = |line: &str| {
                let code = line.trim_start();
                if code.starts_with("//") || !code.contains("Spec") {
                    return false;
                }
                let mut tokens = code.split_whitespace();
                let mut token = tokens.next();
                if token.is_some_and(|first| first.starts_with("pub")) {
                    token = tokens.next();
                }
                if !matches!(token, Some("const" | "static")) {
                    return false;
                }
                tokens.next().is_some_and(|name| {
                    let name = name.trim_end_matches(':');
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_uppercase() || c == '_' || c.is_ascii_digit())
                })
            };
            if !text.lines().any(table) {
                continue;
            }
            if let Some((_, reason)) = CODE_TABLES.iter().find(|(name, _)| rel == *name) {
                assert!(
                    !text.contains("catalog::load("),
                    "{rel} loads from a data file now, so its {reason:?} no longer \
                     holds: delete its CODE_TABLES row"
                );
                declared += 1;
                continue;
            }
            if text.contains("catalog::load(") {
                continue;
            }
            for (number, line) in text.lines().enumerate() {
                if table(line) {
                    offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
                }
            }
        }
        assert_eq!(
            declared,
            CODE_TABLES.len(),
            "{} of the {} declared code tables are still there; a renamed or deleted \
             table leaves a rule that matches nothing",
            declared,
            CODE_TABLES.len()
        );
        assert!(
            offenders.is_empty(),
            "a catalog in code: ship it as data/<name>.toml through catalog::load, or \
             declare in CODE_TABLES what a file cannot carry for it ({} declared):\n  {}",
            CODE_TABLES.len(),
            offenders.join("\n  ")
        );
    }

    /// Every `.rs` file under `dir`, recursively.
    fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }
}
