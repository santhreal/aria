//! Background jobs, AI tools, video scopes, and host project queries across
//! the C ABI.

use parking_lot::Mutex;
use std::ffi::c_void;

use aria_plugin_abi::{
    ARIA_API_MAJOR, ARIA_STATUS_ERR, ARIA_STATUS_OK, ARIA_STATUS_PANIC, AriaHostV1, AriaJobStep,
    AriaScopeCall, AriaStr, free_rust_string,
};

use crate::{
    AiToolImage, AiToolOutput, AiToolSpec, AiToolTier, Host, JobStep, LogLevel, MarkerColor,
    Plugin, PluginRuntime, Rational, RationalTime, ScopeFrame, ScopeImage, ScopeSpec, TimeRange,
};

use super::{MOCK_HOST, MockHostState};

/// A plugin that counts to three in job steps. Detail, progress, `done`, and `commit`
/// cross the C ABI in `AriaJobStep` and the result value; a panic in a step returns
/// `ARIA_STATUS_PANIC`, and `job_end` receives the cancelled flag.
#[derive(Default)]
struct CountingJobs {
    taken: Mutex<u32>,
}

impl Plugin for CountingJobs {
    fn mount(&self, _host: &Host) -> Result<(), String> {
        Ok(())
    }

    fn command_run(&self, host: &Host, id: &str, _args: &str) -> Result<(), String> {
        let job = host.start_job(id)?;
        host.log(LogLevel::Info, &format!("started {job}"));
        Ok(())
    }

    fn job_step(&self, _host: &Host, job: u64) -> Result<JobStep, String> {
        if job == 99 {
            panic!("step exploded");
        }
        if job == 98 {
            return Err("step refused".to_owned());
        }
        let taken = {
            let mut taken = self.taken.lock();
            *taken += 1;
            *taken
        };
        Ok(JobStep {
            progress: (taken < 3).then_some(taken as f32 / 3.0),
            detail: if taken == 3 {
                "counted".to_owned()
            } else {
                String::new()
            },
            done: taken == 3,
            commit: taken == 2,
        })
    }

    fn job_end(&self, host: &Host, job: u64, cancelled: bool) {
        host.log(
            LogLevel::Info,
            &format!("ended {job} cancelled={cancelled}"),
        );
    }
}

#[allow(unsafe_code)]
fn step_once(
    runtime: &PluginRuntime<CountingJobs>,
    ctx: *mut c_void,
    job: u64,
) -> (u32, AriaJobStep, String) {
    let mut step = AriaJobStep {
        progress: 7.0,
        done: 9,
        commit: 9,
    };
    // SAFETY: `step` is a live, writable local for the call.
    let res = unsafe { runtime.handle_job_step(ctx, job, &raw mut step) };
    // SAFETY: `res.value` was allocated by the runtime with the Rust allocator.
    let text = unsafe { res.value.into_string_with(free_rust_string) };
    (res.status, step, text)
}

#[test]
fn jobs_start_through_the_host_and_step_across_the_abi() {
    let runtime = PluginRuntime::new(CountingJobs::default());
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    let started =
        runtime.handle_command_run(ctx, AriaStr::from_str("render"), AriaStr::from_str("{}"));
    assert_eq!(started.status, ARIA_STATUS_OK);
    assert_eq!(state.lock().jobs, vec!["render".to_owned()]);
    assert_eq!(
        state.lock().logs,
        vec![(aria_plugin_abi::ARIA_LOG_INFO, "started 41".to_owned())]
    );

    let (status, step, detail) = step_once(&runtime, ctx, 41);
    assert_eq!(status, ARIA_STATUS_OK);
    assert_eq!((step.progress, step.done, step.commit), (1.0 / 3.0, 0, 0));
    assert_eq!(detail, "");
    let (_, step, _) = step_once(&runtime, ctx, 41);
    assert_eq!((step.done, step.commit), (0, 1), "the second step commits");
    let (status, step, detail) = step_once(&runtime, ctx, 41);
    assert_eq!(status, ARIA_STATUS_OK);
    assert!(
        step.progress < 0.0,
        "unknown progress crosses as a negative value"
    );
    assert_eq!((step.done, step.commit, detail.as_str()), (1, 0, "counted"));

    let (status, step, detail) = step_once(&runtime, ctx, 98);
    assert_eq!((status, detail.as_str()), (ARIA_STATUS_ERR, "step refused"));
    assert_eq!(
        (step.progress, step.done, step.commit),
        (7.0, 9, 9),
        "a failed step writes nothing"
    );
    let (status, _, detail) = step_once(&runtime, ctx, 99);
    assert_eq!(
        (status, detail.as_str()),
        (ARIA_STATUS_PANIC, "step exploded")
    );

    #[allow(unsafe_code)]
    // SAFETY: a null step is the case `handle_job_step` rejects.
    let null = unsafe { runtime.handle_job_step(ctx, 41, std::ptr::null_mut()) };
    assert_eq!(null.status, ARIA_STATUS_ERR);

    assert_eq!(runtime.handle_job_end(ctx, 41, 0).status, ARIA_STATUS_OK);
    assert_eq!(runtime.handle_job_end(ctx, 42, 1).status, ARIA_STATUS_OK);
    let logs: Vec<String> = state
        .lock()
        .logs
        .iter()
        .map(|(_, text)| text.clone())
        .collect();
    assert_eq!(
        logs,
        [
            "started 41",
            "ended 41 cancelled=false",
            "ended 42 cancelled=true"
        ]
    );

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_OVERLAYS_SIZE;
    assert!(short_host.has_overlays());
    assert!(!short_host.has_jobs());
    let legacy = PluginRuntime::new(CountingJobs::default());
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused =
        legacy.handle_command_run(ctx, AriaStr::from_str("render"), AriaStr::from_str("{}"));
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("start_job"), "{refused}");
    assert_eq!(
        state.lock().jobs.len(),
        1,
        "a host without start_job is never called"
    );
}

/// Offers `shade`, which returns a label and an image, and fails or panics
/// on request.
struct Shades;

impl Plugin for Shades {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_ai_tool(&AiToolSpec {
            name: "shade".to_owned(),
            description: "Name a shade.".to_owned(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": { "tone": { "type": "string" } },
            }),
            tier: AiToolTier::Workspace,
        })
    }

    fn ai_tool_call(
        &self,
        _host: &Host,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<AiToolOutput, String> {
        match (name, arguments["tone"].as_str()) {
            ("shade", Some("boom")) => panic!("shade exploded"),
            ("shade", Some(tone)) => Ok(AiToolOutput {
                text: format!("{tone} shade"),
                images: vec![AiToolImage {
                    label: "swatch".to_owned(),
                    png: vec![137, 80, 78, 71],
                }],
            }),
            ("shade", None) => Err("tone is required".to_owned()),
            (other, _) => Err(format!("no AI tool {other}")),
        }
    }
}

fn tool_call(
    runtime: &PluginRuntime<Shades>,
    ctx: *mut c_void,
    name: &str,
    arguments: &str,
) -> (u32, String) {
    let res =
        runtime.handle_ai_tool_call(ctx, AriaStr::from_str(name), AriaStr::from_str(arguments));
    #[allow(unsafe_code)]
    // SAFETY: `res.value` was allocated by the runtime with the Rust allocator.
    let text = unsafe { res.value.into_string_with(free_rust_string) };
    (res.status, text)
}

#[test]
fn ai_tools_register_through_the_host_and_answer_across_the_abi() {
    let runtime = PluginRuntime::new(Shades);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    let specs: Vec<serde_json::Value> = state
        .lock()
        .ai_tools
        .iter()
        .map(|spec| serde_json::from_str(spec).expect("spec JSON"))
        .collect();
    assert_eq!(
        specs,
        [serde_json::json!({
            "name": "shade",
            "description": "Name a shade.",
            "parameters": {
                "type": "object",
                "properties": { "tone": { "type": "string" } },
            },
            "tier": "workspace",
        })]
    );

    let (status, json) = tool_call(&runtime, ctx, "shade", r#"{"tone":"teal"}"#);
    assert_eq!(status, ARIA_STATUS_OK, "{json}");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).expect("output JSON"),
        serde_json::json!({
            "text": "teal shade",
            "images": [{ "label": "swatch", "png": [137, 80, 78, 71] }],
        })
    );
    assert_eq!(
        tool_call(&runtime, ctx, "shade", "{}"),
        (ARIA_STATUS_ERR, "tone is required".to_owned())
    );
    assert_eq!(
        tool_call(&runtime, ctx, "tint", r#"{"tone":"teal"}"#),
        (ARIA_STATUS_ERR, "no AI tool tint".to_owned())
    );
    let (status, text) = tool_call(&runtime, ctx, "shade", "{tone");
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(
        text.starts_with("the arguments of shade are not JSON"),
        "{text}"
    );
    assert_eq!(
        tool_call(&runtime, ctx, "shade", r#"{"tone":"boom"}"#),
        (ARIA_STATUS_PANIC, "shade exploded".to_owned())
    );

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_DECODERS_SIZE;
    assert!(short_host.has_decoders());
    assert!(!short_host.has_ai_tools());
    let legacy = PluginRuntime::new(Shades);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("AI tools"), "{refused}");
    assert_eq!(
        state.lock().ai_tools.len(),
        1,
        "a host without register_ai_tool is never called"
    );
}

/// Offers the scope `meter`, which copies the frame's first pixel's red and
/// green into a 2x1 image and reads back the frame size, and the scope
/// `boom`, which panics.
struct Meter;

impl Plugin for Meter {
    fn mount(&self, host: &Host) -> Result<(), String> {
        host.register_scope(&ScopeSpec {
            id: "meter".to_owned(),
            name: "Meter".to_owned(),
            width: 2,
            height: 1,
        })
    }

    fn scope_measure(
        &self,
        _host: &Host,
        id: &str,
        frame: &ScopeFrame<'_>,
        image: &mut ScopeImage<'_>,
    ) -> Result<String, String> {
        match id {
            "meter" => {
                let [r, g, _, _] = frame.pixel(0, 0).ok_or("an empty frame")?;
                image.put(0, 0, [r, 0, 0, 255]);
                image.put(1, 0, [0, g, 0, 255]);
                Ok(format!("{}x{}", frame.width, frame.height))
            }
            "boom" => panic!("scope exploded"),
            other => Err(format!("no scope {other}")),
        }
    }
}

/// Measure a 1x1 frame of `pixel` with scope `id` into an image `image_len`
/// bytes long that states it is 2x1.
fn scope_call(
    runtime: &PluginRuntime<Meter>,
    ctx: *mut c_void,
    id: &str,
    pixel: [u8; 4],
    image_len: usize,
) -> (u32, String, Vec<u8>) {
    let mut image = vec![0_u8; image_len];
    let mut call = AriaScopeCall {
        frame: pixel.as_ptr(),
        frame_len: pixel.len(),
        frame_width: 1,
        frame_height: 1,
        image: image.as_mut_ptr(),
        image_len: image.len(),
        image_width: 2,
        image_height: 1,
    };
    #[allow(unsafe_code)]
    // SAFETY: `call` points at `pixel` and `image`, which outlive the call.
    let res = unsafe { runtime.handle_scope_measure(ctx, AriaStr::from_str(id), &raw mut call) };
    #[allow(unsafe_code)]
    // SAFETY: `res.value` was allocated by the runtime with the Rust allocator.
    let text = unsafe { res.value.into_string_with(free_rust_string) };
    (res.status, text, image)
}

#[test]
fn scopes_register_through_the_host_and_measure_across_the_abi() {
    let runtime = PluginRuntime::new(Meter);
    let state = Mutex::new(MockHostState::default());
    let ctx = (&raw const state) as *mut c_void;
    #[allow(unsafe_code)]
    let bound = unsafe { runtime.bind_host(&MOCK_HOST, ARIA_API_MAJOR) };
    assert!(bound);

    assert_eq!(runtime.handle_mount(ctx).status, ARIA_STATUS_OK);
    let specs: Vec<serde_json::Value> = state
        .lock()
        .scopes
        .iter()
        .map(|spec| serde_json::from_str(spec).expect("spec JSON"))
        .collect();
    assert_eq!(
        specs,
        [serde_json::json!({ "id": "meter", "name": "Meter", "width": 2, "height": 1 })]
    );

    assert_eq!(
        scope_call(&runtime, ctx, "meter", [10, 20, 30, 255], 8),
        (
            ARIA_STATUS_OK,
            "1x1".to_owned(),
            vec![10, 0, 0, 255, 0, 20, 0, 255]
        )
    );
    assert_eq!(
        scope_call(&runtime, ctx, "tint", [10, 20, 30, 255], 8),
        (ARIA_STATUS_ERR, "no scope tint".to_owned(), vec![0; 8])
    );
    let (status, text, image) = scope_call(&runtime, ctx, "meter", [10, 20, 30, 255], 4);
    assert_eq!(status, ARIA_STATUS_ERR);
    assert!(
        text.contains("a 2x1 image in 4 bytes"),
        "a short image buffer is refused before the plugin runs: {text}"
    );
    assert_eq!(image, [0; 4]);
    assert_eq!(
        scope_call(&runtime, ctx, "boom", [10, 20, 30, 255], 8),
        (ARIA_STATUS_PANIC, "scope exploded".to_owned(), vec![0; 8])
    );
    #[allow(unsafe_code)]
    // SAFETY: a null call is refused before it is read.
    let null = unsafe {
        runtime.handle_scope_measure(ctx, AriaStr::from_str("meter"), std::ptr::null_mut())
    };
    assert_eq!(null.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    // SAFETY: `null.value` was allocated by the runtime with the Rust allocator.
    drop(unsafe { null.value.into_string_with(free_rust_string) });

    let mut short_host = MOCK_HOST;
    short_host.struct_size = aria_plugin_abi::ARIA_HOST_V1_AI_TOOLS_SIZE;
    assert!(short_host.has_ai_tools());
    assert!(!short_host.has_scopes());
    let legacy = PluginRuntime::new(Meter);
    #[allow(unsafe_code)]
    let bound = unsafe { legacy.bind_host(&short_host, ARIA_API_MAJOR) };
    assert!(bound);
    let refused = legacy.handle_mount(ctx);
    assert_eq!(refused.status, ARIA_STATUS_ERR);
    #[allow(unsafe_code)]
    let refused = unsafe { refused.value.into_string_with(free_rust_string) };
    assert!(refused.contains("scopes"), "{refused}");
    assert_eq!(
        state.lock().scopes.len(),
        1,
        "a host without register_scope is never called"
    );
}

#[test]
fn host_project_queries_round_trip_and_refuse_on_shorter_table() {
    let mut state = Mutex::new(MockHostState::default());
    let ctx = (&raw mut state).cast::<c_void>();
    #[allow(unsafe_code)]
    let host = unsafe { Host::from_raw(&MOCK_HOST, ctx) };

    let t0 = RationalTime::new(0, Rational::FPS_24);
    let t24 = RationalTime::new(24, Rational::FPS_24);
    let range = TimeRange::new(t0, t24);

    let c1 = host.clip(1).expect("clip 1").expect("present");
    assert_eq!(c1.id, 1);
    assert_eq!(c1.track, 10);
    assert_eq!(c1.end, t24);
    assert_eq!(host.clip(99).expect("clip 99"), None);

    let in_range = host.clips_in_range(Some(7), range).expect("clips_in_range");
    assert_eq!(in_range.len(), 1);
    assert_eq!(in_range[0].id, 7);

    assert!(host.clips_at(None, t0).expect("clips_at").is_empty());
    assert_eq!(host.selected_clips().expect("selected_clips").len(), 1);

    let markers = host
        .markers_in_range(None, range)
        .expect("markers_in_range");
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].name, "Beat");
    assert_eq!(markers[0].color, MarkerColor::Teal);

    let io = host.in_out().expect("in_out");
    assert_eq!(io.mark_in, Some(RationalTime::new(4, Rational::FPS_24)));
    assert_eq!(io.mark_out, Some(RationalTime::new(20, Rational::FPS_24)));

    static SHORT_HOST: AriaHostV1 = {
        let mut h = MOCK_HOST;
        h.struct_size = aria_plugin_abi::ARIA_HOST_V1_CONTRIBUTIONS_SIZE;
        h
    };
    assert!(SHORT_HOST.has_contributions());
    assert!(!SHORT_HOST.has_project_queries());
    #[allow(unsafe_code)]
    let short = unsafe { Host::from_raw(&SHORT_HOST, ctx) };
    assert!(short.clip(1).is_err());
    assert!(short.clips_in_range(None, range).is_err());
    assert!(short.clips_at(None, t0).is_err());
    assert!(short.selected_clips().is_err());
    assert!(short.markers_in_range(None, range).is_err());
    assert!(short.in_out().is_err());
}
