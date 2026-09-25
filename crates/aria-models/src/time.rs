//! Time primitives, re-exported from [`aria_core`].
//!
//! The exact rational clock that the data model is built on now lives in the
//! shared `aria-core` contract crate (the decoder, compositor, and engine
//! all sign it). The model keeps this thin re-export so every `crate::time::*`
//! path resolves to the one canonical implementation instead of a fork.

pub use aria_core::{
    Rational, RationalTime, TimeError, TimeRange, check_same_rate, rate_eq, resample, time_add,
    time_sub,
};
