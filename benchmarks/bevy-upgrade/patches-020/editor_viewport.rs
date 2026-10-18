//! Production camera navigation and viewport action execution; native rendering stays separate.
#[path = "../../../apps/aestra-editor/src/viewport/feedback.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original ECS query contract matches shipping lint policy"
)]
mod feedback;
#[path = "../../../apps/aestra-editor/src/viewport/lifecycle.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original camera ECS query contract, matching shipping lint policy"
)]
mod lifecycle;
#[path = "../../../apps/aestra-editor/src/viewport/photographic.rs"]
mod photographic;
#[path = "../../../apps/aestra-editor/src/viewport/presentation.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original viewport presentation ECS contract, matching shipping lint policy"
)]
mod presentation;
#[path = "../../../apps/aestra-editor/src/viewport/runtime_preview.rs"]
#[allow(
    dead_code,
    clippy::type_complexity,
    reason = "Remaining native runtime consumers stay separate; original ECS query matches shipping lint policy"
)]
mod runtime_preview;
mod keyboard {
    pub(super) use crate::viewport_keyboard::register;
}
#[path = "../../../apps/aestra-editor/src/viewport/actions.rs"]
mod actions;
#[path = "../../../apps/aestra-editor/src/viewport/control_input_020.rs"]
mod control_input;
pub(crate) use crate::viewport_shortcuts::PreviewDisplayMode;
use navigation::PreviewCameraController;
mod shortcuts {
    pub(super) use crate::viewport_shortcuts::PreviewCanvas;
}
#[path = "../../../apps/aestra-editor/src/viewport/navigation.rs"]
#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Original production navigation system's explicit ECS contract, matching shipping lint policy"
)]
mod navigation;
pub(crate) use crate::viewport_shortcuts::{PreviewCanvas, ViewportAction, ViewportSet};
