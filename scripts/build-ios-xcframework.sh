#!/usr/bin/env bash
# Build AriaMobileFFI.xcframework for the AriaMobile Swift package.
#
# The xcframework bundles the aria-mobile static lib for device, simulator,
# and macOS so Xcode can link whichever slice it needs. It is a build artifact
# (gitignored); run this after changing any Rust the mobile FFI depends on,
# then rebuild the app in Xcode.
#
# Usage: scripts/build-ios-xcframework.sh [--debug]

set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE=release
CARGO_FLAGS=(--release)
if [[ "${1:-}" == "--debug" ]]; then
    PROFILE=debug
    CARGO_FLAGS=()
fi

TARGET_DIR="$(
    cargo metadata --locked --no-deps --format-version 1 |
        python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
)"

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim aarch64-apple-darwin)
for t in "${TARGETS[@]}"; do
    rustup target add "$t" >/dev/null
    cargo build -p aria-mobile --target "$t" "${CARGO_FLAGS[@]}" --locked
done

PKG=apps/aria-ios-macos/AriaMobile
OUT="$PKG/AriaMobileFFI.xcframework"
rm -rf "$OUT"
xcodebuild -create-xcframework \
    -library "$TARGET_DIR/aarch64-apple-ios/$PROFILE/libaria_mobile.a" -headers "$PKG/include" \
    -library "$TARGET_DIR/aarch64-apple-ios-sim/$PROFILE/libaria_mobile.a" -headers "$PKG/include" \
    -library "$TARGET_DIR/aarch64-apple-darwin/$PROFILE/libaria_mobile.a" -headers "$PKG/include" \
    -output "$OUT"

echo "wrote $OUT ($PROFILE)"
