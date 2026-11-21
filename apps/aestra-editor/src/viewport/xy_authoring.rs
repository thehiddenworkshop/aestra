//! Screen-space handles over authored world XY positions.
use super::{
    gizmo_state::{EmitterTransformGizmoInteraction, PreviewSceneGizmos, ShapeGizmoState},
    host_motion::HostMotionViewport,
    navigation::{
        EmitterTransformGizmoProxy, PreviewCameraController, PreviewRenderCamera, xy_world_height,
    },
    pixel_preview::PixelPreviewPass,
    scene::PreviewXyCamera,
    shortcuts::PreviewCanvas,
    transform_gizmo::bevy_transform_from_emitter,
};
use crate::{input::ShortcutContext, session::EditorSession, theme, timeline::TimelineState};
use aestra_authoring::SemanticTarget;
use aestra_core::{EmitterId, EmitterShape};
use aestra_runtime::Instruction;
use bevy::{
    ecs::system::SystemParam,
    gizmos::transform_gizmo::{
        TransformGizmoAxis as Axis, TransformGizmoMode as Mode, TransformGizmoSettings,
        TransformGizmoSpace, TransformGizmoState, point_to_segment_dist,
    },
    picking::hover::Hovered,
    prelude::*,
    ui::{Pressed, RelativeCursorPosition},
    window::PrimaryWindow,
};

const MARKER_RADIUS: f32 = 8.0;
const AXIS_START: f32 = 14.0;
const AXIS_END: f32 = 55.0;
const RING_RADIUS: f32 = 43.0;
const HIT_MARGIN: f32 = 7.0;
const MIN_SCALE: f32 = 0.001;

#[derive(Component)]
pub(super) struct XyTransformFocus;

type XyCamera = (With<PreviewXyCamera>, With<PreviewRenderCamera>);

#[derive(SystemParam)]
pub(super) struct XyInteractionView<'w, 's> {
    camera: Single<'w, 's, (&'static Camera, &'static GlobalTransform), XyCamera>,
    pixel_passes: Query<'w, 's, (&'static Camera, &'static PixelPreviewPass)>,
}

pub(super) fn enabled(controller: Res<PreviewCameraController>) -> bool {
    controller.xy
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interact(
    context: ShortcutContext,
    mut session: ResMut<EditorSession>,
    motion: Res<HostMotionViewport>,
    timeline: Res<TimelineState>,
    shape: Res<ShapeGizmoState>,
    settings: Res<TransformGizmoSettings>,
    mut state: ResMut<TransformGizmoState>,
    mut transaction: ResMut<EmitterTransformGizmoInteraction>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    canvases: Query<&RelativeCursorPosition, With<PreviewCanvas>>,
    controls: Query<(Option<&Hovered>, Has<Pressed>), With<super::ViewportAction>>,
    panels: Query<
        &RelativeCursorPosition,
        Or<(
            With<crate::asset_browser::BrowserSurface>,
            With<super::sort_preview::SortOverlay>,
        )>,
    >,
    view: XyInteractionView,
    mut proxy: Single<
        (Entity, &mut Transform, Has<XyTransformFocus>),
        With<EmitterTransformGizmoProxy>,
    >,
) {
    let (camera, camera_transform) = *view.camera;
    let (entity, transform, focused) = &mut *proxy;
    let cursor = window.cursor_position();
    let visible = camera.is_active
        || view
            .pixel_passes
            .iter()
            .any(|(camera, pass)| matches!(pass, PixelPreviewPass::Composite) && camera.is_active);
    let blocked = !visible || !window.focused || canvases.is_empty() || context.blocked();
    if state.active {
        if blocked || !*focused || keys.just_pressed(KeyCode::Escape) {
            if let Some(active) = transaction.active.take() {
                **transform = bevy_transform_from_emitter(active.original);
                session.restore_interaction_preview();
            }
            *state = TransformGizmoState::default();
        } else if !buttons.pressed(MouseButton::Left) {
            state.active = false;
            state.axis = None;
            state.entity = None;
        } else if let Some(cursor) = cursor
            && let Ok(point) = camera.viewport_to_world_2d(camera_transform, cursor)
            && let Some(axis) = state.axis
        {
            **transform = drag(
                &settings,
                axis,
                state.start_transform,
                state.drag_start_world.truncate(),
                point,
            );
        }
        return;
    }

    state.hovered_axis = None;
    if blocked
        || session.pending_change.is_some()
        || motion.hovered.is_some()
        || shape.active.is_some()
        || shape.hovered.is_some()
        || buttons.pressed(MouseButton::Middle)
        || buttons.pressed(MouseButton::Right)
        || controls
            .iter()
            .any(|(hovered, pressed)| hovered.is_some_and(Hovered::get) || pressed)
        || panels.iter().any(RelativeCursorPosition::cursor_over)
        || !canvases.iter().any(RelativeCursorPosition::cursor_over)
    {
        return;
    }
    let Some(cursor) = cursor.filter(|point| {
        camera
            .logical_viewport_rect()
            .is_some_and(|rect| rect.contains(*point))
    }) else {
        return;
    };
    let selected = match session.selection.primary {
        SemanticTarget::Emitter(id) => Some(id),
        _ => None,
    };
    if !crate::timeline::host_motion::active(&session, &timeline)
        && let Some(hit) = pick_emitter(&session, camera, camera_transform, cursor)
        && Some(hit) != selected
    {
        if buttons.just_pressed(MouseButton::Left) {
            session.select_emitter(hit);
        }
        return;
    }
    if !*focused {
        return;
    }
    let Ok(center) = camera.world_to_viewport(camera_transform, transform.translation) else {
        return;
    };
    state.hovered_axis = hit_handle(&settings, **transform, cursor - center);
    if buttons.just_pressed(MouseButton::Left)
        && let Some(axis) = state.hovered_axis
        && let Ok(point) = camera.viewport_to_world_2d(camera_transform, cursor)
    {
        state.active = true;
        state.axis = Some(axis);
        state.entity = Some(*entity);
        state.start_transform = **transform;
        state.drag_start_world = point.extend(0.0);
        state.gizmo_origin = transform.translation;
    }
}

fn pick_emitter(
    session: &EditorSession,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
) -> Option<EmitterId> {
    let rect = camera.logical_viewport_rect()?;
    session
        .effect
        .emitters
        .iter()
        .filter_map(|emitter| {
            let world = Vec3::from_array(emitter.transform.translation);
            let point = camera.world_to_viewport(camera_transform, world).ok()?;
            let distance = point.distance_squared(cursor);
            (rect.contains(point) && distance <= MARKER_RADIUS.powi(2))
                .then_some((emitter.id, distance, world.z))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1).then_with(|| b.2.total_cmp(&a.2)))
        .map(|(id, _, _)| id)
}

fn planar_axes(settings: &TransformGizmoSettings, rotation: Quat) -> [Vec2; 2] {
    let yaw = if settings.mode == Mode::Scale || settings.space == TransformGizmoSpace::Local {
        rotation.to_euler(EulerRot::ZYX).0
    } else {
        0.0
    };
    let (sin, cos) = yaw.sin_cos();
    [Vec2::new(cos, sin), Vec2::new(-sin, cos)]
}

fn hit_handle(settings: &TransformGizmoSettings, pose: Transform, offset: Vec2) -> Option<Axis> {
    if settings.mode == Mode::Rotate {
        return ((offset.length() - RING_RADIUS).abs() <= HIT_MARGIN).then_some(Axis::Z);
    }
    if settings.mode == Mode::Translate && offset.length() <= MARKER_RADIUS {
        return Some(Axis::View);
    }
    planar_axes(settings, pose.rotation)
        .into_iter()
        .zip([Axis::X, Axis::Y])
        .map(|(axis, id)| {
            let screen = Vec2::new(axis.x, -axis.y);
            (
                id,
                point_to_segment_dist(offset, screen * AXIS_START, screen * AXIS_END),
            )
        })
        .filter(|(_, distance)| *distance <= HIT_MARGIN)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(axis, _)| axis)
}

fn drag(
    settings: &TransformGizmoSettings,
    axis: Axis,
    start: Transform,
    from: Vec2,
    to: Vec2,
) -> Transform {
    let mut result = start;
    let origin = start.translation.truncate();
    let [x, y] = planar_axes(settings, start.rotation);
    let direction = if axis == Axis::Y { y } else { x };
    match settings.mode {
        Mode::Translate => {
            let delta = to - from;
            let delta = if axis == Axis::View {
                delta
            } else {
                direction * delta.dot(direction)
            };
            result.translation += delta.extend(0.0);
        }
        Mode::Rotate => {
            let a = from - origin;
            let b = to - origin;
            if a.length_squared() > f32::EPSILON && b.length_squared() > f32::EPSILON {
                result.rotation = Quat::from_rotation_z(a.angle_to(b)) * start.rotation;
            }
        }
        Mode::Scale => {
            let initial = (from - origin).dot(direction);
            if initial.abs() > f32::EPSILON {
                let factor = (to - origin).dot(direction) / initial;
                if axis == Axis::Y {
                    result.scale.y = (start.scale.y * factor).max(MIN_SCALE);
                } else {
                    result.scale.x = (start.scale.x * factor).max(MIN_SCALE);
                }
            }
        }
    }
    result
}

pub(super) fn draw(
    session: Res<EditorSession>,
    settings: Res<TransformGizmoSettings>,
    state: Res<TransformGizmoState>,
    camera: Single<(&Camera, &GlobalTransform, &Projection), XyCamera>,
    proxy: Single<(&Transform, Has<XyTransformFocus>), With<EmitterTransformGizmoProxy>>,
    mut gizmos: Gizmos<PreviewSceneGizmos>,
) {
    let (camera, camera_transform, projection) = *camera;
    let Projection::Orthographic(projection) = projection else {
        return;
    };
    let Some(size) = camera.logical_viewport_size().filter(|size| size.y > 0.0) else {
        return;
    };
    let unit = xy_world_height(projection) * projection.scale / size.y;
    let selected = session.selection.emitter(&session.effect);
    for emitter in &session.effect.emitters {
        let center = if selected == Some(emitter.id) && proxy.1 {
            proxy.0.translation
        } else {
            Vec3::from_array(emitter.transform.translation)
        };
        if camera.world_to_viewport(camera_transform, center).is_err() {
            continue;
        }
        let color = if selected == Some(emitter.id) {
            if matches!(state.axis.or(state.hovered_axis), Some(Axis::View)) {
                theme::TEXT
            } else {
                theme::ACCENT
            }
        } else if emitter.enabled {
            theme::TEXT_MUTED
        } else {
            theme::TEXT_FAINT
        };
        gizmos.circle(
            Isometry3d::from_translation(center),
            MARKER_RADIUS * unit,
            color,
        );
    }
    if !proxy.1 {
        return;
    }
    let pose = *proxy.0;
    let color = |axis, idle| {
        if state.hovered_axis == Some(axis) || state.axis == Some(axis) {
            theme::TEXT
        } else {
            idle
        }
    };
    if settings.mode == Mode::Rotate {
        gizmos.circle(
            Isometry3d::from_translation(pose.translation),
            RING_RADIUS * unit,
            color(Axis::Z, Color::srgb(0.40, 0.65, 1.0)),
        );
        return;
    }
    for (axis, (id, idle)) in planar_axes(&settings, pose.rotation).into_iter().zip([
        (Axis::X, Color::srgb(1.0, 0.32, 0.43)),
        (Axis::Y, Color::srgb(0.37, 0.91, 0.57)),
    ]) {
        let direction = axis.extend(0.0);
        let end = pose.translation + direction * AXIS_END * unit;
        let color = color(id, idle);
        let start = pose.translation + direction * AXIS_START * unit;
        if settings.mode == Mode::Translate {
            gizmos.arrow(start, end, color).with_tip_length(9.0 * unit);
        } else {
            gizmos.line(start, end, color);
            gizmos.rect(
                Isometry3d::from_translation(end),
                Vec2::splat(8.0 * unit),
                color,
            );
        }
    }
}

/// Fit the selected emitter's authored spawn footprint and sprite size.
pub(super) fn frame_scale(
    session: &EditorSession,
    viewport: Vec2,
    world_height: f32,
) -> Option<f32> {
    if !viewport.is_finite() || viewport.min_element() <= 0.0 {
        return None;
    }
    let SemanticTarget::Emitter(id) = session.selection.primary else {
        return None;
    };
    let instance = session.preview()?;
    let emitter = instance
        .effect()
        .emitters
        .iter()
        .find(|emitter| emitter.source == id)?;
    let parameters = instance.parameter_values();
    let shape = emitter
        .execution
        .particle_spawn
        .iter()
        .find_map(|step| match step {
            Instruction::SampleShape { shape, .. } => Some(*shape.resolve(parameters)),
            _ => None,
        })
        .unwrap_or(EmitterShape::Point);
    let half = match shape {
        EmitterShape::Point => Vec3::ZERO,
        EmitterShape::Circle { radius } | EmitterShape::Ring { radius } => {
            Vec3::new(radius, radius, 0.0)
        }
        EmitterShape::Sphere { radius } | EmitterShape::Hemisphere { radius } => {
            Vec3::splat(radius)
        }
        EmitterShape::Box { half_extents } => Vec3::from_array(half_extents),
        EmitterShape::Cylinder { radius, depth } => Vec3::new(radius, depth * 0.5, radius),
        EmitterShape::Cone { radius, depth } => Vec3::new(radius, depth, radius),
    };
    let sprite_radius = emitter
        .execution
        .particle_update
        .iter()
        .find_map(|step| match step {
            Instruction::Appearance { size, .. } => {
                let curve = size.resolve(parameters);
                Some(
                    curve
                        .segments()
                        .iter()
                        .fold(curve.last_value(), |max, segment| {
                            max.max(segment.start_value).max(segment.end_value)
                        })
                        * 0.5,
                )
            }
            _ => None,
        })
        .unwrap_or(0.0);
    let pose = bevy_transform_from_emitter(emitter.transform);
    let half = half * pose.scale;
    let projected = (pose.rotation * Vec3::X * half.x).abs()
        + (pose.rotation * Vec3::Y * half.y).abs()
        + (pose.rotation * Vec3::Z * half.z).abs();
    let half = projected.truncate() + Vec2::splat(sprite_radius * pose.scale.max_element());
    let aspect = viewport.x / viewport.y;
    let frame_height = (half.x / aspect).max(half.y) * 2.0 / 0.75;
    frame_height
        .is_finite()
        .then_some((frame_height / world_height).clamp(0.18, 40.0))
}

#[cfg(test)]
#[path = "tests/xy_authoring.rs"]
mod tests;
