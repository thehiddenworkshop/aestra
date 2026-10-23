//! Actual shape-handle picking, transactional drag editing, readout and gizmo geometry.
use super::{
    gizmo_state::*,
    navigation::{EmitterTransformGizmoProxy, PreviewRenderCamera},
    shortcuts::PreviewCanvas,
    transform_gizmo::{overlay_offset, screen_space_gizmo_radius},
};
use crate::{
    localization::Localizer, menus::MenuState, properties::module_parameter,
    session::EditorSession, theme,
};
use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
use aestra_core::{EmitterId, EmitterShape, ModuleId, Value};
use bevy::{
    camera::visibility::RenderLayers,
    prelude::*,
    ui::RelativeCursorPosition,
    window::{CursorIcon, PrimaryWindow, SystemCursorIcon},
};
use fluent_bundle::FluentArgs;

#[allow(clippy::too_many_arguments)]
pub(super) fn interact_shape_gizmo(
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    menu: Res<MenuState>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cursor_icon: Single<&mut CursorIcon, With<PrimaryWindow>>,
    canvas: Single<
        (&RelativeCursorPosition, &ComputedNode, &UiGlobalTransform),
        With<PreviewCanvas>,
    >,
    camera: Single<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    players: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    mut session: ResMut<EditorSession>,
    mut state: ResMut<ShapeGizmoState>,
    mut labels: Query<(&mut Text, &mut Node, &mut Visibility), With<ShapeGizmoValueLabel>>,
    localizer: Res<Localizer>,
) {
    let was_using_cursor = state.hovered.is_some() || state.active.is_some();
    let cursor_position = window.cursor_position();
    let selected = selected_shape_module(&session);
    let Ok(player) = players.single() else {
        state.hovered = None;
        state.active = None;
        update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
        return;
    };
    let (camera, camera_transform) = *camera;

    if keys.just_pressed(KeyCode::Escape) && state.active.take().is_some() {
        state.hovered = None;
        **cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
        update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
        session.restore_interaction_preview();
        session.status = "Cancelled shape adjustment".into();
        return;
    }

    if let Some(active) = state.active.as_mut() {
        if !buttons.pressed(MouseButton::Left) {
            let active = state.active.take().expect("shape drag is active");
            state.hovered = None;
            **cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
            update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
            if active.current != active.original {
                if !session.execute(
                    "Adjusted spawn shape",
                    EffectCommand::SetModuleParameter {
                        emitter: active.emitter,
                        module: active.module,
                        parameter: "shape".into(),
                        value: Value::Shape(active.current),
                    },
                    true,
                ) {
                    session.restore_interaction_preview();
                }
            } else {
                session.restore_interaction_preview();
            }
            return;
        }
        if let Some(cursor_position) = cursor_position
            && let Some(value) = shape_gizmo_drag_value(
                camera,
                camera_transform,
                player,
                active.original,
                active.handle,
                cursor_position,
            )
        {
            let next = shape_after_gizmo_drag(active.original, active.handle, value);
            if next != active.current {
                active.current = next;
                session.preview_interaction(EffectTransaction::single(
                    "Preview shape adjustment",
                    EffectCommand::SetModuleParameter {
                        emitter: active.emitter,
                        module: active.module,
                        parameter: "shape".into(),
                        value: Value::Shape(active.current),
                    },
                ));
            }
        }
        **cursor_icon = shape_gizmo_cursor(active.handle);
        update_shape_gizmo_label(
            &mut labels,
            cursor_position,
            Some((active.handle, active.current)),
            canvas.1,
            canvas.2,
            &localizer,
        );
        return;
    }

    state.hovered = if menu.open.is_none()
        && menu.tab_context.is_none()
        && canvas.0.cursor_over()
        && session.pending_change.is_none()
    {
        cursor_position.and_then(|cursor_position| {
            selected.and_then(|selected| {
                hit_test_shape_gizmo(
                    camera,
                    camera_transform,
                    player,
                    selected.shape,
                    cursor_position,
                )
            })
        })
    } else {
        None
    };

    if let Some(handle) = state.hovered {
        **cursor_icon = shape_gizmo_cursor(handle);
        if buttons.just_pressed(MouseButton::Left)
            && let Some(selected) = selected
            && !shape_gizmo_target_locked(&session, selected)
        {
            state.active = Some(ActiveShapeGizmoDrag {
                emitter: selected.emitter,
                module: selected.module,
                handle,
                original: selected.shape,
                current: selected.shape,
            });
            update_shape_gizmo_label(
                &mut labels,
                cursor_position,
                Some((handle, selected.shape)),
                canvas.1,
                canvas.2,
                &localizer,
            );
        }
    } else if was_using_cursor {
        **cursor_icon = CursorIcon::System(SystemCursorIcon::Default);
    }
    if state.active.is_none() {
        update_shape_gizmo_label(&mut labels, None, None, canvas.1, canvas.2, &localizer);
    }
}

pub(super) fn shape_gizmo_target_locked(
    session: &EditorSession,
    selected: SelectedShapeModule,
) -> bool {
    session
        .locks
        .is_locked(SemanticTarget::Effect(session.effect.id))
        || session
            .locks
            .is_locked(SemanticTarget::Emitter(selected.emitter))
        || session
            .locks
            .is_locked(SemanticTarget::Module(selected.module))
}

pub(super) fn shape_gizmo_cursor(handle: ShapeGizmoHandle) -> CursorIcon {
    CursorIcon::System(match handle {
        ShapeGizmoHandle::Radius => SystemCursorIcon::EwResize,
        ShapeGizmoHandle::Depth => SystemCursorIcon::NsResize,
        ShapeGizmoHandle::ExtentX => SystemCursorIcon::EwResize,
        ShapeGizmoHandle::ExtentY => SystemCursorIcon::NsResize,
        ShapeGizmoHandle::ExtentZ => SystemCursorIcon::NeswResize,
    })
}

pub(super) fn shape_handle_local_positions(shape: EmitterShape) -> Vec<(ShapeGizmoHandle, Vec3)> {
    match shape {
        EmitterShape::Point => Vec::new(),
        EmitterShape::Circle { radius } | EmitterShape::Ring { radius } => {
            vec![(ShapeGizmoHandle::Radius, Vec3::X * radius)]
        }
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            vec![(ShapeGizmoHandle::Radius, Vec3::X * radius)]
        }
        EmitterShape::Box { half_extents } => vec![
            (ShapeGizmoHandle::ExtentX, Vec3::X * half_extents[0]),
            (ShapeGizmoHandle::ExtentY, Vec3::Y * half_extents[1]),
            (ShapeGizmoHandle::ExtentZ, Vec3::Z * half_extents[2]),
        ],
        EmitterShape::Cylinder { radius, depth } => vec![
            (ShapeGizmoHandle::Radius, Vec3::X * radius),
            (ShapeGizmoHandle::Depth, Vec3::Y * depth * 0.5),
        ],
        EmitterShape::Cone { radius, depth } => vec![
            (ShapeGizmoHandle::Radius, Vec3::new(radius, depth, 0.0)),
            (ShapeGizmoHandle::Depth, Vec3::new(0.0, depth, 0.0)),
        ],
    }
}

pub(super) fn hit_test_shape_gizmo(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    player: &GlobalTransform,
    shape: EmitterShape,
    cursor_position: Vec2,
) -> Option<ShapeGizmoHandle> {
    shape_handle_local_positions(shape)
        .into_iter()
        .filter_map(|(handle, local_position)| {
            let viewport_position = camera
                .world_to_viewport(camera_transform, player.transform_point(local_position))
                .ok()?;
            let distance = viewport_position.distance(cursor_position);
            (distance <= 11.0).then_some((handle, distance))
        })
        .min_by(|left, right| left.1.total_cmp(&right.1))
        .map(|(handle, _)| handle)
}

pub(super) fn shape_gizmo_drag_value(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    player: &GlobalTransform,
    shape: EmitterShape,
    handle: ShapeGizmoHandle,
    cursor_position: Vec2,
) -> Option<f32> {
    let (axis, multiplier, anchor) = match handle {
        ShapeGizmoHandle::Radius => {
            let anchor = match shape {
                EmitterShape::Cone { depth, .. } => Vec3::Y * depth,
                _ => Vec3::ZERO,
            };
            (Vec3::X, 1.0, anchor)
        }
        ShapeGizmoHandle::Depth => match shape {
            EmitterShape::Cylinder { .. } => (Vec3::Y, 2.0, Vec3::ZERO),
            _ => (Vec3::Y, 1.0, Vec3::ZERO),
        },
        ShapeGizmoHandle::ExtentX => (Vec3::X, 1.0, Vec3::ZERO),
        ShapeGizmoHandle::ExtentY => (Vec3::Y, 1.0, Vec3::ZERO),
        ShapeGizmoHandle::ExtentZ => (Vec3::Z, 1.0, Vec3::ZERO),
    };
    let screen_origin = camera
        .world_to_viewport(camera_transform, player.transform_point(anchor))
        .ok()?;
    let screen_axis = camera
        .world_to_viewport(camera_transform, player.transform_point(anchor + axis))
        .ok()?
        - screen_origin;
    let pixels_per_unit = screen_axis.length();
    if pixels_per_unit < 1.0e-4 {
        return None;
    }
    Some(
        ((cursor_position - screen_origin).dot(screen_axis / pixels_per_unit) / pixels_per_unit)
            .abs()
            * multiplier,
    )
}

pub(super) fn shape_after_gizmo_drag(
    original: EmitterShape,
    handle: ShapeGizmoHandle,
    value: f32,
) -> EmitterShape {
    const MIN_HANDLE_VALUE: f32 = 0.1;
    match (original, handle) {
        (EmitterShape::Circle { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Circle {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Ring { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Ring {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Sphere { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Sphere {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Hemisphere { .. }, ShapeGizmoHandle::Radius) => EmitterShape::Hemisphere {
            radius: value.max(MIN_HANDLE_VALUE),
        },
        (EmitterShape::Box { mut half_extents }, ShapeGizmoHandle::ExtentX) => {
            half_extents[0] = value.max(MIN_HANDLE_VALUE);
            EmitterShape::Box { half_extents }
        }
        (EmitterShape::Box { mut half_extents }, ShapeGizmoHandle::ExtentY) => {
            half_extents[1] = value.max(MIN_HANDLE_VALUE);
            EmitterShape::Box { half_extents }
        }
        (EmitterShape::Box { mut half_extents }, ShapeGizmoHandle::ExtentZ) => {
            half_extents[2] = value.max(MIN_HANDLE_VALUE);
            EmitterShape::Box { half_extents }
        }
        (EmitterShape::Cylinder { depth, .. }, ShapeGizmoHandle::Radius) => {
            EmitterShape::Cylinder {
                radius: value.max(MIN_HANDLE_VALUE),
                depth,
            }
        }
        (EmitterShape::Cylinder { radius, .. }, ShapeGizmoHandle::Depth) => {
            EmitterShape::Cylinder {
                radius,
                depth: value.max(MIN_HANDLE_VALUE),
            }
        }
        (EmitterShape::Cone { depth, .. }, ShapeGizmoHandle::Radius) => EmitterShape::Cone {
            radius: value.max(MIN_HANDLE_VALUE),
            depth,
        },
        (EmitterShape::Cone { radius, .. }, ShapeGizmoHandle::Depth) => EmitterShape::Cone {
            radius,
            depth: value.max(MIN_HANDLE_VALUE),
        },
        (shape, _) => shape,
    }
}

pub(super) fn update_shape_gizmo_label(
    labels: &mut Query<(&mut Text, &mut Node, &mut Visibility), With<ShapeGizmoValueLabel>>,
    cursor_position: Option<Vec2>,
    value: Option<(ShapeGizmoHandle, EmitterShape)>,
    canvas: &ComputedNode,
    canvas_transform: &UiGlobalTransform,
    localizer: &Localizer,
) {
    let Some((handle, shape)) = value else {
        for (_, _, mut visibility) in labels {
            *visibility = Visibility::Hidden;
        }
        return;
    };
    let scalar = match (handle, shape) {
        (ShapeGizmoHandle::Radius, EmitterShape::Circle { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Ring { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Sphere { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Hemisphere { radius })
        | (ShapeGizmoHandle::Radius, EmitterShape::Cylinder { radius, .. })
        | (ShapeGizmoHandle::Radius, EmitterShape::Cone { radius, .. }) => radius,
        (ShapeGizmoHandle::Depth, EmitterShape::Cone { depth, .. })
        | (ShapeGizmoHandle::Depth, EmitterShape::Cylinder { depth, .. }) => depth,
        (ShapeGizmoHandle::ExtentX, EmitterShape::Box { half_extents }) => half_extents[0],
        (ShapeGizmoHandle::ExtentY, EmitterShape::Box { half_extents }) => half_extents[1],
        (ShapeGizmoHandle::ExtentZ, EmitterShape::Box { half_extents }) => half_extents[2],
        _ => return,
    };
    let mut args = FluentArgs::new();
    args.set("value", format!("{scalar:.2}"));
    let message = localizer.text_with(
        match handle {
            ShapeGizmoHandle::Radius => "viewport-shape-radius",
            ShapeGizmoHandle::Depth => "viewport-shape-depth",
            ShapeGizmoHandle::ExtentX => "viewport-shape-extent-x",
            ShapeGizmoHandle::ExtentY => "viewport-shape-extent-y",
            ShapeGizmoHandle::ExtentZ => "viewport-shape-extent-z",
        },
        &args,
    );
    let top_left = canvas_transform.translation.trunc() - canvas.size() * 0.5;
    let local_position = cursor_position.unwrap_or(top_left + Vec2::splat(24.0)) - top_left;
    for (mut text, mut node, mut visibility) in labels {
        text.0.clone_from(&message);
        node.left = Val::Px(overlay_offset(local_position.x, canvas.size().x, 120.0));
        node.top = Val::Px(overlay_offset(local_position.y, canvas.size().y, 32.0));
        *visibility = Visibility::Inherited;
    }
}

pub(super) fn draw_preview_scene_gizmos(
    session: Res<EditorSession>,
    state: Res<ShapeGizmoState>,
    camera: Single<(&Camera, &GlobalTransform), With<PreviewRenderCamera>>,
    players: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    mut gizmos: Gizmos<PreviewSceneGizmos>,
) {
    let Ok(player) = players.single() else {
        return;
    };
    if let Some(selected) = selected_shape_module(&session) {
        let shape = state
            .active
            .filter(|active| active.module == selected.module)
            .map_or(selected.shape, |active| active.current);
        draw_emitter_shape_gizmo(&mut gizmos, player, shape, theme::ACCENT.with_alpha(0.9));
        for (handle, local_position) in shape_handle_local_positions(shape) {
            let highlighted = state.active.is_some_and(|active| active.handle == handle)
                || state.hovered == Some(handle);
            let world_position = player.transform_point(local_position);
            let handle_radius = screen_space_gizmo_radius(
                camera.0,
                camera.1,
                world_position,
                if highlighted { 7.5 } else { 6.0 },
            );
            gizmos.sphere(
                world_position,
                handle_radius,
                if highlighted {
                    theme::TEXT
                } else {
                    theme::ACCENT
                },
            );
        }
    }
}

pub(super) fn draw_emitter_shape_gizmo(
    gizmos: &mut Gizmos<PreviewSceneGizmos>,
    player: &GlobalTransform,
    shape: EmitterShape,
    color: Color,
) {
    let translation = player.translation();
    let rotation = player.rotation();
    let scale = player.to_scale_rotation_translation().0;
    let axis_scale = scale
        .x
        .abs()
        .max(scale.y.abs())
        .max(scale.z.abs())
        .max(0.001);
    let isometry = Isometry3d::new(translation, rotation);
    match shape {
        EmitterShape::Point => {
            gizmos.cross(isometry, 2.0 * axis_scale, color);
        }
        EmitterShape::Circle { radius } => {
            draw_local_ring(gizmos, player, Vec3::ZERO, radius, RingPlane::Xy, color);
            gizmos.line(
                player.transform_point(Vec3::ZERO),
                player.transform_point(Vec3::X * radius),
                color,
            );
        }
        EmitterShape::Ring { radius } => {
            draw_local_ring(gizmos, player, Vec3::ZERO, radius, RingPlane::Xy, color);
            draw_local_ring(
                gizmos,
                player,
                Vec3::ZERO,
                radius * 0.92,
                RingPlane::Xy,
                color.with_alpha(0.45),
            );
        }
        EmitterShape::Sphere { radius } => {
            for plane in [RingPlane::Xy, RingPlane::Xz, RingPlane::Yz] {
                draw_local_ring(gizmos, player, Vec3::ZERO, radius, plane, color);
            }
        }
        EmitterShape::Hemisphere { radius } => {
            draw_local_ring(gizmos, player, Vec3::ZERO, radius, RingPlane::Xz, color);
            for latitude in 1..=3 {
                let angle = latitude as f32 * std::f32::consts::FRAC_PI_2 / 4.0;
                draw_local_ring(
                    gizmos,
                    player,
                    Vec3::Y * radius * angle.sin(),
                    radius * angle.cos(),
                    RingPlane::Xz,
                    color.with_alpha(0.72),
                );
            }
            for longitude in 0..8 {
                let angle = longitude as f32 * std::f32::consts::TAU / 8.0;
                let mut previous = Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin());
                for segment in 1..=16 {
                    let elevation = segment as f32 * std::f32::consts::FRAC_PI_2 / 16.0;
                    let next = Vec3::new(
                        radius * elevation.cos() * angle.cos(),
                        radius * elevation.sin(),
                        radius * elevation.cos() * angle.sin(),
                    );
                    gizmos.line(
                        player.transform_point(previous),
                        player.transform_point(next),
                        color.with_alpha(0.72),
                    );
                    previous = next;
                }
            }
        }
        EmitterShape::Box { half_extents } => {
            let half = Vec3::from_array(half_extents);
            for x in [-half.x, half.x] {
                for y in [-half.y, half.y] {
                    gizmos.line(
                        player.transform_point(Vec3::new(x, y, -half.z)),
                        player.transform_point(Vec3::new(x, y, half.z)),
                        color,
                    );
                }
            }
            for x in [-half.x, half.x] {
                for z in [-half.z, half.z] {
                    gizmos.line(
                        player.transform_point(Vec3::new(x, -half.y, z)),
                        player.transform_point(Vec3::new(x, half.y, z)),
                        color,
                    );
                }
            }
            for y in [-half.y, half.y] {
                for z in [-half.z, half.z] {
                    gizmos.line(
                        player.transform_point(Vec3::new(-half.x, y, z)),
                        player.transform_point(Vec3::new(half.x, y, z)),
                        color,
                    );
                }
            }
        }
        EmitterShape::Cylinder { radius, depth } => {
            let half_depth = depth * 0.5;
            draw_local_ring(
                gizmos,
                player,
                Vec3::Y * half_depth,
                radius,
                RingPlane::Xz,
                color,
            );
            draw_local_ring(
                gizmos,
                player,
                Vec3::Y * -half_depth,
                radius,
                RingPlane::Xz,
                color,
            );
            for quarter in 0..4 {
                let angle = quarter as f32 * std::f32::consts::FRAC_PI_2;
                let radial = Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
                gizmos.line(
                    player.transform_point(radial - Vec3::Y * half_depth),
                    player.transform_point(radial + Vec3::Y * half_depth),
                    color,
                );
            }
        }
        EmitterShape::Cone { radius, depth } => {
            let origin = player.transform_point(Vec3::ZERO);
            draw_local_ring(
                gizmos,
                player,
                Vec3::Y * depth,
                radius,
                RingPlane::Xz,
                color.with_alpha(0.72),
            );
            for quarter in 0..4 {
                let angle = quarter as f32 * std::f32::consts::FRAC_PI_2;
                gizmos.line(
                    origin,
                    player.transform_point(Vec3::new(
                        angle.cos() * radius,
                        depth,
                        angle.sin() * radius,
                    )),
                    color,
                );
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum RingPlane {
    Xy,
    Xz,
    Yz,
}

pub(super) fn draw_local_ring(
    gizmos: &mut Gizmos<PreviewSceneGizmos>,
    player: &GlobalTransform,
    center: Vec3,
    radius: f32,
    plane: RingPlane,
    color: Color,
) {
    const SEGMENTS: usize = 64;
    let point = |index: usize| {
        let angle = index as f32 * std::f32::consts::TAU / SEGMENTS as f32;
        let radial = match plane {
            RingPlane::Xy => Vec3::new(angle.cos() * radius, angle.sin() * radius, 0.0),
            RingPlane::Xz => Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius),
            RingPlane::Yz => Vec3::new(0.0, angle.cos() * radius, angle.sin() * radius),
        };
        player.transform_point(center + radial)
    };
    for index in 0..SEGMENTS {
        gizmos.line(point(index), point((index + 1) % SEGMENTS), color);
    }
}

pub(super) fn configure_preview_scene_gizmos(mut config_store: ResMut<GizmoConfigStore>) {
    let (config, _) = config_store.config_mut::<PreviewSceneGizmos>();
    config.render_layers = RenderLayers::layer(15);
    config.line.width = 1.5;
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SelectedShapeModule {
    pub(super) emitter: EmitterId,
    pub(super) module: ModuleId,
    pub(super) shape: EmitterShape,
}

pub(super) fn selected_shape_module(session: &EditorSession) -> Option<SelectedShapeModule> {
    if session.pending_change.is_some() {
        return None;
    }
    let SemanticTarget::Module(module_id) = session.selection.primary else {
        return None;
    };
    let emitter = session.selected_layer()?;
    let module = emitter
        .modules
        .iter()
        .find(|module| module.id == module_id)?;
    match module_parameter(module, "shape") {
        Some(Value::Shape(shape)) if module.enabled => Some(SelectedShapeModule {
            emitter: emitter.id,
            module: module_id,
            shape,
        }),
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/shape_gizmo.rs"]
mod tests;
