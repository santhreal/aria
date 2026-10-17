#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MANIFEST_PATH="${SCRIPT_DIR}/Cargo.toml"

cargo build --manifest-path "${MANIFEST_PATH}" --lib --target wasm32-unknown-unknown --release
TARGET_DIR="$(cargo metadata --manifest-path "${MANIFEST_PATH}" --no-deps --format-version 1 | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')"
CORE_WASM="${TARGET_DIR}/wasm32-unknown-unknown/release/aria_example_wasm_plugin.wasm"

cargo run --manifest-path "${MANIFEST_PATH}" --bin bundle-component --release -- \
    "${CORE_WASM}" \
    "${SCRIPT_DIR}/plugin.wasm" \
    "${SCRIPT_DIR}/../../crates/aria-plugin-wasm/tests/fixtures/example_plugin.wasm"
