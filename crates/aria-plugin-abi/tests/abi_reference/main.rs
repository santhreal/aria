//! `include/aria_plugin.h` and `docs/plugins/native-abi-reference.md` are
//! generated from the crate source under `src/`. The doc comments there are the single
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

mod names_and_docs;
mod render;

use std::collections::BTreeSet;

use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, ExprLit, Item, Lit, ReturnType, Type, Visibility};

const SOURCE: &str = include_str!("../../src/lib.rs");
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
/// helpers free Rust allocations only), or a private constant, struct, or type.
fn outside_the_abi(item: &Item) -> bool {
    match item {
        Item::Fn(_) => true,
        Item::Const(item) => !is_pub(&item.vis),
        Item::Struct(item) => !is_pub(&item.vis),
        Item::Type(item) => !is_pub(&item.vis),
        _ => false,
    }
}

fn is_cfg_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Ident>()
                .is_ok_and(|ident| ident == "test")
    })
}

fn module_reexport(item: &syn::ItemUse) -> Option<String> {
    if !is_pub(&item.vis) || item.leading_colon.is_some() {
        return None;
    }
    let syn::UseTree::Path(path) = &item.tree else {
        return None;
    };
    if !matches!(*path.tree, syn::UseTree::Glob(_)) {
        return None;
    }
    Some(path.ident.to_string())
}

fn allowed_submodule_use(item: &syn::ItemUse) -> bool {
    if is_pub(&item.vis) {
        return false;
    }
    match &item.tree {
        syn::UseTree::Path(path) => matches!(
            path.ident.to_string().as_str(),
            "super" | "crate" | "std" | "core"
        ),
        _ => false,
    }
}

/// Expand `src/lib.rs` into `(file_label, item)` pairs in declaration order,
/// splicing each non-test `mod <name>;` from `src/<name>.rs` in place.
fn expand_crate_items(
    root_source: &str,
    src_dir: &std::path::Path,
) -> (Vec<String>, Vec<(String, Item)>) {
    let file = syn::parse_file(root_source).unwrap_or_else(|err| panic!("src/lib.rs: {err}"));
    let mut modules = BTreeSet::new();
    let mut reexported = BTreeSet::new();
    let mut items = Vec::new();
    for item in file.items {
        match item {
            Item::Mod(item) if is_cfg_test(&item.attrs) => {}
            Item::Mod(item) => {
                let mod_name = item.ident.to_string();
                assert!(
                    !is_pub(&item.vis) && item.content.is_none(),
                    "src/lib.rs module `{mod_name}` must be a private non-inline `mod {mod_name};`"
                );
                let rel = format!("src/{mod_name}.rs");
                let path = src_dir.join(format!("{mod_name}.rs"));
                let mod_src = std::fs::read_to_string(&path).unwrap_or_else(|err| {
                    panic!(
                        "src/lib.rs declares `mod {mod_name};`, failed to read {rel} ({}): {err}",
                        path.display()
                    )
                });
                let mod_file =
                    syn::parse_file(&mod_src).unwrap_or_else(|err| panic!("{rel}: {err}"));
                for sub_item in mod_file.items {
                    match sub_item {
                        Item::Use(use_item) if allowed_submodule_use(&use_item) => {}
                        Item::Use(_) => panic!(
                            "{rel} declares an unexpected use item; only private `use` imports \
                             are allowed in ABI submodules"
                        ),
                        Item::Mod(nested) => panic!(
                            "{rel} declares nested module `{}`; ABI submodules must not declare \
                             submodules",
                            nested.ident
                        ),
                        other => items.push((rel.clone(), other)),
                    }
                }
                modules.insert(mod_name);
            }
            Item::Use(item) if is_cfg_test(&item.attrs) => {}
            Item::Use(item) => {
                let Some(name) = module_reexport(&item) else {
                    panic!(
                        "src/lib.rs declares a use item other than a `pub use <module>::*;` \
                         re-export"
                    );
                };
                reexported.insert(name);
            }
            other => items.push(("src/lib.rs".to_owned(), other)),
        }
    }
    for name in &reexported {
        assert!(
            modules.contains(name),
            "src/lib.rs declares `pub use {name}::*;` for an undeclared module `{name}`"
        );
    }
    for name in &modules {
        assert!(
            reexported.contains(name),
            "src/lib.rs declares `mod {name};` without `pub use {name}::*;`"
        );
    }
    (docs(&file.attrs), items)
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
