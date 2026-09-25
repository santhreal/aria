#!/usr/bin/env bash
# Assemble a distributable Linux tarball for Aria (alpha packaging).
#
# Decode/encode use host GStreamer plugins. Missing plugins fail with the
# package to install. Stills work without them.
#
# Usage:
#   ./scripts/package-linux.sh
#
# Output:
#   dist/Aria-<version>-linux-<arch>.tar.gz

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

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
STAGING="$DIST/staging-linux-$ARCH"
PKG="aria-$VERSION-linux-$ARCH"

echo "==> packaging Aria $VERSION for Linux ($ARCH)"

BINARY_SRC="$TARGET_DIR/release/aria-desktop"
if [[ ! -f "$BINARY_SRC" ]]; then
    echo "==> release binary missing; building aria-desktop"
    cargo build --release -p aria-desktop --locked
fi

rm -rf "$STAGING"
mkdir -p "$STAGING/$PKG"
cp "$BINARY_SRC" "$STAGING/$PKG/aria-desktop"
chmod +x "$STAGING/$PKG/aria-desktop"
cp LICENSE-MIT LICENSE-APACHE "$STAGING/$PKG/"

# Desktop integration, laid out the way it will be installed: a launcher, an
# icon in the hicolor theme, and the project file type. `install.sh` copies
# these into a prefix and refreshes whatever caches the desktop has.
mkdir -p "$STAGING/$PKG/share/applications" \
    "$STAGING/$PKG/share/icons/hicolor/512x512/apps" \
    "$STAGING/$PKG/share/mime/packages"
cp packaging/linux/aria.desktop "$STAGING/$PKG/share/applications/aria.desktop"
cp assets/icon/aria-icon.png "$STAGING/$PKG/share/icons/hicolor/512x512/apps/aria.png"
cp packaging/linux/aria-mime.xml "$STAGING/$PKG/share/mime/packages/aria.xml"
cp packaging/linux/install.sh "$STAGING/$PKG/install.sh"
chmod +x "$STAGING/$PKG/install.sh"

cat >"$STAGING/$PKG/README-INSTALL.txt" <<EOF
Aria $VERSION — Linux ($ARCH)
================================

Run in place:
  ./aria-desktop

Or put it in the desktop's menus (no root, installs under ~/.local):
  ./install.sh

  --prefix /usr/local   install system-wide instead
  --uninstall           remove what install.sh copied

Installing adds a launcher, an icon, and the .aria project file type, so
projects open by double-click and media files offer Aria under "Open With".
Projects, settings and caches live in ~/.aria and are never touched by
install.sh.

Needs GStreamer plugins on the host (\`gstreamer1.0-plugins-base\`,
\`-good\`, and either \`-ugly\`/\`-libav\` or a VAAPI/NVENC plugin). Stills
work without them. Missing plugins name the package to install.

See https://github.com/santhreal/aria for source and issue tracker.
EOF

TARBALL="$DIST/Aria-${VERSION}-linux-${ARCH}.tar.gz"
rm -f "$TARBALL"
tar -C "$STAGING" -czf "$TARBALL" "$PKG"

echo "==> wrote $TARBALL ($(du -h "$TARBALL" | awk '{print $1}'))"
