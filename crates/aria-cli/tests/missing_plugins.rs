//! `aria frame` states each entry a project holds that no catalog lists and
//! renders it as the editor renders an entry whose plugin is missing.
//!
//! Closes the class "the command line renders a plugin's part differently
//! from the editor and states nothing": `aria` loads no plugins, so a plugin
//! effect, generator, or transition rendered as passthrough, a slate, or a cut
//! with an empty stderr. Each case holds one unlisted id of one
//! [`CatalogKind`] kind; the match over the kind does not compile for a kind
//! without a case. Not covered: `aria export`, which prints the same lines
//! through the same function before it renders, and the slate's pixels, which
//! the renderer's tests own.

mod support;

use std::ffi::OsStr;
use std::path::Path;

use aria_models::{
    AudioEffectInstance, CatalogKind, Clip, ClipId, EffectInstance, Generator, Project, Rational,
    TimeRange, TrackKind,
};
use aria_render::{Renderer, decode_png};
use support::{aria, scratch};

const GONE: &str = "com.example.gone.part";

/// A red solid on 0..1 s cut to a blue one on 1..2 s, and the red solid.
fn cut() -> (Project, ClipId) {
    let mut project = Project::new("missing", Rational::FPS_24);
    let solid = |rgba| Generator::SolidColor { rgba };
    // A sticker lane holds solids and transitions.
    let track = project.add_track(TrackKind::Sticker, "Solids");
    let mut place = |rgba, start| {
        project
            .add_generated(
                track,
                solid(rgba),
                TimeRange::at_rate(start, 24, Rational::FPS_24),
            )
            .expect("place a solid")
    };
    let red = place([220, 30, 30, 255], 0);
    place([30, 30, 220, 255], 24);
    (project, red)
}

/// The clip `id` of `project`.
fn clip(project: &mut Project, id: ClipId) -> &mut Clip {
    project.clip_mut(id).expect("the clip")
}

/// The saved text of [`cut`] holding `GONE` as `kind`, or nothing unlisted,
/// and the warning `aria frame` prints for it.
fn holding(kind: Option<CatalogKind>) -> (String, Option<String>) {
    let (mut project, red) = cut();
    let warned = |noun: &str, stand_in: &str, places: &str| {
        Some(format!(
            "aria: warning: no catalog lists the {noun} {GONE}, and aria loads no plugins: \
             it renders as {stand_in} in {places}\n"
        ))
    };
    let warning = match kind {
        None => None,
        Some(CatalogKind::ClipType) => {
            clip(&mut project, red).clip_type = Some(GONE.into());
            None
        }
        Some(CatalogKind::Generator) => {
            let mut generator = EffectInstance::new(GONE);
            generator.generator = true;
            clip(&mut project, red).effects = vec![generator];
            warned("generator", "a slate stating its id", "1 place")
        }
        Some(CatalogKind::Effect) => {
            clip(&mut project, red).effects =
                vec![EffectInstance::new(GONE), EffectInstance::new(GONE)];
            warned("effect", "passthrough", "2 places")
        }
        Some(CatalogKind::AudioEffect) => {
            clip(&mut project, red).audio_effects = vec![AudioEffectInstance::new(GONE)];
            warned("audio effect", "passthrough", "1 place")
        }
        Some(CatalogKind::Transition) => {
            project
                .add_transition(red, "crossfade")
                .expect("a crossfade at the cut");
            warned("transition", "a cut", "1 place")
        }
    };
    let text = project.save_to_string().expect("save");
    // The catalog holds every id `add_transition` accepts, so the unlisted id
    // goes in by text.
    (
        text.replace("\"crossfade\"", &format!("\"{GONE}\"")),
        warning,
    )
}

/// `aria frame` of `text` at 0.95 s, inside the crossfade's window: the center
/// pixel and stderr.
fn frame(dir: &Path, name: &str, text: &str) -> ([u8; 4], String) {
    let path = dir.join(format!("{name}.aria"));
    std::fs::write(&path, text).expect("write the project");
    let out = dir.join(format!("{name}.png"));
    let output = aria([
        OsStr::new("frame"),
        path.as_os_str(),
        out.as_os_str(),
        OsStr::new("--at"),
        OsStr::new("0.95"),
        OsStr::new("--fit"),
        OsStr::new("64x36"),
    ]);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{name}: {stderr}");
    let image = decode_png(&std::fs::read(&out).expect("read the frame")).expect("decode it");
    let at = ((image.height / 2 * image.width + image.width / 2) * 4) as usize;
    let pixel = image.pixels[at..at + 4]
        .try_into()
        .expect("a pixel is four bytes");
    (pixel, stderr)
}

#[test]
fn the_command_line_states_each_unlisted_entry_and_renders_its_stand_in() {
    if Renderer::new_headless().is_err() {
        return;
    }
    let dir = scratch("missing-plugins");
    let (plain_text, none) = holding(None);
    assert_eq!(none, None);
    let (plain, stderr) = frame(&dir, "plain", &plain_text);
    assert!(!stderr.contains("aria: warning"), "{stderr}");
    for kind in CatalogKind::ALL {
        let name = kind.noun().replace(' ', "-");
        let (text, warning) = holding(Some(kind));
        let (pixel, stderr) = frame(&dir, &name, &text);
        match warning {
            Some(line) => assert!(stderr.contains(&line), "{name}: {stderr}"),
            None => assert!(!stderr.contains("aria: warning"), "{name}: {stderr}"),
        }
        assert_eq!(
            stderr.matches("aria: warning").count(),
            usize::from(kind.stand_in().is_some()),
            "{name}: {stderr}"
        );
        // A slate is the one stand-in that draws something of its own.
        if kind != CatalogKind::Generator {
            assert_eq!(pixel, plain, "{name}: rendered other than without it");
        }
    }
}
