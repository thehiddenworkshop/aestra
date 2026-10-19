//! Production shape consumers on both engines, with actual camera projection and GizmoAsset output.
use super::*;
use crate::test_support;
use aestra_core::ModuleParameters;
use bevy::{
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
    gizmos::{GizmoAsset, GizmoHandles},
    math::Affine2,
    window::WindowResolution,
};

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

struct Fixture {
    app: App,
    window: Entity,
    canvas: Entity,
    camera: Entity,
    player: Entity,
    label: Entity,
    emitter: EmitterId,
    module: ModuleId,
}
impl Fixture {
    fn new(shape: EmitterShape) -> Self {
        Self::localized(shape, "en-US")
    }
    fn localized(shape: EmitterShape, locale: &str) -> Self {
        let mut effect = test_support::effect_with_timing_slack();
        let emitter = effect.emitters[0].id;
        let module = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|m| m.module_type.0 == aestra_core::MODULE_SHAPE)
            .unwrap();
        module.parameters = ModuleParameters::Shape { shape };
        let module = module.id;
        let mut session = EditorSession::from_test_effect(effect);
        session.selection.primary = SemanticTarget::Module(module);
        let mut app = App::new();
        app.insert_resource(session)
            .insert_resource(Localizer::new(locale).unwrap())
            .init_resource::<MenuState>()
            .init_resource::<ShapeGizmoState>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ButtonInput<KeyCode>>()
            .add_systems(Update, interact_shape_gizmo);
        let window = app
            .world_mut()
            .spawn((
                Window {
                    resolution: WindowResolution::new(800, 600),
                    ..default()
                },
                PrimaryWindow,
                CursorIcon::System(SystemCursorIcon::Default),
            ))
            .id();
        let canvas = app
            .world_mut()
            .spawn((
                PreviewCanvas,
                RelativeCursorPosition {
                    cursor_over: true,
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(600.0, 400.0),
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(400.0, 250.0))),
            ))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                PreviewRenderCamera,
                Camera {
                    viewport: Some(Viewport {
                        physical_position: UVec2::new(100, 50),
                        physical_size: UVec2::new(600, 400),
                        ..default()
                    }),
                    computed: ComputedCameraValues {
                        clip_from_view: Mat4::from_scale(Vec3::new(0.02, 0.02, -0.001)),
                        target_info: Some(RenderTargetInfo {
                            physical_size: UVec2::new(800, 600),
                            scale_factor: 1.0,
                        }),
                        ..default()
                    },
                    ..default()
                },
                GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 10.0)),
            ))
            .id();
        let player = app
            .world_mut()
            .spawn((EmitterTransformGizmoProxy, GlobalTransform::IDENTITY))
            .id();
        let label = app
            .world_mut()
            .spawn((
                ShapeGizmoValueLabel,
                Text::new(""),
                Node::default(),
                Visibility::Hidden,
            ))
            .id();
        Self {
            app,
            window,
            canvas,
            camera,
            player,
            label,
            emitter,
            module,
        }
    }
    fn tick(&mut self) {
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }
    fn cursor_world(&mut self, local: Vec3) {
        let world = self
            .app
            .world()
            .get::<GlobalTransform>(self.player)
            .unwrap()
            .transform_point(local);
        let camera = self.app.world().get::<Camera>(self.camera).unwrap();
        let transform = self
            .app
            .world()
            .get::<GlobalTransform>(self.camera)
            .unwrap();
        let cursor = camera.world_to_viewport(transform, world).unwrap();
        self.app
            .world_mut()
            .get_mut::<Window>(self.window)
            .unwrap()
            .set_cursor_position(Some(cursor));
    }
    fn start(&mut self, handle: ShapeGizmoHandle) {
        let shape = self.authored();
        let local = shape_handle_local_positions(shape)
            .into_iter()
            .find(|(h, _)| *h == handle)
            .unwrap()
            .1;
        self.cursor_world(local);
        self.tick();
        assert_eq!(
            self.app.world().resource::<ShapeGizmoState>().hovered,
            Some(handle)
        );
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        self.tick();
        assert!(
            self.app
                .world()
                .resource::<ShapeGizmoState>()
                .active
                .is_some()
        );
    }
    fn release(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        self.tick();
    }
    fn authored(&self) -> EmitterShape {
        let s = self.app.world().resource::<EditorSession>();
        let module = s.effect.emitters[0]
            .modules
            .iter()
            .find(|m| m.id == self.module)
            .unwrap();
        let Some(Value::Shape(shape)) = module_parameter(module, "shape") else {
            panic!("shape parameter")
        };
        shape
    }
    fn drawing(&mut self) {
        self.app
            .add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<GizmoAsset>()
            .init_gizmo_group::<PreviewSceneGizmos>()
            .add_systems(PostUpdate, draw_preview_scene_gizmos);
    }
}

#[test]
fn every_shape_keeps_its_original_handle_positions() {
    assert!(shape_handle_local_positions(EmitterShape::Point).is_empty());
    for shape in [
        EmitterShape::Circle { radius: 5.0 },
        EmitterShape::Ring { radius: 5.0 },
        EmitterShape::Sphere { radius: 5.0 },
        EmitterShape::Hemisphere { radius: 5.0 },
    ] {
        assert_eq!(
            shape_handle_local_positions(shape),
            vec![(ShapeGizmoHandle::Radius, Vec3::new(5.0, 0.0, 0.0))]
        );
    }
    assert_eq!(
        shape_handle_local_positions(EmitterShape::Box {
            half_extents: [2.0, 3.0, 4.0]
        }),
        vec![
            (ShapeGizmoHandle::ExtentX, Vec3::new(2.0, 0.0, 0.0)),
            (ShapeGizmoHandle::ExtentY, Vec3::new(0.0, 3.0, 0.0)),
            (ShapeGizmoHandle::ExtentZ, Vec3::new(0.0, 0.0, 4.0))
        ]
    );
    assert_eq!(
        shape_handle_local_positions(EmitterShape::Cylinder {
            radius: 5.0,
            depth: 8.0
        }),
        vec![
            (ShapeGizmoHandle::Radius, Vec3::new(5.0, 0.0, 0.0)),
            (ShapeGizmoHandle::Depth, Vec3::new(0.0, 4.0, 0.0))
        ]
    );
    assert_eq!(
        shape_handle_local_positions(EmitterShape::Cone {
            radius: 5.0,
            depth: 8.0
        }),
        vec![
            (ShapeGizmoHandle::Radius, Vec3::new(5.0, 8.0, 0.0)),
            (ShapeGizmoHandle::Depth, Vec3::new(0.0, 8.0, 0.0))
        ]
    );
}
#[test]
fn shape_dimension_changes_are_clamped_and_unrelated_handles_are_noops() {
    for shape in [
        EmitterShape::Circle { radius: 5.0 },
        EmitterShape::Ring { radius: 5.0 },
        EmitterShape::Sphere { radius: 5.0 },
        EmitterShape::Hemisphere { radius: 5.0 },
    ] {
        assert_eq!(
            shape_handle_local_positions(shape_after_gizmo_drag(
                shape,
                ShapeGizmoHandle::Radius,
                -8.0
            ))[0]
                .1
                .x,
            0.1
        );
        assert_eq!(
            shape_after_gizmo_drag(shape, ShapeGizmoHandle::Depth, 9.0),
            shape
        );
    }
    assert_eq!(
        shape_after_gizmo_drag(EmitterShape::Point, ShapeGizmoHandle::Radius, 9.0),
        EmitterShape::Point
    );
}
#[test]
fn actual_radius_drag_previews_then_commits_exactly_one_undo_step() {
    let original = EmitterShape::Circle { radius: 5.0 };
    let mut f = Fixture::new(original);
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    let revision = f
        .app
        .world()
        .resource::<EditorSession>()
        .document_revision();
    f.start(ShapeGizmoHandle::Radius);
    f.cursor_world(Vec3::new(8.0, 0.0, 0.0));
    f.tick();
    assert_eq!(f.authored(), original);
    assert!(!f.app.world().resource::<EditorSession>().dirty);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .document_revision(),
        revision
    );
    assert_eq!(f.app.world().resource::<EditorSession>().effect, before);
    assert!(!f.app.world().resource::<EditorSession>().can_undo());
    assert_eq!(
        f.app
            .world()
            .resource::<ShapeGizmoState>()
            .active
            .unwrap()
            .current,
        EmitterShape::Circle { radius: 8.0 }
    );
    f.release();
    assert_eq!(f.authored(), EmitterShape::Circle { radius: 8.0 });
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect_undo_len(),
        1
    );
    f.app.world_mut().resource_mut::<EditorSession>().undo();
    assert_eq!(f.authored(), original);
    f.app.world_mut().resource_mut::<EditorSession>().redo();
    assert_eq!(f.authored(), EmitterShape::Circle { radius: 8.0 });
}
#[test]
fn unchanged_release_creates_no_history_and_restores_cursor() {
    let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
    f.start(ShapeGizmoHandle::Radius);
    f.release();
    assert!(!f.app.world().resource::<EditorSession>().can_undo());
    assert!(!f.app.world().resource::<EditorSession>().dirty);
    assert_eq!(
        *f.app.world().get::<CursorIcon>(f.window).unwrap(),
        CursorIcon::System(SystemCursorIcon::Default)
    );
    assert_eq!(
        *f.app.world().get::<Visibility>(f.label).unwrap(),
        Visibility::Hidden
    );
}
#[test]
fn escape_restores_shape_preview_and_late_release_does_not_commit() {
    let original = EmitterShape::Circle { radius: 5.0 };
    let mut f = Fixture::new(original);
    f.start(ShapeGizmoHandle::Radius);
    f.cursor_world(Vec3::new(9.0, 0.0, 0.0));
    f.tick();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    f.tick();
    f.release();
    assert_eq!(f.authored(), original);
    assert!(!f.app.world().resource::<EditorSession>().can_undo());
    assert!(!f.app.world().resource::<EditorSession>().dirty);
    assert!(f.app.world().resource::<ShapeGizmoState>().active.is_none());
}
#[test]
fn effect_emitter_and_module_locks_prevent_start_but_do_not_edit_selection() {
    for kind in 0..3 {
        let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
        let effect = f.app.world().resource::<EditorSession>().effect.id;
        let target = match kind {
            0 => SemanticTarget::Effect(effect),
            1 => SemanticTarget::Emitter(f.emitter),
            _ => SemanticTarget::Module(f.module),
        };
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .locks
            .lock(target);
        f.cursor_world(Vec3::X * 5.0);
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        f.tick();
        assert!(f.app.world().resource::<ShapeGizmoState>().active.is_none());
        assert!(!f.app.world().resource::<EditorSession>().dirty);
        assert_eq!(
            f.app.world().resource::<EditorSession>().selection.primary,
            SemanticTarget::Module(f.module)
        );
    }
}
#[test]
fn menu_context_and_canvas_ownership_block_new_hover_and_drags() {
    for kind in 0..3 {
        let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
        match kind {
            0 => {
                f.app.world_mut().resource_mut::<MenuState>().open =
                    Some(crate::menus::MenuKind::File)
            }
            1 => {
                f.app.world_mut().resource_mut::<MenuState>().tab_context =
                    Some(crate::menus::TabContextMenu {
                        panel: crate::ToolPanel::Viewport,
                        position: [0.0; 2],
                    })
            }
            _ => {
                f.app
                    .world_mut()
                    .get_mut::<RelativeCursorPosition>(f.canvas)
                    .unwrap()
                    .cursor_over = false
            }
        }
        f.cursor_world(Vec3::X * 5.0);
        f.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        f.tick();
        let state = f.app.world().resource::<ShapeGizmoState>();
        assert!(state.hovered.is_none() && state.active.is_none());
    }
}
#[test]
fn pending_review_and_disabled_module_do_not_produce_shape_targets() {
    let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
    assert!(
        f.app
            .world_mut()
            .resource_mut::<EditorSession>()
            .preview_transaction(EffectTransaction::single(
                "Review",
                EffectCommand::SetEffectName {
                    name: "Candidate".into()
                }
            ))
    );
    assert!(selected_shape_module(f.app.world().resource::<EditorSession>()).is_none());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .discard_pending_change();
    let id = f.module;
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .effect
        .emitters[0]
        .modules
        .iter_mut()
        .find(|m| m.id == id)
        .unwrap()
        .enabled = false;
    assert!(selected_shape_module(f.app.world().resource::<EditorSession>()).is_none());
}
#[test]
fn missing_proxy_clears_old_handle_state_and_hides_readout() {
    let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
    f.cursor_world(Vec3::X * 5.0);
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<ShapeGizmoState>()
            .hovered
            .is_some()
    );
    f.app.world_mut().entity_mut(f.player).despawn();
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<ShapeGizmoState>()
            .hovered
            .is_none()
    );
    assert!(f.app.world().resource::<ShapeGizmoState>().active.is_none());
    assert_eq!(
        *f.app.world().get::<Visibility>(f.label).unwrap(),
        Visibility::Hidden
    );
}
#[test]
fn projected_hit_radius_and_degenerate_drag_axis_keep_existing_contracts() {
    let f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
    let camera = f.app.world().get::<Camera>(f.camera).unwrap();
    let transform = f.app.world().get::<GlobalTransform>(f.camera).unwrap();
    let player = f.app.world().get::<GlobalTransform>(f.player).unwrap();
    let center = camera.world_to_viewport(transform, Vec3::X * 5.0).unwrap();
    for (offset, expected) in [(11.0, Some(ShapeGizmoHandle::Radius)), (11.1, None)] {
        assert_eq!(
            hit_test_shape_gizmo(
                camera,
                transform,
                player,
                EmitterShape::Circle { radius: 5.0 },
                center + Vec2::new(offset, 0.0)
            ),
            expected
        );
    }
    assert!(
        shape_gizmo_drag_value(
            camera,
            transform,
            player,
            EmitterShape::Box {
                half_extents: [2.0, 3.0, 4.0]
            },
            ShapeGizmoHandle::ExtentZ,
            center
        )
        .is_none()
    );
    assert!(
        hit_test_shape_gizmo(
            &Camera::default(),
            transform,
            player,
            EmitterShape::Circle { radius: 5.0 },
            center
        )
        .is_none()
    );
}
#[test]
fn transformed_projection_recovers_cylinder_depth_and_cone_radius() {
    let mut f = Fixture::new(EmitterShape::Cylinder {
        radius: 5.0,
        depth: 8.0,
    });
    f.app
        .world_mut()
        .entity_mut(f.player)
        .insert(GlobalTransform::from(Transform {
            translation: Vec3::new(2.0, 3.0, 0.0),
            rotation: Quat::from_rotation_z(0.4),
            scale: Vec3::new(2.0, 3.0, 1.0),
        }));
    let camera = f.app.world().get::<Camera>(f.camera).unwrap();
    let transform = f.app.world().get::<GlobalTransform>(f.camera).unwrap();
    let player = f.app.world().get::<GlobalTransform>(f.player).unwrap();
    for (shape, handle, point, expected) in [
        (
            EmitterShape::Cylinder {
                radius: 5.0,
                depth: 8.0,
            },
            ShapeGizmoHandle::Depth,
            Vec3::Y * 6.0,
            12.0,
        ),
        (
            EmitterShape::Cone {
                radius: 5.0,
                depth: 8.0,
            },
            ShapeGizmoHandle::Radius,
            Vec3::new(9.0, 8.0, 0.0),
            9.0,
        ),
    ] {
        let cursor = camera
            .world_to_viewport(transform, player.transform_point(point))
            .unwrap();
        assert!(
            (shape_gizmo_drag_value(camera, transform, player, shape, handle, cursor).unwrap()
                - expected)
                .abs()
                < 1e-4
        );
    }
}
#[test]
fn localized_readouts_keep_shape_scalar_and_canvas_clamping() {
    for locale in ["en-US", "fr-FR"] {
        for (shape, handle, key, value) in [
            (
                EmitterShape::Circle { radius: 5.0 },
                ShapeGizmoHandle::Radius,
                "viewport-shape-radius",
                5.0,
            ),
            (
                EmitterShape::Cylinder {
                    radius: 5.0,
                    depth: 8.0,
                },
                ShapeGizmoHandle::Depth,
                "viewport-shape-depth",
                8.0,
            ),
            (
                EmitterShape::Box {
                    half_extents: [5.0, 7.0, 9.0],
                },
                ShapeGizmoHandle::ExtentX,
                "viewport-shape-extent-x",
                5.0,
            ),
            (
                EmitterShape::Box {
                    half_extents: [5.0, 7.0, 9.0],
                },
                ShapeGizmoHandle::ExtentY,
                "viewport-shape-extent-y",
                7.0,
            ),
            (
                EmitterShape::Box {
                    half_extents: [5.0, 7.0, 9.0],
                },
                ShapeGizmoHandle::ExtentZ,
                "viewport-shape-extent-z",
                9.0,
            ),
        ] {
            let mut f = Fixture::localized(shape, locale);
            f.start(handle);
            let mut args = FluentArgs::new();
            args.set("value", format!("{value:.2}"));
            let expected = f.app.world().resource::<Localizer>().text_with(key, &args);
            assert_eq!(f.app.world().get::<Text>(f.label).unwrap().0, expected);
            assert_eq!(
                *f.app.world().get::<Visibility>(f.label).unwrap(),
                Visibility::Inherited
            );
            let node = f.app.world().get::<Node>(f.label).unwrap();
            assert!(matches!(node.left,Val::Px(x) if (8.0..=480.0).contains(&x)));
            assert!(matches!(node.top,Val::Px(y) if (8.0..=368.0).contains(&y)));
        }
    }
}
#[test]
fn handle_cursor_icons_keep_axis_specific_resize_shapes() {
    for (handle, icon) in [
        (ShapeGizmoHandle::Radius, SystemCursorIcon::EwResize),
        (ShapeGizmoHandle::Depth, SystemCursorIcon::NsResize),
        (ShapeGizmoHandle::ExtentX, SystemCursorIcon::EwResize),
        (ShapeGizmoHandle::ExtentY, SystemCursorIcon::NsResize),
        (ShapeGizmoHandle::ExtentZ, SystemCursorIcon::NeswResize),
    ] {
        assert_eq!(shape_gizmo_cursor(handle), CursorIcon::System(icon));
    }
}

#[derive(Resource)]
struct DrawInput {
    shape: EmitterShape,
    player: GlobalTransform,
}
fn emit(input: Res<DrawInput>, mut gizmos: Gizmos<PreviewSceneGizmos>) {
    draw_emitter_shape_gizmo(&mut gizmos, &input.player, input.shape, theme::ACCENT);
}
fn geometry(shape: EmitterShape, player: GlobalTransform) -> (Vec<Vec3>, Vec<LinearRgba>) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<GizmoAsset>()
        .init_gizmo_group::<PreviewSceneGizmos>()
        .insert_resource(DrawInput { shape, player })
        .add_systems(Update, emit);
    app.update();
    let assets = app.world().resource::<Assets<GizmoAsset>>();
    let (_, asset) = assets
        .iter()
        .find(|(_, a)| a.config_typeid() == std::any::TypeId::of::<PreviewSceneGizmos>())
        .unwrap();
    let buffer = asset.buffer().buffer();
    (buffer.list_positions.clone(), buffer.list_colors.clone())
}
#[test]
fn all_eight_shapes_emit_original_line_counts_and_finite_geometry() {
    for (shape, vertices) in [
        (EmitterShape::Point, 6),
        (EmitterShape::Circle { radius: 5.0 }, 130),
        (EmitterShape::Ring { radius: 5.0 }, 256),
        (EmitterShape::Sphere { radius: 5.0 }, 384),
        (EmitterShape::Hemisphere { radius: 5.0 }, 768),
        (
            EmitterShape::Box {
                half_extents: [2.0, 3.0, 4.0],
            },
            24,
        ),
        (
            EmitterShape::Cylinder {
                radius: 5.0,
                depth: 8.0,
            },
            264,
        ),
        (
            EmitterShape::Cone {
                radius: 5.0,
                depth: 8.0,
            },
            136,
        ),
    ] {
        let (positions, colors) = geometry(shape, GlobalTransform::IDENTITY);
        assert_eq!(positions.len(), vertices, "{shape:?}");
        assert_eq!(colors.len(), positions.len());
        assert!(positions.iter().all(|p| p.is_finite()));
    }
}
#[test]
fn ring_geometry_preserves_nonuniform_player_transform_and_closed_seam() {
    let player = GlobalTransform::from(Transform {
        translation: Vec3::new(7.0, 8.0, 9.0),
        rotation: Quat::from_rotation_z(0.7),
        scale: Vec3::new(2.0, 3.0, 4.0),
    });
    let (positions, _) = geometry(EmitterShape::Ring { radius: 5.0 }, player);
    assert!(positions[0].abs_diff_eq(player.transform_point(Vec3::X * 5.0), 1e-5));
    assert!(positions[127].abs_diff_eq(positions[0], 1e-5));
    for p in &positions[..128] {
        let local = player.affine().inverse().transform_point3(*p);
        assert!((local.truncate().length() - 5.0).abs() < 1e-4);
        assert!(local.z.abs() < 1e-4);
    }
}
#[test]
fn hemisphere_geometry_stays_above_its_local_equator() {
    let (positions, _) = geometry(
        EmitterShape::Hemisphere { radius: 5.0 },
        GlobalTransform::IDENTITY,
    );
    assert!(positions.iter().all(|p| p.y >= -1e-5));
    assert!(positions.iter().any(|p| (p.y - 5.0).abs() < 1e-5));
}
#[test]
fn drawing_configuration_preserves_overlay_layer_and_line_width() {
    let mut app = App::new();
    app.init_gizmo_group::<PreviewSceneGizmos>();
    app.world_mut()
        .run_system_cached(configure_preview_scene_gizmos)
        .unwrap();
    let (config, _) = app
        .world()
        .resource::<GizmoConfigStore>()
        .config::<PreviewSceneGizmos>();
    assert_eq!(config.render_layers, RenderLayers::layer(15));
    assert_eq!(config.line.width, 1.5);
}
#[test]
fn scene_drawing_includes_live_shape_and_highlights_hovered_handle() {
    let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
    f.drawing();
    f.cursor_world(Vec3::X * 5.0);
    f.tick();
    let assets = f.app.world().resource::<Assets<GizmoAsset>>();
    let (_, asset) = assets
        .iter()
        .find(|(_, a)| a.config_typeid() == std::any::TypeId::of::<PreviewSceneGizmos>())
        .unwrap();
    let buffer = asset.buffer().buffer();
    assert_eq!(buffer.list_positions.len(), 130);
    assert!(!buffer.strip_positions.is_empty());
    assert!(
        buffer
            .list_colors
            .iter()
            .chain(buffer.strip_colors)
            .any(|color| *color == theme::TEXT.to_linear())
    );
    f.start(ShapeGizmoHandle::Radius);
    f.cursor_world(Vec3::X * 8.0);
    f.tick();
    let assets = f.app.world().resource::<Assets<GizmoAsset>>();
    let (_, asset) = assets
        .iter()
        .find(|(_, a)| a.config_typeid() == std::any::TypeId::of::<PreviewSceneGizmos>())
        .unwrap();
    assert!((asset.buffer().buffer().list_positions[0].x - 8.0).abs() < 1e-4);
    assert_eq!(f.authored(), EmitterShape::Circle { radius: 5.0 });
}
#[test]
fn unselected_scene_retires_current_shape_gizmo_handle() {
    let mut f = Fixture::new(EmitterShape::Circle { radius: 5.0 });
    f.drawing();
    f.tick();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .selection
        .primary = SemanticTarget::Emitter(f.emitter);
    f.tick();
    assert!(
        f.app
            .world()
            .resource::<GizmoHandles>()
            .handles()
            .get(&std::any::TypeId::of::<PreviewSceneGizmos>())
            .unwrap()
            .is_none()
    );
}
