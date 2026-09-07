use super::state::*;
use super::{
    actions::BrowserAction,
    panel::{BrowserItems, BrowserRow, BrowserSearch},
};
use crate::*;
use aestra_project::{ProjectContent, ProjectContentVersion};
use bevy::{
    input_focus::{InputFocus, InputFocusPlugin, dispatch_focused_input},
    scene::ScenePlugin,
    text::TextPlugin,
    ui::Selected,
    ui_widgets::{ActiveDescendant, ListBoxPlugin},
};
use std::{collections::BTreeMap, path::Path};

const VERSION: ProjectContentVersion = ProjectContentVersion {
    generation: 1,
    revision: 1,
};

#[test]
fn browser_reads_published_snapshot_even_when_sources_are_gone() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("textures")).unwrap();
    std::fs::write(root.path().join("textures/smoke.png"), []).unwrap();
    std::fs::write(root.path().join("readme.txt"), []).unwrap();
    let content = ProjectContent::scan(root.path());
    root.close().unwrap();
    let mut state = AssetBrowserState::default();
    state.reconcile(&content, VERSION);
    assert_eq!(state.filtered(&content).len(), 2);
    state.navigate(
        &content,
        content
            .source_tree()
            .at_relative_path(std::path::Path::new("textures"))
            .unwrap()
            .id,
    );
    assert_eq!(state.filtered(&content)[0].name, "smoke.png");
    state.history(&content, false);
    assert!(state.folder.as_os_str().is_empty());
    state.history(&content, true);
    assert_eq!(state.folder, std::path::Path::new("textures"));
}

#[test]
fn filter_scope_types_and_layout_share_one_ordered_projection() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    for file in ["z.png", "A.wgsl", "nested/a.png", "note.txt"] {
        std::fs::write(root.path().join(file), []).unwrap();
    }
    let content = ProjectContent::scan(root.path());
    let mut state = AssetBrowserState::default();
    state.reconcile(&content, VERSION);
    let list = state
        .filtered(&content)
        .iter()
        .map(|e| e.id)
        .collect::<Vec<_>>();
    assert_eq!(state.filtered(&content)[0].name, "nested");
    state.view = ViewMode::Grid;
    assert_eq!(
        list,
        state
            .filtered(&content)
            .iter()
            .map(|e| e.id)
            .collect::<Vec<_>>()
    );
    state.kinds.insert(Kind::Texture);
    state.kinds.insert(Kind::Shader);
    assert_eq!(state.filtered(&content).len(), 3);
    state.recursive = true;
    state.query = "A.".into();
    assert_eq!(state.filtered(&content).len(), 2);
    state.kinds.remove(&Kind::Shader);
    assert_eq!(
        state.filtered(&content)[0].relative_path,
        std::path::Path::new("nested/a.png")
    );
}

#[test]
fn removed_folder_falls_back_and_project_generation_clears_navigation() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("a/b")).unwrap();
    let content = ProjectContent::scan(root.path());
    let mut state = AssetBrowserState::default();
    state.reconcile(&content, VERSION);
    let id = content
        .source_tree()
        .at_relative_path(std::path::Path::new("a/b"))
        .unwrap()
        .id;
    state.navigate(&content, id);
    state.selected = Some(id);
    std::fs::remove_dir(root.path().join("a/b")).unwrap();
    let content = ProjectContent::scan(root.path());
    state.reconcile(
        &content,
        ProjectContentVersion {
            revision: 2,
            ..VERSION
        },
    );
    assert_eq!(state.folder, std::path::Path::new("a"));
    assert!(state.selected.is_none());
    state.query = "smoke".into();
    state.reconcile(
        &content,
        ProjectContentVersion {
            generation: 2,
            revision: 0,
        },
    );
    assert!(state.folder.as_os_str().is_empty());
    assert!(state.back.is_empty());
    assert!(state.query.is_empty());
}

fn browser_app(root: &Path) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        ScenePlugin,
        TextPlugin,
        InputFocusPlugin,
        ListBoxPlugin,
    ))
    .init_asset::<Image>()
    .init_asset::<bevy_resvg::prelude::SvgFile>()
    .insert_resource(ProjectEffectCatalog::scan(root))
    .insert_resource(test_support::session_with_timing_slack())
    .init_resource::<LibraryState>()
    .insert_resource(Localizer::new("en-US").unwrap())
    .add_plugins(super::EditorAssetBrowserPlugin)
    .add_systems(Startup, spawn_browser_fixture)
    .add_message::<KeyboardInput>()
    .add_systems(PreUpdate, dispatch_focused_input::<KeyboardInput>);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app.update();
    app.update();
    app
}

fn spawn_browser_fixture(
    mut commands: Commands,
    state: Res<AssetBrowserState>,
    session: Res<EditorSession>,
    catalog: Res<ProjectEffectCatalog>,
    library: Res<LibraryState>,
    localizer: Res<Localizer>,
) {
    commands
        .spawn(Node {
            width: Val::Px(420.0),
            height: Val::Px(520.0),
            ..default()
        })
        .with_children(|parent| {
            super::spawn_assets_panel(parent, &session, &catalog, &library, &state, &localizer);
        });
}

fn rows(app: &mut App) -> BTreeMap<aestra_project::ProjectSourceId, Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &BrowserRow)>()
        .iter(world)
        .map(|(e, row)| (row.0, e))
        .collect()
}

#[test]
fn folder_labels_have_visible_layout_inside_their_buttons() {
    use bevy::{
        app::{HierarchyPropagatePlugin, PropagateSet},
        camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
        ui::{
            ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system,
            ui_surface::UiSurface,
            update::propagate_ui_target_cameras,
            widget::{measure_text_system, text_system},
        },
    };
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("effects")).unwrap();
    std::fs::write(root.path().join("effects/texture.png"), []).unwrap();
    let mut app = browser_app(root.path());
    app.add_plugins((
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ))
    .init_resource::<UiScale>()
    .init_resource::<UiSurface>()
    .add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            measure_text_system,
            ui_layout_system,
            text_system,
        )
            .chain()
            .after(bevy::text::load_font_assets_into_font_collection),
    )
    .configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    )
    .configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(measure_text_system),
    );
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(420, 520),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(420, 520),
                ..default()
            }),
            ..default()
        },
    ));
    for scale in [1.0, 1.5, 2.0] {
        for width in [90.0, 140.0, 240.0] {
            app.world_mut().resource_mut::<UiScale>().0 = scale;
            app.world_mut()
                .resource_mut::<AssetBrowserState>()
                .sources_width = width;
            for _ in 0..4 {
                app.update();
            }
            let world = app.world_mut();
            let buttons = world
                .query_filtered::<(Entity, &ComputedNode, &Children), With<super::panel::BrowserFolderButton>>()
                .iter(world)
                .map(|(entity, node, children)| (entity, node.size(), children.to_vec()))
                .collect::<Vec<_>>();
            assert_eq!(buttons.len(), 2);
            for (button, button_size, children) in buttons {
                assert!(button_size.x > 20.0, "folder button has no visible width");
                assert_eq!(
                    world.get::<Node>(button).unwrap().justify_content,
                    JustifyContent::Start
                );
                for child in children {
                    if let Some(text) = world.get::<Text>(child) {
                        let size = world.get::<ComputedNode>(child).unwrap().size();
                        assert!(
                            size.x > 20.0 && size.y > 8.0,
                            "{}: text {size:?}, button {button_size:?} ({button:?}), pane {width}, scale {scale}",
                            text.0
                        );
                        assert!(size.y <= button_size.y, "text height exceeds folder row");
                    }
                }
            }
        }
    }
    let folder = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .source_tree()
        .at_relative_path(Path::new("effects"))
        .unwrap()
        .id;
    app.world_mut().trigger(BrowserAction::Navigate(folder));
    for view in [ViewMode::List, ViewMode::Grid] {
        app.world_mut().trigger(BrowserAction::View(view));
        for _ in 0..4 {
            app.update();
        }
        let (_, row) = rows(&mut app)
            .into_iter()
            .find(|(_, entity)| app.world().get::<ListItem>(*entity).is_some())
            .unwrap();
        let world = app.world_mut();
        let size = world.get::<ComputedNode>(row).unwrap().size();
        assert!(size.x > 50.0 && size.y > 20.0, "empty asset row: {size:?}");
        let mut descendants = world.get::<Children>(row).unwrap().to_vec();
        while let Some(entity) = descendants.pop() {
            if let Some(children) = world.get::<Children>(entity) {
                descendants.extend(children.iter());
            }
            if world.get::<Text>(entity).is_some() {
                let label = world.get::<ComputedNode>(entity).unwrap().size();
                assert!(
                    label.x > 20.0 && label.y > 8.0,
                    "empty asset caption in {view:?}: {label:?}"
                );
                assert!(
                    label.x <= size.x && label.y <= size.y,
                    "caption exceeds row"
                );
            }
        }
    }
}

fn list(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<BrowserItems>>()
        .single(world)
        .unwrap()
}

#[test]
fn retained_rows_survive_selection_filtering_and_grid_list_changes() {
    let root = tempfile::tempdir().unwrap();
    for name in ["a.png", "b.png", "c.wgsl"] {
        std::fs::write(root.path().join(name), []).unwrap();
    }
    let mut app = browser_app(root.path());
    let original = rows(&mut app);
    let grid_button = {
        let world = app.world_mut();
        world
            .query::<(Entity, &BrowserAction)>()
            .iter(world)
            .find(|(_, action)| **action == BrowserAction::View(ViewMode::Grid))
            .unwrap()
            .0
    };
    assert_eq!(original.len(), 3);
    let (id, entity) = (
        *original.first_key_value().unwrap().0,
        *original.first_key_value().unwrap().1,
    );
    let ui_revision = app.world().resource::<EditorSession>().ui_revision;
    let list = list(&mut app);
    app.world_mut().trigger(ValueChange {
        source: list,
        value: entity,
        is_final: true,
    });
    app.update();
    assert_eq!(
        app.world().resource::<AssetBrowserState>().selected,
        Some(id)
    );
    assert!(app.world().get::<Selected>(entity).is_some());
    app.world_mut().trigger(BrowserAction::View(ViewMode::Grid));
    app.update();
    assert_eq!(
        app.world()
            .get::<bevy::feathers::controls::ButtonVariant>(grid_button),
        Some(&bevy::feathers::controls::ButtonVariant::Primary)
    );
    assert_eq!(rows(&mut app), original);
    assert_eq!(
        app.world().get::<Node>(entity).unwrap().height,
        Val::Px(132.0)
    );
    app.world_mut()
        .trigger(BrowserAction::Kind(Some(Kind::Shader)));
    app.update();
    app.world_mut().trigger(BrowserAction::Kind(None));
    app.update();
    app.world_mut().trigger(BrowserAction::View(ViewMode::List));
    app.update();
    assert_eq!(rows(&mut app), original);
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        ui_revision
    );
    let world = app.world_mut();
    let search = world
        .query_filtered::<Entity, With<BrowserSearch>>()
        .single(world)
        .unwrap();
    app.world_mut().trigger(ValueChange {
        source: search,
        value: "b.png".to_owned(),
        is_final: false,
    });
    app.update();
    assert_eq!(rows(&mut app), original);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, (With<BrowserRow>, With<ListItem>)>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn all_browser_messages_exist_in_both_locales() {
    for locale in crate::localization::SUPPORTED_LOCALES {
        let localizer = Localizer::new(locale).unwrap();
        for kind in Kind::FILTERS.into_iter().chain([Kind::Folder]) {
            assert_ne!(localizer.text(kind.label()), kind.label());
        }
        for suffix in [
            "mode",
            "legacy",
            "search",
            "items",
            "resize-sources",
            "sources",
            "list",
            "grid",
            "back",
            "forward",
            "up",
            "refresh",
            "open-project",
            "ancestors",
            "types",
            "all-types",
            "filtered-types",
            "sort",
            "sort-name",
            "sort-type",
            "recursive",
            "expand-folder",
            "next-page",
            "previous-page",
            "empty",
            "read-only",
            "effect-unavailable",
        ] {
            let key = format!("browser-{suffix}");
            assert_ne!(localizer.text(&key), key);
        }
    }
}

#[test]
fn browser_popovers_block_navigation_only_while_open() {
    let root = tempfile::tempdir().unwrap();
    let mut app = browser_app(root.path());
    let popup = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<bevy::ui_widgets::popover::Popover>>()
            .iter(world)
            .next()
            .unwrap()
    };
    assert!(app.world().get::<super::BrowserSurface>(popup).is_none());
    app.world_mut()
        .entity_mut(popup)
        .insert(Visibility::Visible);
    app.update();
    assert!(app.world().get::<super::BrowserSurface>(popup).is_some());
    assert!(
        app.world()
            .get::<crate::feathers::node_graph::FeathersGraphNavigationBlocker>(popup)
            .is_some()
    );
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(popup)
        .unwrap()
        .cursor_over = true;
    app.world_mut().entity_mut(popup).insert(Visibility::Hidden);
    app.update();
    assert!(app.world().get::<super::BrowserSurface>(popup).is_none());
    assert!(
        app.world()
            .get::<crate::feathers::node_graph::FeathersGraphNavigationBlocker>(popup)
            .is_none()
    );
}

#[test]
fn large_folder_pages_bound_entities_and_hidden_rows_leave_keyboard_navigation() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..PAGE_SIZE * 4 + 3 {
        std::fs::write(root.path().join(format!("{i:04}.txt")), []).unwrap();
    }
    let mut app = browser_app(root.path());
    assert_eq!(rows(&mut app).len(), PAGE_SIZE);
    for _ in 0..5 {
        app.world_mut().trigger(BrowserAction::Page(true));
        app.update();
        assert!(rows(&mut app).len() <= PAGE_SIZE * 2);
        let world = app.world_mut();
        let visible = world
            .query_filtered::<Entity, (With<BrowserRow>, With<ListItem>)>()
            .iter(world)
            .count();
        assert!(visible <= PAGE_SIZE);
        for (node, keyboard) in world
            .query::<(&Node, Has<KeyboardNavigableListRow>)>()
            .iter(world)
        {
            if node.display == Display::None {
                assert!(!keyboard);
            }
        }
    }
    assert_eq!(app.world().resource::<AssetBrowserState>().page, 4);
}

fn key(app: &mut App, code: KeyCode) {
    let window = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap()
    };
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: bevy::input::keyboard::Key::Unidentified(
            bevy::input::keyboard::NativeKey::Unidentified,
        ),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

#[test]
fn keyboard_navigates_folders_without_replacing_the_effect_selection() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("folder")).unwrap();
    std::fs::write(root.path().join("folder/first.txt"), []).unwrap();
    std::fs::write(root.path().join("folder/second.txt"), []).unwrap();
    let mut app = browser_app(root.path());
    let list = list(&mut app);
    app.insert_resource(InputFocus::from_entity(list));
    app.update();
    let revision = app.world().resource::<EditorSession>().ui_revision;
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<AssetBrowserState>().folder,
        Path::new("folder")
    );
    let first = app.world().get::<ActiveDescendant>(list).unwrap().0;
    key(&mut app, KeyCode::ArrowDown);
    assert_ne!(app.world().get::<ActiveDescendant>(list).unwrap().0, first);
    key(&mut app, KeyCode::Space);
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .selected
            .is_some()
    );
    key(&mut app, KeyCode::Escape);
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .selected
            .is_none()
    );
    key(&mut app, KeyCode::Backspace);
    assert!(
        app.world()
            .resource::<AssetBrowserState>()
            .folder
            .as_os_str()
            .is_empty()
    );
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision
    );
}

#[derive(Resource, Default)]
struct OpenRequests(Vec<DocumentAction>);

fn click(app: &mut App, target: Entity, count: u8) {
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        },
    };
    app.world_mut().trigger(Pointer::new(
        PointerId::Mouse,
        Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        },
        Click {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
            duration: std::time::Duration::ZERO,
            count,
        },
        target,
    ));
    app.update();
}

#[test]
fn pointer_selects_and_opens_effect_from_row_content() {
    let root = tempfile::tempdir().unwrap();
    let effect = test_support::session_with_timing_slack().effect;
    effect
        .save_ron(root.path().join("valid.aestra.ron"))
        .unwrap();
    let mut app = browser_app(root.path());
    app.init_resource::<OpenRequests>().add_observer(
        |action: On<DocumentAction>, mut requests: ResMut<OpenRequests>| requests.0.push(*action),
    );
    let (source, row) = rows(&mut app).into_iter().next().unwrap();
    let content = app.world().get::<Children>(row).unwrap()[0];
    click(&mut app, content, 1);
    assert_eq!(
        app.world().resource::<AssetBrowserState>().selected,
        Some(source)
    );
    assert!(app.world().resource::<OpenRequests>().0.is_empty());
    let list = list(&mut app);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(list));
    assert_eq!(
        app.world().get::<ActiveDescendant>(list).unwrap().0,
        Some(row)
    );
    // The physical hit changed from the caption wrapper to the row itself: Bevy
    // starts its hit-entity click count again, but this is the same asset.
    click(&mut app, row, 1);
    assert_eq!(
        app.world().resource::<OpenRequests>().0,
        [DocumentAction::OpenCatalog(EffectAssetRef::new(effect.id))]
    );
}

#[test]
fn material_selection_inspects_and_activation_opens_current_effect_graph() {
    use aestra_core::material::{
        MaterialInstance, MaterialProgram, MaterialProgramRef, MaterialRenderState,
    };
    let root = tempfile::tempdir().unwrap();
    let program = MaterialProgram::additive_sprite("Material").normalized();
    program
        .save_ron(root.path().join("test.aestra.material.ron"))
        .unwrap();
    let mut app = browser_app(root.path());
    let mut layout = WorkspaceLayout::default();
    layout.show(DockPanel::MaterialGraph); // Already visible, so no layout file is written.
    app.insert_resource(layout);
    let (source, row) = rows(&mut app).into_iter().next().unwrap();
    let original_selection = app.world().resource::<EditorSession>().selection;
    click(&mut app, row, 1);
    assert_eq!(
        app.world().resource::<AssetBrowserState>().selected,
        Some(source)
    );
    assert_eq!(
        app.world().resource::<EditorSession>().selection,
        original_selection
    );
    let expected = app
        .world()
        .resource::<Localizer>()
        .text("browser-material-context");
    click(&mut app, row, 2);
    assert_eq!(app.world().resource::<EditorSession>().status, expected);
    assert_eq!(
        app.world().resource::<EditorSession>().selection,
        original_selection
    );
    let (renderer, before) = {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        let renderer = &session.effect.emitters[0].renderers[0];
        let (renderer_id, material) = (renderer.id, renderer.material);
        session.effect.material_instances.push(MaterialInstance {
            id: material,
            program: MaterialProgramRef::Project(program.id),
            values: BTreeMap::new(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        (renderer_id, session.effect.clone())
    };
    app.world_mut().trigger(BrowserAction::OpenSelected);
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().selection.primary,
        SemanticTarget::Renderer(renderer)
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    program
        .save_ron(root.path().join("duplicate.aestra.material.ron"))
        .unwrap();
    app.insert_resource(ProjectEffectCatalog::scan(root.path()));
    app.update();
    app.world_mut().resource_mut::<EditorSession>().selection = original_selection;
    app.world_mut().resource_mut::<AssetBrowserState>().selected = Some(source);
    app.world_mut().trigger(BrowserAction::OpenSelected);
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().selection,
        original_selection
    );
}

#[test]
fn effect_activation_uses_guarded_document_routing_once_and_rejects_duplicates() {
    let root = tempfile::tempdir().unwrap();
    let effect = test_support::session_with_timing_slack().effect;
    effect
        .save_ron(root.path().join("valid.aestra.ron"))
        .unwrap();
    let mut app = browser_app(root.path());
    app.init_resource::<OpenRequests>().add_observer(
        |action: On<DocumentAction>, mut requests: ResMut<OpenRequests>| {
            requests.0.push(*action);
        },
    );
    let list = list(&mut app);
    app.insert_resource(InputFocus::from_entity(list));
    app.update();
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world().resource::<OpenRequests>().0,
        [DocumentAction::OpenCatalog(EffectAssetRef::new(effect.id))]
    );
    effect
        .save_ron(root.path().join("duplicate.aestra.ron"))
        .unwrap();
    app.insert_resource(ProjectEffectCatalog::scan(root.path()));
    app.update();
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.world().resource::<OpenRequests>().0.len(), 1);
}

#[test]
fn browser_effect_activation_loads_documents_through_the_background_worker() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("effects")).unwrap();
    let mut first = EffectAsset::from_ron(include_str!(
        "../../../../assets/effects/ember_sigil.aestra.ron"
    ))
    .unwrap();
    first.name = "First browser effect".into();
    let mut second = first.clone();
    second.id = aestra_core::EffectId::new();
    second.name = "Second browser effect".into();
    let first_path = root.path().join("effects/first.aestra.ron");
    let second_path = root.path().join("effects/second.aestra.ron");
    first.save_ron(&first_path).unwrap();
    second.save_ron(&second_path).unwrap();
    let mut app = browser_app(root.path());
    app.insert_resource(EditorSession::from_embedded_sample(EFFECT_SOURCE));
    crate::persistence::install_document_open_test_runtime(&mut app, root.path().join("recovery"));
    let folder = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .source_tree()
        .at_relative_path(Path::new("effects"))
        .unwrap()
        .id;
    app.world_mut().trigger(BrowserAction::Navigate(folder));
    app.update();
    for (effect, path, activation) in [
        (first.clone(), first_path.clone(), 0),
        (second, second_path, 1),
        (first, first_path, 2),
    ] {
        let source = app
            .world()
            .resource::<ProjectEffectCatalog>()
            .content()
            .source_tree()
            .at_relative_path(path.strip_prefix(root.path()).unwrap())
            .unwrap()
            .id;
        let row = rows(&mut app)[&source];
        click(&mut app, row, 1);
        match activation {
            0 => click(&mut app, row, 2),
            1 => key(&mut app, KeyCode::Enter),
            _ => app.world_mut().trigger(BrowserAction::OpenSelected),
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while app.world().resource::<EditorSession>().source_path.as_ref() != Some(&path) {
            assert!(
                std::time::Instant::now() < deadline,
                "Browser did not open {}: {}",
                path.display(),
                app.world().resource::<EditorSession>().status
            );
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let session = app.world().resource::<EditorSession>();
        assert_eq!(session.effect, effect);
        assert!(session.preview.is_some());
    }
}

#[test]
fn legacy_switch_keeps_the_same_dock_slot_and_browser_choices() {
    let root = tempfile::tempdir().unwrap();
    let mut app = browser_app(root.path());
    app.world_mut().trigger(BrowserAction::View(ViewMode::Grid));
    app.update();
    let revision = app.world().resource::<EditorSession>().ui_revision;
    app.world_mut().trigger(BrowserAction::Legacy(true));
    app.update();
    assert!(app.world().resource::<AssetBrowserState>().legacy);
    assert_eq!(
        app.world().resource::<EditorSession>().ui_revision,
        revision + 1
    );
    app.world_mut().trigger(BrowserAction::Legacy(false));
    app.update();
    assert_eq!(
        app.world().resource::<AssetBrowserState>().view,
        ViewMode::Grid
    );
    assert_eq!(DockPanel::Assets.message_id(), "panel-assets");
}
