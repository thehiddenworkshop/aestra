//! Existing physical viewport synchronization and delayed camera activation contract.
use super::{
    PreviewDisplayState,
    navigation::{PreviewRenderCamera, xy_world_height},
    pixel_preview::{PixelPreviewPass, VIRTUAL_SIZE},
    scene::{PreviewPerspectiveCamera, PreviewXyCamera},
    shortcuts::PreviewCanvas,
};
use aestra_bevy_render::pixel_view::integer_pixel_viewport;
use bevy::{
    camera::{CameraProjection, Viewport, visibility::RenderLayers},
    prelude::*,
    window::PrimaryWindow,
};

type PreviewCameras = Or<(
    With<PreviewRenderCamera>,
    With<PreviewPerspectiveCamera>,
    With<PreviewXyCamera>,
    With<PixelPreviewPass>,
)>;
type PreviewCameraViews<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Camera,
        Option<&'static Projection>,
        Has<PreviewRenderCamera>,
        Has<PreviewXyCamera>,
        Option<&'static PixelPreviewPass>,
    ),
    PreviewCameras,
>;

type OverlayCameraFilter = (
    With<Camera3d>,
    Without<PreviewRenderCamera>,
    Without<PreviewPerspectiveCamera>,
    Without<PreviewXyCamera>,
    Without<PixelPreviewPass>,
);
type OverlayCameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static RenderLayers,
        &'static mut Camera,
        Option<&'static Projection>,
    ),
    OverlayCameraFilter,
>;

const GIZMO_LAYER: usize = 15;

pub(super) fn configure_transform_gizmo_overlay_camera(mut cameras: OverlayCameras) {
    let gizmo_layers = RenderLayers::layer(GIZMO_LAYER);
    for (layers, mut camera, _) in &mut cameras {
        if layers == &gizmo_layers {
            camera.clear_color = ClearColorConfig::None;
            camera.order = 1;
            camera.is_active = true;
        }
    }
}

pub(super) fn sync_gizmo_overlay_projection(
    camera: Single<(&Projection, &Transform, Has<PreviewXyCamera>), With<PreviewRenderCamera>>,
    mut overlays: Query<(&RenderLayers, &mut Projection, &mut Transform), OverlayCameraFilter>,
) {
    let (projection, pose, xy) = camera.into_inner();
    let gizmo_layers = RenderLayers::layer(GIZMO_LAYER);
    // The planar tools share the overlay camera even without native gizmo focus.
    for (layers, mut overlay, mut transform) in &mut overlays {
        if layers != &gizmo_layers {
            continue;
        }
        transform.set_if_neq(*pose);
        if xy {
            if let Projection::Orthographic(xy) = projection
                && !matches!(&*overlay, Projection::Orthographic(current)
                    if current.scale == xy.scale && xy_world_height(current) == xy_world_height(xy))
            {
                *overlay = projection.clone();
            }
        } else if matches!(&*overlay, Projection::Orthographic(_)) {
            *overlay = Projection::default();
        }
    }
}

pub(super) fn sync_preview_camera_viewport(
    canvas: Query<(&ComputedNode, &UiGlobalTransform), With<PreviewCanvas>>,
    window: Single<&Window, With<PrimaryWindow>>,
    display: Option<Res<PreviewDisplayState>>,
    mut preview_cameras: PreviewCameraViews,
    mut overlay_cameras: OverlayCameras,
    // Whether the viewport was valid on the previous frame. See the reactivation
    // note below.
    mut viewport_ready: Local<bool>,
) {
    let Ok((computed, transform)) = canvas.single() else {
        disable_preview_cameras(&mut preview_cameras, &mut overlay_cameras);
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
        disable_preview_cameras(&mut preview_cameras, &mut overlay_cameras);
        *viewport_ready = false;
        return;
    }
    // An odd-sized canvas can have a half-pixel center and an integer origin.
    let top_left = transform.translation - size * 0.5;
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
    // Keep input mapping and the full-resolution overlay on the very same
    // physical rectangle as the compositor. A cramped dock keeps the live 2D view.
    let pixel_viewport = display
        .filter(|display| display.pixel_grid)
        .filter(|_| {
            preview_cameras
                .iter()
                .any(|(_, _, selected, xy, _)| selected && xy)
        })
        .and_then(|_| integer_pixel_viewport(VIRTUAL_SIZE, physical_size))
        .map(|mut fitted| {
            fitted.physical_position += position;
            fitted
        });
    let scene_viewport = pixel_viewport.as_ref().unwrap_or(&viewport);
    // Only (re)activate once the viewport has been valid for a full frame. When
    // the window is un-minimized, `camera_system` has clamped the explicit
    // viewport to the (0,0) surface, and `assign_objects_to_clusters` can run
    // before this system on the restore frame — clearing the cluster grid to zero
    // dimensions. If we activated the same frame, the render world would extract
    // an active view with a zero-size grid and abort building the "clustering
    // dummy texture". Deferring activation one frame guarantees assignment has
    // already produced a non-zero grid from the restored viewport (set above,
    // persisted) before the camera is ever extracted active.
    for (mut camera, projection, selected, xy, pass) in &mut preview_cameras {
        match pass {
            Some(PixelPreviewPass::Scene) => {
                camera.is_active = *viewport_ready && pixel_viewport.is_some();
            }
            Some(PixelPreviewPass::Composite) => {
                if let Some(fitted) = &pixel_viewport {
                    assign_viewport(&mut camera, projection, fitted);
                }
                camera.is_active = *viewport_ready && pixel_viewport.is_some();
            }
            None => {
                assign_viewport(
                    &mut camera,
                    projection,
                    if xy && selected {
                        scene_viewport
                    } else {
                        &viewport
                    },
                );
                camera.is_active = *viewport_ready && selected && pixel_viewport.is_none();
            }
        }
    }
    let gizmo_layers = RenderLayers::layer(GIZMO_LAYER);
    for (layers, mut camera, projection) in &mut overlay_cameras {
        if layers == &gizmo_layers {
            assign_viewport(&mut camera, projection, scene_viewport);
            camera.is_active = *viewport_ready;
        }
    }
    *viewport_ready = true;
}

fn assign_viewport(camera: &mut Camera, projection: Option<&Projection>, viewport: &Viewport) {
    if camera.viewport.as_ref().is_some_and(|current| {
        current.physical_position == viewport.physical_position
            && current.physical_size == viewport.physical_size
            && current.depth == viewport.depth
    }) {
        return;
    }
    camera.viewport = Some(viewport.clone());
    // UI layout commits after Bevy's camera update. Keep the committed XY
    // projection and rect paired even on the first frame after a dock resize.
    if let Some(Projection::Orthographic(projection)) = projection {
        let size = viewport.physical_size.as_vec2() / camera.target_scaling_factor().unwrap_or(1.0);
        let mut projection = projection.clone();
        projection.update(size.x, size.y);
        camera.computed.clip_from_view = projection.get_clip_from_view();
    }
}

fn disable_preview_cameras(
    preview_cameras: &mut PreviewCameraViews,
    overlay_cameras: &mut OverlayCameras,
) {
    for (mut camera, ..) in preview_cameras {
        camera.is_active = false;
    }
    let gizmo_layers = RenderLayers::layer(GIZMO_LAYER);
    for (layers, mut camera, _) in overlay_cameras {
        if layers == &gizmo_layers {
            camera.is_active = false;
        }
    }
}

#[cfg(test)]
#[path = "tests/lifecycle.rs"]
mod tests;
