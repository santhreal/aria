//! Track-level command rows: lane flags, view settings, mixer routing, and
//! placement onto tracks.

macro_rules! track_rows {
    ($next:path, $($accum:tt)*) => {
        $next! {
            $($accum)*
            addresses lane {
                AddClip {
                    label: "Add clip",
                    reach: internal(
                        "a pool item dropped on a lane resolves through OverwriteClip or \
                         RippleInsert, which also decide what happens to what is already there",
                    ),
                    junctions: keeps,
                    lock: guarded,
                    undo: restores("nothing of the lane; it removes the clip that was added"),
                },
                AddGenerated { label: "Add generated clip", reach: operator("the palette's adjustment-layer row, and the generator catalog in the sidebar"), junctions: keeps, lock: guarded,
                    undo: restores("nothing of the lane; it removes the generated clip"), },
                AddMulticamClip { label: "Place multicam clip", reach: operator("the pool sidebar's Place button on a multicam group row"), junctions: keeps, lock: guarded,
                    undo: restores("nothing of the lane; it removes the multicam clip"), },
                RemoveTrack { label: "Remove track", reach: operator("the lane context menu's Remove Track item"), junctions: breaks, lock: guarded,
                    undo: restores("the lane at its stack position, with every clip it held"), },
                MoveTrack { label: "Reorder track", reach: operator("the lane context menu's move items, and dragging a lane header"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous stack position"), },
                SetTrackName { label: "Rename track", reach: operator("the rename bar, opened from the lane header or its context menu"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous name"), },
                SetTrackEnabled { label: "Toggle track", reach: operator("the lane header's eye button"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous flags"), },
                SetTrackMuted { label: "Mute track", reach: operator("the lane header's mute button, and the lane context menu"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous flags"), },
                SetTrackSolo { label: "Solo track", reach: operator("the palette's Solo Track row, and the lane header's solo button"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous flags"), },
                SetTrackLocked { label: "Lock track", reach: operator("the lane header's lock button"), junctions: keeps, lock: ignored,
                    undo: restores("the lane's previous flags"), },
                SetTrackDuckSource { label: "Set duck source", reach: operator("the lane context menu's duck-source item"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous flags"), },
                SetTrackTargeted { label: "Target track", reach: operator("the lane header's target button"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous flags"), },
                SetTrackSyncLock { label: "Sync lock", reach: operator("the lane header's sync-lock button"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous flags"), },
                SetTrackDisplay { label: "Lane display", reach: operator("the lane context menu's height, waveform, filmstrip and collapse items"), junctions: keeps, lock: ignored,
                    undo: restores("the lane's previous view options"), },
                SetTrackGain { label: "Lane fader", reach: operator("the mixer's fader, on release"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous routing"), },
                SetTrackOutput { label: "Lane output", reach: operator("the mixer's output picker on each strip"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous routing"), },
                AddSend { label: "Add send", reach: operator("the mixer's send button on each strip"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous routing"), },
                SetSend { label: "Set send", reach: operator("the mixer's send fader and its pre-fader toggle"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous routing"), },
                RemoveSend { label: "Remove send", reach: operator("the mixer's send button, cycled past its last bus"), junctions: keeps, lock: guarded,
                    undo: restores("the lane's previous routing"), },
                ShiftClips { label: "Shift clips", reach: operator("the palette's gap and lane-shift rows, and the lane-shift chords"), junctions: breaks, lock: guarded,
                    undo: restores("the lane's placement by shifting it back the same distance"), },
                RippleInsert { label: "Ripple insert", reach: operator("the source monitor's insert button, a media drop that ripples, and a click on a pool card while the sequence ripples"), junctions: breaks, lock: guarded,
                    undo: restores("the lane by removing the clip, re-closing the hole it opened, and rejoining the clip it cut to open it"), },
                OverwriteClip { label: "Overwrite", reach: operator("the source monitor's overwrite button, a media drop onto a lane, and a click on a pool card while the sequence overwrites"), junctions: breaks, lock: guarded,
                    undo: restores("the lane by removing the clip and putting back everything it covered"), },
                FitToFill { label: "Fit to fill", reach: operator("the source monitor's insert and overwrite buttons, over a range marked on the sequence"), junctions: breaks, lock: guarded,
                    undo: restores("the lane by removing the retimed clip and undoing the span it opened or covered"), },
                PlaceNestedSequence { label: "Place nested sequence", reach: operator("the Sequence menu's Place Nest At Playhead row"), junctions: breaks, lock: guarded,
                    undo: restores("the lane by removing the placed nested sequence clip"), },
            }
        }
    };
}

pub(super) use track_rows;
