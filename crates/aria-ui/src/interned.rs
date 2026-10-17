//! Text held for the process, once per distinct value.
//!
//! The menu, key and settings tables are `Copy` rows of `&'static str`, so a
//! row a plugin adds at run time needs its id and label for the process too. Each distinct
//! text is allocated once and returned on every later request, so memory grows
//! with the number of distinct ids and titles plugins declare, not with reloads
//! or redraws.

use std::collections::HashSet;
use std::sync::LazyLock;

use parking_lot::Mutex;

static HELD: LazyLock<Mutex<HashSet<&'static str>>> = LazyLock::new(Mutex::default);

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

#[cfg(test)]
mod tests {
    use super::intern;

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
}
