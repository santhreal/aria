//! One row per project-wide number to check before an export.
//!
//! `media_count`, `bin_count`, `marker_count`, `clip_count` and
//! `has_transitions` are the counts the model already keeps and nothing showed.
//! A row names one of them and reads it, so a surface renders the table rather
//! than hand-writing a label beside each call.
//!
//! Soloed lanes are read per lane kind through [`track_kind_specs`], not
//! through a list of kinds written here: a new lane kind is counted the moment
//! the lane catalog names it.

use crate::project::Project;
use crate::track::Track;
use crate::track_catalog::track_kind_specs;

/// One project-wide number or flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectFact {
    /// Sources in the pool.
    Sources,
    /// Folders in the pool.
    Folders,
    /// Lanes in the sequence.
    Lanes,
    /// Clips across every lane.
    Clips,
    /// Ruler markers.
    Markers,
    /// Exclusive end tick of the last-ending clip.
    ContentEnd,
    /// Whether any junction carries a transition.
    Transitions,
    /// Lane kinds with a lane currently soloed.
    SoloedKinds,
}

/// What one project number is called and how it reads.
#[derive(Debug, Clone, Copy)]
pub struct ProjectFactSpec {
    /// The number this row describes.
    pub fact: ProjectFact,
    /// Stable wire / UI id.
    pub id: &'static str,
    /// Operator-facing label.
    pub label: &'static str,
    /// Read it off a project.
    pub read: fn(&Project) -> FactValue,
}

/// What one project row reads, before a surface writes it as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactValue {
    /// A count.
    Count(usize),
    /// A yes-or-no answer.
    Flag(bool),
    /// Names, in the order a readout lists them; empty when nothing answers.
    Names(Vec<&'static str>),
    /// A position on the sequence, in ticks.
    Tick(i64),
}

/// The text a yes-or-no row reads when the answer is yes.
const YES: &str = "Yes";
/// The text a yes-or-no row reads when the answer is no.
const NO: &str = "No";
/// The text a list row reads when the list is empty.
const NONE: &str = "None";

impl FactValue {
    /// Whether this is the empty answer: zero, no, none, or the sequence's
    /// first tick.
    ///
    /// A surface draws an empty answer dim rather than hiding it.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Count(count) => *count == 0,
            Self::Flag(flag) => !flag,
            Self::Names(names) => names.is_empty(),
            Self::Tick(tick) => *tick == 0,
        }
    }

    /// The value as a readout draws it.
    ///
    /// `position` writes a [`FactValue::Tick`]: a sequence reads a position as
    /// timecode, frames, or feet and frames, as its time display selects, and
    /// that formatter belongs to the surface that draws the ruler.
    pub fn text(&self, position: impl FnOnce(i64) -> String) -> String {
        match self {
            Self::Count(count) => count.to_string(),
            Self::Flag(true) => YES.to_string(),
            Self::Flag(false) => NO.to_string(),
            Self::Names(names) if names.is_empty() => NONE.to_string(),
            Self::Names(names) => names.join(", "),
            Self::Tick(tick) => position(*tick),
        }
    }
}

/// The lane kinds with a soloed lane, by label, in lane-catalog order.
///
/// Derived from the lane catalog so a new kind is covered without an edit here.
fn soloed_kinds(project: &Project) -> Vec<&'static str> {
    track_kind_specs()
        .iter()
        .filter(|spec| project.timeline().has_solo(spec.kind))
        .map(|spec| spec.label)
        .collect()
}

/// The shipped numbers, in the order a readout lists them.
const SHIPPED: &[ProjectFactSpec] = &[
    ProjectFactSpec {
        fact: ProjectFact::Sources,
        id: "sources",
        label: "Sources in pool",
        read: |project| FactValue::Count(project.media_count()),
    },
    ProjectFactSpec {
        fact: ProjectFact::Folders,
        id: "folders",
        label: "Pool folders",
        read: |project| FactValue::Count(project.bin_count()),
    },
    ProjectFactSpec {
        fact: ProjectFact::Lanes,
        id: "lanes",
        label: "Lanes",
        read: |project| FactValue::Count(project.timeline().tracks_ordered().count()),
    },
    ProjectFactSpec {
        fact: ProjectFact::Clips,
        id: "clips",
        label: "Clips",
        read: |project| FactValue::Count(project.timeline().clip_count()),
    },
    ProjectFactSpec {
        fact: ProjectFact::Markers,
        id: "markers",
        label: "Markers",
        read: |project| FactValue::Count(project.timeline().marker_count()),
    },
    ProjectFactSpec {
        fact: ProjectFact::ContentEnd,
        id: "content_end",
        label: "Content ends at",
        read: |project| {
            FactValue::Tick(
                project
                    .timeline()
                    .tracks_ordered()
                    .map(Track::content_end)
                    .max()
                    .unwrap_or(0),
            )
        },
    },
    ProjectFactSpec {
        fact: ProjectFact::Transitions,
        id: "transitions",
        label: "Carries transitions",
        read: |project| FactValue::Flag(project.has_transitions()),
    },
    ProjectFactSpec {
        fact: ProjectFact::SoloedKinds,
        id: "soloed_kinds",
        label: "Soloed lane kinds",
        read: |project| FactValue::Names(soloed_kinds(project)),
    },
];

/// Which row describes `fact`.
///
/// Exhaustive on purpose: a number added without a row does not compile.
const fn index_of(fact: ProjectFact) -> usize {
    match fact {
        ProjectFact::Sources => 0,
        ProjectFact::Folders => 1,
        ProjectFact::Lanes => 2,
        ProjectFact::Clips => 3,
        ProjectFact::Markers => 4,
        ProjectFact::ContentEnd => 5,
        ProjectFact::Transitions => 6,
        ProjectFact::SoloedKinds => 7,
    }
}

/// Every project number, in declaration order.
pub const fn project_facts() -> &'static [ProjectFactSpec] {
    SHIPPED
}

impl ProjectFact {
    /// The row describing this number.
    pub const fn spec(self) -> &'static ProjectFactSpec {
        &SHIPPED[index_of(self)]
    }

    /// Stable wire / UI id.
    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    /// Operator-facing label.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Read this number off `project`.
    pub fn read(self, project: &Project) -> FactValue {
        (self.spec().read)(project)
    }
}
