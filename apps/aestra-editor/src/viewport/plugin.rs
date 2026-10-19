//! Actual viewport plugin: resource, observer, startup and ordered consumer registration.
use super::{
    PreviewDisplayState, ViewportSet,
    actions::{
        audit_viewport_controls, execute_viewport_action, handle_viewport_action_buttons,
        queue_viewport_action_activation,
    },
    feedback::{apply_preview_mocks, record_preview_outputs, track_event_link_drops},
    gizmo_state::*,
    host_motion, keyboard,
    lifecycle::{configure_transform_gizmo_overlay_camera, sync_preview_camera_viewport},
    navigation::{PreviewCameraController, PreviewNavigationState, navigate_preview_camera},
    photographic::{sync_photographic_overlay_composition, sync_preview_photographic_response},
    presentation::{
        PreviewGridMaterial, sync_preview_display_mode, sync_preview_grid,
        update_preview_display_controls, update_transform_gizmo_controls,
    },
    runtime_preview::{EditorPreviewProject, sync_project_preview, sync_rendered_preview},
    scene::{draw_preview_mocks, setup_preview_scene},
    shape_gizmo::{
        configure_preview_scene_gizmos, draw_preview_scene_gizmos, interact_shape_gizmo,
    },
    telemetry::update_preview,
    transform_gizmo::*,
    workspace::update_viewport_status_label,
};
use aestra_bevy_render::AestraRenderSet;
use bevy::{
    app::TransformGizmoRenderStep,
    gizmos::transform_gizmo::{TransformGizmoPlugin, TransformGizmoSystems},
    prelude::*,
    ui::UiSystems,
};

pub(crate) struct ViewportPlugin;

impl Plugin for ViewportPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(TransformGizmoPlugin)
            .insert_resource(preview_transform_gizmo_settings())
            .add_plugins(MaterialPlugin::<PreviewGridMaterial>::default())
            .init_gizmo_group::<PreviewSceneGizmos>()
            .init_resource::<PreviewCameraController>()
            .init_resource::<PreviewNavigationState>()
            .init_resource::<PreviewDisplayState>()
            .init_resource::<EditorPreviewProject>()
            .init_resource::<ShapeGizmoState>()
            .init_resource::<EmitterTransformGizmoInteraction>()
            .init_resource::<host_motion::HostMotionViewport>()
            .add_observer(queue_viewport_action_activation)
            .add_observer(execute_viewport_action)
            .add_systems(
                Startup,
                (setup_preview_scene, configure_preview_scene_gizmos)
                    .chain()
                    .in_set(ViewportSet::Setup),
            )
            .add_systems(
                PostStartup,
                (
                    configure_transform_gizmo_overlay_camera,
                    configure_transform_gizmo_overlay_materials,
                ),
            )
            .add_systems(
                Update,
                (handle_viewport_action_buttons, audit_viewport_controls)
                    .chain()
                    .in_set(ViewportSet::Actions),
            )
            .add_systems(
                Update,
                (
                    sync_project_preview,
                    sync_rendered_preview,
                    apply_preview_mocks,
                    update_preview.after(AestraRenderSet::Prepare),
                    track_event_link_drops,
                    record_preview_outputs,
                    navigate_preview_camera,
                    sync_preview_grid,
                    sync_preview_display_mode,
                    sync_preview_photographic_response,
                    sync_photographic_overlay_composition,
                    update_preview_display_controls,
                    update_transform_gizmo_controls,
                    host_motion::pick_pose,
                    sync_emitter_transform_proxy,
                    interact_shape_gizmo,
                    sync_transform_gizmo_focus,
                    draw_preview_scene_gizmos,
                    draw_preview_mocks,
                    update_viewport_status_label,
                )
                    .chain()
                    .in_set(ViewportSet::Update),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_preview_camera_viewport
                        .after(UiSystems::Layout)
                        .before(TransformGizmoSystems),
                    update_emitter_transform_gizmo
                        .after(TransformGizmoSystems)
                        .before(TransformGizmoRenderStep),
                    apply_transform_gizmo_drag_feedback
                        .after(TransformGizmoRenderStep)
                        .after(update_emitter_transform_gizmo),
                    update_transform_gizmo_value_label.after(update_emitter_transform_gizmo),
                    host_motion::draw_path.after(update_emitter_transform_gizmo),
                ),
            )
            .configure_sets(Update, AestraRenderSet::Prepare.after(apply_preview_mocks));
        keyboard::register(app);
    }
}

#[cfg(test)]
#[path = "plugin/tests.rs"]
mod tests;
