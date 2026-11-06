//! Consolidated integration test binary for `aria-ai`.
#![allow(clippy::duplicate_mod)]

#[path = "../agent_eval.rs"]
mod agent_eval;
#[path = "../engine_roundtrip.rs"]
mod engine_roundtrip;
#[path = "../tool_schema.rs"]
mod tool_schema;
