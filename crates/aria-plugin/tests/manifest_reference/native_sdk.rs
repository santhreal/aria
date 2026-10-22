//! Reference documentation checks for `docs/plugins/native.md` against
//! `crates/aria-sdk/src`.

use super::*;

/// Every `.rs` file under `dir`, recursively, with its text, sorted by path.
fn rust_sources_under(dir: &Path) -> Vec<(PathBuf, String)> {
    let mut pending = vec![dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(next) = pending.pop() {
        let entries = std::fs::read_dir(&next)
            .unwrap_or_else(|err| panic!("failed to list {}: {err}", next.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|err| {
                    panic!("failed to read an entry of {}: {err}", next.display())
                })
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let src = std::fs::read_to_string(&path)
                    .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
                files.push((path, src));
            }
        }
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// Whether the trimmed line `trimmed` opens `pub <kind> <name>`.
fn opens_item(trimmed: &str, kind: &str, name: &str) -> bool {
    trimmed
        .strip_prefix("pub ")
        .and_then(|rest| rest.strip_prefix(kind))
        .and_then(|rest| rest.strip_prefix(' '))
        .and_then(|rest| rest.strip_prefix(name))
        .is_some_and(|rest| {
            rest.chars()
                .next()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'))
        })
}

/// The name a `pub use` item brings into scope: its alias, or its last path
/// segment.
fn use_item_name(item: &str) -> String {
    match item.split_once(" as ") {
        Some((_, alias)) => alias.trim().to_owned(),
        None => item.rsplit("::").next().unwrap_or(item).trim().to_owned(),
    }
}

/// Every name `aria_sdk` exports: each `pub use` name or alias in `lib.rs`,
/// each item declared `pub` at the root of `lib.rs`, and each
/// `#[macro_export]` macro under `src/` as `name!`.
fn sdk_exports(lib_src: &str, sdk_sources: &[(PathBuf, String)]) -> Vec<String> {
    let mut exports = Vec::new();
    let mut rest = lib_src;
    while let Some(start) = rest.find("\npub use ") {
        let statement_start = &rest[start + "\npub use ".len()..];
        let end = statement_start
            .find(';')
            .expect("a `pub use` statement in aria-sdk lib.rs ends with `;`");
        let statement = statement_start[..end]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        match statement.split_once("::{") {
            Some((_, group)) => {
                let group = group
                    .strip_suffix('}')
                    .unwrap_or_else(|| panic!("unterminated `pub use` group: {statement}"));
                assert!(
                    !group.contains('{'),
                    "nested `pub use` group in aria-sdk lib.rs: {statement}"
                );
                exports.extend(
                    group
                        .split(',')
                        .map(str::trim)
                        .filter(|item| !item.is_empty())
                        .map(use_item_name),
                );
            }
            None => exports.push(use_item_name(&statement)),
        }
        rest = &statement_start[end..];
    }
    for line in lib_src.lines() {
        let Some(item) = line.strip_prefix("pub ") else {
            continue;
        };
        if item.starts_with("use ") {
            continue;
        }
        let after_keyword = [
            "const fn ",
            "unsafe fn ",
            "fn ",
            "struct ",
            "enum ",
            "trait ",
            "type ",
            "const ",
            "static ",
            "mod ",
            "union ",
        ]
        .into_iter()
        .find_map(|keyword| item.strip_prefix(keyword))
        .unwrap_or_else(|| panic!("unrecognised root item in aria-sdk lib.rs: `{line}`"));
        exports.push(leading_ident(after_keyword));
    }
    for (path, src) in sdk_sources {
        let mut rest = src.as_str();
        while let Some(at) = rest.find("#[macro_export]") {
            let after = &rest[at..];
            let name_start = after.find("macro_rules!").unwrap_or_else(|| {
                panic!(
                    "`#[macro_export]` without `macro_rules!` in {}",
                    path.display()
                )
            }) + "macro_rules!".len();
            exports.push(format!(
                "{}!",
                leading_ident(after[name_start..].trim_start())
            ));
            rest = &after[name_start..];
        }
    }
    exports
}

/// The members the chapter documenting the `aria_sdk` item `name` must state:
/// a struct's `pub` fields, an enum's variants, a trait's methods, and the
/// `pub` functions and constants of its inherent impls in any SDK file. An
/// item that is not a struct, enum, or trait (a module, function, constant,
/// macro, alias, or re-exported crate) and a type marked `#[doc(hidden)]` have
/// none.
fn sdk_item_members(sdk_sources: &[(PathBuf, String)], name: &str) -> Vec<String> {
    let mut members = Vec::new();
    let mut definitions = 0usize;
    for (_, src) in sdk_sources {
        let lines: Vec<&str> = src.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let Some(kind) = ["struct", "enum", "trait"]
                .into_iter()
                .find(|kind| opens_item(trimmed, kind, name))
            else {
                continue;
            };
            definitions += 1;
            let hidden = lines[..idx]
                .iter()
                .rev()
                .map(|above| above.trim())
                .take_while(|above| above.starts_with("///") || above.starts_with("#["))
                .any(|above| above == "#[doc(hidden)]");
            if hidden {
                return Vec::new();
            }
            members.extend(match kind {
                "struct" => struct_pub_fields(src, name),
                "enum" => enum_variants(src, name),
                _ => trait_fns(src, name),
            });
        }
        members.extend(impl_pub_items(src, name, true));
    }
    assert!(
        definitions <= 1,
        "`{name}` is defined {definitions} times in aria-sdk"
    );
    members
}

/// The identifiers of the backticked tokens in `cell` that are called as
/// `ident<marker>`, such as `clip(` or `mount(&self`.
fn called_idents(cell: &str, marker: &str) -> Vec<String> {
    extract_backticked_tokens(cell)
        .into_iter()
        .filter_map(|token| {
            let ident = leading_ident(&token);
            (!ident.is_empty() && token[ident.len()..].starts_with(marker)).then_some(ident)
        })
        .collect()
}

/// The file a Markdown link in `cell` points to, without its fragment.
fn link_target(cell: &str) -> &str {
    let (_, after) = cell
        .split_once("](")
        .unwrap_or_else(|| panic!("cell `{cell}` holds no Markdown link"));
    let (target, _) = after
        .split_once(')')
        .unwrap_or_else(|| panic!("unterminated Markdown link in `{cell}`"));
    target.split('#').next().unwrap_or(target)
}

/// `docs/plugins/native.md` against `crates/aria-sdk/src`, every member set
/// enumerated from the source at run time:
/// - the host API table lists each `pub fn` of every inherent `impl Host` in
///   any SDK file, and each `Plugin` trait method as `name(&self`;
/// - the project types table lists each type `project.rs` declares with its
///   fields or variants and its inherent `pub` functions and constants;
/// - the SDK exports table lists each `aria_sdk` export once, and the linked
///   chapter states the export and each of its members inside code.
///
/// Closes the class where an SDK method, callback, type, field, variant, or
/// export is added without documentation, or the documentation keeps a removed
/// one. It does not catch a member whose name is a common word (`new`, `id`)
/// that the linked chapter states for another reason, nor a wrong description.
#[test]
fn native_doc_matches_sdk_host_plugin_project_and_export_registries() {
    let doc = read_workspace_file("docs/plugins/native.md");
    let sdk_dir = workspace_root().join("crates/aria-sdk/src");
    let sdk_sources = rust_sources_under(&sdk_dir);
    let sdk_file = |rel: &str| {
        let path = sdk_dir.join(rel);
        sdk_sources
            .iter()
            .position(|(candidate, _)| *candidate == path)
            .unwrap_or_else(|| panic!("missing aria-sdk source {rel}"))
    };
    let lib_src = sdk_sources[sdk_file("lib.rs")].1.as_str();

    // 1. The host API table.
    let api_tables = parse_markdown_tables(extract_heading_section(&doc, "## `aria-sdk` host API"));
    assert_eq!(api_tables.len(), 2, "host API and project types tables");
    assert_eq!(
        api_tables[0].headers,
        ["Group", "`Host` calls", "`Plugin` callbacks"]
    );
    let mut doc_host_calls = Vec::new();
    let mut doc_plugin_callbacks = Vec::new();
    for row in &api_tables[0].rows {
        assert_eq!(row.len(), 3, "host API row {row:?} does not have 3 columns");
        doc_host_calls.extend(called_idents(&row[1], "("));
        doc_plugin_callbacks.extend(called_idents(&row[2], "(&self"));
    }
    let host_fns: Vec<String> = sdk_sources
        .iter()
        .flat_map(|(_, src)| impl_pub_items(src, "Host", false))
        .collect();
    assert!(!host_fns.is_empty(), "no `impl Host` pub fns in aria-sdk");
    assert_key_parity(
        "docs/plugins/native.md Host calls",
        &host_fns,
        &doc_host_calls,
    );
    assert_key_parity(
        "docs/plugins/native.md Plugin callbacks",
        &extract_trait_fns(lib_src, "Plugin"),
        &doc_plugin_callbacks,
    );

    // 2. The project types table.
    let project_src = sdk_sources[sdk_file("project.rs")].1.as_str();
    let project_types: Vec<String> = project_src
        .lines()
        .filter_map(|line| {
            ["pub struct ", "pub enum "]
                .into_iter()
                .find_map(|keyword| line.strip_prefix(keyword))
        })
        .map(leading_ident)
        .collect();
    let doc_types = extract_type_members_map(&api_tables[1]);
    assert_key_parity(
        "docs/plugins/native.md aria_sdk project types",
        &project_types,
        &doc_types.keys().cloned().collect::<Vec<_>>(),
    );
    for ty in &project_types {
        assert_key_parity(
            &format!("docs/plugins/native.md aria_sdk::{ty} members"),
            &sdk_item_members(&sdk_sources, ty),
            &doc_types[ty],
        );
    }

    // 3. The SDK exports table.
    let export_tables = parse_markdown_tables(extract_heading_section(&doc, "## SDK exports"));
    assert_eq!(export_tables.len(), 1);
    assert_eq!(export_tables[0].headers, ["Chapter", "Exports"]);
    let mut doc_exports = Vec::new();
    for row in &export_tables[0].rows {
        let chapter_path = format!("docs/plugins/{}", link_target(&row[0]));
        let chapter = read_workspace_file(&chapter_path);
        let stated = markdown_code_identifiers(&chapter);
        for export in extract_backticked_tokens(&row[1]) {
            let ident = export.strip_suffix('!').unwrap_or(&export);
            assert!(
                stated.contains(ident) && chapter.contains(export.as_str()),
                "{chapter_path} does not state `{export}` in code"
            );
            let missing: Vec<String> = sdk_item_members(&sdk_sources, ident)
                .into_iter()
                .filter(|member| !stated.contains(member))
                .collect();
            assert!(
                missing.is_empty(),
                "{chapter_path} documents `{export}` without its members {missing:?}"
            );
            doc_exports.push(export);
        }
    }
    assert_key_parity(
        "docs/plugins/native.md SDK exports",
        &sdk_exports(lib_src, &sdk_sources),
        &doc_exports,
    );
}
