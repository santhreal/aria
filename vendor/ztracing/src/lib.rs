//! Allocation-free compatibility surface for GPUI's disabled tracing build.

pub use tracing::{Level, field};
pub use ztracing_macro::instrument;

pub use __consume_all_tokens as debug_span;
pub use __consume_all_tokens as error_span;
pub use __consume_all_tokens as event;
pub use __consume_all_tokens as info_span;
pub use __consume_all_tokens as span;
pub use __consume_all_tokens as trace_span;
pub use __consume_all_tokens as warn_span;

#[macro_export]
macro_rules! __consume_all_tokens {
    ($($token:tt)*) => {
        $crate::Span
    };
}

pub struct Span;

impl Span {
    pub fn current() -> Self {
        Self
    }

    pub fn enter(&self) {}

    pub fn record<T, S>(&self, _field: T, _value: S) {}
}

pub fn init() {}
