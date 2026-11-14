//! The aria-ui integration tests, in one binary: the shell drawn in a headless
//! window, and catalog registrations reaching the editor's lists.
//!
//! The registrations change the process-wide catalog, so they stay out of the
//! library's test binary, whose tests read that catalog. Here they share a
//! process with the shell cases under `cargo test`: every registration uses an
//! id of its own, and no shell case asserts on what the catalog lists.
//!
//! `frame_budget` is a binary of its own: its clock readings run with no other
//! test of the crate beside them.

#[path = "../registered_catalog.rs"]
mod registered_catalog;
#[path = "../shell.rs"]
mod shell;
