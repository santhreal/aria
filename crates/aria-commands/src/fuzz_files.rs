//! Damaged forms of a saved project file: what a file edited by hand, by a
//! script, or cut short holds that no edit writes.

use serde_json::{Value, json};

/// The values each value of a file is replaced by in turn: every JSON kind,
/// the integer and float extremes, zero, a fraction, and the empty forms.
fn hostile() -> [Value; 11] {
    [
        Value::Null,
        json!(true),
        json!(-1),
        json!(0),
        json!(i64::MAX),
        json!(u64::MAX),
        json!(-1.0e308),
        json!(0.5),
        json!(""),
        json!([]),
        json!({}),
    ]
}

/// The JSON pointer of every value under `value`, `at` included, children
/// first.
pub fn pointers(value: &Value, at: String, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let key = key.replace('~', "~0").replace('/', "~1");
                pointers(child, format!("{at}/{key}"), out);
            }
        }
        Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                pointers(child, format!("{at}/{index}"), out);
            }
        }
        _ => {}
    }
    out.push(at);
}

/// Hand `visit` every damaged form of `doc`, one at a time, with a line
/// stating the damage: each value below the root replaced in turn by each
/// hostile value, and each key and element removed in turn. Returns how many
/// values were damaged, so a caller can reject a document too small to
/// exercise anything.
///
/// The values come from `doc`, so a field a new version of the file adds is
/// damaged with no edit here.
pub fn for_each_damaged_file(doc: &Value, mut visit: impl FnMut(String, &Value)) -> usize {
    let mut all = Vec::new();
    pointers(doc, String::new(), &mut all);
    all.retain(|pointer| !pointer.is_empty());
    let hostile = hostile();
    for pointer in &all {
        for value in &hostile {
            let mut edited = doc.clone();
            if let Some(slot) = edited.pointer_mut(pointer) {
                *slot = value.clone();
                visit(format!("{pointer} = {value}"), &edited);
            }
        }
        let Some((parent, last)) = pointer.rsplit_once('/') else {
            continue;
        };
        let mut edited = doc.clone();
        let removed = match edited.pointer_mut(parent) {
            Some(Value::Object(map)) => map
                .remove(&last.replace("~1", "/").replace("~0", "~"))
                .is_some(),
            Some(Value::Array(items)) => match last.parse::<usize>() {
                Ok(index) if index < items.len() => {
                    items.remove(index);
                    true
                }
                _ => false,
            },
            _ => false,
        };
        if removed {
            visit(format!("{pointer} removed"), &edited);
        }
    }
    all.len()
}

#[cfg(test)]
#[path = "fuzz_files_tests.rs"]
mod tests;
