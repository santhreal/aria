# Release packaging

Aria alpha builds ship as prebuilt binaries. The desktop editor is
`aria-desktop`; the iOS/macOS SwiftUI app ships through Xcode/TestFlight
and is not covered here.

**Agent / release checklist:** when cutting a public desktop alpha, follow
[`.agents/skills/aria-release/SKILL.md`](../.agents/skills/aria-release/SKILL.md)
(PR into `main`, green CI, tag from `main`). This file is the packaging
reference for version strings and local package scripts.

macOS (AVFoundation), Windows (Media Foundation), and Linux (host
GStreamer plugins) have working media stacks. Linux packages need the
distro GStreamer plugins listed in the tarball README.

## Versioning

- Cargo workspace version: `0.7.0-alpha.0` (semver pre-release) in the root
  `Cargo.toml`.
- Git tag for the alpha line: `alpha-0.7.0` (and `alpha-0.7.1`, …).
- macOS `Info.plist` short version: `0.7.0-alpha`.

## Local builds

```bash
# macOS .app: no bundled media libraries; AVFoundation is part of the OS
cargo build --release -p aria-desktop
./scripts/package-macos.sh
# → dist/Aria-0.7.0-alpha.0-macos-arm64.zip

# Linux tarball (needs host GStreamer plugins for video/audio)
cargo build --release -p aria-desktop
./scripts/package-linux.sh
# → dist/Aria-0.7.0-alpha.0-linux-x86_64.tar.gz

# Windows zip: no bundled media libraries; Media Foundation is part of the OS
cargo build --release -p aria-desktop
.\scripts\package-windows.ps1
# → dist/Aria-0.7.0-alpha.0-windows-x86_64.zip
```

### Linux desktop integration

The tarball carries the desktop-integration files beside the binary, laid out
the way they install:

```text
aria-<version>-linux-<arch>/
  aria-desktop
  install.sh
  share/applications/aria.desktop
  share/icons/hicolor/512x512/apps/aria.png
  share/mime/packages/aria.xml
```

Sources live in `packaging/linux/`. `install.sh` copies them into a prefix
(`~/.local` by default, `--prefix` for a system one, `--uninstall` to reverse
it), rewrites `Exec=`/`TryExec=` to the absolute installed path, and refreshes
the desktop, MIME, and icon caches when those tools exist. The `.aria` type is
registered as `application/x-aria-project`, matched by both the `*.aria` glob
and the schema string inside the file, so a project opens by double-click and
media files list Aria under "Open With".

### Windows installer (Setup.exe)

The portable zip above runs in place. For a real setup wizard (Start-menu
shortcut, uninstaller, optional desktop icon) build an Inno Setup installer:

```powershell
# one-time: install the Inno Setup compiler
choco install innosetup

# stages the payload (reusing package-windows.ps1) and compiles the installer
.\scripts\package-windows-installer.ps1
# → dist/Aria-0.7.0-alpha.0-windows-x86_64-Setup.exe

# native ARM64 installer (run on an ARM64 Windows host):
.\scripts\package-windows-installer.ps1 -Arch arm64
# → dist/Aria-0.7.0-alpha.0-windows-arm64-Setup.exe
```

The Inno Setup script lives at `packaging/windows/aria.iss`; the PowerShell
wrapper passes the version, staged source dir, and output path as `/D` defines.
The installer is unsigned for now. Windows SmartScreen will warn on first run
until the `Setup.exe` is Authenticode-signed.

`dist/` is gitignored.

### macOS Gatekeeper (alpha)

Release zips are **adhoc-signed**, not notarized. Double-click may do nothing
until the user right-clicks `Aria.app` → **Open** once. The zip includes
`INSTALL-macos.txt` with full steps. Proper fix for a stable channel is Apple
Developer ID signing + notarization in CI.

## Licensing

Bundles carry no third-party media libraries: decode/encode goes through the
operating system's frameworks (AVFoundation/VideoToolbox on Apple platforms,
Media Foundation on Windows). Aria itself is MIT OR Apache-2.0.
