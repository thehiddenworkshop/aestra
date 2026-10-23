//! Actual color/numeric/text dependencies for production timeline popovers.
#[path = "../../../apps/aestra-editor/src/feathers/color_picker.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Production Bevy system dependencies match shipping lint policy"
)]
pub(crate) mod color_picker;
#[path = "../../../apps/aestra-editor/src/feathers/color_picker/pointer.rs"]
mod color_pointer;
#[path = "../../../apps/aestra-editor/src/feathers/number_input.rs"]
#[allow(
    dead_code,
    unused_imports,
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Property-owned numeric policy is tested by the real editor package"
)]
pub(crate) mod number_input;
#[path = "../../../apps/aestra-editor/src/feathers/text_input.rs"]
pub(crate) mod text_input;
use crate::scenes;
