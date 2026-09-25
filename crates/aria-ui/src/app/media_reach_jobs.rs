//! The two media-support verbs that decode a whole file.
//!
//! Split from the parent to stay under the file ceiling, and split along the
//! line that matters: everything here goes to the job board, because a whole-file
//! decode or a whole-stream read must not happen on the thread that draws. The
//! completion detail is the answer, so the jobs panel is the surface — no
//! channel back into the state, and nothing to hold between frames.
//!
//! The refusals stay synchronous. A file over the verify cap, a source with no
//! audio, or no selection at all is answered on the status strip before a job is
//! ever started, so the board never carries an entry that could not have run.

use super::*;

impl AppState {
    /// Decode the selected clip's still or animation both ways and compare.
    ///
    /// A path decode and a byte decode take different catalog columns, so a
    /// container that reads from disk and not from memory is possible and has
    /// been shipped before. The export path uses the byte column for embedded
    /// content; this is the only way to find out before the export does.
    pub fn verify_selected_decode(&mut self) {
        let Some(item) = self.selected_media_item() else {
            self.status_message = Some("Select a clip that plays a pool entry".into());
            return;
        };
        let path = PathBuf::from(&*item.path);
        let name = item.name.clone();
        let Some(format) = sniff_image(&path) else {
            self.status_message = Some(format!(
                "{name} is not a still or an animated image container"
            ));
            return;
        };
        let info = match probe_image(&path) {
            Ok(info) => info,
            Err(err) => {
                self.status_message = Some(format!("{name}: {err}"));
                return;
            }
        };
        let pixels = u64::from(info.width) * u64::from(info.height);
        if pixels > MAX_VERIFY_PIXELS {
            self.status_message = Some(format!(
                "{name} is {}×{} — over the {MAX_VERIFY_PIXELS} pixel verify cap",
                info.width, info.height
            ));
            return;
        }
        let animates = format.animates();
        let label = format!("Verifying {name} ({})", format.label());
        self.spawn_job(label, move |context| {
            if context.cancelled() {
                return Err("cancelled before the decode started".into());
            }
            let bytes = std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
            let from_bytes = sniff_bytes(&bytes)
                .ok_or_else(|| format!("{}: the header names no container", path.display()))?;
            if from_bytes != format {
                return Err(format!(
                    "{}: the path reads as {} and the bytes as {}",
                    path.display(),
                    format.label(),
                    from_bytes.label()
                ));
            }
            if animates {
                verify_animation(&path, &bytes)
            } else {
                verify_still(&path, &bytes)
            }
        });
    }

    /// Fold the selected source's whole audio stream into a fixed overview and
    /// report where its loudest moment is.
    ///
    /// The timeline's waveform is per-second, so its length tracks duration and
    /// it cannot answer "how far into this file is the loud part". A fixed
    /// bucket count can, at the same cost for any length.
    pub fn file_loudness_overview(&mut self) {
        let Some(item) = self.selected_media_item() else {
            self.status_message = Some("Select a clip that plays a pool entry".into());
            return;
        };
        if !item.has_audio {
            self.status_message = Some(format!("{} carries no audio", item.name));
            return;
        }
        let path = PathBuf::from(&*item.path);
        let name = item.name.clone();
        self.spawn_job(format!("Reading {name} loudness"), move |context| {
            if context.cancelled() {
                return Err("cancelled before the read started".into());
            }
            let peaks = aria_decoder::audio_peaks(&path, OVERVIEW_BUCKETS)
                .map_err(|err| err.to_string())?;
            let floor = linear_from_db(LOUD_THRESHOLD_DBFS);
            let loud = peaks.iter().filter(|peak| **peak >= floor).count();
            let (at, peak) = peaks.iter().copied().enumerate().fold(
                (0_usize, 0.0_f32),
                |best, (index, value)| {
                    if value > best.1 { (index, value) } else { best }
                },
            );
            let across = if peaks.len() > 1 {
                at as f64 / (peaks.len() - 1) as f64 * 100.0
            } else {
                0.0
            };
            let level = db_from_linear(peak)
                .map(|db| format!("{db:.1} dBFS"))
                .unwrap_or_else(|| "silence".to_string());
            JobCompletion::new(format!(
                "loudest {level} at {across:.0}% in; {loud}/{} bucket(s) over \
                     {LOUD_THRESHOLD_DBFS:.0} dBFS",
                peaks.len()
            ))
            .with_output("peak_at_percent", format!("{across:.1}"))?
            .with_output("loud_buckets", loud.to_string())
            .map_err(String::from)
        });
    }
}

/// Decode an animation from its path and from the same bytes, and refuse if the
/// two disagree on how many frames the file holds.
fn verify_animation(path: &std::path::Path, bytes: &[u8]) -> Result<JobCompletion, String> {
    let framed = decode_animation_file(path).map_err(|err| err.to_string())?;
    let embedded = decode_animation(bytes).map_err(|err| err.to_string())?;
    if framed.len() != embedded.len() {
        return Err(format!(
            "{}: {} frame(s) from the path and {} from memory",
            path.display(),
            framed.len(),
            embedded.len()
        ));
    }
    let millis: u32 = framed.iter().map(|frame| frame.delay_ms).sum();
    let first = &framed[0].image;
    JobCompletion::new(format!(
        "{} frame(s), {}×{}, {:.2}s per loop",
        framed.len(),
        first.width,
        first.height,
        f64::from(millis) / 1000.0
    ))
    .with_output("frames", framed.len().to_string())?
    .with_output("loop_ms", millis.to_string())
    .map_err(String::from)
}

/// Decode a still from its path and from the same bytes, and refuse if the two
/// disagree on its size.
fn verify_still(path: &std::path::Path, bytes: &[u8]) -> Result<JobCompletion, String> {
    let from_path = decode_image(path).map_err(|err| err.to_string())?;
    let in_memory = decode_image_bytes(bytes).map_err(|err| err.to_string())?;
    if (from_path.width, from_path.height) != (in_memory.width, in_memory.height) {
        return Err(format!(
            "{}: {}×{} from the path and {}×{} from memory",
            path.display(),
            from_path.width,
            from_path.height,
            in_memory.width,
            in_memory.height
        ));
    }
    JobCompletion::new(format!(
        "{}×{} from the path and from memory",
        from_path.width, from_path.height
    ))
    .with_output("width", from_path.width.to_string())?
    .with_output("height", from_path.height.to_string())
    .map_err(String::from)
}
