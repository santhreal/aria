//! An error on the status strip reads at body contrast and marks the strip.
//!
//! WHY: the strip drew an error in `danger` over a ground of `danger_muted`.
//! Both are reds: in the light look `danger_muted` is the darker one, so
//! "Select a clip to analyze" stood red on red at 1.3:1, and in the dark look
//! it is a light pink under a mid red. The class closed here, in both looks:
//! an error whose ink falls under WCAG AA on its ground, and an error ground
//! the plain strip draws alike, so nothing marks the message as an error.
//!
//! Not covered: a status line, which is a receipt drawn in `text_faint` and
//! held to the theme's hint tier by `theme_tests`; a theme file that sets its
//! own danger hue, which installs through the same tokens but is not
//! enumerated here.

use super::note_colors;
use crate::theme::{Theme, contrast_ratio};

#[test]
fn an_error_on_the_status_strip_reads_on_a_ground_of_its_own() {
    for theme in [Theme::dark(), Theme::light()] {
        let (plain, _) = note_colors(&theme, false);
        let (ground, ink) = note_colors(&theme, true);
        let reads = contrast_ratio(ink, ground);
        assert!(
            reads >= 4.5,
            "{:?}: an error's ink stands {reads:.2}:1 on its ground, under WCAG AA (4.5:1)",
            theme.appearance
        );
        let marked = contrast_ratio(ground, plain);
        assert!(
            marked >= 1.2,
            "{:?}: an error's ground stands {marked:.2}:1 off the plain strip, which draws it alike",
            theme.appearance
        );
    }
}
