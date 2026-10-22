//! Complete production registrar and composition, using real project snapshots and image assets.
use super::*;
use crate::EditorSession;
use aestra_core::material::MaterialPresetRecipe;
use bevy::ecs::system::RunSystemOnce;

struct Fixture {
    root: tempfile::TempDir,
    preset: MaterialPresetDescriptor,
    app: App,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut preset = MaterialCompiler
            .material_preset_catalog()
            .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
            .unwrap()
            .clone();
        preset.id = MaterialPresetId::new();
        preset
            .save_ron(root.path().join("preview.aestra.material-preset.ron"))
            .unwrap();
        let mut app = App::new();
        app.insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(crate::test_support::session_with_timing_slack())
            .init_resource::<Assets<Image>>();
        register(&mut app);
        Self { root, preset, app }
    }
    fn spawn(&mut self, preset: MaterialPresetId) -> (Entity, Entity) {
        let entity = self
            .app
            .world_mut()
            .run_system_once(move |mut commands: Commands| {
                let root = commands.spawn(Node::default()).id();
                let mut preview = None;
                commands.entity(root).with_children(|parent| {
                    preview = Some(spawn_material_preset_preview(parent, preset, 48.0));
                });
                preview.unwrap()
            })
            .unwrap();
        let label = self
            .app
            .world()
            .get::<MaterialPresetPreviewRaster>(entity)
            .unwrap()
            .status_label;
        (entity, label)
    }
    fn own_preview(&mut self) -> (Entity, Entity) {
        self.spawn(self.preset.id)
    }
    fn path(&self) -> std::path::PathBuf {
        self.root.path().join("preview.aestra.material-preset.ron")
    }
    fn publish(&mut self) {
        self.preset.save_ron(self.path()).unwrap();
        self.refresh();
    }
    fn refresh(&mut self) {
        self.app
            .world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .bypass_change_detection()
            .refresh();
    }
    fn image(&self, entity: Entity) -> Handle<Image> {
        self.app
            .world()
            .get::<ImageNode>(entity)
            .unwrap()
            .image
            .clone()
    }
}

#[test]
fn real_composition_keeps_clipped_dimensions_border_and_noninteractive_label() {
    let mut f = Fixture::new();
    let (entity, label) = f.own_preview();
    let world = f.app.world();
    let node = world.get::<Node>(entity).unwrap();
    assert_eq!(node.width, Val::Px(48.0));
    assert_eq!(node.height, Val::Px(48.0));
    assert_eq!(node.min_width, node.width);
    assert_eq!(node.min_height, node.height);
    assert_eq!(node.flex_shrink, 0.0);
    assert_eq!(node.border, UiRect::all(Val::Px(1.0)));
    assert_eq!(node.border_radius, BorderRadius::all(Val::Px(3.0)));
    assert_eq!(node.overflow, Overflow::clip());
    assert_eq!(world.get::<BorderColor>(entity).unwrap().top, theme::BORDER);
    assert_eq!(
        world.get::<BackgroundColor>(entity).unwrap().0,
        theme::PANEL_DARK
    );
    for target in [entity, label] {
        let pickable = world.get::<Pickable>(target).unwrap();
        assert!(!pickable.is_hoverable && !pickable.should_block_lower);
    }
    assert_eq!(world.get::<ChildOf>(label).unwrap().parent(), entity);
    assert!(world.get::<Text>(label).unwrap().0.is_empty());
    assert_eq!(
        world.get::<TextFont>(label).unwrap().font_size,
        FontSize::Px(9.0)
    );
}

#[test]
fn registrar_deduplicates_actual_images_without_changing_effect_or_history() {
    let mut f = Fixture::new();
    let before = f.app.world().resource::<EditorSession>().effect.clone();
    let (first, label) = f.own_preview();
    let (second, _) = f.own_preview();
    f.app.update();
    assert_eq!(f.image(first), f.image(second));
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 1);
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialPresetPreviewState>()
            .cache
            .len(),
        1
    );
    assert!(f.app.world().get::<Text>(label).unwrap().0.is_empty());
    assert_eq!(
        f.app.world().get::<BackgroundColor>(first).unwrap().0,
        Color::NONE
    );
    assert!(f.app.world().get::<EditorTooltip>(first).is_none());
    assert_eq!(f.app.world().resource::<EditorSession>().effect, before);
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[test]
fn idle_catalog_does_not_dirty_cache_label_or_image_components() {
    let mut f = Fixture::new();
    let (entity, label) = f.own_preview();
    f.app.update();
    let cache_tick = f
        .app
        .world()
        .get_resource_ref::<MaterialPresetPreviewState>()
        .unwrap()
        .last_changed();
    let label_tick = f
        .app
        .world()
        .entity(label)
        .get_ref::<Text>()
        .unwrap()
        .last_changed();
    let image_tick = f
        .app
        .world()
        .entity(entity)
        .get_ref::<ImageNode>()
        .unwrap()
        .last_changed();
    for _ in 0..3 {
        f.app.update();
    }
    assert_eq!(
        f.app
            .world()
            .get_resource_ref::<MaterialPresetPreviewState>()
            .unwrap()
            .last_changed(),
        cache_tick
    );
    assert_eq!(
        f.app
            .world()
            .entity(label)
            .get_ref::<Text>()
            .unwrap()
            .last_changed(),
        label_tick
    );
    assert_eq!(
        f.app
            .world()
            .entity(entity)
            .get_ref::<ImageNode>()
            .unwrap()
            .last_changed(),
        image_tick
    );
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 1);
}

#[test]
fn a_late_added_request_mounts_cached_pixels_without_rewriting_existing_views() {
    let mut f = Fixture::new();
    let (first, label) = f.own_preview();
    f.app.update();
    f.app.update();
    let label_tick = f
        .app
        .world()
        .entity(label)
        .get_ref::<Text>()
        .unwrap()
        .last_changed();
    let (second, _) = f.own_preview();
    f.app.update();
    assert_eq!(f.image(first), f.image(second));
    assert_eq!(
        f.app
            .world()
            .entity(label)
            .get_ref::<Text>()
            .unwrap()
            .last_changed(),
        label_tick
    );
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 1);
}

#[test]
fn metadata_publication_reuses_the_image_while_recipe_publication_replaces_it() {
    let mut f = Fixture::new();
    let (entity, _) = f.own_preview();
    f.app.update();
    let first = f.image(entity);
    f.preset.description.push_str(" metadata only");
    f.publish();
    f.app.update();
    assert_eq!(f.image(entity), first);
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 1);
    let MaterialPresetRecipe::Stack { defaults, .. } = &mut f.preset.recipe else {
        panic!("stack")
    };
    defaults[0].value = MaterialValue::Float(0.123);
    f.publish();
    f.app.update();
    assert_ne!(f.image(entity), first);
    assert_eq!(f.app.world().resource::<Assets<Image>>().len(), 2);
}

#[test]
fn removal_clears_old_pixels_reports_failure_prunes_cache_and_can_recover() {
    let mut f = Fixture::new();
    let (entity, label) = f.own_preview();
    f.app.update();
    let first = f.image(entity);
    std::fs::remove_file(f.path()).unwrap();
    f.refresh();
    f.app.update();
    assert!(f.app.world().get::<ImageNode>(entity).is_none());
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "!");
    assert!(
        f.app
            .world()
            .get::<EditorTooltip>(entity)
            .unwrap()
            .accessible_label()
            .contains("Preset is unavailable")
    );
    assert_eq!(
        f.app.world().get::<BackgroundColor>(entity).unwrap().0,
        theme::PANEL_DARK
    );
    assert!(
        f.app
            .world()
            .resource::<MaterialPresetPreviewState>()
            .cache
            .is_empty()
    );
    f.publish();
    f.app.update();
    assert_ne!(f.image(entity), first);
    assert!(f.app.world().get::<Text>(label).unwrap().0.is_empty());
    assert!(f.app.world().get::<EditorTooltip>(entity).is_none());
}

#[test]
fn same_id_in_another_project_revalidates_the_recipe_without_semantic_edits() {
    let mut f = Fixture::new();
    let (entity, _) = f.own_preview();
    f.app.update();
    let first = f.image(entity);
    let other = tempfile::tempdir().unwrap();
    let mut preset = f.preset.clone();
    let MaterialPresetRecipe::Stack { defaults, .. } = &mut preset.recipe else {
        panic!("stack")
    };
    defaults[0].value = MaterialValue::Float(0.123);
    preset
        .save_ron(other.path().join("same.aestra.material-preset.ron"))
        .unwrap();
    f.app
        .insert_resource(ProjectEffectCatalog::scan(other.path()));
    f.app.update();
    assert_ne!(f.image(entity), first);
    assert_eq!(
        f.app
            .world()
            .resource::<MaterialPresetPreviewState>()
            .cache
            .len(),
        1
    );
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .operation_order
            .edit_serial(),
        0
    );
}

#[test]
fn malformed_project_presets_are_unavailable_without_poisoning_builtin_requests() {
    let mut f = Fixture::new();
    std::fs::write(f.path(), "malformed preset").unwrap();
    f.refresh();
    assert!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_preset_catalog()
            .unwrap()
            .get(f.preset.id)
            .is_none()
    );
    let (builtin, _) = f.spawn(aestra_compiler::MATERIAL_PRESET_DISSOLVE);
    let (unavailable, label) = f.own_preview();
    f.app.update();
    assert!(f.app.world().get::<ImageNode>(builtin).is_some());
    assert!(f.app.world().get::<ImageNode>(unavailable).is_none());
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "!");
}

#[test]
fn conflicting_project_catalog_falls_back_to_the_real_builtin_catalog() {
    let mut f = Fixture::new();
    let custom = f.preset.id;
    f.preset.id = aestra_compiler::MATERIAL_PRESET_DISSOLVE;
    f.publish();
    assert!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_preset_catalog()
            .is_err()
    );
    let (builtin, _) = f.spawn(aestra_compiler::MATERIAL_PRESET_DISSOLVE);
    let (unavailable, label) = f.spawn(custom);
    f.app.update();
    assert!(f.app.world().get::<ImageNode>(builtin).is_some());
    assert!(f.app.world().get::<ImageNode>(unavailable).is_none());
    assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "!");
}

#[test]
fn status_transitions_clear_stale_images_tooltips_and_labels_and_allow_missing_labels() {
    let mut f = Fixture::new();
    let (entity, label) = f.own_preview();
    let image = f
        .app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::default());
    for status in [
        MaterialPresetPreviewStatus::Ready(image.clone()),
        MaterialPresetPreviewStatus::Failed("raster detail".into()),
        MaterialPresetPreviewStatus::Incompatible,
        MaterialPresetPreviewStatus::Ready(image.clone()),
    ] {
        let expected = status.clone();
        f.app
            .world_mut()
            .run_system_once(
                move |mut commands: Commands, mut labels: Query<&mut Text>| {
                    apply_material_preset_preview_status(
                        &mut commands,
                        &mut labels,
                        entity,
                        label,
                        status.clone(),
                    );
                },
            )
            .unwrap();
        match expected {
            MaterialPresetPreviewStatus::Ready(_) => {
                assert_eq!(f.image(entity), image);
                assert!(f.app.world().get::<Text>(label).unwrap().0.is_empty());
                assert!(f.app.world().get::<EditorTooltip>(entity).is_none());
            }
            MaterialPresetPreviewStatus::Failed(_) => {
                assert!(f.app.world().get::<ImageNode>(entity).is_none());
                assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "!");
                assert!(
                    f.app
                        .world()
                        .get::<EditorTooltip>(entity)
                        .unwrap()
                        .accessible_label()
                        .contains("raster detail")
                );
            }
            MaterialPresetPreviewStatus::Incompatible => {
                assert!(f.app.world().get::<ImageNode>(entity).is_none());
                assert_eq!(f.app.world().get::<Text>(label).unwrap().0, "N/A");
                assert!(f.app.world().get::<EditorTooltip>(entity).is_none());
            }
        }
    }
    f.app.world_mut().despawn(label);
    f.app
        .world_mut()
        .run_system_once(
            move |mut commands: Commands, mut labels: Query<&mut Text>| {
                apply_material_preset_preview_status(
                    &mut commands,
                    &mut labels,
                    entity,
                    label,
                    MaterialPresetPreviewStatus::Failed("missing label".into()),
                );
            },
        )
        .unwrap();
    assert_eq!(f.image(entity), image);
}

#[test]
fn resolved_program_reuses_actual_preview_pixels_and_unknown_presets_are_incompatible() {
    let catalog = MaterialCompiler.material_preset_catalog();
    let preset = catalog
        .get(aestra_compiler::MATERIAL_PRESET_DISSOLVE)
        .unwrap();
    let program = resolve_preset_program(&catalog, preset.id).unwrap();
    let image = render_material_graph_preview(
        &program,
        None,
        &aestra_compiler::MaterialFunctionLibrary::default(),
        MaterialGraphPreviewTarget::Output,
        None,
    );
    let mut images = Assets::<Image>::default();
    let MaterialPresetPreviewStatus::Ready(handle) =
        build_material_preset_preview(preset, &catalog, &mut images)
    else {
        panic!("ready preset")
    };
    assert_eq!(images.get(&handle).unwrap().data, image.data);
    let mut unknown = preset.clone();
    unknown.id = MaterialPresetId::new();
    assert!(resolve_preset_program(&catalog, unknown.id).is_err());
    assert!(matches!(
        build_material_preset_preview(&unknown, &catalog, &mut images),
        MaterialPresetPreviewStatus::Incompatible
    ));
}
