//! Actual grid assets, PresentedEffect payloads and viewport icon/text consumers.
use super::super::navigation::{DEFAULT_PREVIEW_PITCH, preview_camera_transform};
use super::*;
use std::sync::Arc;

struct Fixture {
    app: App,
    grid: Entity,
    camera: Entity,
    canvas: Entity,
    material: Handle<PreviewGridMaterial>,
}
impl Fixture {
    fn new() -> Self {
        let mut app = App::new();
        app.init_resource::<MenuState>()
            .init_resource::<PreviewCameraController>()
            .init_resource::<PreviewDisplayState>()
            .init_resource::<TransformGizmoSettings>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<Assets<PreviewGridMaterial>>()
            .add_systems(
                Update,
                (
                    sync_preview_grid,
                    sync_preview_display_mode,
                    update_preview_display_controls,
                    update_transform_gizmo_controls,
                )
                    .chain(),
            );
        let camera = app
            .world_mut()
            .spawn((
                PreviewRenderCamera,
                GlobalTransform::from(preview_camera_transform(
                    Vec3::ZERO,
                    140.0,
                    0.0,
                    DEFAULT_PREVIEW_PITCH,
                )),
            ))
            .id();
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
        Self {
            app,
            grid,
            camera,
            canvas,
            material,
        }
    }
    fn tick(&mut self) {
        self.app.update();
        self.app
            .world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
    }
    fn uniform(&self) -> PreviewGridUniform {
        self.app
            .world()
            .resource::<Assets<PreviewGridMaterial>>()
            .get(&self.material)
            .unwrap()
            .grid
    }
    fn display(&mut self, mode: PreviewDisplayMode) {
        self.app
            .world_mut()
            .resource_mut::<PreviewDisplayState>()
            .set_mode(mode);
    }
    fn player(&mut self, owned: bool) -> Entity {
        let compiled = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&aestra_core::EffectAsset::new("Display", 3.0))
                .unwrap(),
        );
        let mut player = PresentedEffect::new(compiled)
            .with_history_policy(aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly);
        player.instance.set_seed(42);
        player.instance.seek(0.75);
        player.set_seek_quality(aestra_runtime::SeekQuality::Preview);
        let mut entity = self.app.world_mut().spawn((player, Visibility::Hidden));
        if owned {
            entity.insert(PreviewPresentedEffect);
        }
        entity.id()
    }
    fn icons(&mut self) -> (Entity, Entity) {
        let wireframe = self
            .app
            .world_mut()
            .spawn((
                PreviewDisplayModeIcon(PreviewDisplayMode::Wireframe),
                SvgColor(Color::NONE),
            ))
            .id();
        let rendered = self
            .app
            .world_mut()
            .spawn((
                PreviewDisplayModeIcon(PreviewDisplayMode::Rendered),
                SvgColor(Color::NONE),
            ))
            .id();
        (wireframe, rendered)
    }
    fn gizmo_icons(&mut self) -> [Entity; 3] {
        [
            TransformGizmoMode::Translate,
            TransformGizmoMode::Rotate,
            TransformGizmoMode::Scale,
        ]
        .map(|mode| {
            self.app
                .world_mut()
                .spawn((TransformGizmoModeIcon(mode), SvgColor(Color::NONE)))
                .id()
        })
    }
}

#[test]
fn uniform_matches_the_shader_record_size_colors_spacing_fade_and_world_origin() {
    let uniform = preview_grid_uniform(180.0, 0.4);
    assert_eq!(<PreviewGridUniform as ShaderType>::min_size().get(), 96);
    assert_eq!(uniform.minor_color, Vec4::new(0.20, 0.22, 0.29, 0.22));
    assert_eq!(uniform.major_color, Vec4::new(0.27, 0.30, 0.39, 0.38));
    assert_eq!(uniform.x_axis_color, Vec4::new(0.58, 0.20, 0.27, 0.62));
    assert_eq!(uniform.z_axis_color, Vec4::new(0.20, 0.40, 0.68, 0.62));
    assert_eq!(uniform.parameters, Vec4::new(10.0, 1440.0, 0.4, 10.0));
    assert_eq!(uniform.focus, Vec4::ZERO);
}

#[test]
fn grid_remains_world_anchored_while_camera_focus_and_distance_change() {
    let mut f = Fixture::new();
    for (distance, radius) in [
        (1.0, 200.0),
        (140.0, 3360.0),
        (1000.0, 24_000.0),
        (20_000.0, 100_000.0),
    ] {
        {
            let mut controller = f.app.world_mut().resource_mut::<PreviewCameraController>();
            controller.focus = Vec3::new(123.0, 80.0, -72.0);
            controller.distance = distance;
        }
        f.tick();
        let transform = f.app.world().get::<Transform>(f.grid).unwrap();
        assert_eq!(transform.translation, Vec3::new(0.0, PREVIEW_GRID_Y, 0.0));
        assert_eq!(transform.scale, Vec3::new(radius, 1.0, radius));
        assert_eq!(f.uniform().focus, Vec4::ZERO);
    }
}

#[test]
fn menu_visibility_controls_only_the_grid_and_does_not_remove_its_asset() {
    let mut f = Fixture::new();
    for (show, expected) in [(false, Visibility::Hidden), (true, Visibility::Inherited)] {
        f.app.world_mut().resource_mut::<MenuState>().show_grid = show;
        f.tick();
        assert_eq!(*f.app.world().get::<Visibility>(f.grid).unwrap(), expected);
        assert!(
            f.app
                .world()
                .resource::<Assets<PreviewGridMaterial>>()
                .get(&f.material)
                .is_some()
        );
    }
}

#[test]
fn camera_angle_controls_grid_fade_and_not_its_world_center() {
    let mut f = Fixture::new();
    for (pitch, expected) in [(0.0_f32, 0.0_f32), (-0.095_f32.asin(), 0.5), (-1.0, 1.0)] {
        f.app
            .world_mut()
            .entity_mut(f.camera)
            .insert(GlobalTransform::from(preview_camera_transform(
                Vec3::ZERO,
                140.0,
                0.0,
                pitch,
            )));
        f.tick();
        assert!((f.uniform().parameters.z - expected).abs() < 0.0001);
        assert_eq!(f.uniform().focus, Vec4::ZERO);
    }
}

#[test]
fn missing_grid_material_does_not_prevent_transform_or_visibility_updates() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<Assets<PreviewGridMaterial>>()
        .remove(f.material.id());
    f.app.world_mut().resource_mut::<MenuState>().show_grid = false;
    f.tick();
    assert_eq!(
        *f.app.world().get::<Visibility>(f.grid).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        f.app.world().get::<Transform>(f.grid).unwrap().translation,
        Vec3::new(0.0, PREVIEW_GRID_Y, 0.0)
    );
}

#[test]
fn unrelated_grid_material_and_unmarked_plane_are_not_rewritten() {
    let mut f = Fixture::new();
    let original = preview_grid_uniform(987.0, 0.12);
    let material = f
        .app
        .world_mut()
        .resource_mut::<Assets<PreviewGridMaterial>>()
        .add(PreviewGridMaterial { grid: original });
    let transform = Transform::from_xyz(8.0, 9.0, 10.0);
    let other = f
        .app
        .world_mut()
        .spawn((
            transform,
            Visibility::Hidden,
            MeshMaterial3d(material.clone()),
        ))
        .id();
    f.tick();
    assert_eq!(*f.app.world().get::<Transform>(other).unwrap(), transform);
    assert_eq!(
        *f.app.world().get::<Visibility>(other).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        f.app
            .world()
            .resource::<Assets<PreviewGridMaterial>>()
            .get(&material)
            .unwrap()
            .grid,
        original
    );
}

#[test]
fn missing_or_ambiguous_grid_ownership_defers_the_single_owner_consumer() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .entity_mut(f.grid)
        .remove::<PreviewGridPlane>();
    f.tick();
    assert_eq!(
        *f.app.world().get::<Transform>(f.grid).unwrap(),
        Transform::default()
    );
    f.app
        .world_mut()
        .entity_mut(f.grid)
        .insert(PreviewGridPlane);
    let extra = f
        .app
        .world_mut()
        .spawn((
            PreviewGridPlane,
            Transform::default(),
            Visibility::Inherited,
            MeshMaterial3d(f.material.clone()),
        ))
        .id();
    f.tick();
    assert_eq!(
        *f.app.world().get::<Transform>(f.grid).unwrap(),
        Transform::default()
    );
    f.app.world_mut().despawn(extra);
    f.tick();
    assert_eq!(
        f.app
            .world()
            .get::<Transform>(f.grid)
            .unwrap()
            .translation
            .y,
        PREVIEW_GRID_Y
    );
}

#[test]
fn replacement_material_handle_is_updated_without_changing_the_old_asset() {
    let mut f = Fixture::new();
    let before = f.uniform();
    let replacement = f
        .app
        .world_mut()
        .resource_mut::<Assets<PreviewGridMaterial>>()
        .add(PreviewGridMaterial {
            grid: preview_grid_uniform(1.0, 0.0),
        });
    f.app
        .world_mut()
        .entity_mut(f.grid)
        .insert(MeshMaterial3d(replacement.clone()));
    f.tick();
    let assets = f.app.world().resource::<Assets<PreviewGridMaterial>>();
    assert_eq!(assets.get(&f.material).unwrap().grid, before);
    assert_eq!(
        assets.get(&replacement).unwrap().grid.parameters.x,
        adaptive_grid_spacing(140.0 / 18.0)
    );
}

#[test]
fn adaptive_spacing_retains_stable_one_two_five_steps_and_invalid_input_fallbacks() {
    for (desired, expected) in [
        (0.04, 0.05),
        (0.7, 1.0),
        (1.01, 2.0),
        (2.01, 5.0),
        (5.01, 10.0),
        (14.0, 20.0),
        (260.0, 500.0),
    ] {
        assert!((adaptive_grid_spacing(desired) - expected).abs() < 0.00001);
    }
    for desired in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(adaptive_grid_spacing(desired), 1.0);
    }
}

#[test]
fn material_retains_its_shader_path_alpha_depth_prepass_and_shadow_contract() {
    let material = PreviewGridMaterial {
        grid: preview_grid_uniform(140.0, 1.0),
    };
    assert!(
        matches!(PreviewGridMaterial::fragment_shader(), ShaderRef::Path(path)
        if path.path() == std::path::Path::new(PREVIEW_GRID_SHADER_PATH))
    );
    assert_eq!(material.alpha_mode(), AlphaMode::Blend);
    assert_eq!(material.depth_bias(), 100.0);
    assert!(!PreviewGridMaterial::enable_prepass());
    assert!(!PreviewGridMaterial::enable_shadows());
}

#[test]
fn display_modes_reach_real_presentations_without_changing_playback_or_material_identity() {
    let mut f = Fixture::new();
    let player = f.player(true);
    let compiled = f
        .app
        .world()
        .get::<PresentedEffect>(player)
        .unwrap()
        .effect()
        .clone();
    for (mode, expected) in [
        (PreviewDisplayMode::Wireframe, EffectRenderMode::Wireframe),
        (PreviewDisplayMode::Rendered, EffectRenderMode::Rendered),
    ] {
        f.display(mode);
        f.tick();
        let player = f.app.world().get::<PresentedEffect>(player).unwrap();
        assert_eq!(player.render_mode(), expected);
        assert_eq!(player.simulation_time(), 0.75);
        assert_eq!(player.instance.seed(), 42);
        assert_eq!(
            player.history_policy(),
            aestra_runtime::PlaybackHistoryPolicy::PlaybackOnly
        );
        assert_eq!(player.seek_quality(), aestra_runtime::SeekQuality::Preview);
        assert!(Arc::ptr_eq(player.effect(), &compiled));
    }
}

#[test]
fn display_sync_restores_owned_visibility_and_leaves_unowned_presentations_untouched() {
    let mut f = Fixture::new();
    let owned = f.player(true);
    let other = f.player(false);
    f.display(PreviewDisplayMode::Wireframe);
    f.tick();
    assert_eq!(
        *f.app.world().get::<Visibility>(owned).unwrap(),
        Visibility::Inherited
    );
    assert_eq!(
        *f.app.world().get::<Visibility>(other).unwrap(),
        Visibility::Hidden
    );
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(other)
            .unwrap()
            .render_mode(),
        EffectRenderMode::Rendered
    );
}

#[test]
fn new_presentations_receive_the_current_display_mode_on_the_next_update() {
    let mut f = Fixture::new();
    f.display(PreviewDisplayMode::Wireframe);
    f.tick();
    let player = f.player(true);
    f.tick();
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .render_mode(),
        EffectRenderMode::Wireframe
    );
}

#[test]
fn display_icons_use_the_selected_accent_and_unselected_text_colors() {
    let mut f = Fixture::new();
    let (wireframe, rendered) = f.icons();
    for (mode, active, inactive) in [
        (PreviewDisplayMode::Wireframe, wireframe, rendered),
        (PreviewDisplayMode::Rendered, rendered, wireframe),
    ] {
        f.display(mode);
        f.tick();
        assert_eq!(
            f.app.world().get::<SvgColor>(active).unwrap().0,
            theme::ACCENT
        );
        assert_eq!(
            f.app.world().get::<SvgColor>(inactive).unwrap().0,
            theme::TEXT
        );
    }
}

#[test]
fn gizmo_modes_and_spaces_drive_existing_label_and_icon_feedback() {
    let mut f = Fixture::new();
    let icons = f.gizmo_icons();
    let label = f
        .app
        .world_mut()
        .spawn((GizmoModeLabel, Text::new("old")))
        .id();
    for (index, mode, name) in [
        (0, TransformGizmoMode::Translate, "MOVE"),
        (1, TransformGizmoMode::Rotate, "ROTATE"),
        (2, TransformGizmoMode::Scale, "SCALE"),
    ] {
        for (space, suffix) in [
            (TransformGizmoSpace::World, "WORLD"),
            (TransformGizmoSpace::Local, "LOCAL"),
        ] {
            {
                let mut settings = f.app.world_mut().resource_mut::<TransformGizmoSettings>();
                settings.mode = mode;
                settings.space = space;
            }
            f.tick();
            assert_eq!(
                f.app.world().get::<Text>(label).unwrap().0,
                format!("{name} · {suffix}")
            );
            for (i, entity) in icons.iter().enumerate() {
                assert_eq!(
                    f.app.world().get::<SvgColor>(*entity).unwrap().0,
                    if i == index {
                        theme::ACCENT
                    } else {
                        theme::TEXT
                    }
                );
            }
        }
    }
}

#[test]
fn x_space_toggle_uses_the_original_hover_and_just_pressed_contract() {
    let mut f = Fixture::new();
    f.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyX);
    f.tick();
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().space,
        TransformGizmoSpace::Local
    );
    f.tick();
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().space,
        TransformGizmoSpace::Local
    );
    f.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(f.canvas)
        .unwrap()
        .cursor_over = false;
    {
        let mut keys = f.app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release(KeyCode::KeyX);
        keys.press(KeyCode::KeyX);
    }
    f.tick();
    assert_eq!(
        f.app.world().resource::<TransformGizmoSettings>().space,
        TransformGizmoSpace::Local
    );
}

#[test]
fn missing_canvas_defers_gizmo_feedback_without_destroying_control_entities() {
    let mut f = Fixture::new();
    let icons = f.gizmo_icons();
    let label = f
        .app
        .world_mut()
        .spawn((GizmoModeLabel, Text::new("retained")))
        .id();
    f.app.world_mut().despawn(f.canvas);
    f.tick();
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "retained");
    assert!(
        icons
            .into_iter()
            .all(|entity| f.app.world().get::<SvgColor>(entity).unwrap().0 == Color::NONE)
    );
}

fn install_actions(f: &mut Fixture, root: &tempfile::TempDir) {
    use crate::{
        docking::MaximizedPanel,
        localization::Localizer,
        settings::{EditorSettings, SettingsPersistence},
    };
    f.app
        .insert_resource(crate::test_support::session_with_timing_slack())
        .init_resource::<EditorSettings>()
        .insert_resource(SettingsPersistence::for_test(
            root.path().join("settings.ron"),
        ))
        .insert_resource(Localizer::new("en-US").unwrap())
        .init_resource::<MaximizedPanel>()
        .add_observer(super::super::actions::execute_viewport_action);
}

#[test]
fn real_grid_action_updates_persistence_and_the_visible_grid_without_editing_the_effect() {
    let mut f = Fixture::new();
    let root = tempfile::tempdir().unwrap();
    install_actions(&mut f, &root);
    let before = f
        .app
        .world()
        .resource::<crate::EditorSession>()
        .effect
        .clone();
    f.app
        .world_mut()
        .trigger(super::super::ViewportAction::ToggleGrid);
    f.tick();
    assert_eq!(
        *f.app.world().get::<Visibility>(f.grid).unwrap(),
        Visibility::Hidden
    );
    let saved: crate::settings::EditorSettings =
        ron::from_str(&std::fs::read_to_string(root.path().join("settings.ron")).unwrap()).unwrap();
    assert!(!saved.preview.show_grid);
    let session = f.app.world().resource::<crate::EditorSession>();
    assert_eq!(session.effect, before);
    assert!(!session.dirty);
}

#[test]
fn real_display_and_gizmo_actions_reach_presentation_payloads_labels_and_icons() {
    let mut f = Fixture::new();
    let root = tempfile::tempdir().unwrap();
    install_actions(&mut f, &root);
    let player = f.player(true);
    let (wireframe, _) = f.icons();
    let icons = f.gizmo_icons();
    let label = f
        .app
        .world_mut()
        .spawn((GizmoModeLabel, Text::default()))
        .id();
    f.app
        .world_mut()
        .trigger(super::super::ViewportAction::SetPreviewDisplayMode(
            PreviewDisplayMode::Wireframe,
        ));
    f.app
        .world_mut()
        .trigger(super::super::ViewportAction::SetTransformGizmoMode(
            TransformGizmoMode::Rotate,
        ));
    f.tick();
    assert_eq!(
        f.app
            .world()
            .get::<PresentedEffect>(player)
            .unwrap()
            .render_mode(),
        EffectRenderMode::Wireframe
    );
    assert_eq!(
        f.app.world().get::<SvgColor>(wireframe).unwrap().0,
        theme::ACCENT
    );
    assert_eq!(
        f.app.world().get::<SvgColor>(icons[1]).unwrap().0,
        theme::ACCENT
    );
    assert_eq!(
        f.app.world().get::<Text>(label).unwrap().0,
        "ROTATE · WORLD"
    );
    assert!(!root.path().join("settings.ron").exists());
    let session = f.app.world().resource::<crate::EditorSession>();
    assert!(!session.dirty);
    assert_eq!(session.effect_undo_len(), 0);
}

#[test]
fn idle_updates_keep_the_same_grid_and_feedback_entities_and_material_handles() {
    let mut f = Fixture::new();
    let (wireframe, rendered) = f.icons();
    let icons = f.gizmo_icons();
    let label = f
        .app
        .world_mut()
        .spawn((GizmoModeLabel, Text::default()))
        .id();
    f.tick();
    for _ in 0..3 {
        f.tick();
    }
    assert!(f.app.world().get::<PreviewGridPlane>(f.grid).is_some());
    assert_eq!(
        f.app
            .world()
            .get::<MeshMaterial3d<PreviewGridMaterial>>(f.grid)
            .unwrap()
            .0,
        f.material
    );
    for entity in [wireframe, rendered].into_iter().chain(icons) {
        assert!(f.app.world().get::<SvgColor>(entity).is_some());
    }
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "MOVE · WORLD");
}
