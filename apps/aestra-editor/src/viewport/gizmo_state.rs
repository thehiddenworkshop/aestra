//! Original viewport shape/TRS state shared without native plugin setup.
use aestra_core::{EffectClipId, EmitterId, EmitterShape, EmitterTransform, ModuleId};
use bevy::prelude::*;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub(super) struct PreviewSceneGizmos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShapeGizmoHandle {
    Radius,
    Depth,
    ExtentX,
    ExtentY,
    ExtentZ,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ActiveShapeGizmoDrag {
    pub(super) emitter: EmitterId,
    pub(super) module: ModuleId,
    pub(super) handle: ShapeGizmoHandle,
    pub(super) original: EmitterShape,
    pub(super) current: EmitterShape,
}

#[derive(Resource, Default)]
pub(super) struct ShapeGizmoState {
    pub(super) hovered: Option<ShapeGizmoHandle>,
    pub(super) active: Option<ActiveShapeGizmoDrag>,
}

#[derive(Component)]
pub(super) struct TransformGizmoVisualRoot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreviewTransformTarget {
    Emitter(EmitterId),
    EffectClip(EffectClipId),
    HostPose(aestra_core::EffectId, usize),
    /// A module input its schema marks as a position handle (fluid F3), e.g. a fluid source's
    /// centre: the gizmo translates the point; rotation and scale do not apply.
    ModuleHandle(ModuleId, &'static str),
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ActiveEmitterTransformGizmo {
    pub(super) target: PreviewTransformTarget,
    pub(super) original: EmitterTransform,
    pub(super) current: EmitterTransform,
    pub(super) raw_anchor: EmitterTransform,
    pub(super) output_anchor: EmitterTransform,
    pub(super) precision: bool,
    pub(super) effect: aestra_core::EffectId,
    pub(super) revision: u64,
}

#[derive(Resource, Default)]
pub(crate) struct EmitterTransformGizmoInteraction {
    pub(super) active: Option<ActiveEmitterTransformGizmo>,
    pub(super) cancelled: bool,
}

impl EmitterTransformGizmoInteraction {
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

#[derive(Component)]
pub(super) struct ShapeGizmoValueLabel;

#[derive(Component)]
pub(super) struct TransformGizmoValueLabel;
