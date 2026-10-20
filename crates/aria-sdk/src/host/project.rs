//! Project reads and edits: snapshots, the editor view, transactions, undo,
//! and clip, selection, marker, and in/out queries.

use aria_plugin_abi::AriaStr;

use super::Host;
use crate::{ClipRef, InOut, Marker, RationalTime, TimeRange};

impl Host {
    /// Take an immutable snapshot of the open project as serde JSON (`aria_models::Project`).
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn project_snapshot_json(&self) -> Result<String, String> {
        // SAFETY: `self.table.project_snapshot` is a valid host function.
        let res = unsafe { (self.table.project_snapshot)(self.ctx) };
        self.decode_string(res)
    }

    /// Take an immutable snapshot of the open project as a [`serde_json::Value`].
    pub fn project_snapshot(&self) -> Result<serde_json::Value, String> {
        let json = self.project_snapshot_json()?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Read the editor view state (selection, playhead, in/out) as JSON.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn editor_view_json(&self) -> Result<String, String> {
        // SAFETY: `self.table.editor_view` is a valid host function.
        let res = unsafe { (self.table.editor_view)(self.ctx) };
        self.decode_string(res)
    }

    /// Read the editor view state as a [`serde_json::Value`].
    pub fn editor_view(&self) -> Result<serde_json::Value, String> {
        let json = self.editor_view_json()?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Apply a batch of commands (serde JSON of `Vec<aria_commands::Command>`) as one
    /// undo step named `label`, returning the receipt JSON.
    ///
    /// Requires permission `project = "read-write"`.
    #[allow(unsafe_code)]
    pub fn project_transact_json(
        &self,
        label: &str,
        commands_json: &str,
    ) -> Result<String, String> {
        // SAFETY: `self.table.project_transact` is a valid host function.
        let res = unsafe {
            (self.table.project_transact)(
                self.ctx,
                AriaStr::from_str(label),
                AriaStr::from_str(commands_json),
            )
        };
        self.decode_string(res)
    }

    /// Apply a slice of command JSON values as one undo step named `label`.
    pub fn project_transact(
        &self,
        label: &str,
        commands: &[serde_json::Value],
    ) -> Result<serde_json::Value, String> {
        let commands_json = serde_json::to_string(commands).map_err(|err| err.to_string())?;
        let receipt_json = self.project_transact_json(label, &commands_json)?;
        serde_json::from_str(&receipt_json).map_err(|err| err.to_string())
    }

    /// Undo the latest step on the project host.
    ///
    /// Requires permission `project = "read-write"`.
    #[allow(unsafe_code)]
    pub fn project_undo(&self) -> Result<bool, String> {
        // SAFETY: `self.table.project_undo` is a valid host function.
        let res = unsafe { (self.table.project_undo)(self.ctx) };
        let text = self.decode_string(res)?;
        match text.trim() {
            "true" => Ok(true),
            "false" => Ok(false),
            other => serde_json::from_str(other).map_err(|err| err.to_string()),
        }
    }

    /// Look up clip `id` across the project's sequences as raw JSON, or `None` if absent.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn clip_json(&self, id: u64) -> Result<Option<String>, String> {
        if !self.table.has_project_queries() {
            return Err("host table does not support project queries".to_owned());
        }
        // SAFETY: `self.table.has_project_queries()` confirmed `clip` is present in `self.table`.
        let res = unsafe { (self.table.clip)(self.ctx, id) };
        let is_null = res.value.is_null();
        let text = self.decode_string(res)?;
        if is_null || text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(text))
        }
    }

    /// Look up clip `id` across the project's sequences.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    pub fn clip(&self, id: u64) -> Result<Option<ClipRef>, String> {
        match self.clip_json(id)? {
            Some(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|err| err.to_string()),
            None => Ok(None),
        }
    }

    /// Query clips on `sequence` (`None` for the open sequence) overlapping `range_json`
    /// (`TimeRange` JSON), returning `Vec<ClipRef>` JSON.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn clips_in_range_json(
        &self,
        sequence: Option<u64>,
        range_json: &str,
    ) -> Result<String, String> {
        if !self.table.has_project_queries() {
            return Err("host table does not support project queries".to_owned());
        }
        // SAFETY: `self.table.has_project_queries()` confirmed `clips_in_range` is present in `self.table`.
        let res = unsafe {
            (self.table.clips_in_range)(
                self.ctx,
                sequence.unwrap_or(0),
                AriaStr::from_str(range_json),
            )
        };
        self.decode_string(res)
    }

    /// Query clips on `sequence` (`None` for the open sequence) overlapping the
    /// half-open `range`, in track order then timeline start order.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    pub fn clips_in_range(
        &self,
        sequence: Option<u64>,
        range: TimeRange,
    ) -> Result<Vec<ClipRef>, String> {
        let range_json = serde_json::to_string(&range).map_err(|err| err.to_string())?;
        let json = self.clips_in_range_json(sequence, &range_json)?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Query clips on `sequence` (`None` for the open sequence) covering `tick_json`
    /// (`RationalTime` JSON), returning `Vec<ClipRef>` JSON.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn clips_at_json(&self, sequence: Option<u64>, tick_json: &str) -> Result<String, String> {
        if !self.table.has_project_queries() {
            return Err("host table does not support project queries".to_owned());
        }
        // SAFETY: `self.table.has_project_queries()` confirmed `clips_at` is present in `self.table`.
        let res = unsafe {
            (self.table.clips_at)(
                self.ctx,
                sequence.unwrap_or(0),
                AriaStr::from_str(tick_json),
            )
        };
        self.decode_string(res)
    }

    /// Query clips on `sequence` (`None` for the open sequence) covering `tick`,
    /// in track order then timeline start order.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    pub fn clips_at(
        &self,
        sequence: Option<u64>,
        tick: RationalTime,
    ) -> Result<Vec<ClipRef>, String> {
        let tick_json = serde_json::to_string(&tick).map_err(|err| err.to_string())?;
        let json = self.clips_at_json(sequence, &tick_json)?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Query the resolved [`ClipRef`]s for the current `EditorView` selection as JSON.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn selected_clips_json(&self) -> Result<String, String> {
        if !self.table.has_project_queries() {
            return Err("host table does not support project queries".to_owned());
        }
        // SAFETY: `self.table.has_project_queries()` confirmed `selected_clips` is present in `self.table`.
        let res = unsafe { (self.table.selected_clips)(self.ctx) };
        self.decode_string(res)
    }

    /// Query the resolved [`ClipRef`]s for the current `EditorView` selection,
    /// dropping any clip ids no longer in the project.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    pub fn selected_clips(&self) -> Result<Vec<ClipRef>, String> {
        let json = self.selected_clips_json()?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Query ruler markers on `sequence` (`None` for the open sequence) overlapping
    /// `range_json` (`TimeRange` JSON), returning `Vec<Marker>` JSON.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn markers_in_range_json(
        &self,
        sequence: Option<u64>,
        range_json: &str,
    ) -> Result<String, String> {
        if !self.table.has_project_queries() {
            return Err("host table does not support project queries".to_owned());
        }
        // SAFETY: `self.table.has_project_queries()` confirmed `markers_in_range` is present in `self.table`.
        let res = unsafe {
            (self.table.markers_in_range)(
                self.ctx,
                sequence.unwrap_or(0),
                AriaStr::from_str(range_json),
            )
        };
        self.decode_string(res)
    }

    /// Query ruler markers on `sequence` (`None` for the open sequence) overlapping
    /// the half-open `range`, in timeline order.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    pub fn markers_in_range(
        &self,
        sequence: Option<u64>,
        range: TimeRange,
    ) -> Result<Vec<Marker>, String> {
        let range_json = serde_json::to_string(&range).map_err(|err| err.to_string())?;
        let json = self.markers_in_range_json(sequence, &range_json)?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Read sequence In and Out points from `EditorView` as JSON.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn in_out_json(&self) -> Result<String, String> {
        if !self.table.has_project_queries() {
            return Err("host table does not support project queries".to_owned());
        }
        // SAFETY: `self.table.has_project_queries()` confirmed `in_out` is present in `self.table`.
        let res = unsafe { (self.table.in_out)(self.ctx) };
        self.decode_string(res)
    }

    /// Read sequence In and Out points from `EditorView`.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    pub fn in_out(&self) -> Result<InOut, String> {
        let json = self.in_out_json()?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }
}
