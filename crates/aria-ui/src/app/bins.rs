//! Pool folders, as the sidebar drives them.
//!
//! The pool shows one folder at a time: `current_bin` names it, and the list is
//! filtered to what is directly inside it. That is Premiere's model and it is the
//! one the rest of the panel already assumes — the count under the filter box
//! describes the rows on screen, so a flat list of every source with a folder
//! column beside it would make that count a lie.
//!
//! Nothing here decides where a folder sits: the project owns the tree, refuses
//! a cycle, and promotes a deleted folder's contents. This module turns a click
//! into one command and keeps the panel's own state (which folder is open, which
//! name is being typed) honest when the project changes underneath it.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{BinId, MediaId};

use crate::app::AppState;

/// The name a new folder gets before it is renamed.
pub const DEFAULT_BIN_NAME: &str = "New bin";

impl AppState {
    /// Which folder is open, and the one place it is written.
    ///
    /// The folder views are built from this and from the folder tree, so a
    /// command that moved the view to another folder and wrote the field
    /// itself left the breadcrumb and the rows describing the folder it came
    /// from. Every way of moving goes through here, and a test holds it: the
    /// field is assigned in one place in the crate.
    fn set_open_bin(&mut self, bin: Option<u64>) {
        self.current_bin = bin;
        self.refresh_bins();
    }

    /// Open a folder, or the pool root with `None`.
    ///
    /// Selecting a folder clears the item selection: the properties block
    /// describes a source, and the one it was describing may not be in view.
    pub fn open_bin(&mut self, bin: Option<u64>) {
        if self.current_bin == bin {
            return;
        }
        self.pool_selection.clear();
        self.bin_rename = None;
        self.set_open_bin(bin);
    }

    /// Create a folder inside the open one and open it.
    pub fn create_bin(&mut self) -> Result<(), EngineError> {
        let parent = self.current_bin.map(BinId::from_raw);
        let outcome = self.apply_command(Command::Edit(EditCommand::CreateBin {
            name: DEFAULT_BIN_NAME.into(),
            parent,
        }))?;
        // The new folder is opened and put straight into rename, because a
        // folder called "New bin" is not why anyone made one.
        if let aria_engine::ApplyOutcome::Edited(aria_commands::EditOutcome::CreatedBin(bin)) =
            outcome
        {
            self.set_open_bin(Some(bin.raw()));
            self.bin_rename = Some(BinRename {
                bin: bin.raw(),
                draft: DEFAULT_BIN_NAME.into(),
            });
        }
        Ok(())
    }

    /// Delete the open folder and step up to its parent.
    ///
    /// Its media and its child folders move up with it, so this never removes a
    /// source from the pool.
    pub fn remove_current_bin(&mut self) -> Result<(), EngineError> {
        let Some(bin) = self.current_bin else {
            return Ok(());
        };
        let parent = self
            .store
            .bins
            .iter()
            .find(|item| item.id == bin)
            .and_then(|item| item.parent);
        self.apply_command(Command::Edit(EditCommand::RemoveBin {
            bin: BinId::from_raw(bin),
        }))?;
        self.bin_rename = None;
        self.set_open_bin(parent);
        Ok(())
    }

    /// Put a source in a folder, or at the pool root with `None`.
    pub fn move_media_to_bin(&mut self, media: u64, bin: Option<u64>) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::MoveMediaToBin {
            media: MediaId::from_raw(media),
            bin: bin.map(BinId::from_raw),
        }))?;
        Ok(())
    }

    /// Place a pool item at `(x, y)` in the currently open folder (or pool root).
    pub fn set_pool_item_position(
        &mut self,
        media: u64,
        x: f32,
        y: f32,
    ) -> Result<(), EngineError> {
        let bin = self.current_bin.map(BinId::from_raw);
        self.apply_command(Command::Edit(EditCommand::SetPoolItemPosition {
            media: MediaId::from_raw(media),
            bin,
            x,
            y,
        }))?;
        Ok(())
    }

    /// Move a folder into another one, or to the pool root with `None`.
    pub fn move_bin(&mut self, bin: u64, parent: Option<u64>) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::MoveBin {
            bin: BinId::from_raw(bin),
            parent: parent.map(BinId::from_raw),
        }))?;
        Ok(())
    }

    /// Start renaming a folder. The draft starts as the current name, so a
    /// commit with no typing is not a rename to nothing.
    pub fn begin_bin_rename(&mut self, bin: u64) {
        let Some(item) = self.store.bins.iter().find(|item| item.id == bin) else {
            return;
        };
        self.bin_rename = Some(BinRename {
            bin,
            draft: item.name.to_string(),
        });
    }

    pub fn bin_rename_push(&mut self, ch: char) {
        if let Some(rename) = self.bin_rename.as_mut()
            && rename.draft.chars().count() < BIN_NAME_MAX_LEN
        {
            rename.draft.push(ch);
        }
    }

    pub fn bin_rename_pop(&mut self) {
        if let Some(rename) = self.bin_rename.as_mut() {
            rename.draft.pop();
        }
    }

    pub fn cancel_bin_rename(&mut self) {
        self.bin_rename = None;
    }

    /// Commit the rename. An empty name is refused rather than applied: a folder
    /// with no name cannot be told from another one.
    pub fn commit_bin_rename(&mut self) -> Result<(), EngineError> {
        let Some(rename) = self.bin_rename.take() else {
            return Ok(());
        };
        let name = rename.draft.trim().to_string();
        if name.is_empty() {
            self.status_message = Some("A bin needs a name".into());
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::RenameBin {
            bin: BinId::from_raw(rename.bin),
            name,
        }))?;
        Ok(())
    }

    /// The folders directly inside the open one, in draw order.
    ///
    /// A handle on the projected list: the strip draws it, the census counts it,
    /// and neither walks the pool to get it.
    pub fn child_bins(&self) -> std::sync::Arc<[crate::types::UiBin]> {
        std::sync::Arc::clone(&self.bin_children)
    }

    /// The open folder and everything above it, root first: the breadcrumb.
    ///
    /// `None` is the pool root and is always the first crumb, so there is always
    /// a way back out of a folder.
    pub fn bin_path(&self) -> std::sync::Arc<[crate::types::UiBin]> {
        std::sync::Arc::clone(&self.bin_trail)
    }

    /// Rebuild both folder views from the store.
    ///
    /// The one owner. Called where the two things they read change: a new
    /// projection (which is where the pool's folders arrive) and opening a
    /// folder. Drawing the strip used to do this per frame, twice, walking the
    /// whole pool for each and cloning every folder it kept.
    pub(crate) fn refresh_bins(&mut self) {
        self.bin_children = self
            .store
            .bins
            .iter()
            .filter(|item| item.parent == self.current_bin)
            .cloned()
            .collect();
        let mut path = Vec::new();
        let mut at = self.current_bin;
        // Bounded by the number of folders: a parent chain that points into a
        // cycle would otherwise spin here rather than draw a broken crumb.
        let mut left = self.store.bins.len();
        while let Some(id) = at {
            if left == 0 {
                break;
            }
            left -= 1;
            let Some(item) = self.store.bins.iter().find(|item| item.id == id) else {
                break;
            };
            path.push(item.clone());
            at = item.parent;
        }
        path.reverse();
        self.bin_trail = path.into();
    }

    /// Drop the open folder and any rename when the project no longer has them.
    ///
    /// A load, an undo of the folder's creation, or a redo of its deletion all
    /// leave the panel pointing at a folder that is gone; the pool then shows the
    /// root rather than an empty list with no way back.
    pub(crate) fn reconcile_bins(&mut self) {
        let gone = self
            .current_bin
            .is_some_and(|bin| !self.store.bins.iter().any(|item| item.id == bin));
        if let Some(rename) = self.bin_rename.as_ref()
            && !self.store.bins.iter().any(|item| item.id == rename.bin)
        {
            self.bin_rename = None;
        }
        // The folders and which one is open are what the two views are built
        // from, and this is the one moment either can have changed under a new
        // projection, so the refresh belongs here rather than at the call site.
        if gone {
            self.set_open_bin(None);
        } else {
            self.refresh_bins();
        }
    }
}

/// Longest folder name the field accepts. Long enough for a sentence, short
/// enough that a row stays readable.
pub const BIN_NAME_MAX_LEN: usize = 64;

/// A folder rename in flight: which folder, and what has been typed.
#[derive(Debug, Clone, PartialEq)]
pub struct BinRename {
    pub bin: u64,
    pub draft: String,
}
