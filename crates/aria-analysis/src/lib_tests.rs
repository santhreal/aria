//! Tests for the crate root: the validated time span every measurement reports.

use super::*;

#[test]
fn time_span_constructor_enforces_invariants() {
    assert!(TimeSpan::new(0.25, 0.5).is_ok());
    assert!(TimeSpan::new(f64::NAN, 1.0).is_err());
    assert!(TimeSpan::new(-0.1, 1.0).is_err());
    assert!(TimeSpan::new(1.0, 0.5).is_err());
}
