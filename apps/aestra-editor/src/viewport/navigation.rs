//! Existing preview camera navigation shared by shipping and compatibility qualification.
use super::shortcuts::PreviewCanvas;
use bevy::{
    input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::RelativeCursorPosition,
};

pub(super) const DEFAULT_PREVIEW_PITCH: f32 = -0.35;

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
}

impl Default for PreviewCameraController {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 140.0,
            yaw: 0.0,
            pitch: DEFAULT_PREVIEW_PITCH,
            frame_requested: false,
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

pub(super) fn navigate_preview_camera(
    context: crate::input::ShortcutContext,
    time: Res<Time<Real>>,
    mut motion: MessageReader<MouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    canvas: Single<&RelativeCursorPosition, With<PreviewCanvas>>,
    browser: Query<&RelativeCursorPosition, With<crate::asset_browser::BrowserSurface>>,
    player: Query<&GlobalTransform, With<EmitterTransformGizmoProxy>>,
    mut navigation: ResMut<PreviewNavigationState>,
    mut controller: ResMut<PreviewCameraController>,
    mut camera: Single<&mut Transform, With<PreviewRenderCamera>>,
) {
    if context.blocked() || keys.just_pressed(KeyCode::Escape) {
        motion.clear();
        wheel.clear();
        navigation.dragging = false;
        navigation.looking = false;
        navigation.flight_blocked = true;
        return;
    }
    let cursor_over =
        canvas.cursor_over() && !browser.iter().any(RelativeCursorPosition::cursor_over);
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
    if cursor_over && !navigation.flight_blocked && !shortcut_modifier {
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

    **camera = preview_camera_transform(
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
