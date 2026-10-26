//! CPU effect passes on one device while other threads render and poll it.
//!
//! WHY: wgpu runs a buffer's map callback in the poll that collected it, after
//! that poll releases the device. A CPU pass that checked a flag set by its map
//! callback as soon as its own poll returned found the flag unset whenever a
//! poll on another thread had collected the callback first. It then passed the
//! frame through and put the still-mapped staging buffer back for the next
//! pass. The headless device is one per process, so any two compositors on two
//! threads, such as the preview worker and an export or two tests, raced this
//! way.
//!
//! This test renders a CPU effect on several threads beside a thread that keeps
//! an idle buffer waiting to map, whose callback sleeps for `SLOW_CALLBACK`. A
//! poll collects the buffers ready to map in the order they became ready, so a
//! renderer's poll that collects the idle buffer and another renderer's buffer
//! runs the other renderer's callback a sleep later, after that renderer's own
//! poll has returned. The test fails when any frame is not the effect's output
//! or any renderer does not end within the deadline.
//!
//! The CPU pass and the GPU scopes both map through `map_read`; this test
//! reaches it through the CPU pass. A reader that maps a buffer another way is
//! not covered.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::time::Duration;

use aria_compositor::{
    CompositeLayer, Compositor, CompositorConfig, CpuFrameIn, CpuFrameOut, EffectDefinition,
    EffectParams, GpuContext, LayerPlacement, PassInstance, pass_id, register_effect, wgpu,
};
use aria_models::EffectSpec;

const EFFECT: &str = "test.readback.invert";
const RENDERERS: usize = 4;
const FRAMES: usize = 150;
const DEADLINE: Duration = Duration::from_secs(120);
const SLOW_CALLBACK: Duration = Duration::from_millis(2);
/// How long an idle buffer waits for a renderer's poll before the polling
/// thread polls itself.
const IDLE_WAIT: Duration = Duration::from_millis(20);
const PLATE: [u8; 4] = [200, 100, 50, 255];
const INVERTED: [u8; 4] = [55, 155, 205, 255];

/// Renders `FRAMES` frames of the invert effect over `PLATE` on the shared
/// device and returns how many of them are not `INVERTED`.
fn render_frames() -> usize {
    let gpu = GpuContext::new_headless_blocking().expect("the device the test opened");
    let mut comp = Compositor::new(&gpu);
    let config = CompositorConfig::new(32, 8).with_background([0, 0, 0, 255]);
    let effects = [PassInstance::new(
        pass_id(EFFECT).expect("the registered effect"),
        Vec::new(),
    )];
    let layer =
        CompositeLayer::solid(PLATE, LayerPlacement::full_canvas(&config)).with_effects(&effects);
    (0..FRAMES)
        .filter(|_| {
            let frame = comp
                .render(&gpu, &config, std::slice::from_ref(&layer))
                .expect("renders the effect");
            frame.pixel(16, 4) != INVERTED
        })
        .count()
}

/// Keeps an idle buffer waiting to map on `device` until `running` clears. Its
/// callback sleeps for `SLOW_CALLBACK` in whichever poll collected it; a
/// buffer no other thread collects within `IDLE_WAIT` is collected here.
fn keep_slow_callbacks_waiting(device: &wgpu::Device, running: &AtomicBool) {
    while running.load(Ordering::Acquire) {
        let idle = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test.readback.idle"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let (fired, ran) = channel();
        idle.slice(..).map_async(wgpu::MapMode::Read, move |_| {
            std::thread::sleep(SLOW_CALLBACK);
            let _ = fired.send(());
        });
        if ran.recv_timeout(IDLE_WAIT).is_err() {
            let _ = device.poll(wgpu::PollType::Poll);
            let _ = ran.recv();
        }
    }
}

#[test]
fn every_cpu_pass_reads_its_input_while_other_threads_poll_the_device() {
    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping cpu effect readback test: no GPU adapter");
        return;
    };
    let _invert = register_effect(EffectDefinition::cpu(
        EffectSpec {
            id: EFFECT.into(),
            label: "Readback Invert".into(),
            ..Default::default()
        },
        |input: CpuFrameIn<'_>, output: &mut CpuFrameOut<'_>, _: &EffectParams<'_>, _: f32| {
            for y in 0..input.height() {
                for x in 0..input.width() {
                    let [r, g, b, a] = input.pixel_f32(x, y);
                    output.set_pixel_f32(x, y, [1.0 - r, 1.0 - g, 1.0 - b, a]);
                }
            }
            Ok(())
        },
    ))
    .expect("registers the invert effect");

    let running = Arc::new(AtomicBool::new(true));
    let idler = {
        let running = Arc::clone(&running);
        std::thread::spawn(move || keep_slow_callbacks_waiting(&gpu.device, &running))
    };
    let (done, finished) = channel();
    for renderer in 0..RENDERERS {
        let done = done.clone();
        std::thread::spawn(move || {
            let _ = done.send((renderer, render_frames()));
        });
    }
    drop(done);
    let mut passed_through: Vec<(usize, usize)> = (0..RENDERERS)
        .map(|_| {
            finished
                .recv_timeout(DEADLINE)
                .expect("every renderer ends within the deadline without a panic")
        })
        .collect();
    running.store(false, Ordering::Release);
    idler.join().expect("the idle buffer thread ends");
    passed_through.sort_unstable();
    assert_eq!(
        passed_through,
        (0..RENDERERS)
            .map(|renderer| (renderer, 0))
            .collect::<Vec<_>>(),
        "frames of {FRAMES} per renderer that are not the effect's output"
    );
}
