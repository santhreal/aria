//! The key every motion store uses to name one animated control.

use std::fmt::{self, Write as _};
use std::hash::{Hash, Hasher};

use gpui::ElementId;

/// One animated thing's identity: which kind of control, and which instance.
///
/// Shared by the hover store, the glide store, and every one-shot element id,
/// so a control is named the same way wherever it moves.
///
/// A key is built on every draw of every animated element, so it holds a hash
/// rather than the text. The string form allocated twice per hovered
/// control per frame — once to format `kind:id`, once for the map's owned copy
/// — and re-hashed those bytes on every lookup. The timeline builds one per
/// clip, per trim grip and per header toggle, so the cost scaled with the cut,
/// not with what the operator was pointing at.
///
/// FNV-1a over `kind`, a separator no id contains, and the id's `Display`
/// bytes. Written straight into the hasher, so no intermediate string exists.
/// A collision would blend two controls' hover fades; at the few hundred keys
/// a frame holds, that is far below the rate at which a dropped frame would
/// hide it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MotionKey(u64);

/// FNV-1a, fed by `fmt::Write` so `Display` output never lands in a `String`.
struct MotionHasher(u64);

impl fmt::Write for MotionHasher {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for byte in text.as_bytes() {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Ok(())
    }
}

impl MotionKey {
    /// The key for one control instance. `kind` separates two controls that
    /// were given the same id — a row and the button inside it.
    pub fn new(kind: &str, id: impl fmt::Display) -> Self {
        let mut hasher = MotionHasher(0xcbf2_9ce4_8422_2325);
        // Both writes are infallible: `MotionHasher` only ever returns `Ok`.
        let _ = hasher.write_str(kind);
        let _ = hasher.write_char('\u{1f}');
        let _ = write!(hasher, "{id}");
        Self(hasher.0)
    }

    /// The same identity as a GPUI element id.
    ///
    /// A one-shot transition is keyed by element id, not by a motion store, and
    /// an id built with `format!` allocates on every frame its element draws.
    /// `NamedInteger` over the hash costs nothing: the name is a static string
    /// and the integer is already computed.
    pub fn element_id(self, name: &'static str) -> ElementId {
        ElementId::named_usize(name, self.0 as usize)
    }

    /// A key one part deeper: a control inside a control that already has a key.
    ///
    /// Folds the extra part into the same hash, so a three-part identity — an
    /// effect slot's parameter row, say — costs no more than a two-part one and
    /// still separates `("fx", 1).with("gain")` from `("fx", 11).with("ain")`.
    pub fn with(self, part: impl fmt::Display) -> Self {
        let mut hasher = MotionHasher(self.0);
        // Infallible, as in `new`.
        let _ = hasher.write_char('\u{1f}');
        let _ = write!(hasher, "{part}");
        Self(hasher.0)
    }
}

/// The identity a draw hands a shared control.
///
/// Two shapes and no third: a name fixed at compile time, or a key hashed from
/// parts. There is deliberately no conversion from `String` or `SharedString`,
/// which is what this type is for — `ElementId` and `SharedString` both convert
/// from `String`, so a control id built with `format!` compiled, and it
/// allocated on every frame the control drew. A multicam angle chip, a mixer
/// fader, a job's progress bar and a curve parameter chip all did exactly that,
/// once per angle, strip, job and property, per frame.
///
/// A fixed name costs nothing to share and stays readable at the call site, so
/// the common case is unchanged: `ghost_button("bin-new", …)` still compiles. An
/// id that varies per row is a [`MotionKey`], which hashes its parts without
/// building a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlId {
    /// A name written in the source, unique among the controls that share a
    /// surface.
    Fixed(&'static str),
    /// A hashed identity, for a control drawn once per row of something.
    Key(MotionKey),
}

impl From<&'static str> for ControlId {
    fn from(name: &'static str) -> Self {
        Self::Fixed(name)
    }
}

impl From<MotionKey> for ControlId {
    fn from(key: MotionKey) -> Self {
        Self::Key(key)
    }
}

impl ControlId {
    /// The element id GPUI addresses this control by.
    ///
    /// `kind` names the control the id is being used for, so one identity can
    /// carry a row and the buttons inside it without them colliding. A fixed
    /// name is already unique in its surface and is used as it stands, which
    /// keeps the ids in the source readable in a debugger.
    pub fn element(self, kind: &'static str) -> ElementId {
        match self {
            Self::Fixed(name) => ElementId::Name(gpui::SharedString::new_static(name)),
            Self::Key(key) => key.element_id(kind),
        }
    }

    /// The source name when this is a fixed control.
    ///
    /// Tooltip registries key their copy by the same static name as the
    /// control. A row-derived key has no shared tooltip name and returns
    /// `None`; no text is recovered from its hash.
    pub fn fixed_name(self) -> Option<&'static str> {
        match self {
            Self::Fixed(name) => Some(name),
            Self::Key(_) => None,
        }
    }

    /// The motion key this control's hover, glide and wait are stored under.
    pub fn motion(self, kind: &'static str) -> MotionKey {
        match self {
            Self::Fixed(name) => MotionKey::new(kind, name),
            Self::Key(key) => key.with(kind),
        }
    }

    /// A phase offset in `0.0..1.0`, so two loaders drawn side by side do not
    /// pulse in lockstep.
    pub fn phase(self) -> f32 {
        let bits = match self {
            Self::Fixed(name) => MotionKey::new("phase", name).0,
            Self::Key(key) => key.0,
        };
        (bits % 1000) as f32 / 1000.0
    }
}

/// A phase offset in `[0, 1)` derived from a loader's key.
///
/// Two loaders on screen run off the same pulse clock, so without an offset
/// they pulse in lockstep and read as one animation drawn twice. Hashing the
/// key keeps the offset stable across frames: the same row lands on the same
/// phase every frame.
pub fn key_phase(key: &str) -> f32 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    // The low 16 bits are enough resolution for a phase and keep the divide
    // exact in f32.
    (hasher.finish() & 0xFFFF) as f32 / 65_536.0
}
