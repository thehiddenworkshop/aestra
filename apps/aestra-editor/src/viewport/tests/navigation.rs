//! Shared execution of the original navigation regressions plus compatibility edge cases.
use super::*;
use crate::menus::MenuState;

fn navigation_app() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.init_resource::<PreviewCameraController>()
        .init_resource::<PreviewNavigationState>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<MouseWheel>()
        .add_message::<MouseMotion>()
        .add_systems(Update, navigate_preview_camera);
    let canvas = app
        .world_mut()
        .spawn((
            PreviewCanvas,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
        ))
        .id();
    let camera = app
        .world_mut()
        .spawn((
            PreviewRenderCamera,
            preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH),
        ))
        .id();
    (app, canvas, camera)
}

fn navigation_tick(app: &mut App, delta: Vec2) {
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs_f32(0.02));
    app.world_mut().write_message(MouseMotion { delta });
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

#[test]
fn dock_tab_focus_blocks_keyboard_flight_but_preserves_explicit_pointer_orbit() {
    let (mut app, _, camera) = navigation_app();
    let strip = app
        .world_mut()
        .spawn(bevy::ui_widgets::TabList::default())
        .id();
    let tab = app
        .world_mut()
        .spawn((bevy::ui_widgets::Tab, ChildOf(strip)))
        .id();
    app.insert_resource(bevy::input_focus::InputFocus::from_entity(tab));
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    navigation_tick(&mut app, Vec2::ZERO);
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(20.0));
    assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    assert_eq!(
        app.world().resource::<PreviewCameraController>().focus,
        Vec3::ZERO,
        "W must not fly while the strip owns keyboard focus"
    );
}

#[test]
fn gizmo_capture_and_initial_handle_press_own_camera_input_without_eating_frame_requests() {
    use super::super::gizmo_state::*;
    use bevy::gizmos::transform_gizmo::TransformGizmoAxis;
    for kind in 0..5 {
        let (mut app, _, camera) = navigation_app();
        let mut native = TransformGizmoState::default();
        let mut shape = ShapeGizmoState::default();
        let mut transform = EmitterTransformGizmoInteraction::default();
        let emitter = aestra_core::EmitterId::new();
        match kind {
            0 => native.active = true,
            1 => {
                shape.active = Some(ActiveShapeGizmoDrag {
                    emitter,
                    module: aestra_core::ModuleId::new(),
                    handle: ShapeGizmoHandle::Radius,
                    original: aestra_core::EmitterShape::Sphere { radius: 1.0 },
                    current: aestra_core::EmitterShape::Sphere { radius: 1.0 },
                })
            }
            2 => {
                transform.active = Some(ActiveEmitterTransformGizmo {
                    target: PreviewTransformTarget::Emitter(emitter),
                    original: default(),
                    current: default(),
                    raw_anchor: default(),
                    output_anchor: default(),
                    precision: false,
                    effect: aestra_core::EffectId::new(),
                    revision: 0,
                })
            }
            3 => native.hovered_axis = Some(TransformGizmoAxis::X),
            _ => shape.hovered = Some(ShapeGizmoHandle::Radius),
        }
        app.insert_resource(native)
            .insert_resource(shape)
            .insert_resource(transform);
        let before = *app.world().get::<Transform>(camera).unwrap();
        app.world_mut()
            .resource_mut::<PreviewCameraController>()
            .request_frame();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Middle);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyW);
        scroll(&mut app, MouseScrollUnit::Line, 3.0);
        navigation_tick(&mut app, Vec2::splat(50.0));
        assert_eq!(
            *app.world().get::<Transform>(camera).unwrap(),
            before,
            "capture kind {kind}"
        );
        assert!(
            app.world()
                .resource::<PreviewCameraController>()
                .frame_requested
        );
        assert!(!app.world().resource::<PreviewNavigationState>().looking);
        assert!(!app.world().resource::<PreviewNavigationState>().dragging);
    }
}

#[test]
fn leaving_gizmo_capture_discards_stale_deltas_and_requires_fresh_drag_and_flight() {
    let (mut app, _, camera) = navigation_app();
    app.insert_resource(TransformGizmoState {
        active: true,
        ..default()
    });
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    scroll(&mut app, MouseScrollUnit::Line, 9.0);
    navigation_tick(&mut app, Vec2::splat(100.0));
    app.world_mut().resource_mut::<TransformGizmoState>().active = false;
    navigation_tick(&mut app, Vec2::ZERO);
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyW);
    navigation_tick(&mut app, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::new(20.0, 0.0));
    assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    let before_flight = app.world().resource::<PreviewCameraController>().focus;
    navigation_tick(&mut app, Vec2::ZERO);
    assert_ne!(
        app.world().resource::<PreviewCameraController>().focus,
        before_flight
    );
    assert_eq!(
        app.world().resource::<PreviewCameraController>().distance,
        140.0
    );
}

#[test]
fn merely_hovering_a_gizmo_does_not_disable_explicit_orbit_or_pan() {
    use super::super::gizmo_state::*;
    use bevy::gizmos::transform_gizmo::TransformGizmoAxis;
    for button in [MouseButton::Right, MouseButton::Middle] {
        let (mut app, _, camera) = navigation_app();
        app.insert_resource(TransformGizmoState {
            hovered_axis: Some(TransformGizmoAxis::X),
            ..default()
        });
        app.insert_resource(ShapeGizmoState {
            hovered: Some(ShapeGizmoHandle::Radius),
            ..default()
        });
        let before = *app.world().get::<Transform>(camera).unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(button);
        navigation_tick(&mut app, Vec2::splat(15.0));
        assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    }
}

#[test]
fn preview_navigation_middle_drag_pans_without_modifiers_or_rotation() {
    let (mut app, _, camera) = navigation_app();
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    let delta = Vec2::new(30.0, -20.0);
    navigation_tick(&mut app, delta);
    let after = app.world().get::<Transform>(camera).unwrap();
    let expected = before.rotation * Vec3::new(-delta.x, delta.y, 0.0) * 140.0 * 0.0018;
    assert!(
        after
            .translation
            .abs_diff_eq(before.translation + expected, 0.0001)
    );
    assert!(after.rotation.abs_diff_eq(before.rotation, 0.0001));
    assert_eq!(app.world().resource::<PreviewCameraController>().yaw, 0.0);
    assert_eq!(
        app.world().resource::<PreviewCameraController>().pitch,
        DEFAULT_PREVIEW_PITCH
    );
}

#[test]
fn preview_navigation_right_drag_orbits_around_the_focus() {
    let (mut app, _, camera) = navigation_app();
    let before = *app.world().get::<Transform>(camera).unwrap();
    let focus = app.world().resource::<PreviewCameraController>().focus;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::new(40.0, -30.0));
    let after = app.world().get::<Transform>(camera).unwrap();
    assert!(!after.translation.abs_diff_eq(before.translation, 0.0001));
    assert!(!after.rotation.abs_diff_eq(before.rotation, 0.001));
    assert_eq!(
        app.world().resource::<PreviewCameraController>().focus,
        focus
    );
    assert!((after.translation.distance(focus) - 140.0).abs() < 0.0001);
    assert!(
        after.forward().dot(*before.right()) > 0.0,
        "dragging right rotates the view right"
    );
    assert_eq!(
        app.world().resource::<PreviewCameraController>().distance,
        140.0
    );
    navigation_tick(&mut app, Vec2::new(0.0, -100_000.0));
    assert_eq!(
        app.world().resource::<PreviewCameraController>().pitch,
        1.54
    );
}

#[test]
fn preview_navigation_wasd_flies_without_a_mouse_button_only_over_the_preview() {
    for (key, local) in [
        (KeyCode::KeyW, Vec3::NEG_Z),
        (KeyCode::KeyS, Vec3::Z),
        (KeyCode::KeyA, Vec3::NEG_X),
        (KeyCode::KeyD, Vec3::X),
    ] {
        let (mut app, canvas, camera) = navigation_app();
        let before = *app.world().get::<Transform>(camera).unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        navigation_tick(&mut app, Vec2::ZERO);
        let after = *app.world().get::<Transform>(camera).unwrap();
        assert!(
            after
                .translation
                .abs_diff_eq(before.translation + before.rotation * local * 1.4, 0.0001)
        );
        app.world_mut()
            .get_mut::<RelativeCursorPosition>(canvas)
            .unwrap()
            .cursor_over = false;
        navigation_tick(&mut app, Vec2::ZERO);
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation,
            after.translation
        );
    }
}

#[test]
fn preview_navigation_flight_normalizes_diagonals_and_is_frame_rate_independent() {
    let mut keys = ButtonInput::default();
    keys.press(KeyCode::KeyW);
    keys.press(KeyCode::KeyD);
    let mut sixty = PreviewCameraController::default();
    let mut one_twenty = PreviewCameraController::default();
    for _ in 0..60 {
        sixty.fly(&keys, 1.0 / 60.0);
    }
    for _ in 0..120 {
        one_twenty.fly(&keys, 1.0 / 120.0);
    }
    assert!(sixty.focus.abs_diff_eq(one_twenty.focus, 0.001));
    assert!((sixty.focus.length() - 70.0).abs() < 0.001);
    let mut boosted = PreviewCameraController::default();
    keys.press(KeyCode::ShiftLeft);
    boosted.fly(&keys, 0.02);
    assert!((boosted.focus.length() - 4.2).abs() < 0.0001);
    let mut stalled = PreviewCameraController::default();
    stalled.fly(&keys, 10.0);
    assert!((stalled.focus.length() - 21.0).abs() < 0.0001);
}

#[test]
fn preview_navigation_orbit_requires_a_press_inside_and_escape_stops_flight() {
    let (mut app, canvas, camera) = navigation_app();
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(canvas)
        .unwrap()
        .cursor_over = false;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(40.0));
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(canvas)
        .unwrap()
        .cursor_over = true;
    navigation_tick(&mut app, Vec2::splat(40.0));
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    navigation_tick(&mut app, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(40.0));
    let moved = *app.world().get::<Transform>(camera).unwrap();
    assert_ne!(moved.translation, before.translation);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    navigation_tick(&mut app, Vec2::splat(40.0));
    assert!(!app.world().resource::<PreviewNavigationState>().looking);
    navigation_tick(&mut app, Vec2::ZERO);
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), moved);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::KeyW);
    navigation_tick(&mut app, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    navigation_tick(&mut app, Vec2::ZERO);
    assert_ne!(*app.world().get::<Transform>(camera).unwrap(), moved);
}

#[test]
fn preview_navigation_focus_loss_cancels_flight_without_resuming_on_return() {
    let (mut app, _, camera) = navigation_app();
    let window = app
        .world_mut()
        .spawn(Window {
            focused: true,
            ..default()
        })
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::ZERO);
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
    navigation_tick(&mut app, Vec2::splat(30.0));
    assert!(!app.world().resource::<PreviewNavigationState>().looking);
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    navigation_tick(&mut app, Vec2::splat(30.0));
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
}

#[test]
fn preview_navigation_is_blocked_while_typing_or_a_menu_is_open() {
    let (mut app, _, camera) = navigation_app();
    let before = *app.world().get::<Transform>(camera).unwrap();
    let input = app
        .world_mut()
        .spawn(bevy::text::EditableText::new("draft"))
        .id();
    app.world_mut()
        .insert_resource(bevy::input_focus::InputFocus::from_entity(input));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(30.0));
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<bevy::input_focus::InputFocus>()
        .clear();
    app.init_resource::<MenuState>();
    app.world_mut().resource_mut::<MenuState>().show_about = true;
    navigation_tick(&mut app, Vec2::splat(30.0));
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
}

#[test]
fn asset_browser_blocks_preview_scroll_and_pan_start() {
    let mut app = App::new();
    app.init_resource::<PreviewCameraController>()
        .init_resource::<PreviewNavigationState>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .add_message::<MouseWheel>()
        .add_message::<MouseMotion>()
        .add_systems(Update, navigate_preview_camera);
    app.world_mut().spawn((
        PreviewCanvas,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        },
    ));
    let browser = app
        .world_mut()
        .spawn((
            crate::asset_browser::BrowserSurface,
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
        ))
        .id();
    app.world_mut()
        .spawn((PreviewRenderCamera, Transform::default()));
    let distance = app.world().resource::<PreviewCameraController>().distance;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 3.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    app.update();
    assert_eq!(
        app.world().resource::<PreviewCameraController>().distance,
        distance
    );
    assert!(!app.world().resource::<PreviewNavigationState>().dragging);
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(browser)
        .unwrap()
        .cursor_over = false;
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: 3.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    app.update();
    assert!(app.world().resource::<PreviewCameraController>().distance < distance);
}

#[test]
fn framing_preview_restores_default_camera_around_effect() {
    let mut controller = PreviewCameraController {
        focus: Vec3::new(12.0, -7.0, 3.0),
        distance: 9.0,
        yaw: 1.2,
        pitch: -0.8,
        frame_requested: true,
    };
    let effect_position = Vec3::new(24.0, 6.0, -2.0);

    controller.frame_effect(effect_position);

    assert_eq!(controller.focus, effect_position);
    assert_eq!(controller.distance, 140.0);
    assert_eq!(controller.yaw, 0.0);
    assert_eq!(controller.pitch, DEFAULT_PREVIEW_PITCH);
    assert!(!controller.frame_requested);
}

#[test]
fn default_preview_camera_views_the_ground_grid_from_above() {
    let camera = preview_camera_transform(Vec3::ZERO, 140.0, 0.0, DEFAULT_PREVIEW_PITCH);

    assert!(camera.translation.y > 0.0);
    assert!((camera.forward().dot(Vec3::NEG_Y)).abs() > 0.25);
}

fn scroll(app: &mut App, unit: MouseScrollUnit, y: f32) {
    app.world_mut().write_message(MouseWheel {
        unit,
        x: 0.0,
        y,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
}

#[test]
fn line_and_pixel_wheels_accumulate_equally_and_zoom_stays_bounded() {
    let (mut lines, _, _) = navigation_app();
    let (mut pixels, _, _) = navigation_app();
    for delta in [1.0, 2.0, -0.5] {
        scroll(&mut lines, MouseScrollUnit::Line, delta);
        scroll(&mut pixels, MouseScrollUnit::Pixel, delta * 50.0);
    }
    navigation_tick(&mut lines, Vec2::ZERO);
    navigation_tick(&mut pixels, Vec2::ZERO);
    let distance = lines.world().resource::<PreviewCameraController>().distance;
    assert!((distance - 140.0 * (-2.5_f32 * 0.12).exp()).abs() < 0.0001);
    assert_eq!(
        distance,
        pixels
            .world()
            .resource::<PreviewCameraController>()
            .distance
    );
    for (delta, expected) in [(100_000.0, 1.0), (-100_000.0, 4_000.0)] {
        scroll(&mut lines, MouseScrollUnit::Line, delta);
        navigation_tick(&mut lines, Vec2::ZERO);
        assert_eq!(
            lines.world().resource::<PreviewCameraController>().distance,
            expected
        );
    }
}

#[test]
fn captured_orbit_continues_outside_then_release_retires_capture() {
    let (mut app, canvas, camera) = navigation_app();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(10.0));
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(canvas)
        .unwrap()
        .cursor_over = false;
    navigation_tick(&mut app, Vec2::splat(10.0));
    assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    navigation_tick(&mut app, Vec2::ZERO);
    let stopped = *app.world().get::<Transform>(camera).unwrap();
    navigation_tick(&mut app, Vec2::splat(10.0));
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), stopped);
    assert!(!app.world().resource::<PreviewNavigationState>().looking);
}

#[test]
fn simultaneous_orbit_and_pan_preserve_orbit_precedence_then_pan_takes_over() {
    let (mut app, _, _) = navigation_app();
    for button in [MouseButton::Middle, MouseButton::Right] {
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(button);
    }
    navigation_tick(&mut app, Vec2::splat(20.0));
    let controller = app.world().resource::<PreviewCameraController>();
    assert_eq!(controller.focus, Vec3::ZERO);
    assert_ne!(controller.yaw, 0.0);
    let yaw = controller.yaw;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(20.0));
    let controller = app.world().resource::<PreviewCameraController>();
    assert_eq!(controller.yaw, yaw);
    assert_ne!(controller.focus, Vec3::ZERO);
}

#[test]
fn shortcut_modifiers_block_flight_but_either_shift_boosts_it() {
    for modifier in [
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ] {
        let (mut app, _, camera) = navigation_app();
        let before = *app.world().get::<Transform>(camera).unwrap();
        for key in [KeyCode::KeyW, modifier] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
        }
        navigation_tick(&mut app, Vec2::ZERO);
        assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(modifier);
        navigation_tick(&mut app, Vec2::ZERO);
        assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
    }
    for shift in [KeyCode::ShiftLeft, KeyCode::ShiftRight] {
        let (mut app, _, _) = navigation_app();
        for key in [KeyCode::KeyW, shift] {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
        }
        navigation_tick(&mut app, Vec2::ZERO);
        assert!(
            (app.world()
                .resource::<PreviewCameraController>()
                .focus
                .length()
                - 4.2)
                .abs()
                < 0.0001
        );
    }
}

#[test]
fn blocked_update_discards_motion_and_scroll_and_requires_fresh_drag() {
    let (mut app, _, camera) = navigation_app();
    app.init_resource::<MenuState>();
    app.world_mut().resource_mut::<MenuState>().show_about = true;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    scroll(&mut app, MouseScrollUnit::Line, 5.0);
    navigation_tick(&mut app, Vec2::splat(50.0));
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut().resource_mut::<MenuState>().show_about = false;
    navigation_tick(&mut app, Vec2::ZERO);
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    assert_eq!(
        app.world().resource::<PreviewCameraController>().distance,
        140.0
    );
    assert!(!app.world().resource::<PreviewNavigationState>().looking);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    navigation_tick(&mut app, Vec2::ZERO);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(10.0));
    assert_ne!(*app.world().get::<Transform>(camera).unwrap(), before);
}

#[test]
fn frame_request_uses_single_proxy_or_origin_and_waits_while_blocked() {
    for count in [0, 1, 2] {
        let (mut app, _, camera) = navigation_app();
        let position = Vec3::new(42.0, 7.0, -12.0);
        for _ in 0..count {
            app.world_mut().spawn((
                EmitterTransformGizmoProxy,
                GlobalTransform::from_translation(position),
            ));
        }
        app.init_resource::<MenuState>();
        app.world_mut().resource_mut::<MenuState>().show_about = true;
        app.world_mut()
            .resource_mut::<PreviewCameraController>()
            .request_frame();
        navigation_tick(&mut app, Vec2::ZERO);
        assert!(
            app.world()
                .resource::<PreviewCameraController>()
                .frame_requested
        );
        app.world_mut().resource_mut::<MenuState>().show_about = false;
        navigation_tick(&mut app, Vec2::ZERO);
        let expected = if count == 1 { position } else { Vec3::ZERO };
        let controller = app.world().resource::<PreviewCameraController>();
        assert_eq!(controller.focus, expected);
        assert!(!controller.frame_requested);
        assert_eq!(
            *app.world().get::<Transform>(camera).unwrap(),
            preview_camera_transform(expected, 140.0, 0.0, DEFAULT_PREVIEW_PITCH)
        );
    }
}

#[derive(Resource, Default)]
struct CameraWrites(usize);

#[test]
fn idle_and_zero_input_do_not_rewrite_the_camera_or_unrelated_transform() {
    let (mut app, _, camera) = navigation_app();
    app.init_resource::<CameraWrites>().add_systems(
        Update,
        (|changed: Query<(), (With<PreviewRenderCamera>, Changed<Transform>)>,
          mut writes: ResMut<CameraWrites>| {
            writes.0 += changed.iter().count();
        })
        .after(navigate_preview_camera),
    );
    let unrelated = app
        .world_mut()
        .spawn(Transform::from_xyz(1.0, 2.0, 3.0))
        .id();
    navigation_tick(&mut app, Vec2::ZERO);
    let before = *app.world().get::<Transform>(camera).unwrap();
    app.world_mut().resource_mut::<CameraWrites>().0 = 0;
    for _ in 0..3 {
        navigation_tick(&mut app, Vec2::ZERO);
    }
    assert_eq!(app.world().resource::<CameraWrites>().0, 0);
    assert_eq!(*app.world().get::<Transform>(camera).unwrap(), before);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    navigation_tick(&mut app, Vec2::splat(5.0));
    assert_eq!(app.world().resource::<CameraWrites>().0, 1);
    assert_eq!(
        *app.world().get::<Transform>(unrelated).unwrap(),
        Transform::from_xyz(1.0, 2.0, 3.0)
    );
}

#[test]
fn ambiguous_canvas_defers_navigation_until_the_single_owner_returns() {
    let (mut app, _, _) = navigation_app();
    let extra = app
        .world_mut()
        .spawn((PreviewCanvas, RelativeCursorPosition::default()))
        .id();
    app.world_mut()
        .resource_mut::<PreviewCameraController>()
        .request_frame();
    navigation_tick(&mut app, Vec2::ZERO);
    assert!(
        app.world()
            .resource::<PreviewCameraController>()
            .frame_requested
    );
    app.world_mut().despawn(extra);
    navigation_tick(&mut app, Vec2::ZERO);
    assert!(
        !app.world()
            .resource::<PreviewCameraController>()
            .frame_requested
    );
}
