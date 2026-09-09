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
#[cfg(test)]
mod tests;

#[derive(Event)]
pub(super) struct Open(pub Option<ProjectSourceId>);
#[derive(Resource, Default)]
struct State {
    open: bool,
    busy: bool,
    generation: u64,
    page: usize,
    source: Option<ProjectSourceId>,
    plan: Option<DeletePlan>,
    guard: Option<IoGuard>,
    entries: Vec<DeletedSource>,
    message: String,
}
#[derive(Component, Clone, Copy)]
enum Choice {
    Cancel,
    Confirm,
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
        .init_resource::<DocumentProtectionState>()
        .add_observer(open)
        .add_observer(choose)
        .add_systems(Update, sync);
}

/// Active documents are not silently closed or recreated by filesystem deletion.
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
        return true;
    }
    let ids: Vec<_> = catalog
        .content()
        .source_tree()
        .entries()
        .filter(|e| e.relative_path.starts_with(&entry.relative_path))
        .filter_map(|e| catalog.content().asset_for_source(e.id))
        .collect();
    if session
        .standalone_material()
        .is_some_and(|id| ids.contains(&aestra_project::ProjectAssetId::MaterialProgram(id)))
        || session
            .standalone_function()
            .is_some_and(|id| ids.contains(&aestra_project::ProjectAssetId::MaterialFunction(id)))
        || session
            .effect
            .effect_clips
            .iter()
            .any(|clip| ids.contains(&clip.source.into()))
        || session
            .effect
            .material_instances
            .iter()
            .any(|instance| match instance.program {
                aestra_core::material::MaterialProgramRef::Project(id) => {
                    ids.contains(&aestra_project::ProjectAssetId::MaterialProgram(id))
                }
                _ => false,
            })
    {
        return true;
    }
    session.effect.assets.iter().any(|asset| {
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
        open: true,
        busy: true,
        source: event.0,
        generation,
        message: localizer.text("browser-delete-checking"),
        ..default()
    };
    protection.asset_delete_open = true;
    let source = event.0;
    let blocked = source.is_some_and(|source| active_block(&catalog, &session, source));
    let active_error = localizer.text("browser-delete-active");
    let (drafts, complete) = super::inspection::draft_inventory(&catalog, &session);
    let prepared = catalog.clone();
    let guard = IoGuard::capture(&catalog, &session);
    io::enqueue(&mut commands, guard.clone(), move || {
        let result = if let Some(source) = source {
            if blocked {
                Err(active_error)
            } else {
                prepared
                    .content()
                    .plan_delete_source(source, &drafts, complete)
                    .map(|plan| (Some(plan), Vec::new()))
                    .map_err(|e| e.to_string())
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
            let mut state = world.resource_mut::<State>();
            state.busy = false;
            state.guard = Some(guard);
            if !valid {
                state.message = "Project or document changed; check again.".into();
                return;
            }
            match result {
                Ok((plan, entries)) => {
                    state.plan = plan;
                    state.entries = entries;
                    state.message.clear();
                }
                Err(error) => state.message = error,
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
    let (drafts, complete) = super::inspection::draft_inventory(&catalog, &session);
    if !complete
        || !drafts.is_empty()
        || !state
            .guard
            .as_ref()
            .is_some_and(|guard| guard.matches(&catalog, &session))
    {
        state.message = "Project or drafts changed. Save/discard drafts, then check again.".into();
        state.plan = None;
        return;
    }
    enum Operation {
        Delete(DeletePlan),
        Restore(DeletedSource),
    }
    let operation = match *choice {
        Choice::Confirm => {
            if state
                .source
                .is_none_or(|source| active_block(&catalog, &session, source))
            {
                return;
            }
            let Some(plan) = state.plan.take() else {
                return;
            };
            Operation::Delete(plan)
        }
        Choice::Restore(index) => {
            let Some(entry) = state.entries.get(index) else {
                return;
            };
            Operation::Restore(entry.clone())
        }
        _ => return,
    };
    state.busy = true;
    let generation = state.generation;
    let guard = IoGuard::capture(&catalog, &session);
    let mut prepared = catalog.clone();
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
                state.plan = None;
                state.message = "Operation cancelled: project or document changed.".into();
                return;
            }
            let deleting = matches!(&operation, Operation::Delete(_));
            let result = match operation {
                Operation::Delete(plan) => plan.apply().map(|item| item.original),
                Operation::Restore(item) => item.restore(&[], true),
            };
            let status = match result {
                Ok(path) => {
                    prepared.refresh();
                    if let Some(mut browser) = world.get_resource_mut::<super::AssetBrowserState>()
                    {
                        browser.reconcile(prepared.content(), prepared.content_revision());
                        if browser
                            .inspected
                            .is_some_and(|id| prepared.content().source(id).is_none())
                        {
                            browser.inspected = None;
                        }
                        if !deleting
                            && let Ok(relative) = path.strip_prefix(prepared.root())
                            && let Some(entry) =
                                prepared.content().source_tree().at_relative_path(relative)
                        {
                            browser.locate(prepared.content(), entry.id);
                        }
                    }
                    io::publish_catalog(world, prepared);
                    world.resource_mut::<State>().open = false;
                    world
                        .resource_mut::<DocumentProtectionState>()
                        .asset_delete_open = false;
                    format!(
                        "{}: {}",
                        if deleting {
                            "Moved to Deleted Items"
                        } else {
                            "Restored"
                        },
                        path.display()
                    )
                }
                Err(error) => format!("Operation blocked: {error}"),
            };
            let mut state = world.resource_mut::<State>();
            state.busy = false;
            state.message = status.clone();
            world.resource_mut::<EditorSession>().status = status;
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
            label(
                host,
                localizer.text(if state.source.is_some() {
                    "browser-delete-title"
                } else {
                    "browser-deleted-items"
                }),
            );
            label(host, localizer.text("browser-delete-description"));
            if let Some(plan) = &state.plan {
                label(
                    host,
                    format!(
                        "{}\n{} files · {} folders",
                        plan.original().display(),
                        plan.file_count(),
                        plan.folder_count()
                    ),
                );
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
            let cancel = button(
                host,
                &localizer.text("browser-folder-cancel"),
                Choice::Cancel,
                state.busy,
            );
            button(
                host,
                &localizer.text("browser-recovery-retry"),
                Choice::Refresh,
                state.busy,
            );
            if state.source.is_some() {
                button(
                    host,
                    &localizer.text("browser-delete-confirm"),
                    Choice::Confirm,
                    state.plan.is_none() || state.busy,
                );
            }
            if let Some(focus) = focus.as_deref_mut() {
                focus.set(cancel, FocusCause::Navigated);
            }
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
                padding: UiRect::all(Val::Px(5.0)),
                ..default()
            },
        ))
        .with_child((Text::new(value), ThemedText, Pickable::IGNORE));
    if disabled {
        button.insert(InteractionDisabled);
    }
    button.id()
}
