//! Scrubbable values: a number a press either drags or opens as text.
//!
//! One gesture for every numeric readout, in the inspector and in the timeline
//! property rows. A press arms it. Moving the pointer more than
//! [`CLICK_SLOP_PX`] turns it into a drag that moves the value one unit per
//! [`PX_PER_UNIT`] pixels, ten units with Shift and a tenth of one with Ctrl.
//! Travel is relative motion: the shell locks the pointer for the drag, so the
//! cursor hides, holds its place, and never stops the drag at a screen edge.
//! Letting go without moving opens the value as a text field: Enter commits,
//! Escape reverts, Tab commits and opens the next value drawn after it.
//!
//! A drag is one undo step. Every move rolls back the group the previous move
//! applied and writes the whole travel from the state the press found, so a
//! write that reads the clip's current value and one that adds to the value the
//! row was drawn with land at the same place, and Escape restores the value
//! exactly.

use std::rc::Rc;

use aria_engine::EngineError;
use aria_plugin::EditGroup;

use super::AppState;
use crate::motion::MotionKey;

/// Pointer travel, in pixels, that moves a value one unit.
pub const PX_PER_UNIT: f32 = 2.0;

/// Pointer travel under which a press and release is a click.
pub const CLICK_SLOP_PX: f32 = 3.0;

/// Shift multiplies the rate by this.
pub const COARSE: f32 = 10.0;

/// Ctrl multiplies the rate by this.
pub const FINE: f32 = 0.1;

/// The longest draft a value field holds.
const DRAFT_CAP: usize = 24;

/// Moves the value by a delta, in the units the readout prints, from the state
/// the gesture started on.
pub type Write = Rc<dyn Fn(&mut AppState, f32) -> Result<(), EngineError>>;

/// How a value is printed into, and read back out of, its text field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ValueKind {
    /// A number with `decimals` digits after the point.
    Number { decimals: u8 },
    /// A frame count, typed as a frame count or as timecode at this rate.
    Frames { num: i32, den: i32 },
}

impl ValueKind {
    /// The text a field opens with.
    pub fn format(self, value: f32) -> String {
        match self {
            Self::Number { decimals } => format!("{value:.*}", usize::from(decimals)),
            Self::Frames { num, den } => {
                crate::timecode::format_ticks(value.round() as i64, num, den)
            }
        }
    }

    /// The value typed text states, or `None` when it states none.
    pub fn parse(self, text: &str) -> Option<f32> {
        let text = text.trim();
        let value = match self {
            Self::Number { .. } => text.parse::<f32>().ok()?,
            Self::Frames { num, den } => crate::timecode::parse_ticks(text, num, den)? as f32,
        };
        value.is_finite().then_some(value)
    }

    /// The finest change a drag writes. A frame count moves by whole frames,
    /// and a number by no less than the digits it prints.
    fn grain(self, unit: f32) -> f32 {
        match self {
            Self::Frames { .. } => unit.max(1.0),
            Self::Number { decimals } => (unit * FINE).max(10f32.powi(-i32::from(decimals))),
        }
    }

    /// Whether `ch` can be part of a typed value.
    pub(crate) fn accepts(self, ch: char) -> bool {
        match self {
            Self::Number { .. } => ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+'),
            Self::Frames { .. } => ch.is_ascii_digit() || matches!(ch, ':' | ';' | '.'),
        }
    }
}

/// One readout a gesture can move.
#[derive(Clone)]
pub struct ScrubTarget {
    /// The readout's identity: the row draws its field and its lit state when
    /// the open gesture carries this key.
    pub key: MotionKey,
    /// The value as the readout prints it.
    pub value: f32,
    /// What one unit of travel moves the value by.
    pub unit: f32,
    pub kind: ValueKind,
    pub write: Write,
}

impl std::fmt::Debug for ScrubTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrubTarget")
            .field("key", &self.key)
            .field("value", &self.value)
            .field("unit", &self.unit)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

/// A press on a readout, until it is released.
struct Press {
    target: ScrubTarget,
    /// Pointer travel, in pixels, from the press until the drag passes the
    /// click slop.
    offset: f32,
    /// Units travelled so far, rate changes included.
    travel: f32,
    /// Past the click slop: this press is a drag and will not open a field.
    dragging: bool,
    /// The delta the open group holds.
    applied: f32,
    group: Option<EditGroup>,
}

/// A readout open as text.
#[derive(Debug)]
pub struct ValueEntry {
    pub target: ScrubTarget,
    pub draft: String,
    /// The whole draft is selected: the next character replaces it and
    /// Backspace clears it.
    pub all: bool,
}

/// The readouts drawn on either side of the open field, so Tab and Shift+Tab
/// can open them. Rows report themselves while they paint.
#[derive(Default)]
struct Neighbours {
    last: Option<ScrubTarget>,
    before: Option<ScrubTarget>,
    after: Option<ScrubTarget>,
}

/// Every in-flight value gesture.
#[derive(Default)]
pub struct ValueScrubs {
    press: Option<Press>,
    entry: Option<ValueEntry>,
    neighbours: Neighbours,
}

impl std::fmt::Debug for ValueScrubs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ValueScrubs")
            .field(
                "pressed",
                &self.press.as_ref().map(|press| press.target.key),
            )
            .field("entry", &self.entry)
            .finish_non_exhaustive()
    }
}

impl ValueScrubs {
    /// The open field, when it belongs to `key`.
    pub fn entry_for(&self, key: MotionKey) -> Option<&ValueEntry> {
        self.entry.as_ref().filter(|entry| entry.target.key == key)
    }

    /// Whether a field is open anywhere.
    pub fn entry_open(&self) -> bool {
        self.entry.is_some()
    }

    /// Whether the readout `key` is being dragged.
    pub fn dragging(&self, key: MotionKey) -> bool {
        self.press
            .as_ref()
            .is_some_and(|press| press.dragging && press.target.key == key)
    }

    /// Whether a press is held anywhere.
    pub fn pressed(&self) -> bool {
        self.press.is_some()
    }

    /// Whether a held press has passed the click slop. The shell locks the
    /// pointer while one is.
    pub fn drag_held(&self) -> bool {
        self.press.as_ref().is_some_and(|press| press.dragging)
    }

    /// A readout painted. While a field is open this records the readouts
    /// either side of it in paint order, wrapping from the last to the first.
    pub fn painted(&mut self, target: &ScrubTarget) {
        let Some(open) = self.entry.as_ref().map(|entry| entry.target.key) else {
            return;
        };
        let neighbours = &mut self.neighbours;
        if neighbours
            .last
            .as_ref()
            .is_some_and(|last| last.key == open)
            && target.key != open
        {
            neighbours.after = Some(target.clone());
        }
        if target.key == open {
            neighbours.before = neighbours.last.take().filter(|last| last.key != open);
        }
        neighbours.last = Some(target.clone());
    }
}

/// The rate one pixel of travel moves at under the held modifiers.
pub fn rate(shift: bool, ctrl: bool) -> f32 {
    let mut rate = 1.0 / PX_PER_UNIT;
    if shift {
        rate *= COARSE;
    }
    if ctrl {
        rate *= FINE;
    }
    rate
}

impl AppState {
    /// The value gestures, for a row to draw its field and lit state from.
    pub fn value_scrubs(&self) -> &ValueScrubs {
        &self.value_scrubs
    }

    /// A readout painted; see [`ValueScrubs::painted`].
    pub fn value_painted(&mut self, target: &ScrubTarget) {
        self.value_scrubs.painted(target);
    }

    /// Press on a readout. An open field is committed first, as a click
    /// elsewhere commits a text box.
    pub fn press_value(&mut self, target: ScrubTarget) {
        if self.value_scrubs.entry.is_some() {
            self.commit_value_entry();
        }
        self.cancel_value_drag();
        self.value_scrubs.press = Some(Press {
            target,
            offset: 0.0,
            travel: 0.0,
            dragging: false,
            applied: 0.0,
            group: None,
        });
    }

    /// The pointer moved `dx` pixels with the press held. The travel is
    /// relative, so a locked pointer that holds its place still drags. Returns
    /// whether the value changed.
    pub fn drag_value(&mut self, dx: f32, shift: bool, ctrl: bool) -> bool {
        let Some(press) = self.value_scrubs.press.as_mut() else {
            return false;
        };
        let mut moved = dx;
        if !press.dragging {
            press.offset += dx;
            if press.offset.abs() < CLICK_SLOP_PX {
                return false;
            }
            press.dragging = true;
            // The travel inside the slop counts toward the value.
            moved = press.offset;
        }
        press.travel += moved * rate(shift, ctrl);
        let grain = press.target.kind.grain(press.target.unit);
        let delta = (press.travel * press.target.unit / grain).round() * grain;
        if delta.to_bits() == press.applied.to_bits() {
            return false;
        }
        let write = Rc::clone(&press.target.write);
        if let Some(group) = press.group.take() {
            self.roll_back(group);
            self.sync_from_engine();
        }
        let wrote = if delta == 0.0 {
            Ok(None)
        } else {
            self.edit_group(|state| write(state, delta))
                .map(|(group, ())| Some(group))
        };
        let press = self.value_scrubs.press.as_mut().expect("held above");
        match wrote {
            Ok(group) => {
                press.applied = delta;
                press.group = group;
            }
            Err(err) => {
                press.applied = 0.0;
                self.sync_from_engine();
                self.report_error(&err);
            }
        }
        true
    }

    /// Let go of the press. A drag keeps what it wrote as one undo step; a
    /// press that never moved opens the value as text. Returns whether a press
    /// was held.
    pub fn release_value(&mut self) -> bool {
        let Some(press) = self.value_scrubs.press.take() else {
            return false;
        };
        if press.dragging {
            if let Some(group) = press.group {
                group.commit();
                // The commit changes the History list and not the project.
                self.sync_from_engine();
            }
            return true;
        }
        let draft = press.target.kind.format(press.target.value);
        self.value_scrubs.neighbours = Neighbours::default();
        self.value_scrubs.entry = Some(ValueEntry {
            target: press.target,
            draft,
            all: true,
        });
        true
    }

    /// Escape during a drag: put the value back where the press found it.
    /// Returns whether a drag was held.
    pub fn cancel_value_drag(&mut self) -> bool {
        let Some(press) = self.value_scrubs.press.take() else {
            return false;
        };
        if let Some(group) = press.group {
            self.roll_back(group);
            self.sync_from_engine();
        }
        true
    }

    /// Whether a value field holds the keyboard.
    pub fn value_entry_open(&self) -> bool {
        self.value_scrubs.entry.is_some()
    }

    /// What the open value field holds.
    pub fn value_entry_text(&self) -> String {
        self.value_scrubs
            .entry
            .as_ref()
            .map(|entry| entry.draft.clone())
            .unwrap_or_default()
    }

    pub fn value_entry_push(&mut self, ch: char) {
        let Some(entry) = self.value_scrubs.entry.as_mut() else {
            return;
        };
        if !entry.target.kind.accepts(ch) {
            return;
        }
        if entry.all {
            entry.draft.clear();
            entry.all = false;
        }
        if entry.draft.len() < DRAFT_CAP {
            entry.draft.push(ch);
        }
    }

    pub fn value_entry_pop(&mut self) {
        let Some(entry) = self.value_scrubs.entry.as_mut() else {
            return;
        };
        if entry.all {
            entry.draft.clear();
            entry.all = false;
        } else {
            entry.draft.pop();
        }
    }

    /// Escape: close the field and keep the value it opened on.
    pub fn cancel_value_entry(&mut self) {
        self.value_scrubs.entry = None;
    }

    /// Enter: write the typed value and close the field. Text that states no
    /// value keeps the field open and reports why.
    pub fn commit_value_entry(&mut self) -> bool {
        let Some(entry) = self.value_scrubs.entry.as_ref() else {
            return false;
        };
        let Some(typed) = entry.target.kind.parse(&entry.draft) else {
            let message = match entry.target.kind {
                ValueKind::Number { .. } => format!("{} is not a number.", entry.draft),
                ValueKind::Frames { .. } => format!(
                    "{} is not a duration. Type frames or HH:MM:SS:FF.",
                    entry.draft
                ),
            };
            self.report_error(message);
            return false;
        };
        let entry = self.value_scrubs.entry.take().expect("checked above");
        let delta = typed - entry.target.value;
        if delta == 0.0 {
            return true;
        }
        let write = entry.target.write;
        if let Err(err) = self.one_step(|state| write(state, delta)) {
            self.report_error(&err);
        }
        true
    }

    /// Tab: commit, then open the readout painted after this one, or before it
    /// with Shift.
    pub fn tab_value_entry(&mut self, back: bool) {
        // A draft that states no value keeps its field, and its neighbours.
        if !self.commit_value_entry() {
            return;
        }
        let neighbours = std::mem::take(&mut self.value_scrubs.neighbours);
        let next = if back {
            neighbours.before
        } else {
            neighbours.after
        };
        if let Some(target) = next {
            let draft = target.kind.format(target.value);
            self.value_scrubs.entry = Some(ValueEntry {
                target,
                draft,
                all: true,
            });
        }
    }
}
