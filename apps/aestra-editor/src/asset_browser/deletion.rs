//! Explicit Delete/Restore coordinator. Preparation is read-only; publication uses
//! the serialized queue and rechecks the captured document/draft state.
use crate::{
    project_content::io::{self, IoGuard},
    *,
};
use aestra_project::{
    ProjectSourceId,
    content::operations::{DeletePlan, DeletedSource},
};
use bevy::input_focus::{FocusCause, InputFocus, tab_navigation::TabGroup};
use bevy::ui_widgets::Activate;
mod documents;
#[cfg(test)]
mod tests;
use documents::ClosedDocuments;

#[derive(Event)]
pub(super) struct Open(pub Option<ProjectSourceId>);
#[derive(Resource, Default)]
struct State {
    open: bool,
    busy: bool,
    generation: u64,
    page: usize,
    source: Option<ProjectSourceId>,
    guard: Option<IoGuard>,
    entries: Vec<DeletedSource>,
    message: String,
}
#[derive(Component, Clone, Copy)]
enum Choice {
    Cancel,
    Refresh,
    Restore(usize),
    Page(bool),
}
#[derive(Component)]
struct Overlay;
#[derive(Component)]
struct Body;

pub(super) fn register(app: &mut App) {
    app.init_resource::<State>()
        .init_resource::<crate::history::asset_order::AssetOrder>()
        .init_resource::<DocumentProtectionState>()
        .add_observer(open)
        .add_observer(choose)
        .add_systems(Update, sync);
}

/// Opening a source is not a usage; references from a surviving effect still block.
fn active_block(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    source: ProjectSourceId,
) -> bool {
    let Some(entry) = catalog.content().source(source) else {
        return true;
    };
    let Ok(selected_path) = entry.path.canonicalize() else {
        return true;
    };
    if session
        .source_path
        .as_ref()
        .and_then(|path| path.canonicalize().ok())
        .is_some_and(|path| path.starts_with(&selected_path))
    {
        return false;
    }
    let ids: Vec<_> = catalog
        .content()
        .source_tree()
        .entries()
        .filter(|e| e.relative_path.starts_with(&entry.relative_path))
        .filter_map(|e| catalog.content().asset_for_source(e.id))
        .collect();
    let used_by_effect = |effect: &aestra_core::EffectAsset| {
        effect
            .effect_clips
            .iter()
            .any(|clip| ids.contains(&clip.source.into()))
            || effect
                .material_instances
                .iter()
                .any(|instance| match instance.program {
                    aestra_core::material::MaterialProgramRef::Project(id) => {
                        ids.contains(&aestra_project::ProjectAssetId::MaterialProgram(id))
                    }
                    _ => false,
                })
            || effect.assets.iter().any(|asset| {
                let file = asset
                    .path
                    .rsplit_once('#')
                    .map_or(asset.path.as_str(), |(file, _)| file);
                catalog
                    .root()
                    .join(file.replace('\\', "/"))
                    .canonicalize()
                    .ok()
                    .is_some_and(|path| path.starts_with(&selected_path))
            })
    };
    used_by_effect(&session.effect)
        || session
            .pending_change
            .as_ref()
            .is_some_and(|pending| used_by_effect(pending.preview.candidate()))
}

#[allow(clippy::too_many_arguments)]
fn open(
    event: On<Open>,
    mut state: ResMut<State>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    localizer: Res<Localizer>,
    mut commands: Commands,
) {
    if !io::idle(tasks) || protection.is_open() && !state.open {
        return;
    }
    let generation = state.generation.wrapping_add(1);
    *state = State {
        open: event.0.is_none(),
        busy: true,
        source: event.0,
        generation,
        message: localizer.text("browser-delete-checking"),
        ..default()
    };
    protection.asset_delete_open = state.open;
    let source = event.0;
    let blocked = source.is_some_and(|source| active_block(&catalog, &session, source));
    let active_error = localizer.text("browser-delete-active");
    let closed = source.map_or_else(
        || Ok(ClosedDocuments::default()),
        |source| ClosedDocuments::capture(&catalog, &session, source),
    );
    let for_checks = closed.as_ref().map_or_else(
        |_| catalog.clone(),
        |closed| closed.without_drafts(&catalog),
    );
    let (drafts, complete) = super::inspection::deletion_draft_inventory(&for_checks, &session);
    let prepared = catalog.clone();
    let guard = IoGuard::capture(&catalog, &session);
    io::enqueue(&mut commands, guard.clone(), move || {
        let result = if let Some(source) = source {
            if blocked {
                Err(active_error)
            } else if let Err(error) = &closed {
                Err(error.clone())
            } else {
                prepared
                    .content()
                    .plan_delete_source(source, &drafts, complete)
                    .map_err(|e| e.to_string())
                    .and_then(|plan| {
                        closed
                            .as_ref()
                            .unwrap()
                            .encode()
                            .map(|data| (Some(plan.with_recovery_data(data)), Vec::new()))
                    })
            }
        } else {
            prepared
                .content()
                .deleted_sources()
                .map(|items| (None, items))
                .map_err(|e| e.to_string())
        };
        io::completion(move |world| {
            if world.resource::<State>().generation != generation {
                return;
            }
            let valid = guard.matches(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            );
            if !valid {
                show_error(world, "Project or document changed; try again.".into());
                return;
            }
            world.resource_mut::<State>().guard = Some(guard);
            match result {
                Ok((Some(plan), _)) => match plan.apply() {
                    Ok(item) => {
                        crate::history::asset_order::record_delete(world, item.clone());
                        closed.as_ref().unwrap().close(world);
                        publish_change(world, prepared, &item.original, true);
                    }
                    Err(error) => show_error(world, error.to_string()),
                },
                Ok((plan, entries)) => {
                    debug_assert!(plan.is_none());
                    let mut state = world.resource_mut::<State>();
                    state.busy = false;
                    state.entries = entries;
                    state.message.clear();
                }
                Err(error) => show_error(world, error),
            }
        })
    });
}

#[allow(clippy::too_many_arguments)]
fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut state: ResMut<State>,
    catalog: Res<ProjectEffectCatalog>,
    session: Res<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    mut commands: Commands,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if !state.open || state.busy || !io::idle(tasks) {
        return;
    }
    match *choice {
        Choice::Cancel => {
            state.open = false;
            protection.asset_delete_open = false;
            return;
        }
        Choice::Page(next) => {
            state.page = if next {
                (state.page + 1).min(state.entries.len().saturating_sub(1) / 8)
            } else {
                state.page.saturating_sub(1)
            };
            return;
        }
        Choice::Refresh => {
            commands.trigger(Open(state.source));
            return;
        }
        _ => {}
    }
    let (drafts, complete) = super::inspection::deletion_draft_inventory(&catalog, &session);
    if !complete
        || !state
            .guard
            .as_ref()
            .is_some_and(|guard| guard.matches(&catalog, &session))
    {
        state.message = "Project or drafts changed. Save/discard drafts, then check again.".into();
        return;
    }
    let item = match *choice {
        Choice::Restore(index) => {
            let Some(entry) = state.entries.get(index) else {
                return;
            };
            entry.clone()
        }
        _ => return,
    };
    let closed = match ClosedDocuments::read(&item).and_then(|closed| {
        closed.validate_restore(&catalog, &item.original)?;
        Ok(closed)
    }) {
        Ok(closed) => closed,
        Err(error) => {
            state.message = error;
            return;
        }
    };
    state.busy = true;
    let generation = state.generation;
    let guard = IoGuard::capture(&catalog, &session);
    let prepared = catalog.clone();
    io::enqueue(&mut commands, guard.clone(), move || {
        io::completion(move |world| {
            let valid = world.resource::<State>().generation == generation
                && world
                    .resource::<DocumentProtectionState>()
                    .asset_delete_open
                && guard.matches(
                    world.resource::<ProjectEffectCatalog>(),
                    world.resource::<EditorSession>(),
                );
            if !valid {
                let mut state = world.resource_mut::<State>();
                state.busy = false;
                state.message = "Operation cancelled: project or document changed.".into();
                return;
            }
            match item.clone().restore(&drafts, complete) {
                Ok(_) => {
                    world
                        .resource_mut::<crate::history::asset_order::AssetOrder>()
                        .forget_restored(item.journal());
                    crate::history::asset_order::clear_redo(world);
                    closed.restore(world);
                    publish_change(world, prepared, &item.original, false);
                }
                Err(error) => show_error(world, error.to_string()),
            }
        })
    });
}

fn show_error(world: &mut World, message: String) {
    let mut state = world.resource_mut::<State>();
    state.open = true;
    state.busy = false;
    state.message = message.clone();
    world
        .resource_mut::<DocumentProtectionState>()
        .asset_delete_open = true;
    world.resource_mut::<EditorSession>().status = message;
}

fn publish_change(
    world: &mut World,
    mut prepared: ProjectEffectCatalog,
    relative: &std::path::Path,
    deleting: bool,
) {
    prepared.refresh();
    if let Some(mut browser) = world.get_resource_mut::<super::AssetBrowserState>() {
        browser.reconcile(prepared.content(), prepared.content_revision());
        if browser
            .inspected
            .is_some_and(|id| prepared.content().source(id).is_none())
        {
            browser.inspected = None;
        }
        if !deleting
            && let Some(entry) = prepared.content().source_tree().at_relative_path(relative)
        {
            browser.locate(prepared.content(), entry.id);
        }
    }
    io::publish_catalog(world, prepared);
    let mut state = world.resource_mut::<State>();
    state.open = false;
    state.busy = false;
    state.message.clear();
    world
        .resource_mut::<DocumentProtectionState>()
        .asset_delete_open = false;
    world.resource_mut::<EditorSession>().status = format!(
        "{}: {}",
        world.resource::<Localizer>().text(if deleting {
            "browser-delete-status"
        } else {
            "browser-restore-status"
        }),
        relative.display()
    );
}

/// Undo/Redo keeps its stack entry until guarded, serialized publication succeeds.
pub(crate) fn history_step(world: &mut World, item: DeletedSource, undo: bool, revision: u64) {
    use crate::history::asset_order::AssetOrder;
    if !io::idle_world(world) {
        return;
    }
    // Callers queue this after the observer; compare ordering again before capturing I/O.
    world.resource_scope(|world, mut order: Mut<AssetOrder>| {
        order.sync(
            world.resource::<EditorSession>(),
            world.resource::<ProjectEffectCatalog>(),
        );
    });
    if !world
        .resource::<AssetOrder>()
        .matches_delete(undo, revision)
    {
        return;
    }
    if world.resource::<DocumentProtectionState>().is_open() {
        return;
    }
    let catalog = world.resource::<ProjectEffectCatalog>();
    let session = world.resource::<EditorSession>();
    let closed = match ClosedDocuments::read(&item).and_then(|closed| {
        if undo {
            closed.validate_restore(catalog, &item.original)?;
        } else {
            let source = catalog
                .content()
                .source_tree()
                .at_relative_path(&item.original)
                .ok_or("Deleted source disappeared")?
                .id;
            let current = ClosedDocuments::capture(catalog, session, source)?;
            closed.validate_redo(&current)?;
            return Ok(current);
        }
        Ok(closed)
    }) {
        Ok(closed) => closed,
        Err(error) => {
            world.resource_mut::<EditorSession>().status = error;
            return;
        }
    };
    let for_checks = if undo {
        catalog.clone()
    } else {
        closed.without_drafts(catalog)
    };
    let (drafts, complete) = super::inspection::deletion_draft_inventory(&for_checks, session);
    if !complete {
        world.resource_mut::<EditorSession>().status =
            "Asset Undo/Redo blocked: draft inventory is incomplete".into();
        return;
    }
    if !undo
        && catalog
            .content()
            .source_tree()
            .at_relative_path(&item.original)
            .is_none_or(|source| active_block(catalog, session, source.id))
    {
        world.resource_mut::<EditorSession>().status =
            "Redo deletion blocked: source is missing or referenced by the active document".into();
        return;
    }
    let prepared = catalog.clone();
    let guard = IoGuard::capture(catalog, session);
    io::enqueue(&mut world.commands(), guard.clone(), move || {
        let planned: Result<Option<DeletePlan>, String> = if undo {
            Ok(None)
        } else {
            item.plan_redo(&drafts, complete)
                .map(Some)
                .map_err(|error| error.to_string())
        };
        io::completion(move |world| {
            if !guard.matches(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) || !world
                .resource::<AssetOrder>()
                .matches_delete(undo, revision)
                || world.resource::<DocumentProtectionState>().is_open()
            {
                world.resource_mut::<EditorSession>().status =
                    "Asset Undo/Redo cancelled: project or document changed".into();
                return;
            }
            let result = planned.and_then(|plan| match plan {
                Some(plan) => plan.apply().map_err(|e| e.to_string()),
                None => item
                    .clone()
                    .restore(&drafts, complete)
                    .map(|_| item)
                    .map_err(|e| e.to_string()),
            });
            match result {
                Ok(item) => {
                    let relative = item.original.clone();
                    world
                        .resource_mut::<AssetOrder>()
                        .finish_delete(undo, revision, item);
                    if undo {
                        closed.restore(world);
                    } else {
                        closed.close(world);
                    }
                    publish_change(world, prepared, &relative, !undo);
                }
                Err(error) => {
                    world.resource_mut::<EditorSession>().status =
                        format!("Asset Undo/Redo blocked: {error}");
                }
            }
        })
    });
}

pub(crate) fn spawn(parent: &mut ChildSpawnerCommands) {
    parent
        .spawn((
            Overlay,
            TabGroup::modal(),
            GlobalZIndex(310),
            Node {
                display: Display::None,
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.005, 0.007, 0.014, 0.82)),
        ))
        .with_children(|host| {
            host.spawn((
                Node {
                    width: Val::Px(600.0),
                    max_width: Val::Percent(92.0),
                    max_height: Val::Percent(90.0),
                    ..default()
                },
                BackgroundColor(theme::PANEL),
            ))
            .with_children(|panel| {
                let target = panel
                    .spawn((
                        Body,
                        bevy::ui_widgets::ScrollArea,
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            min_height: Val::Px(0.0),
                            max_height: Val::Vh(80.0),
                            overflow: Overflow::scroll_y(),
                            padding: UiRect::all(Val::Px(20.0)),
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(12.0),
                            ..default()
                        },
                    ))
                    .id();
                crate::feathers::scroll::spawn_vertical_scrollbar(panel, target);
            });
        });
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut state: ResMut<State>,
    mut protection: ResMut<DocumentProtectionState>,
    catalog: Res<ProjectEffectCatalog>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    localizer: Res<Localizer>,
    mut overlays: Query<&mut Node, With<Overlay>>,
    bodies: Query<Entity, With<Body>>,
    mut commands: Commands,
    mut focus: Option<ResMut<InputFocus>>,
    mut return_focus: Local<Option<Entity>>,
    mut was_open: Local<bool>,
) {
    if state.open
        && (!protection.asset_delete_open
            || state
                .guard
                .as_ref()
                .is_some_and(|guard| !guard.same_project(&catalog)))
    {
        state.open = false;
    }
    if state.open && !state.busy && keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape)) {
        state.open = false;
    }
    protection.asset_delete_open = state.open;
    if state.busy && io::idle(tasks) {
        state.busy = false;
        state.message = "Preparation cancelled. Check again.".into();
    }
    if !state.is_changed() && *was_open == state.open {
        return;
    }
    if state.open && !*was_open {
        *return_focus = focus.as_deref().and_then(|focus| focus.get());
    }
    if !state.open
        && *was_open
        && let Some(focus) = focus.as_deref_mut()
    {
        if let Some(entity) = return_focus
            .take()
            .filter(|entity| commands.get_entity(*entity).is_ok())
        {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
    *was_open = state.open;
    for mut node in &mut overlays {
        node.display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !state.open {
        return;
    }
    for body in &bodies {
        commands.entity(body).despawn_children();
        commands.entity(body).with_children(|host| {
            let source = state.source.and_then(|id| catalog.content().source(id));
            let folder = source
                .is_some_and(|source| source.kind == aestra_project::ProjectSourceKind::Directory);
            label(
                host,
                localizer.text(if folder {
                    "browser-delete-folder-title"
                } else if state.source.is_some() {
                    "browser-delete-title"
                } else {
                    "browser-deleted-items"
                }),
            );
            if let Some(source) = source {
                label(host, source.relative_path.display().to_string());
            }
            if state.source.is_none() {
                label(host, localizer.text("browser-deleted-description"));
            }
            if !state.message.is_empty() {
                label(host, state.message.clone());
            }
            if state.source.is_none() && state.entries.is_empty() && !state.busy {
                label(host, localizer.text("browser-deleted-empty"));
            }
            for (index, item) in state
                .entries
                .iter()
                .enumerate()
                .skip(state.page * 8)
                .take(8)
            {
                if let Some(reason) = &item.blocked_reason {
                    label(host, reason.clone());
                }
                button(
                    host,
                    &format!(
                        "{} — {}",
                        localizer.text("browser-delete-restore"),
                        item.original.display()
                    ),
                    Choice::Restore(index),
                    state.busy || item.blocked_reason.is_some(),
                );
            }
            if state.entries.len() > 8 {
                button(
                    host,
                    "←",
                    Choice::Page(false),
                    state.busy || state.page == 0,
                );
                button(
                    host,
                    "→",
                    Choice::Page(true),
                    state.busy || (state.page + 1) * 8 >= state.entries.len(),
                );
            }
            host.spawn(Node {
                justify_content: JustifyContent::End,
                flex_wrap: FlexWrap::Wrap,
                flex_shrink: 0.0,
                column_gap: Val::Px(8.0),
                row_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|actions| {
                if state.source.is_none() || (!state.busy && !state.message.is_empty()) {
                    button(
                        actions,
                        &localizer.text("browser-recovery-retry"),
                        Choice::Refresh,
                        state.busy,
                    );
                }
                let cancel = button(
                    actions,
                    &localizer.text("common-close"),
                    Choice::Cancel,
                    state.busy,
                );
                if let Some(focus) = focus.as_deref_mut() {
                    focus.set(cancel, FocusCause::Navigated);
                }
            });
        });
    }
}
fn label(host: &mut ChildSpawnerCommands, value: String) {
    host.spawn((
        Text::new(value),
        TextColor(theme::TEXT),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        Pickable::IGNORE,
        Node {
            flex_shrink: 0.0,
            ..default()
        },
    ));
}
fn button(host: &mut ChildSpawnerCommands, value: &str, choice: Choice, disabled: bool) -> Entity {
    let mut button = host.spawn_empty();
    button
        .apply_scene(ui_shell::feathers_button())
        .insert((
            choice,
            FeathersActionButton,
            TabIndex(0),
            AccessibleLabel(value.into()),
            Node {
                min_height: Val::Px(28.0),
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((Text::new(value), ThemedText, Pickable::IGNORE));
    if disabled {
        button.insert(InteractionDisabled);
    }
    button.id()
}
