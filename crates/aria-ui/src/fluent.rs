//! Fluent conditionals. GPUI's `util::FluentBuilder` is crate-private.

pub trait FluentBuilder {
    fn when(self, condition: bool, then: impl FnOnce(Self) -> Self) -> Self
    where
        Self: Sized,
    {
        if condition { then(self) } else { self }
    }

    /// Pick one of two shapes for the same builder.
    ///
    /// `when` can only add to one branch, so a control that is a row in one
    /// view and a column in another had to be written twice or hoisted out of
    /// the chain. Both arms return `Self`, which is what separates this from a
    /// branch that erases the element's type: that one has to be hoisted.
    fn either(
        self,
        condition: bool,
        then: impl FnOnce(Self) -> Self,
        otherwise: impl FnOnce(Self) -> Self,
    ) -> Self
    where
        Self: Sized,
    {
        if condition {
            then(self)
        } else {
            otherwise(self)
        }
    }

    fn when_some<T>(self, option: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self
    where
        Self: Sized,
    {
        match option {
            Some(value) => then(self, value),
            None => self,
        }
    }
}

impl<T> FluentBuilder for T {}
