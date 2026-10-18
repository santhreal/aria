# Playback

Playback runs in real time without a pre-render, as in Premiere Pro. Frames that
cannot be produced in time are dropped and counted, never stretched; the audio
clock does not wait for the picture.

## Behaviour

| Input | Result |
| --- | --- |
| Space | Play from the playhead, or stop. Stopping leaves the playhead where it is. |
| Shift+Space | Play from the start of the work area to its end, looping when loop is on. |
| L | Play forward. Each further press: 2x, 4x, 8x. |
| J | Play backward. Each further press: 2x, 4x, 8x in reverse. |
| K | Stop. K held with J or L steps one frame at a time; held K while dragging J or L plays at 1/4 speed. |
| Shift+L / Shift+J | Increase / decrease the shuttle speed by 10 % (slow motion shuttle). |
| Left / Right | Step one frame. Held, steps at the sequence rate. |
| Shift+Left / Shift+Right | Step five frames. |
| Home / End | Sequence start / end. |
| Ctrl+Shift+L | Loop on or off. |
| Dragging the playhead | Scrubs. Audio scrub plays 40 ms grains at the pointer's position when audio scrubbing is on. |

Audio plays at every forward and reverse shuttle speed up to 4x with its pitch
held, and is muted above 4x. Starting playback reaches the first moving frame
within 100 ms on a warm cache (see [performance.md](performance.md)).

## Clocks

The audio device clock is the master. Video frames are presented on the
display's vsync, choosing the frame whose time is closest to the audio clock at
that vsync. A frame that is not ready at its vsync is a dropped frame: the
previous frame stays on screen, the drop is counted, and the next ready frame is
shown at its own time. With no audio device, or with all audio muted, the
master is the monotonic system clock.

A/V offset stays inside half a frame during playback. The audio output latency
reported by the device is subtracted from the audio clock.

## Resolution

| Setting | Meaning |
| --- | --- |
| Playback resolution | Full, 1/2, 1/4, 1/8, or Auto. Frames rendered during playback use it. Auto starts at Full and steps down one level after three drops in a second, and back up after five seconds without a drop. |
| Paused resolution | Full or 1/2. A paused or scrubbed frame renders at it. |
| High quality playback | Off by default. When on, scaling uses the highest quality filter during playback. |

Scaled frames are rendered at the reduced size from the start: decode outputs at
the reduced size where the codec supports it, and effects run on the reduced
frame with their spatial parameters scaled. The Viewer upscales the result.

## Pipeline

```
decode workers  ->  frame cache  ->  render (GPU)  ->  present (GPU, no copy)
      ^                                   ^
   decode-ahead                    render-ahead
   per active clip                 N frames
```

1. **Decode-ahead.** Each clip that will be visible in the next two seconds has
   a decoder on the shared decode pool, up to 16 open decoders per renderer.
   Stacked clips of one media file at the same source time share one decoder and
   one decoded CPU frame. Video decode pipelines expose only raw video pads
   (`expose-all-streams=false`, `video/x-raw(ANY)`) and bound software video
   decoder thread pools across active pipelines (`max-threads` scaled from `16`
   down to `8` as concurrent pipelines open). A decoder runs ahead of the playhead by a window sized
   by frame size and codec (at least 8 frames). Reverse playback decodes each
   GOP forward once and serves its frames backward from the cache. A paused
   frame redrawn at the same source time, for a parameter drag or a selection
   change, shows the CPU frame its last draw decoded; a GPU surface and a new
   source time decode again. A scrub snaps to the sync frame before the target
   when one seek and one decode cost less than decoding forward from the
   decoder's position. After a seek the video decoder starts at the latest video
   keyframe at or before the target, including in an MP4 whose audio packets
   would place the segment start before that keyframe.
2. **Frame cache.** Decoded and rendered frames are stored in a GPU-resident
   cache bounded by a size in Settings (default: 25 % of video memory) and a
   system memory spill tier (default: 4 GB). Frames are keyed by sequence,
   time, resolution, and the hash of everything that affects them, so an edit
   invalidates only the frames it changes. Cached ranges draw green in the
   render bar.
3. **Render-ahead.** The compositor renders frames ahead of the playhead on its
   own thread into the cache. During playback it renders at most 4 frames
   ahead; when paused it renders outward from the playhead into the work area
   while the machine is idle.
4. **Present.** The Viewer draws the cached texture directly. There is no GPU to
   CPU readback and no CPU copy between render and display. The device that
   renders is the device that presents.
5. **Edits.** An edit during playback invalidates the affected cache entries and
   playback continues; the next frame shown reflects the edit. The render bar's
   colours are computed on a thread of their own, so the next Viewer frame
   never waits for them; a newer edit stops an older computation.

No thread is created per frame. The decode pool, render thread, audio thread,
and present path are created once per session.

## Rendering ranges

Sequence > Render In to Out (Enter) renders the work area into the disk cache at
full resolution so that heavy effects play without drops. Sequence > Render
Selection renders the frames the selected clips cover; a frame between two
selected clips that neither covers is not rendered. The render bar turns green
over rendered ranges. Delete Render Files clears them. Renders survive reopening
the project as long as nothing that affects those frames changed.

## Proxies

Right-click media > Proxy > Create Proxies transcodes to an intra-frame proxy at
a chosen fraction of the source size, as a background job. The Proxy button in
the Viewer and View > Proxies switch playback between proxies and originals;
export always uses originals. A proxy that is missing falls back to the original.

## Dropped frames

The dropped-frame indicator shows green while no frame has dropped since it was
last reset and yellow with the count otherwise. The status bar shows the
achieved rate during playback. Settings > Playback > Log dropped frames writes
each drop with its cause (decode late, render late, present late) to the log.
