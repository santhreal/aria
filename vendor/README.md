# Vendored / pinned UI dependencies

The GPUI platform crates remain pinned as git dependencies at Comet's proven
revision of the `wingleeio/zed` fork. The GPUI library has a local MSRV patch.

## gpui_platform / gpui_tokio

- Source: https://github.com/wingleeio/zed
- Rev: `5d1f83d9f27a19bec1fb241dc33b42238af9cf8d`
  (`gpui: horizontal per-pixel EdgeFade for quads and images`)
- Date pinned: 2026-08-17
- Why this rev: it is the pin in [Comet](https://github.com/zeronsh/comet)
  (`Cargo.toml` workspace.dependencies). Comet's motion, frost, and
  edge-fade code depend on APIs that exist at this rev
  (`Window::with_edge_fade`, backdrop blur, `gpui_tokio::init`).
- Modifications: none. Consumed as git dependencies, not tree copies.
- License: Apache-2.0 (see each crate's `Cargo.toml` in the Zed tree).
  Zed's product as a whole is GPL-3; these specific crates declare
  Apache-2.0.

A full tree copy of `crates/gpui`, `crates/gpui_platform`, and
`crates/gpui_tokio` is not viable on its own: `gpui` depends on a large
set of Zed-internal workspace crates (`gpui_macros`, `gpui_shared_string`,
`gpui_util`, `collections`, `refineable`, `sum_tree`, `gpui_linux`,
`gpui_macos`, `gpui_windows`, …). Pinning the git rev is the same
strategy Comet uses and is the only way to resolve that graph.

To bump: rebase against Comet's current pin, then update the three
`rev =` fields in the workspace `Cargo.toml` together.

## gpui

The pinned GPUI revision uses `std::hint::cold_path`, which is not stable in
the workspace's Rust 1.92 toolchain. The local patch retains the upstream
library source and replaces its six cold hints with a stable `#[cold]`
function. The path remains cold without an unstable standard-library API.

- Source crate: `crates/gpui` at the GPUI revision above
- Local path: `vendor/gpui`
- Modifications: workspace-inherited manifest fields were replaced with
  standalone values; dev targets were omitted; six `std::hint::cold_path`
  calls use the local stable helper
- License: Apache-2.0

## gpui_shared_string

Zed's tree also ships `tooling/lints/test_fixture/gpui_shared_string`
(version `0.0.0`, a `Clone`-only stub). Cargo resolver 3 picks that
fixture over `crates/gpui_shared_string` when `gpui` is consumed as a
git dependency. The real crate is copied to `vendor/gpui_shared_string`
and forced via `[patch."https://github.com/wingleeio/zed"]`.

- Source crate: `crates/gpui_shared_string` at the same rev as gpui
- Local path: `vendor/gpui_shared_string`
- Modifications: workspace inherit (`edition.workspace`,
  `schemars.workspace`, `serde.workspace`, `[lints]`) rewritten so the
  crate builds as a standalone path dep. Implementation is unchanged.

## gpui_util

The pinned GPUI revision uses `slice::as_array`, which is not stable in the
workspace's Rust 1.92 toolchain. The local patch retains the upstream crate and
expresses the eight-byte conversion through stable `TryInto`.

- Source crate: `crates/gpui_util` at the GPUI revision above
- Local path: `vendor/gpui_util`
- Modifications: workspace-inherited manifest fields were replaced with
  standalone values; one `slice::as_array` call was replaced with `TryInto`
- License: Apache-2.0

## ztracing / ztracing_macro

`sum_tree` imports the disabled instrumentation surface from Zed's GPL tracing
crates. The local patches provide the same no-op attribute, span macros, and
`Span` methods under the workspace's dual MIT/Apache-2.0 license. The patch
does not include Zed's logger or Tracy profiler.

- Source API: `crates/ztracing` at the GPUI revision above
- Local paths: `vendor/ztracing`, `vendor/ztracing_macro`
- Modifications: the disabled configuration is retained as a standalone
  compatibility implementation; profiler-only code is omitted.
