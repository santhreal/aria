//! Popover / menu primitives.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt

use std::time::Duration;

use gpui::motion::presets;

pub use crate::types::Loadable;

pub struct Popup<T> {
    inner: Option<(T, Option<std::time::Instant>)>,
    pressed_while_open: bool,
}

impl<T> Default for Popup<T> {
    fn default() -> Self {
        Self {
            inner: None,
            pressed_while_open: false,
        }
    }
}

impl<T> Popup<T> {
    pub fn open(&mut self, value: T) {
        self.inner = Some((value, None));
    }

    pub fn is_open(&self) -> bool {
        matches!(self.inner, Some((_, None)))
    }

    pub fn is_closing(&self) -> bool {
        matches!(self.inner, Some((_, Some(_))))
    }

    pub fn closing_since(&self) -> Option<std::time::Instant> {
        match &self.inner {
            Some((_, Some(since))) => Some(*since),
            _ => None,
        }
    }

    pub fn get(&self) -> Option<&T> {
        self.inner.as_ref().map(|(value, _)| value)
    }

    pub fn as_open(&self) -> Option<&T> {
        match &self.inner {
            Some((value, None)) => Some(value),
            _ => None,
        }
    }

    pub fn open_mut(&mut self) -> Option<&mut T> {
        match &mut self.inner {
            Some((value, None)) => Some(value),
            _ => None,
        }
    }

    pub fn begin_close(&mut self) -> bool {
        match &mut self.inner {
            Some((_, closing @ None)) => {
                *closing = Some(std::time::Instant::now());
                true
            }
            _ => false,
        }
    }

    pub fn note_trigger_press(&mut self) {
        self.note_trigger_press_matching(|_| true);
    }

    pub fn note_trigger_press_matching(&mut self, owns: impl FnOnce(&T) -> bool) {
        self.pressed_while_open = self.inner.as_ref().is_some_and(|(value, _)| owns(value));
    }

    pub fn take_press_was_open(&mut self) -> bool {
        std::mem::take(&mut self.pressed_while_open)
    }

    /// Drops a closing popup once `exit`, the length of its exit animation,
    /// has passed since [`Self::begin_close`].
    pub fn finish_close(&mut self, exit: Duration) {
        if let Some((_, Some(since))) = &self.inner
            && since.elapsed() >= exit
        {
            self.inner = None;
        }
    }
}

pub fn reap_popup<V: 'static, T: 'static>(
    cx: &mut gpui::Context<V>,
    popup: impl Fn(&mut V) -> &mut Popup<T> + 'static,
) {
    let exit = cx.motion_policy().scale(presets::MENU_OUT.total());
    cx.spawn(async move |view, cx| {
        cx.background_executor()
            .timer(exit.saturating_add(Duration::from_millis(20)))
            .await;
        view.update(cx, |view, cx| {
            popup(view).finish_close(exit);
            cx.notify();
        })
        .ok();
    })
    .detach();
}

pub fn menu_step(active: Option<usize>, count: usize, delta: isize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let count_i = count as isize;
    let next = match active {
        None => {
            if delta >= 0 {
                0
            } else {
                count_i - 1
            }
        }
        Some(at) => (at as isize + delta).rem_euclid(count_i),
    };
    Some(next as usize)
}

/// How well `label` matches `query`, lower being better. `None` is no match.
///
/// Four tiers, because a palette that ranks only prefix and substring buries the
/// intended row: typing `sav` should offer **Save** before **Autosave
/// interval**, and typing `fs` should still find **Fullscreen preview**.
///
/// 0. the label starts with the query,
/// 1. a word in the label starts with the query,
/// 2. the query appears anywhere in the label,
/// 3. the query's characters appear in order, as initials or otherwise.
pub fn match_rank(query: &str, label: &str) -> Option<usize> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Some(1);
    }
    let label = label.to_lowercase();
    if label.starts_with(&query) {
        return Some(0);
    }
    let word_start = label
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.starts_with(&query));
    if word_start {
        return Some(1);
    }
    if label.contains(&query) {
        return Some(2);
    }
    if is_subsequence(&query, &label) {
        return Some(3);
    }
    None
}

/// Whether every character of `needle` appears in `haystack`, in order.
fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut chars = haystack.chars();
    needle.chars().all(|want| chars.any(|have| have == want))
}

pub fn filter_indices<S: AsRef<str>>(query: &str, labels: &[S]) -> Vec<usize> {
    let mut ranked: Vec<(usize, usize)> = labels
        .iter()
        .enumerate()
        .filter_map(|(ix, label)| match_rank(query, label.as_ref()).map(|rank| (rank, ix)))
        .collect();
    ranked.sort_by_key(|&(rank, ix)| (rank, ix));
    ranked.into_iter().map(|(_, ix)| ix).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKey {
    Up,
    Down,
    Enter,
    ModEnter,
    Escape,
    Backspace,
    Other,
}

pub fn classify_key(key: &str, cmd: bool, ctrl: bool) -> MenuKey {
    match key {
        "up" => MenuKey::Up,
        "down" => MenuKey::Down,
        "n" if ctrl => MenuKey::Down,
        "p" if ctrl => MenuKey::Up,
        "enter" if cmd || ctrl => MenuKey::ModEnter,
        "enter" => MenuKey::Enter,
        "escape" => MenuKey::Escape,
        "backspace" => MenuKey::Backspace,
        _ => MenuKey::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_open_is_open() {
        let mut popup = Popup::default();
        popup.open("menu");
        assert!(popup.is_open());
        assert!(!popup.is_closing());
        assert_eq!(popup.get(), Some(&"menu"));
    }

    #[test]
    fn popup_begin_close_marks_closing() {
        let mut popup = Popup::default();
        popup.open(1u8);
        assert!(popup.begin_close());
        assert!(popup.is_closing());
        assert!(!popup.is_open());
        assert!(popup.get().is_some());
    }

    #[test]
    fn popup_finish_close_after_menu_out() {
        let exit = presets::MENU_OUT.total();
        let mut popup = Popup::default();
        popup.open(1u8);
        popup.begin_close();
        popup.finish_close(exit);
        assert!(popup.get().is_some(), "the exit has not been spent yet");
        if let Some((_, Some(since))) = &mut popup.inner {
            *since = std::time::Instant::now() - exit - Duration::from_millis(1);
        }
        popup.finish_close(exit);
        assert!(popup.get().is_none());
    }

    #[test]
    fn menu_step_enters_at_top() {
        assert_eq!(menu_step(None, 5, 1), Some(0));
    }

    #[test]
    fn menu_step_wraps() {
        assert_eq!(menu_step(Some(2), 5, 1), Some(3));
        assert_eq!(menu_step(Some(4), 5, 1), Some(0));
        assert_eq!(menu_step(Some(0), 5, -1), Some(4));
    }

    #[test]
    fn match_rank_orders_prefix_word_substring_then_subsequence() {
        assert_eq!(match_rank("split", "SplitClip"), Some(0));
        assert_eq!(match_rank("clip", "Split Clip"), Some(1), "a word start");
        assert_eq!(match_rank("clip", "SplitClip"), Some(2), "mid-label");
        assert_eq!(match_rank("fs", "Fullscreen preview"), Some(3), "in order");
        assert_eq!(match_rank("zzz", "SplitClip"), None);
    }

    #[test]
    fn a_word_start_beats_a_later_substring() {
        let labels = ["Autosave interval", "Save project"];
        assert_eq!(
            filter_indices("save", &labels),
            vec![1, 0],
            "the row that starts with the query comes first"
        );
    }

    #[test]
    fn a_subsequence_is_offered_last() {
        let labels = ["Fullscreen preview", "Fit preview"];
        assert_eq!(filter_indices("fi", &labels), vec![1, 0]);
    }

    #[test]
    fn filter_indices_ranks_prefix_first() {
        let labels = ["SplitClip", "RippleDelete", "DuplicateClip"];
        assert_eq!(filter_indices("clip", &labels), vec![0, 2]);
        assert_eq!(filter_indices("dup", &labels), vec![2]);
    }

    #[test]
    fn classify_key_enter() {
        assert_eq!(classify_key("enter", false, false), MenuKey::Enter);
        assert_eq!(classify_key("enter", true, false), MenuKey::ModEnter);
        assert_eq!(classify_key("escape", false, false), MenuKey::Escape);
        assert_eq!(classify_key("n", false, true), MenuKey::Down);
        assert_eq!(classify_key("p", false, true), MenuKey::Up);
    }
}
