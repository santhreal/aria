//! Loudness: measuring a clip's contour, and reading the result.
//!
//! A pure move: the same `impl AppState` methods, under the same names, so
//! every call site keeps its path.

use aria_core::count::counted;

use super::*;

impl AppState {
    /// Measure the selected clip's loudness contour as a job.
    pub fn measure_loudness(&mut self, cx: &mut Context<Self>) -> Result<(), String> {
        let request = self.analysis_precheck(AnalysisKind::Silence)?;
        let (wake, wakes) = crate::handoff::wakes::<Result<Vec<UiLoudness>, String>>();
        let label = format!("Measuring loudness for {}", request.media_path.display());
        let offset = self.selected_ui_clip().map_or(0, |clip| {
            clip.timeline_start_ticks - clip.source_start_ticks
        });
        self.spawn_job(label, move |context| {
            let outcome = measure_contour(context, &request, offset);
            let reported = match (&outcome, context.cancelled()) {
                (_, true) => Err(String::new()),
                (Ok(points), false) => Ok(points.clone()),
                (Err(err), false) => Err(err.clone()),
            };
            wake.wake(reported);
            let points = outcome?;
            JobCompletion::new(counted(points.len(), "window", "windows").to_string())
                .with_output("windows", points.len().to_string())
                .map_err(String::from)
        });

        cx.spawn(async move |this, cx| {
            let outcome = wakes.next_from_a_thread(cx.background_executor()).await;
            let _ = this.update(cx, |state, cx| {
                match outcome {
                    Some(Ok(points)) => {
                        state.status_message = Some(format!(
                            "Measured {}",
                            counted(points.len(), "loudness window", "loudness windows")
                        ));
                        state.analysis.loudness = points.into();
                    }
                    Some(Err(reason)) if reason.is_empty() => {
                        state.status_message = Some("Cancelled the measurement".into());
                    }
                    Some(Err(reason)) => state.report_error(reason),
                    None => state.report_error("The measurement did not report"),
                }
                cx.notify();
            });
        })
        .detach();
        Ok(())
    }

    /// The loudest window in the contour, so a clipping row has one owner.
    pub fn loudness_peak_dbfs(&self) -> Option<f32> {
        self.analysis
            .loudness
            .iter()
            .map(|point| point.dbfs)
            .fold(None, |peak: Option<f32>, dbfs| {
                Some(peak.map_or(dbfs, |current| current.max(dbfs)))
            })
    }

    /// Whether the contour reaches the digital ceiling.
    pub fn loudness_clips(&self) -> bool {
        self.loudness_peak_dbfs().is_some_and(|peak| peak >= -0.1)
    }
}

/// The line that states a contour's peak: `Peak -3.2 dBFS`, with `, clipping`
/// when it reaches the ceiling. One spelling for every surface that shows it.
pub fn peak_line(peak: f32, clipping: bool) -> String {
    if clipping {
        format!("Peak {peak:.1} dBFS, clipping")
    } else {
        format!("Peak {peak:.1} dBFS")
    }
}
