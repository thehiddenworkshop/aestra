//! Production camera navigation and viewport action execution; native rendering stays separate.
#[path = "../../../apps/aestra-editor/src/viewport/feedback.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original ECS query contract matches shipping lint policy"
)]
mod feedback;
#[path = "../../../apps/aestra-editor/src/viewport/gizmo_state.rs"]
mod gizmo_state;
#[path = "../../../apps/aestra-editor/src/viewport/host_motion.rs"]
mod host_motion;
#[path = "../../../apps/aestra-editor/src/viewport/lifecycle.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original camera ECS query contract, matching shipping lint policy"
)]
mod lifecycle;
#[path = "../../../apps/aestra-editor/src/viewport/photographic.rs"]
mod photographic;
#[path = "../../../apps/aestra-editor/src/viewport/plugin.rs"]
mod plugin;
#[path = "../../../apps/aestra-editor/src/viewport/presentation.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original viewport presentation ECS contract, matching shipping lint policy"
)]
mod presentation;
#[path = "../../../apps/aestra-editor/src/viewport/runtime_preview.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original runtime preview ECS query matches shipping lint policy"
)]
mod runtime_preview;
#[path = "../../../apps/aestra-editor/src/viewport/scene.rs"]
mod scene;
#[path = "../../../apps/aestra-editor/src/viewport/shape_gizmo.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original shape ECS query contracts match shipping policy"
)]
mod shape_gizmo;
#[path = "../../../apps/aestra-editor/src/viewport/telemetry.rs"]
#[allow(
    clippy::type_complexity,
    reason = "Original explicit ECS telemetry query matches shipping policy"
)]
mod telemetry;
#[path = "../../../apps/aestra-editor/src/viewport/tests/telemetry_candidate.rs"]
mod telemetry_observation_tests;
#[path = "../../../apps/aestra-editor/src/viewport/transform_gizmo.rs"]
#[allow(
    clippy::type_complexity,
    clippy::too_many_arguments,
    reason = "Original native ECS query contracts match shipping lint policy"
)]
mod transform_gizmo;
#[path = "../../../apps/aestra-editor/src/viewport/workspace.rs"]
mod workspace;
mod keyboard {
    pub(super) use crate::viewport_keyboard::register;
}
#[path = "../../../apps/aestra-editor/src/viewport/actions.rs"]
mod actions;
use actions::PreviewDisplayState;
#[path = "../../../apps/aestra-editor/src/viewport/control_input.rs"]
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
