//! Which of a frame's tabs its tab row draws, and how wide.
//!
//! A tab shrinks as far as its title's first letters and an ellipsis, never
//! further. When the frame's tabs do not fit at that width, the row draws as
//! many as fit, always the active one, and ends in an overflow control that
//! lists the rest. A row short of room for its titles whole keeps the front
//! tab's title whole while the others fit, and narrows its widest tabs first.

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

/// The width a tab titled `title` is drawn at with its whole title and its
/// close gone: its padding, the `gap`, its right border and its title.
pub(super) fn whole_width(title: &str, pad: f32, gap: f32, font: &Font, cx: &App) -> f32 {
    (2.0 * pad + gap + 1.0 + label_width(title, font, cx)).ceil()
}

/// The width each tab of a row `room` px wide is drawn at when the row has no
/// room for every title whole, given each tab's [`whole_width`] and its
/// [`least_width`], and the slot of the front tab.
///
/// The front tab takes the room the others leave at their least widths, up to
/// its whole width, and never less than sharing would give it; the others
/// share what is left. Sharing, the widest tabs narrow first, to one shared
/// width in whole pixels, the widest at which the row fits: a short title
/// stays whole while a longer one takes the ellipsis. A tab narrows no further
/// than its least width, so a row too narrow for every least width draws each
/// at its least.
pub(super) fn squeeze(room: f32, whole: &[f32], least: &[f32], front: Option<usize>) -> Vec<f32> {
    let shared = share(room, whole, least);
    let Some(front) = front.filter(|&at| at < whole.len()) else {
        return shared;
    };
    let others = least.iter().sum::<f32>() - least[front];
    let own = whole[front].min((room - others).floor());
    if own <= shared[front] {
        return shared;
    }
    let mut rest_whole = whole.to_vec();
    let mut rest_least = least.to_vec();
    rest_whole.remove(front);
    rest_least.remove(front);
    let mut widths = share(room - own, &rest_whole, &rest_least);
    widths.insert(front, own);
    widths
}

/// Every tab of `whole` narrowed to one shared width in whole pixels, the
/// widest at which the row fits in `room`, and no tab under its `least`.
fn share(room: f32, whole: &[f32], least: &[f32]) -> Vec<f32> {
    let widths = |cap: f32| {
        whole
            .iter()
            .zip(least)
            .map(move |(&full, &floor)| full.min(cap).max(floor))
    };
    let widest = whole.iter().copied().fold(0.0, f32::max).ceil();
    // `cap` rises and `over` falls until they meet: the row fits at `cap`, or
    // `cap` is 0, and does not fit at `over`, or `over` is past every tab.
    let (mut cap, mut over) = (0.0_f32, widest + 1.0);
    while over - cap > 1.0 {
        let mid = ((cap + over) / 2.0).floor();
        if widths(mid).sum::<f32>() <= room {
            cap = mid;
        } else {
            over = mid;
        }
    }
    widths(cap).collect()
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

    #[test]
    fn a_squeezed_row_narrows_its_widest_tabs_first() {
        let whole = [120.0, 50.0, 45.0, 75.0];
        let least = [40.0, 40.0, 38.0, 42.0];
        // Room for every title whole: each tab at its whole width.
        assert_eq!(squeeze(290.0, &whole, &least, None), whole.to_vec());
        // The widest narrows alone until it meets the next.
        assert_eq!(
            squeeze(260.0, &whole, &least, None),
            vec![90.0, 50.0, 45.0, 75.0]
        );
        // Then the two widest share one width; the short titles stay whole.
        assert_eq!(
            squeeze(200.0, &whole, &least, None),
            vec![52.0, 50.0, 45.0, 52.0]
        );
        // A tab held at its least width stands wider than the shared width.
        assert_eq!(
            squeeze(190.0, &whole, &[40.0, 40.0, 38.0, 60.0], None),
            vec![43.0, 43.0, 43.0, 60.0]
        );
        // Too narrow for every least width: each at its least.
        assert_eq!(squeeze(100.0, &whole, &least, None), least.to_vec());
        assert_eq!(squeeze(0.0, &[], &[], None), Vec::<f32>::new());
    }

    #[test]
    fn a_squeezed_row_keeps_the_front_title_whole_while_the_others_fit() {
        let whole = [120.0, 50.0, 45.0, 75.0];
        let least = [40.0, 40.0, 38.0, 42.0];
        // The front tab whole, the others sharing the 140 px left.
        assert_eq!(
            squeeze(260.0, &whole, &least, Some(0)),
            vec![120.0, 47.0, 45.0, 47.0]
        );
        // 120 + 40 + 38 + 42 = 240: the others at their least.
        assert_eq!(
            squeeze(240.0, &whole, &least, Some(0)),
            vec![120.0, 40.0, 38.0, 42.0]
        );
        // One pixel short: the front tab gives up that pixel, the others stay
        // at their least.
        assert_eq!(
            squeeze(239.0, &whole, &least, Some(0)),
            vec![119.0, 40.0, 38.0, 42.0]
        );
        // At every least width the front tab gains nothing on sharing.
        assert_eq!(squeeze(160.0, &whole, &least, Some(0)), least.to_vec());
        // A short front title is whole either way.
        assert_eq!(
            squeeze(200.0, &whole, &least, Some(1)),
            vec![52.0, 50.0, 45.0, 52.0]
        );
    }

    #[test]
    fn a_squeezed_row_fills_its_room_in_whole_pixels() {
        let whole = [131.0, 64.0, 58.0, 97.0, 64.0];
        let least = [44.0, 41.0, 43.0, 45.0, 41.0];
        let floor: f32 = least.iter().sum();
        let top: f32 = whole.iter().sum();
        let fronts = std::iter::once(None).chain((0..whole.len()).map(Some));
        for front in fronts {
            let mut room = floor;
            while room <= top {
                let widths = squeeze(room, &whole, &least, front);
                let used: f32 = widths.iter().sum();
                let what = format!("{room} px, front {front:?}");
                assert!(used <= room, "{what}: the row draws {used} px");
                // One pixel more on the shared width would not fit.
                assert!(
                    used + whole.len() as f32 > room,
                    "{what}: the row leaves {} px unused",
                    room - used
                );
                if let Some(front) = front {
                    let others = floor - least[front];
                    let shared = squeeze(room, &whole, &least, None);
                    assert!(
                        whole[front] + others > room || widths[front] == whole[front],
                        "{what}: the front tab at {} px with room for its whole {} px",
                        widths[front],
                        whole[front]
                    );
                    assert!(
                        widths[front] >= shared[front],
                        "{what}: the front tab at {} px, under its shared {} px",
                        widths[front],
                        shared[front]
                    );
                    let others_at_least =
                        (0..whole.len()).all(|at| at == front || widths[at] == least[at]);
                    assert!(
                        widths[front] == shared[front]
                            || widths[front] == whole[front]
                            || others_at_least,
                        "{what}: the front tab is cut to {} px while another tab is over its \
                         least: {widths:?}",
                        widths[front]
                    );
                }
                for (at, width) in widths.iter().enumerate() {
                    assert!(
                        (least[at]..=whole[at]).contains(width) && width.fract() == 0.0,
                        "{what}: tab {at} at {width} px"
                    );
                    // A narrowed tab is the widest in its row, but for a tab
                    // held at its least width and the front tab.
                    for (other, &beside) in widths.iter().enumerate() {
                        assert!(
                            *width == whole[at]
                                || beside <= *width
                                || beside == least[other]
                                || front == Some(other),
                            "{what}: tab {at} at {width} px beside tab {other} at {beside} px"
                        );
                    }
                }
                room += 0.5;
            }
        }
    }
}
