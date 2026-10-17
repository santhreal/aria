//! Final Cut Pro XML (`.fcpxml`), written as version 1.10.
//!
//! A project's sequence holds a spine: clips and gaps end to end, with
//! transitions over the cuts between them. Clips connected to a spine item
//! sit on numbered lanes above it (positive) or below it (negative). Times
//! are rational seconds such as `1001/30000s`. A clip's `offset` places it in
//! its parent's time, `start` is the first instant of its own time it shows,
//! and a `timeMap` maps its own time to its media's when it is retimed.

mod read;
mod write;

pub use read::read;
pub use write::write;

/// The version written.
const VERSION: &str = "1.10";
/// Final Cut Pro's Cross Dissolve.
const DISSOLVE_UID: &str = "FxPlug:4731E73A-8DAC-4113-9A30-AE85B1761265";

#[cfg(test)]
mod tests;
