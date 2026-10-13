//! Editor session types: routes, tools, menus, drag payloads and store state.
//!
//! Moved out of `types.rs` to stay under the file ceiling. Each type travels
//! with its own `impl` blocks, and `types.rs` re-exports this module with
//! `pub use`, so every existing `crate::types::Name` path still resolves.

use super::*;

/// Generic loadable state machine.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Loadable<T> {
    #[default]
    Idle,
    Loading,
    Ready(T),
    Error(String),
}

impl<T> Loadable<T> {
    pub fn ready(&self) -> Option<&T> {
        match self {
            Loadable::Ready(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }

    pub fn error(&self) -> Option<&str> {
        match self {
            Loadable::Error(message) => Some(message),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiGroupGhost {
    pub clip_id: u64,
    pub original_track: u64,
    pub target_track: u64,
    pub delta_ticks: i64,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UiSelectionUpdate {
    pub selected: Vec<u64>,
    pub deselected: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiParamKeyframe {
    pub tick: i32,
    pub value: f32,
    pub easing: UiEasing,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct UiParamRowState {
    pub keyframes: Vec<UiParamKeyframe>,
    pub current_value: f32,
    pub has_keyframe_at_playhead: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiGraphChannel {
    pub param_name: SharedString,
    pub keyframes: Vec<UiParamKeyframe>,
    pub color: u32,
    pub visible: bool,
}

/// One camera angle of a multicam group, as the sidebar draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct UiMulticamAngle {
    /// The pool item this angle plays.
    pub media: u64,
    /// The angle's own label, or `None` to fall back to its position.
    pub name: Option<SharedString>,
    /// Where this camera's first frame sits on the group's common clock, in
    /// ticks. A camera that started rolling late carries a positive offset.
    ///
    /// Projected because the sidebar both draws it and moves it: the waveform
    /// strip beside an angle is that camera's sound placed on the common clock,
    /// which is the offset applied to the peaks, and a nudge is read against the
    /// value the operator was looking at rather than the engine's.
    pub sync_offset: i64,
}

/// A multicam group and the cuts that say which angle is live when.
///
/// The cut list is projected, and the live angle is derived from it on read.
/// Storing the resolved index instead froze it at the tick of the last edit:
/// the projection is rebuilt when the project changes, and the playhead moves
/// per drawn frame, so scrubbing past a cut left the sidebar showing the
/// camera that was live when the cut was made.
#[derive(Debug, Clone, PartialEq)]
pub struct UiMulticamGroup {
    pub id: u64,
    pub name: SharedString,
    pub angles: Vec<UiMulticamAngle>,
    /// Cuts in tick order, as the model holds them.
    pub cuts: Vec<aria_models::MulticamCut>,
}

impl UiMulticamGroup {
    /// Index into `angles` of the angle live at `tick`.
    pub fn active_angle_at(&self, tick: i64) -> usize {
        aria_models::angle_at(&self.cuts, tick)
    }
}

/// A secondary sequence, as a panel names it.
///
/// A nest is a clip on one timeline and a timeline of its own, and the editor
/// has to say which one is open. The name comes along because an id in a
/// breadcrumb tells the operator nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct UiSequenceRef {
    pub id: u64,
    pub name: SharedString,
    /// Length of the nest's own timeline, in its own ticks.
    pub duration_ticks: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiEditorStore {
    pub project_name: SharedString,
    pub is_dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub playhead_ticks: i64,
    pub zoom: f32,
    pub scroll_x: f32,
    pub selection: Vec<u64>,
    pub sequence: UiSequence,
    pub media: Vec<UiMediaItem>,
    /// Pool folders in draw order: a parent immediately before its children,
    /// each row carrying its own depth.
    pub bins: Vec<UiBin>,
    /// Multicam groups in id order, each with the cuts that say which angle is
    /// live. A handle, because the strip that draws it cannot hold a borrow of
    /// the store across the frame and would otherwise copy every group.
    pub multicam: std::sync::Arc<[UiMulticamGroup]>,
    /// Secondary sequences in id order. Empty in a project with no nests.
    pub sequences: Vec<UiSequenceRef>,
    /// The sequence open for editing, or `None` for the project's root
    /// timeline. Everything else in this store projects whichever one it is,
    /// so this is what a breadcrumb reads to say where the edits are landing.
    pub open_sequence: Option<u64>,
}

impl Default for UiEditorStore {
    fn default() -> Self {
        Self {
            project_name: SharedString::default(),
            is_dirty: false,
            can_undo: false,
            can_redo: false,
            playhead_ticks: 0,
            zoom: 1.0,
            scroll_x: 0.0,
            selection: Vec::new(),
            sequence: UiSequence {
                tracks: Vec::new(),
                duration_ticks: 0,
                markers: Vec::new().into(),
                frame_rate: Rational { num: 24, den: 1 },
            },
            media: Vec::new(),
            bins: Vec::new(),
            multicam: std::sync::Arc::from([]),
            sequences: Vec::new(),
            open_sequence: None,
        }
    }
}

/// Who said one line of the conversation.
///
/// A closed set rather than a free string: the panel styles each kind
/// differently, and a typo in a role name would have rendered as a silently
/// unstyled bubble.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentEntryKind {
    /// The operator's prompt.
    You,
    /// The model's answer.
    Assistant,
    /// A provider-generated reasoning summary. Never sent back as history.
    Reasoning,
    /// An edit the agent applied, or a host tool it ran.
    Action,
    /// A refusal, or the reason a turn stopped.
    Error,
}

impl AgentEntryKind {
    /// Every kind, so a persisted conversation and this enum cannot drift.
    pub const ALL: [Self; 5] = [
        Self::You,
        Self::Assistant,
        Self::Reasoning,
        Self::Action,
        Self::Error,
    ];

    /// The speaker's name, as the bubble prints it.
    pub fn label(self) -> &'static str {
        match self {
            Self::You => "you",
            Self::Assistant => "assistant",
            Self::Reasoning => "thinking",
            Self::Action => "edit",
            Self::Error => "error",
        }
    }

    /// The name a saved conversation stores.
    ///
    /// Separate from [`Self::label`] on purpose: the label is display text and
    /// may be reworded, and rewording it must not make yesterday's saved
    /// conversation unreadable.
    pub fn key(self) -> &'static str {
        match self {
            Self::You => "you",
            Self::Assistant => "assistant",
            Self::Reasoning => "reasoning",
            Self::Action => "action",
            Self::Error => "error",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.key() == key)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UiAgentEntry {
    pub id: SharedString,
    pub kind: AgentEntryKind,
    pub content: SharedString,
    pub timestamp: f64,
}

impl UiAgentEntry {
    /// Append `text` to what this entry says.
    ///
    /// A streamed answer arrives in fragments, and the entry holds a
    /// `SharedString` because the transcript is redrawn every frame: growing it
    /// costs one allocation per fragment, where holding a `String` cost one per
    /// visible entry per frame. Fragments arrive tens of times a second and
    /// frames sixty, over a transcript of dozens of entries.
    pub fn append(&mut self, text: &str) {
        let mut grown = String::with_capacity(self.content.len() + text.len());
        grown.push_str(&self.content);
        grown.push_str(text);
        self.content = grown.into();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivePanel {
    Media,
    Preview,
    Timeline,
    Inspector,
    Ai,
    /// The audio mixer: one strip per lane and bus, with the faders, the
    /// output routing and the sends.
    Mixer,
}

impl ActivePanel {
    /// Every pane, so a census enumerates them from here rather than from a
    /// list that goes stale the day a pane is added.
    pub const ALL: [Self; 6] = [
        Self::Media,
        Self::Preview,
        Self::Timeline,
        Self::Inspector,
        Self::Ai,
        Self::Mixer,
    ];

    /// Whether this pane draws itself over the viewer instead of beside it.
    ///
    /// The viewer is one rectangle and three things want it: the program
    /// monitor, the assistant and the mixer. An exhaustive match rather than a
    /// predicate per panel row, so a pane added to the enum does not compile
    /// until it has answered this.
    pub fn takes_over_viewer(self) -> bool {
        match self {
            Self::Ai | Self::Mixer => true,
            Self::Media | Self::Preview | Self::Timeline | Self::Inspector => false,
        }
    }

    /// Focus `pane`, or go back to the program monitor when it already has
    /// focus.
    ///
    /// One owner for the gesture: the View menu, the palette and the keyboard
    /// all bring a pane forward, and a pane that toggled back to a different
    /// place depending on which of the three was used would be a bug nobody
    /// would find twice.
    pub fn toggle_to(self, pane: Self) -> Self {
        if self == pane { Self::Preview } else { pane }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UiRoute {
    #[default]
    Home,
    Editor,
}

/// What a drag on the timeline means. Premiere's model: the gesture is chosen by
/// tool rather than by modifier, because slip, slide, roll, and rate stretch are
/// all drags on the same pixels and a modifier stack that distinguished four of
/// them would collide with selection and snapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorTool {
    #[default]
    Pointer,
    Razor,
    Hand,
    Zoom,
    /// Move the take inside a clip, leaving the clip where it is.
    Slip,
    /// Move a clip and give the difference to its neighbours.
    Slide,
    /// Move the cut between two abutting clips.
    Roll,
    /// Change how long a clip plays without dropping any of its source.
    Rate,
    /// Trim an edge and move the rest of the lane so no gap is left.
    Ripple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditMode {
    #[default]
    Ripple,
    Overwrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMenu {
    File,
    Edit,
    Sequence,
    View,
    Window,
    Help,
}

impl AppMenu {
    pub const ALL: [Self; 6] = [
        Self::File,
        Self::Edit,
        Self::Sequence,
        Self::View,
        Self::Window,
        Self::Help,
    ];

    /// The name on the bar. One owner, so a menu cannot be titled one way in the
    /// bar and another in a test.
    pub fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::Sequence => "Sequence",
            Self::View => "View",
            Self::Window => "Window",
            Self::Help => "Help",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HomeRail {
    New,
    Open,
    #[default]
    Recents,
    Samples,
    Learn,
}

impl HomeRail {
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "New",
            Self::Open => "Open",
            Self::Recents => "Recents",
            Self::Samples => "Samples",
            Self::Learn => "Learn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarTab {
    #[default]
    Media,
    Titles,
    Shapes,
    Stickers,
    Animations,
    Masks,
    Transitions,
    /// Cloud stock search.
    Stock,
    /// The cloud sound-effect catalog.
    CloudSfx,
    /// AI generation: prompt in, media out.
    Generate,
    /// Offline transcription: captions from the selected clip's audio.
    Captions,
    /// Shots, silence, beats, and loudness over the selected clip's media.
    Analysis,
    Audio,
    /// Every ruler marker, with its time, name, colour, and note.
    Markers,
}

impl SidebarTab {
    /// Every tab, in the order the chip row shows them. The chips are generated
    /// from this, so a new tab appears in the sidebar instead of existing as a
    /// variant nothing can reach.
    pub const ALL: &'static [Self] = &[
        Self::Media,
        Self::Titles,
        Self::Shapes,
        Self::Stickers,
        Self::Animations,
        Self::Masks,
        Self::Transitions,
        Self::Stock,
        Self::CloudSfx,
        Self::Generate,
        Self::Captions,
        Self::Analysis,
        Self::Audio,
        Self::Markers,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Titles => "Titles",
            Self::Shapes => "Shapes",
            Self::Stickers => "Stickers",
            Self::Animations => "Animations",
            Self::Masks => "Masks",
            Self::Transitions => "Transitions",
            Self::Stock => "Stock",
            Self::CloudSfx => "SFX",
            Self::Generate => "Generate",
            Self::Captions => "Captions",
            Self::Analysis => "Analysis",
            Self::Audio => "Audio",
            Self::Markers => "Markers",
        }
    }
}

/// How the bin orders the pool.
///
/// Sorting is a way of reading the metadata the pool already carries, so each
/// order names the field it reads rather than an opaque "sort mode".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaSort {
    #[default]
    Name,
    Duration,
    /// Pixel count, so a 4K source sorts above a 1080p one whatever its shape.
    Resolution,
    /// Newest import first. An entry from a project saved before import times
    /// were recorded has none, and sorts after every dated one.
    DateAdded,
}

impl MediaSort {
    pub const ALL: &'static [Self] = &[
        Self::Name,
        Self::Duration,
        Self::Resolution,
        Self::DateAdded,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Duration => "Duration",
            Self::Resolution => "Resolution",
            Self::DateAdded => "Date added",
        }
    }
}

/// What a dragged sidebar catalog entry does when it lands.
///
/// The kind decides the legal target, so a drop handler can refuse a payload it
/// cannot use instead of guessing from the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogDragKind {
    /// Places a sticker clip on the lane and tick it lands on.
    Sticker,
    /// Applies an animation preset to the clip it lands on.
    Animation,
    /// Applies a mask shape to the clip it lands on.
    Mask,
    /// Applies a blend at the right cut of the clip it lands on.
    Transition,
}

/// A catalog entry in flight from the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogDrag {
    pub kind: CatalogDragKind,
    pub id: SharedString,
    pub label: SharedString,
}

impl CatalogDrag {
    /// Whether this payload applies to a clip rather than to empty lane space.
    pub fn targets_a_clip(&self) -> bool {
        !matches!(self.kind, CatalogDragKind::Sticker)
    }
}

/// A pool entry in flight from the media sidebar.
///
/// Carries the name as well as the id so the drag ghost can be drawn without
/// reading the pool back out of the state mid-gesture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDrag {
    pub media: u64,
    pub label: SharedString,
}

/// A pool folder in flight, for a drop that re-parents it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinDrag {
    pub bin: u64,
    pub label: SharedString,
}

/// Which on-canvas handles the preview offers.
///
/// One at a time, and named rather than inferred: a crop corner and a scale
/// corner sit on the same pixel when the crop is full, so guessing from the
/// clip's state would make the same drag mean two things.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayMode {
    /// Move the picture, and scale it from its corners.
    #[default]
    Transform,
    /// Move the crop window's edges.
    Crop,
    /// Move, resize, and rotate the mask.
    Mask,
}

impl OverlayMode {
    pub const ALL: [OverlayMode; 3] =
        [OverlayMode::Transform, OverlayMode::Crop, OverlayMode::Mask];

    pub fn label(self) -> &'static str {
        match self {
            OverlayMode::Transform => "Transform",
            OverlayMode::Crop => "Crop",
            OverlayMode::Mask => "Mask",
        }
    }

    /// The next mode in the cycle, wrapping.
    pub fn next(self) -> Self {
        match self {
            OverlayMode::Transform => OverlayMode::Crop,
            OverlayMode::Crop => OverlayMode::Mask,
            OverlayMode::Mask => OverlayMode::Transform,
        }
    }
}
