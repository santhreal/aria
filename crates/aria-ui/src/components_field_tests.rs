//! A text box draws its caret where the next typed character goes, and none
//! while it holds no focus.
//!
//! The class closed here: a caret typed into a box's text. `text_box` wrote
//! `{value}|`, so the caret was a glyph of the value: an empty box that took
//! focus dropped its placeholder for a lone `|`, and a value wider than the box
//! ended in an ellipsis that cut off the caret and the end being typed.
//!
//! Each case draws one [`text_box`] in a column of its own width and reads the
//! drawn run and the caret's mark back: an empty box holding focus keeps its
//! placeholder with the caret at its start, a box with a value puts the caret
//! at the value's end, a value wider than a box holding focus gives up its
//! start to the ellipsis with the caret after its end and inside the border,
//! the same value without focus gives up its end, and a box without focus
//! draws no caret.
//!
//! Not covered: the caret's colour and the rename bar's blink, which the test
//! renderer does not read.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Render, Styled, TestAppContext,
    Window, div, px,
};

use super::text_box;
use crate::theme::Theme;

const EPS: f32 = 0.51;

/// How far past its run a text element may end: a text element rounds its
/// width up to a whole pixel, so the caret after it stands up to a pixel past
/// the run's last glyph.
const EDGE: f32 = 1.0;

const PLACEHOLDER: &str = "Search";

/// Which end of a value wider than its box gives way to the ellipsis.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Cut {
    /// The value fits.
    None,
    /// The end gives way: the box holds no focus, and reads from the start.
    End,
    /// The start gives way: the next character joins the end, which stays in
    /// view beside the caret.
    Start,
}

/// Where a case's caret stands.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Caret {
    /// No focus, no caret.
    None,
    /// At the start of the placeholder of an empty box.
    Start,
    /// After the last glyph drawn.
    End,
}

struct Case {
    value: &'static str,
    focused: bool,
    width: f32,
    caret: Caret,
    cut: Cut,
}

const LONG: &str = "a gaussian blur over the whole frame at dusk";

const CASES: [Case; 6] = [
    Case {
        value: "",
        focused: false,
        width: 200.0,
        caret: Caret::None,
        cut: Cut::None,
    },
    Case {
        value: "",
        focused: true,
        width: 200.0,
        caret: Caret::Start,
        cut: Cut::None,
    },
    Case {
        value: "blur",
        focused: false,
        width: 200.0,
        caret: Caret::None,
        cut: Cut::None,
    },
    Case {
        value: "blur",
        focused: true,
        width: 200.0,
        caret: Caret::End,
        cut: Cut::None,
    },
    Case {
        value: LONG,
        focused: false,
        width: 120.0,
        caret: Caret::None,
        cut: Cut::End,
    },
    Case {
        value: LONG,
        focused: true,
        width: 120.0,
        caret: Caret::End,
        cut: Cut::Start,
    },
];

struct Field(usize);

impl Render for Field {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A window root opens the frame its controls' motion reads.
        crate::motion::begin_frame(cx);
        let theme = Theme::of(cx);
        let case = &CASES[self.0];
        div()
            .w(px(case.width))
            .font_family(theme.font_sans.clone())
            .flex()
            .flex_col()
            .child(
                text_box(
                    ("field-case", self.0),
                    case.value,
                    PLACEHOLDER,
                    case.focused,
                    &theme,
                )
                .debug_selector(|| "field".into()),
            )
    }
}

#[gpui::test]
fn a_focused_box_draws_its_caret_where_the_next_character_goes(cx: &mut TestAppContext) {
    cx.update(|app| {
        crate::register_fonts(app);
        Theme::install(
            crate::theme::Appearance::Dark,
            Default::default(),
            None,
            app,
        );
    });
    for (index, case) in CASES.iter().enumerate() {
        let (_, visual) = crate::test_support::window_view(cx, move |_, _| Field(index));
        visual.run_until_parked();
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        let [run] = runs.as_slice() else {
            panic!(
                "case {index} drew {} runs, one expected: {:?}",
                runs.len(),
                runs.iter().map(|run| &run.text).collect::<Vec<_>>()
            );
        };
        let shown = if case.value.is_empty() {
            PLACEHOLDER
        } else {
            case.value
        };
        let kept = match case.cut {
            Cut::None => Some(run.text.as_ref()),
            Cut::End => run
                .text
                .strip_suffix('…')
                .filter(|kept| shown.starts_with(kept)),
            Cut::Start => run
                .text
                .strip_prefix('…')
                .filter(|kept| shown.ends_with(kept)),
        };
        assert!(
            kept.is_some_and(|kept| (case.cut == Cut::None) == (kept == shown)),
            "case {index} drew {:?} for {shown:?}, cut at {:?}",
            run.text,
            case.cut
        );

        let field = visual
            .debug_bounds("field")
            .unwrap_or_else(|| panic!("case {index} drew no box"));
        let caret = visual.debug_bounds("text-caret");
        let at = match (case.caret, caret) {
            (Caret::None, None) => continue,
            (Caret::None, Some(at)) => {
                panic!("case {index}: a box without focus drew a caret at {at:?}")
            }
            (_, None) => panic!("case {index}: a box holding focus drew no caret"),
            (_, Some(at)) => at,
        };
        if case.caret == Caret::Start {
            assert!(
                (at.left() - run.bounds.left()).abs() < px(EPS),
                "case {index}: the caret stands at {:?}, the placeholder starts at {:?}",
                at.left(),
                run.bounds.left()
            );
        } else {
            let gap = at.left() - run.bounds.right();
            assert!(
                gap > px(-EPS) && (case.cut != Cut::None || gap < px(EDGE)),
                "case {index}: the caret stands at {:?}, the text ends at {:?}",
                at.left(),
                run.bounds.right()
            );
        }
        // The border and the padding on either side.
        let inset = px(1.0 + Theme::SPACE_SM);
        assert!(
            at.left() >= field.left() + inset - px(EPS)
                && at.left() <= field.right() - inset + px(EPS),
            "case {index}: the caret stands at {:?}, outside the box's text area {:?}..{:?}",
            at.left(),
            field.left() + inset,
            field.right() - inset
        );
        assert!(
            at.top() >= field.top() && at.bottom() <= field.bottom(),
            "case {index}: the caret spans {:?}..{:?}, past the box's {:?}..{:?}",
            at.top(),
            at.bottom(),
            field.top(),
            field.bottom()
        );
    }
}
