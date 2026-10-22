use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::render::crate_dir;
use super::*;

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
