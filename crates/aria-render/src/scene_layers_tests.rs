use super::*;

/// The exhaustive `index_of` match is the compile-time gate; this is the proof
/// that each arm points at its *own* row instead of reusing a neighbour's, which
/// would compile and silently give a topology the wrong answers.
///
/// Both sides are derived: the rows come from [`SHIPPED`], the samples from each
/// row's own census. Nothing here counts variants.
#[test]
fn a_row_per_layer_topology() {
    for (index, spec) in SHIPPED.iter().enumerate() {
        let sample = (spec.census)();
        assert_eq!(
            index_of(&sample),
            index,
            "the '{}' row's census resolves to '{}' instead",
            spec.id,
            SHIPPED[index_of(&sample)].id
        );
        assert_eq!(
            layer_source_spec(&sample).id,
            spec.id,
            "layer_source_spec did not return the '{}' row",
            spec.id
        );
        assert_eq!(
            sample.kind(),
            spec.id,
            "LayerSource::kind disagrees with the '{}' row",
            spec.id
        );
        assert_eq!(
            layer_source_by_id(spec.id).map(|found| found.id),
            Some(spec.id),
            "'{}' is not reachable by id",
            spec.id
        );
    }
}

#[test]
fn every_row_has_a_distinct_id() {
    let mut ids: Vec<&str> = SHIPPED.iter().map(|spec| spec.id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), before, "two topology rows share an id");
}

/// A refusal message spells the topology's id with spaces, so an id has to
/// survive that transformation into something an operator can read.
#[test]
fn every_id_reads_as_prose() {
    for spec in SHIPPED {
        let named = spec.id.replace('-', " ");
        assert!(!named.is_empty(), "a row has no id");
        assert!(
            named
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == ' ' || c.is_ascii_digit()),
            "'{}' is not spellable in a refusal message",
            spec.id
        );
    }
}

/// A canvas pass has no geometry, and everything with geometry paints. The two
/// columns are one fact from both sides, so a painting canvas pass is a
/// contradiction.
#[test]
fn painting_and_canvas_pass_are_opposites() {
    for spec in SHIPPED {
        assert_ne!(
            spec.paints, spec.is_canvas_pass,
            "'{}' claims paints={} and is_canvas_pass={}",
            spec.id, spec.paints, spec.is_canvas_pass
        );
    }
}

/// The predicates the resolver and the motion-blur pass read cannot contradict
/// each other: a layer with no quad takes no quad styles and no transform
/// supersampling, and only a wrapper can be refused for holding layers.
#[test]
fn topology_predicates_do_not_contradict_each_other() {
    for spec in SHIPPED {
        assert!(
            !spec.is_canvas_pass || (!spec.takes_layer_styles && !spec.takes_motion_blur),
            "'{}' is a canvas pass that still takes quad work",
            spec.id
        );
        assert!(
            !spec.sprite_partitionable || spec.paints,
            "'{}' can be lifted out as a sprite without drawing anything",
            spec.id
        );
        assert!(
            spec.sprite_partitionable || spec.is_canvas_pass || spec.wraps_layers,
            "'{}' is an ordinary placed layer that cannot be partitioned out",
            spec.id
        );
        assert!(
            !spec.wraps_layers || !spec.takes_motion_blur,
            "'{}' wraps layers and samples a shutter of its own over theirs",
            spec.id
        );
        assert!(
            !spec.wraps_layers || spec.nested == Nested::Refuses,
            "'{}' wraps layers and would nest inside itself",
            spec.id
        );
        assert!(
            spec.nested == Nested::Paints || spec.is_canvas_pass || spec.wraps_layers,
            "'{}' is an ordinary placed layer the sub-scene dispatch refuses",
            spec.id
        );
    }
}

/// The table has to describe a real spread, or a column that is constant is
/// proving nothing: at least one topology on each side of every question the
/// resolver asks.
#[test]
fn every_column_separates_at_least_one_topology() {
    let columns: &[(&str, fn(&LayerSourceSpec) -> bool)] = &[
        ("paints", |spec| spec.paints),
        ("needs_media", |spec| spec.needs_media),
        ("is_canvas_pass", |spec| spec.is_canvas_pass),
        ("wraps_layers", |spec| spec.wraps_layers),
        ("takes_motion_blur", |spec| spec.takes_motion_blur),
        ("takes_layer_styles", |spec| spec.takes_layer_styles),
        ("cpu_raster", |spec| spec.cpu_raster.is_cpu()),
        ("sprite_partitionable", |spec| spec.sprite_partitionable),
        ("nested", |spec| spec.nested == Nested::Refuses),
    ];
    for &(name, column) in columns {
        let yes = SHIPPED.iter().filter(|spec| column(spec)).count();
        assert!(
            yes > 0 && yes < SHIPPED.len(),
            "the {name} column is constant across every topology"
        );
    }
}

/// Every generator's declared `renders_with` names a topology that exists here.
/// The model cannot depend on this crate, so its rows name topologies by id;
/// this is the join that keeps the two lists honest, and both sides are derived.
#[test]
fn every_generator_renders_with_a_declared_topology() {
    for generator in aria_models::generator_specs() {
        for topology in generator.renders_with {
            assert!(
                layer_source_by_id(topology).is_some(),
                "generator '{}' renders with '{topology}', which is not a layer topology",
                generator.id
            );
        }
    }
}

/// The rasterizer column has to name the rasterizer that can actually read the
/// topology's payload, or the generator sizing path picks a rasterizer and then
/// finds no payload to hand it — which is exactly the silent `None` the column
/// replaced.
///
/// Derived from the table: every row, its own census sample, and the variant its
/// column claims. A topology that grows a CPU raster without saying which one,
/// or names one that does not match its payload, goes red here.
#[test]
fn the_raster_column_names_the_rasterizer_that_can_read_the_payload() {
    for spec in SHIPPED {
        let sample = (spec.census)();
        let readable = match spec.cpu_raster {
            CpuRaster::None => !matches!(
                sample,
                LayerSource::Text { .. } | LayerSource::PathShape { .. }
            ),
            CpuRaster::Text => matches!(sample, LayerSource::Text { .. }),
            CpuRaster::Path => matches!(sample, LayerSource::PathShape { .. }),
        };
        assert!(
            readable,
            "the '{}' row declares {:?} but its payload cannot be read by it",
            spec.id, spec.cpu_raster
        );
    }
}
