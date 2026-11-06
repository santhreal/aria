//! Every palette row that does not run now is drawn faint, and every row that
//! runs is drawn live.
//!
//! The class closed here: a palette row drawn live while choosing it is
//! rejected. The palette dimmed a plugin command and a clip type it could not
//! run, and drew every menu row live, so Revert, Lift or Go to In read as ready
//! in the palette while the menu bar drew the same row disabled and choosing it
//! only reported the guard's reason.
//!
//! The whole catalog is drawn with an empty query, and each row's drawn state
//! is read against the source that rejects it: a menu row against the guard of
//! the row the menu registry draws now, a plugin command against its
//! enablement condition, a clip type against its catalog entry. The match on
//! the offer's kind is exhaustive, so a new kind of row fails to compile here
//! until its rejection is stated.
//!
//! Not covered: the faint tone itself, which the frame's text runs do not hold;
//! the selector is set on the same branch that picks the tone.

use gpui::TestAppContext;

use crate::app::palette::Offered;
use crate::chrome::menu_items::items_of;
use crate::interned::intern;
use crate::test_support::drawn_shell;

#[gpui::test]
fn a_palette_row_is_drawn_faint_exactly_while_choosing_it_is_rejected(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "palette-withheld");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.simulate_keystrokes("ctrl-shift-p");
    visual.run_until_parked();
    let rows: Vec<(String, bool, bool)> = visual.read(|app| {
        let state = state.read(app);
        let keys = state.ctx.context_keys();
        state
            .palette_hits()
            .iter()
            .map(|offer| {
                let (menu_row, rejected) = match &offer.action {
                    Offered::Menu(menu, id, _) => {
                        let item = items_of(*menu, state)
                            .into_iter()
                            .find(|item| item.id == *id)
                            .unwrap_or_else(|| {
                                panic!(
                                    "{id} is offered but the {} menu draws no such row",
                                    menu.label()
                                )
                            });
                        (true, item.blocked_by(state).is_some())
                    }
                    Offered::Plugin(spec) => (false, !spec.enabled(keys)),
                    Offered::ClipType { id, .. } => {
                        let catalog = aria_models::clip_type_catalog();
                        let spec = catalog
                            .iter()
                            .find(|spec| spec.id.as_str() == id.as_str())
                            .unwrap_or_else(|| panic!("{id} is offered but not in the catalog"));
                        (false, spec.unavailable().is_some())
                    }
                    Offered::Command(_, _) | Offered::Windowed(_, _) => {
                        let host = crate::commands::host::host_of(&state.ctx)
                            .expect("the ui plugin mounts commands");
                        let entry = host
                            .all()
                            .into_iter()
                            .find(|entry| entry.id == offer.id.as_str())
                            .unwrap_or_else(|| {
                                panic!(
                                    "{} is offered as a command but not in CommandHost",
                                    offer.id
                                )
                            });
                        (false, entry.blocked_by(state).is_some())
                    }
                    Offered::Generator(_) => (false, false),
                };
                (offer.id.to_string(), menu_row, rejected)
            })
            .collect()
    });
    assert!(
        rows.iter()
            .any(|&(_, menu_row, rejected)| menu_row && rejected),
        "the fixture withholds no menu row, so a live-drawn withheld row goes unmeasured"
    );
    assert!(
        rows.iter()
            .any(|&(_, menu_row, rejected)| menu_row && !rejected),
        "the fixture withholds every menu row, so a faint-drawn live row goes unmeasured"
    );
    let wrong: Vec<String> = rows
        .iter()
        .filter_map(|(id, _, rejected)| {
            let faint = visual
                .debug_bounds(intern(&format!("palette-withheld-{id}")))
                .is_some();
            (faint != *rejected).then(|| {
                let drawn = if faint { "faint" } else { "live" };
                format!("{id} is drawn {drawn} and choosing it is rejected: {rejected}")
            })
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
