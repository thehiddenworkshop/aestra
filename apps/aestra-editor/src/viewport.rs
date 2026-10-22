//! Preview viewport ownership: rendering, navigation, grid, display modes, and gizmos.

#[cfg(test)]
use crate::timeline::TimelineState;
#[cfg(test)]
use crate::{FeathersActionButton, localization::Localizer, session::EditorSession};
#[cfg(test)]
use crate::{
    MenuState, PendingFeathersActivation, ProjectEffectCatalog,
    profiler::ProfilerState,
    settings::{EditorSettings, SettingsPersistence},
};
#[cfg(test)]
use aestra_authoring::{EffectCommand, EffectTransaction, SemanticTarget};
#[cfg(test)]
use aestra_bevy_render::PresentedEffect;
#[cfg(test)]
use aestra_bevy_render::{ActiveBackend, EffectRuntimeStatus};
#[cfg(test)]
use aestra_core::EmitterTransform;
#[cfg(test)]
use aestra_core::{EmitterId, EmitterShape, Value};
#[cfg(test)]
use aestra_runtime::CompiledEffectProject;
#[cfg(test)]
use bevy::gizmos::transform_gizmo::{
    TransformGizmoAxis, TransformGizmoFocus, TransformGizmoMode, TransformGizmoSettings,
    TransformGizmoSpace, TransformGizmoState,
};
#[cfg(test)]
use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
#[cfg(test)]
use bevy::prelude::*;
#[cfg(test)]
use bevy::ui_widgets::Activate;
#[cfg(test)]
use bevy::window::PrimaryWindow;
#[cfg(test)]
use bevy::{camera::visibility::RenderLayers, ui::RelativeCursorPosition};
#[cfg(test)]
use std::sync::Arc;

mod actions;
#[path = "viewport/control_input_020.rs"]
mod control_input;
mod gizmo_state;
mod host_motion;
pub(crate) use gizmo_state::EmitterTransformGizmoInteraction;
#[cfg(test)]
use gizmo_state::*;
mod shape_gizmo;
#[cfg(test)]
use shape_gizmo::*;
mod scene;
mod transform_gizmo;
mod workspace;
pub(crate) use actions::PreviewDisplayState;
#[cfg(test)]
use actions::{execute_viewport_action, queue_viewport_action_activation};
pub(crate) use transform_gizmo::emitter_transform_from_bevy;
#[cfg(test)]
use transform_gizmo::*;
pub(crate) use workspace::spawn_preview;
mod feedback;
mod keyboard;
#[cfg(test)]
use feedback::event_link_drops;
#[cfg(test)]
use feedback::{apply_preview_mocks, record_preview_outputs};
mod lifecycle;
#[cfg(test)]
use lifecycle::{configure_transform_gizmo_overlay_camera, sync_preview_camera_viewport};
mod navigation;
mod photographic;
mod presentation;
mod runtime_preview;
mod telemetry;
#[cfg(test)]
use photographic::{sync_photographic_overlay_composition, sync_preview_photographic_response};
#[cfg(test)]
use presentation::adaptive_grid_spacing;
#[cfg(test)]
use presentation::{
    PREVIEW_GRID_Y, PreviewGridPlane, PreviewPresentedEffect, preview_grid_uniform,
};
#[cfg(test)]
use presentation::{PreviewGridMaterial, sync_preview_grid};
#[cfg(test)]
use runtime_preview::{EditorPreviewProject, sync_project_preview, sync_rendered_preview};
#[cfg(test)]
use runtime_preview::{
    PreviewEffectInstancePath, compiled_effects_differ_only_in_live_edits,
    configured_preview_instance, configured_preview_player, desired_preview_instances,
};
#[cfg(test)]
use telemetry::update_preview;
mod shortcuts;
#[cfg(test)]
use crate::ModulePaletteState;
#[cfg(test)]
use keyboard::viewport_keyboard_input;
#[cfg(test)]
use navigation::{DEFAULT_PREVIEW_PITCH, PreviewRenderCamera, preview_camera_transform};
pub(crate) use navigation::{EmitterTransformGizmoProxy, PreviewCameraController};
#[cfg(test)]
use navigation::{PreviewNavigationState, navigate_preview_camera};
use shortcuts::PreviewCanvas;
pub(crate) use shortcuts::{PreviewDisplayMode, ViewportAction, ViewportSet};

mod plugin;
pub(crate) use plugin::ViewportPlugin;

#[cfg(test)]
pub(crate) fn install_project_preview_test_runtime(app: &mut App) {
    app.init_resource::<EditorPreviewProject>().add_systems(
        Update,
        sync_project_preview.before(crate::project_content::io::poll),
    );
}

#[cfg(test)]
fn presented_playhead_time(player: &PresentedEffect) -> f32 {
    let effect = player.effect();
    if effect.playback_mode.is_looping() && effect.duration > 0.0 {
        player.simulation_time().rem_euclid(effect.duration)
    } else {
        player
            .simulation_time()
            .clamp(0.0, effect.duration.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn photographic_response_updates_restores_and_isolates_the_effect_camera() {
        use bevy::{
            camera::Hdr,
            core_pipeline::tonemapping::{DebandDither, Tonemapping},
            post_process::bloom::Bloom,
            render::view::ColorGrading,
        };
        let mut app = App::new();
        app.init_resource::<EditorSettings>().add_systems(
            Update,
            (
                sync_preview_photographic_response,
                sync_photographic_overlay_composition,
            )
                .chain(),
        );
        let effect = app
            .world_mut()
            .spawn((Camera3d::default(), PreviewRenderCamera))
            .id();
        let overlay = app
            .world_mut()
            .spawn((Camera3d::default(), RenderLayers::layer(15)))
            .id();
        let thumbnail = app.world_mut().spawn(Camera3d::default()).id();
        let ui = app.world_mut().spawn((Camera2d, IsDefaultUiCamera)).id();
        app.update();
        assert!(app.world().get::<Hdr>(effect).is_none());
        {
            let mut settings = app.world_mut().resource_mut::<EditorSettings>();
            settings.preview.photographic_enabled = true;
            settings.preview.photographic.exposure_stops = 2.0;
        }
        app.update();
        assert!(app.world().get::<Hdr>(effect).is_some());
        assert!(app.world().get::<Bloom>(effect).is_some());
        assert_eq!(
            app.world()
                .get::<ColorGrading>(effect)
                .unwrap()
                .global
                .exposure,
            2.0
        );
        for camera in [overlay, thumbnail] {
            assert!(app.world().get::<Hdr>(camera).is_none());
            assert!(app.world().get::<ColorGrading>(camera).is_none());
        }
        for camera in [overlay, ui] {
            let camera = app.world().get::<Camera>(camera).unwrap();
            assert!(
                matches!(camera.clear_color, ClearColorConfig::Custom(color) if color == Color::NONE)
            );
            assert!(matches!(
                camera.output_mode,
                bevy::camera::CameraOutputMode::Write {
                    blend_state: Some(_),
                    clear_color: ClearColorConfig::None
                }
            ));
        }
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .preview
            .photographic
            .bloom_intensity = 0.0;
        app.update();
        assert!(app.world().get::<Bloom>(effect).is_none());
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .preview
            .photographic_enabled = false;
        app.update();
        assert!(app.world().get::<Hdr>(effect).is_none());
        assert!(app.world().get::<Bloom>(effect).is_none());
        assert_eq!(
            app.world()
                .get::<ColorGrading>(effect)
                .unwrap()
                .global
                .exposure,
            0.0
        );
        assert_eq!(
            *app.world().get::<Tonemapping>(effect).unwrap(),
            Tonemapping::default()
        );
        assert_eq!(
            *app.world().get::<DebandDither>(effect).unwrap(),
            DebandDither::Enabled
        );
        for camera in [overlay, ui] {
            assert!(matches!(
                app.world().get::<Camera>(camera).unwrap().clear_color,
                ClearColorConfig::None
            ));
        }
        // A restored or recreated viewport camera receives the already-enabled profile.
        app.world_mut()
            .resource_mut::<EditorSettings>()
            .preview
            .photographic_enabled = true;
        let recreated = app
            .world_mut()
            .spawn((Camera3d::default(), PreviewRenderCamera))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<ColorGrading>(recreated)
                .unwrap()
                .global
                .exposure,
            2.0
        );
        assert!(app.world().get::<Hdr>(recreated).is_some());
    }

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
    use crate::test_support;
    use aestra_core::EffectPlaybackMode;

    #[test]
    fn preview_transform_gizmo_is_larger_and_easier_to_hit_than_the_bevy_default() {
        let defaults = TransformGizmoSettings::default();
        let preview = preview_transform_gizmo_settings();

        assert_eq!(
            preview.screen_scale_factor,
            PREVIEW_TRANSFORM_GIZMO_SCREEN_SCALE
        );
        assert_eq!(
            preview.axis_hit_distance,
            PREVIEW_TRANSFORM_GIZMO_HIT_DISTANCE
        );
        assert!(preview.screen_scale_factor > defaults.screen_scale_factor);
        assert!(preview.axis_hit_distance > defaults.axis_hit_distance);
    }

    #[test]
    fn transform_snap_rounds_absolute_values_instead_of_drag_deltas() {
        let transform = EmitterTransform {
            translation: [0.695_576, 2.4, -3.6],
            ..default()
        };

        let snapped = snap_transform_absolute(
            TransformGizmoMode::Translate,
            Some(TransformGizmoAxis::X),
            transform,
            false,
        );

        assert_eq!(snapped.translation, [1.0, 2.4, -3.6]);
    }

    #[test]
    fn precise_transform_snap_uses_the_finer_absolute_grid() {
        let transform = EmitterTransform {
            translation: [0.695_576, 0.0, 0.0],
            ..default()
        };

        let snapped = snap_transform_absolute(
            TransformGizmoMode::Translate,
            Some(TransformGizmoAxis::X),
            transform,
            true,
        );

        assert_eq!(snapped.translation, [0.7, 0.0, 0.0]);
    }

    #[test]
    fn precision_drag_scales_motion_without_changing_unedited_components() {
        let original = EmitterTransform::default();
        let mut raw = original;
        raw.translation = [10.0, -5.0, 2.0];
        let precise = transform_with_drag_precision(
            TransformGizmoMode::Translate,
            original,
            original,
            raw,
            PREVIEW_TRANSFORM_PRECISION_FACTOR,
        );

        assert_eq!(precise.translation, [1.0, -0.5, 0.2]);
        assert_eq!(precise.rotation, original.rotation);
        assert_eq!(precise.scale, original.scale);
    }

    #[test]
    fn precision_reanchor_keeps_modifier_transitions_continuous() {
        let raw_anchor = EmitterTransform {
            translation: [10.0, 0.0, 0.0],
            ..default()
        };
        let output_anchor = EmitterTransform {
            translation: [4.0, 0.0, 0.0],
            ..default()
        };
        let raw_current = EmitterTransform {
            translation: [20.0, 0.0, 0.0],
            ..default()
        };

        let precise = transform_with_drag_precision(
            TransformGizmoMode::Translate,
            raw_anchor,
            output_anchor,
            raw_current,
            PREVIEW_TRANSFORM_PRECISION_FACTOR,
        );

        assert_eq!(precise.translation, [5.0, 0.0, 0.0]);
    }

    #[test]
    fn gizmo_value_readout_exposes_active_modifiers() {
        let current = EmitterTransform {
            translation: [2.123_456, 3.0, 4.0],
            ..default()
        };

        assert_eq!(
            transform_gizmo_value_text(
                TransformGizmoMode::Translate,
                Some(TransformGizmoAxis::X),
                current,
                false,
                true,
            ),
            "MOVE X  +2.1235  \u{00b7} PRECISE"
        );
    }

    fn viewport_action_app() -> (App, tempfile::TempDir) {
        let temporary = tempfile::tempdir().expect("temporary settings directory should exist");
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack())
            .init_resource::<MenuState>()
            .init_resource::<EditorSettings>()
            .insert_resource(SettingsPersistence::for_test(
                temporary.path().join("settings.ron"),
            ))
            .insert_resource(Localizer::new("en-US").expect("test locale should load"))
            .init_resource::<PreviewCameraController>()
            .init_resource::<PreviewDisplayState>()
            .init_resource::<TransformGizmoSettings>()
            .init_resource::<crate::docking::MaximizedPanel>()
            .add_observer(execute_viewport_action);
        (app, temporary)
    }

    #[test]
    fn viewport_actions_own_grid_framing_gizmo_and_display_modes() {
        let (mut app, _temporary) = viewport_action_app();

        app.world_mut().trigger(ViewportAction::ToggleGrid);
        app.update();
        assert!(!app.world().resource::<MenuState>().show_grid);
        assert!(!app.world().resource::<EditorSettings>().preview.show_grid);

        app.world_mut().trigger(ViewportAction::FramePreview);
        app.update();
        assert!(
            app.world()
                .resource::<PreviewCameraController>()
                .frame_requested
        );

        app.world_mut()
            .trigger(ViewportAction::SetTransformGizmoMode(
                TransformGizmoMode::Rotate,
            ));
        app.world_mut()
            .trigger(ViewportAction::SetPreviewDisplayMode(
                PreviewDisplayMode::Wireframe,
            ));
        app.update();
        assert_eq!(
            app.world().resource::<TransformGizmoSettings>().mode,
            TransformGizmoMode::Rotate
        );
        assert_eq!(
            app.world().resource::<PreviewDisplayState>().mode,
            PreviewDisplayMode::Wireframe
        );
    }

    #[test]
    fn toggle_maximize_flips_the_maximized_viewport_panel() {
        use crate::docking::{MaximizedPanel, ToolPanel};
        let (mut app, _temporary) = viewport_action_app();

        app.world_mut().trigger(ViewportAction::ToggleMaximize);
        app.update();
        assert_eq!(
            app.world().resource::<MaximizedPanel>().0,
            Some(ToolPanel::Viewport)
        );

        app.world_mut().trigger(ViewportAction::ToggleMaximize);
        app.update();
        assert_eq!(app.world().resource::<MaximizedPanel>().0, None);
    }

    #[test]
    fn grid_shortcut_routes_through_the_viewport_action_contract() {
        let (mut app, _temporary) = viewport_action_app();
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyG);
        app.insert_resource(keys)
            .init_resource::<ModulePaletteState>()
            .add_systems(Update, viewport_keyboard_input);

        app.update();

        assert!(!app.world().resource::<MenuState>().show_grid);
        assert!(!app.world().resource::<EditorSettings>().preview.show_grid);
    }

    #[test]
    fn feathers_activation_queues_one_viewport_action() {
        let mut app = App::new();
        app.add_observer(queue_viewport_action_activation);
        let action = app
            .world_mut()
            .spawn((ViewportAction::FramePreview, FeathersActionButton))
            .id();

        app.world_mut().trigger(Activate { entity: action });
        app.update();

        let action = app.world().entity(action);
        assert!(action.contains::<PendingFeathersActivation>());
        assert!(action.contains::<bevy::ui::Pressed>());
    }

    #[test]
    fn preview_cameras_are_disabled_without_an_active_viewport_canvas() {
        let mut app = App::new();
        app.add_systems(Update, sync_preview_camera_viewport);
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        let preview = app
            .world_mut()
            .spawn((PreviewRenderCamera, Camera::default()))
            .id();
        let overlay = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera::default(),
                RenderLayers::layer(15),
            ))
            .id();

        app.update();

        assert!(!app.world().get::<Camera>(preview).unwrap().is_active);
        assert!(!app.world().get::<Camera>(overlay).unwrap().is_active);
    }

    #[test]
    fn transform_gizmo_overlay_preserves_the_preview_color_buffer() {
        let mut app = App::new();
        app.add_systems(Update, configure_transform_gizmo_overlay_camera);
        let overlay = app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera::default(),
                RenderLayers::layer(15),
            ))
            .id();
        let preview = app
            .world_mut()
            .spawn((
                PreviewRenderCamera,
                Camera3d::default(),
                Camera::default(),
                RenderLayers::layer(0),
            ))
            .id();

        app.update();

        assert!(matches!(
            app.world().get::<Camera>(overlay).unwrap().clear_color,
            ClearColorConfig::None
        ));
        assert_eq!(app.world().get::<Camera>(overlay).unwrap().order, 1);
        assert!(app.world().get::<Camera>(overlay).unwrap().is_active);
        assert!(!matches!(
            app.world().get::<Camera>(preview).unwrap().clear_color,
            ClearColorConfig::None
        ));
    }

    #[test]
    fn transform_gizmo_materials_render_after_transparent_particles() {
        let mut app = App::new();
        app.init_resource::<Assets<StandardMaterial>>()
            .add_systems(Update, configure_transform_gizmo_overlay_materials);
        let overlay_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let scene_material = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let overlay_root = app.world_mut().spawn_empty().id();
        let scene_root = app.world_mut().spawn_empty().id();
        app.world_mut().spawn((
            RenderLayers::layer(15),
            MeshMaterial3d(overlay_material.clone()),
            ChildOf(overlay_root),
        ));
        app.world_mut().spawn((
            RenderLayers::layer(0),
            MeshMaterial3d(scene_material.clone()),
            ChildOf(scene_root),
        ));

        app.update();

        let materials = app.world().resource::<Assets<StandardMaterial>>();
        let overlay = materials.get(&overlay_material).unwrap();
        assert_eq!(overlay.alpha_mode, AlphaMode::Blend);
        assert_eq!(overlay.depth_bias, 10_000.0);
        let scene = materials.get(&scene_material).unwrap();
        assert_eq!(scene.alpha_mode, AlphaMode::Opaque);
        assert_eq!(scene.depth_bias, 0.0);
        assert!(
            app.world()
                .entity(overlay_root)
                .contains::<TransformGizmoVisualRoot>()
        );
        assert!(
            !app.world()
                .entity(scene_root)
                .contains::<TransformGizmoVisualRoot>()
        );
    }

    #[test]
    fn transform_gizmo_scale_feedback_stretches_only_the_dragged_axis() {
        let original = Transform::from_scale(Vec3::new(2.0, 3.0, 4.0));
        let current = Transform::from_scale(Vec3::new(5.0, 3.0, 4.0));

        let visual = transform_gizmo_drag_visual(
            TransformGizmoMode::Scale,
            TransformGizmoSpace::World,
            Some(TransformGizmoAxis::X),
            original,
            current,
        );

        assert_eq!(visual.scale, Vec3::new(2.5, 1.0, 1.0));
        assert!(visual.rotation.is_none());
    }

    #[test]
    fn transform_gizmo_rotation_feedback_uses_the_live_drag_delta() {
        let original = Transform::from_rotation(Quat::from_rotation_z(30.0_f32.to_radians()));
        let current = Transform::from_rotation(Quat::from_rotation_z(75.0_f32.to_radians()));

        let visual = transform_gizmo_drag_visual(
            TransformGizmoMode::Rotate,
            TransformGizmoSpace::World,
            Some(TransformGizmoAxis::Z),
            original,
            current,
        );

        let rotation = visual.rotation.unwrap();
        assert!(rotation.angle_between(Quat::from_rotation_z(45.0_f32.to_radians())) < 0.0001);
        assert_eq!(visual.scale, Vec3::ONE);
    }

    #[test]
    fn preview_grid_stays_at_world_origin_after_pan_and_zoom() {
        let mut app = App::new();
        app.init_resource::<MenuState>()
            .init_resource::<PreviewCameraController>()
            .init_resource::<Assets<PreviewGridMaterial>>()
            .add_systems(Update, sync_preview_grid);
        app.world_mut().spawn((
            PreviewRenderCamera,
            GlobalTransform::from(preview_camera_transform(
                Vec3::ZERO,
                140.0,
                0.0,
                DEFAULT_PREVIEW_PITCH,
            )),
        ));
        let material = app
            .world_mut()
            .resource_mut::<Assets<PreviewGridMaterial>>()
            .add(PreviewGridMaterial {
                grid: preview_grid_uniform(140.0, 1.0),
            });
        let grid = app
            .world_mut()
            .spawn((
                PreviewGridPlane,
                Transform::default(),
                Visibility::Inherited,
                MeshMaterial3d(material.clone()),
            ))
            .id();

        for distance in [140.0, 20.0, 600.0, 140.0] {
            {
                let mut controller = app.world_mut().resource_mut::<PreviewCameraController>();
                controller.focus = Vec3::new(120.0, 80.0, -90.0);
                controller.distance = distance;
            }
            app.update();
            assert_eq!(
                app.world().get::<Transform>(grid).unwrap().translation,
                Vec3::new(0.0, PREVIEW_GRID_Y, 0.0),
            );
            let materials = app.world().resource::<Assets<PreviewGridMaterial>>();
            assert_eq!(materials.get(&material).unwrap().grid.focus, Vec4::ZERO);
        }
    }

    #[test]
    fn adaptive_grid_spacing_uses_stable_editor_scale_steps() {
        assert!((adaptive_grid_spacing(0.04) - 0.05).abs() < 0.000_001);
        assert_eq!(adaptive_grid_spacing(0.7), 1.0);
        assert_eq!(adaptive_grid_spacing(14.0), 20.0);
        assert_eq!(adaptive_grid_spacing(260.0), 500.0);
        assert_eq!(adaptive_grid_spacing(f32::NAN), 1.0);
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

    #[test]
    fn shape_gizmo_drag_preserves_unedited_cone_component() {
        let cone = EmitterShape::Cone {
            radius: 12.0,
            depth: 24.0,
        };

        assert_eq!(
            shape_after_gizmo_drag(cone, ShapeGizmoHandle::Radius, 18.0),
            EmitterShape::Cone {
                radius: 18.0,
                depth: 24.0,
            }
        );
        assert_eq!(
            shape_after_gizmo_drag(cone, ShapeGizmoHandle::Depth, 31.0),
            EmitterShape::Cone {
                radius: 12.0,
                depth: 31.0,
            }
        );
    }

    #[test]
    fn shape_gizmo_drag_clamps_degenerate_values() {
        assert_eq!(
            shape_after_gizmo_drag(
                EmitterShape::Circle { radius: 12.0 },
                ShapeGizmoHandle::Radius,
                0.0,
            ),
            EmitterShape::Circle { radius: 0.1 }
        );
        assert_eq!(
            shape_after_gizmo_drag(
                EmitterShape::Cone {
                    radius: 12.0,
                    depth: 24.0,
                },
                ShapeGizmoHandle::Depth,
                0.0,
            ),
            EmitterShape::Cone {
                radius: 12.0,
                depth: 0.1,
            }
        );
    }

    #[test]
    fn shape_gizmo_edits_volumetric_dimensions_independently() {
        let box_shape = EmitterShape::Box {
            half_extents: [2.0, 3.0, 4.0],
        };
        assert_eq!(
            shape_after_gizmo_drag(box_shape, ShapeGizmoHandle::ExtentZ, 9.0),
            EmitterShape::Box {
                half_extents: [2.0, 3.0, 9.0],
            }
        );
        assert_eq!(
            shape_after_gizmo_drag(
                EmitterShape::Cylinder {
                    radius: 5.0,
                    depth: 8.0,
                },
                ShapeGizmoHandle::Depth,
                12.0,
            ),
            EmitterShape::Cylinder {
                radius: 5.0,
                depth: 12.0,
            }
        );
    }

    #[test]
    fn empty_effect_disables_emitter_gizmo_but_keeps_clip_transforms() {
        let session = EditorSession::from_test_effect(aestra_core::EffectAsset::new("Empty", 4.0));
        assert!(selected_preview_transform(&session, None).is_none());
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ShapeGizmoState>()
            .init_resource::<EmitterTransformGizmoInteraction>()
            .add_systems(Update, sync_transform_gizmo_focus);
        let proxy = app
            .world_mut()
            .spawn((EmitterTransformGizmoProxy, TransformGizmoFocus))
            .id();
        app.update();
        assert!(app.world().get::<TransformGizmoFocus>(proxy).is_none());
        let mut clip = aestra_core::EffectClip::new(aestra_core::EffectId::new(), 0.0, 1.0);
        clip.transform.translation = [1.0, 2.0, 3.0];
        let id = clip.id;
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        session.effect.effect_clips.push(clip);
        session.selection.select_effect_clip(id);
        let (target, transform) = selected_preview_transform(&session, None).unwrap();
        assert!(matches!(target, PreviewTransformTarget::EffectClip(selected) if selected == id));
        assert_eq!(transform.translation, [1.0, 2.0, 3.0]);
        app.update();
        assert!(app.world().get::<TransformGizmoFocus>(proxy).is_some());
    }

    #[test]
    fn a_selected_fluid_source_is_moved_by_the_gizmo_without_restarting_the_fluid() {
        aestra_fluid::link();
        let registry = aestra_compiler::ExtensionRegistry::linked();
        let mut session = EditorSession::from_test_effect(aestra_fluid::fire_effect(&registry));
        let source = session.effect.simulation_stages[0]
            .modules
            .iter()
            .find(|module| module.module_type.0 == aestra_fluid::MODULE_DENSITY_SOURCE)
            .unwrap()
            .id;
        session.selection.primary = SemanticTarget::Module(source);
        let (target, transform) =
            selected_preview_transform(&session, Some(&registry.modules)).unwrap();
        assert_eq!(
            target,
            PreviewTransformTarget::ModuleHandle(source, "position")
        );
        assert_eq!(transform.translation, [0.0, 15.0, 0.0]);

        // Dragging edits the domain's source through the effect scope, as one live preview.
        let before = session.preview().unwrap().effect().clone();
        let moved = EmitterTransform {
            translation: [20.0, 10.0, 0.0],
            ..default()
        };
        let command = preview_transform_command(target, moved, &session).unwrap();
        assert!(matches!(
            &command,
            EffectCommand::SetModuleParameter { emitter, parameter, .. }
                if *emitter == EmitterId::EFFECT_SCOPE && parameter == "position"
        ));
        assert!(
            session.preview_interaction(EffectTransaction::single("Preview transform", command))
        );
        let after = session.preview().unwrap().effect();
        assert_ne!(
            before.extension_stages[0].block.constants,
            after.extension_stages[0].block.constants
        );
        assert!(
            compiled_effects_differ_only_in_live_edits(&before, after),
            "the player is swapped in place, so the running fluid is kept"
        );

        // A position a host object drives offers no handle.
        let module = session.effect.simulation_stages[0]
            .modules
            .iter_mut()
            .find(|module| module.id == source)
            .unwrap();
        module
            .property_sources
            .insert("position".into(), aestra_core::PropertySource::HostBinding);
        assert!(!matches!(
            selected_preview_transform(&session, Some(&registry.modules)),
            Some((PreviewTransformTarget::ModuleHandle(..), _))
        ));
    }

    #[test]
    fn selecting_shape_module_keeps_root_transform_gizmo_available() {
        let mut session = test_support::session_with_timing_slack();
        let shape_module = session
            .selected_layer()
            .unwrap()
            .module_by_type(aestra_core::MODULE_SHAPE)
            .unwrap()
            .id;
        session.selection.primary = SemanticTarget::Module(shape_module);
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ShapeGizmoState>()
            .init_resource::<EmitterTransformGizmoInteraction>()
            .add_systems(Update, sync_transform_gizmo_focus);
        let player = app.world_mut().spawn(EmitterTransformGizmoProxy).id();

        app.update();

        assert!(app.world().entity(player).contains::<TransformGizmoFocus>());
    }

    #[test]
    fn emitter_gizmo_previews_then_commits_one_undoable_transform() {
        let session = test_support::session_with_timing_slack();
        let gizmo = TransformGizmoState {
            active: true,
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(gizmo)
            .insert_resource(preview_transform_gizmo_settings())
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<EmitterTransformGizmoInteraction>()
            .add_systems(Update, update_emitter_transform_gizmo);
        app.world_mut().spawn((
            EmitterTransformGizmoProxy,
            Transform::from_xyz(4.0, 5.0, 6.0),
        ));

        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            session.selected_layer().unwrap().transform,
            EmitterTransform::default()
        );
        assert_eq!(
            session.preview().unwrap().effect().emitters[0]
                .transform
                .translation,
            [4.0, 5.0, 6.0],
            "drag preview must update the compiled effect before release"
        );

        app.world_mut().resource_mut::<TransformGizmoState>().active = false;
        app.update();
        let session = app.world().resource::<EditorSession>();
        assert_eq!(
            session.selected_layer().unwrap().transform.translation,
            [4.0, 5.0, 6.0]
        );

        app.world_mut().resource_mut::<EditorSession>().undo();
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .selected_layer()
                .unwrap()
                .transform,
            EmitterTransform::default()
        );
    }

    #[test]
    fn editor_preview_player_uses_the_compiled_effect_timeline_and_seed() {
        let mut session = test_support::session_with_timing_slack();
        session.preview_seed = 42;
        session
            .driver
            .clock
            .seek_frame(37, session.playback_duration());

        let player = configured_preview_player(&session).unwrap();

        assert!(std::sync::Arc::ptr_eq(
            player.effect(),
            session.preview().unwrap().effect()
        ));
        assert_eq!(player.simulation_time(), session.simulation_time());
        assert_eq!(player.instance.seed(), 42);
    }

    #[test]
    fn transform_only_preview_updates_the_existing_player_in_place() {
        let mut session = test_support::session_with_timing_slack();
        let player = configured_preview_player(&session).unwrap();
        let emitter = session.selected_layer().unwrap().id;
        let transform = EmitterTransform {
            translation: [7.0, -3.0, 2.0],
            ..default()
        };
        assert!(session.preview_interaction(EffectTransaction::single(
            "Preview emitter transform",
            EffectCommand::SetEmitterTransform {
                id: emitter,
                transform,
            },
        )));

        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ProjectEffectCatalog>()
            .init_resource::<EditorPreviewProject>()
            .init_resource::<TimelineState>()
            .add_systems(
                Update,
                (sync_project_preview, sync_rendered_preview).chain(),
            );
        let player_entity = app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath::default(),
                player,
            ))
            .id();

        app.update();

        let world = app.world();
        let player = world.get::<PresentedEffect>(player_entity).unwrap();
        assert_eq!(player.effect().emitters[0].transform, transform);
        assert!(std::sync::Arc::ptr_eq(
            player.effect(),
            world
                .resource::<EditorSession>()
                .preview()
                .unwrap()
                .effect()
        ));
    }

    #[test]
    fn the_preview_keeps_its_latest_root_outputs() {
        use aestra_bevy_render::gpu::AestraOutputEvent;
        let mut app = App::new();
        app.insert_resource(test_support::session_with_timing_slack())
            .add_message::<AestraOutputEvent>()
            .add_systems(Update, record_preview_outputs);
        let root = app
            .world_mut()
            .spawn((PreviewPresentedEffect, PreviewEffectInstancePath::default()))
            .id();
        let nested = app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath(vec![aestra_core::EffectClipId::new()]),
            ))
            .id();
        let event = |tick| {
            aestra_runtime::EffectOutputEvent::new(
                aestra_runtime::EVENT_IMPACT,
                aestra_runtime::EventOrigin::Effect,
                "",
                Vec::new(),
                1.0,
                tick,
            )
        };
        for tick in 0..10 {
            app.world_mut()
                .write_message(AestraOutputEvent::root(root, event(tick)));
        }
        app.world_mut()
            .write_message(AestraOutputEvent::root(nested, event(99)));
        app.update();
        let session = app.world().resource::<EditorSession>();
        let ticks: Vec<u64> = session
            .preview_outputs
            .iter()
            .map(|event| event.tick)
            .collect();
        assert_eq!(
            ticks,
            (2..10).collect::<Vec<_>>(),
            "the latest eight, root only"
        );
        assert_eq!(session.preview_outputs_heard, 10);
    }

    #[test]
    fn gpu_link_drops_map_back_to_authored_links() {
        let mut session = test_support::session_with_timing_slack();
        session.duplicate_selected_layer();
        let (rocket, stars) = (session.effect.emitters[0].id, session.effect.emitters[1].id);
        session.selection.primary = SemanticTarget::Emitter(rocket);
        let id = session
            .add_event_link(aestra_core::EventTrigger::OnDeath, stars)
            .unwrap();
        let compiled = session.preview().unwrap().effect().clone();
        let statistics = |dropped: u64| aestra_bevy_render::gpu::GpuEventLinkStatistics {
            dropped: vec![dropped],
            ..default()
        };
        assert_eq!(
            event_link_drops(&session.effect, &compiled, &statistics(3072)),
            std::collections::BTreeMap::from([(id, 3072)])
        );
        assert!(event_link_drops(&session.effect, &compiled, &statistics(0)).is_empty());
    }

    #[test]
    fn preview_stand_ins_drive_the_edited_effect_and_are_never_saved() {
        let mut session = test_support::session_with_timing_slack();
        let target =
            aestra_core::EffectBinding::spatial("Target", aestra_core::BindingUpdateMode::Live);
        let id = target.id;
        assert!(session.execute(
            "Add binding",
            EffectCommand::AddBinding {
                binding: target,
                index: 0,
            },
            false,
        ));
        session
            .preview_mocks
            .insert(id, crate::preview_mocks::PreviewMock::Orbit);
        let player = configured_preview_player(&session).unwrap();

        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ProjectEffectCatalog>()
            .init_resource::<EditorPreviewProject>()
            .init_resource::<TimelineState>()
            .add_systems(
                Update,
                (
                    sync_project_preview,
                    sync_rendered_preview,
                    apply_preview_mocks,
                )
                    .chain(),
            );
        let player_entity = app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath::default(),
                player,
            ))
            .id();
        app.update();

        let traced = |app: &App| {
            let world = app.world();
            let player = world.get::<PresentedEffect>(player_entity).unwrap();
            let session = world.resource::<EditorSession>();
            (
                player
                    .instance
                    .binding_trace()
                    .map(|trace| trace.identity()),
                session
                    .preview()
                    .unwrap()
                    .binding_trace()
                    .map(|trace| trace.identity()),
            )
        };
        let (presented, edited) = traced(&app);
        assert!(
            presented.is_some(),
            "the rendered preview reads the stand-in"
        );
        assert_eq!(presented, edited, "both previews read the same recording");
        let saved = app
            .world()
            .resource::<EditorSession>()
            .effect
            .to_pretty_ron()
            .unwrap();
        assert!(!saved.contains("Orbit"), "a stand-in is never saved");

        app.world_mut()
            .resource_mut::<EditorSession>()
            .preview_mocks
            .clear();
        app.update();
        assert_eq!(traced(&app), (None, None));
    }

    #[test]
    fn live_player_replacement_rejects_structural_compiler_changes() {
        let session = test_support::session_with_timing_slack();
        let current = session.preview().unwrap().effect();
        let mut transformed = current.as_ref().clone();
        transformed.emitters[0].transform.translation[0] = 4.0;
        assert!(compiled_effects_differ_only_in_live_edits(
            current,
            &transformed
        ));

        transformed.emitters[0].duration += 0.25;
        assert!(!compiled_effects_differ_only_in_live_edits(
            current,
            &transformed
        ));
    }

    #[test]
    fn project_preview_collects_active_effect_clips_with_local_time_and_seed() {
        let temporary = tempfile::tempdir().unwrap();
        let mut child = aestra_core::EffectAsset::new("Child", 1.0);
        let parameter = aestra_core::EffectParameter {
            id: aestra_core::ParameterId::new(),
            name: "Spawn Rate".into(),
            default: Value::Scalar(4.0),
            exposed: true,
        };
        let parameter_id = parameter.id;
        let mut emitter = aestra_core::Emitter::basic_sprite("Child emitter", 1.0);
        emitter
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == aestra_core::MODULE_EMISSION)
            .unwrap()
            .bindings
            .insert("spawn_rate".into(), parameter_id);
        child.parameters.push(parameter);
        child.emitters.push(emitter);
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();

        let mut root = aestra_core::EffectAsset::new("Root", 2.0);
        root.playback_mode = EffectPlaybackMode::LoopContinuous;
        let mut clip = aestra_core::EffectClip::new(child.id, 0.5, 1.0);
        clip.source_offset = 0.1;
        clip.seed = aestra_core::EffectClipSeed::Fixed(77);
        clip.transform.translation = [5.0, 2.0, -1.0];
        clip.parameter_overrides
            .insert(parameter_id, Value::Scalar(20.0));
        let clip_id = clip.id;
        root.effect_clips.push(clip);
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        let project = Arc::new(catalog.compile_project(&root).unwrap());
        let preview = EditorPreviewProject {
            source_root: Some(project.root.clone()),
            project: Some(project),
            ..default()
        };
        let timeline = TimelineState::framed(root.duration);

        let before = desired_preview_instances(&preview, &timeline, 0.25, 9);
        assert_eq!(before.len(), 1, "the child must not run before its clip");

        let active = desired_preview_instances(&preview, &timeline, 0.75, 9);
        assert_eq!(active.len(), 2);
        assert!(active[0].path.is_empty());
        assert_eq!(active[1].path, vec![clip_id]);
        assert_eq!(active[1].effect.source, child.id);
        assert!((active[1].time - 0.35).abs() < 0.000_1);
        assert_eq!(active[1].seed, 77);
        let context = aestra_runtime::HostTransformContext {
            motion: active[1].effect.host_transform_track.clone(),
            inherited: active[1].inherited.clone(),
        };
        assert_eq!(
            Mat4::from_cols_array(&context.matrix_at(active[1].time))
                .w_axis
                .truncate(),
            Vec3::new(5.0, 2.0, -1.0)
        );
        assert_eq!(active[1].parameter_overrides.len(), 1);
        let player = configured_preview_instance(
            active[1].effect.clone(),
            active[1].time,
            active[1].seed,
            &active[1].parameter_overrides,
        );
        assert_eq!(
            player.instance.parameter(parameter_id),
            Some(&aestra_runtime::RuntimeValue::Scalar(20.0))
        );

        let repeated = desired_preview_instances(&preview, &timeline, 2.75, 9);
        assert_eq!(repeated.len(), 2);
        assert_eq!(repeated[0].time, 2.75);
        assert!((repeated[1].time - 0.35).abs() < 0.000_1);
    }

    #[test]
    fn nested_preview_matches_reference_motion_across_parent_loops_and_retiming() {
        let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
        let mut root = aestra_core::EffectAsset::load_ron(
            assets.join("effects/nested_moving_trail_lab.aestra.ron"),
        )
        .unwrap();
        let catalog = ProjectEffectCatalog::scan(&assets);
        let timeline = TimelineState::framed(root.duration);
        let mut previous_context = None;
        for offset in [0.75, 1.0] {
            root.effect_clips[0].source_offset = offset;
            let project = Arc::new(catalog.compile_project(&root).unwrap());
            let preview = EditorPreviewProject {
                source_root: Some(project.root.clone()),
                project: Some(project.clone()),
                ..default()
            };
            for time in [1.0, 3.0, 5.0, 7.0, 9.0, 1.0] {
                let desired = desired_preview_instances(&preview, &timeline, time, 23);
                let leaf = desired.last().unwrap();
                assert_eq!(leaf.path.len(), 2);
                assert_eq!(leaf.transform, Transform::IDENTITY);
                let context = aestra_runtime::HostTransformContext {
                    motion: leaf.effect.host_transform_track.clone(),
                    inherited: leaf.inherited.clone(),
                };
                let mut samples = Vec::new();
                project.evaluate(time, 23, &mut samples);
                assert!(!samples.is_empty());
                for sample in samples {
                    assert_eq!(sample.instance_path, leaf.path);
                    assert_eq!(sample.world_from_effect, context.matrix_at(leaf.time));
                }
                if time == 1.0 && offset == 1.0 {
                    assert_ne!(
                        previous_context.as_ref(),
                        Some(&context),
                        "retiming changes replay ancestry"
                    );
                }
                if offset == 0.75 {
                    previous_context = Some(context);
                }
            }
        }
    }

    #[test]
    fn project_preview_error_identifies_a_missing_effect_reference() {
        let directory = tempfile::tempdir().unwrap();
        let mut session = test_support::session_with_timing_slack();
        let missing = aestra_core::EffectId::from_u128(0xfeed_cafe);
        session
            .effect
            .effect_clips
            .push(aestra_core::EffectClip::new(missing, 0.0, 0.5));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(directory.path()))
            .init_resource::<EditorPreviewProject>()
            .add_systems(Update, sync_project_preview);

        app.update();

        let preview = app.world().resource::<EditorPreviewProject>();
        let error = preview.error.as_deref().expect("missing source must fail");
        assert!(error.contains(&missing.to_string()), "{error}");
        assert!(
            preview.project.is_some(),
            "the valid root remains previewable"
        );
    }

    #[test]
    fn editor_preview_keeps_authored_motion_across_seek_and_recompile() {
        let mut effect = aestra_core::EffectAsset::from_ron(include_str!(
            "../../../assets/test/effects/moving_trail_lab.aestra.ron"
        ))
        .unwrap();
        let program = aestra_core::material::MaterialProgram::from_ron(include_str!(
            "../../../assets/test/materials/trail_lab.aestra.material.ron"
        ))
        .unwrap();
        let programs = std::collections::BTreeMap::from([(program.id, program)]);
        let compile = |effect: &aestra_core::EffectAsset| {
            Arc::new(
                aestra_compiler::EffectCompiler::default()
                    .compile_with_material_programs(effect, &programs)
                    .unwrap(),
            )
        };
        let compiled = compile(&effect);
        for time in [0.0, 1.5, 4.5, 0.75] {
            let player = configured_preview_instance(compiled.clone(), time, 42, &[]);
            assert_eq!(
                player.instance.host_transform_at(time),
                compiled.host_transform_track.as_ref().unwrap().sample(time)
            );
            assert_eq!(player.simulation_time(), time);
        }
        let replacement = aestra_core::HostTransformTrack::from_pose_keys(
            vec![aestra_core::HostTransformKey {
                time: 0.0,
                transform: aestra_core::EmitterTransform {
                    translation: [50.0, 0.0, 0.0],
                    ..default()
                },
            }],
            false,
        );
        let undo = aestra_authoring::CommandExecutor::execute(
            &mut effect,
            &default(),
            &aestra_authoring::EffectTransaction::single(
                "Edit host motion",
                aestra_authoring::EffectCommand::SetHostTransformTrack {
                    track: Some(replacement),
                },
            ),
        )
        .unwrap();
        let edited = configured_preview_instance(compile(&effect), 1.5, 42, &[]);
        assert_eq!(
            edited.instance.host_transform_at(1.5).translation,
            [50.0, 0.0, 0.0]
        );
        aestra_authoring::CommandExecutor::execute(&mut effect, &default(), &undo.inverse).unwrap();
        assert_eq!(
            compile(&effect).host_transform_track,
            compiled.host_transform_track
        );
    }

    #[test]
    fn project_preview_recompiles_referenced_effects_when_the_catalog_refreshes() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("child.aestra.ron");
        let mut child = aestra_core::EffectAsset::new("Child A", 1.0);
        child
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Child emitter", 1.0));
        child.save_ron(&path).unwrap();
        let child_id = child.id;
        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        session
            .effect
            .effect_clips
            .push(aestra_core::EffectClip::new(child_id, 0.0, 0.5));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
            .init_resource::<EditorPreviewProject>()
            .add_systems(Update, sync_project_preview);

        app.update();
        assert_eq!(
            app.world()
                .resource::<EditorPreviewProject>()
                .project
                .as_ref()
                .unwrap()
                .effect(child_id)
                .unwrap()
                .name,
            "Child A"
        );

        child.name = "Child B".into();
        child.save_ron(&path).unwrap();
        app.world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .refresh();
        app.update();

        assert_eq!(
            app.world()
                .resource::<EditorPreviewProject>()
                .project
                .as_ref()
                .unwrap()
                .effect(child_id)
                .unwrap()
                .name,
            "Child B"
        );
    }

    #[test]
    fn project_preview_players_follow_seek_and_restart_deterministically() {
        let temporary = tempfile::tempdir().unwrap();
        let mut child = aestra_core::EffectAsset::new("Child", 1.0);
        child
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Child emitter", 1.0));
        child
            .save_ron(temporary.path().join("child.aestra.ron"))
            .unwrap();

        let mut session = test_support::session_with_timing_slack();
        session.effect.effect_clips.clear();
        let mut clip = aestra_core::EffectClip::new(child.id, 0.5, 1.0);
        clip.source_offset = 0.1;
        clip.seed = aestra_core::EffectClipSeed::Fixed(77);
        let clip_id = clip.id;
        assert!(session.execute(
            "Add child",
            EffectCommand::AddEffectClip {
                clip,
                index: session.effect.effect_clips.len(),
            },
            false,
        ));
        session.seek_time(0.75);

        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
            .init_resource::<EditorPreviewProject>()
            .init_resource::<TimelineState>()
            .add_systems(
                Update,
                (sync_project_preview, sync_rendered_preview).chain(),
            );
        app.update();

        let child_state = {
            let world = app.world_mut();
            let mut players = world.query::<(&PreviewEffectInstancePath, &PresentedEffect)>();
            let instances = players.iter(world).collect::<Vec<_>>();
            assert_eq!(instances.len(), 2);
            let (_, player) = instances
                .into_iter()
                .find(|(path, _)| path.0.as_slice() == [clip_id])
                .expect("active clip must have a player");
            (presented_playhead_time(player), player.instance.seed())
        };
        assert!((child_state.0 - 0.35).abs() <= 1.0 / 60.0);
        assert_eq!(child_state.1, 77);

        app.world_mut()
            .resource_mut::<EditorSession>()
            .seek_time(0.25);
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&PreviewEffectInstancePath>()
                .iter(app.world())
                .count(),
            1,
            "seeking before the clip removes its player"
        );

        {
            let mut session = app.world_mut().resource_mut::<EditorSession>();
            session.restart();
            session.seek_time(0.75);
        }
        app.update();
        let replayed = {
            let world = app.world_mut();
            let mut players = world.query::<(&PreviewEffectInstancePath, &PresentedEffect)>();
            players
                .iter(world)
                .find(|(path, _)| path.0.as_slice() == [clip_id])
                .map(|(_, player)| (presented_playhead_time(player), player.instance.seed()))
                .expect("restarted clip must be active after seeking back into it")
        };
        assert_eq!(replayed, child_state);
    }

    #[test]
    fn continuous_project_preview_uses_unwrapped_simulation_time() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = test_support::session_with_playback_mode(EffectPlaybackMode::LoopRestart);
        session.effect.effect_clips.clear();
        assert!(session.set_effect_playback_mode(EffectPlaybackMode::LoopContinuous));
        session.restart();
        let duration = session.effect.duration;
        session.advance_playback(duration + 0.25);
        let expected_simulation_time = session.simulation_time();
        let expected_playhead = session.time();
        assert!(expected_simulation_time > duration);
        assert!(expected_playhead < duration);

        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(ProjectEffectCatalog::scan(temporary.path()))
            .init_resource::<EditorPreviewProject>()
            .init_resource::<TimelineState>()
            .add_systems(
                Update,
                (sync_project_preview, sync_rendered_preview).chain(),
            );
        app.update();

        let world = app.world_mut();
        let mut players = world.query::<(&PreviewEffectInstancePath, &PresentedEffect)>();
        let (_, player) = players
            .iter(world)
            .find(|(path, _)| path.0.is_empty())
            .expect("the root preview player must exist");
        assert!((player.simulation_time() - expected_simulation_time).abs() < 0.000_1);
        assert!((presented_playhead_time(player) - expected_playhead).abs() < 0.000_1);
    }

    #[test]
    fn material_refresh_updates_root_without_losing_interaction_preview() {
        use aestra_core::material::{
            MaterialInstance, MaterialProgram, MaterialProgramRef, MaterialRenderState,
        };
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("material.aestra.material.ron");
        let mut program = MaterialProgram::additive_sprite("Before");
        program.save_ron(&path).unwrap();
        let mut session = test_support::session_with_timing_slack();
        session.effect.material_instances.push(MaterialInstance {
            id: aestra_core::MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: default(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        let catalog = ProjectEffectCatalog::scan(temporary.path());
        session
            .install_compiled_project_root(catalog.compile_project(&session.effect).unwrap().root)
            .unwrap();
        let emitter = session.selected_layer().unwrap().id;
        let transform = EmitterTransform {
            translation: [9.0, 0.0, 0.0],
            ..default()
        };
        assert!(session.preview_interaction(EffectTransaction::single(
            "Drag",
            EffectCommand::SetEmitterTransform {
                id: emitter,
                transform
            }
        )));
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(catalog)
            .init_resource::<EditorPreviewProject>()
            .add_systems(Update, sync_project_preview);
        app.update();
        program.name = "After external save".into();
        program.save_ron(&path).unwrap();
        app.world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .refresh();
        app.update();
        let session = app.world().resource::<EditorSession>();
        let root = session.preview().unwrap().effect();
        assert_eq!(
            root.material_program(program.id).unwrap().name,
            program.name
        );
        assert_eq!(root.emitters[0].transform, transform);
        assert_ne!(session.effect.emitters[0].transform, transform);
    }

    #[test]
    fn gpu_preview_uses_observations_without_cpu_reference_evaluation() {
        let mut session = test_support::session_with_timing_slack();
        session.seek_time(0.5);
        let mut player = configured_preview_player(&session).unwrap();
        let root = player.effect().clone();
        let mut reference = Vec::new();
        session.evaluate_preview(&mut reference);
        assert!(!reference.is_empty());
        let particle_count = reference.len();
        player.restore_gpu_samples(reference);
        let mut app = App::new();
        app.insert_resource(session)
            .init_resource::<ProfilerState>()
            .init_resource::<TimelineState>()
            .insert_resource(EditorPreviewProject {
                source_root: Some(root.clone()),
                project: Some(Arc::new(CompiledEffectProject {
                    root,
                    dependencies: default(),
                })),
                ..default()
            })
            .add_systems(Update, update_preview);
        let player_entity = app
            .world_mut()
            .spawn((
                PreviewPresentedEffect,
                PreviewEffectInstancePath::default(),
                player,
                EffectRuntimeStatus {
                    active: ActiveBackend::Gpu,
                    reason: "Native test".into(),
                    compatibility: aestra_runtime::CompatibilityReport::compatible(
                        aestra_runtime::CompatibilityTarget::NativeGpu,
                    ),
                },
            ))
            .id();
        app.update();
        assert!(app.world().resource::<EditorSession>().samples.is_empty());
        assert_eq!(
            app.world()
                .resource::<ProfilerState>()
                .current_profile()
                .unwrap()
                .cpu_time_ns,
            aestra_runtime::ProfileValue::Unavailable
        );
        assert_eq!(
            app.world()
                .resource::<EditorPreviewProject>()
                .live_particle_count,
            0
        );
        app.world_mut()
            .get_mut::<EffectRuntimeStatus>(player_entity)
            .unwrap()
            .active = ActiveBackend::GpuReadback;
        app.update();
        let profile = app
            .world()
            .resource::<ProfilerState>()
            .current_profile()
            .unwrap();
        assert_eq!(
            profile.cpu_time_ns,
            aestra_runtime::ProfileValue::Unavailable
        );
        assert_eq!(
            profile.alive_particles,
            aestra_runtime::ProfileValue::Measured(particle_count as u32)
        );
        assert_eq!(
            app.world().resource::<EditorSession>().samples.len(),
            particle_count
        );
    }
}
