//! The clips of one lane, shared between projections.

use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};

use super::UiClip;

/// A lane's clips, in timeline order.
///
/// A clone shares the clips. A write copies them first while another store
/// still holds them, so a projection that leaves a lane alone hands the next
/// store this lane's clips without copying one.
#[derive(Debug, Clone, Default)]
pub struct UiClips(Arc<Vec<UiClip>>);

impl UiClips {
    /// The clips for writing, copied first while another store holds them.
    pub fn to_mut(&mut self) -> &mut Vec<UiClip> {
        Arc::make_mut(&mut self.0)
    }

    /// Whether `self` and `other` are one allocation rather than two equal
    /// lists.
    pub fn shares(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// A handle that identifies these clips without holding them.
    pub(crate) fn downgrade(&self) -> Weak<Vec<UiClip>> {
        Arc::downgrade(&self.0)
    }

    /// Whether these are the clips `handle` was taken from. The handle keeps
    /// the allocation's address reserved, so no later list can reuse it.
    pub(crate) fn is(&self, handle: &Weak<Vec<UiClip>>) -> bool {
        std::ptr::eq(Arc::as_ptr(&self.0), handle.as_ptr())
    }
}

impl Deref for UiClips {
    type Target = [UiClip];

    fn deref(&self) -> &[UiClip] {
        &self.0
    }
}

impl DerefMut for UiClips {
    fn deref_mut(&mut self) -> &mut [UiClip] {
        self.to_mut()
    }
}

impl PartialEq for UiClips {
    fn eq(&self, other: &Self) -> bool {
        self.shares(other) || self.0 == other.0
    }
}

impl From<Vec<UiClip>> for UiClips {
    fn from(clips: Vec<UiClip>) -> Self {
        Self(Arc::new(clips))
    }
}

impl FromIterator<UiClip> for UiClips {
    fn from_iter<I: IntoIterator<Item = UiClip>>(clips: I) -> Self {
        Self(Arc::new(clips.into_iter().collect()))
    }
}

impl<'a> IntoIterator for &'a UiClips {
    type Item = &'a UiClip;
    type IntoIter = std::slice::Iter<'a, UiClip>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
