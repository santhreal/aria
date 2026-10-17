# Audio effects

A plugin adds an audio effect by supplying a catalog spec and a processor
factory. The mixer runs it like a shipped effect: in a clip's chain, a lane's,
or the master's, in playback, in export, and in `AudioHost` mixes.

```rust
use aria_plugin::{
    AudioBlock, AudioEffectSpec, AudioProcessor, EffectParamKind, EffectParamSpec,
};

struct Drive {
    memory: [f32; 2],
}

impl AudioProcessor for Drive {
    fn process(&mut self, block: &mut AudioBlock<'_>) {
        let amount = block.param(0).expect("slot 0");
        let (left, right) = block.channels();
        for (frame, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let k = amount.at(frame);
            *l = (*l * k).tanh();
            *r = (*r * k).tanh();
        }
    }

    fn reset(&mut self) {
        self.memory = [0.0; 2];
    }
}

fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
    ctx.register_audio_effect(
        AudioEffectSpec {
            id: "example.drive".into(),
            label: "Drive".into(),
            category: "Distortion".into(),
            params: vec![EffectParamSpec {
                name: "amount".into(),
                label: "Amount".into(),
                kind: EffectParamKind::Scalar,
                default: 1.0,
                min: 1.0,
                max: 20.0,
                default_color: [0; 4],
                default_vec2: [0.0; 2],
            }],
            latency: 0,
            sidechain: false,
        },
        |_rate: u32| Box::new(Drive { memory: [0.0; 2] }) as Box<dyn AudioProcessor>,
    )?;
    Ok(())
}
```

The returned `Registration` is held on the terms of `register_effect`.

`category` is the Effect menu submenu the effect is listed under. An empty
category lists it under Other Audio. A native plugin sets the same field on
`aria_sdk::AudioEffectSpec`. A WebAssembly guest calls
`set-effect-category(id, category)` after `register-audio-effect`; without that
call, the effect is listed under Other Audio.

The Audio block of Effect Controls lists the effect under Audio effects for a clip
with sound, and each mixer strip and the master strip list it under Inserts. A
lane and the master hold at most `STRIP_INSERTS` (5) inserts; a clip has no
limit. An insert whose effect is not registered keeps its parameters, draws as
not installed, and passes its input through.

## Processing

The mixer calls the factory once per insert when it opens over a project, with
the sample rate (`EXPORT_AUDIO_RATE`, 48000). It then calls `process` once per
block, in signal order along the chain. The block length is the length the
mix was asked for and varies between calls. `process` runs on the audio thread
during playback: allocate in the factory, and do not lock, allocate, or
perform I/O in `process`.

| `AudioBlock` call | Result |
| --- | --- |
| `frames()` | Frames in the block |
| `channels()` | Left and right, planar, written in place |
| `position()` | The sequence position of the first frame, in frames at the sample rate |
| `sample_rate()` | Frames per second |
| `param(slot)` | The parameter in spec order: `ParamSignal::Constant(value)`, or `ParamSignal::PerFrame(values)` when it is keyframed and changes within the block. `at(frame)` reads either |
| `sidechain()` | The key signal, planar, when the spec sets `sidechain` and the insert has a key lane |

Consecutive blocks continue each other. `reset` is called before a block that
does not continue the previous one, such as after a seek, and clears every
value carried between blocks.

A parameter keyed on a clip insert is in ticks from the clip's start, like any
clip curve. A parameter keyed on a lane or master insert is in sequence ticks.

`latency` is the number of frames at 48 kHz by which the output trails the
input, at most `MAX_AUDIO_LATENCY` (48000). The mixer delays every parallel
path by the same amount, so lanes stay aligned.

An effect with `sidechain: true` shows a Key from row on a mixer strip. The
master accepts a key from any lane with sound. A lane accepts any lane with
sound except itself and the lanes its own signal reaches through outputs,
sends, and keys. A clip insert takes no key. A muted or silent key lane reads
as zeros.

## Faults

A panic in the factory or in `process` bypasses the insert for the rest of the
mix and fails the plugin for the session, which drops its registrations.
During playback, a block has the real time it covers as its deadline; three
misses in a row bypass the insert for the rest of the mix and record a
`PluginFault` in `ctx.faults()`. A bypassed insert keeps delaying its input by
its declared latency.

The mixer records the duration of each `process` call in `ctx.costs()` and
`MountedPluginInfo::block_cost`, and the Plugins section in Settings displays
the per-block audio cost beside the plugin's mount and per-frame costs.

Registration returns `PluginError::AudioEffectRefused(message)` when:

- the id has no `.` namespace, is a shipped id, or is already registered;
- the label is empty, a parameter name repeats, or a range is inverted;
- a parameter is not `EffectParamKind::Scalar`, or its default lies outside
  its range;
- the latency exceeds `MAX_AUDIO_LATENCY`.
