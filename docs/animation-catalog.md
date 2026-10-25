# Animation catalog

Every appearance, disappearance, and traveling value in the UI uses a named
`MotionSpec`: a reference preset from `gpui::motion::presets`, or one of the
editor's own specs in `crates/aria-ui/src/motion.rs`. A spec is a duration, an
optional delay, and a named curve.

`crates/aria-ui/src/motion_catalog.rs` holds the same specs as a table with a
purpose and a call site per row. Its tests check this document: a preset or an
editor spec with no row, a duration or a curve that moved, a call site that
does not exist, and a spec missing from this table all fail.

## Speed and reduced motion

Settings → Appearance offers four choices. Each one installs a
`gpui::motion::MotionPolicy`: the multiplier applied to every duration below,
and whether transitions run at all.

| Choice | Multiplier | Transitions |
| --- | --- | --- |
| Full motion | 1.0 | run |
| Brisk motion | 0.6 | run |
| Relaxed motion | 1.4 | run |
| No motion | 1.0 | skipped; the end state is drawn immediately |

`ARIA_MOTION_SCALE` seeds the multiplier at startup, before the settings file is
read, and is clamped by `MotionPolicy::parse_duration_scale`. The choice in
Settings overrides it for the rest of the session and persists to
`ui-settings.json`.

No motion is honored in two places, not at each call site. A one-shot spec runs
through `with_animation`, which draws the end state and schedules no frames when
motion is reduced. A traveling value runs through `motion::glide`, which reads
the policy of the open motion frame, so a new eased value cannot forget it.
Loaders read a phase of zero and hold still.

## Curves

The curves are `gpui::motion::Easing` variants. The table lists the ones the
catalog rides, by the name `Easing::name` returns.

| Curve | Bezier | Use |
| --- | --- | --- |
| `ease_out_expo` | 0.16, 1.0, 0.3, 1.0 | Long fades, the loader wait |
| `ease_out` | 0.0, 0.0, 0.58, 1.0 | Resize, tab slide, collapse |
| `ease` | 0.25, 0.1, 0.25, 1.0 | Menus, dialogs, splash, chevron, loaders |
| `ease_in_out` | 0.42, 0.0, 0.58, 1.0 | Scroll glide |
| `standard` | 0.4, 0.0, 0.2, 1.0 | Hover fade, caret |
| `ease_out_back` | 0.34, 1.56, 0.64, 1.0 | Momentum settling after a released pan |

A curve's handles determine whether it can overshoot: a bezier stays inside the
hull of its control points, so the curves whose handles are inside the unit
square cannot pass their target, and `ease_out_back`, whose first handle reaches
1.56, does. `CubicBezier::overshoots` reads this off the numbers, and the tests
use it to select which rules a curve is held to. `CubicBezier::eval` clamps to
that hull, so the overshoot is bounded by construction. `ease_out_back` peaks
9.8% past its target and is within half a percent of resting by 94% of its span.

## Specs

| Spec | Source | Duration | Delay | Curve | Spent by |
| --- | --- | --- | --- | --- | --- |
| `FADE_IN` | `presets` | 500 ms | 0 | `ease_out_expo` | `motion::fade_in`: home cards, panel bodies |
| `FADE_QUICK` | `presets` | 150 ms | 0 | `ease` | `motion::fade_quick`: seek badge, drop hints |
| `MENU_IN` | `presets` | 140 ms | 0 | `ease` | `motion::menu_in`: menus and popovers |
| `MENU_OUT` | `presets` | 100 ms | 0 | `ease` | `plugins::overlay_host::exit_window`: every overlay leaving, and `motion::glide::PLAYHEAD` |
| `DIALOG_IN` | `presets` | 180 ms | 0 | `ease` | `motion::dialog_in`: dialog surfaces; and `motion::glide::ROUTE`, the route the shell is drawing |
| `SPLASH_OUT` | `presets` | 500 ms | 150 ms | `ease` | `motion::splash_out`: the boot mark |
| `LOADER_WAIT` | `motion` | 320 ms | 0 | `ease_out_expo` | `motion::glide::WAIT`: the span a wait must outlast before a loader appears |
| `RESIZE` | `presets` | 200 ms | 0 | `ease_out` | `motion::glide::EDIT`, `ZOOM`, and `PROGRESS`: rippled clips, the horizontal scale and scroll offset when the editor chose them, and a progress fill catching up with a report |
| `TAB_SLIDE` | `presets` | 150 ms | 0 | `ease_out` | `inspector::tab_strip`: the tab indicator |
| `COLLAPSE` | `presets` | 180 ms | 0 | `ease_out` | `motion::glide::LANE`: lane heights and the property rows a twirl opens |
| `CHEVRON` | `presets` | 200 ms | 0 | `ease` | `components::chevron`: every disclosure arrow, and `panels/inspector/blocks/widgets_keys.rs`' stopwatch |
| `SCROLL_GLIDE` | `presets` | 500 ms | 0 | `ease_in_out` | `motion::glide::SCROLL`: the assistant transcript reaching its newest row |
| `FLING` | `motion` | 500 ms | 0 | `ease_out_back` | `motion::glide::FLING`: the timeline offset after a pan is released |
| `HOVER_FADE` | `presets` | 150 ms | 0 | `standard` | `motion::hover_blend` on interactive chrome, and `motion::glide::STATE`: a clip card's selection ring and lock badge |
| `CARET_BLINK` | `motion` | 1000 ms | 0 | `standard` | `panels::rename_bar`: the caret in a rename field |
| `ZERON_PULSE` | `presets` | 2400 ms | 0 | `ease` | `loaders`: the pulse cycle |
| `GRADIENT_SPIN` | `presets` | 750 ms | 0 | `ease` | `loaders`: the gradient sweep |

A list that arrives at once staggers its rows by `motion::ROW_STAGGER_MS`
(16 ms, one frame at 60 Hz) times the row's place in the list.

The editor opening has no spec of its own. `shell::opening` draws the bar and
the docked panels at `motion::open_stage` of the route value, which travels
on `glide::ROUTE` (`DIALOG_IN`, 180 ms). The panels start
`motion::OPEN_STAGE_LAG` (a quarter) of the route behind the bar. The opening
ends when the route lands, a route change during it continues from the
value on screen, and reduced motion draws every stage at rest.

## Stores

Two stores hold motion that is not a one-shot transition. Both are
`gpui::motion::TweenStore`s keyed by `MotionKey`, a hashed `(kind, id)` pair, so
a per-frame key costs no allocation.

The hover store holds hover blends. `motion::hover_listener` sets a key's toggle,
notifies the cached panel body (or shell chrome beacon) that drew the key, and
`motion::hover_blend` reads how far its fade has come.

`motion::glide` holds traveling `f32` geometry (the playhead, lane heights,
clip positions, pane widths, the timeline scroll offset, the horizontal scale, a
clip card's selection and lock, and how far the route being drawn has arrived)
as a target each value chases. The shell's playhead layer reads the playhead's
glide, so while a keyed step or a jump eases the playhead, the shell chrome
redraws on each frame and the timeline body replays.

Both stores sample one motion frame per draw. The shell, which is the window
root, calls `motion::begin_frame` first in its render: the frame is the app's
`frame_instant` under its `motion_policy`. A zero-size canvas mounted by the
shell calls `motion::end_frame` during prepaint, after every element that read
the stores. It drops the keys no element read during the previous full frame
and notifies each moving panel body (or the shell chrome beacon) on the next
frame while a value is still moving. There is
one such canvas, because a second would advance the stores' frame counter twice
and drop the values the first drew in the same frame.

The glide store also reports how old a wait is. `motion::glide::since` starts a
key at zero instead of at its target and reports how far through a span it has
got, which is what `loaders::worth_showing` reads: a loader appears only once
its wait has lasted one `LOADER_WAIT`, and a wait that ends sooner leaves no entry
behind.

`PulseClock` parks when no spinner is mounted.

## Playback

While playback runs, the shell calls `motion::hold_for_playback` after
`motion::begin_frame`, and both stores draw every value at its target: hover
fades, glides, and the timeline playhead show their end state, so a frame of
playback is spent on the picture and the playhead stands on the frame shown.
One-shot entrances still play. The hold lifts on the first frame after playback
stops.

## Interruption

Both stores retarget. A target that changes mid-flight becomes the new target
with the current drawn value as its origin, so a reversal continues from where
the value is instead of restarting the leg or snapping to the end it never
reached. Nothing queues: a value has one target at a time.

A spec in the table above is a one-shot, played by `gpui`'s animation element
when an element appears or leaves. A one-shot cannot be retargeted, so anything
that can be interrupted belongs in a store rather than in a one-shot. That is
the rule the two stores exist to satisfy: hover, selection, lock, geometry,
scale, and the route are chased values, and the one-shots are entrances and
exits, which happen once per appearance.

## Momentum

A drag under the hand is the hand's: every frame draws the offset the pointer
put there, with no easing at all. The frame the pointer leaves is the only one
that needs physics, and it needs one number: how far the view should keep
going.

`gpui::motion::fling::sample_velocity` keeps that number while the pan moves.
It folds each pointer position into a rate in pixels per millisecond, weighting
the newest sample at 0.4 so one jittery frame cannot decide the throw, clamping a single
sample to 6 px/ms so a pointer that jumps cannot throw the view across the
sequence, and restarting rather than blending across a gap of 90 ms.

`gpui::motion::fling::fling_distance` turns the rate at the release into a
distance: the rate spent over 220 ms of travel. It returns zero, meaning the view stays where the
hand left it, in three cases: a rate under 0.05 px/ms, a last sample 90 ms old
or older, and motion turned off. A pan that was placed rather than thrown,
or paused before it was released, does not drift on afterwards.

A throw toward the start of the sequence lands at zero, and `ease_out_back` draws
that as a bounded rubber band: the offset passes zero, the picture slides right
by under a tenth of the throw, and it comes back. A hand held past the edge does
not stretch, because an offset under the hand is the hand's; the clamp is the
committed value and the overshoot is only ever drawn.

The distance is committed as an ordinary scroll target through
`AppState::fling_timeline_to`, and the travel to it is the `FLING` spec. There is
no second animation system integrating velocity per frame: the gesture produces
a target, and the store that already chases targets carries it there. Any other
scroll clears the flag, whether it is a wheel notch, a fit, or playhead follow,
so the overshoot belongs to the release that earned it.
