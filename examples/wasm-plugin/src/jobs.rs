//! Stepped background jobs (`example.count-job` and `example.endless-job`).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::{JobStep, start_job, storage_set};

/// Steps a counting job takes before it is done.
const COUNT_STEPS: u32 = 4;

pub(crate) struct JobModel {
    taken: u32,
    endless: bool,
}

pub(crate) static JOBS: Mutex<Option<HashMap<u64, JobModel>>> = Mutex::new(None);

/// Start a job that counts to [`COUNT_STEPS`], or one that steps until
/// cancelled, and record its id in storage under `last-job`.
pub(crate) fn start_counting_job(endless: bool) -> Result<(), String> {
    let job = start_job(if endless { "endless" } else { "count" })?;
    JOBS.lock()
        .map_err(|_| "job state lock poisoned".to_owned())?
        .get_or_insert_with(HashMap::new)
        .insert(job, JobModel { taken: 0, endless });
    storage_set("last-job", job.to_string().as_bytes())
}

pub(crate) fn job_step(job: u64) -> Result<JobStep, String> {
    let mut guard = JOBS
        .lock()
        .map_err(|_| "job state lock poisoned".to_owned())?;
    let model = guard
        .get_or_insert_with(HashMap::new)
        .get_mut(&job)
        .ok_or_else(|| format!("unknown job {job}"))?;
    model.taken += 1;
    let taken = model.taken;
    if model.endless {
        return Ok(JobStep {
            progress: None,
            detail: format!("step {taken}"),
            done: false,
            commit: false,
        });
    }
    let done = taken >= COUNT_STEPS;
    Ok(JobStep {
        progress: Some(taken as f32 / COUNT_STEPS as f32),
        detail: if done {
            format!("counted to {COUNT_STEPS}")
        } else {
            format!("step {taken}")
        },
        done,
        commit: false,
    })
}

pub(crate) fn job_end(job: u64, cancelled: bool) {
    if let Ok(mut guard) = JOBS.lock()
        && let Some(jobs) = guard.as_mut()
    {
        jobs.remove(&job);
    }
    let _ = storage_set("job-end", format!("{job}:{cancelled}").as_bytes());
}
