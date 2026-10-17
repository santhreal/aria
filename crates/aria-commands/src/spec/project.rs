//! Project-level command rows: project settings, canvas, audio layout,
//! markers, media pool / bins, subclips, sequence nesting, and session lifecycle.

macro_rules! project_edit_rows {
    ($next:path, $($accum:tt)*) => {
        $next! {
            $($accum)*
            addresses nothing {
                AddTrack { label: "Add track", reach: operator("the palette's adjustment-layer and mixer-bus rows, and the Sequence menu"), junctions: keeps, lock: guarded,
                    undo: restores("the stack by removing the lane it created"), },
                AddMarker { label: "Add marker", reach: operator("the palette's Add Marker row, the marker chord, and the markers panel"), junctions: keeps, lock: guarded,
                    undo: restores("the ruler by removing the marker"), },
                RemoveMarker { label: "Remove marker", reach: operator("the markers panel's delete button"), junctions: keeps, lock: guarded,
                    undo: restores("the marker with its own id, position, name, colour and note"), },
                SetMarker { label: "Edit marker", reach: operator("the markers panel: the colour cycle and the rename field"), junctions: keeps, lock: guarded,
                    undo: restores("the marker's previous position, name, colour and note"), },
                SetProjectName { label: "Rename project", reach: operator("the rename bar, opened from the title in the window bar"), junctions: keeps, lock: guarded,
                    undo: restores("the project's previous name"), },
                SetCanvas { label: "Set canvas", reach: operator("the settings dialog's canvas pickers: the aspect rows and the delivery sizes, plus the palette's reset-background row"), junctions: keeps, lock: guarded,
                    undo: restores("the previous aspect, background and explicit size"), },
                SetSequenceRate { label: "Set sequence frame rate", reach: operator("the settings dialog's frame-rate picker"), junctions: keeps, lock: guarded,
                    undo: restores("the previous sequence frame rate and clip placements"), },
                SetAudioLayout { label: "Set audio layout", reach: operator("the settings dialog's audio-layout picker"), junctions: keeps, lock: guarded,
                    undo: restores("the previous audio channel layout"), },
                SetTimeDisplay { label: "Set time display", reach: operator("the settings dialog's time-display picker"), junctions: keeps, lock: guarded,
                    undo: restores("the previous time display"), },
                SetMasterMix { label: "Master mix", reach: operator("the mixer's master strip fader, pan, mute and automation controls"), junctions: keeps, lock: guarded,
                    undo: restores("the master strip's previous fader, pan, mute, automation mode and envelopes"), },
                CreateBin { label: "New bin", reach: operator("the palette's New Bin row, and the sidebar's new-bin button"), junctions: keeps, lock: guarded,
                    undo: restores("the pool by deleting the folder it created"), },
                RenameBin { label: "Rename bin", reach: operator("the rename bar, opened from a bin in the sidebar"), junctions: keeps, lock: guarded,
                    undo: restores("the folder's previous name"), },
                RemoveBin { label: "Delete bin", reach: operator("the sidebar's bin context menu: the delete item"), junctions: keeps, lock: guarded,
                    undo: restores("the folder with its own id, and puts its media and child folders back inside it"), },
                MoveBin { label: "Move bin", reach: operator("dragging a bin onto another in the sidebar"), junctions: keeps, lock: guarded,
                    undo: restores("the folder's previous parent"), },
                MoveMediaToBin { label: "Move media to bin", reach: operator("dragging a pool item onto a bin in the sidebar"), junctions: keeps, lock: guarded,
                    undo: restores("the source's previous folder"), },
                SetPoolItemPosition { label: "Set pool item position", reach: operator("dragging a pool item within its bin"), junctions: keeps, lock: guarded,
                    undo: restores("the item's previous position in the bin"), },
                CreateSubclip { label: "Create subclip", reach: operator("the sidebar's subclip button, over a marked source range"), junctions: keeps, lock: guarded,
                    undo: restores("the pool by deleting the subclip it created"), },
                CreateMulticamGroup { label: "Create multicam group", reach: operator("the sidebar's multicam button, over the selected pool item or the whole pool"), junctions: keeps, lock: guarded,
                    undo: restores("the pool by deleting the multicam group it created"), },
                SwitchMulticamAngle { label: "Switch multicam angle", reach: operator("the sidebar's multicam angle rows"), junctions: keeps, lock: guarded,
                    undo: restores("the multicam group's previous angle switches"), },
                SetMulticamSync { label: "Set multicam sync offset", reach: operator("the sidebar's angle rows: the nudge arrows beside an angle's waveform, and the group's Sync Audio button"), junctions: keeps, lock: guarded,
                    undo: restores("the angle's previous offset on the group's clock"), },
                OpenSequence { label: "Open sequence", reach: operator("the Sequence menu's Open Nested Sequence and Leave Nested Sequence rows"), junctions: keeps, lock: guarded,
                    undo: restores("the previously open sequence"), },
                CreateSequence { label: "New sequence", reach: operator("the File menu's New Sequence row, the palette, and Ctrl+N"), junctions: keeps, lock: guarded,
                    undo: restores("the previously open sequence, and removes the sequence it created"), },
                SetSourcePatch { label: "Patch source", reach: operator("the lane header's source patch toggle"), junctions: keeps, lock: guarded,
                    undo: restores("the channel's previous patch"), },
            }
        }
    };
}

pub(super) use project_edit_rows;

macro_rules! project_command_rows {
    ($next:path) => {
        $next! {
            Import {
                label: "Import media",
                reach: operator("the File menu's Import row, and a file drop onto the pool"),
                undo: undo_if("the source is new to the pool; re-importing a file already there returns its id and records nothing"),
            },
            RemoveMedia {
                label: "Remove media",
                reach: operator("the pool context menu's Remove item"),
                undo: restores("the source with its original id, so clips removed alongside it reattach"),
            },
            Save {
                label: "Save project",
                reach: operator("the palette's Save row, the save chord, and the File menu"),
                undo: no_undo("writing a file changes no session state, so there is nothing to step back over"),
            },
            Open {
                label: "Open project",
                reach: operator("the File menu's Open row, and the Home screen's recent list"),
                undo: no_undo("replacing the session clears history, so there is no earlier state left to return to"),
            },
            Load {
                label: "Load project",
                reach: operator("the Home screen's recovery prompt, and an Open whose media has gone offline"),
                undo: no_undo("replacing the session clears history, so there is no earlier state left to return to"),
            },
            RelinkMedia {
                label: "Relink media",
                reach: operator("the palette's Reload All Media row, and the relink prompt on a missing file"),
                undo: no_undo("it repairs project state to match the disk, and undoing back to a dead path is never what was wanted"),
            },
            SetMediaLabel {
                label: "Set media label",
                reach: operator("the pool context menu's label colours"),
                undo: restores("the pool entry's previous label colour"),
            },
            Export {
                label: "Export",
                reach: internal("the desktop export sheet queues equivalent background work so the editor stays responsive; this blocking command is for non-interactive hosts"),
                undo: no_undo("rendering a file changes no session state, so there is nothing to step back over"),
            },
            SaveTemplate {
                label: "Save template",
                reach: operator("the File menu's Save Template row"),
                undo: no_undo("writing a template file leaves the session untouched, dirty flag and project path included"),
            },
            ApplyTemplate {
                label: "Apply template",
                reach: operator("the File menu's Use Template row, and the Home screen's template rail"),
                undo: no_undo("replacing the session clears history, so there is no earlier state left to return to"),
            },
        }
    };
}

pub(super) use project_command_rows;
