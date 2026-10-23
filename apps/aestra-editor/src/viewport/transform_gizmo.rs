//! Actual transform selection, transactional drag previews, feedback and readout consumers.
use super::{
    gizmo_state::*, host_motion, navigation::EmitterTransformGizmoProxy, shortcuts::PreviewCanvas,
};
use crate::{session::EditorSession, timeline::TimelineState};
use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_core::{EmitterTransform, ModuleId};
use bevy::{
    camera::visibility::RenderLayers,
    gizmos::transform_gizmo::{
        TransformGizmoAxis, TransformGizmoFocus, TransformGizmoMode, TransformGizmoSettings,
        TransformGizmoSpace, TransformGizmoState,
    },
    prelude::*,
    window::PrimaryWindow,
};

pub(super) const PREVIEW_TRANSFORM_GIZMO_SCREEN_SCALE: f32 = 0.14;
pub(super) const PREVIEW_TRANSFORM_GIZMO_HIT_DISTANCE: f32 = 42.0;
pub(super) const PREVIEW_TRANSFORM_TRANSLATE_SNAP: f32 = 1.0;
pub(super) const PREVIEW_TRANSFORM_ROTATE_SNAP: f32 = std::f32::consts::PI / 12.0;
pub(super) const PREVIEW_TRANSFORM_SCALE_SNAP: f32 = 0.1;
pub(super) const PREVIEW_TRANSFORM_PRECISION_FACTOR: f32 = 0.1;

pub(super) fn preview_transform_gizmo_settings() -> TransformGizmoSettings {
    TransformGizmoSettings {
        screen_scale_factor: PREVIEW_TRANSFORM_GIZMO_SCREEN_SCALE,
        axis_hit_distance: PREVIEW_TRANSFORM_GIZMO_HIT_DISTANCE,
        ..default()
    }
}

pub(super) fn configure_transform_gizmo_overlay_materials(
    mut commands: Commands,
    gizmo_meshes: Query<(&RenderLayers, &MeshMaterial3d<StandardMaterial>, &ChildOf)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, material, parent) in &gizmo_meshes {
        if layers != &gizmo_layers {
            continue;
        }
        commands
            .entity(parent.parent())
            .insert(TransformGizmoVisualRoot);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.alpha_mode = AlphaMode::Blend;
            material.depth_bias = 10_000.0;
        }
    }
}

pub(super) fn sync_transform_gizmo_focus(
    mut commands: Commands,
    protection: Option<Res<crate::persistence::DocumentProtectionState>>,
    session: Res<EditorSession>,
    shape_gizmo: Res<ShapeGizmoState>,
    timeline: Option<Res<TimelineState>>,
    motion: Option<Res<host_motion::HostMotionViewport>>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    proxies: Query<(Entity, Has<TransformGizmoFocus>), With<EmitterTransformGizmoProxy>>,
    modules: Option<Res<crate::properties::EditorModuleRegistry>>,
) {
    let target = selected_gizmo_transform(
        &session,
        timeline.as_deref(),
        modules.as_deref().map(|modules| &modules.0),
    );
    let allowed = target.is_some()
        && !protection.as_ref().is_some_and(|state| state.is_open())
        && !interaction.cancelled
        && motion
            .as_ref()
            .is_none_or(|motion| motion.hovered.is_none())
        && timeline
            .as_ref()
            .is_none_or(|timeline| !crate::timeline::host_motion::busy(timeline))
        && session.pending_change.is_none()
        && shape_gizmo.hovered.is_none()
        && shape_gizmo.active.is_none()
        && !session
            .locks
            .is_locked(SemanticTarget::Effect(session.effect.id))
        && !session.locks.is_locked(target.map_or(
            SemanticTarget::Effect(session.effect.id),
            |(target, _)| match target {
                PreviewTransformTarget::Emitter(id) => SemanticTarget::Emitter(id),
                PreviewTransformTarget::EffectClip(id) => SemanticTarget::EffectClip(id),
                PreviewTransformTarget::HostPose(id, _) => SemanticTarget::Effect(id),
                PreviewTransformTarget::ModuleHandle(id, _) => SemanticTarget::Module(id),
            },
        ));
    for (entity, has_focus) in &proxies {
        if allowed && !has_focus {
            commands.entity(entity).insert(TransformGizmoFocus);
        } else if !allowed && has_focus {
            commands.entity(entity).remove::<TransformGizmoFocus>();
        }
    }
}

pub(super) fn sync_emitter_transform_proxy(
    session: Res<EditorSession>,
    timeline: Option<Res<TimelineState>>,
    gizmo: Res<TransformGizmoState>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    mut proxies: Query<&mut Transform, With<EmitterTransformGizmoProxy>>,
    modules: Option<Res<crate::properties::EditorModuleRegistry>>,
) {
    if gizmo.active || interaction.active.is_some() {
        return;
    }
    let Some((_, authored)) = selected_gizmo_transform(
        &session,
        timeline.as_deref(),
        modules.as_deref().map(|modules| &modules.0),
    ) else {
        return;
    };
    let desired = bevy_transform_from_emitter(authored);
    for mut transform in &mut proxies {
        if *transform != desired {
            *transform = desired;
        }
    }
}

pub(super) fn update_emitter_transform_gizmo(
    gizmo: Res<TransformGizmoState>,
    settings: Res<TransformGizmoSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    mut interaction: ResMut<EmitterTransformGizmoInteraction>,
    mut proxies: Query<&mut Transform, With<EmitterTransformGizmoProxy>>,
    mut session: ResMut<EditorSession>,
    timeline: Option<Res<TimelineState>>,
    modules: Option<Res<crate::properties::EditorModuleRegistry>>,
) {
    let Ok(mut transform) = proxies.single_mut() else {
        return;
    };
    let raw_current = emitter_transform_from_bevy(&transform);
    let selected = selected_gizmo_transform(
        &session,
        timeline.as_deref(),
        modules.as_deref().map(|modules| &modules.0),
    );
    if interaction.cancelled {
        if let Some((_, pose)) = selected {
            *transform = bevy_transform_from_emitter(pose);
        }
        if !gizmo.active {
            interaction.cancelled = false;
        }
        return;
    }
    if let Some(active) = interaction.active
        && (keys.just_pressed(KeyCode::Escape)
            || active.effect != session.effect.id
            || active.revision != session.document_revision()
            || selected.map(|(target, _)| target) != Some(active.target)
            || session.pending_change.is_some()
            || session
                .locks
                .is_locked(SemanticTarget::Effect(session.effect.id)))
    {
        interaction.active = None;
        interaction.cancelled = gizmo.active;
        session.restore_interaction_preview();
        if let Some((_, pose)) = selected {
            *transform = bevy_transform_from_emitter(pose);
        }
        return;
    }
    if gizmo.active {
        let Some((target, authored)) = selected else {
            return;
        };
        let precision = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let active = interaction
            .active
            .get_or_insert(ActiveEmitterTransformGizmo {
                target,
                original: authored,
                current: authored,
                raw_anchor: authored,
                output_anchor: authored,
                precision,
                effect: session.effect.id,
                revision: session.document_revision(),
            });
        if active.precision != precision {
            active.raw_anchor = raw_current;
            active.output_anchor = active.current;
            active.precision = precision;
        }
        let current = transform_with_drag_precision(
            settings.mode,
            active.raw_anchor,
            active.output_anchor,
            raw_current,
            if precision {
                PREVIEW_TRANSFORM_PRECISION_FACTOR
            } else {
                1.0
            },
        );
        let snapping = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
        let current = if snapping {
            snap_transform_absolute(settings.mode, gizmo.axis, current, precision)
        } else {
            current
        };
        *transform = bevy_transform_from_emitter(current);
        if active.current != current {
            if let Some(command) = preview_transform_command(active.target, current, &session)
                && session
                    .preview_interaction(EffectTransaction::single("Preview transform", command))
            {
                active.current = current;
            } else {
                *transform = bevy_transform_from_emitter(active.current);
            }
        }
        return;
    }

    let Some(active) = interaction.active.take() else {
        return;
    };
    if active.current != active.original
        && let Some(command) = preview_transform_command(active.target, active.current, &session)
    {
        if !session.execute(
            match active.target {
                PreviewTransformTarget::Emitter(_) => "Transformed emitter",
                PreviewTransformTarget::EffectClip(_) => "Transformed effect clip",
                PreviewTransformTarget::HostPose(_, _) => "Transformed host pose",
                PreviewTransformTarget::ModuleHandle(_, _) => "Moved module position",
            },
            command,
            true,
        ) {
            session.restore_interaction_preview();
        }
    } else {
        session.restore_interaction_preview();
    }
}

pub(super) fn transform_with_drag_precision(
    mode: TransformGizmoMode,
    raw_anchor: EmitterTransform,
    output_anchor: EmitterTransform,
    raw_current: EmitterTransform,
    factor: f32,
) -> EmitterTransform {
    let mut output = output_anchor;
    match mode {
        TransformGizmoMode::Translate => {
            let raw_anchor = Vec3::from_array(raw_anchor.translation);
            let raw_current = Vec3::from_array(raw_current.translation);
            output.translation = (Vec3::from_array(output_anchor.translation)
                + (raw_current - raw_anchor) * factor)
                .to_array();
        }
        TransformGizmoMode::Rotate => {
            let raw_anchor = Quat::from_array(raw_anchor.rotation).normalize();
            let raw_current = Quat::from_array(raw_current.rotation).normalize();
            let output_anchor = Quat::from_array(output_anchor.rotation).normalize();
            let raw_delta = (raw_current * raw_anchor.inverse()).normalize();
            output.rotation = (Quat::IDENTITY.slerp(raw_delta, factor) * output_anchor)
                .normalize()
                .to_array();
        }
        TransformGizmoMode::Scale => {
            let raw_anchor = Vec3::from_array(raw_anchor.scale);
            let raw_current = Vec3::from_array(raw_current.scale);
            output.scale = (Vec3::from_array(output_anchor.scale)
                + (raw_current - raw_anchor) * factor)
                .max(Vec3::splat(0.001))
                .to_array();
        }
    }
    output
}

pub(super) fn snap_transform_absolute(
    mode: TransformGizmoMode,
    axis: Option<TransformGizmoAxis>,
    mut transform: EmitterTransform,
    precision: bool,
) -> EmitterTransform {
    let precision_factor = if precision {
        PREVIEW_TRANSFORM_PRECISION_FACTOR
    } else {
        1.0
    };
    let snap_selected = |values: &mut Vec3, step: f32| {
        let step = step * precision_factor;
        let snap = |value: f32| (value / step).round() * step;
        match axis {
            Some(TransformGizmoAxis::X) => values.x = snap(values.x),
            Some(TransformGizmoAxis::Y) => values.y = snap(values.y),
            Some(TransformGizmoAxis::Z) => values.z = snap(values.z),
            Some(TransformGizmoAxis::View) | None => {
                *values = Vec3::new(snap(values.x), snap(values.y), snap(values.z));
            }
        }
    };
    match mode {
        TransformGizmoMode::Translate => {
            let mut translation = Vec3::from_array(transform.translation);
            snap_selected(&mut translation, PREVIEW_TRANSFORM_TRANSLATE_SNAP);
            transform.translation = translation.to_array();
        }
        TransformGizmoMode::Rotate => {
            let (x, y, z) = Quat::from_array(transform.rotation)
                .normalize()
                .to_euler(EulerRot::XYZ);
            let mut rotation = Vec3::new(x, y, z);
            snap_selected(&mut rotation, PREVIEW_TRANSFORM_ROTATE_SNAP);
            transform.rotation =
                Quat::from_euler(EulerRot::XYZ, rotation.x, rotation.y, rotation.z).to_array();
        }
        TransformGizmoMode::Scale => {
            let mut scale = Vec3::from_array(transform.scale);
            snap_selected(&mut scale, PREVIEW_TRANSFORM_SCALE_SNAP);
            transform.scale = scale.max(Vec3::splat(0.001)).to_array();
        }
    }
    transform
}

pub(super) fn format_transform_value(value: f32, decimals: usize, sign: bool) -> String {
    if sign {
        format!("{value:+.*}", decimals)
    } else {
        format!("{value:.*}", decimals)
    }
}

pub(super) fn transform_gizmo_value_text(
    mode: TransformGizmoMode,
    axis: Option<TransformGizmoAxis>,
    current: EmitterTransform,
    snapping: bool,
    precision: bool,
) -> String {
    let axis_name = match axis {
        Some(TransformGizmoAxis::X) => "X",
        Some(TransformGizmoAxis::Y) => "Y",
        Some(TransformGizmoAxis::Z) => "Z",
        Some(TransformGizmoAxis::View) | None => "XYZ",
    };
    let mut value = match mode {
        TransformGizmoMode::Translate => {
            let position = Vec3::from_array(current.translation);
            let decimals = match (snapping, precision) {
                (true, false) => 0,
                (true, true) => 1,
                (false, true) => 4,
                (false, false) => 2,
            };
            match axis {
                Some(TransformGizmoAxis::X) => {
                    format!(
                        "MOVE X  {}",
                        format_transform_value(position.x, decimals, true)
                    )
                }
                Some(TransformGizmoAxis::Y) => {
                    format!(
                        "MOVE Y  {}",
                        format_transform_value(position.y, decimals, true)
                    )
                }
                Some(TransformGizmoAxis::Z) => {
                    format!(
                        "MOVE Z  {}",
                        format_transform_value(position.z, decimals, true)
                    )
                }
                Some(TransformGizmoAxis::View) | None => format!(
                    "MOVE XYZ  {}  {}  {}",
                    format_transform_value(position.x, decimals, true),
                    format_transform_value(position.y, decimals, true),
                    format_transform_value(position.z, decimals, true),
                ),
            }
        }
        TransformGizmoMode::Rotate => {
            let (x, y, z) = Quat::from_array(current.rotation)
                .normalize()
                .to_euler(EulerRot::XYZ);
            let degrees = Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI);
            let decimals = match (snapping, precision) {
                (true, false) => 0,
                (true, true) => 1,
                (false, true) => 3,
                (false, false) => 1,
            };
            let angle = match axis {
                Some(TransformGizmoAxis::X) => degrees.x,
                Some(TransformGizmoAxis::Y) => degrees.y,
                Some(TransformGizmoAxis::Z) => degrees.z,
                Some(TransformGizmoAxis::View) | None => degrees.length(),
            };
            format!(
                "ROTATE {axis_name}  {}\u{00b0}",
                format_transform_value(angle, decimals, true)
            )
        }
        TransformGizmoMode::Scale => {
            let scale = Vec3::from_array(current.scale);
            let decimals = match (snapping, precision) {
                (true, false) => 1,
                (true, true) => 2,
                (false, true) => 4,
                (false, false) => 2,
            };
            match axis {
                Some(TransformGizmoAxis::X) => {
                    format!(
                        "SCALE X  {}",
                        format_transform_value(scale.x, decimals, false)
                    )
                }
                Some(TransformGizmoAxis::Y) => {
                    format!(
                        "SCALE Y  {}",
                        format_transform_value(scale.y, decimals, false)
                    )
                }
                Some(TransformGizmoAxis::Z) => {
                    format!(
                        "SCALE Z  {}",
                        format_transform_value(scale.z, decimals, false)
                    )
                }
                Some(TransformGizmoAxis::View) | None => {
                    format!(
                        "SCALE XYZ  {}  {}  {}",
                        format_transform_value(scale.x, decimals, false),
                        format_transform_value(scale.y, decimals, false),
                        format_transform_value(scale.z, decimals, false),
                    )
                }
            }
        }
    };
    if snapping {
        value.push_str("  \u{00b7} SNAP");
    }
    if precision {
        value.push_str("  \u{00b7} PRECISE");
    }
    value
}

/// Where a cursor-following overlay `extent` wide sits along one canvas axis: just past the cursor,
/// kept inside the canvas, and pinned to the 8 px inset when the canvas is too small to hold it.
pub(super) fn overlay_offset(cursor: f32, canvas: f32, extent: f32) -> f32 {
    (cursor + 14.0).clamp(8.0, (canvas - extent).max(8.0))
}

pub(super) fn update_transform_gizmo_value_label(
    window: Single<&Window, With<PrimaryWindow>>,
    canvas: Single<(&ComputedNode, &UiGlobalTransform), With<PreviewCanvas>>,
    gizmo: Res<TransformGizmoState>,
    settings: Res<TransformGizmoSettings>,
    keys: Res<ButtonInput<KeyCode>>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    mut labels: Query<(&mut Text, &mut Node, &mut Visibility), With<TransformGizmoValueLabel>>,
) {
    let Some(active) = interaction.active.filter(|_| gizmo.active) else {
        for (_, _, mut visibility) in &mut labels {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let snapping = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let precision = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let message = transform_gizmo_value_text(
        settings.mode,
        gizmo.axis,
        active.current,
        snapping,
        precision,
    );
    let top_left = canvas.1.translation.trunc() - canvas.0.size() * 0.5;
    let local_position = window
        .cursor_position()
        .unwrap_or(top_left + Vec2::splat(24.0))
        - top_left;
    for (mut text, mut node, mut visibility) in &mut labels {
        text.0.clone_from(&message);
        node.left = Val::Px(overlay_offset(local_position.x, canvas.0.size().x, 200.0));
        node.top = Val::Px(overlay_offset(local_position.y, canvas.0.size().y, 32.0));
        *visibility = Visibility::Inherited;
    }
}

/// The position handle of a module (fluid F3): its first `Vec3` input the schema marks as one,
/// unless a host object drives that input. Returns the input's name and current value.
pub(super) fn module_position_handle(
    session: &EditorSession,
    modules: &aestra_compiler::ModuleRegistry,
    id: ModuleId,
) -> Option<(&'static str, [f32; 3])> {
    let (_, module) = session.owned_module(id)?;
    let metadata = modules.get(&module.module_type)?;
    let input = metadata.inputs.iter().find(|input| {
        input.handle == Some(aestra_core::PropertyHandle::Position)
            && input.value_type == aestra_core::ValueType::Vec3
    })?;
    if module.property_source(input.name) == Some(aestra_core::PropertySource::HostBinding) {
        return None;
    }
    let value = match &module.parameters {
        aestra_core::ModuleParameters::Custom(values) => values.get(input.name).cloned(),
        _ => None,
    }
    .unwrap_or_else(|| input.default_value.clone());
    let aestra_core::Value::Vec3(position) = value else {
        return None;
    };
    Some((input.name, position))
}

pub(super) fn selected_preview_transform(
    session: &EditorSession,
    modules: Option<&aestra_compiler::ModuleRegistry>,
) -> Option<(PreviewTransformTarget, EmitterTransform)> {
    if let (SemanticTarget::Module(id), Some(modules)) = (session.selection.primary, modules)
        && let Some((input, position)) = module_position_handle(session, modules, id)
    {
        return Some((
            PreviewTransformTarget::ModuleHandle(id, input),
            EmitterTransform {
                translation: position,
                ..default()
            },
        ));
    }
    if let SemanticTarget::EffectClip(id) = session.selection.primary {
        return session
            .effect
            .effect_clips
            .iter()
            .find(|clip| clip.id == id)
            .map(|clip| (PreviewTransformTarget::EffectClip(id), clip.transform));
    }
    let emitter = session.selected_layer()?;
    Some((
        PreviewTransformTarget::Emitter(emitter.id),
        emitter.transform,
    ))
}

pub(super) fn selected_gizmo_transform(
    session: &EditorSession,
    timeline: Option<&TimelineState>,
    modules: Option<&aestra_compiler::ModuleRegistry>,
) -> Option<(PreviewTransformTarget, EmitterTransform)> {
    if let Some(timeline) = timeline
        && crate::timeline::host_motion::active(session, timeline)
    {
        return crate::timeline::host_motion::selected_pose(session, timeline).map(
            |(index, transform)| {
                (
                    PreviewTransformTarget::HostPose(session.effect.id, index),
                    transform,
                )
            },
        );
    }
    selected_preview_transform(session, modules)
}

pub(super) fn preview_transform_command(
    target: PreviewTransformTarget,
    transform: EmitterTransform,
    session: &EditorSession,
) -> Option<EffectCommand> {
    Some(match target {
        PreviewTransformTarget::Emitter(id) => EffectCommand::SetEmitterTransform { id, transform },
        PreviewTransformTarget::EffectClip(id) => {
            EffectCommand::SetEffectClipTransform { id, transform }
        }
        PreviewTransformTarget::HostPose(effect, index) => {
            if effect != session.effect.id {
                return None;
            }
            let track = crate::timeline::host_motion::replace_pose(
                session.effect.host_transform_track.as_ref()?,
                index,
                transform,
            )
            .ok()?;
            EffectCommand::SetHostTransformTrack { track: Some(track) }
        }
        PreviewTransformTarget::ModuleHandle(module, input) => {
            let (owner, _) = session.owned_module(module)?;
            EffectCommand::SetModuleParameter {
                emitter: owner,
                module,
                parameter: input.to_string(),
                value: aestra_core::Value::Vec3(transform.translation),
            }
        }
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TransformGizmoDragVisual {
    pub(super) rotation: Option<Quat>,
    pub(super) scale: Vec3,
}

pub(super) fn transform_gizmo_drag_visual(
    mode: TransformGizmoMode,
    space: TransformGizmoSpace,
    axis: Option<TransformGizmoAxis>,
    original: Transform,
    current: Transform,
) -> TransformGizmoDragVisual {
    let mut visual = TransformGizmoDragVisual {
        rotation: None,
        scale: Vec3::ONE,
    };
    match mode {
        TransformGizmoMode::Translate => {}
        TransformGizmoMode::Rotate => {
            visual.rotation = Some(match space {
                TransformGizmoSpace::World => {
                    (current.rotation * original.rotation.inverse()).normalize()
                }
                TransformGizmoSpace::Local => current.rotation.normalize(),
            });
        }
        TransformGizmoMode::Scale => {
            let ratio = (current.scale / original.scale.max(Vec3::splat(0.001)))
                .abs()
                .clamp(Vec3::splat(0.15), Vec3::splat(6.0));
            visual.scale = match axis {
                Some(TransformGizmoAxis::X) => Vec3::new(ratio.x, 1.0, 1.0),
                Some(TransformGizmoAxis::Y) => Vec3::new(1.0, ratio.y, 1.0),
                Some(TransformGizmoAxis::Z) => Vec3::new(1.0, 1.0, ratio.z),
                Some(TransformGizmoAxis::View) | None => ratio,
            };
        }
    }
    visual
}

pub(super) fn apply_transform_gizmo_drag_feedback(
    gizmo: Res<TransformGizmoState>,
    settings: Res<TransformGizmoSettings>,
    interaction: Res<EmitterTransformGizmoInteraction>,
    proxy: Single<&Transform, With<EmitterTransformGizmoProxy>>,
    mut roots: Query<
        (Entity, &mut Transform, &mut GlobalTransform),
        (
            With<TransformGizmoVisualRoot>,
            Without<EmitterTransformGizmoProxy>,
        ),
    >,
    mut children: Query<
        (&ChildOf, &Transform, &mut GlobalTransform),
        (
            With<Mesh3d>,
            Without<TransformGizmoVisualRoot>,
            Without<EmitterTransformGizmoProxy>,
        ),
    >,
) {
    if !gizmo.active {
        return;
    }
    let Some(active) = interaction.active else {
        return;
    };
    let visual = transform_gizmo_drag_visual(
        settings.mode,
        settings.space,
        gizmo.axis,
        bevy_transform_from_emitter(active.original),
        **proxy,
    );
    for (root_entity, mut root, mut root_global) in &mut roots {
        if let Some(rotation) = visual.rotation {
            root.rotation = rotation;
        }
        root.scale *= visual.scale;
        *root_global = GlobalTransform::from(*root);
        for (parent, local, mut global) in &mut children {
            if parent.parent() == root_entity {
                *global = root_global.mul_transform(*local);
            }
        }
    }
}

pub(super) fn bevy_transform_from_emitter(transform: EmitterTransform) -> Transform {
    Transform {
        translation: Vec3::from_array(transform.translation),
        rotation: Quat::from_array(transform.rotation).normalize(),
        scale: Vec3::from_array(transform.scale),
    }
}

pub(crate) fn emitter_transform_from_bevy(transform: &Transform) -> EmitterTransform {
    EmitterTransform {
        translation: transform.translation.to_array(),
        rotation: transform.rotation.normalize().to_array(),
        scale: transform.scale.max(Vec3::splat(0.001)).to_array(),
    }
}

pub(super) fn screen_space_gizmo_radius(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    world_position: Vec3,
    radius_pixels: f32,
) -> f32 {
    let camera_right = camera_transform.rotation() * Vec3::X;
    let Ok(center) = camera.world_to_viewport(camera_transform, world_position) else {
        return 0.01;
    };
    let Ok(unit_right) = camera.world_to_viewport(camera_transform, world_position + camera_right)
    else {
        return 0.01;
    };
    let pixels_per_world_unit = center.distance(unit_right);
    if !pixels_per_world_unit.is_finite() || pixels_per_world_unit <= 1.0e-4 {
        return 0.01;
    }
    (radius_pixels / pixels_per_world_unit).clamp(0.001, 1_000.0)
}

#[cfg(test)]
#[path = "tests/transform_gizmo.rs"]
mod tests;
