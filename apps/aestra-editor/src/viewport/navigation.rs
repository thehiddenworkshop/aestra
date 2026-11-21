//! Existing preview camera navigation shared by shipping and compatibility qualification.
use super::shortcuts::PreviewCanvas;
use super::{
    PreviewDisplayState,
    gizmo_state::{EmitterTransformGizmoInteraction, ShapeGizmoState},
};
use bevy::{
    camera::ScalingMode,
    ecs::system::SystemParam,
    gizmos::transform_gizmo::TransformGizmoState,
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::RelativeCursorPosition,
    window::PrimaryWindow,
};

pub(super) const DEFAULT_PREVIEW_PITCH: f32 = -0.35;
pub(super) const DEFAULT_XY_HEIGHT: f32 = 100.0;

pub(super) fn xy_world_height(projection: &OrthographicProjection) -> f32 {
    match projection.scaling_mode {
        ScalingMode::FixedVertical { viewport_height } => viewport_height,
        _ => DEFAULT_XY_HEIGHT,
    }
}

type NavigationCamera = (
    &'static mut Transform,
    Option<&'static mut Projection>,
    Option<&'static Camera>,
);

#[derive(Component)]
pub(crate) struct PreviewRenderCamera;

#[derive(Component)]
pub(crate) struct EmitterTransformGizmoProxy;

#[derive(Resource)]
pub(crate) struct PreviewCameraController {
    pub(super) focus: Vec3,
    pub(super) distance: f32,
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) frame_requested: bool,
    pub(super) xy: bool,
}

impl Default for PreviewCameraController {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 140.0,
            yaw: 0.0,
            pitch: DEFAULT_PREVIEW_PITCH,
            frame_requested: false,
            xy: false,
        }
    }
}

impl PreviewCameraController {
    pub(super) fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.005;
        self.pitch = (self.pitch - delta.y * 0.005).clamp(-1.54, 1.54);
    }

    pub(super) fn fly(&mut self, keys: &ButtonInput<KeyCode>, seconds: f32) -> bool {
        let axis = Vec2::new(
            f32::from(keys.pressed(KeyCode::KeyD)) - f32::from(keys.pressed(KeyCode::KeyA)),
            f32::from(keys.pressed(KeyCode::KeyW)) - f32::from(keys.pressed(KeyCode::KeyS)),
        )
        .normalize_or_zero();
        if axis == Vec2::ZERO || seconds <= 0.0 {
            return false;
        }
        let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
            3.0
        } else {
            1.0
        };
        let rotation = Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(self.pitch);
        let speed = self.distance * 0.5 * boost;
        // Real time keeps navigation independent of preview playback/pausing. Bound a stalled
        // frame so returning to the app never causes a large camera jump.
        self.focus += rotation * Vec3::new(axis.x, 0.0, -axis.y) * speed * seconds.min(0.1);
        true
    }

    pub(super) fn frame_effect(&mut self, position: Vec3) {
        self.focus = position;
        self.distance = 140.0;
        self.yaw = 0.0;
        self.pitch = DEFAULT_PREVIEW_PITCH;
        self.frame_requested = false;
    }

    pub(crate) fn request_frame(&mut self) {
        self.frame_requested = true;
    }
}

#[derive(Resource, Default)]
pub(super) struct PreviewNavigationState {
    pub(super) dragging: bool,
    pub(super) looking: bool,
    pub(super) flight_blocked: bool,
}

/// Ownership stays application-side even if camera motion is delegated later.
#[derive(SystemParam)]
pub(super) struct PreviewInputCapture<'w> {
    native: Option<Res<'w, TransformGizmoState>>,
    shape: Option<Res<'w, ShapeGizmoState>>,
    transform: Option<Res<'w, EmitterTransformGizmoInteraction>>,
}

impl PreviewInputCapture<'_> {
    fn gizmo_owns_input(&self, buttons: &ButtonInput<MouseButton>) -> bool {
        self.native.as_ref().is_some_and(|state| state.active)
            || self.shape.as_ref().is_some_and(|state| state.active.is_some())
            || self.transform.as_ref().is_some_and(|state| state.is_active())
            // Picking/drag activation happens after Update. Reserve the initial
            // left press from the last committed hover state, not one frame late.
            || (buttons.pressed(MouseButton::Left)
                && (self.native.as_ref().is_some_and(|state| state.hovered_axis.is_some())
                    || self.shape.as_ref().is_some_and(|state| state.hovered.is_some())))
    }
}

pub(super) fn navigate_preview_camera(
    context: crate::input::ShortcutContext,
    capture: PreviewInputCapture,
    time: Res<Time<Real>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    panels: Query<
        &RelativeCursorPosition,
        Or<(
            With<crate::asset_browser::BrowserSurface>,
            With<super::sort_preview::SortOverlay>,
        )>,
    >,
    player: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    session: Option<Res<crate::session::EditorSession>>,
    mut navigation: ResMut<PreviewNavigationState>,
    mut controller: ResMut<PreviewCameraController>,
    camera: Single<NavigationCamera, With<PreviewRenderCamera>>,
    display: Option<Res<PreviewDisplayState>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    if context.blocked() || capture.gizmo_owns_input(&buttons) || keys.just_pressed(KeyCode::Escape)
    {
        motion.clear();
        wheel.clear();
        navigation.dragging = false;
        navigation.looking = false;
        navigation.flight_blocked = true;
        return;
    }
    let mut cursor_over =
        canvas.cursor_over() && !panels.iter().any(RelativeCursorPosition::cursor_over);
    let (mut camera, projection, viewport) = camera.into_inner();
    if controller.xy && display.is_some_and(|display| display.pixel_grid) {
        cursor_over &= window
            .single()
            .ok()
            .and_then(Window::cursor_position)
            .is_some_and(|cursor| {
                viewport
                    .and_then(Camera::logical_viewport_rect)
                    .is_some_and(|rect| rect.contains(cursor))
            });
    }
    let pointer_delta = motion
        .read()
        .fold(Vec2::ZERO, |sum, event| sum + event.delta);
    let scroll_delta = wheel.read().fold(0.0, |sum, event| {
        let scale = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.02,
        };
        sum + event.y * scale
    });
    if buttons.just_pressed(MouseButton::Middle) && cursor_over {
        navigation.dragging = true;
    }
    if !buttons.pressed(MouseButton::Middle) {
        navigation.dragging = false;
    }
    if controller.xy {
        navigation.looking = false;
        let panning = navigation.dragging && pointer_delta != Vec2::ZERO;
        let zooming = cursor_over && scroll_delta != 0.0;
        if !(controller.frame_requested || panning || zooming) {
            return;
        }
        let Some(mut projection) = projection else {
            return;
        };
        let Projection::Orthographic(projection) = &mut *projection else {
            return;
        };
        let world_height = xy_world_height(projection);
        if controller.frame_requested {
            let focus = player
                .single()
                .map_or(Vec3::ZERO, GlobalTransform::translation);
            projection.scale = session
                .as_deref()
                .zip(viewport.and_then(Camera::logical_viewport_size))
                .and_then(|(session, size)| {
                    super::xy_authoring::frame_scale(session, size, world_height)
                })
                .unwrap_or(1.0);
            camera.translation = focus + Vec3::Z * DEFAULT_XY_HEIGHT * projection.scale;
            controller.frame_requested = false;
        }
        if panning {
            let height = viewport
                .and_then(Camera::physical_viewport_size)
                .map_or(400.0, |size| size.y.max(1) as f32);
            let units_per_pixel = world_height * projection.scale / height;
            camera.translation +=
                Vec3::new(-pointer_delta.x, pointer_delta.y, 0.0) * units_per_pixel;
        }
        if zooming {
            let previous_scale = projection.scale;
            projection.scale = (projection.scale * (-scroll_delta * 0.12).exp()).clamp(0.01, 40.0);
            camera.translation.z += DEFAULT_XY_HEIGHT * (projection.scale - previous_scale);
        }
        return;
    }
    if buttons.just_pressed(MouseButton::Right) && cursor_over {
        navigation.looking = true;
    }
    if !buttons.pressed(MouseButton::Right) {
        navigation.looking = false;
    }
    let mut changed = false;
    if controller.frame_requested {
        let focus = player
            .single()
            .map_or(Vec3::ZERO, GlobalTransform::translation);
        controller.frame_effect(focus);
        changed = true;
    }
    if navigation.looking {
        if pointer_delta != Vec2::ZERO {
            controller.orbit(pointer_delta);
            changed = true;
        }
    } else if navigation.dragging && pointer_delta != Vec2::ZERO {
        let right = camera.rotation * Vec3::X;
        let up = camera.rotation * Vec3::Y;
        let units_per_pixel = controller.distance * 0.0018;
        controller.focus += (-right * pointer_delta.x + up * pointer_delta.y) * units_per_pixel;
        changed = true;
    }
    let tab_focused = context.tab_strip_focused();
    if tab_focused {
        // Keyboard navigation belongs to the strip. Explicit pointer orbit/pan
        // and zoom over the viewport must remain usable without an extra click.
        navigation.flight_blocked = true;
    }
    if ![KeyCode::KeyW, KeyCode::KeyA, KeyCode::KeyS, KeyCode::KeyD]
        .into_iter()
        .any(|key| keys.pressed(key))
    {
        navigation.flight_blocked = false;
    }
    let shortcut_modifier = [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]
    .into_iter()
    .any(|key| keys.pressed(key));
    if cursor_over && !tab_focused && !navigation.flight_blocked && !shortcut_modifier {
        changed |= controller.fly(&keys, time.delta_secs());
    }
    if cursor_over && scroll_delta != 0.0 {
        controller.distance =
            (controller.distance * (-scroll_delta * 0.12).exp()).clamp(1.0, 4_000.0);
        changed = true;
    }
    if !changed {
        return;
    }

    *camera = preview_camera_transform(
        controller.focus,
        controller.distance,
        controller.yaw,
        controller.pitch,
    );
}

pub(super) fn preview_camera_transform(
    focus: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
) -> Transform {
    let orbit = Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch);
    Transform::from_translation(focus + orbit * Vec3::Z * distance).looking_at(focus, Vec3::Y)
}

#[cfg(test)]
#[path = "tests/navigation.rs"]
mod tests;
