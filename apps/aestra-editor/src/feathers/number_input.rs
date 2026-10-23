//! Native Feathers numeric interaction with editor semantic and visual policy.
#[path = "number_input/arrows.rs"]
pub(crate) mod arrows;
#[path = "number_input/native.rs"]
mod native;
#[path = "number_input/scrub.rs"]
mod scrub;
#[path = "number_input/widget.rs"]
mod widget;
pub(crate) use native::{DisplayNumberInput, EditPhase, NumberEdit, OptionalNumberInput};
pub(crate) use widget::{
    EditorNumberInput, NumberInputValue, UpdateNumberInput, feathers_integer_input,
    feathers_labeled_scalar_input, feathers_scalar_input, spawn_scalar_input,
};

#[cfg(test)]
pub(crate) use scrub::scrub_delta;
pub(crate) use scrub::{
    NumberScrubPlugin, NumberScrubSet, ScrubbableNumber, decimal_places, formatted, rounded,
    scrub_multiplier,
};
