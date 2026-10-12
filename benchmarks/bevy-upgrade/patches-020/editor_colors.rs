//! Full production color picker, using the same numeric and text dependencies.
include!("editor_widget_support.rs");

#[derive(bevy::prelude::Component)]
pub(crate) struct EditorNativeControl;

#[path = "../../../apps/aestra-editor/src/feathers/color_picker.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Match the shipping editor's Bevy system-parameter policy"
)]
mod color_picker;
#[path = "../../../apps/aestra-editor/src/feathers/color_picker/pointer_020.rs"]
mod color_pointer;
