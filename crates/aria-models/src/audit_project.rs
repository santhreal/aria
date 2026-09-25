//! One row per project-wide number an operator wants before an export.
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
    /// Read it, already formatted for a readout.
    pub read: fn(&Project) -> String,
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
        read: |project| project.media_count().to_string(),
    },
    ProjectFactSpec {
        fact: ProjectFact::Folders,
        id: "folders",
        label: "Pool folders",
        read: |project| project.bin_count().to_string(),
    },
    ProjectFactSpec {
        fact: ProjectFact::Lanes,
        id: "lanes",
        label: "Lanes",
        read: |project| project.timeline().tracks_ordered().count().to_string(),
    },
    ProjectFactSpec {
        fact: ProjectFact::Clips,
        id: "clips",
        label: "Clips",
        read: |project| project.timeline().clip_count().to_string(),
    },
    ProjectFactSpec {
        fact: ProjectFact::Markers,
        id: "markers",
        label: "Markers",
        read: |project| project.timeline().marker_count().to_string(),
    },
    ProjectFactSpec {
        fact: ProjectFact::ContentEnd,
        id: "content_end",
        label: "Content ends at tick",
        read: |project| {
            project
                .timeline()
                .tracks_ordered()
                .map(Track::content_end)
                .max()
                .unwrap_or(0)
                .to_string()
        },
    },
    ProjectFactSpec {
        fact: ProjectFact::Transitions,
        id: "transitions",
        label: "Carries transitions",
        read: |project| {
            if project.has_transitions() {
                "yes".to_string()
            } else {
                "no".to_string()
            }
        },
    },
    ProjectFactSpec {
        fact: ProjectFact::SoloedKinds,
        id: "soloed_kinds",
        label: "Soloed lane kinds",
        read: |project| {
            let soloed = soloed_kinds(project);
            if soloed.is_empty() {
                "none".to_string()
            } else {
                soloed.join(", ")
            }
        },
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
    pub fn read(self, project: &Project) -> String {
        (self.spec().read)(project)
    }
}
