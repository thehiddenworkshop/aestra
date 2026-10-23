//! Production Curves plugin, controls, keyboard and complete panel composition.
pub(crate) use crate::curves_action::CurvesAction;
pub(super) use crate::curves_state as state;
#[path = "../../../apps/aestra-editor/src/curves/host_install.rs"]
mod host_install;
mod host_motion {
    pub(super) use super::host_install::install;
}
#[path = "../../../apps/aestra-editor/src/curves/plugin.rs"]
mod plugin;
use plugin::EditorCurvesPlugin;
#[path = "../../../apps/aestra-editor/src/curves/composition.rs"]
#[allow(
    clippy::too_many_arguments,
    reason = "Original production graph constructor's explicit resource contract"
)]
mod composition;
#[path = "../../../apps/aestra-editor/src/curves/control_input.rs"]
mod control_input;
#[path = "../../../apps/aestra-editor/src/curves/controls.rs"]
mod controls;
use crate::timeline::panel_input as cursor;
#[path = "../../../apps/aestra-editor/src/curves/host_composition.rs"]
mod host_composition;
#[path = "../../../apps/aestra-editor/src/curves/host_editing.rs"]
mod host_editing;
#[path = "../../../apps/aestra-editor/src/curves/keyboard.rs"]
mod keyboard;
use crate::timeline::panel_input as pointer;
#[path = "../../../apps/aestra-editor/src/curves/property_editing.rs"]
mod property_editing;
