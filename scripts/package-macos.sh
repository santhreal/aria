#!/usr/bin/env bash
# Assemble a distributable Aria.app for macOS (alpha packaging).
#
# Usage:
#   ./scripts/package-macos.sh   # aarch64 (native)
#
# Output:
#   dist/Aria-<version>-macos-<arch>.zip   (the .app inside)
#
# Prerequisites:
#   - Rust stable (see rust-toolchain.toml)
#
# Media decode/encode uses the system's AVFoundation/VideoToolbox, so the
# bundle carries no third-party dylibs (no FFmpeg, no dylibbundler step).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

for arg in "$@"; do
    case "$arg" in
        -h|--help)
            sed -n '2,14p' "$0"
            exit 0
            ;;
        *)
            echo "unknown argument: $arg" >&2
            exit 1
            ;;
    esac
done

METADATA="$(cargo metadata --locked --no-deps --format-version 1)"
IFS=$'\t' read -r VERSION TARGET_DIR < <(
    python3 -c '
import json
import sys

metadata = json.load(sys.stdin)
version = next(
    package["version"]
    for package in metadata["packages"]
    if package["name"] == "aria-desktop"
)
print(f"{version}\t{metadata['"'"'target_directory'"'"']}")
' <<<"$METADATA"
)
ARCH="$(uname -m)"
DIST="dist"
APP_NAME="Aria.app"
STAGING="$DIST/staging-$ARCH"
APP="$STAGING/$APP_NAME"
BINARY_SRC="$TARGET_DIR/release/aria-desktop"
ICON_PNG="assets/icon/aria-icon.png"

echo "==> packaging Aria $VERSION for macOS ($ARCH)"

if [[ ! -f "$BINARY_SRC" ]]; then
    echo "==> release binary missing; building aria-desktop"
    cargo build --release -p aria-desktop --locked
fi

rm -rf "$STAGING"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"

cp packaging/macos/Info.plist "$APP/Contents/Info.plist"
cp "$BINARY_SRC" "$APP/Contents/MacOS/aria-desktop"
chmod +x "$APP/Contents/MacOS/aria-desktop"

# App icon (.icns) from the canonical Aria mark.
# Optional: skip if the source PNG isn't present so packaging still succeeds.
if [[ -f "$ICON_PNG" ]]; then
    ICONSET="$STAGING/AppIcon.iconset"
    mkdir -p "$ICONSET"
    for spec in \
        "16:icon_16x16.png" \
        "32:icon_16x16@2x.png" \
        "32:icon_32x32.png" \
        "64:icon_32x32@2x.png" \
        "128:icon_128x128.png" \
        "256:icon_128x128@2x.png" \
        "256:icon_256x256.png" \
        "512:icon_256x256@2x.png" \
        "512:icon_512x512.png" \
        "1024:icon_512x512@2x.png"; do
        pixels="${spec%%:*}"
        output="${spec#*:}"
        sips -z "$pixels" "$pixels" "$ICON_PNG" --out "$ICONSET/$output" >/dev/null
    done
    iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
else
    echo "==> note: $ICON_PNG missing; building .app without a custom icon"
fi

if [[ -n "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    echo "==> signing app bundle with Developer ID"
    codesign \
        --force \
        --deep \
        --options runtime \
        --timestamp \
        --sign "$APPLE_SIGNING_IDENTITY" \
        "$APP"
else
    echo "==> ad hoc-signing app bundle"
    codesign --force --deep --sign - "$APP"
fi
codesign --verify --deep --strict --verbose=2 "$APP"

RELEASE="$STAGING/release"
rm -rf "$RELEASE"
mkdir -p "$RELEASE"
ditto "$APP" "$RELEASE/$APP_NAME"
cp packaging/macos/INSTALL.txt "$RELEASE/INSTALL-macos.txt"

ZIP="$DIST/Aria-${VERSION}-macos-${ARCH}.zip"
rm -f "$ZIP"
(
    cd "$RELEASE"
    # zip -y preserves symlinks, should the bundle ever grow any.
    zip -r -y "$ROOT/$ZIP" "$APP_NAME" INSTALL-macos.txt
)

echo "==> wrote $ZIP ($(du -h "$ZIP" | awk '{print $1}'))"
echo "    install: unzip, read INSTALL-macos.txt, drag Aria.app to /Applications"
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
    echo "    first launch: Right-click Aria.app → Open (ad hoc-signed build)"
fi
