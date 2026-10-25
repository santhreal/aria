//! Driver evaluation over one project: per frame for the picture, per clip
//! for the audio envelopes, and the cycle check run before code is attached.

use std::borrow::Cow;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::time::Instant;

use aria_models::{
    Clip, ClipId, ClipParam, DriverCode, Easing, Keyframe, Param, ParamValue, Project,
    PropertyDriver, Timeline, clip_param_catalog, effect_spec, visit_clip_params,
};

use super::{
    DriverClipInfo, DriverInput, DriverProgram, DriverScope, InputClip, InputProperty, clear_fault,
    compile_driver, driver_budget, is_stopped, record_fault,
};

/// How a cycle error begins, so a cycle reported to a program and returned by
/// it stays a cycle on the way up.
const CYCLE: &str = "drivers read each other in a cycle";

/// Why a driver produced no value.
#[derive(Debug, Clone)]
enum Failure {
    /// The property is not reachable on the clip; nothing is recorded.
    Unavailable,
    /// The driver failed and its fault is recorded.
    Failed,
    /// Evaluation walked back into a driver that was already running.
    Cycle(String),
}

/// What one frame's evaluation keeps: values already computed and the
/// drivers running now.
#[derive(Default)]
struct State<'p> {
    memo: HashMap<(ClipId, ClipParam, u64), Result<ParamValue, Failure>>,
    running: Vec<(ClipId, ClipParam)>,
    /// Clips by title, topmost lane first, built on the first named read.
    names: Option<HashMap<String, Vec<&'p Clip>>>,
}

/// Evaluates the drivers of the clips on one timeline. Costs nothing until a
/// clip with a driver is applied.
pub struct DriverFrame<'p> {
    project: &'p Project,
    timeline: &'p Timeline,
    fps: f64,
    state: Option<Box<State<'p>>>,
}

impl std::fmt::Debug for DriverFrame<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DriverFrame")
            .field("fps", &self.fps)
            .finish_non_exhaustive()
    }
}

impl<'p> DriverFrame<'p> {
    /// Drivers on the open timeline.
    pub fn new(project: &'p Project) -> Self {
        Self::in_timeline(project, project.timeline())
    }

    /// Drivers on `timeline`, the open one or a nested sequence's. Times are
    /// that timeline's and named inputs are clips on it.
    pub fn in_timeline(project: &'p Project, timeline: &'p Timeline) -> Self {
        Self {
            project,
            timeline,
            fps: timeline.frame_rate.as_f64(),
            state: None,
        }
    }

    fn state(&mut self) -> &mut State<'p> {
        self.state.get_or_insert_with(Box::default)
    }

    /// `clip` with every driven property replaced by its driver's value at
    /// timeline tick `tick`, which may fall between frames. Borrowed unchanged
    /// when no driver produced a value.
    pub fn apply(&mut self, clip: &'p Clip, tick: f64) -> Cow<'p, Clip> {
        if clip.drivers.is_empty() {
            return Cow::Borrowed(clip);
        }
        let mut driven: Option<Clip> = None;
        for driver in &clip.drivers {
            let Ok(value) = self.evaluate(clip, driver, tick) else {
                continue;
            };
            let target = driven.get_or_insert_with(|| clip.clone());
            if let Err(err) = driver.param.set_constant(target, value) {
                record_fault(
                    clip.id,
                    driver.param,
                    &driver.code,
                    format!("the driver's value was refused: {err}"),
                    false,
                );
            }
        }
        driven.map_or(Cow::Borrowed(clip), Cow::Owned)
    }

    /// The value of `param` on `clip` at timeline tick `tick`: its driver's
    /// value, else its keyframed value.
    fn value_of(
        &mut self,
        clip: &'p Clip,
        param: ClipParam,
        tick: f64,
    ) -> Result<ParamValue, String> {
        if let Some(driver) = clip.drivers.iter().find(|driver| driver.param == param) {
            match self.evaluate(clip, driver, tick) {
                Ok(value) => return Ok(value),
                Err(Failure::Cycle(message)) => return Err(message),
                Err(Failure::Unavailable | Failure::Failed) => {}
            }
        }
        param
            .sample_at(clip, clip.param_tick_f(tick))
            .ok_or_else(|| {
                format!(
                    "{} has no {}",
                    self.project.clip_title(clip),
                    property_name(clip, param)
                )
            })
    }

    fn evaluate(
        &mut self,
        clip: &'p Clip,
        driver: &'p PropertyDriver,
        tick: f64,
    ) -> Result<ParamValue, Failure> {
        let param = driver.param;
        let key = (clip.id, param, tick.to_bits());
        if let Some(done) = self.state().memo.get(&key) {
            return done.clone();
        }
        if self.state().running.contains(&(clip.id, param)) {
            let project = self.project;
            let path: Vec<String> = self
                .state()
                .running
                .iter()
                .chain([&(clip.id, param)])
                .map(|&node| describe(project, node))
                .collect();
            return Err(Failure::Cycle(format!("{CYCLE}: {}", path.join(" -> "))));
        }
        let done = self.run(clip, driver, tick);
        self.state().memo.insert(key, done.clone());
        done
    }

    fn run(
        &mut self,
        clip: &'p Clip,
        driver: &'p PropertyDriver,
        tick: f64,
    ) -> Result<ParamValue, Failure> {
        let param = driver.param;
        let Some(own) = param.sample_at(clip, clip.param_tick_f(tick)) else {
            return Err(Failure::Unavailable);
        };
        if is_stopped(clip.id, param, &driver.code) {
            return Err(Failure::Failed);
        }
        let program = match compile_driver(&driver.code) {
            Ok(program) => program,
            Err(message) => {
                record_fault(clip.id, param, &driver.code, message, false);
                return Err(Failure::Failed);
            }
        };
        let budget = driver_budget();
        let started = Instant::now();
        self.state().running.push((clip.id, param));
        let (outcome, cycle) = {
            let mut scope = Scope {
                frame: self,
                clip,
                param,
                program: &program,
                tick,
                own,
                deadline: started + budget,
                keys: OnceCell::new(),
                cycle: None,
            };
            let outcome = catch_unwind(AssertUnwindSafe(|| program.evaluate(&mut scope)));
            (outcome, scope.cycle)
        };
        self.state().running.pop();
        let took = started.elapsed();
        let fail = |message: String, stopped: bool| {
            record_fault(clip.id, param, &driver.code, message, stopped);
            Err(Failure::Failed)
        };
        // A cycle found below is a cycle here too, whatever the program made
        // of the error it was handed.
        if let Some(message) = cycle {
            record_fault(clip.id, param, &driver.code, message.clone(), false);
            return Err(Failure::Cycle(message));
        }
        if took > budget {
            return fail(
                format!(
                    "the driver took {} ms, over its {} ms budget, and is stopped until its code \
                     changes",
                    took.as_millis(),
                    budget.as_millis()
                ),
                true,
            );
        }
        let value = match outcome {
            Err(payload) => {
                return fail(
                    format!(
                        "the driver panicked: {}",
                        crate::audio_processor::panic_message(payload.as_ref())
                    ),
                    false,
                );
            }
            Ok(Err(message)) => return fail(message, false),
            Ok(Ok(value)) => value,
        };
        if std::mem::discriminant(&value) != std::mem::discriminant(&own) {
            return fail(
                format!(
                    "the driver returned {} for a property that holds {}",
                    kind_name(&value),
                    kind_name(&own)
                ),
                false,
            );
        }
        let Some(value) = param.clamp_on(clip, value) else {
            return fail(
                "the driver returned a value that is not finite".into(),
                false,
            );
        };
        clear_fault(clip.id, param);
        Ok(value)
    }

    /// The clip an input names, seen from `from` at timeline tick `tick`.
    fn input_clip(
        &mut self,
        from: &'p Clip,
        clip: &InputClip,
        tick: f64,
    ) -> Result<&'p Clip, String> {
        let name = match clip {
            InputClip::This => return Ok(from),
            InputClip::Named(name) => name,
        };
        let (project, timeline) = (self.project, self.timeline);
        let names = self
            .state()
            .names
            .get_or_insert_with(|| clip_names(project, timeline));
        let Some(candidates) = names.get(name.as_str()) else {
            return Err(format!("no clip in the sequence is named {name:?}"));
        };
        let at = tick.floor() as i64;
        Ok(candidates
            .iter()
            .copied()
            .find(|clip| clip.timeline.start.value <= at && at < clip.timeline.end_tick())
            .unwrap_or(candidates[0]))
    }
}

/// Every clip on `timeline` by title, topmost lane first and in time order
/// within a lane.
fn clip_names<'p>(project: &Project, timeline: &'p Timeline) -> HashMap<String, Vec<&'p Clip>> {
    let mut names: HashMap<String, Vec<&'p Clip>> = HashMap::new();
    let tracks: Vec<_> = timeline.tracks_ordered().collect();
    for track in tracks.into_iter().rev() {
        for clip in track.clips_ordered() {
            names
                .entry(project.clip_title(clip).into_owned())
                .or_default()
                .push(clip);
        }
    }
    names
}

/// The parameter an input property names on `clip`, if `clip` has it.
fn input_param(clip: &Clip, property: &InputProperty) -> Option<ClipParam> {
    match property {
        InputProperty::Id(id) => {
            if let Some(spec) = clip_param_catalog()
                .iter()
                .find(|spec| spec.id == id.as_str())
            {
                return spec.available(clip).then_some(spec.param);
            }
            let mut found = None;
            visit_clip_params(clip, |spec| {
                if found.is_none() && spec.id == id.as_str() {
                    found = Some(spec.param);
                }
            });
            found
        }
        InputProperty::Effect { effect, param } => {
            clip.effects
                .iter()
                .enumerate()
                .find_map(|(index, instance)| {
                    let spec = effect_spec(&instance.effect_id)?;
                    if !spec.label.eq_ignore_ascii_case(effect)
                        && !instance.effect_id.eq_ignore_ascii_case(effect)
                    {
                        return None;
                    }
                    let slot = spec.params.iter().position(|slot| {
                        slot.label.eq_ignore_ascii_case(param)
                            || slot.name.eq_ignore_ascii_case(param)
                    })?;
                    Some(ClipParam::Effect {
                        effect: index as u32,
                        param: slot as u32,
                    })
                })
        }
    }
}

/// A property's catalog id on `clip`, or its debug form when it has none.
fn property_name(clip: &Clip, param: ClipParam) -> String {
    let mut name = None;
    visit_clip_params(clip, |spec| {
        if name.is_none() && spec.param == param {
            name = Some(spec.id.to_string());
        }
    });
    name.unwrap_or_else(|| format!("{param:?}"))
}

fn kind_name(value: &ParamValue) -> &'static str {
    match value {
        ParamValue::Scalar(_) => "a number",
        ParamValue::Vec2(_) => "a pair",
        ParamValue::Vec3(_) => "a 3D point",
        ParamValue::Color(_) => "a color",
        ParamValue::Rect(_) => "a rectangle",
    }
}

struct Scope<'f, 'p> {
    frame: &'f mut DriverFrame<'p>,
    clip: &'p Clip,
    param: ClipParam,
    program: &'f Arc<dyn DriverProgram>,
    tick: f64,
    own: ParamValue,
    deadline: Instant,
    /// The property's keyframe ticks on the timeline, read on first use.
    keys: OnceCell<Vec<i64>>,
    /// The first cycle an input read walked into.
    cycle: Option<String>,
}

impl Scope<'_, '_> {
    fn keys(&self) -> &[i64] {
        self.keys.get_or_init(|| {
            let mut keys = Vec::new();
            self.param.visit_keyframe_ticks(self.clip, |tick| {
                keys.push(self.clip.param_timeline_tick(tick))
            });
            keys
        })
    }

    fn read(&mut self, index: usize, tick: f64) -> Result<ParamValue, String> {
        let Some(input) = self.program.inputs().get(index) else {
            return Err(format!(
                "the driver read input {index}, which it did not declare"
            ));
        };
        let target = self.frame.input_clip(self.clip, &input.clip, tick)?;
        let param = input_param(target, &input.property).ok_or_else(|| {
            format!(
                "{} has no property {}",
                self.frame.project.clip_title(target),
                describe_property(&input.property)
            )
        })?;
        let value = self.frame.value_of(target, param, tick);
        if let Err(message) = &value
            && message.starts_with(CYCLE)
        {
            self.cycle.get_or_insert_with(|| message.clone());
        }
        value
    }
}

fn describe_property(property: &InputProperty) -> String {
    match property {
        InputProperty::Id(id) => id.clone(),
        InputProperty::Effect { effect, param } => format!("{effect} > {param}"),
    }
}

impl DriverScope for Scope<'_, '_> {
    fn time(&self) -> f64 {
        self.tick / self.frame.fps
    }

    fn property(&self) -> ClipParam {
        self.param
    }

    fn clip_time(&self) -> f64 {
        (self.tick - self.clip.timeline.start.value as f64) / self.frame.fps
    }

    fn frame_rate(&self) -> f64 {
        self.frame.fps
    }

    fn duration(&self) -> f64 {
        self.frame.timeline.duration().value as f64 / self.frame.fps
    }

    fn canvas(&self) -> [u32; 2] {
        let (width, height) = crate::resolve::canvas_size(self.frame.project);
        [width, height]
    }

    fn value(&self) -> ParamValue {
        self.own
    }

    fn value_at(&self, seconds: f64) -> ParamValue {
        let tick = seconds * self.frame.fps;
        self.param
            .sample_at(self.clip, self.clip.param_tick_f(tick))
            .unwrap_or(self.own)
    }

    fn key_count(&self) -> usize {
        self.keys().len()
    }

    fn key_time(&self, index: usize) -> Option<f64> {
        self.keys()
            .get(index)
            .map(|&tick| tick as f64 / self.frame.fps)
    }

    fn input(&mut self, index: usize) -> Result<ParamValue, String> {
        self.read(index, self.tick)
    }

    fn input_at(&mut self, index: usize, seconds: f64) -> Result<ParamValue, String> {
        if !seconds.is_finite() {
            return Err("an input was read at a time that is not a number".into());
        }
        self.read(index, seconds * self.frame.fps)
    }

    fn clip_info(&mut self, clip: &InputClip) -> Result<DriverClipInfo, String> {
        let target = self.frame.input_clip(self.clip, clip, self.tick)?;
        Ok(DriverClipInfo {
            name: self.frame.project.clip_title(target).into_owned(),
            in_point: target.timeline.start.value as f64 / self.frame.fps,
            out_point: target.timeline.end_tick() as f64 / self.frame.fps,
        })
    }

    fn seed(&self) -> u64 {
        // FNV-1a, so a seed survives a toolchain upgrade.
        let param = format!("{:?}", self.param);
        self.clip
            .id
            .raw()
            .to_le_bytes()
            .iter()
            .chain(param.as_bytes())
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            })
    }

    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }
}

/// Check that attaching `code` to `param` on clip `clip` keeps the driver
/// graph free of cycles. Fails with the compile error when the code does not
/// compile, and with the cycle when one forms.
pub fn check_driver(
    project: &Project,
    clip: ClipId,
    param: ClipParam,
    code: &DriverCode,
) -> Result<(), String> {
    let program = compile_driver(code)?;
    let Some(start) = project.clip(clip) else {
        return Err(format!("clip {} does not exist", clip.raw()));
    };
    let timeline = timeline_of(project, clip).unwrap_or(project.timeline());
    let names = clip_names(project, timeline);
    let root = (clip, param);
    // Depth-first over driven properties, with the path back to the root.
    let mut parent: HashMap<(ClipId, ClipParam), (ClipId, ClipParam)> = HashMap::new();
    let mut seen: HashSet<(ClipId, ClipParam)> = HashSet::new();
    let mut stack = vec![(start, program, root)];
    while let Some((at, program, node)) = stack.pop() {
        if !seen.insert(node) {
            continue;
        }
        for input in program.inputs() {
            let targets: &[&Clip] = match &input.clip {
                InputClip::This => std::slice::from_ref(&at),
                InputClip::Named(name) => names
                    .get(name.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
            };
            for &target in targets {
                let Some(read) = input_param(target, &input.property) else {
                    continue;
                };
                let next = (target.id, read);
                if next == root {
                    let mut path = vec![describe(project, next)];
                    let mut cursor = node;
                    while cursor != root {
                        path.push(describe(project, cursor));
                        let Some(&up) = parent.get(&cursor) else {
                            break;
                        };
                        cursor = up;
                    }
                    path.push(describe(project, root));
                    path.reverse();
                    return Err(format!(
                        "this driver would read its own value back: {}",
                        path.join(" -> ")
                    ));
                }
                let Some(code) = target.driver(read) else {
                    continue;
                };
                let Ok(program) = compile_driver(code) else {
                    continue;
                };
                parent.entry(next).or_insert(node);
                stack.push((target, program, next));
            }
        }
    }
    Ok(())
}

/// The timeline holding clip `id`: the open one first, then the root, then
/// every other sequence, the order [`Project::clip`] searches.
fn timeline_of(project: &Project, id: ClipId) -> Option<&Timeline> {
    [project.timeline(), project.root_timeline()]
        .into_iter()
        .chain(project.sequences().map(|sequence| &sequence.timeline))
        .find(|timeline| timeline.clip(id).is_some())
}

/// A driven property as messages state it: the clip's title and the
/// property's catalog id, such as `Logo opacity`.
pub fn driver_target(project: &Project, clip: ClipId, param: ClipParam) -> String {
    describe(project, (clip, param))
}

fn describe(project: &Project, (id, param): (ClipId, ClipParam)) -> String {
    match project.clip(id) {
        Some(clip) => format!(
            "{} {}",
            project.clip_title(clip),
            property_name(clip, param)
        ),
        None => format!("{param:?}"),
    }
}

/// The input a driver on clip `from` declares to read `param` of clip
/// `source`, which is what the pick whip inserts a reference to. An effect
/// parameter is addressed by the effect's label and the parameter's label,
/// which survive reordering the effect stack, when that pair finds this
/// effect; any other property by its catalog id. `None` when `source` has no
/// such property or no address reads it back.
pub fn driver_input(
    project: &Project,
    from: ClipId,
    source: ClipId,
    param: ClipParam,
) -> Option<DriverInput> {
    let clip = project.clip(source)?;
    let input_clip = if from == source {
        InputClip::This
    } else {
        InputClip::Named(project.clip_title(clip).into_owned())
    };
    let by_label = match param {
        ClipParam::Effect {
            effect,
            param: slot,
        } => clip.effects.get(effect as usize).and_then(|instance| {
            let spec = effect_spec(&instance.effect_id)?;
            let slot = spec.params.get(slot as usize)?;
            Some(InputProperty::Effect {
                effect: spec.label.clone(),
                param: slot.label.clone(),
            })
        }),
        _ => None,
    };
    let by_id = InputProperty::Id(property_name(clip, param));
    by_label
        .into_iter()
        .chain(std::iter::once(by_id))
        .find(|property| input_param(clip, property) == Some(param))
        .map(|property| DriverInput {
            clip: input_clip,
            property,
        })
}

/// A clip's `param` envelope with its driver evaluated at every frame of the
/// clip, as a keyframed curve in the clip's parameter ticks. `None` when the
/// clip has no driver on `param` or the driver produced no value at any frame.
///
/// For the audio envelopes, which the mixer samples per audio frame and
/// cannot evaluate a driver for.
pub fn driven_envelope(project: &Project, clip: &Clip, param: ClipParam) -> Option<Param<f32>> {
    let driver = clip.drivers.iter().find(|driver| driver.param == param)?;
    let timeline = timeline_of(project, clip.id).unwrap_or(project.timeline());
    let mut frame = DriverFrame::in_timeline(project, timeline);
    let start = clip.timeline.start.value;
    let mut keyframes = Vec::with_capacity(clip.timeline.duration.value.max(0) as usize);
    let mut driven = false;
    for tick in start..clip.timeline.end_tick() {
        if let Some(state) = frame.state.as_deref_mut() {
            state.memo.clear();
        }
        let value = match frame.evaluate(clip, driver, tick as f64) {
            Ok(value) => {
                driven = true;
                value
            }
            Err(_) => param.sample_at(clip, clip.param_tick_f(tick as f64))?,
        };
        let ParamValue::Scalar(value) = value else {
            return None;
        };
        let at = clip.param_tick(tick);
        if keyframes
            .last()
            .is_some_and(|last: &Keyframe<f32>| last.tick >= at)
        {
            continue;
        }
        keyframes.push(Keyframe::new(at, value, Easing::Linear));
    }
    (driven && !keyframes.is_empty()).then_some(Param::Keyframed { keyframes })
}
