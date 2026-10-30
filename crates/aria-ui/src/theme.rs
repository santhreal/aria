//! The app theme — two concrete appearances, one token set.
//!
//! Ported from Comet (<https://github.com/zeronsh/comet>)
//! MIT License — see LICENSES/comet-MIT.txt
//!
//! Colors are precomputed from an oklch-derived neutral scale into gpui
//! [`Hsla`]. Numbers drive layout, colors are paint.

use gpui::{App, Global, Hsla, Pixels, SharedString, hsla, px};
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
/// Which appearance the app is painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Appearance {
    #[default]
    Dark,
    Light,
}
impl Appearance {
    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    pub fn is_light(self) -> bool {
        matches!(self, Self::Light)
    }

    pub fn from_window(appearance: gpui::WindowAppearance) -> Self {
        use gpui::WindowAppearance::*;
        match appearance {
            Light | VibrantLight => Self::Light,
            Dark | VibrantDark => Self::Dark,
        }
    }
}
/// The installed theme's appearance and a count of its changes, for the
/// drawing helpers below, which have no `App` to read the installed theme
/// from.
struct Painted {
    appearance: AtomicU8,
    generation: AtomicU32,
}

impl Painted {
    const fn new() -> Self {
        Self {
            appearance: AtomicU8::new(0),
            generation: AtomicU32::new(0),
        }
    }
}

/// The editor installs its theme and draws on one UI thread.
#[cfg(not(test))]
static PAINTED: Painted = Painted::new();

// Unit tests run on parallel threads of one process, each drawing the window
// of an `App` of its own. A copy per thread keeps a test that installs the
// light theme from changing the colours and panel stamps of another test's
// window while it draws.
#[cfg(test)]
thread_local! {
    static PAINTED: Painted = const { Painted::new() };
}

fn painted<R>(read: impl FnOnce(&Painted) -> R) -> R {
    #[cfg(not(test))]
    {
        read(&PAINTED)
    }
    #[cfg(test)]
    {
        PAINTED.with(read)
    }
}

pub fn current_appearance() -> Appearance {
    match painted(|painted| painted.appearance.load(Ordering::Relaxed)) {
        1 => Appearance::Light,
        _ => Appearance::Dark,
    }
}
pub fn theme_generation() -> u32 {
    painted(|painted| painted.generation.load(Ordering::Relaxed))
}
pub fn set_current_appearance(appearance: Appearance) {
    let encoded = match appearance {
        Appearance::Dark => 0,
        Appearance::Light => 1,
    };
    painted(|painted| {
        if painted.appearance.swap(encoded, Ordering::Relaxed) != encoded {
            painted.generation.fetch_add(1, Ordering::Relaxed);
        }
    });
}
pub const INK_FILL_SCALE: f32 = 1.0;
/// The share of a label's saturation a clip, a pool row and a swatch draw.
/// Lightness stays the label's own: the palette sits where both inks are
/// near 4.5:1, and a darker fill drops some hues below WCAG AA.
pub const LABEL_SATURATION: f32 = 0.5;
pub const INK_HAIRLINE_SCALE: f32 = 1.35;
/// Git history intentionally softens lane saturation so the graph remains
/// colorful without competing with content. Syntax and track tints use the
/// same treatment.
pub fn git_graph_tone(mut color: Hsla) -> Hsla {
    color.s *= 0.72;
    color
}
/// The app theme. Two concrete instances — [`Theme::dark`] and [`Theme::light`].
#[derive(Debug, Clone)]
pub struct Theme {
    pub appearance: Appearance,
    pub bg: Hsla,
    pub surface: Hsla,
    /// Tab rows, track headers, and the property name column.
    pub surface_header: Hsla,
    pub surface_raised: Hsla,
    pub surface_card: Hsla,
    pub surface_dialog: Hsla,
    pub surface_overlay: Hsla,
    pub element_hover: Hsla,
    pub element_active: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    /// Row separators.
    pub hairline: Hsla,
    pub text: Hsla,
    pub text_muted: Hsla,
    pub text_faint: Hsla,
    pub text_dim: Hsla,
    /// Editable numbers: a value that scrubs under a drag.
    pub value: Hsla,
    pub solid: Hsla,
    pub on_solid: Hsla,
    pub accent: Hsla,
    pub accent_strong: Hsla,
    pub on_accent: Hsla,
    pub danger: Hsla,
    pub danger_muted: Hsla,
    pub warning: Hsla,
    pub warning_muted: Hsla,
    pub success: Hsla,
    pub busy: Hsla,
    pub success_muted: Hsla,
    pub surface_raised_hover: Hsla,
    pub band: Hsla,
    pub input_bg: Hsla,
    pub selection: Hsla,
    pub cursor: Hsla,
    pub caret: Hsla,
    pub danger_strong: Hsla,
    pub font_sans: SharedString,
    pub font_mono: SharedString,
    pub font_sans_fallback: SharedString,
    pub font_mono_fallback: SharedString,
    // Video-editor tokens (Comet derivation, chroma 0.03).
    pub track_video: Hsla,
    pub track_audio: Hsla,
    pub track_text: Hsla,
    pub track_generator: Hsla,
    pub playhead: Hsla,
    pub snap_guide: Hsla,
    pub waveform_peak: Hsla,
    pub waveform_rms: Hsla,
    pub recording_dot: Hsla,
}
impl Theme {
    pub const GLASS_ALPHA: f32 = if cfg!(target_os = "macos") { 0.80 } else { 1.0 };
    pub const GLASS_ALPHA_LIGHT: f32 = if cfg!(target_os = "macos") { 0.80 } else { 1.0 };
    pub const GLASS_BLUR: f32 = 44.0;
    pub const HEADER_HEIGHT: f32 = 44.0;
    pub const TITLEBAR_HEIGHT: f32 = 38.0;
    pub const TITLEBAR_TOP_PAD: f32 = 2.0;
    pub const STATUS_STRIP_HEIGHT: f32 = 22.0;
    /// Corner radius of a surface that stands over the editor: a menu, a
    /// popover, a dialog, a card.
    pub const PANEL_RADIUS: f32 = 4.0;
    /// Corner radius of a control: a button, a field, a tab, a chip, a toggle,
    /// a clip. A dot or a handle drawn as a circle is round instead.
    pub const CONTROL_RADIUS: f32 = 2.0;
    pub const SPACE_XXS: f32 = 2.0;
    pub const SPACE_XS: f32 = 4.0;
    pub const SPACE_SM: f32 = 8.0;
    pub const SPACE_MD: f32 = 12.0;
    pub const SPACE_LG: f32 = 16.0;

    /// Type scale (look.md Density). Every `text_size` in the crate is one of
    /// these; `theme_tests` fails on a literal size.
    pub const TEXT_MICRO: Pixels = px(9.0);
    pub const TEXT_SMALL: Pixels = px(10.0);
    /// UI text: lists, properties, menus, dialogs, and the window default.
    pub const TEXT_UI: Pixels = px(11.0);
    /// Panel tab and title text.
    pub const TEXT_TITLE: Pixels = px(12.0);
    pub const TEXT_HEADING: Pixels = px(14.0);
    pub const TEXT_LARGE: Pixels = px(16.0);
    /// The timecode in the timeline header.
    pub const TEXT_TIMECODE: Pixels = px(18.0);
    pub const TEXT_DISPLAY: Pixels = px(28.0);

    /// Row height of lists, properties, and menus.
    pub const ROW_HEIGHT: f32 = 22.0;
    /// Height of a tab row.
    pub const TAB_ROW_HEIGHT: f32 = 24.0;
    /// Thickness of the line under the active tab. It is drawn over the tab's
    /// bottom edge, not in its layout, so every label in a row stands on one
    /// centre line.
    pub const TAB_UNDERLINE: f32 = 2.0;
    /// Height of a text box and a worded button: one control row, the height
    /// of an icon control's hit box, so a box and the controls beside it stand
    /// level.
    pub const CONTROL_HEIGHT: f32 = 24.0;
    /// Drawn size of an icon glyph.
    pub const ICON_SIZE: f32 = 16.0;
    /// Hit box of an icon control on a bar or a header row.
    pub const ICON_HIT: f32 = 24.0;
    /// Hit box of an icon control on the tool bar and the transport.
    pub const ICON_HIT_LARGE: f32 = 28.0;
    /// Hit box of an icon control inside a 22 px list or property row.
    pub const ICON_HIT_ROW: f32 = 20.0;

    pub fn glass(&self) -> Hsla {
        match self.appearance {
            Appearance::Dark => {
                if Self::GLASS_ALPHA < 1.0 {
                    grey(8).opacity(Self::GLASS_ALPHA)
                } else {
                    self.surface
                }
            }
            Appearance::Light => {
                if Self::GLASS_ALPHA_LIGHT < 1.0 {
                    grey(0xfa).opacity(Self::GLASS_ALPHA_LIGHT)
                } else {
                    self.surface
                }
            }
        }
    }

    pub fn is_glass(&self) -> bool {
        self.glass().a < 1.0
    }

    pub fn glass_hover(&self) -> Hsla {
        match self.appearance {
            Appearance::Dark => wash_for(Appearance::Dark, 0.11),
            Appearance::Light => wash_for(Appearance::Light, 0.06),
        }
    }

    pub fn glass_overlay(&self) -> Hsla {
        match self.appearance {
            Appearance::Dark => oklch(0.33, 0.0, 0.0).opacity(0.34),
            Appearance::Light => self.surface_overlay.opacity(0.85),
        }
    }

    pub fn input_glass_bg(&self) -> Hsla {
        if self.is_glass() && matches!(self.appearance, Appearance::Light) {
            self.input_bg.opacity(0.30)
        } else {
            self.input_bg
        }
    }

    pub fn card_glass_bg(&self) -> Hsla {
        if self.is_glass() {
            self.surface.opacity(0.40)
        } else {
            self.surface
        }
    }

    pub fn scrim(&self) -> Hsla {
        scrim_for(self.appearance, SCRIM_ALPHA_DARK)
    }

    pub fn window_background_appearance(&self) -> gpui::WindowBackgroundAppearance {
        if self.is_glass() {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Opaque
        }
    }

    pub fn dark() -> Self {
        // A near-black ramp with a faint cool cast, a few levels apart, so the
        // chrome reads as one material and the picture and the clips carry
        // the color.
        let hex = |value: u32| -> Hsla { gpui::rgb(value).into() };
        let text = hex(0xd8d8db);
        let text_muted = hex(0x9e9ea4);
        let text_faint = hex(0x67676d);
        let accent = accent::accent_tokens(aria_settings::ThemeChoice::default(), Appearance::Dark);
        let warning = oklch(0.828, 0.189, 84.429);
        let danger = oklch(0.704, 0.191, 22.216);
        Self {
            appearance: Appearance::Dark,
            bg: hex(0x0e0e10),
            surface: hex(0x161618),
            surface_header: hex(0x19191b),
            surface_raised: hex(0x232326),
            surface_card: hex(0x131315),
            surface_dialog: hex(0x1c1c1f),
            surface_overlay: hex(0x202023),
            element_hover: hsla(0.0, 0.0, 0.92, 0.08),
            element_active: hsla(0.0, 0.0, 0.92, 0.13),
            border: hex(0x050506),
            border_strong: hsla(0.0, 0.0, 1.0, 0.14),
            hairline: hsla(0.0, 0.0, 1.0, 0.06),
            text,
            text_muted,
            text_faint,
            text_dim: hex(0x97979d),
            value: gpui::rgb(0x5aa8ff).into(),
            solid: neutral(0.922),
            on_solid: grey(0x0e),
            accent: accent.accent,
            accent_strong: accent.accent_strong,
            on_accent: accent.on_accent,
            danger,
            danger_muted: oklch(0.808, 0.114, 19.571),
            warning,
            warning_muted: oklch(0.924, 0.12, 95.746),
            success: oklch(0.765, 0.177, 163.223),
            busy: oklch(0.718, 0.202, 349.761),
            success_muted: oklch(0.845, 0.143, 164.978),
            surface_raised_hover: hex(0x2b2b2e),
            band: band_for(Appearance::Dark),
            input_bg: hsla(0.0, 0.0, 0.0, 0.32),
            selection: accent.selection,
            cursor: hsla(0.0, 0.0, 1.0, 0.35),
            caret: accent.caret,
            danger_strong: oklch(0.58, 0.16, 25.0),
            font_sans: UI_FAMILY.into(),
            font_mono: system_mono().into(),
            font_sans_fallback: system_sans().into(),
            font_mono_fallback: system_mono().into(),
            track_video: oklch(0.65, 0.03, 250.0),
            track_audio: oklch(0.65, 0.03, 140.0),
            track_text: oklch(0.65, 0.03, 60.0),
            track_generator: oklch(0.65, 0.03, 0.0),
            playhead: accent.playhead,
            snap_guide: warning,
            waveform_peak: text_muted.opacity(0.6),
            waveform_rms: text_faint.opacity(0.4),
            recording_dot: danger,
        }
    }

    /// The dark neutral ramp inverted, with the same hues.
    pub fn light() -> Self {
        let text = grey(0x26);
        let text_muted = grey(0x52);
        let text_faint = grey(0x7d);
        let accent =
            accent::accent_tokens(aria_settings::ThemeChoice::default(), Appearance::Light);
        let warning = oklch(0.555, 0.163, 48.998);
        let danger = oklch(0.577, 0.245, 27.325);
        Self {
            appearance: Appearance::Light,
            bg: grey(0xeb),
            surface: grey(0xe3),
            surface_header: grey(0xdc),
            surface_raised: grey(0xd5),
            surface_card: grey(0xe7),
            surface_dialog: grey(0xd9),
            surface_overlay: grey(0xd1),
            element_hover: hsla(0.0, 0.0, 0.10, 0.06),
            element_active: hsla(0.0, 0.0, 0.10, 0.10),
            border: grey(0xf1),
            border_strong: hsla(0.0, 0.0, 0.0, 0.17),
            hairline: hsla(0.0, 0.0, 0.0, 0.06),
            text,
            text_muted,
            text_faint,
            text_dim: neutral(0.50),
            value: gpui::rgb(0x1a73d9).into(),
            solid: neutral(0.205),
            on_solid: neutral(0.985),
            accent: accent.accent,
            accent_strong: accent.accent_strong,
            on_accent: accent.on_accent,
            danger,
            danger_muted: oklch(0.505, 0.213, 27.518),
            warning,
            warning_muted: oklch(0.473, 0.137, 46.201),
            success: oklch(0.596, 0.145, 163.225),
            busy: oklch(0.592, 0.249, 0.584),
            success_muted: oklch(0.508, 0.118, 165.612),
            surface_raised_hover: grey(0xcc),
            band: band_for(Appearance::Light),
            input_bg: grey(0xff),
            selection: accent.selection,
            cursor: hsla(0.0, 0.0, 0.0, 0.55),
            caret: accent.caret,
            danger_strong: oklch(0.51, 0.20, 25.0),
            font_sans: UI_FAMILY.into(),
            font_mono: system_mono().into(),
            font_sans_fallback: system_sans().into(),
            font_mono_fallback: system_mono().into(),
            // Darker than the dark appearance's, so a track badge drawn in its
            // tint reads against a light surface as it does against a dark one.
            track_video: oklch(0.45, 0.03, 250.0),
            track_audio: oklch(0.45, 0.03, 140.0),
            track_text: oklch(0.45, 0.03, 60.0),
            track_generator: oklch(0.45, 0.03, 0.0),
            playhead: accent.playhead,
            snap_guide: warning,
            waveform_peak: text_muted.opacity(0.6),
            waveform_rms: text_faint.opacity(0.4),
            recording_dot: danger,
        }
    }

    pub fn for_appearance(appearance: Appearance) -> Self {
        match appearance {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light(),
        }
    }

    /// The saved theme choice, applied on top of the current appearance.
    pub fn for_choice(appearance: Appearance, choice: aria_settings::ThemeChoice) -> Self {
        Self::for_appearance(appearance).with_choice(choice)
    }

    /// Build the theme for this appearance, choice, and theme file, and install
    /// it as the one the whole window reads.
    ///
    /// Built here and nowhere else: a render reads the installed value, so the
    /// token construction below runs once per appearance or choice change
    /// rather than once per panel per frame.
    pub fn install(
        appearance: Appearance,
        choice: aria_settings::ThemeChoice,
        file: Option<&file::ThemeFile>,
        cx: &mut App,
    ) {
        set_current_appearance(appearance);
        let theme = Self::for_choice(appearance, choice);
        let theme = match file {
            Some(file) => theme.with_file(file),
            None => theme,
        };
        cx.set_global(ThemeHandle::from(theme));
    }

    /// The installed theme, as a handle.
    ///
    /// A handle rather than a borrow because a panel cannot hold a borrow of
    /// the app across the listeners it attaches, and rather than a copy
    /// because a copy is every token in the struct: this is one refcount bump.
    /// `theme.token`, `&theme` and `theme.method()` all keep working, through
    /// [`Deref`](std::ops::Deref).
    pub fn of(cx: &App) -> ThemeHandle {
        cx.global::<ThemeHandle>().clone()
    }

    pub fn ink(&self, alpha: f32) -> Hsla {
        ink_for(self.appearance, alpha)
    }

    pub fn hairline(&self, alpha: f32) -> Hsla {
        hairline_for(self.appearance, alpha)
    }

    pub fn wash(&self, alpha: f32) -> Hsla {
        wash_for(self.appearance, alpha)
    }

    pub fn glass_selected_bg(&self) -> Hsla {
        match self.appearance {
            Appearance::Dark => wash_for(Appearance::Dark, 0.11),
            Appearance::Light => wash_for(Appearance::Light, 0.06),
        }
    }

    /// The screen colour of a marker flag.
    ///
    /// One owner for the eight palette entries, so the ruler flag, the marker
    /// list, and anything that shows a marker later cannot disagree about what
    /// "Teal" looks like. Saturation and lightness are fixed and the hue is the
    /// only thing that varies, so no entry reads as brighter than another.
    pub fn marker(&self, color: aria_models::MarkerColor) -> Hsla {
        let hue = match color {
            aria_models::MarkerColor::Teal => 0.48,
            aria_models::MarkerColor::Blue => 0.58,
            aria_models::MarkerColor::Purple => 0.75,
            aria_models::MarkerColor::Pink => 0.90,
            aria_models::MarkerColor::Red => 0.99,
            aria_models::MarkerColor::Orange => 0.07,
            aria_models::MarkerColor::Yellow => 0.14,
            aria_models::MarkerColor::Green => 0.34,
        };
        let lightness = match self.appearance {
            Appearance::Dark => 0.62,
            Appearance::Light => 0.46,
        };
        hsla(hue, 0.68, lightness, 1.0)
    }

    /// The fill of a clip or pool row with `label`, or the fill a selected
    /// clip with that label draws.
    ///
    /// The same in both appearances: a label is a chosen colour, and a clip
    /// keeps it when the appearance changes. Drawn at [`LABEL_SATURATION`] of
    /// the label's saturation, so a lane of clips reads as a hue beside the
    /// picture rather than as a second picture.
    pub fn label(&self, label: aria_models::LabelColor, selected: bool) -> Hsla {
        let [r, g, b] = if selected {
            label.selected_rgb()
        } else {
            label.rgb()
        };
        let fill: Hsla = gpui::rgb(u32::from_be_bytes([0, r, g, b])).into();
        hsla(fill.h, fill.s * LABEL_SATURATION, fill.l, 1.0)
    }

    /// The text and glyph colour drawn on a `label` fill. See [`Theme::ink_on`].
    pub fn label_ink(&self, label: aria_models::LabelColor, selected: bool) -> Hsla {
        Self::ink_on(self.label(label, selected))
    }

    /// The text and glyph colour drawn on `fill`: near black on a light fill
    /// and near white on a dark one, whichever contrasts more.
    pub fn ink_on(fill: Hsla) -> Hsla {
        let (dark, light) = (grey(0x14), grey(0xf4));
        if contrast_ratio(dark, fill) >= contrast_ratio(light, fill) {
            dark
        } else {
            light
        }
    }
}
impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// A shared reference to the installed [`Theme`].
///
/// Cloning one is a refcount bump, which is the point: a render used to copy
/// every token in the struct at each of the panels that asked for the theme.
/// It derefs to [`Theme`], so a field read, a method call, and passing `&theme`
/// to a helper that takes `&Theme` all read as before.
#[derive(Clone, Debug)]
pub struct ThemeHandle(std::sync::Arc<Theme>);

impl std::ops::Deref for ThemeHandle {
    type Target = Theme;

    fn deref(&self) -> &Theme {
        &self.0
    }
}

impl From<Theme> for ThemeHandle {
    fn from(theme: Theme) -> Self {
        Self(std::sync::Arc::new(theme))
    }
}

impl Default for ThemeHandle {
    fn default() -> Self {
        Theme::default().into()
    }
}

/// The global is the handle, not the theme: whoever installs a theme hands
/// over ownership once and every reader after that shares it.
impl Global for ThemeHandle {}
pub fn neutral(lightness: f32) -> Hsla {
    let [v, _, _] = oklch_to_srgb(lightness, 0.0, 0.0);
    hsla(0.0, 0.0, v, 1.0)
}
pub fn ink(alpha: f32) -> Hsla {
    ink_for(current_appearance(), alpha)
}
pub fn ink_for(appearance: Appearance, alpha: f32) -> Hsla {
    match appearance {
        Appearance::Dark => hsla(0.0, 0.0, 1.0, alpha),
        Appearance::Light => hsla(0.0, 0.0, 0.0, alpha * INK_FILL_SCALE),
    }
}
pub fn hairline(alpha: f32) -> Hsla {
    hairline_for(current_appearance(), alpha)
}
pub fn hairline_for(appearance: Appearance, alpha: f32) -> Hsla {
    match appearance {
        Appearance::Dark => hsla(0.0, 0.0, 1.0, alpha),
        Appearance::Light => hsla(0.0, 0.0, 0.0, (alpha * INK_HAIRLINE_SCALE).min(0.5)),
    }
}
pub fn wash(alpha: f32) -> Hsla {
    wash_for(current_appearance(), alpha)
}
pub fn wash_for(appearance: Appearance, alpha: f32) -> Hsla {
    match appearance {
        Appearance::Dark => hsla(0.0, 0.0, 0.92, alpha),
        Appearance::Light => hsla(0.0, 0.0, 0.10, alpha * INK_FILL_SCALE),
    }
}
pub const SCRIM_ALPHA_DARK: f32 = 0.60;
pub fn scrim(alpha_dark: f32) -> Hsla {
    scrim_for(current_appearance(), alpha_dark)
}
pub fn scrim_for(appearance: Appearance, alpha_dark: f32) -> Hsla {
    match appearance {
        Appearance::Dark => hsla(0.0, 0.0, 0.0, alpha_dark),
        Appearance::Light => hsla(0.0, 0.0, 0.0, 0.32 * (alpha_dark / SCRIM_ALPHA_DARK)),
    }
}
pub fn band() -> Hsla {
    band_for(current_appearance())
}
pub fn band_for(appearance: Appearance) -> Hsla {
    match appearance {
        Appearance::Dark => hsla(0.0, 0.0, 0.0, 0.16),
        Appearance::Light => hsla(0.0, 0.0, 0.0, 0.045),
    }
}
#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;

// Helpers live in sibling files, declared here. Each is `pub(super)`, so the
// parent and the siblings reach it by name and nothing is left unreachable.
#[path = "theme_system.rs"]
mod system;
use system::{gamma_encode, system_mono, system_sans};

/// The accent families a theme choice selects.
#[path = "theme_accent.rs"]
mod accent;

/// The UI typeface and the features UI text is shaped with.
#[path = "theme_type.rs"]
mod typeface;
pub use typeface::*;

#[cfg(test)]
#[path = "theme_figures_tests.rs"]
mod figures_tests;

// Moved to a sibling file to stay under the file ceiling; re-exported so
// every existing path keeps resolving.
#[path = "theme_tokens.rs"]
mod tokens;
pub use tokens::*;

/// Theme files read from `<data dir>/themes`.
#[path = "theme_file.rs"]
pub mod file;
