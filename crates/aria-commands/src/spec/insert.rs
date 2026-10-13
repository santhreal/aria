//! Insert chain command rows: audio effects on a clip, a lane, or the master.

macro_rules! insert_rows {
    ($next:path, $($accum:tt)*) => {
        $next! {
            $($accum)*
            addresses insert {
                AddAudioEffect { label: "Add audio effect", reach: operator("the inspector's Audio block insert picker, and the mixer's insert slot picker"), junctions: keeps, lock: guarded,
                    undo: restores("the chain as it was, without the new insert"), },
                RemoveAudioEffect { label: "Remove audio effect", reach: operator("the remove button on each insert in the inspector's Audio block and in the mixer's insert slots"), junctions: keeps, lock: guarded,
                    undo: restores("the chain with the insert back at its index"), },
                MoveAudioEffect { label: "Reorder audio effect", reach: operator("the up and down arrows on each insert in the inspector's Audio block and in the mixer's insert slots"), junctions: keeps, lock: guarded,
                    undo: restores("the chain's previous order"), },
                SetAudioEffectParam { label: "Set audio effect parameter", reach: operator("the parameter sliders under each insert in the inspector's Audio block and in the mixer's insert slots"), junctions: keeps, lock: guarded,
                    undo: restores("the chain with the parameter's previous curve"), },
                SetAudioEffectBypass { label: "Bypass audio effect", reach: operator("the bypass toggle on each insert in the inspector's Audio block and in the mixer's insert slots"), junctions: keeps, lock: guarded,
                    undo: restores("the chain with the insert's previous bypass"), },
                SetAudioEffectSidechain { label: "Set sidechain", reach: operator("the mixer's sidechain picker on an insert that reads a key"), junctions: keeps, lock: guarded,
                    undo: restores("the chain with the insert's previous key"), },
            }
        }
    };
}

pub(super) use insert_rows;
