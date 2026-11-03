//! Project reads and edits: snapshots, the editor view, transactions, undo,
//! and clip, selection, marker, and in/out queries.

use aria_plugin_abi::{
    ARIA_BYTES_FOUND, ARIA_FRAME_SIZE_CANVAS, ARIA_FRAME_SIZE_EXACT, ARIA_FRAME_SIZE_FIT,
    ARIA_FRAME_SOURCE_CLIP, ARIA_FRAME_SOURCE_OPEN, ARIA_FRAME_SOURCE_SEQUENCE, AriaFrameRequest,
    AriaStr,
};

use super::Host;
use crate::{
    ClipRef, FrameSize, FrameSource, InOut, Marker, RationalTime, RenderedFrame, TimeRange,
};

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

    /// Render a frame of `source` at `time` and `size` as straight-alpha RGBA8 pixels.
    ///
    /// Requires permission `project = "read"` or `"read-write"`.
    #[allow(unsafe_code)]
    pub fn render_frame(
        &self,
        source: FrameSource,
        time: RationalTime,
        size: FrameSize,
    ) -> Result<RenderedFrame, String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support render_frame".to_owned());
        }
        let (source_kind, source_id) = match source {
            FrameSource::Open => (ARIA_FRAME_SOURCE_OPEN, 0),
            FrameSource::Sequence(seq) => (ARIA_FRAME_SOURCE_SEQUENCE, seq.unwrap_or(0)),
            FrameSource::Clip(clip) => (ARIA_FRAME_SOURCE_CLIP, clip),
        };
        let (size_kind, width, height) = match size {
            FrameSize::Canvas => (ARIA_FRAME_SIZE_CANVAS, 0, 0),
            FrameSize::Fit {
                max_width,
                max_height,
            } => (ARIA_FRAME_SIZE_FIT, max_width, max_height),
            FrameSize::Exact { width, height } => (ARIA_FRAME_SIZE_EXACT, width, height),
        };
        let time_rate_num = i32::try_from(time.rate.num)
            .map_err(|_| format!("invalid frame rate numerator {}", time.rate.num))?;
        let time_rate_den = i32::try_from(time.rate.den)
            .map_err(|_| format!("invalid frame rate denominator {}", time.rate.den))?;
        let mut req = AriaFrameRequest {
            source_kind,
            size_kind,
            source_id,
            time_value: time.value,
            time_rate_num,
            time_rate_den,
            width,
            height,
        };
        // SAFETY: `self.table.has_editor_reach()` confirmed `render_frame` is present in `self.table`.
        let res = unsafe { (self.table.render_frame)(self.ctx, &raw mut req) };
        if res.status == ARIA_BYTES_FOUND {
            // SAFETY: `res.bytes` was allocated by `self.table`.
            let data = unsafe { res.bytes.into_vec_with(self.table.free_bytes) };
            Ok(RenderedFrame {
                width: req.width,
                height: req.height,
                data,
            })
        } else {
            // SAFETY: `res.error` was allocated by `self.table`.
            let err = unsafe { res.error.into_string_with(self.table.free_string) };
            Err(err)
        }
    }

    /// Move the playhead to `tick_json` (`RationalTime` JSON) and return the new
    /// playhead position as JSON.
    ///
    /// Requires permission `project = "read-write"`.
    #[allow(unsafe_code)]
    pub fn editor_seek_json(&self, tick_json: &str) -> Result<String, String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support editor_seek".to_owned());
        }
        // SAFETY: `self.table.has_editor_reach()` confirmed `editor_seek` is present in `self.table`.
        let res = unsafe { (self.table.editor_seek)(self.ctx, AriaStr::from_str(tick_json)) };
        self.decode_string(res)
    }

    /// Move the playhead to `time` and return the resulting playhead position.
    ///
    /// Requires permission `project = "read-write"`.
    pub fn editor_seek(&self, time: RationalTime) -> Result<RationalTime, String> {
        let tick_json = serde_json::to_string(&time).map_err(|err| err.to_string())?;
        let json = self.editor_seek_json(&tick_json)?;
        serde_json::from_str(&json).map_err(|err| err.to_string())
    }

    /// Start playback at signed integer multiplier `speed` (`-8..=-1` or `1..=8`).
    ///
    /// Requires permission `project = "read-write"`.
    #[allow(unsafe_code)]
    pub fn editor_play(&self, speed: i32) -> Result<(), String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support editor_play".to_owned());
        }
        // SAFETY: `self.table.has_editor_reach()` confirmed `editor_play` is present in `self.table`.
        let res = unsafe { (self.table.editor_play)(self.ctx, speed) };
        self.decode_unit(res)
    }

    /// Stop playback.
    ///
    /// Requires permission `project = "read-write"`.
    #[allow(unsafe_code)]
    pub fn editor_stop(&self) -> Result<(), String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support editor_stop".to_owned());
        }
        // SAFETY: `self.table.has_editor_reach()` confirmed `editor_stop` is present in `self.table`.
        let res = unsafe { (self.table.editor_stop)(self.ctx) };
        self.decode_unit(res)
    }

    /// Open the `.aria` project at `path` when the open project has no unsaved changes.
    ///
    /// Requires permission `project = "read-write"` and filesystem read permission for `path`.
    #[allow(unsafe_code)]
    pub fn project_open(&self, path: &str) -> Result<(), String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support project_open".to_owned());
        }
        // SAFETY: `self.table.has_editor_reach()` confirmed `project_open` is present in `self.table`.
        let res = unsafe { (self.table.project_open)(self.ctx, AriaStr::from_str(path)) };
        self.decode_unit(res)
    }

    /// Save the open project to its current path without adding an undo step, returning
    /// the saved path.
    ///
    /// Requires permission `project = "read-write"` and filesystem write permission for
    /// the project's path.
    #[allow(unsafe_code)]
    pub fn project_save(&self) -> Result<String, String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support project_save".to_owned());
        }
        // SAFETY: `self.table.has_editor_reach()` confirmed `project_save` is present in `self.table`.
        let res = unsafe { (self.table.project_save)(self.ctx) };
        self.decode_string(res)
    }

    /// Save the open project to `path` without adding an undo step, setting the project
    /// path and returning the saved path.
    ///
    /// Requires permission `project = "read-write"` and filesystem write permission for `path`.
    #[allow(unsafe_code)]
    pub fn project_save_as(&self, path: &str) -> Result<String, String> {
        if !self.table.has_editor_reach() {
            return Err("host table does not support project_save_as".to_owned());
        }
        // SAFETY: `self.table.has_editor_reach()` confirmed `project_save_as` is present in `self.table`.
        let res = unsafe { (self.table.project_save_as)(self.ctx, AriaStr::from_str(path)) };
        self.decode_string(res)
    }
}
