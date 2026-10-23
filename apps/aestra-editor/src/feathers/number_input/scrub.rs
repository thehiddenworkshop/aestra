//! Numeric policy metadata and formatting; interaction is owned by Feathers.
use bevy::prelude::*;

pub(crate) struct NumberScrubPlugin;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NumberScrubSet;

impl Plugin for NumberScrubPlugin {
    fn build(&self, app: &mut App) {
        super::native::register(app);
    }
}

/// Compatibility metadata for panel-owned limits/steps, mapped onto native controls.
/// This component installs no pointer observers.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrubbableNumber {
    pub(crate) value: f32,
    pub(crate) min: f32,
    pub(crate) max: f32,
    pub(crate) step: f32,
}

impl ScrubbableNumber {
    pub(crate) fn new(value: f32, min: f32, max: f32, step: f32) -> Self {
        Self {
            value,
            min,
            max,
            step,
        }
    }

    #[cfg(test)]
    pub(super) fn normalize(self, value: f32, multiplier: f32) -> f32 {
        rounded(
            value.clamp(self.min, self.max),
            decimal_places(self.step * multiplier),
        )
    }
}

pub(crate) fn scrub_multiplier(shift: bool, control: bool) -> f32 {
    if shift {
        0.1
    } else if control {
        10.0
    } else {
        1.0
    }
}

#[cfg(test)]
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
#[path = "tests/scrub.rs"]
mod tests;
