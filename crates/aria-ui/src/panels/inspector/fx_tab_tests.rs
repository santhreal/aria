//! The Effects tab's Add buttons.
//!
//! Closes a defect of the tab's layout: Add drew one button per line, so the
//! tab ran several frames past its body. The buttons are read from the effect
//! catalog at run time, so an effect added to the catalog is held to the
//! layout. The None note over an empty chain is pinned by `empty_prompt_tests`.
//! Not covered: how full each wrapped line is.

use gpui::TestAppContext;

use super::strip_tests::drawn_with;

#[gpui::test]
fn the_add_buttons_wrap_into_lines_inside_the_body(cx: &mut TestAppContext) {
    let (state, visual) = drawn_with(cx, "fx-add-wrap", "texture.png", "effects", |_| {});
    let body = visual
        .debug_bounds("inspector-body")
        .expect("Effect Controls draws its body");
    let ids: Vec<String> = visual.read(|app| {
        state
            .read(app)
            .effect_browse_list()
            .into_iter()
            .map(|(spec, _)| spec.id.to_string())
            .collect()
    });
    assert!(ids.len() > 2, "the catalog offers too few effects: {ids:?}");
    let mut tops: Vec<f32> = Vec::with_capacity(ids.len());
    for id in &ids {
        let selector: &'static str = format!("fx-add-{id}").leak();
        let drawn = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("the Effects tab draws no {selector}"));
        assert!(
            drawn.left() >= body.left() && drawn.right() <= body.right(),
            "{selector} is drawn at {drawn:?}, past the sides of the body {body:?}"
        );
        tops.push(f32::from(drawn.top()));
    }
    tops.sort_by(f32::total_cmp);
    tops.dedup();
    assert!(
        tops.len() < ids.len(),
        "the {} Add buttons are drawn on {} lines, one each",
        ids.len(),
        tops.len()
    );
}
