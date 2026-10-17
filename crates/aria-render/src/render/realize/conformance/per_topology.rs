//! One named test per layer topology.
//!
//! A named test says which topology stopped drawing; the sweep in [`super`]
//! says whether one is missing a proof at all. Both are needed: the sweep
//! alone reports a set difference, and a name alone cannot notice an absence.

use super::prove;

#[test]
fn solid_topology_paints_its_fill() {
    prove("solid");
}

#[test]
fn shape_topology_paints_its_sdf() {
    prove("shape");
}

#[test]
fn path_shape_topology_paints_its_raster() {
    prove("path-shape");
}

#[test]
fn still_topology_paints_its_decoded_image() {
    prove("still");
}

#[test]
fn media_topology_paints_its_decoded_frame() {
    prove("media");
}

#[test]
fn lottie_topology_paints_its_rasterized_frame() {
    prove("lottie");
}

#[test]
fn sticker_topology_paints_its_catalog_art() {
    prove("sticker");
}

#[test]
fn text_topology_paints_its_shaped_run() {
    prove("text");
}

#[test]
fn canvas_pass_topology_changes_the_canvas_under_it() {
    prove("canvas-pass");
}

#[test]
fn transition_topology_paints_both_of_its_sides() {
    prove("transition");
}

#[test]
fn matte_topology_keeps_content_only_under_its_stencil() {
    prove("matte");
}

#[test]
fn nest_topology_paints_every_layer_of_its_scene() {
    prove("nest");
}
