//! Text held for the process, once per distinct value.
//!
//! The menu, key and settings tables are `Copy` rows of `&'static str`, so a
//! row a plugin adds at run time needs its id and label for the process too. Each distinct
//! text is allocated once and returned on every later request, so memory grows
//! with the number of distinct ids and titles plugins declare, not with reloads
//! or redraws.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use parking_lot::Mutex;

static HELD: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Mutex::default);

/// Joined text by its two parts, so a caller that joins the same pair again
/// finds it without building the text to look it up.
static JOINED: LazyLock<Mutex<HashMap<(&'static str, &'static str), &'static str>>> =
    LazyLock::new(Mutex::default);

/// The process's copy of `text`.
pub(crate) fn intern(text: &str) -> &'static str {
    let mut held = HELD.lock();
    if let Some(found) = held.get(text) {
        return found;
    }
    let owned: &'static str = Box::leak(Box::from(text));
    held.insert(owned);
    owned
}

/// The process's copy of `prefix` followed by `rest`.
///
/// A menu row id made of a fixed prefix and a panel id is asked for every time
/// the menu is built. The pair is looked up as it stands, so only the first
/// request for a pair allocates. The text is held through [`intern`], so the
/// joined id and the same text interned whole are one allocation.
pub(crate) fn joined(prefix: &'static str, rest: &'static str) -> &'static str {
    let mut held = JOINED.lock();
    if let Some(found) = held.get(&(prefix, rest)) {
        return found;
    }
    let mut text = String::with_capacity(prefix.len() + rest.len());
    text.push_str(prefix);
    text.push_str(rest);
    let owned = intern(&text);
    held.insert((prefix, rest), owned);
    owned
}

#[cfg(test)]
mod tests {
    use super::{intern, joined};

    /// Equal text returns one allocation, so a redraw or reload that asks again
    /// allocates nothing.
    #[test]
    fn equal_text_is_held_once() {
        let first = intern("test.interned.title");
        let again = intern(&String::from("test.interned.title"));
        assert!(std::ptr::eq(first, again));
        assert_eq!(first, "test.interned.title");
        assert!(!std::ptr::eq(first, intern("test.interned.other")));
    }

    /// A joined id is its prefix followed by the rest, held once, and the same
    /// allocation as that text interned whole or joined at another split, so a
    /// row looked up by its full id is the row the menu built.
    #[test]
    fn a_joined_id_is_its_parts_held_once() {
        let first = joined("test.joined:", "panel");
        assert_eq!(first, "test.joined:panel");
        assert!(std::ptr::eq(first, joined("test.joined:", "panel")));
        assert!(std::ptr::eq(first, intern("test.joined:panel")));
        assert!(std::ptr::eq(first, joined("test.joined:p", "anel")));
        let other = joined("test.joined:", "other");
        assert_eq!(other, "test.joined:other");
        assert!(!std::ptr::eq(first, other));
    }
}
