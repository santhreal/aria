# Audio

## On the timeline

- Audio clips show a rectified waveform, logarithmic by default; the timeline
  display settings switch to linear or hide waveforms.
- The volume line on a clip sets clip volume in dB. Ctrl+click adds a keyframe;
  dragging a segment between two keyframes moves both.
- Ctrl+drag on a clip edge adjusts the fade length when the clip has fade
  handles; a fade draws as a shaded ramp at the clip's head or tail.
- `]` / `[` on a selected audio clip raises or lowers its gain by 1 dB
  (Shift: 6 dB); G opens Audio Gain (set, adjust, normalize peak, normalize
  all peaks).
- Linked audio of a video clip moves with it while linked selection is on.

## Meters

The Audio Meters panel (default: right of the timeline) shows one bar per output
channel: peak with a 1.5 s peak hold, and RMS behind it. The scale runs from
-60 to 0 dBFS. A clip above 0 dBFS lights the clip indicator until clicked.
The meters run during playback, scrubbing, and from the Source tab.

## Audio Track Mixer

One strip per audio track and the master: pan, fader in dB, mute, solo, record
arm reserved for a later version, effect inserts (five per strip), sends, and
the output selector. Fader and pan changes during playback write keyframes when
the strip's automation mode is Write or Touch, and are read back in Read mode.

## Audio effects

Audio effects stack on clips, tracks, and the master. Each is a plugin
contribution ([plugins.md](plugins.md#audio-processors)) with automatable
parameters. Aria ships: Gain, Parametric EQ (8 bands), Compressor, Limiter,
De-esser, Noise Reduction (the current RNNoise denoiser), Reverb, Delay,
Pitch Shifter, Channel tools (swap, fill left, fill right, mono), and the
Loudness Normalizer (target LUFS).

Effects run on the audio thread in blocks of 128 to 1024 frames and report
their latency; the mixer compensates so tracks stay aligned.

## Audio analysis

Beat detection, silence detection, loudness measurement, and ducking stay as
jobs that write markers, keyframes, or clip metadata. Each is exposed as a plugin
contribution so a third-party analyzer appears in the same menus.
