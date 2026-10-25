# Contributing to Aria

Thanks for your interest in Aria. It is early alpha software and there is
plenty of room to help: fixing bugs, filling in editing features, improving
docs, sharpening the AI assistant, or just reporting what breaks.

This guide explains how the repository is organized, how to build and test it,
and what we expect from a change before it lands.

## Human and AI contributions both welcome

We judge a contribution by what it does, not by how it was made. Whether you
wrote it by hand, with an AI assistant, or it was largely AI-generated, we
accept it as long as it adds something useful (a bug fix, a new feature, a
performance win, tests, or docs) and meets the bar in this guide.

That bar is the same for everyone: the change has to build, pass the checks
below, be reasonably scoped and reviewable, and actually improve the project.
You are responsible for any code you submit, so understand it, test it, and be
ready to explain and revise it in review. Don't open low-effort or untested
PRs (AI-generated or otherwise) just to make noise. Those waste reviewer time
and will be closed.

It's nice (and appreciated) to mention in the PR description when a change was
AI-generated or AI-assisted. It's not required and won't count against you. It
just gives reviewers helpful context.

## Before you start

- Read the [README](README.md) to understand what Aria is and how to run it.
- Read the [specification](docs/spec/README.md) and [ROADMAP.md](ROADMAP.md)
  for the target editor and the delivery order. Parked work is in
  [docs/deferred.md](docs/deferred.md).
- Search [open issues](https://github.com/santhreal/aria/issues) before
  filing a new one or starting work, so effort is not duplicated.
- For anything larger than a small fix, open an issue first to discuss the
  approach. It saves everyone a wasted PR.

## Ways to contribute

- **Report bugs** using the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md).
  Include your OS, how you built or installed Aria, exact steps to reproduce,
  what you expected, and what happened. Screenshots and a sample `.aria`
  project help a lot.
- **Request features** using the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md).
  Describe the problem first, then the change you have in mind.
- **Send code** for bug fixes, editing features, performance work, tests, or
  docs. See the workflow below.

## Development setup

Aria is a Rust workspace. Install Rust 1.97 or newer. `Cargo.toml` and CI
enforce 1.97 as the minimum supported Rust version; `rust-toolchain.toml`
tracks stable for normal development. Media decode and encode use AVFoundation
on Apple, Media Foundation on Windows, MediaCodec on Android, and GStreamer
1.24 on Linux. Linux builds require the GStreamer development packages.
Runtime decode and encode require the base and good plugins plus ugly or libav.

Build and test the whole workspace:

```bash
cargo build --workspace
./scripts/test.sh
```

`scripts/test.sh` forwards its arguments to `cargo test`, so
`./scripts/test.sh -p aria-ui --lib` works. Use it in preference to `cargo
test`: tests write their fixtures under `scratch/` (`.cargo/config.toml` points
`TMPDIR` there, so nothing lands in a shared `/tmp`), and the script empties
that directory before and after the run, which is what collects the fixtures a
run interrupted by Ctrl-C or by the OOM killer leaves behind. It also caps
compiler parallelism at half the machine; set `JOBS` to override.

Run the desktop editor (optionally with a media file):

```bash
cargo run -p aria-desktop
cargo run -p aria-desktop -- path/to/video.mp4
```

The iOS/macOS SwiftUI app (`apps/aria-ios-macos`) builds with Xcode on top
of the engine's C ABI; `./scripts/build-ios-xcframework.sh` produces the
XCFramework it links.

## Project layout

The workspace is split into focused crates, layered from data up to the apps:

- `aria-core`: shared primitives (rational time, colorimetry, pixel
  formats, the GPU/CPU frame handoff, and the decode/encode contracts).
- `aria-models`: project/timeline data model, media pool, templates, and
  the `.aria` file schema. UI-, decode-, and render-agnostic.
- `aria-commands`: the structured command vocabulary every edit goes through.
- `aria-decoder`: platform-native video/image decode behind the core
  `VideoDecoder` trait (AVFoundation/VideoToolbox on Apple), plus audio peaks.
- `aria-encoder`: platform-native H.264/mp4 encode behind the core
  `VideoEncoder` trait (VideoToolbox on Apple).
- `aria-compositor`: WGPU GPU compositor that combines layers into RGBA
  frames.
- `aria-text` / `aria-shapes`: text and vector-shape generators the
  compositor places as layers.
- `aria-render`: resolves a project at an instant into a scene, renders
  frames, mixes audio, and exports timelines to video.
- `aria-engine`: the headless editing engine. Applies commands, records
  undo/redo, produces preview frames. Main integration point for non-UI
  callers.
- `aria-mobile`: C ABI + JNI bridge the iOS/Android apps link.
- `aria-settings`: desktop user settings (`~/.aria/config.toml`).
- `aria-cli`: command-line compositor demos (headless GPU to image file).
- `aria-py`: MoviePy-style Python bindings (excluded from the default
  workspace build; built with maturin).
- `aria-plugin`: typed service registry and event bus every plugin mounts into.
- `aria-ui`: GPUI shell, panels, theme, motion, and built-in plugins.
- `apps/aria-desktop`: the headed editor binary. Mounts plugins and calls
  `aria_ui::run_app`.
- `apps/aria-ios-macos`, `apps/aria-android`: the mobile apps.

Each crate has its own `README.md` describing its responsibilities and what
does and does not belong there. Read the relevant one before adding code.

Two boundaries matter:

- **Keep the engine UI-agnostic.** GPUI views, widgets, file dialogs, and
  interaction state belong in `aria-ui` / `apps/aria-desktop`, not `aria-engine`.
- **Keep `aria-models` pure data.** No file I/O, GPU work, UI bindings,
  or undo/redo dispatch. Those live in higher crates.

## Coding conventions

- **Performance is a correctness concern** on interactive paths (timeline
  scrubbing, preview, per-frame and per-sample work, export). Know the
  asymptotic cost of hot loops, avoid accidental O(n²), and profile or
  benchmark before rewriting hot code. Prioritize clarity on cold paths. See
  [`.cursor/rules/perf.mdc`](.cursor/rules/perf.mdc).
- **Comments explain intent, not mechanics.** Skip comments that just narrate
  what the code does; document non-obvious trade-offs and constraints.
- **Text the editor shows** is spelled in US English: `Color`, `center`,
  `Favorites`, `normalize`. Commands, menus, dialog titles and context menu
  rows use Title Case; other labels and buttons use sentence case.
  `commands::casing_tests` and `panels::panel_casing_tests` fail on a label
  in the wrong case, `test_support::us_spelling` on a string literal in
  British spelling, and `keys::docs_markdown_tests` on British spelling in
  the documentation. Identifiers and published ids keep their spelling.
- **Icons**: never block a feature on missing art. Ship a short
  text/char placeholder, register it in [`docs/icons.md`](docs/icons.md), and
  move on.
- **Do not commit ignored paths**: build artifacts (`/target`, `/dist`), media
  scratch dirs (`frames/`, `proxy/`, `local-assets/`), `.env`, or `.aria/`.

## Commits

Commit history is a deliverable. Optimize for small, real commits rather than
one giant change.

- **One logical change per commit.** Land a feature as a series (model/schema,
  then engine/actions, then UI wiring, then tests/docs) instead of a single
  mega-commit. Bug fixes and refactors are their own commits.
- **Every commit must build and pass its targeted tests.** No empty, padding,
  or fabricated commits.
- **Message style** matches existing history (`git log --oneline`): an
  imperative, descriptive subject ending with a period that says what shipped
  and why it matters, not which files changed. Feature milestones use the
  `Ship <milestone> <name>: <summary>.` shape. Use the body to explain intent,
  trade-offs, and deliberate gaps.
- Use an author identity that matches your GitHub account so your contributions
  are attributed correctly.

## Submitting a pull request

1. Fork the repo and create a topic branch from `main`.
2. Make your change as a clean series of commits (see above).
3. Run the same checks CI runs, and make sure they pass:

   ```bash
   cargo fmt --all
   cargo clippy --workspace --all-targets -- -D warnings
   cargo build --workspace --all-targets
   cargo test --workspace
   ```

   CI enforces clippy with `-D warnings`, a workspace build, and the test
   suite. Running `cargo fmt` keeps formatting consistent.
4. Add or update tests. Engine and model changes should cover invariants and
   behavior; for undoable edits, test both the forward edit and its inverse.
   Run a single crate's tests with `cargo test -p <crate>`.
5. Update docs (crate `README.md`, `docs/`, the icon registry) when behavior or
   structure changes.
6. Open the PR with a clear description of what changed and why, and link any
   related issue. Keep PRs focused; unrelated drive-by changes should land
   separately.

A maintainer will review and may ask for changes. Address feedback as new
commits rather than rewriting published history.

## Code of conduct

Be respectful and constructive. Assume good faith, keep discussion focused on
the work, and help make this a welcoming project to contribute to.

## License

By contributing, you agree that your contributions are dual licensed under the
[Apache License 2.0](LICENSE-APACHE) and the [MIT license](LICENSE-MIT),
matching the rest of Aria, without additional terms or conditions.
