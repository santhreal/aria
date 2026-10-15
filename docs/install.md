# Install

Builds are on the [releases page](https://github.com/santhreal/aria/releases).
Settings live in `~/.aria/config.toml`. Projects use the `.aria` extension.

## macOS

Apple Silicon. Unzip and drag `Aria.app` to Applications. On first launch,
right-click the app and choose **Open**: builds are not notarized yet, so
double-clicking is refused by Gatekeeper.

Decode and encode use AVFoundation. The encoder writes H.264 video and AAC
audio; no other codec is offered, and the bitrate is the encoder's own.

## Windows

x64. Unzip and run, or install with `Setup.exe`. Builds are unsigned, so
SmartScreen warns on first run.

Decode and encode use Media Foundation, with the same H.264 and AAC limit as
macOS.

## Linux

Preview builds. Decode and encode use the host GStreamer stack: VAAPI, NVDEC,
or V4L2 when those plugins are present, then software.

Install the plugin sets:

```bash
sudo apt install gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-ugly gstreamer1.0-libav
```

Add a VAAPI or NVENC plugin for hardware encode. Export uses the first encoder
that opens, in the order VAAPI, NVENC, then `x264enc` or `x265enc`, so a
hardware plugin whose driver does not load falls through to the next one. When
none opens, the error lists each encoder's reason and the package to install.
Still images always work, with no plugin at all.

The tarball runs in place with `./aria-desktop`. To put Aria in the desktop's
menus, run the installer inside it:

```bash
./install.sh                    # under ~/.local, no root
./install.sh --prefix /usr/local
./install.sh --uninstall
```

It copies the binary, a launcher, an icon, and the `.aria` file type, then
refreshes the desktop, MIME, and icon caches when those tools are present.
Projects then open by double-click, and media files offer Aria under **Open
With**. Projects, settings, and caches in `~/.aria` are untouched by either
direction.

## From source

A recent stable Rust toolchain:

```bash
cargo run -p aria-desktop
```

The workspace builds the whole editor, including the engine, the compositor, and
the GPUI shell. GPUI comes from
[santh-gpui](https://github.com/santhreal/santh-gpui), pinned by git revision
in the workspace `Cargo.toml`.
