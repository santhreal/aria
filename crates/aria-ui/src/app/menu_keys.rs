//! The menu bar from the keyboard.
//!
//! F10, or Alt pressed and released alone, opens File with its first row lit.
//! While a drop is open, Up and Down walk its rows, Left and Right move to the
//! neighbouring menu, Home and End jump to the ends, a letter jumps to the next
//! row whose label starts with it, Enter or Space runs the lit row, and Escape
//! closes the drop. Right or Enter on a submenu opens its flyout, which the same
//! keys then walk; Left or Escape closes it again. A disabled row is lit like
//! any other, and Enter on it states why it cannot run. The shell hands these keys here ahead of a focused box and
//! the bindings, so an open drop holds the keyboard.
//!
//! The lit row is the row a click would run: the drop draws it from
//! [`AppState::menu_row`], and a pointer moving over a lit drop moves the light.

use crate::chrome::menu_host;
use crate::chrome::menu_items::{DropEntry, MenuItems, drop_entries};
use crate::types::AppMenu;

use super::AppState;

/// One keystroke the open drop answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuNav {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    /// Enter or Space: run the lit row.
    Run,
    /// Escape: close the drop.
    Close,
    /// A letter or digit, lowercased: light the next row whose label starts
    /// with it.
    Letter(char),
}

impl MenuNav {
    /// The navigation a GPUI key name stands for, or `None` for a key the drop
    /// does not answer, which then reaches the bindings as usual.
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "up" => Self::Up,
            "down" => Self::Down,
            "left" => Self::Left,
            "right" => Self::Right,
            "home" => Self::Home,
            "end" => Self::End,
            "enter" | "space" => Self::Run,
            "escape" => Self::Close,
            _ => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(ch), None) if ch.is_alphanumeric() => {
                        Self::Letter(ch.to_ascii_lowercase())
                    }
                    _ => return None,
                }
            }
        })
    }
}

impl AppState {
    /// F10 and a lone Alt: open the first menu with its first row lit, or close
    /// whichever drop is open.
    pub fn toggle_menu_from_keyboard(&mut self) {
        if self.open_menu.is_some() {
            self.close_menu();
        } else {
            self.open_lit(AppMenu::ALL[0]);
        }
    }

    /// The rows the open drop draws, from the registry the bar reads.
    pub(crate) fn menu_rows(&self, menu: AppMenu) -> MenuItems {
        menu_host::host_of(&self.ctx)
            .map(|host| host.items(menu, self))
            .unwrap_or_default()
    }

    /// Answer one keystroke while a drop is open. `false` when no drop is open,
    /// so the key goes on to the bindings.
    ///
    /// The light walks the drop's entries, a submenu counting as one. Right or
    /// Enter on a submenu opens its flyout with the first row lit, and the light
    /// then walks the flyout until Left or Escape closes it.
    pub fn menu_key(&mut self, nav: MenuNav, cx: &mut gpui::Context<AppState>) -> bool {
        let Some(menu) = self.open_menu else {
            return false;
        };
        let rows = self.menu_rows(menu);
        let entries = drop_entries(&rows);
        let inside = match self.menu_flyout {
            Some((at, Some(child))) => match entries.get(at) {
                Some(DropEntry::Sub { rows: held, .. }) => Some((at, child, held.as_slice())),
                _ => None,
            },
            _ => None,
        };
        if let Some((at, child, held)) = inside {
            let len = held.len();
            match nav {
                MenuNav::Down => self.menu_flyout = Some((at, step(Some(child), len, 1))),
                MenuNav::Up => self.menu_flyout = Some((at, step(Some(child), len, -1))),
                MenuNav::Home => self.menu_flyout = Some((at, Some(0))),
                MenuNav::End => self.menu_flyout = Some((at, len.checked_sub(1))),
                // A flyout a button opened is drawn without its drop, so
                // leaving it leaves the menu, and there is no bar to walk.
                MenuNav::Left | MenuNav::Close if self.menu_owner.is_some() => self.close_menu(),
                MenuNav::Left | MenuNav::Close => self.menu_flyout = None,
                MenuNav::Right if self.menu_owner.is_some() => {}
                MenuNav::Right => self.open_neighbour(menu, 1),
                MenuNav::Letter(letter) => {
                    let captions: Vec<&str> = held.iter().map(|row| rows[*row].label).collect();
                    let next = next_starting(&captions, Some(child), letter);
                    self.menu_flyout = Some((at, next.or(Some(child))));
                }
                MenuNav::Run => {
                    if let Some(item) = held.get(child).map(|row| rows[*row]) {
                        self.press_lit(&item, cx);
                    }
                }
            }
            return true;
        }
        let len = entries.len();
        let lit_sub = |row: Option<usize>| {
            row.filter(|row| matches!(entries.get(*row), Some(DropEntry::Sub { .. })))
        };
        match nav {
            MenuNav::Down => self.light_entry(step(self.menu_row, len, 1)),
            MenuNav::Up => self.light_entry(step(self.menu_row, len, -1)),
            MenuNav::Home => self.light_entry((len > 0).then_some(0)),
            MenuNav::End => self.light_entry(len.checked_sub(1)),
            MenuNav::Right => match lit_sub(self.menu_row) {
                Some(at) => self.menu_flyout = Some((at, Some(0))),
                None => self.open_neighbour(menu, 1),
            },
            MenuNav::Left => self.open_neighbour(menu, -1),
            MenuNav::Letter(letter) => {
                let captions: Vec<&str> =
                    entries.iter().map(|entry| entry.caption(&rows)).collect();
                let next = next_starting(&captions, self.menu_row, letter);
                self.light_entry(next.or(self.menu_row));
            }
            MenuNav::Close => self.close_menu(),
            MenuNav::Run => match self.menu_row.and_then(|row| entries.get(row)) {
                Some(DropEntry::Row(at)) => {
                    let item = rows[*at];
                    self.press_lit(&item, cx);
                }
                Some(DropEntry::Sub { .. }) => {
                    self.menu_flyout = self.menu_row.map(|at| (at, Some(0)));
                }
                None => {}
            },
        }
        true
    }

    /// The pointer moved onto entry `row` of the open drop. A drop the keyboard
    /// lit follows the pointer, so Enter runs the entry under it; a drop the
    /// pointer opened keeps its hover fade and lights nothing. Either way a
    /// submenu under the pointer opens its flyout, and any other entry closes
    /// the one that was open.
    pub fn point_menu_row(&mut self, row: usize) {
        if self.menu_row.is_some() {
            self.menu_row = Some(row);
        }
        let Some(menu) = self.open_menu else {
            return;
        };
        if self.menu_flyout.is_some_and(|(at, _)| at == row) {
            return;
        }
        let entries = drop_entries(&self.menu_rows(menu));
        self.menu_flyout =
            matches!(entries.get(row), Some(DropEntry::Sub { .. })).then_some((row, None));
    }

    /// The pointer moved onto row `child` of the open flyout. A flyout the
    /// keyboard entered follows the pointer.
    pub fn point_flyout_row(&mut self, child: usize) {
        if let Some((at, Some(_))) = self.menu_flyout {
            self.menu_flyout = Some((at, Some(child)));
        }
    }

    /// A click on submenu entry `row`: open its flyout. The pointer has
    /// usually opened it already on the way to the click, so a flyout already
    /// out stays out, with the row the keyboard lit in it.
    pub fn open_menu_flyout(&mut self, row: usize) {
        if self.menu_flyout.is_some_and(|(at, _)| at == row) {
            return;
        }
        self.menu_flyout = Some((row, None));
    }

    fn open_lit(&mut self, menu: AppMenu) {
        let empty = self.menu_rows(menu).is_empty();
        self.open_menu = Some(menu);
        self.menu_row = (!empty).then_some(0);
        self.menu_flyout = None;
    }

    /// Light drop entry `row`, closing any open flyout.
    fn light_entry(&mut self, row: Option<usize>) {
        self.menu_row = row;
        self.menu_flyout = None;
    }

    /// Open the menu `by` places along the bar from `menu`, wrapping.
    fn open_neighbour(&mut self, menu: AppMenu, by: isize) {
        let all = AppMenu::ALL;
        let at = all.iter().position(|m| *m == menu).unwrap_or(0) as isize;
        let next = (at + by).rem_euclid(all.len() as isize) as usize;
        self.open_lit(all[next]);
    }

    /// Run a lit row and close the drop, or leave the drop open with the reason
    /// on the status line when the row is blocked.
    fn press_lit(
        &mut self,
        item: &crate::chrome::menu_items::MenuItem,
        cx: &mut gpui::Context<AppState>,
    ) {
        if crate::chrome::menu_items::press(item, self, cx) {
            self.close_menu();
        }
    }
}

/// The next of `captions` after `from` whose first letter is `letter`, wrapping.
fn next_starting(captions: &[&str], from: Option<usize>, letter: char) -> Option<usize> {
    let len = captions.len();
    let start = from.map_or(0, |row| row + 1);
    (0..len).map(|offset| (start + offset) % len).find(|&row| {
        captions[row]
            .chars()
            .next()
            .is_some_and(|first| first.to_ascii_lowercase() == letter)
    })
}

/// The row `by` steps from `from` in a drop of `len` rows, wrapping at both
/// ends. From no lit row, Down lights the first and Up the last.
fn step(from: Option<usize>, len: usize, by: isize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match from {
        None if by > 0 => 0,
        None => len - 1,
        Some(row) => (row as isize + by).rem_euclid(len as isize) as usize,
    })
}

#[cfg(test)]
#[path = "menu_keys_tests.rs"]
mod tests;
