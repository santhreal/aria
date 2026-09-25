# Animation catalog

Every appearance, disappearance, and travelling value in the UI uses a named
`MotionSpec` from `crates/aria-ui/src/motion.rs`. A spec is one step on the
duration scale and one named curve. Neither can be written as a number: the
scale mints `Beat`, the curve list mints `CubicBezier`, and both constructors
are private, so a panel cannot pass a duration of its own to
`MotionSpec::new`.

`crates/aria-ui/src/motion_catalog.rs` holds the same specs as a table with a
purpose and a call site per row. Its tests check this document: a spec with no
row, a duration that moved, a call site that does not exist, and a spec missing
from this table all fail.

## Speed and reduced motion

Settings → Appearance offers four choices. Each one sets the multiplier applied
to every duration below, and whether transitions run at all.

| Choice | Multiplier | Transitions |
| --- | --- | --- |
| Full motion | 1.0 | run |
| Brisk motion | 0.6 | run |
| Relaxed motion | 1.4 | run |
| No motion | 1.0 | skipped; the end state is drawn immediately |

`ARIA_MOTION_SCALE` seeds the multiplier at startup, before the settings file is
read, and is clamped to 0.01–100. The choice in Settings overrides it for the
rest of the session and persists to `ui-settings.json`.

No motion is honoured in two places, not at each call site. A one-shot spec runs
through `with_animation`, which draws the end state and schedules no frames when
through `motion::glide`, which reads the setting itself, so a new eased value
cannot forget it. Loaders read a phase of zero and hold still.

## The duration scale

`crates/aria-ui/src/motion_scale.rs` holds every length an animation may be.
Adding one is a `pub const` and a row in `SCALE`.

| Step | Length | Use |
| --- | --- | --- |
| `FRAME` | 16 ms | The offset between neighbours arriving together |
| `TAP` | 80 ms | The offset between stages of a layout resolving |
| `FLICK` | 120 ms | Something leaving |
| `QUICK` | 160 ms | A change that must not read as a jump |
| `SNAP` | 200 ms | Geometry settling |
| `SETTLE` | 320 ms | Contents arriving into a frame already on screen |
| `GLIDE` | 500 ms | A whole surface, or a view the editor moved |
| `CYCLE` | 750 ms | One turn of something that repeats |
| `BLINK` | 1000 ms | A caret |
| `BREATH` | 2400 ms | The idle loader breathing |

`FRAME` and `TAP` are staggers rather than transitions. A stagger is
`Beat::times(n)`, so a queue of rows is still on the scale's grid.

## Curves

`motion::CURVES` is the list. A curve that is not in it does not exist, and the
gallery draws a specimen of each row without being told about it.

| Constant | Bezier | Use |
| --- | --- | --- |
| `EASE_OUT_EXPO` | 0.16, 1.0, 0.3, 1.0 | Long fades |
| `EASE_OUT` | 0.0, 0.0, 0.58, 1.0 | Resize, tab slide, collapse |
| `EASE` | 0.25, 0.1, 0.25, 1.0 | Menus, dialogs, splash, chevron |
| `EASE_RESORT` | 0.22, 1.0, 0.36, 1.0 | Clip settle after drag |
| `EASE_IN_OUT` | 0.42, 0.0, 0.58, 1.0 | Scroll glide |
| `EASE_STANDARD` | 0.4, 0.0, 0.2, 1.0 | Hover fade, caret |
| `SPRING` | 0.34, 1.56, 0.64, 1.0 | Momentum settling after a released pan |

A curve's handles say whether it can overshoot: a bezier stays inside the hull
of its control points, so the six curves whose handles are inside the unit
square cannot pass their target, and `SPRING`, whose first handle reaches 1.56,
must. `CubicBezier::overshoots` reads this off the numbers, and the tests use it
to decide which rules a curve is held to. `eval` clamps every curve to 0.15 past
its endpoints, so the overshoot is bounded by construction rather than by
whoever chose the handles. `SPRING` peaks 9.8% past its target and is within
half a percent of resting by 94% of its span.

## Specs

| Spec | Step | Duration | Delay | Curve | Spent by |
| --- | --- | --- | --- | --- | --- |
| `FADE_IN` | `GLIDE` | 500 ms | 0 | `EASE_OUT_EXPO` | `motion::fade_in`: home cards, panel bodies |
| `FADE_QUICK` | `QUICK` | 160 ms | 0 | `EASE` | `motion::fade_quick`: seek badge, drop hints |
| `MENU_IN` | `QUICK` | 160 ms | 0 | `EASE` | `motion::menu_in`: menus and popovers |
| `MENU_OUT` | `FLICK` | 120 ms | 0 | `EASE` | `plugins::overlay_host::exit_window`: every overlay leaving, and `motion::glide::PLAYHEAD` |
| `DIALOG_IN` | `SNAP` | 200 ms | 0 | `EASE` | `motion::dialog_in`: dialog surfaces; and `motion::glide::ROUTE`, the route the shell is drawing |
| `SPLASH_OUT` | `GLIDE` | 500 ms | 160 ms | `EASE` | `loaders`: the boot mark |
| `OPEN` | `SETTLE` | 320 ms | 0 | `EASE_OUT_EXPO` | `shell::opening`: the editor arriving in three stages, one `OPEN_STAGE` (80 ms) apart; and `motion::glide::WAIT`, the span a wait must outlast before a loader appears |
| `RESIZE` | `SNAP` | 200 ms | 0 | `EASE_OUT` | `motion::glide::PANE`, `EDIT`, `ZOOM`, and `PROGRESS`: pane widths, released splitters, rippled clips, the horizontal scale when the editor chose it, and a progress fill catching up with a report |
| `TAB_SLIDE` | `QUICK` | 160 ms | 0 | `EASE_OUT` | `inspector::tab_strip`: the tab indicator |
| `COLLAPSE` | `SNAP` | 200 ms | 0 | `EASE_OUT` | `motion::glide::LANE`: lane heights |
| `CHEVRON` | `SNAP` | 200 ms | 0 | `EASE` | `components::chevron`: every disclosure arrow, and `panels/inspector/blocks/widgets.rs`' stopwatch |
| `SCROLL_GLIDE` | `GLIDE` | 500 ms | 0 | `EASE_IN_OUT` | `motion::glide::SCROLL`: a fit, playhead follow, and the assistant transcript reaching its newest row |
| `FLING` | `GLIDE` | 500 ms | 0 | `SPRING` | `motion::glide::FLING`: the timeline offset after a pan is released |
| `HOVER_FADE` | `QUICK` | 160 ms | 0 | `EASE_STANDARD` | `motion::hover_blend` on interactive chrome, and `motion::glide::STATE`: a clip card's selection ring and lock badge |
| `CARET_BLINK` | `BLINK` | 1000 ms | 0 | `EASE_STANDARD` | `panels::rename_bar`: the caret in a rename field |
| `ZERON_PULSE` | `BREATH` | 2400 ms | 0 | `EASE` | `loaders`: the pulse cycle |
| `GRADIENT_SPIN` | `CYCLE` | 750 ms | 0 | `EASE` | `loaders`: the gradient sweep |

## Stores

Two stores hold motion that is not a one-shot transition.

`motion::HoverFades` holds hover blends, keyed by `MotionKey`. `MotionKey` is a
hashed `(kind, id)` pair, so a per-frame key costs no allocation.

`motion::glide` holds travelling `f32` geometry — the playhead, lane heights,
clip positions, pane widths, the timeline scroll offset, the horizontal scale, a
clip card's selection and lock, and how far the route being drawn has arrived —
as a target each value chases. The shell mounts one driver: a zero-size canvas
that requests the next animation frame while any value is still moving. There is
exactly one, because a second would advance the store's frame counter twice and
evict values the first had just drawn, and it belongs to the shell rather than to
the store because it is the window that draws the frames.

The same store answers how old a wait is. `motion::glide::since` starts a key at
zero instead of at its target and reports how far through a span it has got, which
is what `loaders::worth_showing` reads: a loader appears only once its wait has
lasted one `SETTLE`, and a wait that ends sooner leaves no entry behind.

`PulseClock` parks when no spinner is mounted.

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

`motion::sample_velocity` keeps that number while the pan moves. It folds each
pointer position into a rate in pixels per millisecond, weighting the newest
sample at 0.4 so one jittery frame cannot decide the throw, clamping a single
sample to 6 px/ms so a pointer that jumps cannot throw the view across the
sequence, and restarting rather than blending across a gap of 90 ms.

`motion::fling_distance` turns the rate at the release into a distance: the rate
spent over 220 ms of travel. It returns zero, meaning the view stays where the
hand left it, in three cases: a rate under 0.05 px/ms, a last sample 90 ms old
or older, and motion turned off. A pan that was placed rather than thrown,
or paused before it was released, does not drift on afterwards.

A throw toward the start of the sequence lands at zero, and the spring draws
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
