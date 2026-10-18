//! Actual color/numeric/text dependencies for production timeline popovers.
#[path = "../../../apps/aestra-editor/src/feathers/number_input/arrows.rs"]
mod arrows;
#[path = "../../../apps/aestra-editor/src/feathers/number_input/pointer_020.rs"]
mod pointer;
#[path = "../../../apps/aestra-editor/src/feathers/number_input/scrub.rs"]
mod scrub;
#[path = "../../../apps/aestra-editor/src/feathers/number_input/widget.rs"]
mod widget;
use scrub::{ScrubbableNumber, decimal_places, formatted, replace_number_text, scrub_multiplier};
#[path = "../../../apps/aestra-editor/src/feathers/color_picker.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production Bevy system dependencies match shipping lint policy"
)]
pub(crate) mod color_picker;
#[path = "../../../apps/aestra-editor/src/feathers/color_picker/pointer_020.rs"]
mod color_pointer;
#[path = "../../../apps/aestra-editor/src/feathers/text_input.rs"]
pub(crate) mod text_input;
use crate::scenes;
pub(crate) mod number_input {
    pub(crate) use super::scrub::{NumberScrubPlugin, ScrubbableNumber, rounded};
    pub(crate) use super::widget::{NumberInputValue, UpdateNumberInput, feathers_scalar_input};
}
