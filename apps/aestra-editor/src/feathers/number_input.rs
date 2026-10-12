//! Shared numeric scrubbing and editor step arrows.
pub(crate) mod arrows;
#[path = "number_input/pointer_019.rs"]
mod pointer;
mod scrub;
mod widget;
pub(crate) use widget::{
    EditorNumberInput, NumberInputValue, UpdateNumberInput, feathers_integer_input,
    feathers_labeled_scalar_input, feathers_scalar_input,
};

use scrub::replace_number_text;
pub(crate) use scrub::{
    NumberScrubPlugin, NumberScrubSet, ScrubbableNumber, decimal_places, formatted, rounded,
    scrub_delta, scrub_multiplier,
};
