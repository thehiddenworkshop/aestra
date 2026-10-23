use super::*;
use crate::history::{EditorHistoryLedger, HistoryAction, MaterialProgramEditHistory};
use std::fs;

struct Fixture {
    root: tempfile::TempDir,
    app: App,
    source: ProjectSourceId,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("folder/empty")).unwrap();
        fs::write(root.path().join("folder/image.png"), b"exact source bytes").unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let source = catalog
            .content()
            .source_tree()
            .at_relative_path("folder")
            .unwrap()
            .id;
        let mut browser = AssetBrowserState::default();
        browser.reconcile(catalog.content(), catalog.content_revision());
        browser.toggle_favorite(catalog.content(), source);
        browser.record_recent(catalog.content(), source);
        browser.selected = Some(source);
        browser.inspected = Some(source);
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .insert_resource(browser);
        register(&mut app);
        Self { root, app, source }
    }
    fn delete(&mut self) {
        self.app.world_mut().trigger(Open(Some(self.source)));
        io::drain(self.app.world_mut());
    }
    fn bin(&mut self) {
        self.app.world_mut().trigger(Open(None));
        io::drain(self.app.world_mut());
    }
    fn choose(&mut self, choice: Choice) {
        let entity = self.app.world_mut().spawn(choice).id();
        self.app.world_mut().trigger(Activate { entity });
        self.app.world_mut().flush();
    }
    fn complete(&mut self) {
        self.app.world_mut().flush();
        if !io::idle_world(self.app.world()) {
            io::drain(self.app.world_mut());
        }
    }
    fn widgets(&mut self, size: UVec2, scale: f32) {
        use bevy::{
            feathers::FeathersPlugins,
            input::InputPlugin,
            input_focus::{InputDispatchPlugin, InputFocusPlugin},
            render::{RenderPlugin, settings::WgpuSettings},
            ui_widgets::UiWidgetsPlugins,
            window::{ExitCondition, WindowPlugin, WindowResolution},
        };
        // Explicit headless dependencies avoid Winit and native GPU initialization on both engines.
        self.app
            .add_plugins((
                MinimalPlugins,
                bevy::asset::AssetPlugin::default(),
                bevy::scene::ScenePlugin,
                RenderPlugin {
                    render_creation: WgpuSettings {
                        backends: None,
                        ..default()
                    }
                    .into(),
                    ..default()
                },
                bevy::image::ImagePlugin::default(),
                bevy::mesh::MeshPlugin,
                bevy::camera::CameraPlugin,
                bevy::transform::TransformPlugin,
                bevy::text::TextPlugin,
                InputPlugin,
                InputFocusPlugin,
                InputDispatchPlugin,
                bevy::picking::DefaultPickingPlugins,
                UiWidgetsPlugins,
            ))
            .add_plugins((
                bevy::sprite::SpritePlugin,
                bevy::ui::UiPlugin,
                WindowPlugin {
                    primary_window: None,
                    exit_condition: ExitCondition::DontExit,
                    ..default()
                },
                FeathersPlugins,
            ))
            .insert_resource(theme::feathers_theme())
            .add_systems(
                PostUpdate,
                crate::feathers::scroll::update_scrollbar_visibility
                    .after(bevy::ui::UiSystems::Layout),
            );
        self.app.finish();
        self.app.cleanup();
        let window = self
            .app
            .world_mut()
            .spawn((
                Window {
                    resolution: WindowResolution::new(size.x, size.y)
                        .with_scale_factor_override(scale),
                    ..default()
                },
                bevy::window::PrimaryWindow,
            ))
            .id();
        let camera = self
            .app
            .world_mut()
            .spawn((
                Camera2d,
                bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)),
            ))
            .id();
        self.app
            .world_mut()
            .commands()
            .spawn((
                Node {
                    width: Val::Px(size.x as f32 / scale),
                    height: Val::Px(size.y as f32 / scale),
                    ..default()
                },
                UiTargetCamera(camera),
            ))
            .with_children(spawn);
        self.settle();
    }
    fn settle(&mut self) {
        for _ in 0..4 {
            self.app.update();
        }
    }
    fn control(&mut self, predicate: impl Fn(&Choice) -> bool) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &Choice)>()
            .iter(world)
            .find_map(|(entity, choice)| predicate(choice).then_some(entity))
            .unwrap()
    }
}

#[test]
fn empty_recovery_dialog_loads_from_real_serialized_io() {
    let root = tempfile::tempdir().unwrap();
    let mut app = App::new();
    app.insert_resource(ProjectEffectCatalog::scan(root.path()))
        .insert_resource(crate::test_support::session_with_timing_slack())
        .insert_resource(Localizer::new("en-US").unwrap());
    register(&mut app);
    app.world_mut().trigger(Open(None));
    io::drain(app.world_mut());
    assert!(app.world().resource::<State>().open);
    assert!(!app.world().resource::<State>().busy);
    assert!(app.world().resource::<State>().entries.is_empty());
}

#[test]
fn initial_delete_and_manual_restore_publish_real_browser_bookmarks_selection_and_status() {
    let mut f = Fixture::new();
    f.app.world_mut().trigger(Open(Some(f.source)));
    assert!(f.root.path().join("folder/image.png").exists());
    assert!(!f.app.world().resource::<State>().open);
    assert!(f.app.world().resource::<State>().busy);
    io::drain(f.app.world_mut());
    assert!(!f.root.path().join("folder").exists());
    let browser = f.app.world().resource::<AssetBrowserState>();
    assert_eq!(browser.selected, None);
    assert_eq!(browser.inspected, None);
    assert_eq!(browser.favorites.len(), 1);
    assert_eq!(browser.recent.len(), 1);
    f.bin();
    assert_eq!(f.app.world().resource::<State>().entries.len(), 1);
    {
        let mut browser = f.app.world_mut().resource_mut::<AssetBrowserState>();
        browser.query = "filtered out".into();
        browser.recursive = true;
        browser.sources_visible = false;
    }
    f.choose(Choice::Restore(0));
    f.complete();
    assert_eq!(
        fs::read(f.root.path().join("folder/image.png")).unwrap(),
        b"exact source bytes"
    );
    let catalog = f.app.world().resource::<ProjectEffectCatalog>();
    let restored = catalog
        .content()
        .source_tree()
        .at_relative_path("folder")
        .unwrap()
        .id;
    let browser = f.app.world().resource::<AssetBrowserState>();
    assert_eq!(browser.selected, Some(restored));
    assert!(browser.query.is_empty() && browser.sources_visible && !browser.recursive);
    assert!(browser.is_favorite(catalog.content(), restored));
    assert!(!f.app.world().resource::<State>().open);
    assert!(
        !f.app
            .world()
            .resource::<DocumentProtectionState>()
            .asset_delete_open
    );
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .status
            .contains("folder")
    );
}

#[test]
fn initial_delete_rechecks_document_guard_then_allows_explicit_retry() {
    let mut f = Fixture::new();
    f.app.world_mut().trigger(Open(Some(f.source)));
    let mut completion = io::prepared_completion(f.app.world_mut());
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Newer edit");
    completion.apply(f.app.world_mut());
    assert!(f.root.path().join("folder/image.png").exists());
    assert!(f.app.world().resource::<State>().open);
    assert!(!f.app.world().resource::<State>().busy);
    f.choose(Choice::Refresh);
    f.complete();
    assert!(!f.root.path().join("folder").exists());
    assert_eq!(
        f.app.world().resource::<EditorSession>().effect.name,
        "Newer edit"
    );
}

#[test]
fn other_modals_and_busy_io_reject_actions_without_replacing_the_dialog_generation() {
    let mut f = Fixture::new();
    for protection in DocumentProtectionState::blocking_states_for_test() {
        f.app.insert_resource(protection);
        f.app.world_mut().trigger(Open(Some(f.source)));
        assert!(io::idle_world(f.app.world()));
        assert_eq!(f.app.world().resource::<State>().generation, 0);
    }
    f.app.insert_resource(DocumentProtectionState::default());
    f.app.world_mut().trigger(Open(None));
    let generation = f.app.world().resource::<State>().generation;
    let mut completion = io::prepared_completion(f.app.world_mut());
    for choice in [
        Choice::Cancel,
        Choice::Refresh,
        Choice::Page(true),
        Choice::Restore(0),
    ] {
        f.choose(choice);
        assert!(f.app.world().resource::<State>().busy);
        assert!(f.app.world().resource::<State>().open);
    }
    f.app.world_mut().trigger(Open(Some(f.source)));
    assert_eq!(f.app.world().resource::<State>().generation, generation);
    completion.apply(f.app.world_mut());
    assert!(f.root.path().join("folder/image.png").exists());
}

#[test]
fn manual_restore_rejects_stale_preflight_and_reloads_for_safe_retry() {
    let mut f = Fixture::new();
    f.delete();
    f.bin();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .set_effect_name("Changed after listing");
    f.choose(Choice::Restore(0));
    assert!(io::idle_world(f.app.world()));
    assert!(!f.root.path().join("folder").exists());
    assert!(
        f.app
            .world()
            .resource::<State>()
            .message
            .contains("Project or drafts changed")
    );
    f.choose(Choice::Refresh);
    f.complete();
    f.choose(Choice::Restore(0));
    f.complete();
    assert!(f.root.path().join("folder").exists());
}

#[test]
fn stale_dialog_generation_cannot_apply_a_prepared_delete() {
    let mut f = Fixture::new();
    f.app.world_mut().trigger(Open(Some(f.source)));
    let mut completion = io::prepared_completion(f.app.world_mut());
    f.app.world_mut().resource_mut::<State>().generation += 1;
    completion.apply(f.app.world_mut());
    assert!(f.root.path().join("folder").exists());
    assert!(!f.root.path().join(".aestra/deleted").exists());
    f.app.update();
    assert!(!f.app.world().resource::<State>().busy);
    assert!(
        f.app
            .world()
            .resource::<State>()
            .message
            .contains("cancelled")
    );
}

#[test]
fn malformed_activation_and_missing_restore_index_leave_recovery_intact() {
    let mut f = Fixture::new();
    f.delete();
    f.bin();
    let unrelated = f.app.world_mut().spawn_empty().id();
    f.app.world_mut().trigger(Activate { entity: unrelated });
    f.choose(Choice::Restore(usize::MAX));
    assert!(io::idle_world(f.app.world()));
    assert_eq!(f.app.world().resource::<State>().entries.len(), 1);
    assert!(!f.root.path().join("folder").exists());
    f.choose(Choice::Restore(0));
    f.complete();
    assert!(f.root.path().join("folder").exists());
}

#[test]
fn paginated_real_recovery_entries_clamp_to_the_first_and_last_page() {
    let mut f = Fixture::new();
    for index in 0..9 {
        let path = format!("pack-{index}");
        fs::create_dir(f.root.path().join(&path)).unwrap();
        f.app
            .world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .refresh();
        let source = f
            .app
            .world()
            .resource::<ProjectEffectCatalog>()
            .content()
            .source_tree()
            .at_relative_path(&path)
            .unwrap()
            .id;
        f.app.world_mut().trigger(Open(Some(source)));
        io::drain(f.app.world_mut());
    }
    f.bin();
    assert_eq!(f.app.world().resource::<State>().entries.len(), 9);
    for _ in 0..4 {
        f.choose(Choice::Page(true));
    }
    assert_eq!(f.app.world().resource::<State>().page, 1);
    for _ in 0..4 {
        f.choose(Choice::Page(false));
    }
    assert_eq!(f.app.world().resource::<State>().page, 0);
}

#[test]
fn real_dialog_keeps_long_errors_scrolled_and_close_accessible_at_multiple_dpi_and_locales() {
    use bevy::{feathers::controls::FeathersButton, ui_widgets::Scrollbar};
    for (size, scale, locale) in [
        (UVec2::new(800, 500), 1.0, "en-US"),
        (UVec2::new(900, 600), 1.5, "fr-FR"),
    ] {
        let mut f = Fixture::new();
        f.app.insert_resource(Localizer::new(locale).unwrap());
        f.widgets(size, scale);
        show_error(f.app.world_mut(), "Long dependency blocker\n".repeat(90));
        f.settle();
        let close = f.control(|c| matches!(c, Choice::Cancel));
        assert!(f.app.world().get::<FeathersButton>(close).is_some());
        assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(close));
        assert_eq!(
            f.app.world().get::<AccessibleLabel>(close).unwrap().0,
            f.app.world().resource::<Localizer>().text("common-close")
        );
        let world = f.app.world_mut();
        let (computed, body) = world
            .query_filtered::<(&ComputedNode, &Node), With<Body>>()
            .single(world)
            .unwrap();
        assert!(
            computed.size().y > 40.0 && computed.size().y <= size.y as f32 * 0.8 + 1.0,
            "{:?}",
            computed.size()
        );
        assert_eq!(body.overflow, Overflow::scroll_y());
        assert!(
            world
                .query::<(&Scrollbar, &Node)>()
                .iter(world)
                .any(|(_, node)| node.display == Display::Flex)
        );
        assert_eq!(world.query::<&Choice>().iter(world).count(), 2); // Retry + Close, never force-delete.
    }
}

#[test]
fn real_close_and_escape_restore_previous_focus_or_clear_removed_targets() {
    for (escape, removed) in [(false, false), (true, false), (false, true)] {
        let mut f = Fixture::new();
        f.widgets(UVec2::new(800, 500), 1.0);
        let previous = f.app.world_mut().spawn(TabIndex(0)).id();
        f.app
            .world_mut()
            .resource_mut::<InputFocus>()
            .set(previous, FocusCause::Navigated);
        show_error(f.app.world_mut(), "Error".into());
        f.settle();
        if removed {
            f.app.world_mut().entity_mut(previous).despawn();
        }
        if escape {
            use bevy::input::{
                ButtonState,
                keyboard::{Key, KeyboardInput},
            };
            let window = {
                let world = f.app.world_mut();
                world
                    .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
                    .single(world)
                    .unwrap()
            };
            f.app.world_mut().write_message(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        } else {
            let entity = f.control(|c| matches!(c, Choice::Cancel));
            f.app.world_mut().trigger(Activate { entity });
        }
        f.app.update();
        assert!(!f.app.world().resource::<State>().open);
        assert!(
            !f.app
                .world()
                .resource::<DocumentProtectionState>()
                .asset_delete_open
        );
        assert_eq!(
            f.app.world().resource::<InputFocus>().get(),
            (!removed).then_some(previous)
        );
    }
}

#[test]
fn busy_dialog_disables_real_controls_and_releases_them_only_after_completion() {
    let mut f = Fixture::new();
    f.widgets(UVec2::new(800, 500), 1.0);
    f.app.world_mut().trigger(Open(None));
    let mut completion = io::prepared_completion(f.app.world_mut());
    f.settle();
    let world = f.app.world_mut();
    assert!(
        world
            .query_filtered::<Entity, With<Choice>>()
            .iter(world)
            .all(|e| world.get::<InteractionDisabled>(e).is_some())
    );
    let close = f.control(|c| matches!(c, Choice::Cancel));
    f.app.world_mut().trigger(Activate { entity: close });
    assert!(f.app.world().resource::<State>().open);
    completion.apply(f.app.world_mut());
    f.settle();
    let close = f.control(|c| matches!(c, Choice::Cancel));
    assert!(f.app.world().get::<InteractionDisabled>(close).is_none());
}

#[test]
fn idle_dialog_preserves_control_entities_and_focus_without_rebuilding_its_body() {
    let mut f = Fixture::new();
    f.widgets(UVec2::new(800, 500), 1.0);
    show_error(f.app.world_mut(), "Stable blocker".into());
    f.settle();
    let close = f.control(|c| matches!(c, Choice::Cancel));
    let before = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<Choice>>()
        .iter(f.app.world())
        .collect::<Vec<_>>();
    f.settle();
    let after = f
        .app
        .world_mut()
        .query_filtered::<Entity, With<Choice>>()
        .iter(f.app.world())
        .collect::<Vec<_>>();
    assert_eq!(before, after);
    assert_eq!(f.app.world().resource::<InputFocus>().get(), Some(close));
}

#[test]
fn switching_projects_closes_the_real_recovery_dialog_without_touching_either_root() {
    let mut f = Fixture::new();
    f.delete();
    f.bin();
    let other = tempfile::tempdir().unwrap();
    f.app
        .insert_resource(ProjectEffectCatalog::scan(other.path()));
    f.app.update();
    assert!(!f.app.world().resource::<State>().open);
    assert!(
        !f.app
            .world()
            .resource::<DocumentProtectionState>()
            .asset_delete_open
    );
    assert!(!f.root.path().join("folder").exists());
    assert!(!other.path().join("folder").exists());
}

#[test]
fn actual_history_router_uses_real_browser_publication_adapters_for_favorite_revival() {
    fn observe(
        action: On<HistoryAction>,
        routing: crate::history::routing::Routing,
        mut commands: Commands,
    ) {
        routing.execute(*action, &mut commands, history_step);
    }
    let mut f = Fixture::new();
    f.app
        .init_resource::<EditorHistoryLedger>()
        .init_resource::<MaterialProgramEditHistory>()
        .init_resource::<crate::material_function_editor::FunctionEditor>()
        .add_observer(observe);
    f.delete();
    f.app.world_mut().trigger(HistoryAction::Undo);
    f.complete();
    let catalog = f.app.world().resource::<ProjectEffectCatalog>();
    let restored = catalog
        .content()
        .source_tree()
        .at_relative_path("folder")
        .unwrap()
        .id;
    assert!(
        f.app
            .world()
            .resource::<AssetBrowserState>()
            .is_favorite(catalog.content(), restored)
    );
    assert_eq!(
        f.app.world().resource::<AssetBrowserState>().selected,
        Some(restored)
    );
    f.app.world_mut().trigger(HistoryAction::Redo);
    f.complete();
    assert_eq!(f.app.world().resource::<AssetBrowserState>().selected, None);
    assert!(!f.app.world().resource::<State>().open);
}

#[test]
fn initial_delete_and_bin_restore_recover_the_open_unsaved_material_without_saving_over_disk() {
    use aestra_core::material::MaterialProgram;
    let mut f = Fixture::new();
    let before = MaterialProgram::additive_sprite("Original").normalized();
    let path = f.root.path().join("folder/program.aestra.material.ron");
    before.save_ron(&path).unwrap();
    f.app
        .world_mut()
        .resource_mut::<ProjectEffectCatalog>()
        .refresh();
    let mut after = before.clone();
    after.name = "Unsaved".into();
    let mut catalog = f
        .app
        .world_mut()
        .remove_resource::<ProjectEffectCatalog>()
        .unwrap();
    catalog.replace_material_program(&before, &after).unwrap();
    f.app
        .world_mut()
        .resource_mut::<EditorSession>()
        .open_material_program(&catalog, before.id)
        .unwrap();
    f.app.insert_resource(catalog);
    f.delete();
    assert!(
        f.app
            .world()
            .resource::<EditorSession>()
            .standalone_material()
            .is_none()
    );
    f.bin();
    f.choose(Choice::Restore(0));
    f.complete();
    assert_eq!(
        f.app
            .world()
            .resource::<EditorSession>()
            .standalone_material(),
        Some(before.id)
    );
    assert_eq!(
        f.app
            .world()
            .resource::<ProjectEffectCatalog>()
            .material_program(before.id)
            .unwrap(),
        after
    );
    assert_eq!(MaterialProgram::load_ron(path).unwrap(), before);
}
