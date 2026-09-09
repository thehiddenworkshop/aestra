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
fn function_browser_activation_preserves_effect_and_reopens_same_target() {
    let root = tempfile::tempdir().unwrap();
    let function = aestra_core::material::MaterialFunction::from_ron(include_str!(
        "../../../../assets/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function
        .save_ron(root.path().join("unused.aestra.material-function.ron"))
        .unwrap();
    let mut app = browser_app(root.path());
    app.init_resource::<WorkspaceLayout>();
    let effect = app.world().resource::<EditorSession>().effect.clone();
    let selection = app.world().resource::<EditorSession>().selection;
    let source = app
        .world()
        .resource::<ProjectEffectCatalog>()
        .content()
        .source_tree()
        .at_relative_path(Path::new("unused.aestra.material-function.ron"))
        .unwrap()
        .id;
    let row = rows(&mut app)[&source];
    click(&mut app, row, 1);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .standalone_function()
            .is_none()
    );
    key(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .standalone_function(),
        Some(function.id)
    );
    app.world_mut()
        .resource_mut::<EditorSession>()
        .return_to_effect_material();
    click(&mut app, row, 1);
    click(&mut app, row, 2);
    app.update();
    let session = app.world().resource::<EditorSession>();
    assert_eq!(session.standalone_function(), Some(function.id));
    assert_eq!(session.effect, effect);
    assert_eq!(session.selection, selection);
    assert_eq!(
        session
            .graph_function(app.world().resource::<ProjectEffectCatalog>())
            .unwrap(),
        function
    );
}

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

pub(super) fn browser_app(root: &Path) -> App {
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

pub(super) fn rows(app: &mut App) -> BTreeMap<aestra_project::ProjectSourceId, Entity> {
    let world = app.world_mut();
    world
        .query::<(Entity, &BrowserRow)>()
        .iter(world)
        .map(|(e, row)| (row.0, e))
        .collect()
}

pub(super) fn browser_layout_app(root: &Path, size: UVec2, scale_factor: f32) -> App {
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
    let mut app = browser_app(root);
    // The same panel is laid out against an explicit secondary-window camera, as
    // it is when detached. This does not substitute for native window/input QA.
    let window = app.world_mut().spawn(Window::default()).id();
    let panel = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, (With<Node>, Without<ChildOf>)>()
            .single(world)
            .unwrap()
    };
    app.world_mut().get_mut::<Node>(panel).unwrap().width = Val::Px(size.x as f32 / scale_factor);
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
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window)),
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: size,
                        scale_factor,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: size,
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    app.world_mut()
        .entity_mut(panel)
        .insert(UiTargetCamera(camera));
    app
}

#[test]
fn folder_labels_have_visible_layout_inside_their_buttons() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("effects")).unwrap();
    std::fs::write(root.path().join("effects/texture.png"), []).unwrap();
    let mut app = browser_layout_app(root.path(), UVec2::new(420, 520), 1.0);
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
fn sources_divider_has_a_full_height_hit_area_and_scales_drag_on_secondary_targets() {
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::pointer::{Location, PointerId},
    };
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("effects")).unwrap();
    for logical_width in [320, 640, 1440] {
        for scale in [1.0, 1.5, 2.0] {
            let size = UVec2::new(
                (logical_width as f32 * scale) as u32,
                (520.0 * scale) as u32,
            );
            let mut app = browser_layout_app(root.path(), size, scale);
            app.world_mut()
                .resource_mut::<AssetBrowserState>()
                .sources_width = 140.0;
            for _ in 0..4 {
                app.update();
            }
            let (splitter, before, inverse_scale) = {
                let world = app.world_mut();
                let (entity, node, transform) = world
                    .query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<super::panel::SourcesSplitter>>()
                    .single(world).unwrap();
                assert!((node.size().x - 5.0 * scale).abs() < 1.1);
                assert!(
                    node.size().y > 150.0 * scale,
                    "collapsed divider at {logical_width}px/{scale}x"
                );
                assert!((node.inverse_scale_factor() - scale.recip()).abs() < 0.001);
                (entity, transform.translation.x, node.inverse_scale_factor())
            };
            let selected = app.world().resource::<AssetBrowserState>().selected;
            begin_sources_drag(&mut app, splitter, size);
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::None {
                        width: size.x,
                        height: size.y,
                    },
                    position: Vec2::ZERO,
                },
                Drag {
                    button: PointerButton::Primary,
                    distance: Vec2::X * 20.0 * scale,
                    delta: Vec2::X * 20.0 * scale,
                },
                splitter,
            ));
            for _ in 0..4 {
                app.update();
            }
            let state = app.world().resource::<AssetBrowserState>();
            assert!((state.sources_width - 160.0).abs() < 0.01);
            assert_eq!(state.selected, selected, "resizing must not select assets");
            let after = app
                .world()
                .get::<UiGlobalTransform>(splitter)
                .unwrap()
                .translation
                .x;
            assert!(
                ((after - before) * inverse_scale - 20.0).abs() < 1.1,
                "divider did not track drag at {logical_width}px/{scale}x: {before} -> {after}"
            );
            // Saved widths can exceed the pane's 55% layout cap. Starting a new
            // drag must use the visible edge, not that larger preference value.
            app.world_mut()
                .resource_mut::<AssetBrowserState>()
                .sources_width = 360.0;
            for _ in 0..4 {
                app.update();
            }
            let before = app
                .world()
                .get::<UiGlobalTransform>(splitter)
                .unwrap()
                .translation
                .x;
            begin_sources_drag(&mut app, splitter, size);
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::None {
                        width: size.x,
                        height: size.y,
                    },
                    position: Vec2::ZERO,
                },
                Drag {
                    button: PointerButton::Primary,
                    distance: -Vec2::X * 20.0 * scale,
                    delta: -Vec2::X * 20.0 * scale,
                },
                splitter,
            ));
            for _ in 0..4 {
                app.update();
            }
            let after = app
                .world()
                .get::<UiGlobalTransform>(splitter)
                .unwrap()
                .translation
                .x;
            assert!(
                ((after - before) * inverse_scale + 20.0).abs() < 1.1,
                "clamped divider sticks at {logical_width}px/{scale}x: {before} -> {after}"
            );
        }
    }
}

fn begin_sources_drag(app: &mut App, splitter: Entity, size: UVec2) {
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
                width: size.x,
                height: size.y,
            },
            position: Vec2::ZERO,
        },
        DragStart {
            button: PointerButton::Primary,
            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
        },
        splitter,
    ));
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
            "asset-details",
            "references",
            "inspector-empty",
            "inspected-missing",
            "details",
            "dependencies",
            "usages",
            "locate",
            "locate-current",
            "located",
            "snapshot-scope",
            "relations-unknown",
            "usages-incomplete",
            "relations-empty",
            "relation-available",
            "relation-missing",
            "relation-ambiguous",
            "relation-unavailable",
            "relation-builtin",
            "relation-context",
        ] {
            let key = format!("browser-{suffix}");
            assert_ne!(localizer.text(&key), key);
        }
    }
}

#[test]
fn locate_reveals_filtered_off_page_source_without_changing_document() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("deep/nested")).unwrap();
    for i in 0..PAGE_SIZE + 10 {
        std::fs::write(root.path().join(format!("deep/nested/{i:04}.txt")), []).unwrap();
    }
    let mut app = browser_app(root.path());
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let target = catalog
        .content()
        .source_tree()
        .at_relative_path("deep/nested/0100.txt")
        .unwrap()
        .id;
    let version = catalog.content_revision();
    let original = app.world().resource::<EditorSession>().effect.clone();
    let revision = app.world().resource::<EditorSession>().document_revision();
    for view in [ViewMode::List, ViewMode::Grid] {
        {
            let mut state = app.world_mut().resource_mut::<AssetBrowserState>();
            state.query = "nothing matches".into();
            state.kinds.insert(Kind::Material);
            state.expanded.clear();
            state.sources_visible = false;
            state.view = view;
        }
        app.world_mut()
            .trigger(BrowserAction::LocateSource(target, version));
        app.update();
        let state = app.world().resource::<AssetBrowserState>();
        assert_eq!(state.folder, Path::new("deep/nested"));
        assert_eq!(state.selected, Some(target));
        assert_eq!(state.page, 1);
        assert!(state.query.is_empty() && state.kinds.is_empty() && state.sources_visible);
        assert_eq!(state.expanded.len(), 3);
        let row = rows(&mut app)[&target];
        let list = list(&mut app);
        assert_eq!(
            app.world().get::<ActiveDescendant>(list).unwrap().0,
            Some(row)
        );
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(list));
        assert_eq!(app.world().resource::<EditorSession>().effect, original);
        assert_eq!(
            app.world().resource::<EditorSession>().document_revision(),
            revision
        );
    }
}

#[test]
fn inspection_tabs_preserve_browser_rows_and_stale_locate_is_ignored() {
    let root = tempfile::tempdir().unwrap();
    let effect = aestra_core::EffectAsset::new("Effect", 1.0);
    effect
        .save_ron(root.path().join("effect.aestra.ron"))
        .unwrap();
    let mut app = browser_app(root.path());
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("effect.aestra.ron")
        .unwrap()
        .id;
    let version = catalog.content_revision();
    app.world_mut()
        .trigger(BrowserAction::LocateSource(source, version));
    app.update();
    let original_rows = rows(&mut app);
    let revision = app.world().resource::<EditorSession>().ui_revision;
    for tab in [
        InspectionTab::Dependencies,
        InspectionTab::Usages,
        InspectionTab::Details,
    ] {
        app.world_mut().trigger(BrowserAction::InspectionTab(tab));
        app.update();
        assert_eq!(rows(&mut app), original_rows);
        assert_eq!(
            app.world().resource::<AssetBrowserState>().selected,
            Some(source)
        );
        assert_eq!(
            app.world().resource::<EditorSession>().ui_revision,
            revision
        );
    }
    app.world_mut().resource_mut::<AssetBrowserState>().selected = None;
    app.world_mut().trigger(BrowserAction::LocateSource(
        source,
        ProjectContentVersion {
            revision: version.revision + 1,
            ..version
        },
    ));
    app.update();
    assert_eq!(app.world().resource::<AssetBrowserState>().selected, None);
}

#[test]
fn semantic_locate_does_not_choose_between_duplicate_sources() {
    let root = tempfile::tempdir().unwrap();
    let effect = aestra_core::EffectAsset::new("Duplicate", 1.0);
    for name in ["a", "b"] {
        effect
            .save_ron(root.path().join(format!("{name}.aestra.ron")))
            .unwrap();
    }
    let mut app = browser_app(root.path());
    app.world_mut().trigger(super::LocateInAssets(
        aestra_project::ProjectAssetId::Effect(effect.id),
    ));
    app.update();
    assert_eq!(app.world().resource::<AssetBrowserState>().selected, None);
    assert!(
        app.world()
            .resource::<EditorSession>()
            .status
            .starts_with("Cannot locate")
    );
}

#[test]
fn context_menu_explicitly_inspects_without_opening_or_following_selection() {
    let root = tempfile::tempdir().unwrap();
    let effect = aestra_core::EffectAsset::new("Menu target", 1.0);
    effect
        .save_ron(root.path().join("effect.aestra.ron"))
        .unwrap();
    std::fs::write(root.path().join("other.png"), []).unwrap();
    let mut app = browser_app(root.path());
    app.init_resource::<OpenRequests>().add_observer(
        |event: On<DocumentAction>, mut requests: ResMut<OpenRequests>| requests.0.push(*event),
    );
    let entries = rows(&mut app);
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("effect.aestra.ron")
        .unwrap()
        .id;
    let other = catalog
        .content()
        .source_tree()
        .at_relative_path("other.png")
        .unwrap()
        .id;
    let version = catalog.content_revision();
    let revision = app.world().resource::<EditorSession>().document_revision();
    let content = app.world().get::<Children>(entries[&source]).unwrap()[0];
    click_button(&mut app, content, 1, PointerButton::Secondary);
    assert_eq!(
        app.world().resource::<AssetBrowserState>().selected,
        Some(source)
    );
    assert_eq!(app.world().resource::<AssetBrowserState>().inspected, None);
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&super::context_menu::BrowserContextMenu>()
            .iter(world)
            .count(),
        1
    );
    let action = world
        .query::<(Entity, &BrowserAction)>()
        .iter(world)
        .find(|(_, action)| {
            matches!(
                action,
                BrowserAction::InspectSource(_, _, InspectionTab::Dependencies)
            )
        })
        .unwrap()
        .0;
    world.trigger(bevy::ui_widgets::Activate { entity: action });
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&super::context_menu::BrowserContextMenu>()
            .iter(world)
            .count(),
        0
    );
    assert_eq!(
        world.resource::<AssetBrowserState>().inspected,
        Some(source)
    );
    assert_eq!(
        world.resource::<AssetBrowserState>().inspection_tab,
        InspectionTab::Dependencies
    );
    world.spawn(super::inspection::AssetInspectorUi::default());
    app.update();
    {
        let world = app.world_mut();
        let scope = world.resource::<Localizer>().text("browser-snapshot-scope");
        let info = world
            .query::<(&AccessibleLabel, &Children)>()
            .iter(world)
            .find(|(label, _)| label.0 == scope)
            .expect("snapshot scope hover target");
        let icon = info
            .1
            .iter()
            .find(|child| world.get::<bevy_resvg::prelude::UiSvg>(*child).is_some())
            .expect("info is an SVG, not a font-dependent glyph");
        let svg = world.get::<bevy_resvg::prelude::UiSvg>(icon).unwrap();
        assert_eq!(
            svg.0.path().unwrap().path().to_str(),
            Some("icons/info.svg")
        );
        assert_eq!(
            world.get::<bevy_resvg::prelude::SvgColor>(icon).unwrap().0,
            theme::TEXT
        );
        assert_eq!(world.get::<Node>(icon).unwrap().width, Val::Px(16.0));
        assert!(world.get::<Text>(icon).is_none());
    }
    click(&mut app, entries[&other], 1);
    assert_eq!(
        app.world().resource::<AssetBrowserState>().inspected,
        Some(source)
    );
    assert_eq!(
        app.world().resource::<AssetBrowserState>().selected,
        Some(other)
    );
    assert_eq!(
        app.world().resource::<EditorSession>().document_revision(),
        revision
    );
    assert!(app.world().resource::<OpenRequests>().0.is_empty());
    app.world_mut().trigger(BrowserAction::InspectSource(
        other,
        ProjectContentVersion {
            revision: version.revision + 1,
            ..version
        },
        InspectionTab::Details,
    ));
    app.update();
    assert_eq!(
        app.world().resource::<AssetBrowserState>().inspected,
        Some(source)
    );
    let world = app.world_mut();
    assert!(
        !world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0.starts_with("Saved snapshot"))
    );
}

#[test]
fn keyboard_context_menu_and_escape_restore_list_focus() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("asset.png"), []).unwrap();
    let mut app = browser_app(root.path());
    app.init_resource::<ButtonInput<KeyCode>>();
    let list = list(&mut app);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(list, bevy::input_focus::FocusCause::Navigated);
    key(&mut app, KeyCode::ContextMenu);
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&super::context_menu::BrowserContextMenu>()
            .iter(world)
            .count(),
        1
    );
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<&super::context_menu::BrowserContextMenu>()
            .iter(world)
            .count(),
        0
    );
    assert_eq!(world.resource::<InputFocus>().get(), Some(list));
}

#[test]
fn supported_sources_share_menu_capabilities_and_inline_rename() {
    for kind in 0..5 {
        for view in [ViewMode::List, ViewMode::Grid] {
            let root = tempfile::tempdir().unwrap();
            let suffix = match kind {
                0 => ".aestra.ron",
                1 => ".aestra.material.ron",
                2 => ".aestra.material-function.ron",
                3 => ".PNG",
                _ => "",
            };
            let path = root.path().join(format!("original{suffix}"));
            match kind {
                0 => EffectAsset::new("Authored name", 2.0)
                    .save_ron(&path)
                    .unwrap(),
                1 => aestra_core::material::MaterialProgram::additive_sprite("Authored name")
                    .save_ron(&path)
                    .unwrap(),
                2 => aestra_core::material::MaterialFunction::from_ron(include_str!(
                    "../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
                ))
                .unwrap()
                .save_ron(&path)
                .unwrap(),
                3 => std::fs::write(&path, b"test texture").unwrap(),
                _ => std::fs::create_dir(&path).unwrap(),
            }
            let bytes = std::fs::read(&path).ok();
            if kind == 4 {
                // An unrelated mesh subasset in the project must not block folder Rename.
                std::fs::create_dir(root.path().join("z_meshes")).unwrap();
                std::fs::write(
                    root.path().join("z_meshes/lab_cube.gltf"),
                    include_bytes!("../../../../assets/meshes/lab_cube.gltf"),
                )
                .unwrap();
                let mut effect = EffectAsset::new("Mesh effect", 1.0);
                effect.assets.push(aestra_core::AssetDefinition {
                    id: aestra_core::AssetId::new(),
                    name: "Cube".into(),
                    kind: aestra_core::AssetKind::Mesh,
                    path: "z_meshes/lab_cube.gltf#Mesh0/Primitive0".into(),
                });
                effect
                    .save_ron(root.path().join("z_effect.aestra.ron"))
                    .unwrap();
            }
            let mut app = browser_app(root.path());
            app.world_mut().resource_mut::<AssetBrowserState>().view = view;
            app.update();
            let list = list(&mut app);
            app.world_mut()
                .resource_mut::<InputFocus>()
                .set(list, bevy::input_focus::FocusCause::Navigated);
            key(&mut app, KeyCode::ContextMenu);
            let world = app.world_mut();
            let actions = world
                .query::<&BrowserAction>()
                .iter(world)
                .collect::<Vec<_>>();
            assert!(
                actions
                    .iter()
                    .any(|action| matches!(action, BrowserAction::Rename(_, _)))
            );
            assert_eq!(
                actions
                    .iter()
                    .any(|action| matches!(action, BrowserAction::Duplicate(_, _))),
                kind < 3
            );
            app.init_resource::<ButtonInput<KeyCode>>();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .reset_all();
            key(&mut app, KeyCode::F2);
            let input = app.world().resource::<InputFocus>().get().unwrap();
            let mut text = app
                .world_mut()
                .get_mut::<bevy::text::EditableText>(input)
                .expect("shared inline editor");
            assert_eq!(text.value(), "original");
            text.editor_mut().set_text("renamed");
            key(&mut app, KeyCode::Enter);
            crate::project_content::io::drain(app.world_mut());
            app.update();
            assert!(
                !path.exists(),
                "kind={kind}: {}",
                app.world().resource::<EditorSession>().status
            );
            let destination = root.path().join(format!("renamed{suffix}"));
            assert!(destination.exists());
            assert_eq!(std::fs::read(destination).ok(), bytes);
        }
    }
}

#[test]
fn f2_edits_label_inline_and_escape_restores_it_in_both_views() {
    for view in [ViewMode::List, ViewMode::Grid] {
        let root = tempfile::tempdir().unwrap();
        let program = aestra_core::material::MaterialProgram::additive_sprite("Display name");
        let path = root.path().join("original.aestra.material.ron");
        program.save_ron(&path).unwrap();
        let mut app = browser_layout_app(root.path(), UVec2::new(640, 520), 1.0);
        app.world_mut().resource_mut::<AssetBrowserState>().view = view;
        app.update();
        let effect = app.world().resource::<EditorSession>().effect.clone();
        let list = list(&mut app);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(list, bevy::input_focus::FocusCause::Navigated);
        key(&mut app, KeyCode::F2);
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<super::operations::InlineRenameEditor>>()
                .iter(world)
                .count(),
            1
        );
        assert!(
            !world
                .query::<&Text>()
                .iter(world)
                .any(|text| text.0 == "Rename")
        );
        let input = world.resource::<InputFocus>().get().unwrap();
        assert_eq!(
            world
                .get::<bevy::text::EditableText>(input)
                .unwrap()
                .value(),
            "original"
        );
        let wrapper = world
            .query_filtered::<&ComputedNode, With<super::operations::InlineRenameEditor>>()
            .single(world)
            .unwrap();
        assert!(
            wrapper.size().x > 60.0 && wrapper.size().y >= 21.0,
            "{:?}",
            wrapper.size()
        );
        let caption = world.query_filtered::<(Entity, &Text), With<crate::feathers::list_row::ListRowPrimaryLabel>>().iter(world).find(|(_, text)| text.0 == "original").unwrap().0;
        assert_eq!(world.get::<Node>(caption).unwrap().display, Display::None);
        assert_eq!(world.resource::<EditorSession>().effect, effect);
        assert!(path.exists());
        key(&mut app, KeyCode::Escape);
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<super::operations::InlineRenameEditor>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(world.get::<Node>(caption).unwrap().display, Display::Flex);
        assert_eq!(world.resource::<InputFocus>().get(), Some(list));
        assert!(path.exists());
    }
}

#[test]
fn inline_rename_enter_validates_and_invalid_blur_keeps_the_editor() {
    for name in ["original", "renamed", "", "taken"] {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original.aestra.material.ron");
        aestra_core::material::MaterialProgram::additive_sprite("Display name")
            .save_ron(&original)
            .unwrap();
        aestra_core::material::MaterialProgram::additive_sprite("Other")
            .save_ron(root.path().join("taken.aestra.material.ron"))
            .unwrap();
        let mut app = browser_app(root.path());
        let list = list(&mut app);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(list, bevy::input_focus::FocusCause::Navigated);
        key(&mut app, KeyCode::F2);
        let input = app.world().resource::<InputFocus>().get().unwrap();
        app.world_mut()
            .get_mut::<bevy::text::EditableText>(input)
            .unwrap()
            .editor_mut()
            .set_text(name);
        key(&mut app, KeyCode::Enter);
        crate::project_content::io::drain(app.world_mut());
        app.update();
        let world = app.world_mut();
        let active = world
            .query_filtered::<Entity, With<super::operations::InlineRenameEditor>>()
            .iter(world)
            .count();
        assert_eq!(
            active,
            usize::from(name.is_empty() || name == "taken"),
            "{name}"
        );
        assert_eq!(original.exists(), name != "renamed", "{name}");
        if name == "renamed" {
            assert!(root.path().join("renamed.aestra.material.ron").exists());
        }
        if active == 1 {
            world
                .resource_mut::<InputFocus>()
                .set(list, bevy::input_focus::FocusCause::Navigated);
            app.update();
            let world = app.world_mut();
            assert_eq!(
                world
                    .query_filtered::<Entity, With<super::operations::InlineRenameEditor>>()
                    .iter(world)
                    .count(),
                1
            );
            assert!(original.exists());
            key(&mut app, KeyCode::Escape);
        }
    }
}

#[test]
fn inline_rename_submits_latest_queued_text_on_enter_blur_and_outside_press() {
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        },
        text::TextEdit,
    };
    for finish in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original.aestra.material.ron");
        aestra_core::material::MaterialProgram::additive_sprite("Display name")
            .save_ron(&original)
            .unwrap();
        let mut app = browser_app(root.path());
        app.add_observer(crate::feathers::text_input::emit_text_change)
            .add_observer(crate::feathers::text_input::submit_text_on_enter)
            .add_observer(crate::feathers::text_input::submit_text_on_focus_loss);
        let list = list(&mut app);
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(list, bevy::input_focus::FocusCause::Navigated);
        key(&mut app, KeyCode::F2);
        let input = app.world().resource::<InputFocus>().get().unwrap();
        {
            let mut text = app
                .world_mut()
                .get_mut::<bevy::text::EditableText>(input)
                .unwrap();
            // This fixture has no text layout for Parley's selection commands.
            // Reset the editor, then leave the replacement queued until the
            // same frame as confirmation to exercise native edit timing.
            *text = bevy::text::EditableText::new("");
            text.queue_edit(TextEdit::Insert("renamed".into()));
        }
        match finish {
            0 => key(&mut app, KeyCode::Enter),
            1 => {
                app.world_mut()
                    .resource_mut::<InputFocus>()
                    .set(list, bevy::input_focus::FocusCause::Pressed);
                app.update();
            }
            _ => {
                app.world_mut().trigger(Pointer::new(
                    PointerId::Mouse,
                    Location {
                        target: NormalizedRenderTarget::None {
                            width: 800,
                            height: 600,
                        },
                        position: Vec2::ZERO,
                    },
                    Press {
                        button: PointerButton::Primary,
                        count: 1,
                        hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                    },
                    list,
                ));
                app.update();
            }
        }
        // Focus changes must not cancel the asynchronous preflight submitted by Enter.
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(list, bevy::input_focus::FocusCause::Pressed);
        app.update();
        crate::project_content::io::drain(app.world_mut());
        app.update();
        assert!(
            !original.exists(),
            "finish={finish}: {}",
            app.world().resource::<EditorSession>().status
        );
        assert!(
            root.path().join("renamed.aestra.material.ron").exists(),
            "finish={finish}: {}",
            app.world().resource::<EditorSession>().status
        );
        let world = app.world_mut();
        assert!(
            world
                .query_filtered::<&Text, With<crate::feathers::list_row::ListRowPrimaryLabel>>()
                .iter(world)
                .any(|text| text.0 == "renamed")
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<super::operations::InlineRenameEditor>>()
                .iter(world)
                .count(),
            0
        );
    }
}

#[test]
fn rebuilding_browser_after_locate_does_not_steal_focus_back() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("source.png"), []).unwrap();
    let mut app = browser_app(root.path());
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("source.png")
        .unwrap()
        .id;
    let version = catalog.content_revision();
    app.world_mut()
        .trigger(BrowserAction::LocateSource(source, version));
    app.update();
    let world = app.world_mut();
    let panels = world
        .query_filtered::<Entity, With<super::panel::BrowserUi>>()
        .iter(world)
        .collect::<Vec<_>>();
    for panel in panels {
        world.despawn(panel);
    }
    let other_control = world.spawn_empty().id();
    world
        .resource_mut::<InputFocus>()
        .set(other_control, bevy::input_focus::FocusCause::Navigated);
    app.add_systems(
        Update,
        spawn_browser_fixture.run_if(bevy::ecs::schedule::common_conditions::run_once),
    );
    app.update();
    app.update();
    assert_eq!(
        app.world().resource::<InputFocus>().get(),
        Some(other_control)
    );
}

#[test]
fn inspector_pages_bound_locate_controls_and_missing_references_are_not_openable() {
    let root = tempfile::tempdir().unwrap();
    let mut effect = aestra_core::EffectAsset::new("Resources", 1.0);
    for i in 0..65 {
        let path = format!("{i:02}.png");
        if i != 64 {
            std::fs::write(root.path().join(&path), []).unwrap();
        }
        effect.assets.push(aestra_core::AssetDefinition::texture(
            format!("Texture {i}"),
            path,
        ));
    }
    effect
        .save_ron(root.path().join("effect.aestra.ron"))
        .unwrap();
    let mut app = browser_app(root.path());
    let catalog = app.world().resource::<ProjectEffectCatalog>();
    let source = catalog
        .content()
        .source_tree()
        .at_relative_path("effect.aestra.ron")
        .unwrap()
        .id;
    let version = catalog.content_revision();
    app.world_mut()
        .spawn(super::inspection::AssetInspectorUi::default());
    app.world_mut().trigger(BrowserAction::InspectSource(
        source,
        version,
        InspectionTab::Dependencies,
    ));
    app.update();
    for expected in [24, 24, 16] {
        let world = app.world_mut();
        let count = world
            .query::<&BrowserAction>()
            .iter(world)
            .filter(|action| matches!(action, BrowserAction::LocateSource(..)))
            .count();
        assert_eq!(count, expected);
        app.world_mut().trigger(BrowserAction::InspectionPage(true));
        app.update();
    }
    let world = app.world_mut();
    assert!(
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0 == "Missing")
    );
    assert_eq!(world.resource::<AssetBrowserState>().inspection_page, 2);
    // Browsing another source leaves the explicit inspector target/page unchanged.
    let catalog = world.resource::<ProjectEffectCatalog>();
    let texture = catalog
        .content()
        .source_tree()
        .at_relative_path("00.png")
        .unwrap()
        .id;
    world.trigger(BrowserAction::LocateSource(texture, version));
    app.update();
    assert_eq!(
        app.world().resource::<AssetBrowserState>().inspection_page,
        2
    );
    assert_eq!(
        app.world().resource::<AssetBrowserState>().inspected,
        Some(source)
    );
    app.world_mut().trigger(BrowserAction::InspectSource(
        texture,
        version,
        InspectionTab::Dependencies,
    ));
    app.update();
    assert_eq!(
        app.world().resource::<AssetBrowserState>().inspection_page,
        0
    );
    let world = app.world_mut();
    assert!(
        world
            .query::<&Text>()
            .iter(world)
            .any(|text| text.0.starts_with("Dependencies unknown"))
    );
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
    click_button(app, target, count, PointerButton::Primary);
}

fn click_button(app: &mut App, target: Entity, count: u8, button: PointerButton) {
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
            button,
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
    assert_eq!(app.world().resource::<AssetBrowserState>().inspected, None);
    assert_eq!(
        app.world_mut()
            .query::<&super::inspection::AssetInspectorUi>()
            .iter(app.world())
            .count(),
        0
    );
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
fn material_activation_opens_shared_source_without_retargeting_the_effect() {
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
        .text("browser-material-opened");
    click(&mut app, row, 2);
    assert_eq!(app.world().resource::<EditorSession>().status, expected);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .standalone_material(),
        Some(program.id)
    );
    assert_eq!(
        app.world().resource::<EditorSession>().selection,
        original_selection
    );
    let before = {
        let mut session = app.world_mut().resource_mut::<EditorSession>();
        let renderer = &session.effect.emitters[0].renderers[0];
        let material = renderer.material;
        session.effect.material_instances.push(MaterialInstance {
            id: material,
            program: MaterialProgramRef::Project(program.id),
            values: BTreeMap::new(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        session.effect.clone()
    };
    app.world_mut().trigger(BrowserAction::OpenSelected);
    app.update();
    assert_eq!(
        app.world().resource::<EditorSession>().selection,
        original_selection
    );
    assert_eq!(app.world().resource::<EditorSession>().effect, before);
    app.world_mut()
        .resource_mut::<EditorSession>()
        .return_to_effect_material();
    let focused_list = list(&mut app);
    app.insert_resource(InputFocus::from_entity(focused_list));
    app.update();
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.world()
            .resource::<EditorSession>()
            .standalone_material(),
        Some(program.id)
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
