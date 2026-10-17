# Jobs

A job is background work listed on the Jobs board with a label, a progress
bar, a detail line, and a Cancel button. A plugin starts one from a command, an
event handler, a panel event, or `mount`. Starting a job needs no permission.

The board labels a plugin job `<plugin name>: <label>`. A job started during
`mount` waits for the mount to finish and does not run when the mount fails.
Unmounting, reloading, or failing the plugin cancels every job it started that
has not reached its commit step. A panic inside a job fails the plugin for the
session and ends the job as failed. Starting a job is refused when the host has
no job registry; the `aria.jobs` plugin registers it in the editor.

## Commit step

Cancel is accepted until the job begins its commit step: the point after which
stopping would leave partial output, such as renaming a finished file over the
original. From the commit step on, Cancel is refused and the job runs to its
end.

## In-process plugins

`PluginContext::start_job(label, work)` runs `work` on a job thread and
returns its `JobId`. `work` receives a `JobContext`:

| Call | Effect |
| --- | --- |
| `set_progress(fraction, detail)` | Sets the bar to `fraction` (`0.0..=1.0`) and the detail line |
| `set_detail(detail)` | Sets the detail line and leaves the bar unchanged |
| `cancelled()` | True once Cancel was accepted; return from `work` |
| `try_begin_commit()` | Enters the commit step; false when Cancel won first |

`work` returns `Ok(detail)` for the last line the board shows or `Err(reason)`
for a failure.

```rust
ctx.start_job("Transcode proxies", |job| {
    for (done, clip) in clips.iter().enumerate() {
        if job.cancelled() {
            return Err("cancelled".into());
        }
        transcode(clip)?;
        job.set_progress((done + 1) as f32 / clips.len() as f32, clip.name.clone());
    }
    Ok(format!("{} proxies", clips.len()))
})?;
```

## Stepped jobs: WASM and native plugins

The host calls WASM and native plugins one bounded call at a time, so their
jobs run in steps. The plugin starts a job and receives its id; the host then
calls the plugin's job step on the job thread until a step reports `done`, a
step fails, or Cancel is accepted, and then calls the job end with whether the
job was cancelled. A faulted plugin, and a WASM plugin already unloaded,
receive no job end. A step that panics or traps ends without a job end. The host
checks Cancel between steps, so a step should do a bounded amount of work.

A WASM plugin's steps wait for the call that started the job to return. A
native plugin's first step can run before `start_job` returns: hold the lock
that `job_step` reads the job's state under across the `start_job` call, then
record the state under it.

A step returns:

| Field | Meaning |
| --- | --- |
| `progress` | Fraction done, `0.0..=1.0`, or none while unknown |
| `detail` | The line the board shows; the last line when `done` |
| `done` | The job is finished |
| `commit` | The next step cannot be undone: Cancel is refused from here on |

| Tier | Start | Step | End |
| --- | --- | --- | --- |
| WASM | import `start-job(label) -> result<u64, string>` | export `job-step(job) -> result<job-step, string>` | export `job-end(job, cancelled)` |
| Native (`aria-sdk`) | `Host::start_job(label) -> Result<u64, String>` | `Plugin::job_step(&self, host, job) -> Result<JobStep, String>` | `Plugin::job_end(&self, host, job, cancelled)` |
| C ABI | `AriaHostV1.start_job(host_ctx, label) -> AriaInstanceResult` | `AriaPluginV1.job_step(host_ctx, job, AriaJobStep *step) -> AriaResult` | `AriaPluginV1.job_end(host_ctx, job, uint32_t cancelled)` |

In the C ABI, `job_step` writes `progress`, `done`, and `commit` into the
`AriaJobStep` it receives and returns the detail line as the `AriaResult`
value. A negative or NaN `progress` reports unknown progress. A native plugin
table shorter than `sizeof(AriaPluginV1)` has no job callbacks, and
`start_job` from it is refused.

```rust
impl aria_sdk::Plugin for Counter {
    fn command_run(&self, host: &Host, _id: &str, _args: &str) -> Result<(), String> {
        host.start_job("Count").map(drop)
    }

    fn job_step(&self, _host: &Host, _job: u64) -> Result<JobStep, String> {
        let n = self.count.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(JobStep {
            progress: Some(n as f32 / 4.0),
            detail: format!("step {n}"),
            done: n == 4,
            commit: false,
        })
    }
}
```

The WASM example plugin in `examples/wasm-plugin` registers
`example.count-job`, a job of four steps, and `example.endless-job`, a job with
unknown progress that runs until cancelled.
