//! `include/aria_plugin.h` and `docs/plugins/native-abi-reference.md` are
//! generated from `src/lib.rs`. The doc comments there are the single
//! definition of what every define, type, field and table slot of the native
//! ABI means; rustdoc, the C header and the reference contain the same text.
//!
//! The suite fails when the checked-in header or reference differs from the
//! source, when a define, type, field or slot has no doc comment, when a doc
//! comment links an item the C ABI does not declare, when `src/lib.rs` gains a
//! public item with no C rendering rule, when a table size's doc names the
//! wrong last slot, when the header declares a name that C or C++ reserves,
//! and when a chapter under `docs/` names a define or a table member that the
//! source does not declare. It does not check that a comment is true, and it
//! does not compile the header.
//!
//! Regenerate both files with
//! `ARIA_BLESS_DOCS=1 cargo test -p aria-plugin-abi --test abi_reference`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Fields, ImplItem, Item, Lit, ReturnType, Type, Visibility};

const SOURCE: &str = include_str!("../src/lib.rs");
const BLESS: &str = "ARIA_BLESS_DOCS";
const REGENERATE: &str = "ARIA_BLESS_DOCS=1 cargo test -p aria-plugin-abi --test abi_reference";

/// The define that holds the entry function's name, and the alias that types it.
const ENTRY: (&str, &str) = ("ARIA_PLUGIN_ENTRY_SYMBOL", "AriaPluginEntryFn");

/// The define tables of the reference. A rule ending in `_` heads every define
/// with that prefix; any other rule heads the one define of that name.
const DEFINE_GROUPS: &[(&str, &str)] = &[
    ("ARIA_API_MAJOR", "Version and entry"),
    ("ARIA_PLUGIN_ENTRY_SYMBOL", "Version and entry"),
    ("ARIA_STATUS_", "Status codes"),
    ("ARIA_INSTANCE_", "Instance status codes"),
    ("ARIA_BYTES_", "Bytes status codes"),
    ("ARIA_LOG_", "Log levels"),
    ("ARIA_FORMAT_", "Pixel formats"),
    ("ARIA_COLOR_", "Color encodings"),
    ("ARIA_PARAM_", "Parameter kinds"),
    ("ARIA_HOST_V1_", "Table sizes"),
    ("ARIA_PLUGIN_V1_", "Table sizes"),
];

/// The heading of the define table that lists `name`.
fn define_group(name: &str) -> Option<&'static str> {
    DEFINE_GROUPS.iter().find_map(|(rule, group)| {
        let hit = if rule.ends_with('_') {
            name.starts_with(rule)
        } else {
            name == *rule
        };
        hit.then_some(*group)
    })
}

/// A function signature as C spells its parts.
struct Signature {
    ret: String,
    params: Vec<(String, String)>,
}

impl Signature {
    fn params(&self) -> String {
        if self.params.is_empty() {
            return "void".to_owned();
        }
        self.params
            .iter()
            .map(|(ty, name)| declare(ty, name))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// A pointer to a function of this signature, named `name`.
    fn pointer(&self, name: &str) -> String {
        declare(&self.ret, &format!("(*{name})({})", self.params()))
    }

    /// A function of this signature named `name`.
    fn function(&self, name: &str) -> String {
        declare(&self.ret, &format!("{name}({})", self.params()))
    }
}

enum FieldType {
    /// A C type, such as `const uint8_t *`.
    Value(String),
    /// A function pointer: a slot of a function table.
    Slot(Signature),
}

struct Field {
    name: String,
    docs: Vec<String>,
    ty: FieldType,
}

enum DefineValue {
    /// A C integer literal, such as `1u`.
    Number(String),
    /// Another define.
    Alias(String),
    /// `offsetof(table, slot)`.
    Offset { table: String, slot: String },
    /// A C string literal's contents.
    Text(String),
}

impl DefineValue {
    fn c(&self) -> String {
        match self {
            Self::Number(text) | Self::Alias(text) => text.clone(),
            Self::Offset { table, slot } => format!("offsetof({table}, {slot})"),
            Self::Text(text) => format!("\"{text}\""),
        }
    }
}

struct Define {
    name: String,
    docs: Vec<String>,
    value: DefineValue,
}

struct Struct {
    name: String,
    docs: Vec<String>,
    fields: Vec<Field>,
    /// Public Rust methods, which chapters may name as `Struct::method`.
    methods: BTreeSet<String>,
}

impl Struct {
    /// Whether the struct is a function table: it holds a function pointer.
    fn is_table(&self) -> bool {
        self.fields
            .iter()
            .any(|field| matches!(field.ty, FieldType::Slot(_)))
    }
}

struct FnAlias {
    name: String,
    docs: Vec<String>,
    sig: Signature,
}

enum Decl {
    Define(Define),
    Struct(Struct),
    Alias(FnAlias),
}

/// The C-facing declarations of `src/lib.rs`, in source order.
struct Abi {
    docs: Vec<String>,
    decls: Vec<Decl>,
}

fn is_pub(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}

/// The lines of the doc comments in `attrs`.
fn docs(attrs: &[Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            syn::Meta::NameValue(pair) => match &pair.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(text),
                    ..
                }) => Some(text.value()),
                _ => None,
            },
            _ => None,
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(&line).to_owned())
        .collect()
}

fn repr_c(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("repr")
            && attr
                .parse_args::<syn::Ident>()
                .is_ok_and(|repr| repr == "C")
    })
}

/// The name of a path type without generic arguments, such as `u32` or `c_void`.
fn type_name(ty: &Type) -> Option<String> {
    let Type::Path(path) = ty else { return None };
    let last = path.path.segments.last()?;
    (path.qself.is_none() && last.arguments.is_none()).then(|| last.ident.to_string())
}

fn is_byte_slice(ty: &Type) -> bool {
    let Type::Reference(reference) = ty else {
        return false;
    };
    let Type::Slice(slice) = &*reference.elem else {
        return false;
    };
    reference.mutability.is_none() && type_name(&slice.elem).as_deref() == Some("u8")
}

/// `ty` as C spells it. `declared` holds the types declared before `owner`.
fn c_type(ty: &Type, owner: &str, declared: &BTreeSet<String>) -> String {
    if let Type::Ptr(pointer) = ty {
        assert!(
            !matches!(*pointer.elem, Type::Ptr(_)),
            "`{owner}` has a pointer to a pointer, which has no C rendering rule"
        );
        let pointee = c_type(&pointer.elem, owner, declared);
        return if pointer.mutability.is_some() {
            format!("{pointee} *")
        } else {
            format!("const {pointee} *")
        };
    }
    let name =
        type_name(ty).unwrap_or_else(|| panic!("`{owner}` has a type with no C rendering rule"));
    let scalar = match name.as_str() {
        "u8" => "uint8_t",
        "u16" => "uint16_t",
        "u32" => "uint32_t",
        "u64" => "uint64_t",
        "i8" => "int8_t",
        "i16" => "int16_t",
        "i32" => "int32_t",
        "i64" => "int64_t",
        "usize" => "size_t",
        "f32" => "float",
        "f64" => "double",
        "c_void" => "void",
        _ => {
            assert!(
                declared.contains(&name),
                "`{owner}` uses `{name}`, which is neither a C scalar nor a type declared before it"
            );
            return name;
        }
    };
    scalar.to_owned()
}

fn signature(bare: &syn::TypeBareFn, owner: &str, declared: &BTreeSet<String>) -> Signature {
    let extern_c = bare
        .abi
        .as_ref()
        .and_then(|abi| abi.name.as_ref())
        .is_some_and(|name| name.value() == "C");
    assert!(
        bare.unsafety.is_some() && extern_c && bare.variadic.is_none(),
        "`{owner}` is not an `unsafe extern \"C\" fn` without variadic parameters"
    );
    let params = bare
        .inputs
        .iter()
        .map(|arg| {
            let (name, _) = arg
                .name
                .as_ref()
                .unwrap_or_else(|| panic!("a parameter of `{owner}` has no name"));
            (c_type(&arg.ty, owner, declared), name.to_string())
        })
        .collect();
    let ret = match &bare.output {
        ReturnType::Default => "void".to_owned(),
        ReturnType::Type(_, ty) => c_type(ty, owner, declared),
    };
    Signature { ret, params }
}

fn define_value(name: &str, ty: &Type, expr: &Expr, defines: &BTreeSet<String>) -> DefineValue {
    match (type_name(ty).as_deref(), expr) {
        (
            Some("u32"),
            Expr::Lit(ExprLit {
                lit: Lit::Int(int), ..
            }),
        ) => DefineValue::Number(format!("{}u", int.base10_digits())),
        (Some("u32"), Expr::Path(path)) => {
            let target = path
                .path
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            assert!(
                defines.contains(&target),
                "`{name}` is not a literal or a define declared before it"
            );
            DefineValue::Alias(target)
        }
        (Some("usize"), Expr::Macro(mac))
            if mac
                .mac
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "offset_of") =>
        {
            let args = mac
                .mac
                .parse_body_with(Punctuated::<syn::Ident, syn::Token![,]>::parse_terminated)
                .unwrap_or_else(|err| panic!("`{name}` is not `offset_of!(Table, slot)`: {err}"));
            let [table, slot]: [String; 2] = args
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap_or_else(|_| panic!("`{name}` is not `offset_of!(Table, slot)`"));
            DefineValue::Offset { table, slot }
        }
        (
            None,
            Expr::Lit(ExprLit {
                lit: Lit::ByteStr(bytes),
                ..
            }),
        ) if is_byte_slice(ty) => {
            let text = String::from_utf8(bytes.value()).unwrap_or_default();
            assert!(
                !text.is_empty()
                    && text
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
                "`{name}` is not a C identifier, so it has no C string rendering rule"
            );
            DefineValue::Text(text)
        }
        _ => panic!(
            "`{name}` has no C rendering rule: a define is a `u32` literal, another `u32` \
             define, a `usize` `offset_of!`, or a `&[u8]` byte string"
        ),
    }
}

/// Whether `item` stays out of the C ABI: a function (the `free_rust_*`
/// helpers free Rust allocations only), or a private item.
fn outside_the_abi(item: &Item) -> bool {
    match item {
        Item::Fn(_) => true,
        Item::Const(item) => !is_pub(&item.vis),
        Item::Struct(item) => !is_pub(&item.vis),
        Item::Type(item) => !is_pub(&item.vis),
        Item::Use(item) => !is_pub(&item.vis),
        Item::Mod(item) => !is_pub(&item.vis),
        _ => false,
    }
}

/// `name` declared with type `ty`, as C spells it.
fn declare(ty: &str, name: &str) -> String {
    if ty.ends_with('*') {
        format!("{ty}{name}")
    } else {
        format!("{ty} {name}")
    }
}

/// The anchor GitHub gives a heading.
fn slug(heading: &str) -> String {
    heading
        .chars()
        .filter_map(|ch| match ch {
            ' ' => Some('-'),
            '-' | '_' => Some(ch),
            ch if ch.is_alphanumeric() => Some(ch.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

fn markdown_link(text: &str, anchor: &str) -> String {
    format!("[`{text}`](#{anchor})")
}

fn c_link(text: &str, _anchor: &str) -> String {
    format!("`{text}`")
}

impl Abi {
    fn parse() -> Self {
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
                    "src/lib.rs declares a public item with no rule in tests/abi_reference.rs: \
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

    fn defines(&self) -> impl Iterator<Item = &Define> {
        self.decls.iter().filter_map(|decl| match decl {
            Decl::Define(define) => Some(define),
            _ => None,
        })
    }

    fn structs(&self) -> impl Iterator<Item = &Struct> {
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

    fn entry(&self) -> (&str, &FnAlias) {
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
             // crates/aria-plugin-abi/tests/abi_reference.rs. Edit the doc comments there, then run\n",
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
             crates/aria-plugin-abi/tests/abi_reference.rs. Edit the doc comments, then run \
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

fn crate_dir() -> PathBuf {
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

/// Keywords of C23 and C++23, the alternative operator spellings of C++ and
/// `<iso646.h>`, and the macros of the headers `aria_plugin.h` includes. A
/// header name spelled as one fails to parse in C or in C++. Names starting
/// with `_` or holding `__` are reserved to the implementation and are
/// rejected by rule, so the `_Keyword` forms are not listed.
const C_RESERVED: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "char16_t",
    "char32_t",
    "char8_t",
    "class",
    "co_await",
    "co_return",
    "co_yield",
    "compl",
    "concept",
    "const",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "continue",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "typeof",
    "typeof_unqual",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
    "NULL",
    "offsetof",
];

impl Abi {
    /// Every name the header declares, with the declaration it appears in.
    fn header_names(&self) -> Vec<(String, &str)> {
        fn params<'a>(out: &mut Vec<(String, &'a str)>, owner: &str, sig: &'a Signature) {
            for (_, name) in &sig.params {
                out.push((format!("parameter of `{owner}`"), name.as_str()));
            }
        }
        let mut names = Vec::new();
        for decl in &self.decls {
            match decl {
                Decl::Define(define) => names.push(("define".to_owned(), define.name.as_str())),
                Decl::Struct(item) => {
                    names.push(("struct".to_owned(), item.name.as_str()));
                    for field in &item.fields {
                        names.push((format!("field of `{}`", item.name), field.name.as_str()));
                        if let FieldType::Slot(sig) = &field.ty {
                            params(&mut names, &format!("{}.{}", item.name, field.name), sig);
                        }
                    }
                }
                Decl::Alias(alias) => {
                    names.push(("function type".to_owned(), alias.name.as_str()));
                    params(&mut names, &alias.name, &alias.sig);
                }
            }
        }
        names.push(("entry function".to_owned(), self.entry().0));
        names
    }
}

/// C and C++ plugins both include `aria_plugin.h`. A slot named `export` or a
/// parameter named `this` compiles as C and fails as C++. The check reads
/// every name the header emits, so a new define, struct, field, slot or
/// parameter is checked as soon as `src/lib.rs` declares it. It does not catch
/// a clash with a macro of a header that a plugin includes after this one.
#[test]
fn every_header_name_is_valid_in_c_and_cpp() {
    let abi = Abi::parse();
    let names = abi.header_names();
    let mut bad = Vec::new();
    for (place, name) in &names {
        let mut chars = name.chars();
        let ascii_identifier = chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
        let reason = if !ascii_identifier {
            "is not an ASCII C identifier"
        } else if name.starts_with('_') || name.contains("__") {
            "is reserved to the C and C++ implementation"
        } else if C_RESERVED.contains(name) {
            "is a C or C++ keyword or standard macro"
        } else {
            continue;
        };
        bad.push(format!("{place}: `{name}` {reason}"));
    }
    assert!(
        names.len() > 100,
        "the header declares only {} names",
        names.len()
    );
    assert!(
        bad.is_empty(),
        "src/lib.rs declares names that aria_plugin.h cannot hold:\n{}",
        bad.join("\n")
    );
}

fn markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()))
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<_, _>>()
        .unwrap_or_else(|err| panic!("read {}: {err}", dir.display()));
    entries.sort();
    for path in entries {
        if path.is_dir() {
            markdown_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "md") {
            out.push(path);
        }
    }
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[test]
fn docs_name_only_declared_defines_and_table_members() {
    let abi = Abi::parse();
    let defines: BTreeSet<&str> = abi.defines().map(|define| define.name.as_str()).collect();
    let members: BTreeMap<&str, BTreeSet<&str>> = abi
        .structs()
        .map(|item| {
            let names = item
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .chain(item.methods.iter().map(String::as_str))
                .collect();
            (item.name.as_str(), names)
        })
        .collect();
    let docs = crate_dir().join("../../docs");
    let mut files = Vec::new();
    markdown_files(&docs, &mut files);
    let mut unknown = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            if !is_word(bytes[at]) {
                at += 1;
                continue;
            }
            let start = at;
            while at < bytes.len() && is_word(bytes[at]) {
                at += 1;
            }
            let word = &text[start..at];
            let line = text[..start].matches('\n').count() + 1;
            let place = || {
                format!(
                    "{}:{line}",
                    path.strip_prefix(&docs).unwrap_or(path).display()
                )
            };
            // A name ending in `_` stands for a family, such as `ARIA_STATUS_*`.
            if define_group(word).is_some() && !word.ends_with('_') && !defines.contains(word) {
                unknown.push(format!("{}: `{word}`", place()));
            }
            if let Some(known) = members.get(word) {
                let rest = &text[at..];
                let Some(after) = rest.strip_prefix("::").or_else(|| rest.strip_prefix('.')) else {
                    continue;
                };
                let member: String = after
                    .bytes()
                    .take_while(|byte| is_word(*byte))
                    .map(char::from)
                    .collect();
                if !member.is_empty() && !known.contains(member.as_str()) {
                    unknown.push(format!("{}: `{word}.{member}`", place()));
                }
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "docs name ABI items that src/lib.rs does not declare:\n{}",
        unknown.join("\n")
    );
}
