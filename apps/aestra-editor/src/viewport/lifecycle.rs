//! Existing physical viewport synchronization and delayed camera activation contract.
use super::{navigation::PreviewRenderCamera, shortcuts::PreviewCanvas};
use bevy::{
    camera::{Viewport, visibility::RenderLayers},
    prelude::*,
    window::PrimaryWindow,
};

pub(super) fn configure_transform_gizmo_overlay_camera(
    mut cameras: Query<
        (&RenderLayers, &mut Camera),
        (With<Camera3d>, Without<PreviewRenderCamera>),
    >,
) {
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, mut camera) in &mut cameras {
        if layers == &gizmo_layers {
            camera.clear_color = ClearColorConfig::None;
            camera.order = 1;
            camera.is_active = true;
        }
    }
}

pub(super) fn sync_preview_camera_viewport(
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<PreviewCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut preview_camera: Single<&mut Camera, With<PreviewRenderCamera>>,
    mut overlay_cameras: Query<
        (&RenderLayers, &mut Camera),
        (With<Camera3d>, Without<PreviewRenderCamera>),
    >,
    // Whether the viewport was valid on the previous frame. See the reactivation
    // note below.
    mut viewport_ready: Local<bool>,
) {
    let Ok((computed, transform)) = canvas.single() else {
        set_preview_cameras_active(&mut preview_camera, &mut overlay_cameras, false);
        *viewport_ready = false;
        return;
    };
    let size = computed.size();
    if !size.is_finite()
        || size.x < 16.0
        || size.y < 16.0
        || window.physical_width() == 0
        || window.physical_height() == 0
    {
        set_preview_cameras_active(&mut preview_camera, &mut overlay_cameras, false);
        *viewport_ready = false;
        return;
    }
    let top_left = transform.translation.trunc() - size * 0.5;
    let target_size = UVec2::new(
        window.physical_width().max(1),
        window.physical_height().max(1),
    );
    let position = top_left.max(Vec2::ZERO).as_uvec2().min(target_size - 1);
    let available = target_size.saturating_sub(position).max(UVec2::ONE);
    let physical_size = size.as_uvec2().max(UVec2::ONE).min(available);
    let viewport = Viewport {
        physical_position: position,
        physical_size,
        ..default()
    };
    // Set the viewport on BOTH the preview camera and the gizmo overlay camera so
    // PBR cluster assignment sees a valid (non-zero) size for each this frame.
    preview_camera.viewport = Some(viewport.clone());
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, mut camera) in &mut overlay_cameras {
        if layers == &gizmo_layers {
            camera.viewport = Some(viewport.clone());
        }
    }
    // Only (re)activate once the viewport has been valid for a full frame. When
    // the window is un-minimized, `camera_system` has clamped the explicit
    // viewport to the (0,0) surface, and `assign_objects_to_clusters` can run
    // before this system on the restore frame — clearing the cluster grid to zero
    // dimensions. If we activated the same frame, the render world would extract
    // an active view with a zero-size grid and abort building the "clustering
    // dummy texture". Deferring activation one frame guarantees assignment has
    // already produced a non-zero grid from the restored viewport (set above,
    // persisted) before the camera is ever extracted active.
    set_preview_cameras_active(&mut preview_camera, &mut overlay_cameras, *viewport_ready);
    *viewport_ready = true;
}

fn set_preview_cameras_active(
    preview_camera: &mut Camera,
    overlay_cameras: &mut Query<
        (&RenderLayers, &mut Camera),
        (With<Camera3d>, Without<PreviewRenderCamera>),
    >,
    active: bool,
) {
    preview_camera.is_active = active;
    let gizmo_layers = RenderLayers::layer(15);
    for (layers, mut camera) in overlay_cameras {
        if layers == &gizmo_layers {
            camera.is_active = active;
        }
    }
}

#[cfg(test)]
#[path = "lifecycle/tests.rs"]
mod tests;
