//! State the panels drawn in one window keep for that window.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

/// One value of each type a panel keeps for the window, made on first use.
///
/// The window's shell holds it, so a gesture held when the window closes ends
/// with the window and the next window starts with its own. The shell holds
/// the values without naming their types, which belong to the panels.
#[derive(Default)]
pub(crate) struct PanelMemory(RefCell<Vec<Rc<dyn Any>>>);

impl PanelMemory {
    /// This window's `T`.
    pub(crate) fn get<T: Default + 'static>(&self) -> Rc<T> {
        let mut slots = self.0.borrow_mut();
        if let Some(kept) = slots
            .iter()
            .find_map(|slot| Rc::clone(slot).downcast::<T>().ok())
        {
            return kept;
        }
        let made = Rc::new(T::default());
        slots.push(Rc::clone(&made) as Rc<dyn Any>);
        made
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[derive(Default)]
    struct Press(Cell<u32>);

    #[derive(Default)]
    struct Hover(Cell<u32>);

    /// A value written through one `get` is the value the next `get` of its
    /// type reads, and a value of another type is a separate slot: a store
    /// that made a fresh value per call would drop every gesture between the
    /// press and the release.
    #[test]
    fn each_type_keeps_one_value_per_window() {
        let window = PanelMemory::default();
        window.get::<Press>().0.set(3);
        window.get::<Hover>().0.set(7);
        assert_eq!(window.get::<Press>().0.get(), 3);
        assert_eq!(window.get::<Hover>().0.get(), 7);
        assert_eq!(window.0.borrow().len(), 2);

        let next = PanelMemory::default();
        assert_eq!(next.get::<Press>().0.get(), 0, "a new window starts clear");
    }
}
