//! Shared policy for Blender-style numeric scrubbing.
//!
//! Domain-specific value lookup and semantic command commits stay with the owning panel.

pub(crate) fn scrub_multiplier(shift: bool, control: bool) -> f32 {
    if shift {
        0.1
    } else if control {
        10.0
    } else {
        1.0
    }
}

pub(crate) fn scrub_delta(pixel_delta: f32, step: f32, multiplier: f32) -> f32 {
    pixel_delta * step * multiplier / 8.0
}

pub(crate) fn decimal_places(effective_step: f32) -> usize {
    let effective_step = effective_step.abs();
    if effective_step >= 1.0 {
        0
    } else if effective_step >= 0.1 {
        1
    } else if effective_step >= 0.01 {
        2
    } else if effective_step >= 0.001 {
        3
    } else {
        4
    }
}

pub(crate) fn rounded(value: f32, precision: usize) -> f32 {
    let factor = 10.0_f32.powi(precision as i32);
    (value * factor).round() / factor
}

pub(crate) fn formatted(value: f32, precision: usize) -> String {
    let zero_threshold = 0.5 * 10.0_f32.powi(-(precision as i32));
    let value = if value.abs() < zero_threshold {
        0.0
    } else {
        value
    };
    let mut formatted = format!("{value:.precision$}");
    if formatted.contains('.') {
        while formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
    }
    formatted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_change_scrub_precision() {
        assert_eq!(scrub_multiplier(false, false), 1.0);
        assert_eq!(scrub_multiplier(true, false), 0.1);
        assert_eq!(scrub_multiplier(false, true), 10.0);
        assert_eq!(decimal_places(0.005), 3);
        assert_eq!(formatted(1.2301, 2), "1.23");
    }
}
