//! Installed viewport plugin, real Feathers panel, upstream gizmo plugins and main-world schedules.
use super::super::{
    PreviewDisplayMode, ViewportAction,
    navigation::{EmitterTransformGizmoProxy, PreviewRenderCamera},
    presentation::{PreviewGridPlane, PreviewPresentedEffect},
    shortcuts::PreviewCanvas,
    workspace::spawn_preview,
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
use aestra_bevy_render::{ActiveBackend, EffectRuntimeStatus, PresentedEffect};
use bevy::{
    asset::AssetPlugin,
    camera::visibility::RenderLayers,
    gizmos::GizmoAsset,
    gizmos::transform_gizmo::{
        TransformGizmoMeshMarker, TransformGizmoMode, TransformGizmoRoot, TransformGizmoSettings,
        TransformGizmoState,
    },
    gizmos_render::transform_gizmo_render::TransformGizmoRenderPlugin,
    input::{
        ButtonState, InputPlugin,
        keyboard::{Key, KeyboardInput},
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
