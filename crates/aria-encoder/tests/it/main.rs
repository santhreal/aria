//! Consolidated integration test binary for `aria-encoder`.
#![allow(clippy::duplicate_mod)]

#[path = "../long_av.rs"]
mod long_av;
#[path = "../roundtrip.rs"]
mod roundtrip;
#[path = "../session_limit.rs"]
mod session_limit;
