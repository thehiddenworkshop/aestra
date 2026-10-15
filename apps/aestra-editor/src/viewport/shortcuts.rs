//! Actual viewport action identities and canvas ownership, shared with compatibility gates.
use bevy::{gizmos::transform_gizmo::TransformGizmoMode, prelude::*};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ViewportSet {
    Setup,
    Input,
    Actions,
    Update,
}

#[derive(Component, Event, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewportAction {
    ToggleGrid,
    FramePreview,
    SetTransformGizmoMode(TransformGizmoMode),
    SetPreviewDisplayMode(PreviewDisplayMode),
    /// Enlarge the viewport to fill the whole editor, or restore it to its docked place.
    ToggleMaximize,
}

#[derive(Component)]
pub(crate) struct PreviewCanvas;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PreviewDisplayMode {
    Wireframe,
    #[default]
    Rendered,
}
