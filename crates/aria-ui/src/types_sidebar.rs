//! The sidebar's tabs, and the groups its top chip row lists.

/// What the sidebar draws below its chip rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarTab {
    #[default]
    Media,
    Titles,
    Shapes,
    Stickers,
    /// Effects that draw a picture of their own, placed as clips.
    Generators,
    Animations,
    Masks,
    Transitions,
    /// Every effect, transition, generator and preset as one searchable tree.
    Effects,
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
}

impl SidebarTab {
    /// Every tab, group by group in chip order. [`SidebarGroup::tabs`] slices
    /// this list, so the tabs of one group are adjacent and the groups follow
    /// [`SidebarGroup::ALL`].
    pub const ALL: &'static [Self] = &[
        Self::Media,
        Self::Audio,
        Self::Titles,
        Self::Shapes,
        Self::Stickers,
        Self::Masks,
        Self::Effects,
        Self::Transitions,
        Self::Generators,
        Self::Animations,
        Self::Stock,
        Self::CloudSfx,
        Self::Generate,
        Self::Captions,
        Self::Analysis,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Titles => "Titles",
            Self::Shapes => "Shapes",
            Self::Stickers => "Stickers",
            Self::Generators => "Generators",
            Self::Animations => "Animations",
            Self::Masks => "Masks",
            Self::Transitions => "Transitions",
            Self::Effects => "Effects",
            Self::Stock => "Stock",
            Self::CloudSfx => "SFX",
            Self::Generate => "Generate",
            Self::Captions => "Captions",
            Self::Analysis => "Analysis",
            Self::Audio => "Audio",
        }
    }

    /// Whether this tab lists the pool.
    pub const fn shows_pool(self) -> bool {
        matches!(self, Self::Media | Self::Audio)
    }

    /// The group whose chip row lists this tab.
    pub fn group(self) -> SidebarGroup {
        match self {
            Self::Media | Self::Audio => SidebarGroup::Media,
            Self::Titles | Self::Shapes | Self::Stickers | Self::Masks => SidebarGroup::Graphics,
            Self::Effects | Self::Transitions | Self::Generators | Self::Animations => {
                SidebarGroup::Effects
            }
            Self::Stock | Self::CloudSfx | Self::Generate => SidebarGroup::Library,
            Self::Captions | Self::Analysis => SidebarGroup::Analyze,
        }
    }
}

/// One chip of the sidebar's top row. The chips are generated from
/// [`SidebarGroup::ALL`] and the second row from the open group's
/// [`SidebarGroup::tabs`], so a new tab appears under its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarGroup {
    Media,
    Graphics,
    Effects,
    Library,
    Analyze,
}

impl SidebarGroup {
    pub const ALL: [Self; 5] = [
        Self::Media,
        Self::Graphics,
        Self::Effects,
        Self::Library,
        Self::Analyze,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Media => "Media",
            Self::Graphics => "Graphics",
            Self::Effects => "Effects",
            Self::Library => "Library",
            Self::Analyze => "Analyze",
        }
    }

    /// The tabs the group lists, in chip order. The first opens when the
    /// group is chosen.
    pub fn tabs(self) -> &'static [SidebarTab] {
        let all = SidebarTab::ALL;
        let Some(start) = all.iter().position(|tab| tab.group() == self) else {
            return &[];
        };
        let len = all[start..]
            .iter()
            .take_while(|tab| tab.group() == self)
            .count();
        &all[start..start + len]
    }
}

#[cfg(test)]
mod tests {
    use super::{SidebarGroup, SidebarTab};

    /// The class closed: a tab no chip row reaches. The group rows joined in
    /// order must be [`SidebarTab::ALL`]; a tab listed away from the rest of
    /// its group, or a group out of order, drops out of the join.
    ///
    /// Not caught: a variant missing from `ALL` altogether, which the enum
    /// cannot enumerate on its own.
    #[test]
    fn the_group_rows_list_every_tab_once_in_order() {
        let joined: Vec<SidebarTab> = SidebarGroup::ALL
            .iter()
            .flat_map(|group| group.tabs().iter().copied())
            .collect();
        assert_eq!(joined, SidebarTab::ALL);
        for group in SidebarGroup::ALL {
            let tabs = group.tabs();
            assert!(!tabs.is_empty(), "{group:?} has no tab to open");
            for tab in tabs {
                assert_eq!(tab.group(), group, "{tab:?} is listed under {group:?}");
            }
        }
    }
}
