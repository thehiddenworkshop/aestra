//! Browser-only folder drops. The menu is an intent; filesystem authority stays
//! in the shared project planners and the serialized, draft-guarded I/O queue.
use super::{
    actions::BrowserAction,
    panel::{BrowserFolderButton, BrowserRow},
};
use crate::{
    feathers::context_menu::*,
    project_content::io::{self, IoGuard},
    *,
};
use aestra_project::{
    ProjectContentVersion, ProjectSourceId, ProjectSourceKind,
    content::operations::OperationRequest,
};
use bevy::{
    picking::events::{DragEnter, DragLeave, Press},
    ui_widgets::Activate,
};

#[derive(Resource, Default)]
pub(super) struct AssetDrag {
    source: Option<(ProjectSourceId, ProjectContentVersion)>,
    ended: bool,
    pub(super) suppress_click: bool,
    preview: Option<Entity>,
    origin: Option<Entity>,
}

#[derive(Component)]
struct DropHighlight;
#[derive(Component)]
struct DropMenu;
#[derive(Component)]
struct DropAnchor(ProjectContentVersion);
#[derive(Component, Clone, Copy)]
struct DropChoice {
    source: ProjectSourceId,
    parent: ProjectSourceId,
    version: ProjectContentVersion,
    copy: bool,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<AssetDrag>()
        .add_observer(begin)
        .add_observer(super::drag_preview::follow_pointer)
        .add_observer(press)
        .add_observer(enter)
        .add_observer(leave)
        .add_observer(drop_asset)
        .add_observer(end)
        .add_observer(choose)
        .add_systems(Update, (dismiss, recover_on_project_open))
        .add_systems(PostUpdate, clear_ended.before(bevy::ui::UiSystems::Layout));
}

fn recover_on_project_open(
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    mut checked: Local<Option<u64>>,
    mut commands: Commands,
) {
    let generation = catalog.content_revision().generation;
    if *checked == Some(generation) || !io::idle(tasks) {
        return;
    }
    *checked = Some(generation);
    let root = catalog.content().source_tree().root_path();
    if !root.join(".aestra/asset-moves").exists()
        || super::relocation_recovery::journal_present(root)
    {
        return;
    }
    let prepared = catalog.clone();
    let previous_status = session.status.clone();
    let guard = IoGuard::capture(&catalog, &session);
    io::enqueue(&mut commands, guard.clone(), move || {
        let result = prepared.content().recover_asset_moves();
        io::completion(move |world| {
            if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
                return;
            }
            match result {
                Ok(0) => {
                    let running = world
                        .resource::<Localizer>()
                        .text("project-operation-running");
                    let mut session = world.resource_mut::<EditorSession>();
                    if session.status == running {
                        session.status = previous_status;
                    }
                }
                Ok(count) => {
                    world.resource_mut::<EditorSession>().status = format!(
                        "Verified {count} interrupted asset move(s); no asset files were changed during recovery"
                    )
                }
                Err(error) => {
                    world.resource_mut::<EditorSession>().status =
                        format!("Asset move recovery: {error}")
                }
            }
        })
    });
}

#[allow(clippy::too_many_arguments)]
fn begin(
    event: On<Pointer<DragStart>>,
    rows: Query<&BrowserRow>,
    editors: Query<(), With<super::operations::InlineRenameEditor>>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    mut drag: ResMut<AssetDrag>,
    mut clicks: ResMut<super::actions::BrowserClickState>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    assets: Res<AssetServer>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary || event.entity != event.original_event_target() {
        return;
    }
    let ancestors: Vec<_> = std::iter::once(event.entity)
        .chain(parents.iter_ancestors(event.entity))
        .collect();
    if ancestors.iter().any(|entity| editors.contains(*entity)) {
        return;
    }
    if let Some(row) = ancestors.iter().find_map(|entity| rows.get(*entity).ok()) {
        if let Some(preview) = drag.preview.take() {
            commands.entity(preview).try_despawn();
        }
        // Attach to the originating window's UI root, not the scroll-clipped row.
        if let Some(root) = ancestors
            .iter()
            .rev()
            .copied()
            .find(|entity| geometry.contains(*entity))
            && let Some(entry) = catalog.content().source(row.0)
            && let Ok((node, transform)) = geometry.get(root)
        {
            drag.preview = Some(super::drag_preview::spawn(
                &mut commands,
                root,
                event.entity,
                event.pointer_id,
                pointer_position_in_node(event.pointer_location.position, node, transform)
                    * node.inverse_scale_factor(),
                entry,
                &assets,
                &localizer,
            ));
        }
        drag.source = Some((row.0, catalog.content_revision()));
        drag.origin = Some(event.entity);
        drag.ended = false;
        drag.suppress_click = true;
        *clicks = default();
    }
}

fn press(event: On<Pointer<Press>>, mut drag: ResMut<AssetDrag>) {
    if event.button == PointerButton::Primary {
        drag.suppress_click = false;
    }
}

fn target(
    entity: Entity,
    rows: &Query<&BrowserRow>,
    folders: &Query<&BrowserAction, With<BrowserFolderButton>>,
    parents: &Query<&ChildOf>,
    catalog: &ProjectEffectCatalog,
) -> Option<(Entity, ProjectSourceId)> {
    std::iter::once(entity)
        .chain(parents.iter_ancestors(entity))
        .find_map(|entity| {
            let id = if let Ok(BrowserAction::Navigate(id)) = folders.get(entity) {
                *id
            } else {
                rows.get(entity).ok()?.0
            };
            catalog
                .content()
                .source(id)
                .filter(|entry| entry.kind == ProjectSourceKind::Directory)
                .map(|_| (entity, id))
        })
}

fn eligible(
    catalog: &ProjectEffectCatalog,
    source: ProjectSourceId,
    parent: ProjectSourceId,
    version: ProjectContentVersion,
) -> Result<(), String> {
    if version != catalog.content_revision() {
        return Err("Project changed; drag the asset again".into());
    }
    if catalog.content().source_relocation_suffix(source).is_none() {
        return Err(
            "This source is read-only or its format is not supported for relocation.".into(),
        );
    }
    let entry = catalog
        .content()
        .source(source)
        .ok_or("Source no longer exists")?;
    if entry.parent == Some(parent) {
        return Err("Asset is already in this folder".into());
    }
    let target = catalog
        .content()
        .source(parent)
        .ok_or("Destination no longer exists")?;
    if target.kind != ProjectSourceKind::Directory {
        return Err("Drop onto a folder".into());
    }
    if entry.kind == ProjectSourceKind::Directory
        && target.relative_path.starts_with(&entry.relative_path)
    {
        return Err("A folder cannot move into itself or one of its descendants".into());
    }
    catalog
        .content()
        .plan_operation(OperationRequest::CreateFolder {
            parent,
            name: entry.name.to_string_lossy().into_owned(),
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
fn enter(
    mut event: On<Pointer<DragEnter>>,
    rows: Query<&BrowserRow>,
    folders: Query<&BrowserAction, With<BrowserFolderButton>>,
    parents: Query<&ChildOf>,
    catalog: Res<ProjectEffectCatalog>,
    drag: Res<AssetDrag>,
    highlights: Query<Entity, With<DropHighlight>>,
    mut session: ResMut<EditorSession>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some((source, version)) = drag.source else {
        return;
    };
    let Some((entity, parent)) = target(event.entity, &rows, &folders, &parents, &catalog) else {
        return;
    };
    event.propagate(false);
    for highlight in &highlights {
        commands.entity(highlight).despawn();
    }
    let valid = eligible(&catalog, source, parent, version);
    session.status = valid.as_ref().map_or_else(
        |reason| format!("Drop blocked: {reason}"),
        |_| {
            if catalog.content().asset_operation_suffix(source).is_some() {
                "Release to choose Move Here or Copy Here".into()
            } else {
                "Release to choose Move Here".into()
            }
        },
    );
    commands.entity(entity).with_children(|parent| {
        parent.spawn((
            DropHighlight,
            Pickable::IGNORE,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BorderColor::all(if valid.is_ok() {
                theme::ACCENT
            } else {
                Color::srgb(0.95, 0.3, 0.3)
            }),
        ));
    });
}

fn leave(
    event: On<Pointer<DragLeave>>,
    parents: Query<&ChildOf>,
    highlights: Query<(Entity, &ChildOf), With<DropHighlight>>,
    mut commands: Commands,
) {
    for (entity, parent) in &highlights {
        if parent.parent() == event.entity
            || parents
                .iter_ancestors(event.entity)
                .any(|ancestor| ancestor == parent.parent())
        {
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn drop_asset(
    mut event: On<Pointer<DragDrop>>,
    rows: Query<&BrowserRow>,
    folders: Query<&BrowserAction, With<BrowserFolderButton>>,
    parents: Query<&ChildOf>,
    geometry: Query<(&ComputedNode, &UiGlobalTransform)>,
    catalog: Res<ProjectEffectCatalog>,
    drag: Res<AssetDrag>,
    menus: Query<Entity, With<DropAnchor>>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Some((source, version)) = drag.source else {
        return;
    };
    if !std::iter::once(event.dropped)
        .chain(parents.iter_ancestors(event.dropped))
        .any(|entity| rows.get(entity).is_ok_and(|row| row.0 == source))
    {
        return;
    }
    let Some((entity, parent)) = target(event.entity, &rows, &folders, &parents, &catalog) else {
        return;
    };
    event.propagate(false);
    if let Err(reason) = eligible(&catalog, source, parent, version) {
        session.status = format!("Drop blocked: {reason}");
        return;
    }
    let Ok((node, transform)) = geometry.get(entity) else {
        return;
    };
    let position = pointer_position_in_node(event.pointer_location.position, node, transform)
        * node.inverse_scale_factor();
    for menu in &menus {
        commands.entity(menu).despawn();
    }
    commands.entity(entity).with_children(|host| {
        spawn_pointer_context_menu(host, position, DropAnchor(version), DropMenu, |menu| {
            for (key, copy) in [("browser-move-here", false), ("browser-copy-here", true)] {
                if copy && catalog.content().asset_operation_suffix(source).is_none() {
                    continue;
                }
                spawn_pointer_context_menu_item(
                    menu,
                    &localizer.text(key),
                    DropChoice {
                        source,
                        parent,
                        version,
                        copy,
                    },
                );
            }
        });
    });
}

fn end(event: On<Pointer<DragEnd>>, mut drag: ResMut<AssetDrag>) {
    if event.button == PointerButton::Primary {
        drag.ended = true;
    }
}

fn clear_ended(
    mut drag: ResMut<AssetDrag>,
    highlights: Query<Entity, With<DropHighlight>>,
    entities: Query<()>,
    mut commands: Commands,
) {
    if drag.ended || drag.origin.is_some_and(|entity| !entities.contains(entity)) {
        if let Some(preview) = drag.preview.take() {
            commands.entity(preview).try_despawn();
        }
        drag.source = None;
        drag.origin = None;
        drag.ended = false;
        for highlight in &highlights {
            commands.entity(highlight).despawn();
        }
    }
}

fn dismiss(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    surfaces: Query<&RelativeCursorPosition, With<DropMenu>>,
    menus: Query<(Entity, &DropAnchor)>,
    catalog: Res<ProjectEffectCatalog>,
    mut drag: ResMut<AssetDrag>,
    mut commands: Commands,
) {
    let escape = keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
    if escape
        || drag
            .source
            .is_some_and(|(_, version)| version != catalog.content_revision())
    {
        drag.source = None;
        drag.ended = true;
    }
    let outside = should_dismiss_pointer_context_menu(
        true,
        buttons.is_some_and(|buttons| buttons.just_pressed(MouseButton::Left)),
        escape,
        surfaces.iter().any(RelativeCursorPosition::cursor_over),
    );
    for (entity, anchor) in &menus {
        if outside || anchor.0 != catalog.content_revision() {
            commands.entity(entity).despawn();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn choose(
    event: On<Activate>,
    choices: Query<&DropChoice>,
    menus: Query<Entity, With<DropAnchor>>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    protection: Option<Res<DocumentProtectionState>>,
    mut commands: Commands,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if !io::idle(tasks) || protection.is_some_and(|state| state.is_open()) {
        return;
    }
    let choice = *choice;
    for menu in &menus {
        commands.entity(menu).despawn();
    }
    if let Err(reason) = eligible(&catalog, choice.source, choice.parent, choice.version) {
        session.status = reason;
        return;
    }
    let entry = catalog.content().source(choice.source).unwrap();
    let suffix = catalog.content().asset_operation_suffix(choice.source);
    if choice.copy && suffix.is_none() {
        session.status = "Copy is not supported for this source type".into();
        return;
    }
    let name = entry
        .name
        .to_string_lossy()
        .strip_suffix(suffix.unwrap_or(""))
        .unwrap()
        .to_owned();
    let source_path = entry.path.clone();
    let (drafts, complete) = super::inspection::draft_inventory(&catalog, &session);
    let guard = IoGuard::capture(&catalog, &session);
    let mut prepared = catalog.clone();
    io::enqueue(&mut commands, guard.clone(), move || {
        enum Plan {
            Move(aestra_project::content::operations::AssetMoveBatchPlan),
            Copy(aestra_project::content::operations::DuplicatePlan),
        }
        let plan = if choice.copy {
            prepared
                .content()
                .plan_saved_asset_duplicate(
                    OperationRequest::Duplicate {
                        source: choice.source,
                        parent: choice.parent,
                        name,
                    },
                    &drafts,
                    complete,
                )
                .map(Plan::Copy)
        } else {
            prepared
                .content()
                .plan_content_relocations(
                    vec![OperationRequest::Move {
                        source: choice.source,
                        parent: choice.parent,
                    }],
                    &drafts,
                    complete,
                )
                .map(Plan::Move)
        };
        io::completion(move |world| {
            if !guard.matches_material_reload(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) {
                io::set_status(world, "project-operation-queued-cancelled");
                return;
            }
            let result = plan.and_then(|plan| match plan {
                Plan::Move(plan) => plan.apply().map(|result| {
                    let (path, warning) =
                        super::relocation::publish(world, prepared.clone(), result, &source_path);
                    (None, path, warning)
                }),
                Plan::Copy(plan) => plan
                    .apply()
                    .map(|result| (Some(result.asset), result.created_source, None)),
            });
            match result {
                Ok((asset, path, warning)) => {
                    if let Some(asset) = asset {
                        prepared.refresh();
                        if let Ok(entry) = prepared.content().unique_source_for_asset(asset)
                            && let Some(mut state) =
                                world.get_resource_mut::<super::AssetBrowserState>()
                        {
                            state.reconcile(prepared.content(), prepared.content_revision());
                            state.locate(prepared.content(), entry.id);
                        }
                        io::publish_catalog(world, prepared);
                    }
                    let mut status = format!(
                        "{}: {}",
                        if choice.copy {
                            "Copied saved asset"
                        } else {
                            "Moved asset"
                        },
                        path.display()
                    );
                    if let Some(warning) = warning {
                        status.push_str(&format!("\n{warning}"));
                    }
                    world.resource_mut::<EditorSession>().status = status;
                }
                Err(error) => {
                    super::relocation::failed(world);
                    world.resource_mut::<EditorSession>().status =
                        format!("Asset operation blocked: {error}")
                }
            }
        })
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        camera::NormalizedRenderTarget,
        picking::{
            backend::HitData,
            pointer::{Location, PointerId},
        },
    };

    fn location() -> Location {
        Location {
            target: NormalizedRenderTarget::None {
                width: 800,
                height: 600,
            },
            position: Vec2::ZERO,
        }
    }

    #[test]
    fn legacy_recovery_defers_pending_batches_to_explicit_dialog() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(".aestra/asset-transactions");
        std::fs::create_dir_all(&directory).unwrap();
        let journal = directory.join("active.pending");
        std::fs::write(&journal, b"unrecognized journal; never replay").unwrap();
        let original = root.path().join("effect.aestra.ron");
        EffectAsset::new("Effect", 1.0).save_ron(&original).unwrap();
        let bytes = std::fs::read(&original).unwrap();
        let mut app = App::new();
        app.insert_resource(ProjectEffectCatalog::scan(root.path()))
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_systems(Update, recover_on_project_open);
        app.update();
        io::drain(app.world_mut());
        assert_eq!(std::fs::read(original).unwrap(), bytes);
        assert_eq!(
            std::fs::read(journal).unwrap(),
            b"unrecognized journal; never replay"
        );
        assert!(!root.path().join(".aestra/asset-moves").exists());
    }

    #[test]
    fn drop_preflight_rejects_stale_colliding_same_folder_and_recursive_targets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("destination")).unwrap();
        EffectAsset::new("Effect", 1.0)
            .save_ron(root.path().join("effect.aestra.ron"))
            .unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let source = catalog
            .content()
            .source_tree()
            .at_relative_path(std::path::Path::new("effect.aestra.ron"))
            .unwrap()
            .id;
        let parent = catalog
            .content()
            .source_tree()
            .at_relative_path(std::path::Path::new("destination"))
            .unwrap()
            .id;
        let version = catalog.content_revision();
        assert!(eligible(&catalog, parent, parent, version).is_err());
        assert!(eligible(&catalog, source, parent, version).is_ok());
        assert!(
            eligible(
                &catalog,
                source,
                catalog.content().source_tree().root(),
                version
            )
            .is_err()
        );
        assert!(
            eligible(
                &catalog,
                parent,
                catalog.content().source_tree().root(),
                version
            )
            .is_err()
        );
        assert!(
            eligible(
                &catalog,
                source,
                parent,
                ProjectContentVersion {
                    revision: version.revision + 1,
                    ..version
                }
            )
            .is_err()
        );
        std::fs::write(
            root.path().join("destination/effect.aestra.ron"),
            "collision",
        )
        .unwrap();
        assert!(eligible(&catalog, source, parent, version).is_err());
    }

    #[test]
    fn folder_tree_and_content_drops_offer_move_copy_and_escape_cancels() {
        for kind in 0..3 {
            for tree in [false, true] {
                for view in [
                    super::super::state::ViewMode::List,
                    super::super::state::ViewMode::Grid,
                ] {
                    let root = tempfile::tempdir().unwrap();
                    std::fs::create_dir(root.path().join("destination")).unwrap();
                    let filename = ["effect.aestra.ron", "texture.png", "pack"][kind];
                    let original = root.path().join(filename);
                    let effect = EffectAsset::new("Effect", 1.0);
                    match kind {
                        0 => effect.save_ron(&original).unwrap(),
                        1 => std::fs::write(&original, b"texture").unwrap(),
                        _ => std::fs::create_dir(&original).unwrap(),
                    }
                    let mut app = super::super::tests::browser_app(root.path());
                    app.world_mut()
                        .resource_mut::<super::super::AssetBrowserState>()
                        .view = view;
                    app.update();
                    let catalog = app.world().resource::<ProjectEffectCatalog>();
                    let source = catalog
                        .content()
                        .source_tree()
                        .at_relative_path(filename)
                        .unwrap()
                        .id;
                    let destination = catalog
                        .content()
                        .source_tree()
                        .at_relative_path(std::path::Path::new("destination"))
                        .unwrap()
                        .id;
                    let rows = super::super::tests::rows(&mut app);
                    let source_row = rows[&source];
                    let target = if tree {
                        let world = app.world_mut();
                        world
                            .query_filtered::<(Entity, &BrowserAction), With<BrowserFolderButton>>()
                            .iter(world)
                            .find_map(|(entity, action)| {
                                (*action == BrowserAction::Navigate(destination)).then_some(entity)
                            })
                            .unwrap()
                    } else {
                        rows[&destination]
                    };
                    app.world_mut().trigger(Pointer::new(
                        PointerId::Mouse,
                        location(),
                        DragStart {
                            button: PointerButton::Primary,
                            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                        },
                        source_row,
                    ));
                    app.world_mut().trigger(Pointer::new(
                        PointerId::Mouse,
                        location(),
                        DragEnter {
                            button: PointerButton::Primary,
                            dragged: source_row,
                            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                        },
                        target,
                    ));
                    app.world_mut().flush();
                    assert_eq!(
                        app.world_mut()
                            .query::<&DropHighlight>()
                            .iter(app.world())
                            .count(),
                        1
                    );
                    app.world_mut().trigger(Pointer::new(
                        PointerId::Mouse,
                        location(),
                        DragDrop {
                            button: PointerButton::Primary,
                            dropped: source_row,
                            hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                        },
                        target,
                    ));
                    app.world_mut().flush();
                    let choices: Vec<_> = app
                        .world_mut()
                        .query::<&DropChoice>()
                        .iter(app.world())
                        .map(|choice| choice.copy)
                        .collect();
                    assert_eq!(choices.len(), if kind == 0 { 2 } else { 1 });
                    assert_eq!(choices.contains(&true), kind == 0);
                    assert!(choices.contains(&false));
                    assert!(original.exists(), "A drop only opens a menu");
                    app.init_resource::<ButtonInput<KeyCode>>();
                    app.world_mut()
                        .resource_mut::<ButtonInput<KeyCode>>()
                        .press(KeyCode::Escape);
                    app.update();
                    assert_eq!(
                        app.world_mut()
                            .query::<&DropChoice>()
                            .iter(app.world())
                            .count(),
                        0
                    );
                    assert!(original.exists());
                }
            }
        }
    }

    #[test]
    fn folder_and_resource_moves_reconcile_active_effect_and_reject_queued_drafts() {
        for folder in [false, true] {
            for edit in [false, true] {
                let root = tempfile::tempdir().unwrap();
                std::fs::create_dir_all(root.path().join("pack/empty")).unwrap();
                std::fs::create_dir(root.path().join("destination")).unwrap();
                std::fs::write(root.path().join("pack/texture.png"), b"texture").unwrap();
                let mut effect = crate::test_support::effect_with_timing_slack();
                effect.assets.push(aestra_core::AssetDefinition {
                    id: aestra_core::AssetId::from_u128(12345),
                    name: "Texture".into(),
                    kind: aestra_core::AssetKind::Texture,
                    path: "pack/texture.png".into(),
                });
                // Resource move rewrites a referencing effect outside the moved source.
                let effect_path = root.path().join(if folder {
                    "pack/effect.aestra.ron"
                } else {
                    "effect.aestra.ron"
                });
                effect.save_ron(&effect_path).unwrap();
                let mut session = crate::test_support::session_with_timing_slack();
                session.open(&effect_path).unwrap();
                let catalog = ProjectEffectCatalog::scan(root.path());
                let source_path = if folder { "pack" } else { "pack/texture.png" };
                let source = catalog
                    .content()
                    .source_tree()
                    .at_relative_path(source_path)
                    .unwrap()
                    .id;
                let choice = DropChoice {
                    source,
                    parent: catalog
                        .content()
                        .source_tree()
                        .at_relative_path("destination")
                        .unwrap()
                        .id,
                    version: catalog.content_revision(),
                    copy: false,
                };
                let mut state = super::super::AssetBrowserState::default();
                state.reconcile(catalog.content(), catalog.content_revision());
                state.inspected = Some(source);
                let mut app = App::new();
                app.insert_resource(catalog)
                    .insert_resource(session)
                    .insert_resource(state)
                    .insert_resource(Localizer::new("en-US").unwrap())
                    .add_observer(choose);
                let button = app.world_mut().spawn(choice).id();
                app.world_mut().trigger(Activate { entity: button });
                let mut completion = io::prepared_completion(app.world_mut());
                if edit {
                    app.world_mut().resource_mut::<EditorSession>().execute(
                        "Edit",
                        aestra_authoring::EffectCommand::SetEffectName {
                            name: "Unsaved".into(),
                        },
                        false,
                    );
                }
                completion.apply(app.world_mut());
                let moved = if folder {
                    "destination/pack"
                } else {
                    "destination/texture.png"
                };
                assert_eq!(root.path().join(moved).exists(), !edit);
                assert_eq!(root.path().join(source_path).exists(), edit);
                if edit {
                    continue;
                }
                let session = app.world().resource::<EditorSession>();
                assert_eq!(session.effect.id, effect.id);
                assert_eq!(session.effect.assets[0].id, effect.assets[0].id);
                assert_eq!(
                    session.effect.assets[0].path,
                    if folder {
                        "destination/pack/texture.png"
                    } else {
                        "destination/texture.png"
                    }
                );
                assert_eq!(
                    session.source_path.as_ref().unwrap(),
                    &if folder {
                        root.path().join("destination/pack/effect.aestra.ron")
                    } else {
                        effect_path.clone()
                    }
                );
                let catalog = app.world().resource::<ProjectEffectCatalog>();
                let moved_id = catalog
                    .content()
                    .source_tree()
                    .at_relative_path(moved)
                    .unwrap()
                    .id;
                let state = app.world().resource::<super::super::AssetBrowserState>();
                assert_eq!(state.selected, Some(moved_id));
                assert_eq!(state.inspected, Some(moved_id));
                if folder {
                    assert!(root.path().join("destination/pack/empty").is_dir());
                }
                app.world_mut()
                    .resource_mut::<EditorSession>()
                    .save()
                    .unwrap();
                assert!(
                    !root
                        .path()
                        .join(".aestra/asset-transactions/active.pending")
                        .exists()
                );
            }
        }
    }

    #[test]
    fn move_retargets_open_effect_copy_has_new_identity_and_queued_edits_cancel() {
        for copy in [false, true] {
            for edit in [false, true] {
                let root = tempfile::tempdir().unwrap();
                std::fs::create_dir(root.path().join("destination")).unwrap();
                let original = root.path().join("effect.aestra.ron");
                let destination = root.path().join("destination/effect.aestra.ron");
                let effect = EffectAsset::new("Authored", 1.0);
                let bytes = format!("// retained comment\n{}", effect.to_pretty_ron().unwrap());
                std::fs::write(&original, &bytes).unwrap();
                let mut session = crate::test_support::session_with_timing_slack();
                session.open(&original).unwrap();
                session.seek_time(0.5);
                session.playing = false;
                let clock = session.clock;
                let catalog = ProjectEffectCatalog::scan(root.path());
                let choice = DropChoice {
                    source: catalog
                        .content()
                        .unique_source_for_asset(aestra_project::ProjectAssetId::Effect(effect.id))
                        .unwrap()
                        .id,
                    parent: catalog
                        .content()
                        .source_tree()
                        .at_relative_path(std::path::Path::new("destination"))
                        .unwrap()
                        .id,
                    version: catalog.content_revision(),
                    copy,
                };
                let mut app = App::new();
                app.insert_resource(catalog)
                    .insert_resource(session)
                    .insert_resource(Localizer::new("en-US").unwrap())
                    .add_observer(choose);
                let button = app.world_mut().spawn(choice).id();
                app.world_mut().trigger(Activate { entity: button });
                let mut completion = io::prepared_completion(app.world_mut());
                if edit {
                    app.world_mut().resource_mut::<EditorSession>().execute(
                        "Edit",
                        aestra_authoring::EffectCommand::SetEffectName {
                            name: "New draft".into(),
                        },
                        false,
                    );
                }
                completion.apply(app.world_mut());
                assert_eq!(destination.exists(), !edit);
                assert_eq!(original.exists(), copy || edit);
                let session = app.world().resource::<EditorSession>();
                assert_eq!(session.clock, clock);
                assert_eq!(
                    session.source_path.as_ref().unwrap(),
                    if copy || edit {
                        &original
                    } else {
                        &destination
                    }
                );
                if !edit {
                    let loaded = EffectAsset::load_ron(&destination).unwrap();
                    assert_eq!(loaded.id == effect.id, !copy);
                    assert_eq!(loaded.name, effect.name);
                    if !copy {
                        assert_eq!(std::fs::read(&destination).unwrap(), bytes.as_bytes());
                        app.world_mut()
                            .resource_mut::<EditorSession>()
                            .save()
                            .unwrap();
                    }
                }
            }
        }
    }
}
