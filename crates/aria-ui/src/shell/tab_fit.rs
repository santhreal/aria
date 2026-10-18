//! Which of a frame's tabs its tab row draws.
//!
//! A tab shrinks as far as its title's first letters and an ellipsis, never
//! further. When the frame's tabs do not fit at that width, the row draws as
//! many as fit, always the active one, and ends in an overflow control that
//! lists the rest.

use gpui::{App, Font, FontWeight};

use crate::theme::Theme;

/// Letters of its title a drawn tab always reads before the ellipsis.
pub(super) const TAB_LEAD: usize = 3;

/// Width of the overflow control: its row-sized hit box and a margin on each
/// side.
pub(super) const MORE_W: f32 = Theme::ICON_HIT_ROW + 2.0 * Theme::SPACE_XXS;

/// The ellipsis a truncated title ends in.
const ELLIPSIS: &str = "…";

/// The start of `title` a tab reads when it is as narrow as it goes: its first
/// [`TAB_LEAD`] characters, run on past a space or punctuation, which the
/// truncation drops before the ellipsis, so the letters before the ellipsis
/// still number [`TAB_LEAD`]. The whole title when it is that short.
pub(super) fn tab_lead(title: &str) -> &str {
    let trimmed = |ch: char| ch.is_whitespace() || ch.is_ascii_punctuation();
    for (taken, (at, ch)) in title.char_indices().enumerate() {
        if taken + 1 >= TAB_LEAD && !trimmed(ch) {
            return &title[..at + ch.len_utf8()];
        }
    }
    title
}

/// The UI font tab titles draw in.
pub(super) fn tab_font(theme: &Theme) -> Font {
    theme.ui_font(FontWeight::NORMAL)
}

/// The drawn width of `text` in the tab row's font.
pub(super) fn label_width(text: &str, font: &Font, cx: &App) -> f32 {
    f32::from(
        cx.text_system()
            .advance_for_text(text, font, Theme::TEXT_UI),
    )
}

/// The narrowest a tab's title box is drawn: its [`tab_lead`] with an
/// ellipsis, or the whole title when that is shorter. One pixel over, so the
/// truncation, which compares summed advances against the box, keeps the last
/// lead letter.
pub(super) fn lead_width(title: &str, font: &Font, cx: &App) -> f32 {
    let lead = tab_lead(title);
    let text = if lead.len() == title.len() {
        label_width(title, font, cx)
    } else {
        label_width(lead, font, cx) + label_width(ELLIPSIS, font, cx)
    };
    text + 1.0
}

/// The narrowest a tab titled `title` is drawn: its padding `pad` on each
/// side, the `gap` before its close (which has given up its width by then),
/// its right border, and its [`lead_width`].
pub(super) fn least_width(title: &str, pad: f32, gap: f32, font: &Font, cx: &App) -> f32 {
    (2.0 * pad + gap + 1.0 + lead_width(title, font, cx) - 1.0).ceil() + 1.0
}

/// The width a tab titled `title` is drawn at with room to spare: its
/// padding, the `gap`, its right border, its whole title and its `close`.
pub(super) fn whole_width(
    title: &str,
    pad: f32,
    gap: f32,
    close: f32,
    font: &Font,
    cx: &App,
) -> f32 {
    (2.0 * pad + gap + 1.0 + label_width(title, font, cx) + close).ceil()
}

/// Which tabs of a row `avail` px wide are drawn, given each tab's least width
/// and the index of the active tab.
///
/// Every tab is drawn when their least widths fit. Otherwise the overflow
/// control takes [`MORE_W`] and the row draws the active tab and then as many
/// of the others as fit, narrowest first, so the count drawn is the largest
/// the width allows. A row too narrow for even the active tab draws it alone.
pub(super) fn fit(avail: f32, least: &[f32], active: usize) -> Vec<bool> {
    if least.iter().sum::<f32>() <= avail {
        return vec![true; least.len()];
    }
    let mut drawn = vec![false; least.len()];
    let Some(&own) = least.get(active) else {
        return drawn;
    };
    drawn[active] = true;
    let mut room = avail - MORE_W - own;
    let mut rest: Vec<usize> = (0..least.len()).filter(|&at| at != active).collect();
    rest.sort_by(|&a, &b| least[a].total_cmp(&least[b]).then(a.cmp(&b)));
    for at in rest {
        if least[at] > room {
            break;
        }
        room -= least[at];
        drawn[at] = true;
    }
    drawn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lead_keeps_three_letters_past_the_truncation_trim() {
        assert_eq!(tab_lead("Effect Controls"), "Eff");
        assert_eq!(tab_lead("Info"), "Inf");
        assert_eq!(tab_lead("Mix"), "Mix");
        assert_eq!(tab_lead("AI Assistant"), "AI A");
        assert_eq!(tab_lead("A/B Compare"), "A/B");
        assert_eq!(tab_lead("Ab"), "Ab");
    }

    #[test]
    fn the_row_draws_the_active_tab_and_the_most_that_fit() {
        let least = [50.0, 30.0, 70.0, 30.0, 40.0];
        // Room for all: every tab.
        assert_eq!(fit(220.0, &least, 2), vec![true; 5]);
        // 219 px: the overflow control takes its width; the active tab, then
        // the narrowest: 70 + 30 + 30 + 40 = 170 <= 219 - 24.
        assert_eq!(fit(219.0, &least, 2), vec![false, true, true, true, true]);
        // Too narrow for anything but the active tab.
        assert_eq!(fit(80.0, &least, 2), vec![false, false, true, false, false]);
        assert_eq!(fit(10.0, &least, 0), vec![true, false, false, false, false]);
    }
}
