#!/usr/bin/env bash
# Put Aria in the desktop's menus, or take it back out.
#
# Shipped inside the release tarball and run from the unpacked directory. It
# copies four things and touches nothing else:
#
#   aria-desktop                       -> <prefix>/bin
#   share/applications/aria.desktop    -> <prefix>/share/applications
#   share/icons/.../aria.png           -> <prefix>/share/icons/hicolor/...
#   share/mime/packages/aria.xml       -> <prefix>/share/mime/packages
#
# Usage:
#   ./install.sh                 # into ~/.local (no root)
#   ./install.sh --prefix /usr/local
#   ./install.sh --uninstall     # same prefix rules
#
# A system prefix needs write permission; run it under sudo if that is what you
# want. Nothing is installed setuid, and no service is registered.

set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${HOME}/.local"
MODE="install"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            [[ $# -ge 2 ]] || {
                echo "--prefix needs a directory" >&2
                exit 2
            }
            PREFIX="$2"
            shift 2
            ;;
        --prefix=*)
            PREFIX="${1#--prefix=}"
            shift
            ;;
        --uninstall)
            MODE="uninstall"
            shift
            ;;
        -h | --help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

# A launcher runs with no working directory of the operator's, so a relative
# prefix would produce an `Exec=` line that only works from wherever the
# install was run. Resolve it now, before any path is written into a file.
mkdir -p "$PREFIX"
PREFIX="$(cd "$PREFIX" && pwd)"

BIN_DIR="$PREFIX/bin"
APP_DIR="$PREFIX/share/applications"
ICON_DIR="$PREFIX/share/icons/hicolor/512x512/apps"
MIME_DIR="$PREFIX/share/mime/packages"

# Refresh whatever caches exist. All three are optional: a desktop without them
# reads the files directly, and a missing tool is not a failed install.
refresh() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database "$APP_DIR" 2>/dev/null || true
    fi
    if command -v update-mime-database >/dev/null 2>&1; then
        update-mime-database "$PREFIX/share/mime" 2>/dev/null || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f "$PREFIX/share/icons/hicolor" 2>/dev/null || true
    fi
}

if [[ "$MODE" == "uninstall" ]]; then
    rm -f "$BIN_DIR/aria-desktop" \
        "$APP_DIR/aria.desktop" \
        "$ICON_DIR/aria.png" \
        "$MIME_DIR/aria.xml"
    refresh
    echo "==> removed Aria from $PREFIX"
    echo "    Projects, settings and caches in ~/.aria were left alone."
    exit 0
fi

[[ -f "$HERE/aria-desktop" ]] || {
    echo "aria-desktop is not next to this script; run it from the unpacked tarball" >&2
    exit 1
}

mkdir -p "$BIN_DIR" "$APP_DIR" "$ICON_DIR" "$MIME_DIR"

install -m 755 "$HERE/aria-desktop" "$BIN_DIR/aria-desktop"
install -m 644 "$HERE/share/icons/hicolor/512x512/apps/aria.png" "$ICON_DIR/aria.png"
install -m 644 "$HERE/share/mime/packages/aria.xml" "$MIME_DIR/aria.xml"

# The entry names the binary by absolute path: a launcher does not inherit the
# shell's PATH, so `Exec=aria-desktop` fails for exactly the ~/.local/bin case
# this script's default installs into.
sed -e "s|^Exec=aria-desktop|Exec=$BIN_DIR/aria-desktop|" \
    -e "s|^TryExec=aria-desktop|TryExec=$BIN_DIR/aria-desktop|" \
    "$HERE/share/applications/aria.desktop" >"$APP_DIR/aria.desktop"
chmod 644 "$APP_DIR/aria.desktop"

refresh

echo "==> installed Aria into $PREFIX"
echo "    binary:  $BIN_DIR/aria-desktop"
echo "    launcher: $APP_DIR/aria.desktop"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "    note: $BIN_DIR is not on PATH; the menu entry works regardless" ;;
esac
