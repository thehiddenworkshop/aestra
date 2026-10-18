//! Actual physical camera viewport and Local readiness transitions on both engine graphs.
use super::*;
use bevy::{math::Affine2, window::WindowResolution};

struct Fixture {
    app: App,
    window: Entity,
    canvas: Entity,
    preview: Entity,
    overlay: Entity,
}
impl Fixture {
    fn new() -> Self {
        let mut app = App::new();
        app.add_systems(PostUpdate, sync_preview_camera_viewport);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: WindowResolution::new(1000, 800),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        let canvas = app
            .world_mut()
            .spawn((
                PreviewCanvas,
                geometry(Vec2::new(100.0, 60.0), Vec2::new(400.0, 300.0)),
            ))
            .id();
        let preview = app
            .world_mut()
            .spawn((PreviewRenderCamera, Camera::default()))
            .id();
        let overlay = app
            .world_mut()
            .spawn((Camera3d::default(), RenderLayers::layer(15)))
            .id();
        Self {
            app,
            window,
            canvas,
            preview,
            overlay,
        }
    }
    fn tick(&mut self) {
        self.app.update();
    }
    fn ready(&mut self) {
        self.tick();
        self.tick();
        self.active(true);
    }
    fn active(&self, expected: bool) {
        for entity in [self.preview, self.overlay] {
            assert_eq!(
                self.app.world().get::<Camera>(entity).unwrap().is_active,
                expected
            );
        }
    }
    fn viewport(&self, position: UVec2, size: UVec2) {
        for entity in [self.preview, self.overlay] {
            let viewport = self
                .app
                .world()
                .get::<Camera>(entity)
                .unwrap()
                .viewport
                .as_ref()
                .unwrap();
            assert_eq!(viewport.physical_position, position);
            assert_eq!(viewport.physical_size, size);
            assert_eq!(viewport.depth, 0.0..1.0);
        }
    }
    fn geometry(&mut self, top_left: Vec2, size: Vec2) {
        self.app
            .world_mut()
            .entity_mut(self.canvas)
            .insert(geometry(top_left, size));
    }
    fn resolution(&mut self, width: u32, height: u32) {
        self.app
            .world_mut()
            .get_mut::<Window>(self.window)
            .unwrap()
            .resolution
            .set_physical_resolution(width, height);
    }
}
fn geometry(top_left: Vec2, size: Vec2) -> (ComputedNode, UiGlobalTransform) {
    (
        ComputedNode { size, ..default() },
        UiGlobalTransform::from(Affine2::from_translation(top_left + size * 0.5)),
    )
}

#[test]
fn first_valid_frame_assigns_both_viewports_before_the_second_frame_activates_them() {
    let mut f = Fixture::new();
    f.tick();
    f.viewport(UVec2::new(100, 60), UVec2::new(400, 300));
    f.active(false);
    f.tick();
    f.active(true);
    f.viewport(UVec2::new(100, 60), UVec2::new(400, 300));
}

#[test]
fn moving_and_resizing_a_valid_canvas_updates_both_views_without_rearming() {
    let mut f = Fixture::new();
    f.ready();
    for (origin, size) in [
        (Vec2::new(200.0, 120.0), Vec2::new(600.0, 420.0)),
        (Vec2::ZERO, Vec2::new(1000.0, 800.0)),
    ] {
        f.geometry(origin, size);
        f.tick();
        f.active(true);
        f.viewport(origin.as_uvec2(), size.as_uvec2());
    }
}

#[test]
fn fractional_physical_geometry_uses_the_original_integer_conversion() {
    let mut f = Fixture::new();
    f.geometry(Vec2::new(12.75, 29.25), Vec2::new(333.5, 111.75));
    f.tick();
    f.viewport(UVec2::new(12, 29), UVec2::new(333, 111));
    f.active(false);
}

#[test]
fn negative_origins_and_right_bottom_overflow_keep_the_original_clamping_policy() {
    let mut f = Fixture::new();
    for (origin, size, expected_origin, expected_size) in [
        (
            Vec2::new(-40.0, -20.0),
            Vec2::new(300.0, 250.0),
            UVec2::ZERO,
            UVec2::new(300, 250),
        ),
        (
            Vec2::new(900.0, 750.0),
            Vec2::new(300.0, 250.0),
            UVec2::new(900, 750),
            UVec2::new(100, 50),
        ),
        (
            Vec2::new(800.0, 600.0),
            Vec2::new(5000.0, 5000.0),
            UVec2::new(800, 600),
            UVec2::new(200, 200),
        ),
    ] {
        f.geometry(origin, size);
        f.tick();
        f.viewport(expected_origin, expected_size);
    }
}

#[test]
fn wholly_outside_canvas_clamps_to_a_nonzero_last_pixel_viewport() {
    let mut f = Fixture::new();
    f.geometry(Vec2::new(5000.0, 5000.0), Vec2::splat(200.0));
    f.tick();
    f.viewport(UVec2::new(999, 799), UVec2::ONE);
    f.active(false);
    f.tick();
    f.active(true);
}

#[test]
fn invalid_or_too_small_sizes_disable_both_cameras_and_require_a_new_valid_frame() {
    for size in [
        Vec2::ZERO,
        Vec2::new(15.0, 100.0),
        Vec2::new(100.0, 15.0),
        Vec2::new(-20.0, 100.0),
        Vec2::new(f32::NAN, 100.0),
        Vec2::new(100.0, f32::NAN),
        Vec2::new(f32::INFINITY, 100.0),
        Vec2::new(100.0, f32::NEG_INFINITY),
    ] {
        let mut f = Fixture::new();
        f.ready();
        f.geometry(Vec2::ZERO, size);
        f.tick();
        f.active(false);
        f.viewport(UVec2::new(100, 60), UVec2::new(400, 300));
        f.geometry(Vec2::new(20.0, 30.0), Vec2::splat(100.0));
        f.tick();
        f.active(false);
        f.viewport(UVec2::new(20, 30), UVec2::splat(100));
        f.tick();
        f.active(true);
    }
}

#[test]
fn the_exact_minimum_canvas_size_is_valid() {
    let mut f = Fixture::new();
    f.geometry(Vec2::ZERO, Vec2::splat(16.0));
    f.tick();
    f.active(false);
    f.viewport(UVec2::ZERO, UVec2::splat(16));
    f.tick();
    f.active(true);
}

#[test]
fn removing_and_recreating_the_canvas_disables_then_rearms_the_same_cameras() {
    let mut f = Fixture::new();
    f.ready();
    f.app.world_mut().despawn(f.canvas);
    f.tick();
    f.active(false);
    f.canvas = f
        .app
        .world_mut()
        .spawn((
            PreviewCanvas,
            geometry(Vec2::new(5.0, 10.0), Vec2::new(200.0, 160.0)),
        ))
        .id();
    f.tick();
    f.active(false);
    f.viewport(UVec2::new(5, 10), UVec2::new(200, 160));
    f.tick();
    f.active(true);
}

#[test]
fn ambiguous_canvas_disables_and_recovery_waits_one_valid_frame() {
    let mut f = Fixture::new();
    f.ready();
    let extra = f
        .app
        .world_mut()
        .spawn((PreviewCanvas, geometry(Vec2::ZERO, Vec2::splat(100.0))))
        .id();
    f.tick();
    f.active(false);
    f.app.world_mut().despawn(extra);
    f.tick();
    f.active(false);
    f.tick();
    f.active(true);
}

#[test]
fn zero_surface_dimensions_disable_and_restore_keeps_the_one_frame_activation_delay() {
    for (width, height) in [(0, 800), (1000, 0), (0, 0)] {
        let mut f = Fixture::new();
        f.ready();
        f.resolution(width, height);
        f.tick();
        f.active(false);
        f.resolution(1200, 900);
        f.geometry(Vec2::new(50.0, 25.0), Vec2::new(1100.0, 850.0));
        f.tick();
        f.active(false);
        f.viewport(UVec2::new(50, 25), UVec2::new(1100, 850));
        f.tick();
        f.active(true);
    }
}

#[test]
fn an_invalid_frame_between_valid_frames_resets_readiness_each_time() {
    let mut f = Fixture::new();
    f.ready();
    for _ in 0..3 {
        f.resolution(0, 0);
        f.tick();
        f.active(false);
        f.resolution(1000, 800);
        f.tick();
        f.active(false);
    }
    f.tick();
    f.active(true);
}

#[test]
fn only_preview_and_exact_layer_fifteen_3d_overlays_receive_viewports_or_activation() {
    let mut f = Fixture::new();
    let sentinel = Viewport {
        physical_position: UVec2::new(7, 9),
        physical_size: UVec2::new(51, 53),
        ..default()
    };
    let others = [
        f.app
            .world_mut()
            .spawn((
                Camera3d::default(),
                RenderLayers::layer(0),
                Camera {
                    is_active: false,
                    viewport: Some(sentinel.clone()),
                    ..default()
                },
            ))
            .id(),
        f.app
            .world_mut()
            .spawn((
                Camera3d::default(),
                RenderLayers::from_layers(&[15, 16]),
                Camera {
                    is_active: false,
                    viewport: Some(sentinel.clone()),
                    ..default()
                },
            ))
            .id(),
        f.app
            .world_mut()
            .spawn((
                Camera2d,
                RenderLayers::layer(15),
                Camera {
                    is_active: false,
                    viewport: Some(sentinel.clone()),
                    ..default()
                },
            ))
            .id(),
        f.app
            .world_mut()
            .spawn((
                Camera3d::default(),
                Camera {
                    is_active: false,
                    viewport: Some(sentinel.clone()),
                    ..default()
                },
            ))
            .id(),
    ];
    let expected_active = [false, true, false, true];
    for (entity, active) in others.into_iter().zip(expected_active) {
        f.app
            .world_mut()
            .get_mut::<Camera>(entity)
            .unwrap()
            .is_active = active;
    }
    for valid in [true, false] {
        if valid {
            f.ready();
        } else {
            f.resolution(0, 0);
            f.tick();
            f.active(false);
        }
        for (entity, active) in others.into_iter().zip(expected_active) {
            let camera = f.app.world().get::<Camera>(entity).unwrap();
            assert_eq!(camera.is_active, active);
            let viewport = camera.viewport.as_ref().unwrap();
            assert_eq!(viewport.physical_position, sentinel.physical_position);
            assert_eq!(viewport.physical_size, sentinel.physical_size);
        }
    }
}

#[test]
fn startup_overlay_configuration_keeps_color_then_lifecycle_owns_activation() {
    let mut f = Fixture::new();
    f.app
        .add_systems(PostStartup, configure_transform_gizmo_overlay_camera);
    let other = f
        .app
        .world_mut()
        .spawn((
            Camera3d::default(),
            RenderLayers::layer(0),
            Camera {
                order: 7,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.2, 0.3, 0.4)),
                ..default()
            },
        ))
        .id();
    f.tick();
    f.active(false);
    let overlay = f.app.world().get::<Camera>(f.overlay).unwrap();
    assert_eq!(overlay.order, 1);
    assert!(matches!(overlay.clear_color, ClearColorConfig::None));
    let other = f.app.world().get::<Camera>(other).unwrap();
    assert_eq!(other.order, 7);
    assert!(matches!(other.clear_color, ClearColorConfig::Custom(_)));
    assert!(other.is_active);
    f.tick();
    f.active(true);
}

#[test]
fn already_physical_canvas_geometry_is_not_scaled_again_by_display_or_ui_scale() {
    for display_scale in [1.0, 1.5, 2.0] {
        for ui_scale in [1.0, 1.25, 2.0] {
            let mut f = Fixture::new();
            f.app
                .world_mut()
                .get_mut::<Window>(f.window)
                .unwrap()
                .resolution
                .set_scale_factor_override(Some(display_scale));
            f.app.insert_resource(UiScale(ui_scale));
            f.ready();
            f.viewport(UVec2::new(100, 60), UVec2::new(400, 300));
        }
    }
}

#[test]
fn secondary_window_size_does_not_override_primary_surface_bounds() {
    let mut f = Fixture::new();
    f.app.world_mut().spawn(Window {
        resolution: WindowResolution::new(0, 0),
        ..default()
    });
    f.ready();
    f.viewport(UVec2::new(100, 60), UVec2::new(400, 300));
}

#[test]
fn viewport_sync_preserves_clear_color_order_and_output_composition() {
    let mut f = Fixture::new();
    for entity in [f.preview, f.overlay] {
        let mut camera = f.app.world_mut().get_mut::<Camera>(entity).unwrap();
        camera.order = 9;
        camera.clear_color = ClearColorConfig::Custom(Color::NONE);
        camera.output_mode = bevy::camera::CameraOutputMode::Write {
            blend_state: Some(bevy::render::render_resource::BlendState::ALPHA_BLENDING),
            clear_color: ClearColorConfig::None,
        };
    }
    f.ready();
    for entity in [f.preview, f.overlay] {
        let camera = f.app.world().get::<Camera>(entity).unwrap();
        assert_eq!(camera.order, 9);
        assert!(
            matches!(camera.clear_color, ClearColorConfig::Custom(color) if color == Color::NONE)
        );
        assert!(matches!(
            camera.output_mode,
            bevy::camera::CameraOutputMode::Write {
                blend_state: Some(_),
                clear_color: ClearColorConfig::None,
            }
        ));
    }
}
