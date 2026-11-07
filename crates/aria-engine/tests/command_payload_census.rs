//! WHY: a command reaches the engine as JSON from a script, a plugin or the
//! assistant, with values no menu or drag writes. The registry fuzz draws its
//! hostile values from a hand-written generator, so a field the generator
//! never varies is never tried. Here each variant's first drawn command that
//! applies to the seeded project is the base, and every field of it is
//! replaced in turn by each hostile value, read back through the command's
//! own deserializer, and applied: an edit must undo to the exact project it
//! edited and redo to the one it made, a refusal must change nothing, and
//! every state must save and reopen as itself, all within the command time
//! budget. A base that applies puts each replaced value past every other
//! check, so a missing check on that value is reached. Variants and fields are
//! drawn from the registry and the command's JSON at run time; a variant
//! with no applying draw fails the census unless `NO_APPLYING_DRAW` records
//! it. Does not catch a value the engine accepts that is wrong but keeps
//! every rule, or a field a variant's drawn command leaves out of its JSON.

use std::collections::{BTreeMap, BTreeSet};

use aria_commands::random_edit_command_for;
use aria_models::{AudioEffectSpec, EffectParamKind, EffectParamSpec, register_audio_effect};

use super::*;

/// Variants no walk applies, with the reason.
const NO_APPLYING_DRAW: &[(&str, &str)] = &[];

/// Commands the walk that finds the bases draws at most; it stops once every
/// variant has `BASES` bases.
const DRAWS: usize = 400_000;

/// Bases kept per variant: each is a different project and command, so a
/// value reaches more of the states it can meet.
const BASES: usize = 4;

/// Commands the walk applies before it starts again from the seeded project.
/// It also starts again once the open timeline holds no clip: removals empty
/// it, or an edit opens an empty sequence, and then every command that names
/// a clip is refused until the restart.
const WALK: usize = 200;

/// Register `aria.compressor`, the audio effect the command generator's
/// insert edits name, with a sidechain and one parameter, so those edits reach
/// a chain. It stays registered for the rest of the process: a test running
/// beside this one sees one catalog from start to end, and an undo there
/// never meets an effect that was unregistered under it. No other test names
/// this id.
fn register_the_drawn_audio_effect() {
    let threshold = EffectParamSpec {
        name: "threshold".into(),
        label: "Threshold".into(),
        kind: EffectParamKind::Scalar,
        default: -12.0,
        min: -60.0,
        max: 0.0,
        ..Default::default()
    };
    let (guard, _) = register_audio_effect(AudioEffectSpec {
        id: "aria.compressor".into(),
        category: String::new(),
        label: "Compressor".into(),
        params: vec![threshold],
        latency: 32,
        sidechain: true,
    })
    .expect("no other test registers aria.compressor");
    std::mem::forget(guard);
}

/// The engine a case starts from: `project`, history empty.
fn fresh(project: &Project) -> Engine {
    Engine::with_project(EngineConfig { undo_limit: 256 }, project.clone()).expect("engine")
}

/// For each variant the walk applies, up to `BASES` projects it applied to
/// with the command of that variant that applied, and every rule an applied
/// command broke.
///
/// Each step draws a command of a variant short of `BASES` bases or, on
/// alternate steps, a command of a kin of one (a variant sharing two field
/// names with it) or a random command, and keeps it if it applies, so a
/// variant needing an effect, a mask, a send or two keyframes finds them made
/// by earlier commands. Half the drawn commands of a named variant take
/// fields from a command applied since the last restart, so an ease edit
/// names the clip, parameter and tick an earlier keyframe edit wrote.
fn applying_bases(
    seeded: &Project,
    faults: &mut BTreeMap<String, String>,
) -> BTreeMap<&'static str, Vec<(Project, EditCommand)>> {
    let mut rng = CommandRng::new(11);
    let fields = field_names(seeded);
    let mut bases: BTreeMap<&str, Vec<(Project, EditCommand)>> = BTreeMap::new();
    let short =
        |bases: &BTreeMap<&str, Vec<_>>, id: &str| bases.get(id).map_or(0, Vec::len) < BASES;
    let mut engine = fresh(seeded);
    let mut applied: Vec<serde_json::Value> = Vec::new();
    let mut kin: Vec<&str> = Vec::new();
    let mut kin_of = usize::MAX;
    for step in 0..DRAWS {
        let missing: Vec<&str> = edit_specs()
            .iter()
            .map(|spec| spec.id)
            .filter(|id| short(&bases, id))
            .collect();
        if missing.is_empty() {
            break;
        }
        if missing.len() != kin_of {
            kin_of = missing.len();
            kin = fields
                .iter()
                .filter(|(id, names)| {
                    missing
                        .iter()
                        .any(|want| want != *id && names.intersection(&fields[want]).count() >= 2)
                })
                .map(|(id, _)| *id)
                .collect();
        }
        let variant = if step % 2 == 1 {
            Some(*rng.pick(&missing))
        } else if !kin.is_empty() && rng.next_bool() {
            Some(*rng.pick(&kin))
        } else {
            None
        };
        let cmd = match variant {
            Some(variant) => {
                let drawn = random_edit_command_for(&mut rng, engine.project(), variant);
                if rng.next_bool() {
                    spliced(&mut rng, drawn, &applied)
                } else {
                    drawn
                }
            }
            None => random_edit_command(&mut rng, engine.project()),
        };
        let id = cmd.spec().id;
        let before = short(&bases, id).then(|| engine.project().clone());
        if engine.apply(Command::Edit(cmd.clone())).is_err() {
            continue;
        }
        let kept = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_project_invariants(engine.project());
        }));
        if let Err(payload) = kept {
            faults.entry(format!("walk {id}")).or_insert_with(|| {
                format!(
                    "{}: {}",
                    serde_json::to_string(&cmd).expect("an edit command is JSON"),
                    panic_text(&*payload)
                )
            });
            engine = fresh(seeded);
            applied.clear();
            continue;
        }
        applied.push(serde_json::to_value(&cmd).expect("an edit command is JSON"));
        if let Some(project) = before {
            bases.entry(id).or_default().push((project, cmd));
        }
        if applied.len() == WALK || engine.project().timeline().clip_count() == 0 {
            engine = fresh(seeded);
            applied.clear();
        }
    }
    bases
}

/// The field names of each variant's command, as drawn on `seeded`.
fn field_names(seeded: &Project) -> BTreeMap<&'static str, BTreeSet<String>> {
    let mut rng = CommandRng::new(0);
    edit_specs()
        .iter()
        .map(|spec| {
            let drawn = random_edit_command_for(&mut rng, seeded, spec.id);
            let doc = serde_json::to_value(&drawn).expect("an edit command is JSON");
            let names = doc
                .as_object()
                .map(|fields| {
                    fields
                        .keys()
                        .filter(|name| *name != "type")
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            (spec.id, names)
        })
        .collect()
}

/// `drawn` with the fields it shares with a command in `applied` taken from
/// that command, all of them or, half the time, each with even odds, or
/// `drawn` when no applied command shares two of its fields or the result
/// does not read as a command. An ease edit draws the clip, parameter and
/// tick of a keyframe edit that applied, and a keyframe edit the clip and
/// parameter of one at another tick.
fn spliced(rng: &mut CommandRng, drawn: EditCommand, applied: &[serde_json::Value]) -> EditCommand {
    let mut doc = serde_json::to_value(&drawn).expect("an edit command is JSON");
    let Some(fields) = doc.as_object_mut() else {
        return drawn;
    };
    let shared = |donor: &serde_json::Map<String, serde_json::Value>| {
        fields
            .keys()
            .filter(|name| *name != "type" && donor.contains_key(*name))
            .count()
    };
    let donors: Vec<&serde_json::Map<String, serde_json::Value>> = applied
        .iter()
        .filter_map(serde_json::Value::as_object)
        .filter(|donor| shared(donor) >= 2)
        .collect();
    if donors.is_empty() {
        return drawn;
    }
    let donor = *rng.pick(&donors);
    let all = rng.next_bool();
    for (name, value) in fields.iter_mut() {
        if name != "type"
            && let Some(given) = donor.get(name)
            && (all || rng.next_bool())
        {
            *value = given.clone();
        }
    }
    serde_json::from_value(doc).unwrap_or(drawn)
}

#[test]
fn every_field_of_every_edit_command_replaced_applies_or_is_refused() {
    register_the_drawn_audio_effect();
    let (seeded, _v1, _a1, _c1, _c2) = build_seeded_engine();
    let mut faults = BTreeMap::new();
    let bases = applying_bases(seeded.project(), &mut faults);
    let hostile = [
        serde_json::Value::Null,
        serde_json::json!(-1),
        serde_json::json!(0),
        serde_json::json!(i64::MAX),
        serde_json::json!(i64::MIN),
        serde_json::json!(-1.0e308),
        serde_json::json!(0.5),
        serde_json::json!(""),
        serde_json::json!([]),
    ];
    let (mut cases, mut unapplied) = (0usize, Vec::new());
    for spec in edit_specs() {
        let Some(found) = bases.get(spec.id) else {
            unapplied.push(spec.id);
            continue;
        };
        for (base, cmd) in found {
            let mut engine = fresh(base);
            let doc = serde_json::to_value(cmd).expect("an edit command is JSON");
            let mut fields = Vec::new();
            pointers(&doc, String::new(), &mut fields);
            for pointer in fields.iter().filter(|p| !p.is_empty() && *p != "/type") {
                for value in &hostile {
                    let mut edited = doc.clone();
                    *edited.pointer_mut(pointer).expect("a collected pointer") = value.clone();
                    let Ok(cmd) = serde_json::from_value::<EditCommand>(edited) else {
                        continue;
                    };
                    cases += 1;
                    let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        dispatch_and_check(&mut engine, cmd)
                    }));
                    match checked {
                        Ok(true) => {
                            assert!(engine.undo(), "{} redid but does not undo again", spec.id)
                        }
                        Ok(false) => {}
                        Err(payload) => {
                            faults
                                .entry(format!("{} {pointer}", spec.id))
                                .or_insert_with(|| format!("= {value}: {}", panic_text(&*payload)));
                            engine = fresh(base);
                        }
                    }
                }
            }
        }
    }
    let recorded: Vec<&str> = NO_APPLYING_DRAW.iter().map(|(id, _)| *id).collect();
    assert!(
        faults.is_empty() && unapplied == recorded,
        "{} command fields break a rule under a hostile value, each with its first:\n{}\n\
         variants no walk of {DRAWS} draws applies: {unapplied:?}, recorded: {recorded:?}",
        faults.len(),
        faults
            .iter()
            .map(|(field, fault)| format!("{field} {fault}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(cases > 1000, "the census applied {cases} commands");
}
