//! Pure application step/format policy; real scrubbing is qualified in widget_tests.
use super::*;

#[test]
fn modifiers_keep_shift_precedence_and_decimal_formatting() {
    assert_eq!(scrub_multiplier(false, false), 1.0);
    assert_eq!(scrub_multiplier(true, false), 0.1);
    assert_eq!(scrub_multiplier(false, true), 10.0);
    assert_eq!(scrub_multiplier(true, true), 0.1);
    assert_eq!(decimal_places(0.005), 3);
    assert_eq!(formatted(1.2301, 2), "1.23");
    assert_eq!(formatted(-0.00001, 2), "0");
}

#[test]
fn numeric_policy_clamps_and_uses_modifier_precision() {
    let input = ScrubbableNumber::new(0.5, 0.0, 1.0, 0.01);
    assert_eq!(input.normalize(1.5, 1.0), 1.0);
    assert_eq!(input.normalize(-0.5, 1.0), 0.0);
    assert_eq!(input.normalize(0.123_6, 1.0), 0.12);
    assert_eq!(input.normalize(0.123_6, 0.1), 0.124);
}
