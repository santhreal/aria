use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use syn::{Fields, ImplItem};

use super::*;

impl Abi {
    pub(super) fn parse() -> Self {
        let file = syn::parse_file(SOURCE).unwrap_or_else(|err| panic!("src/lib.rs: {err}"));
        let mut types = BTreeSet::new();
        let mut defines = BTreeSet::new();
        let mut methods: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut decls = Vec::new();
        for item in &file.items {
            match item {
                Item::Const(item) if is_pub(&item.vis) => {
                    let name = item.ident.to_string();
                    let value = define_value(&name, &item.ty, &item.expr, &defines);
                    defines.insert(name.clone());
                    decls.push(Decl::Define(Define {
                        name,
                        docs: docs(&item.attrs),
                        value,
                    }));
                }
                Item::Struct(item) if is_pub(&item.vis) => {
                    let name = item.ident.to_string();
                    assert!(
                        repr_c(&item.attrs),
                        "`{name}` is public without `#[repr(C)]`"
                    );
                    let Fields::Named(named) = &item.fields else {
                        panic!("`{name}` has no named fields")
                    };
                    let fields = named
                        .named
                        .iter()
                        .map(|field| {
                            let field_name = field
                                .ident
                                .as_ref()
                                .map(ToString::to_string)
                                .unwrap_or_default();
                            let owner = format!("{name}.{field_name}");
                            assert!(
                                is_pub(&field.vis),
                                "`{owner}` is private, so a C caller cannot set it"
                            );
                            let ty = match &field.ty {
                                Type::BareFn(bare) => {
                                    FieldType::Slot(signature(bare, &owner, &types))
                                }
                                ty => FieldType::Value(c_type(ty, &owner, &types)),
                            };
                            Field {
                                name: field_name,
                                docs: docs(&field.attrs),
                                ty,
                            }
                        })
                        .collect();
                    types.insert(name.clone());
                    decls.push(Decl::Struct(Struct {
                        name,
                        docs: docs(&item.attrs),
                        fields,
                        methods: BTreeSet::new(),
                    }));
                }
                Item::Type(item) if is_pub(&item.vis) => {
                    let name = item.ident.to_string();
                    let Type::BareFn(bare) = &*item.ty else {
                        panic!("`{name}` aliases a type other than a function pointer")
                    };
                    let sig = signature(bare, &name, &types);
                    types.insert(name.clone());
                    decls.push(Decl::Alias(FnAlias {
                        name,
                        docs: docs(&item.attrs),
                        sig,
                    }));
                }
                Item::Impl(item) => {
                    let Some(owner) = type_name(&item.self_ty) else {
                        continue;
                    };
                    let names = methods.entry(owner).or_default();
                    for member in &item.items {
                        if let ImplItem::Fn(method) = member
                            && is_pub(&method.vis)
                        {
                            names.insert(method.sig.ident.to_string());
                        }
                    }
                }
                other => assert!(
                    outside_the_abi(other),
                    "src/lib.rs declares a public item with no rule in tests/abi_reference/main.rs: \
                     the C ABI holds `u32`, `usize` and byte-string consts, `#[repr(C)]` \
                     structs and function pointer aliases"
                ),
            }
        }
        for decl in &mut decls {
            if let Decl::Struct(item) = decl {
                item.methods = methods.remove(&item.name).unwrap_or_default();
            }
        }
        Self {
            docs: docs(&file.attrs),
            decls,
        }
    }

    pub(super) fn defines(&self) -> impl Iterator<Item = &Define> {
        self.decls.iter().filter_map(|decl| match decl {
            Decl::Define(define) => Some(define),
            _ => None,
        })
    }

    pub(super) fn structs(&self) -> impl Iterator<Item = &Struct> {
        self.decls.iter().filter_map(|decl| match decl {
            Decl::Struct(item) => Some(item),
            _ => None,
        })
    }

    fn find_struct(&self, name: &str) -> Option<&Struct> {
        self.structs().find(|item| item.name == name)
    }

    /// The defines that end a prefix of `table`, each with the index of the
    /// first slot after it, in slot order.
    fn prefixes(&self, table: &Struct) -> Vec<(&str, usize)> {
        let mut prefixes: Vec<(&str, usize)> = self
            .defines()
            .filter_map(|define| match &define.value {
                DefineValue::Offset { table: of, slot } if *of == table.name => {
                    let index = table
                        .fields
                        .iter()
                        .position(|field| field.name == *slot)
                        .unwrap_or_else(|| {
                            panic!("`{}` names `{slot}`, which `{of}` lacks", define.name)
                        });
                    Some((define.name.as_str(), index))
                }
                _ => None,
            })
            .collect();
        prefixes.sort_by_key(|(_, index)| *index);
        prefixes
    }

    /// The define that ends the prefix holding field `index` of `table`, or
    /// `sizeof(table)` past the last define.
    fn prefix_of(&self, table: &Struct, index: usize) -> String {
        self.prefixes(table)
            .into_iter()
            .find(|(_, end)| *end > index)
            .map_or_else(
                || format!("sizeof({})", table.name),
                |(name, _)| name.to_owned(),
            )
    }

    /// The anchor of the reference heading that documents the doc link
    /// `target`, such as `AriaHostV1::log` or `ARIA_STATUS_OK`.
    fn anchor(&self, target: &str) -> Option<String> {
        if let Some((owner, member)) = target.split_once("::") {
            let owner = self.find_struct(owner)?;
            let field = owner.fields.iter().find(|field| field.name == member)?;
            return Some(match field.ty {
                FieldType::Slot(_) => slug(&format!("`{}.{member}`", owner.name)),
                FieldType::Value(_) => slug(&format!("`{}`", owner.name)),
            });
        }
        if let Some(define) = self.defines().find(|define| define.name == target) {
            return define_group(&define.name).map(slug);
        }
        self.decls
            .iter()
            .any(|decl| match decl {
                Decl::Struct(item) => item.name == target,
                Decl::Alias(alias) => alias.name == target,
                Decl::Define(_) => false,
            })
            .then(|| slug(&format!("`{target}`")))
    }

    /// `text` with every doc link `` [`target`] `` replaced by `render(name,
    /// anchor)`, where `name` spells a member as `Struct.member`.
    fn relink(&self, owner: &str, text: &str, render: fn(&str, &str) -> String) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("[`") {
            let after = &rest[start + 2..];
            let end = after.find("`]").unwrap_or_else(|| {
                panic!("a doc link of `{owner}` does not close on its line: {text}")
            });
            let target = &after[..end];
            let tail = &after[end + 2..];
            out.push_str(&rest[..start]);
            if tail.starts_with('(') || tail.starts_with('[') {
                out.push_str(&rest[start..start + 2 + end + 2]);
            } else {
                let anchor = self.anchor(target).unwrap_or_else(|| {
                    panic!(
                        "the doc comment of `{owner}` links `{target}`, which the C ABI does not \
                         declare"
                    )
                });
                out.push_str(&render(&target.replace("::", "."), &anchor));
            }
            rest = tail;
        }
        out.push_str(rest);
        out
    }

    /// The paragraphs of `docs`, each on one line, with links to reference anchors.
    fn paragraphs(&self, owner: &str, docs: &[String]) -> Vec<String> {
        let mut paragraphs = Vec::new();
        let mut current = String::new();
        let end = String::new();
        for line in docs.iter().chain(std::iter::once(&end)) {
            let line = line.trim();
            assert!(
                !["#", "```", "- ", "* ", "| "]
                    .iter()
                    .any(|block| line.starts_with(block)),
                "the doc comment of `{owner}` holds Markdown blocks, and the reference renders \
                 plain paragraphs: {line}"
            );
            if line.is_empty() {
                if !current.is_empty() {
                    paragraphs.push(self.relink(owner, &current, markdown_link));
                    current.clear();
                }
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(line);
            }
        }
        paragraphs
    }

    /// `docs` as one table cell.
    fn cell(&self, owner: &str, docs: &[String]) -> String {
        self.paragraphs(owner, docs).join(" ").replace('|', "\\|")
    }

    /// `docs` as C comment lines starting with `indent` and `marker`.
    fn comment(&self, out: &mut String, owner: &str, indent: &str, marker: &str, docs: &[String]) {
        for line in docs {
            let line = self.relink(owner, line.trim_end(), c_link);
            if line.is_empty() {
                let _ = writeln!(out, "{indent}{marker}");
            } else {
                let _ = writeln!(out, "{indent}{marker} {line}");
            }
        }
    }

    pub(super) fn entry(&self) -> (&str, &FnAlias) {
        let symbol = self
            .defines()
            .find_map(|define| match &define.value {
                DefineValue::Text(text) if define.name == ENTRY.0 => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("src/lib.rs declares no byte-string `{}`", ENTRY.0));
        let alias = self
            .decls
            .iter()
            .find_map(|decl| match decl {
                Decl::Alias(alias) if alias.name == ENTRY.1 => Some(alias),
                _ => None,
            })
            .unwrap_or_else(|| panic!("src/lib.rs declares no `{}`", ENTRY.1));
        (symbol, alias)
    }

    fn header(&self) -> String {
        let mut out = String::new();
        out.push_str(
            "// Generated from crates/aria-plugin-abi/src/lib.rs by\n\
             // crates/aria-plugin-abi/tests/abi_reference/render.rs. Edit the doc comments there, then run\n",
        );
        let _ = writeln!(out, "// `{REGENERATE}`.\n//");
        self.comment(&mut out, "aria_plugin_abi", "", "//", &self.docs);
        out.push_str(
            "\n#ifndef ARIA_PLUGIN_H\n#define ARIA_PLUGIN_H\n\n\
             #include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\n\
             #ifdef __cplusplus\nextern \"C\" {\n#endif\n",
        );
        let (symbol, entry) = self.entry();
        for decl in &self.decls {
            out.push('\n');
            match decl {
                Decl::Define(define) => {
                    self.comment(&mut out, &define.name, "", "///", &define.docs);
                    let _ = writeln!(out, "#define {} {}", define.name, define.value.c());
                }
                Decl::Struct(item) => {
                    self.comment(&mut out, &item.name, "", "///", &item.docs);
                    let _ = writeln!(out, "typedef struct {} {{", item.name);
                    for (index, field) in item.fields.iter().enumerate() {
                        if index > 0 {
                            out.push('\n');
                        }
                        let owner = format!("{}::{}", item.name, field.name);
                        self.comment(&mut out, &owner, "    ", "///", &field.docs);
                        let declaration = match &field.ty {
                            FieldType::Value(ty) => declare(ty, &field.name),
                            FieldType::Slot(sig) => sig.pointer(&field.name),
                        };
                        let _ = writeln!(out, "    {declaration};");
                    }
                    let _ = writeln!(out, "}} {};", item.name);
                }
                Decl::Alias(alias) => {
                    self.comment(&mut out, &alias.name, "", "///", &alias.docs);
                    let _ = writeln!(out, "typedef {};", alias.sig.pointer(&alias.name));
                    if alias.name == entry.name {
                        let _ = writeln!(out, "\n{};", entry.sig.function(symbol));
                    }
                }
            }
        }
        out.push_str("\n#ifdef __cplusplus\n}\n#endif\n\n#endif /* ARIA_PLUGIN_H */\n");
        out
    }

    fn reference(&self) -> String {
        let mut md = String::from("# Native ABI reference\n\n");
        let _ = writeln!(
            md,
            "<!-- Generated from crates/aria-plugin-abi/src/lib.rs by \
             crates/aria-plugin-abi/tests/abi_reference/render.rs. Edit the doc comments, then run \
             `{REGENERATE}`. -->\n"
        );
        for paragraph in self.paragraphs("aria_plugin_abi", &self.docs) {
            let _ = writeln!(md, "{paragraph}\n");
        }
        md.push_str(
            "[Native plugins](native.md) shows how to build, package and load a plugin. \
             `crates/aria-plugin-abi/include/aria_plugin.h` is generated from the same \
             source as this page.\n\n## Defines\n",
        );
        let mut groups: Vec<(&str, Vec<&Define>)> = Vec::new();
        for define in self.defines() {
            let group = define_group(&define.name).unwrap_or_else(|| {
                panic!(
                    "`{}` belongs to no define table; add a rule to DEFINE_GROUPS",
                    define.name
                )
            });
            match groups.iter_mut().find(|(name, _)| *name == group) {
                Some((_, members)) => members.push(define),
                None => groups.push((group, vec![define])),
            }
        }
        for (group, members) in groups {
            let _ = writeln!(
                md,
                "\n### {group}\n\n| Define | Value | Description |\n| --- | --- | --- |"
            );
            for define in members {
                let _ = writeln!(
                    md,
                    "| `{}` | `{}` | {} |",
                    define.name,
                    define.value.c(),
                    self.cell(&define.name, &define.docs)
                );
            }
        }
        md.push_str("\n## Types\n");
        let (symbol, entry) = self.entry();
        for decl in &self.decls {
            match decl {
                Decl::Struct(item) if !item.is_table() => {
                    let _ = writeln!(md, "\n### `{}`\n", item.name);
                    for paragraph in self.paragraphs(&item.name, &item.docs) {
                        let _ = writeln!(md, "{paragraph}\n");
                    }
                    self.field_table(&mut md, item);
                }
                Decl::Alias(alias) => {
                    let _ = writeln!(
                        md,
                        "\n### `{}`\n\n```c\ntypedef {};",
                        alias.name,
                        alias.sig.pointer(&alias.name)
                    );
                    if alias.name == entry.name {
                        let _ = writeln!(md, "{};", entry.sig.function(symbol));
                    }
                    md.push_str("```\n");
                    for paragraph in self.paragraphs(&alias.name, &alias.docs) {
                        let _ = write!(md, "\n{paragraph}\n");
                    }
                }
                _ => {}
            }
        }
        for table in self.structs().filter(|item| item.is_table()) {
            self.table_section(&mut md, table);
        }
        md
    }

    fn field_table(&self, md: &mut String, item: &Struct) {
        md.push_str("| Field | Type | Description |\n| --- | --- | --- |\n");
        for field in &item.fields {
            if let FieldType::Value(ty) = &field.ty {
                let owner = format!("{}::{}", item.name, field.name);
                let _ = writeln!(
                    md,
                    "| `{}` | `{ty}` | {} |",
                    field.name,
                    self.cell(&owner, &field.docs)
                );
            }
        }
    }

    fn table_section(&self, md: &mut String, table: &Struct) {
        let _ = writeln!(md, "\n## `{}`\n", table.name);
        for paragraph in self.paragraphs(&table.name, &table.docs) {
            let _ = writeln!(md, "{paragraph}\n");
        }
        self.field_table(md, table);
        let _ = writeln!(
            md,
            "\n### `{}` prefixes\n\nA table holds the slots of a prefix when its \
             `struct_size` is at least the prefix's size.\n\n| Prefix | Slots |\n| --- | --- |",
            table.name
        );
        let mut rows: Vec<(String, Vec<String>)> = Vec::new();
        for (index, field) in table.fields.iter().enumerate() {
            if !matches!(field.ty, FieldType::Slot(_)) {
                continue;
            }
            let prefix = self.prefix_of(table, index);
            let link = markdown_link(
                &field.name,
                &slug(&format!("`{}.{}`", table.name, field.name)),
            );
            match rows.last_mut() {
                Some((last, links)) if *last == prefix => links.push(link),
                _ => rows.push((prefix, vec![link])),
            }
        }
        for (prefix, links) in rows {
            let _ = writeln!(md, "| `{prefix}` | {} |", links.join(", "));
        }
        for (index, field) in table.fields.iter().enumerate() {
            let FieldType::Slot(sig) = &field.ty else {
                continue;
            };
            let prefix = self.prefix_of(table, index);
            let prefix = if prefix.starts_with("sizeof(") {
                format!("`{prefix}`")
            } else {
                markdown_link(&prefix, &slug("Table sizes"))
            };
            let _ = writeln!(
                md,
                "\n### `{}.{}`\n\n```c\n{};\n```\n\nPrefix: {prefix}.\n",
                table.name,
                field.name,
                sig.pointer(&field.name)
            );
            let owner = format!("{}::{}", table.name, field.name);
            let paragraphs = self.paragraphs(&owner, &field.docs);
            md.push_str(&paragraphs.join("\n\n"));
            md.push('\n');
        }
    }
}

pub(super) fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Write `rendered` to `path` under `ARIA_BLESS_DOCS`, otherwise fail when the
/// stored file differs from it.
fn check_generated(path: &Path, rendered: &str) {
    if std::env::var_os(BLESS).is_some() {
        std::fs::write(path, rendered)
            .unwrap_or_else(|err| panic!("write {}: {err}", path.display()));
        return;
    }
    let stored = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("read {}: {err}; run {REGENERATE}", path.display()));
    if stored != rendered {
        let line = stored
            .lines()
            .zip(rendered.lines())
            .position(|(old, new)| old != new)
            .unwrap_or_else(|| stored.lines().count().min(rendered.lines().count()));
        panic!(
            "{} differs from src/lib.rs from line {}: stored {:?}, rendered {:?}. Run \
             {REGENERATE} and review the diff.",
            path.display(),
            line + 1,
            stored.lines().nth(line),
            rendered.lines().nth(line)
        );
    }
}

#[test]
fn every_abi_item_has_documentation() {
    let abi = Abi::parse();
    let mut missing = Vec::new();
    if abi.docs.iter().all(|line| line.trim().is_empty()) {
        missing.push("the crate".to_owned());
    }
    for decl in &abi.decls {
        let (name, docs) = match decl {
            Decl::Define(define) => (&define.name, &define.docs),
            Decl::Struct(item) => (&item.name, &item.docs),
            Decl::Alias(alias) => (&alias.name, &alias.docs),
        };
        if docs.iter().all(|line| line.trim().is_empty()) {
            missing.push(name.clone());
        }
        if let Decl::Struct(item) = decl {
            for field in &item.fields {
                if field.docs.iter().all(|line| line.trim().is_empty()) {
                    missing.push(format!("{name}.{}", field.name));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "src/lib.rs ABI items without a `///` comment:\n{}",
        missing.join("\n")
    );
}

#[test]
fn header_and_reference_match_lib_rs() {
    let abi = Abi::parse();
    check_generated(&crate_dir().join("include/aria_plugin.h"), &abi.header());
    check_generated(
        &crate_dir().join("../../docs/plugins/native-abi-reference.md"),
        &abi.reference(),
    );
}

#[test]
fn every_table_size_doc_names_the_last_slot_it_covers() {
    let abi = Abi::parse();
    let mut checked = 0;
    for define in abi.defines() {
        let DefineValue::Offset { table, slot } = &define.value else {
            continue;
        };
        let table = abi.find_struct(table).unwrap_or_else(|| {
            panic!(
                "`{}` measures `{table}`, which is not declared",
                define.name
            )
        });
        let index = table
            .fields
            .iter()
            .position(|field| field.name == *slot)
            .unwrap_or_else(|| {
                panic!(
                    "`{}` names `{slot}`, which `{}` lacks",
                    define.name, table.name
                )
            });
        assert!(
            index > 0,
            "`{}` covers no field of `{}`",
            define.name,
            table.name
        );
        let last = &table.fields[index - 1].name;
        let text = define.docs.join(" ");
        let named = text
            .split_once("through `")
            .and_then(|(_, rest)| rest.split_once('`'))
            .map(|(name, _)| name)
            .unwrap_or_else(|| {
                panic!(
                    "the doc comment of `{}` does not name its last slot as \"through `<slot>`\"",
                    define.name
                )
            });
        assert_eq!(
            named, last,
            "`{}` ends before `{slot}`, so the last slot it covers is `{last}`",
            define.name
        );
        checked += 1;
    }
    assert!(checked > 0, "src/lib.rs declares no table size");
}
