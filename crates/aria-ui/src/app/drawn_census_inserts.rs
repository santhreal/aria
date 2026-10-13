//! Driving the insert chains: every gesture the inspector's Audio block and a
//! mixer strip offer, through the registry both draw from.
//!
//! An insert names its chain and its place in that chain, so the census dresses
//! each kind of chain (a clip's, a lane's, the master's) and presses the
//! gestures on it. Every effect the Add list offers is added, and every
//! parameter of every one is stepped: a parameter row wired to its neighbour's
//! slot is the defect a census of one representative would pass.

use std::collections::BTreeSet;

use aria_commands::{Command, EditCommand};
use aria_models::{AudioInsertTarget, ClipId, Easing, Keyframe, Param, TrackId};

use crate::app::AppState;
use crate::app::sent_census::Watch;
use crate::surfaces::insert_rack::{self, Gesture};
use crate::types::UiInsert;

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "insert_rack.rs";

/// Which chain a stage acts on.
#[derive(Clone, Copy)]
enum On {
    Clip,
    Lane,
    Master,
}

/// An editor holding an audio clip on an audio lane, the clip selected.
struct Mix {
    state: AppState,
    clip: ClipId,
    lane: TrackId,
}

fn mix() -> Mix {
    let (state, clip) = crate::app::fixtures::with_audio();
    let lane = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|track| track.clips.iter().any(|drawn| drawn.id == clip.raw()))
        .map(|track| TrackId::from_raw(track.id))
        .expect("the fixture's clip sits on a lane");
    Mix { state, clip, lane }
}

impl Mix {
    fn target(&self, on: On) -> AudioInsertTarget {
        match on {
            On::Clip => AudioInsertTarget::Clip(self.clip),
            On::Lane => AudioInsertTarget::Track(self.lane),
            On::Master => AudioInsertTarget::Master,
        }
    }

    /// `effects` appended to the chain `on`, through the gesture that adds one.
    fn with(mut self, on: On, effects: &[String]) -> Self {
        let target = self.target(on);
        for effect in effects {
            insert_rack::apply(
                &mut self.state,
                Gesture::Add {
                    target,
                    effect_id: effect.clone(),
                },
            )
            .unwrap_or_else(|err| panic!("adding {effect}: {err}"));
        }
        self
    }

    fn rows(&self, on: On) -> &[UiInsert] {
        self.state.audio_insert_rows(self.target(on))
    }
}

/// Every effect the Add list offers, by id.
fn offered() -> Vec<String> {
    crate::catalog_text::audio_effects()
        .iter()
        .map(|choice| choice.id.to_string())
        .collect()
}

/// The first two effects offered: what a reorder needs.
fn two() -> Vec<String> {
    offered().into_iter().take(2).collect()
}

/// The first effect that reads a key lane, read out of the catalog.
fn keyed_effect() -> String {
    aria_models::audio_effect_catalog()
        .iter()
        .find(|spec| spec.sidechain)
        .map(|spec| spec.id.clone())
        .expect("the catalog holds an effect that reads a key")
}

/// One step of `slot` on the insert at 0 of `on`, toward the middle of its
/// range so it is never held at an end.
fn nudge(mix: &Mix, on: On, slot: usize) -> Gesture {
    let param = &mix.rows(on)[0].params[slot];
    let step = (param.max - param.min) / 100.0;
    let by = if param.value < (param.min + param.max) / 2.0 {
        step
    } else {
        -step
    };
    Gesture::Nudge {
        target: mix.target(on),
        index: 0,
        slot,
        by,
    }
}

/// One dressed editor and the gestures pressed on it, in order.
struct Stage {
    mix: Mix,
    gestures: Vec<Gesture>,
}

/// Every gesture that edits the project, each on a chain dressed for it.
fn staged() -> Vec<Stage> {
    let mut out = Vec::new();

    let fresh = mix();
    let clip = fresh.target(On::Clip);
    out.push(Stage {
        gestures: offered()
            .into_iter()
            .map(|effect_id| Gesture::Add {
                target: clip,
                effect_id,
            })
            .collect(),
        mix: fresh,
    });
    for on in [On::Lane, On::Master] {
        let fresh = mix();
        let target = fresh.target(on);
        out.push(Stage {
            gestures: vec![Gesture::Add {
                target,
                effect_id: offered().remove(0),
            }],
            mix: fresh,
        });
    }

    let one = mix().with(On::Clip, &two()[..1]);
    out.push(Stage {
        gestures: vec![
            Gesture::Bypass {
                target: one.target(On::Clip),
                index: 0,
            },
            Gesture::Bypass {
                target: one.target(On::Clip),
                index: 0,
            },
            Gesture::Remove {
                target: one.target(On::Clip),
                index: 0,
            },
        ],
        mix: one,
    });
    for (up, index) in [(true, 1), (false, 0)] {
        let pair = mix().with(On::Clip, &two());
        let target = pair.target(On::Clip);
        out.push(Stage {
            gestures: vec![if up {
                Gesture::MoveUp { target, index }
            } else {
                Gesture::MoveDown { target, index }
            }],
            mix: pair,
        });
    }

    for effect in offered() {
        let dressed = mix().with(On::Clip, std::slice::from_ref(&effect));
        let slots = dressed.rows(On::Clip)[0].params.len();
        if slots == 0 {
            continue;
        }
        out.push(Stage {
            gestures: (0..slots)
                .map(|slot| nudge(&dressed, On::Clip, slot))
                .collect(),
            mix: dressed,
        });
    }

    let keyed = mix().with(On::Master, &[keyed_effect()]);
    let lane = keyed.lane;
    out.push(Stage {
        gestures: vec![
            Gesture::Key {
                target: AudioInsertTarget::Master,
                index: 0,
                key: Some(lane),
            },
            Gesture::Key {
                target: AudioInsertTarget::Master,
                index: 0,
                key: None,
            },
        ],
        mix: keyed,
    });
    out
}

/// The gestures that open and close a list or a row rather than edit: the
/// census of commands is a census of edits, so they are pressed apart.
fn staged_locally() -> Vec<Stage> {
    let one = mix().with(On::Lane, &two()[..1]);
    let lane = one.target(On::Lane);
    vec![Stage {
        gestures: vec![
            Gesture::TogglePicker(lane),
            Gesture::Open {
                target: lane,
                index: 0,
            },
        ],
        mix: one,
    }]
}

/// Every command the chains asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for mut stage in staged() {
        let watch = Watch::on(&stage.mix.state);
        for gesture in stage.gestures {
            let what = gesture.what();
            insert_rack::apply(&mut stage.mix.state, gesture)
                .unwrap_or_else(|err| panic!("{what} was refused: {err}"));
        }
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_gesture_the_chain_offers_is_driven() {
    let driven: BTreeSet<String> = staged()
        .iter()
        .chain(staged_locally().iter())
        .flat_map(|stage| stage.gestures.iter())
        .map(|gesture| gesture.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !driven.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "the insert chain offers gestures no dressed chain drives: {missing:?}"
    );
}

#[test]
fn every_edit_gesture_asks_the_model_for_something() {
    for mut stage in staged() {
        for gesture in stage.gestures {
            let watch = Watch::on(&stage.mix.state);
            let what = gesture.what();
            insert_rack::apply(&mut stage.mix.state, gesture)
                .unwrap_or_else(|err| panic!("{what} was refused: {err}"));
            assert!(
                !watch.take().is_empty(),
                "{what} sent nothing, so either the control is inert or its chain \
                 is dressed with nothing for it to act on"
            );
        }
    }
}

#[test]
fn the_census_reaches_every_insert_command() {
    let sent = dispatched();
    for label in [
        "Add audio effect",
        "Remove audio effect",
        "Reorder audio effect",
        "Set audio effect parameter",
        "Bypass audio effect",
        "Set sidechain",
    ] {
        assert!(
            sent.contains(label),
            "the insert chains sent {sent:?}, which does not include `{label}`"
        );
    }
}

#[test]
fn the_chain_and_its_hosts_call_no_setter_of_their_own() {
    for under in ["insert_chain.rs", "mixer.rs", "inspector"] {
        super::no_setter_escapes(REGISTRY, under);
    }
}

/// Every effect lands on the chain it was added to, in order, under the label
/// the catalog gives it, and an effect added to a lane or the master lands
/// there and nowhere else.
#[test]
fn an_added_effect_lands_on_its_own_chain_in_order() {
    let effects = offered();
    let mut stages = staged().into_iter();
    let mut clip = stages.next().expect("the clip's Add stage");
    for gesture in clip.gestures {
        insert_rack::apply(&mut clip.mix.state, gesture).expect("an effect is added");
    }
    let landed: Vec<String> = clip
        .mix
        .rows(On::Clip)
        .iter()
        .map(|row| row.effect_id.to_string())
        .collect();
    assert_eq!(
        landed, effects,
        "the clip's chain is not the Add list in order"
    );
    assert!(
        clip.mix.rows(On::Clip).iter().all(|row| row.installed),
        "a shipped effect drew as not installed"
    );
    for on in [On::Lane, On::Master] {
        let dressed = mix().with(on, &two()[..1]);
        assert_eq!(dressed.rows(on).len(), 1, "the effect did not land");
        assert!(
            dressed.rows(On::Clip).is_empty(),
            "an effect added to a lane or the master landed on the clip"
        );
    }
}

/// A step moves the slot it was drawn for and no other, for every parameter of
/// every effect offered.
#[test]
fn every_parameter_steps_its_own_slot() {
    for effect in offered() {
        let mut dressed = mix().with(On::Clip, std::slice::from_ref(&effect));
        let slots = dressed.rows(On::Clip)[0].params.len();
        for slot in 0..slots {
            let before: Vec<f32> = values(&dressed);
            let gesture = nudge(&dressed, On::Clip, slot);
            insert_rack::apply(&mut dressed.state, gesture).expect("a step lands");
            let after: Vec<f32> = values(&dressed);
            for (at, (was, is)) in before.iter().zip(&after).enumerate() {
                if at == slot {
                    assert_ne!(was, is, "{effect} slot {slot} did not move");
                } else {
                    assert_eq!(was, is, "stepping {effect} slot {slot} moved slot {at}");
                }
            }
        }
    }
}

fn values(mix: &Mix) -> Vec<f32> {
    mix.rows(On::Clip)[0]
        .params
        .iter()
        .map(|param| param.value)
        .collect()
}

/// A keyframed parameter keeps its curve when stepped: the step writes a
/// keyframe at the playhead, in the clip's own ticks, rather than flattening
/// the automation to a constant.
#[test]
fn stepping_a_keyed_parameter_writes_a_keyframe_at_the_playhead() {
    let effect = offered()
        .into_iter()
        .find(|id| aria_models::audio_effect_spec(id).is_some_and(|spec| !spec.params.is_empty()))
        .expect("an effect with a parameter");
    let mut dressed = mix().with(On::Clip, std::slice::from_ref(&effect));
    let target = dressed.target(On::Clip);
    let row = dressed.rows(On::Clip)[0].params[0].clone();
    let curve = Param::Keyframed {
        keyframes: vec![
            Keyframe::new(0, row.min, Easing::Linear),
            Keyframe::new(10, row.max, Easing::Linear),
        ],
    };
    dressed
        .state
        .apply_command(Command::Edit(EditCommand::SetAudioEffectParam {
            target,
            index: 0,
            param: 0,
            value: curve,
        }))
        .expect("a keyed parameter");
    let start = {
        let engine = dressed.state.engine.lock();
        engine
            .project()
            .timeline()
            .clip(dressed.clip)
            .expect("the clip")
            .timeline
            .start
            .value
    };
    dressed.state.playhead.value = start + 5;
    let by = (row.max - row.min) / 100.0;
    insert_rack::apply(
        &mut dressed.state,
        Gesture::Nudge {
            target,
            index: 0,
            slot: 0,
            by,
        },
    )
    .expect("a step lands");
    let engine = dressed.state.engine.lock();
    let instance = &engine.project().audio_inserts(target).expect("the chain")[0];
    let spec = instance.spec().expect("a shipped effect");
    let curve = instance.param_curve(&spec, 0).expect("the slot");
    let ticks: Vec<i64> = curve.keyframes().iter().map(|key| key.tick).collect();
    assert_eq!(
        ticks,
        [0, 5, 10],
        "the step did not key the playhead's tick"
    );
    let midway = row.min + (row.max - row.min) / 2.0;
    let written = curve.keyframes()[1].value;
    assert!(
        (written - (midway + by)).abs() < 1e-3,
        "the keyframe holds {written}, not the curve's value there plus one step"
    );
}

/// A row names an insert by its place in the chain, and an undo can shorten
/// the chain between the frame that drew the row and the press. That press
/// is inert: it sends nothing and opens nothing.
#[test]
fn a_gesture_past_the_end_of_the_chain_is_inert() {
    let dressed = mix().with(On::Lane, &two()[..1]);
    let lane = dressed.target(On::Lane);
    let mut state = dressed.state;
    for gesture in [
        Gesture::Remove {
            target: lane,
            index: 9,
        },
        Gesture::MoveUp {
            target: lane,
            index: 9,
        },
        Gesture::MoveDown {
            target: lane,
            index: 9,
        },
        Gesture::Bypass {
            target: lane,
            index: 9,
        },
        Gesture::Open {
            target: lane,
            index: 9,
        },
        Gesture::Nudge {
            target: lane,
            index: 9,
            slot: 0,
            by: 0.1,
        },
        Gesture::Key {
            target: lane,
            index: 9,
            key: None,
        },
    ] {
        let watch = Watch::on(&state);
        let what = gesture.what();
        insert_rack::apply(&mut state, gesture)
            .unwrap_or_else(|err| panic!("{what} past the end of the chain: {err}"));
        let sent = watch.take();
        assert!(
            sent.is_empty(),
            "{what} on an insert that is not there sent {sent:?}"
        );
        assert_eq!(
            state.open_insert, None,
            "{what} opened a row that is not there"
        );
    }
}

/// Moving the first insert up and the last one down are the presses the chain
/// does not draw, and both are inert rather than reorder something else.
#[test]
fn moving_an_end_of_the_chain_outward_does_nothing() {
    for (up, edge) in [(true, 0), (false, 1)] {
        let mut pair = mix().with(On::Clip, &two());
        let target = pair.target(On::Clip);
        let gesture = if up {
            Gesture::MoveUp {
                target,
                index: edge,
            }
        } else {
            Gesture::MoveDown {
                target,
                index: edge,
            }
        };
        let watch = Watch::on(&pair.state);
        insert_rack::apply(&mut pair.state, gesture).expect("an edge move is not an error");
        let sent = watch.take();
        assert!(
            sent.is_empty(),
            "moving the insert at {edge} outward sent {sent:?}"
        );
    }
}

/// The Add list closes once it has added, and an open insert's parameters
/// follow it through a reorder and a removal ahead of it.
#[test]
fn the_open_row_follows_its_insert() {
    let mut three = mix().with(On::Lane, &offered()[..3]);
    let lane = three.target(On::Lane);
    insert_rack::apply(&mut three.state, Gesture::TogglePicker(lane)).expect("the list opens");
    assert_eq!(
        three.state.insert_picker,
        Some(lane),
        "the Add list did not open"
    );
    // The chain holds three; this is the fourth.
    insert_rack::apply(
        &mut three.state,
        Gesture::Add {
            target: lane,
            effect_id: offered().remove(0),
        },
    )
    .expect("a fourth insert");
    assert_eq!(
        three.state.insert_picker, None,
        "adding left the Add list open"
    );

    insert_rack::apply(
        &mut three.state,
        Gesture::Open {
            target: lane,
            index: 2,
        },
    )
    .expect("the row opens");
    insert_rack::apply(
        &mut three.state,
        Gesture::MoveUp {
            target: lane,
            index: 2,
        },
    )
    .expect("the insert moves");
    assert_eq!(
        three.state.open_insert,
        Some((lane, 1)),
        "the open row stayed behind"
    );
    insert_rack::apply(
        &mut three.state,
        Gesture::Remove {
            target: lane,
            index: 0,
        },
    )
    .expect("the first insert goes");
    assert_eq!(
        three.state.open_insert,
        Some((lane, 0)),
        "the open row did not follow its insert down"
    );
    insert_rack::apply(
        &mut three.state,
        Gesture::Remove {
            target: lane,
            index: 0,
        },
    )
    .expect("the open insert goes");
    assert_eq!(
        three.state.open_insert, None,
        "a removed insert's row stayed open"
    );
}

/// The master is keyed by any lane with sound, a lane by none that it feeds or
/// is, and a clip by none at all: the key list is the model's refusal, read.
#[test]
fn the_key_list_offers_the_lanes_the_model_accepts() {
    let dressed = mix();
    let master = dressed.state.master_chain();
    assert!(
        master
            .keys
            .iter()
            .any(|choice| choice.track == dressed.lane.raw()),
        "the master offers no key from the audio lane"
    );
    let strip_keys: Vec<u64> = dressed
        .state
        .mixer_strips()
        .expect("the mix reads")
        .iter()
        .find(|strip| strip.track == dressed.lane.raw())
        .map(|strip| strip.keys.iter().map(|choice| choice.track).collect())
        .expect("the audio lane has a strip");
    assert!(
        !strip_keys.contains(&dressed.lane.raw()),
        "a lane offers itself as its own key"
    );
}
