//! Installed viewport plugin, real Feathers panel, upstream gizmo plugins and main-world schedules.
use super::super::{
    PreviewDisplayMode, ViewportAction,
    navigation::{EmitterTransformGizmoProxy, PreviewRenderCamera},
    pixel_preview::{PixelPreviewPass, PixelSceneCamera, VIRTUAL_SIZE},
    presentation::{PreviewGridPlane, PreviewPresentedEffect},
    scene::PreviewXyCamera,
    shortcuts::PreviewCanvas,
    workspace::spawn_preview,
    xy_authoring::XyTransformFocus,
    xy_grid::{PreviewXyGrid, XY_GRID_LAYER},
};
use super::*;
use crate::{
    ModulePaletteState, ProjectEffectCatalog,
    docking::MaximizedPanel,
    localization::Localizer,
    menus::MenuState,
    profiler::ProfilerState,
    session::EditorSession,
    settings::{EditorSettings, SettingsPersistence},
    test_support,
    timeline::TimelineState,
};
use aestra_bevy_render::pixel_view::PixelGrid;
use aestra_bevy_render::{
    ActiveBackend, AestraLights2d, EffectRuntimeStatus, PointLightFalloff2d, PresentedEffect,
};
use bevy::{
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo, visibility::RenderLayers},
    gizmos::GizmoAsset,
    gizmos::transform_gizmo::{
        TransformGizmoMeshMarker, TransformGizmoMode, TransformGizmoRoot, TransformGizmoSettings,
        TransformGizmoState,
    },
    gizmos_render::transform_gizmo_render::TransformGizmoRenderPlugin,
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
        mouse::{MouseButtonInput, MouseMotion},
    },
    input_focus::{InputDispatchPlugin, InputFocusPlugin},
    scene::ScenePlugin,
    text::TextPlugin,
    time::TimeUpdateStrategy,
    transform::TransformPlugin,
    ui::RelativeCursorPosition,
    ui_widgets::Activate,
    window::{PrimaryWindow, WindowResolution},
};

struct Fixture {
    app: App,
    _root: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            ScenePlugin,
            TextPlugin,
            TransformPlugin,
            InputPlugin,
            InputFocusPlugin,
            InputDispatchPlugin,
            crate::input::EditorKeyboardPlugin,
        ))
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<Image>()
        .init_asset::<GizmoAsset>()
        .add_plugins(crate::feathers::icon::SvgPlugin)
        .insert_resource(test_support::session_with_timing_slack())
        .insert_resource(ProjectEffectCatalog::scan(root.path()))
        .insert_resource(Localizer::new("en-US").unwrap())
        .insert_resource(SettingsPersistence::for_test(
            root.path().join("settings.ron"),
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(
            std::time::Duration::ZERO,
        ))
        .init_resource::<MenuState>()
        .init_resource::<EditorSettings>()
        .init_resource::<ModulePaletteState>()
        .init_resource::<MaximizedPanel>()
        .init_resource::<TimelineState>()
        .init_resource::<ProfilerState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<bevy::input::mouse::MouseMotion>()
        .add_message::<aestra_bevy_render::gpu::AestraOutputEvent>()
        .add_plugins(ViewportPlugin)
        .add_plugins(TransformGizmoRenderPlugin)
        // Same relative viewport set order as the shipping application's larger chain.
        .configure_sets(
            Update,
            (
                ViewportSet::Input,
                ViewportSet::Actions,
                ViewportSet::Update,
            )
                .chain(),
        )
        .add_systems(Startup, panel.after(ViewportSet::Setup));
        app.world_mut().spawn((
            Window {
                resolution: WindowResolution::new(1000, 800),
                ..default()
            },
            PrimaryWindow,
            bevy::window::CursorIcon::default(),
        ));
        Self { app, _root: root }
    }
    fn tick(&mut self) {
        self.app.update();
    }
    fn single<T: Component>(&mut self) -> Entity {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<T>>().single(w).unwrap()
    }
    fn action(&mut self, action: ViewportAction) -> Entity {
        let w = self.app.world_mut();
        w.query::<(Entity, &ViewportAction)>()
            .iter(w)
            .find(|(_, a)| **a == action)
            .unwrap()
            .0
    }
    fn activate(&mut self, action: ViewportAction) {
        let e = self.action(action);
        self.app.world_mut().trigger(Activate { entity: e });
        self.tick();
    }
    fn tap(&mut self, code: KeyCode, logical: Key) {
        let window = self.single::<PrimaryWindow>();
        for state in [ButtonState::Pressed, ButtonState::Released] {
            self.app.world_mut().write_message(KeyboardInput {
                key_code: code,
                logical_key: logical.clone(),
                state,
                repeat: false,
                text: None,
                window,
            });
        }
    }
    fn cameras(&mut self) -> Vec<Entity> {
        let w = self.app.world_mut();
        w.query_filtered::<Entity, With<Camera3d>>()
            .iter(w)
            .collect()
    }
}
fn panel(mut commands: Commands, localizer: Res<Localizer>, assets: Res<AssetServer>) {
    commands
        .spawn(Node::default())
        .with_children(|p| spawn_preview(p, false, &localizer, &assets));
}

#[test]
fn xy_view_picks_and_frames_either_emitter_and_commits_planar_drags_once() {
    exercise_xy_authoring(false);
}

#[test]
fn pixel_preview_keeps_planar_picking_and_drags_aligned_to_the_letterbox() {
    exercise_xy_authoring(true);
}

fn exercise_xy_authoring(pixel_preview: bool) {
    let mut f = Fixture::new();
    let mut effect = test_support::effect_with_timing_slack();
    let first = effect.emitters[0].id;
    effect.emitters[0].transform.translation = [-16.0, 5.0, 1.0];
    let mut second = aestra_core::Emitter::basic_sprite("Second", 2.5);
    second.transform.translation = [18.0, -8.0, 4.25];
    second.transform.rotation = Quat::from_rotation_x(0.25).to_array();
    second.transform.scale = [1.0, 1.0, 2.0];
    let original = second.transform;
    let second_id = second.id;
    effect.emitters.push(second);
    f.app
        .insert_resource(EditorSession::from_test_effect(effect));
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .select_emitter(first);
    f.tick();
    let canvas = f.single::<PreviewCanvas>();
    let window = f.single::<PrimaryWindow>();
    let viewport = UVec2::new(480, 320);
    f.app.world_mut().entity_mut(canvas).insert((
        ComputedNode {
            size: viewport.as_vec2(),
            ..default()
        },
        UiGlobalTransform::from(bevy::math::Affine2::from_translation(
            Vec2::new(120.0, 80.0) + viewport.as_vec2() * 0.5,
        )),
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        },
    ));
    f.activate(ViewportAction::TogglePreviewProjection);
    if pixel_preview {
        f.activate(ViewportAction::TogglePixelGrid);
    }
    let camera = f.single::<PreviewXyCamera>();
    let viewport = f
        .app
        .world()
        .get::<Camera>(camera)
        .unwrap()
        .viewport
        .as_ref()
        .unwrap()
        .physical_size;
    f.app
        .world_mut()
        .get_mut::<Transform>(camera)
        .unwrap()
        .translation
        .x = 7.0;
    if let Projection::Orthographic(ortho) =
        &mut *f.app.world_mut().get_mut::<Projection>(camera).unwrap()
    {
        ortho.scale = 1.45;
    }
    apply_test_projection(&mut f, camera, viewport);
    if pixel_preview {
        let pixel_scene = f.single::<PixelSceneCamera>();
        let start = f.app.world().get::<Transform>(camera).unwrap().translation;
        for (cursor, expected) in [
            (Vec2::new(145.0, 239.0), start), // Left letterbox.
            (Vec2::new(360.0, 239.0), start + Vec3::new(-5.8, 2.9, 0.0)),
        ] {
            mouse(&mut f, window, cursor, None);
            for state in [ButtonState::Pressed, ButtonState::Released] {
                f.app.world_mut().write_message(MouseButtonInput {
                    button: MouseButton::Middle,
                    state,
                    window,
                });
                if state == ButtonState::Pressed {
                    f.app.world_mut().write_message(MouseMotion {
                        delta: Vec2::new(8.0, 4.0),
                    });
                }
                f.tick();
            }
            for view in [camera, pixel_scene] {
                let pose = f.app.world().get::<Transform>(view).unwrap();
                assert!(pose.translation.abs_diff_eq(expected, 0.001));
            }
        }
    }

    for (id, translation) in [
        (second_id, original.translation),
        (first, [-16.0, 5.0, 1.0]),
        (second_id, original.translation),
    ] {
        let cursor = project(&f, camera, translation);
        mouse(&mut f, window, cursor, Some(ButtonState::Pressed));
        mouse(&mut f, window, cursor, Some(ButtonState::Released));
        assert_eq!(
            f.app.world().resource::<EditorSession>().selection.primary,
            aestra_authoring::SemanticTarget::Emitter(id)
        );
    }
    f.activate(ViewportAction::FramePreview);
    apply_test_projection(&mut f, camera, viewport);
    let center = project(&f, camera, original.translation);
    assert!(center.abs_diff_eq(Vec2::new(360.0, 240.0), 0.01));
    let expected_scale = xy_authoring::frame_scale(
        f.app.world().resource::<EditorSession>(),
        viewport.as_vec2(),
        super::super::navigation::xy_world_height(
            match f.app.world().get::<Projection>(camera).unwrap() {
                Projection::Orthographic(projection) => projection,
                _ => unreachable!(),
            },
        ),
    )
    .unwrap();
    assert!(expected_scale < 1.0);
    assert!(matches!(
        f.app.world().get::<Projection>(camera),
        Some(Projection::Orthographic(ortho)) if ortho.scale == expected_scale
    ));
    let proxy = f.single::<EmitterTransformGizmoProxy>();
    assert!(f.app.world().get::<XyTransformFocus>(proxy).is_some());

    for (mode, start, end) in [
        (
            TransformGizmoMode::Translate,
            Vec2::ZERO,
            Vec2::new(28.0, -19.0),
        ),
        (
            TransformGizmoMode::Rotate,
            Vec2::X * 43.0,
            Vec2::NEG_Y * 43.0,
        ),
        (TransformGizmoMode::Scale, Vec2::Y * -45.0, Vec2::Y * -64.0),
    ] {
        f.activate(ViewportAction::SetTransformGizmoMode(mode));
        mouse(&mut f, window, center + start, Some(ButtonState::Pressed));
        mouse(&mut f, window, center + end, None);
        assert!(
            f.app
                .world()
                .resource::<EmitterTransformGizmoInteraction>()
                .is_active()
        );
        assert!(!f.app.world().resource::<EditorSession>().can_undo());
        mouse(&mut f, window, center + end, Some(ButtonState::Released));

        let mut session = f.app.world_mut().resource_mut::<EditorSession>();
        let current = session.selected_layer().unwrap().transform;
        assert_eq!(current.translation[2], original.translation[2]);
        assert_eq!(current.scale[2], original.scale[2]);
        match mode {
            TransformGizmoMode::Translate => {
                assert!(current.translation[0] > original.translation[0]);
                assert!(current.translation[1] > original.translation[1]);
            }
            TransformGizmoMode::Rotate => {
                let expected = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
                    * Quat::from_array(original.rotation);
                assert!(Quat::from_array(current.rotation).abs_diff_eq(expected, 0.001));
            }
            TransformGizmoMode::Scale => assert!(current.scale[1] > original.scale[1]),
        }
        assert!(session.can_undo());
        session.undo();
        assert_eq!(session.selected_layer().unwrap().transform, original);
        assert!(!session.can_undo());
        f.tick();
    }
    f.activate(ViewportAction::SetTransformGizmoMode(
        TransformGizmoMode::Translate,
    ));
    mouse(&mut f, window, center, Some(ButtonState::Pressed));
    mouse(&mut f, window, center + Vec2::splat(25.0), None);
    f.tap(KeyCode::Escape, Key::Escape);
    f.tick();
    assert!(
        !f.app
            .world()
            .resource::<EmitterTransformGizmoInteraction>()
            .is_active()
    );
    mouse(
        &mut f,
        window,
        center + Vec2::splat(25.0),
        Some(ButtonState::Released),
    );
    let session = f.app.world().resource::<EditorSession>();
    assert_eq!(session.selected_layer().unwrap().transform, original);
    assert!(!session.can_undo());
}

fn apply_test_projection(f: &mut Fixture, camera: Entity, size: UVec2) {
    let world = f.app.world_mut();
    let mut projection = world.get_mut::<Projection>(camera).unwrap();
    projection.update(size.x as f32, size.y as f32);
    let clip_from_view = projection.get_clip_from_view();
    world.get_mut::<Camera>(camera).unwrap().computed = ComputedCameraValues {
        clip_from_view,
        target_info: Some(RenderTargetInfo {
            physical_size: UVec2::new(1000, 800),
            scale_factor: 1.0,
        }),
        ..default()
    };
    f.tick();
}

fn project(f: &Fixture, camera: Entity, translation: [f32; 3]) -> Vec2 {
    let world = f.app.world();
    world
        .get::<Camera>(camera)
        .unwrap()
        .world_to_viewport(
            world.get::<GlobalTransform>(camera).unwrap(),
            Vec3::from_array(translation),
        )
        .unwrap()
}

fn mouse(f: &mut Fixture, window: Entity, position: Vec2, state: Option<ButtonState>) {
    let world = f.app.world_mut();
    world
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(position));
    if let Some(state) = state {
        world.write_message(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window,
        });
    }
    f.tick();
}

#[test]
fn xy_lighting_control_cycles_and_updates_both_scene_cameras() {
    let mut f = Fixture::new();
    f.tick();
    let xy = f.single::<PreviewXyCamera>();
    let pixel = f.single::<PixelSceneCamera>();
    let action = ViewportAction::CyclePreviewLighting2d;
    let button = f.action(action);
    assert_eq!(
        f.app.world().get::<Node>(button).unwrap().display,
        Display::None
    );

    f.activate(ViewportAction::TogglePreviewProjection);
    assert_eq!(
        f.app.world().get::<Node>(button).unwrap().display,
        Display::Flex
    );
    f.activate(action);
    let lights =
        |world: &World, camera| world.get::<AestraLights2d>(camera).unwrap().lights.clone();
    let continuous = lights(f.app.world(), xy);
    assert_eq!(continuous[0].falloff, PointLightFalloff2d::Continuous);
    assert_eq!(continuous, lights(f.app.world(), pixel));

    f.activate(ViewportAction::TogglePixelGrid);
    f.activate(action);
    let stepped = lights(f.app.world(), xy);
    assert_eq!(
        stepped[0].falloff,
        PointLightFalloff2d::Stepped(4.try_into().unwrap())
    );
    assert_eq!(stepped, lights(f.app.world(), pixel));

    f.app.insert_resource(TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(100),
    ));
    f.tick();
    assert_ne!(
        stepped[0].position,
        lights(f.app.world(), pixel)[0].position
    );

    f.activate(action);
    assert!(lights(f.app.world(), xy).is_empty());
    assert!(lights(f.app.world(), pixel).is_empty());
}

#[test]
fn projection_control_switches_the_scene_and_preserves_each_camera_pose() {
    let mut f = Fixture::new();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
    let path = root.join("effects/pixel_pulse.aestra.ron");
    let effect = aestra_core::EffectAsset::load_ron(&path).unwrap();
    let catalog = ProjectEffectCatalog::scan(&root);
    let compiled = catalog.compile_project(&effect).unwrap();
    f.app.insert_resource(catalog);
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .open_compiled_effect(path, effect, compiled.root);
    f.tick();
    let perspective = f.single::<PreviewRenderCamera>();
    let xy = f.single::<PreviewXyCamera>();
    let xy_grid = f.single::<PreviewXyGrid>();
    let perspective_grid = f.single::<PreviewGridPlane>();
    assert_eq!(
        *f.app.world().get::<RenderLayers>(xy).unwrap(),
        RenderLayers::from_layers(&[0, XY_GRID_LAYER])
    );
    assert_eq!(
        f.app.world().get::<Visibility>(xy_grid),
        Some(&Visibility::Hidden)
    );
    f.app.world_mut().resource_mut::<MenuState>().show_grid = true;
    let overlay = f
        .cameras()
        .into_iter()
        .find(|&id| id != perspective)
        .unwrap();
    let canvas = f.single::<PreviewCanvas>();
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(canvas)
        .unwrap()
        .size = Vec2::splat(360.0);
    f.tick();
    f.tick();
    let original = *f.app.world().get::<Transform>(perspective).unwrap();
    let effect = f.app.world().resource::<EditorSession>().effect.clone();
    let player = f.single::<PreviewPresentedEffect>();
    let control = f.action(ViewportAction::TogglePreviewProjection);
    let pixel_control = f.action(ViewportAction::TogglePixelGrid);
    let cadence_control = f.action(ViewportAction::TogglePresentationCadence);
    let pixel_scene = f.single::<PixelSceneCamera>();
    let pixel_composite = {
        let world = f.app.world_mut();
        world
            .query::<(Entity, &PixelPreviewPass)>()
            .iter(world)
            .find_map(|(entity, pass)| {
                matches!(pass, PixelPreviewPass::Composite).then_some(entity)
            })
            .unwrap()
    };
    assert!(f.app.world().get::<PixelGrid>(pixel_scene).is_some());
    assert!(!f.app.world().get::<Camera>(pixel_scene).unwrap().is_active);
    assert_eq!(
        f.app.world().get::<Node>(pixel_control).unwrap().display,
        Display::None
    );
    assert!(f.app.world().get::<PixelGrid>(xy).is_none());

    f.app.world_mut().trigger(Activate { entity: control });
    f.tick();
    assert_eq!(f.single::<PreviewRenderCamera>(), xy);
    assert!(f.app.world().get::<Camera2d>(xy).is_some());
    assert!(matches!(
        f.app.world().get::<Projection>(xy).unwrap(),
        Projection::Orthographic(_)
    ));
    assert!(matches!(
        f.app.world().get::<Projection>(overlay).unwrap(),
        Projection::Orthographic(_)
    ));
    assert!(f.app.world().get::<Camera>(xy).unwrap().is_active);
    assert!(!f.app.world().get::<Camera>(perspective).unwrap().is_active);
    assert_eq!(
        f.app.world().get::<Node>(pixel_control).unwrap().display,
        Display::Flex
    );
    f.activate(ViewportAction::TogglePixelGrid);
    assert_eq!(
        f.app.world().get::<Node>(cadence_control).unwrap().display,
        Display::Flex
    );
    f.activate(ViewportAction::TogglePresentationCadence);
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .presentation_cadence(),
        Some(aestra_runtime::PresentationCadence::FPS_12)
    );
    let camera =
        |fixture: &Fixture, entity| fixture.app.world().get::<Camera>(entity).unwrap().clone();
    assert!(!camera(&f, xy).is_active);
    assert!(camera(&f, pixel_scene).is_active);
    assert!(camera(&f, pixel_scene).viewport.is_none());
    assert!(camera(&f, pixel_composite).is_active);
    let fitted = camera(&f, pixel_composite).viewport.unwrap();
    assert_eq!(fitted.physical_size, VIRTUAL_SIZE);
    assert_eq!(fitted.physical_position, UVec2::new(84, 116));
    for view in [xy, overlay] {
        let viewport = camera(&f, view).viewport.unwrap();
        assert_eq!(viewport.physical_size, fitted.physical_size);
        assert_eq!(viewport.physical_position, fitted.physical_position);
    }
    // A narrow dock goes back to the live XY scene and restores the pixel
    // view automatically when there is room for a 1x presentation.
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(canvas)
        .unwrap()
        .size
        .x = 180.0;
    f.tick();
    assert!(camera(&f, xy).is_active);
    assert!(!camera(&f, pixel_composite).is_active);
    f.app
        .world_mut()
        .get_mut::<ComputedNode>(canvas)
        .unwrap()
        .size
        .x = 360.0;
    f.tick();
    assert!(!camera(&f, xy).is_active);
    assert!(camera(&f, pixel_composite).is_active);
    assert_eq!(
        f.app.world().get::<Visibility>(xy_grid),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        f.app.world().get::<Visibility>(perspective_grid),
        Some(&Visibility::Hidden)
    );
    let mut pan = *f.app.world().get::<Transform>(xy).unwrap();
    pan.translation.x = 13.0;
    pan.translation.y = 7.0;
    *f.app.world_mut().get_mut::<Transform>(xy).unwrap() = pan;

    f.app.world_mut().trigger(Activate { entity: control });
    f.tick();
    assert_eq!(f.single::<PreviewRenderCamera>(), perspective);
    assert!(matches!(
        f.app.world().get::<Projection>(overlay).unwrap(),
        Projection::Perspective(_)
    ));
    assert!(f.app.world().get::<Camera>(perspective).unwrap().is_active);
    assert_eq!(
        f.app.world().get::<Visibility>(xy_grid),
        Some(&Visibility::Hidden)
    );
    assert_eq!(
        f.app.world().get::<Visibility>(perspective_grid),
        Some(&Visibility::Inherited)
    );
    assert_eq!(
        *f.app.world().get::<Transform>(perspective).unwrap(),
        original
    );
    assert!(!f.app.world().get::<Camera>(xy).unwrap().is_active);
    assert!(!camera(&f, pixel_scene).is_active);
    assert!(!camera(&f, pixel_composite).is_active);
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .presentation_cadence(),
        None
    );

    f.app.world_mut().trigger(Activate { entity: control });
    f.tick();
    assert_eq!(f.single::<PreviewRenderCamera>(), xy);
    assert_eq!(*f.app.world().get::<Transform>(xy).unwrap(), pan);
    assert!(camera(&f, pixel_scene).is_active);
    assert_eq!(*f.app.world().get::<Transform>(pixel_scene).unwrap(), pan);
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .presentation_cadence(),
        Some(aestra_runtime::PresentationCadence::FPS_12)
    );
    f.activate(ViewportAction::TogglePixelGrid);
    assert!(!camera(&f, pixel_scene).is_active);
    assert!(camera(&f, xy).is_active);
    assert_eq!(
        f.app.world().get::<Node>(cadence_control).unwrap().display,
        Display::None
    );
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .presentation_cadence(),
        None
    );
    assert_eq!(f.single::<PreviewPresentedEffect>(), player);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, effect);
}

#[test]
fn installed_plugin_runs_startup_update_and_postupdate_with_actual_upstream_gizmos() {
    let mut f = Fixture::new();
    f.tick();
    assert!(
        f.app
            .is_plugin_added::<bevy::gizmos::transform_gizmo::TransformGizmoPlugin>()
    );
    assert!(
        f.app
            .is_plugin_added::<MaterialPlugin<PreviewGridMaterial>>()
    );
    f.single::<PreviewRenderCamera>();
    f.single::<PreviewGridPlane>();
    f.single::<PreviewPresentedEffect>();
    f.single::<EmitterTransformGizmoProxy>();
    assert!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .project
            .is_some()
    );
    assert!(!f.app.world().resource::<EditorSession>().dirty);
}

#[test]
fn plugin_initializes_real_interaction_resources_and_aestra_gizmo_settings() {
    let f = Fixture::new();
    let w = f.app.world();
    let settings = w.resource::<TransformGizmoSettings>();
    assert_eq!(
        settings.screen_scale_factor,
        super::super::transform_gizmo::PREVIEW_TRANSFORM_GIZMO_SCREEN_SCALE
    );
    assert_eq!(
        settings.axis_hit_distance,
        super::super::transform_gizmo::PREVIEW_TRANSFORM_GIZMO_HIT_DISTANCE
    );
    assert!(!w.resource::<TransformGizmoState>().active);
    assert!(w.resource::<ShapeGizmoState>().active.is_none());
    assert!(!w.resource::<EmitterTransformGizmoInteraction>().is_active());
    assert!(w.contains_resource::<host_motion::HostMotionViewport>());
    assert!(w.contains_resource::<PreviewNavigationState>());
    assert!(w.contains_resource::<bevy::gizmos::config::GizmoConfigStore>());
}

#[test]
fn poststartup_configures_the_actual_upstream_overlay_and_mesh_materials() {
    let mut f = Fixture::new();
    f.tick();
    let root = f.single::<TransformGizmoRoot>();
    assert!(
        f.app
            .world()
            .get::<TransformGizmoVisualRoot>(root)
            .is_some()
    );
    let w = f.app.world_mut();
    let overlay = w
        .query_filtered::<(&Camera, &RenderLayers), (With<Camera3d>, Without<PreviewRenderCamera>)>(
        )
        .single(w)
        .unwrap();
    assert_eq!(overlay.1, &RenderLayers::layer(15));
    assert_eq!(overlay.0.order, 1);
    assert!(matches!(overlay.0.clear_color, ClearColorConfig::None));
    let materials = w.resource::<Assets<StandardMaterial>>();
    assert!(!materials.is_empty());
    for (_, material) in materials.iter() {
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
        assert_eq!(material.depth_bias, 10_000.0);
    }
}

#[test]
fn installed_startup_does_not_duplicate_scene_or_toolbar_on_later_updates() {
    let mut f = Fixture::new();
    f.tick();
    let root = f.single::<PreviewPresentedEffect>();
    let camera = f.single::<PreviewRenderCamera>();
    let grid = f.single::<PreviewGridPlane>();
    let gizmo = f.single::<TransformGizmoRoot>();
    let controls = f
        .app
        .world_mut()
        .query::<&ViewportAction>()
        .iter(f.app.world())
        .count();
    let meshes = f.app.world().resource::<Assets<Mesh>>().len();
    for _ in 0..4 {
        f.tick();
    }
    assert_eq!(f.single::<PreviewPresentedEffect>(), root);
    assert_eq!(f.single::<PreviewRenderCamera>(), camera);
    assert_eq!(f.single::<PreviewGridPlane>(), grid);
    assert_eq!(f.single::<TransformGizmoRoot>(), gizmo);
    assert_eq!(
        f.app
            .world_mut()
            .query::<&ViewportAction>()
            .iter(f.app.world())
            .count(),
        controls
    );
    assert_eq!(f.app.world().resource::<Assets<Mesh>>().len(), meshes);
}

#[test]
fn real_toolbar_activation_reaches_display_and_upstream_gizmo_meshes_in_one_update() {
    let mut f = Fixture::new();
    f.tick();
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    f.activate(ViewportAction::SetTransformGizmoMode(
        TransformGizmoMode::Rotate,
    ));
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().mode,
        TransformGizmoMode::Rotate
    );
    let w = f.app.world_mut();
    let mut visible = 0;
    for (marker, visibility) in w
        .query::<(&TransformGizmoMeshMarker, &Visibility)>()
        .iter(w)
    {
        assert_eq!(
            *visibility,
            if marker.mode == TransformGizmoMode::Rotate {
                visible += 1;
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }
        );
    }
    assert!(visible > 0);
    f.activate(ViewportAction::SetPreviewDisplayMode(
        PreviewDisplayMode::Wireframe,
    ));
    let root = f.single::<PreviewPresentedEffect>();
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(root)
            .unwrap()
            .render_mode(),
        aestra_bevy_render::EffectRenderMode::Wireframe
    );
    let s = f.app.world().resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert!(!s.dirty && !s.can_undo());
}

#[test]
fn actual_keyboard_dispatch_uses_installed_input_actions_and_feedback_sets() {
    let mut f = Fixture::new();
    f.tick();
    let canvas = f.single::<PreviewCanvas>();
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(canvas)
        .unwrap()
        .cursor_over = true;
    f.tap(KeyCode::Digit2, Key::Character("é".into()));
    f.tap(KeyCode::F11, Key::F11);
    f.tick();
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().mode,
        TransformGizmoMode::Rotate
    );
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(crate::docking::ToolPanel::Viewport)
    );
    f.tick();
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(crate::docking::ToolPanel::Viewport)
    );
    assert!(!f.app.world().resource::<EditorSession>().dirty);
}

#[test]
fn modal_blocks_installed_keyboard_then_a_fresh_press_recovers_without_replay() {
    let mut f = Fixture::new();
    f.tick();
    f.app.world_mut().resource_mut::<MenuState>().show_about = true;
    f.tap(KeyCode::F11, Key::F11);
    f.tick();
    assert_eq!(f.app.world().resource::<MaximizedPanel>().0, None);
    f.app.world_mut().resource_mut::<MenuState>().show_about = false;
    f.tick();
    assert_eq!(f.app.world().resource::<MaximizedPanel>().0, None);
    f.tap(KeyCode::F11, Key::F11);
    f.tick();
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(crate::docking::ToolPanel::Viewport)
    );
}

#[derive(Resource)]
struct Enabled(bool);

#[test]
fn application_action_run_condition_defers_real_activation_then_consumes_it_once() {
    let mut f = Fixture::new();
    f.tick();
    f.app.insert_resource(Enabled(false)).configure_sets(
        Update,
        ViewportSet::Actions.run_if(|enabled: Res<Enabled>| enabled.0),
    );
    let button = f.action(ViewportAction::ToggleMaximize);
    f.app.world_mut().trigger(Activate { entity: button });
    f.tick();
    assert_eq!(f.app.world().resource::<MaximizedPanel>().0, None);
    assert!(
        f.app
            .world()
            .get::<crate::feathers::action_activation::PendingFeathersActivation>(button)
            .is_some()
    );
    f.app.world_mut().resource_mut::<Enabled>().0 = true;
    f.tick();
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(crate::docking::ToolPanel::Viewport)
    );
    assert!(
        f.app
            .world()
            .get::<crate::feathers::action_activation::PendingFeathersActivation>(button)
            .is_none()
    );
    f.tick();
    assert_eq!(
        f.app.world().resource::<MaximizedPanel>().0,
        Some(crate::docking::ToolPanel::Viewport)
    );
}

#[test]
fn application_update_run_condition_holds_player_sync_then_resumes_current_time() {
    let mut f = Fixture::new();
    f.tick();
    let root = f.single::<PreviewPresentedEffect>();
    let before = f
        .app
        .world()
        .get::<PresentedEffect>(root)
        .unwrap()
        .simulation_time();
    f.app.insert_resource(Enabled(false)).configure_sets(
        Update,
        ViewportSet::Update.run_if(|enabled: Res<Enabled>| enabled.0),
    );
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(2.0);
    f.tick();
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(root)
            .unwrap()
            .simulation_time(),
        before
    );
    f.app.world_mut().resource_mut::<Enabled>().0 = true;
    f.tick();
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(root)
            .unwrap()
            .simulation_time(),
        2.0
    );
    assert_eq!(f.single::<PreviewPresentedEffect>(), root);
    assert!(!f.app.world().resource::<EditorSession>().dirty);
}

#[test]
fn empty_document_runs_the_installed_plugin_and_hides_upstream_transform_visuals() {
    let mut f = Fixture::new();
    f.app.insert_resource(EditorSession::from_test_effect(
        aestra_core::EffectAsset::new("Empty", 4.0),
    ));
    f.tick();
    f.single::<PreviewPresentedEffect>();
    let root = f.single::<TransformGizmoRoot>();
    assert_eq!(
        *f.app.world().get::<Visibility>(root).unwrap(),
        Visibility::Hidden
    );
    assert!(f.app.world().resource::<EditorSession>().samples.is_empty());
    assert_eq!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .live_particle_count,
        0
    );
    assert!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .error
            .is_none()
    );
}

#[derive(Resource, Clone, Copy)]
struct Layout {
    origin: Vec2,
    size: Vec2,
}
fn layout(
    geometry: Res<Layout>,
    mut canvas: Single<(&mut ComputedNode, &mut UiGlobalTransform), With<PreviewCanvas>>,
) {
    canvas.0.size = geometry.size;
    *canvas.1 = UiGlobalTransform::from(bevy::math::Affine2::from_translation(
        geometry.origin + geometry.size * 0.5,
    ));
}
#[derive(Resource, Default)]
struct GizmoViews(Vec<(bool, UVec2, UVec2)>);
fn observe_gizmo_views(cameras: Query<&Camera, With<Camera3d>>, mut views: ResMut<GizmoViews>) {
    views.0 = cameras
        .iter()
        .map(|c| {
            let vp = c.viewport.as_ref().unwrap();
            (c.is_active, vp.physical_position, vp.physical_size)
        })
        .collect();
}
fn with_layout() -> Fixture {
    let mut f = Fixture::new();
    f.app
        .insert_resource(Layout {
            origin: Vec2::new(100.0, 60.0),
            size: Vec2::new(400.0, 300.0),
        })
        .init_resource::<GizmoViews>()
        // Mirror the real 0.20 ordering even without a native UI surface. A
        // Layout -> whole gizmo-set edge would cycle through propagation.
        .configure_sets(
            PostUpdate,
            UiSystems::Layout.after(bevy::transform::TransformSystems::Propagate),
        )
        // Supply layout output without a native UI/render surface. Production consumers are unmodified.
        .add_systems(PostUpdate, layout.in_set(UiSystems::Layout))
        .add_systems(
            PostUpdate,
            observe_gizmo_views.in_set(TransformGizmoRenderStep),
        );
    f
}
fn assert_views(f: &mut Fixture, active: bool, origin: UVec2, size: UVec2) {
    assert_eq!(f.app.world().resource::<GizmoViews>().0.len(), 2);
    for view in &f.app.world().resource::<GizmoViews>().0 {
        assert_eq!(*view, (active, origin, size));
    }
    for e in f.cameras() {
        let c = f.app.world().get::<Camera>(e).unwrap();
        assert_eq!(c.is_active, active);
        assert_eq!(c.viewport.as_ref().unwrap().physical_position, origin);
        assert_eq!(c.viewport.as_ref().unwrap().physical_size, size);
    }
}

#[test]
fn installed_postupdate_assigns_layout_before_gizmo_render_and_delays_both_camera_activations() {
    let mut f = with_layout();
    f.tick();
    assert_views(&mut f, false, UVec2::new(100, 60), UVec2::new(400, 300));
    f.tick();
    assert_views(&mut f, true, UVec2::new(100, 60), UVec2::new(400, 300));
    *f.app.world_mut().resource_mut::<Layout>() = Layout {
        origin: Vec2::new(200.0, 120.0),
        size: Vec2::new(600.0, 420.0),
    };
    f.tick();
    assert_views(&mut f, true, UVec2::new(200, 120), UVec2::new(600, 420));
}

#[test]
fn invalid_layout_disables_both_installed_cameras_and_restore_rearms_them() {
    let mut f = with_layout();
    f.tick();
    f.tick();
    f.app.world_mut().resource_mut::<Layout>().size = Vec2::ZERO;
    f.tick();
    for e in f.cameras() {
        assert!(!f.app.world().get::<Camera>(e).unwrap().is_active);
    }
    f.app.world_mut().resource_mut::<Layout>().size = Vec2::new(400.0, 300.0);
    f.tick();
    assert_views(&mut f, false, UVec2::new(100, 60), UVec2::new(400, 300));
    f.tick();
    assert_views(&mut f, true, UVec2::new(100, 60), UVec2::new(400, 300));
}

#[test]
fn seek_and_seed_updates_reach_the_installed_player_without_document_or_entity_changes() {
    let mut f = Fixture::new();
    f.tick();
    let root = f.single::<PreviewPresentedEffect>();
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    {
        let mut s = f.app.world_mut().resource_mut::<EditorSession>();
        s.seek_time(2.5);
        s.preview_seed = 55;
    }
    f.tick();
    assert_eq!(f.single::<PreviewPresentedEffect>(), root);
    let player = f.app.world().get::<PresentedEffect>(root).unwrap();
    assert_eq!(player.simulation_time(), 2.5);
    assert_eq!(player.instance.seed(), 55);
    f.tick();
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(root)
            .unwrap()
            .simulation_time(),
        2.5
    );
    let s = f.app.world().resource::<EditorSession>();
    assert_eq!(s.effect, before);
    assert!(!s.dirty && !s.can_undo());
}

#[test]
fn semantic_live_transform_edit_and_undo_reach_the_installed_proxy_without_respawning_player() {
    let mut f = Fixture::new();
    f.tick();
    let root = f.single::<PreviewPresentedEffect>();
    let proxy = f.single::<EmitterTransformGizmoProxy>();
    let s = f.app.world().resource::<EditorSession>();
    let id = s.effect.emitters[0].id;
    let before = s.effect.clone();
    assert!(f.app.world_mut().resource_mut::<EditorSession>().execute(
        "Move emitter",
        aestra_authoring::EffectCommand::SetEmitterTransform {
            id,
            transform: aestra_core::EmitterTransform {
                translation: [3.0, 4.0, 5.0],
                ..default()
            }
        },
        true
    ));
    f.tick();
    assert_eq!(f.single::<PreviewPresentedEffect>(), root);
    assert_eq!(
        f.app.world().get::<Transform>(proxy).unwrap().translation,
        Vec3::new(3.0, 4.0, 5.0)
    );
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect_undo_len(),
        1
    );
    f.app.world_mut().resource_mut::<EditorSession>().undo();
    f.tick();
    assert_eq!(f.single::<PreviewPresentedEffect>(), root);
    assert_eq!(f.app.world().resource::<EditorSession>().effect, before);
    assert_eq!(
        f.app.world().get::<Transform>(proxy).unwrap().translation,
        Vec3::ZERO
    );
}

#[derive(Resource, Default)]
struct PreparedTrace(Option<u64>);
fn prepare_probe(
    mut commands: Commands,
    players: Query<(Entity, &PresentedEffect), With<PreviewPresentedEffect>>,
    mut trace: ResMut<PreparedTrace>,
) {
    let (e, p) = players.single().unwrap();
    trace.0 = p.instance.binding_trace().map(|t| t.identity());
    commands.entity(e).insert(EffectRuntimeStatus {
        active: ActiveBackend::Gpu,
        reason: "Test preparation observation, no GPU dispatch".into(),
        compatibility: aestra_runtime::CompatibilityReport::compatible(
            aestra_runtime::CompatibilityTarget::NativeGpu,
        ),
    });
}

#[test]
fn installed_prepare_boundary_receives_current_mock_trace_before_telemetry_and_status() {
    let mut f = Fixture::new();
    let mut effect = test_support::effect_with_timing_slack();
    let binding =
        aestra_core::EffectBinding::spatial("Target", aestra_core::BindingUpdateMode::Live);
    let id = binding.id;
    effect.bindings.push(binding);
    let mut session = EditorSession::from_test_effect(effect);
    session
        .preview_mocks
        .insert(id, crate::preview_mocks::PreviewMock::Orbit);
    session.seek_time(1.0);
    f.app
        .insert_resource(session)
        .init_resource::<PreparedTrace>()
        .add_systems(Update, prepare_probe.in_set(AestraRenderSet::Prepare));
    f.tick();
    let trace = f.app.world().resource::<PreparedTrace>().0.unwrap();
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .preview()
            .unwrap()
            .binding_trace()
            .unwrap()
            .identity(),
        trace
    );
    // Preparation publishes GPU status before ingestion: no CPU evaluation is manufactured.
    assert!(f.app.world().resource::<EditorSession>().samples.is_empty());
    assert_eq!(
        f.app
            .world()
            .resource::<EditorPreviewProject>()
            .live_particle_count,
        0
    );
    assert!(
        f.app
            .world_mut()
            .query::<&Text>()
            .iter(f.app.world())
            .any(|t| t.0 == "0 LIVE PARTICLES  |  NATIVE GPU")
    );
}
