//! One module brings up the GPU, and the census that keeps it.

use std::path::Path;

use super::collect_rs;

/// Every module that may bring up a GPU device, and why.
///
/// A device is the dearest thing a start does — 176 ms of a 222 ms start on
/// this machine — and it holds video memory until the process exits. The
/// editor used to create three: the compositor plugin's, the render plugin's
/// and the engine's, of which exactly one was ever rendered with. The other
/// two were registered and never read.
const DEVICE_OWNERS: &[(&str, &str)] = &[
    (
        "test_support/gpu.rs",
        "the rule below spells out what it refuses, and would match itself",
    ),
    (
        "app/render_device.rs",
        "the one place that turns the mounted device into a renderer for \
         background work, and falls back to bringing one up where none \
         mounted",
    ),
    (
        "plugins/engine.rs",
        "the render plugin brings the one device up and the engine plugin \
         shares it into the renderer it owns",
    ),
    (
        "app/commands.rs",
        "the fallback for a session built on a context where no plugin \
         mounted, which is every test that does not mount them",
    ),
];

/// **One GPU device per process, owned where it is brought up.**
///
/// Source-derived, because the defect was invisible in behaviour: three
/// devices render exactly like one, only slower to start and three times the
/// video memory, which is why it survived until a start was measured. A
/// fourth caller is what this refuses.
#[test]
fn one_module_brings_up_the_gpu() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs(&src, &mut files);
    assert!(
        files.len() > 50,
        "found {} source files under {src:?}, which cannot be this crate: the rule \
         just went vacuous",
        files.len()
    );
    let mut offenders = Vec::new();
    let mut seen = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            // `Renderer::with_gpu` and `Engine::with_renderer` share a device
            // and are deliberately not matched: it is bringing one *up* that
            // costs, and every constructor that does spells it this way.
            if !line.contains("new_headless") && !line.contains("Engine::new(") {
                continue;
            }
            if line.trim_start().starts_with("//") {
                continue;
            }
            if DEVICE_OWNERS.iter().any(|(owner, _)| rel == *owner) {
                seen.push(rel.to_string());
                continue;
            }
            offenders.push(format!("{rel}:{}: {}", number + 1, line.trim()));
        }
    }
    assert!(
        offenders.is_empty(),
        "these bring up a GPU device of their own, and only {} module(s) may: share \
         the mounted one with `GpuContext::share` and `Renderer::with_gpu`\n  {}",
        DEVICE_OWNERS.len(),
        offenders.join("\n  ")
    );
    for (owner, _) in DEVICE_OWNERS {
        assert!(
            seen.iter().any(|rel| rel == owner),
            "{owner} is declared as bringing up the device and no longer does: delete \
             the row"
        );
    }
}
