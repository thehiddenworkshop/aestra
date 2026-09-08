//! Folder creation and saved duplication use the project planner and serialized project I/O.
use super::AssetBrowserState;
use crate::project_content::io::{self, IoGuard};
use crate::*;
use aestra_project::{
    ProjectContentVersion, ProjectSourceId, content::operations::OperationRequest,
};
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::ui_widgets::{Activate, ValueChange};

#[derive(Event)]
pub(super) struct OpenFolderPrompt(
    pub Option<(ProjectSourceId, ProjectContentVersion)>,
    pub bool,
);
#[derive(Component)]
struct NameField;
#[derive(Component)]
struct PendingFocus;
#[derive(Component)]
struct NameError;
#[derive(Component)]
pub(super) struct InlineRenameEditor;

fn name_collision(prompt: &Prompt, catalog: &ProjectEffectCatalog) -> bool {
    let Some((parent, _)) = prompt.target else {
        return false;
    };
    let name = format!("{}{}", prompt.name, prompt.suffix).to_lowercase();
    if prompt.rename && prompt.name == prompt.original_name {
        return false;
    }
    catalog
        .content()
        .source_tree()
        .children(parent)
        .any(|entry| entry.name.to_string_lossy().to_lowercase() == name)
}

#[allow(clippy::too_many_arguments)]
fn sync_controls(
    mut prompt: ResMut<Prompt>,
    buttons: Query<(Entity, &Choice, Has<InteractionDisabled>)>,
    pending: Query<(Entity, &Children), With<PendingFocus>>,
    mut inputs: Query<
        &mut bevy::text::EditableText,
        With<bevy::feathers::controls::FeathersTextInput>,
    >,
    mut focus: Option<ResMut<InputFocus>>,
    mut commands: Commands,
    catalog: Option<Res<ProjectEffectCatalog>>,
    localizer: Option<Res<Localizer>>,
    mut errors: Query<(&mut Text, &mut Node), With<NameError>>,
    session: Option<Res<EditorSession>>,
) {
    let collision = catalog
        .as_ref()
        .is_some_and(|catalog| name_collision(&prompt, catalog));
    let draft_block = prompt.duplicate.is_some_and(|source| {
        let (Some(catalog), Some(session)) = (&catalog, &session) else {
            return true;
        };
        let (drafts, complete) = super::inspection::draft_inventory(catalog, session);
        !complete || drafts.iter().any(|(owner, _)| *owner == source)
    });
    for (mut text, mut node) in &mut errors {
        node.display = if prompt.inline_label.is_none()
            && (collision || draft_block || prompt.failure.is_some())
        {
            Display::Flex
        } else {
            Display::None
        };
        if let Some(localizer) = &localizer {
            text.0 = prompt.failure.clone().unwrap_or_else(|| {
                localizer.text(if draft_block {
                    if prompt.rename {
                        "browser-rename-save-first"
                    } else {
                        "browser-duplicate-save-first"
                    }
                } else {
                    "browser-folder-exists"
                })
            });
        }
        if let Some(wrapper) = prompt.overlay.filter(|_| prompt.inline_label.is_some()) {
            if collision || draft_block || prompt.failure.is_some() {
                commands.entity(wrapper).try_insert((
                    EditorTooltip::description(text.0.clone()),
                    Outline {
                        color: Color::srgb(1.0, 0.35, 0.3),
                        width: Val::Px(1.0),
                        offset: Val::Px(-1.0),
                    },
                ));
            } else {
                commands
                    .entity(wrapper)
                    .try_remove::<(EditorTooltip, Outline)>();
            }
        }
    }
    for (entity, choice, disabled) in &buttons {
        if !matches!(choice, Choice::Create) {
            continue;
        }
        let empty = prompt.name.trim().is_empty() || collision || draft_block || prompt.pending;
        if empty && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
        if !empty && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
    }
    for (entity, children) in &pending {
        if let Some(input) = children.iter().find(|child| inputs.contains(*child)) {
            prompt.input = Some(input);
            if prompt.inline_label.is_some()
                && let Ok(mut text) = inputs.get_mut(input)
            {
                text.queue_edit(bevy::text::TextEdit::SelectAll);
            }
            if let Some(focus) = focus.as_deref_mut() {
                focus.set(input, FocusCause::Navigated);
            }
            commands.entity(entity).remove::<PendingFocus>();
        }
    }
}

fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    mut prompt: ResMut<Prompt>,
    mut commands: Commands,
    inputs: Query<&bevy::text::EditableText>,
) {
    if prompt.inline_label.is_some()
        && event.input.state == ButtonState::Pressed
        && !event.input.repeat
        && event.input.key_code == KeyCode::Enter
        && Some(event.focused_entity) == prompt.input
        && let Ok(text) = inputs.get(event.focused_entity)
        && !text.is_composing()
    {
        event.propagate(false);
        prompt.submit_requested = true;
        return;
    }
    if prompt.overlay.is_some()
        && event.input.key_code == KeyCode::Escape
        && event.input.state == ButtonState::Pressed
    {
        event.propagate(false);
        close(&mut commands, &mut prompt);
    }
}
#[derive(Component, Clone, Copy)]
enum Choice {
    Create,
    Cancel,
}
#[derive(Resource, Default)]
struct Prompt {
    target: Option<(ProjectSourceId, ProjectContentVersion)>,
    overlay: Option<Entity>,
    name: String,
    duplicate: Option<ProjectSourceId>,
    suffix: String,
    rename: bool,
    pending: bool,
    failure: Option<String>,
    inline_label: Option<Entity>,
    inline_secondary: Option<Entity>,
    inline_row: Option<Entity>,
    return_focus: Option<Entity>,
    original_name: String,
    generation: u64,
    input: Option<Entity>,
    submit_requested: bool,
    blur_attempted: bool,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Prompt>()
        .add_observer(open)
        .add_observer(change)
        .add_observer(choose)
        .add_observer(keyboard)
        .add_observer(submit_inline_on_outside_press)
        .add_systems(Update, (escape, sync_controls, inline_lifecycle).chain())
        .add_systems(
            PostUpdate,
            submit_inline
                .after(bevy::text::EditableTextSystems)
                .after(bevy::input_focus::InputFocusSystems::FocusChangeEvents),
        );
}
#[allow(clippy::too_many_arguments)]
fn open(
    event: On<OpenFolderPrompt>,
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    state: Res<AssetBrowserState>,
    catalog: Res<ProjectEffectCatalog>,
    localizer: Res<Localizer>,
    rows: Query<(Entity, &super::panel::BrowserRow)>,
    labels: Query<(Entity, &ChildOf), With<crate::feathers::list_row::ListRowPrimaryLabel>>,
    parents: Query<&ChildOf>,
    lists: Query<(), With<super::panel::BrowserItems>>,
    texts: Query<(Entity, &ChildOf), With<Text>>,
) {
    if prompt.overlay.is_some() {
        return;
    }
    prompt.generation = prompt.generation.wrapping_add(1);
    prompt.input = None;
    prompt.submit_requested = false;
    prompt.blur_attempted = false;
    prompt.duplicate = None;
    prompt.pending = false;
    prompt.failure = None;
    prompt.inline_label = None;
    prompt.inline_row = None;
    prompt.return_focus = None;
    prompt.rename = event.1;
    prompt.suffix.clear();
    prompt.target = Some((
        state.folder_id(catalog.content()),
        catalog.content_revision(),
    ));
    prompt.name.clear();
    if let Some((source, version)) = event.0 {
        if version != catalog.content_revision() {
            return;
        }
        let Some(entry) = catalog.content().source(source) else {
            return;
        };
        let suffix = match catalog.content().asset_for_source(source) {
            Some(aestra_project::ProjectAssetId::MaterialProgram(_)) => ".aestra.material.ron",
            Some(aestra_project::ProjectAssetId::MaterialFunction(_)) => {
                ".aestra.material-function.ron"
            }
            _ => return,
        };
        prompt.target = Some((
            entry
                .parent
                .unwrap_or(catalog.content().source_tree().root()),
            version,
        ));
        prompt.duplicate = Some(source);
        prompt.suffix = suffix.into();
        if prompt.rename {
            prompt.name = entry
                .name
                .to_string_lossy()
                .strip_suffix(suffix)
                .unwrap_or_default()
                .into();
        }
    }
    let title = if prompt.rename {
        "browser-rename"
    } else if prompt.duplicate.is_some() {
        "browser-duplicate"
    } else {
        "browser-new-folder"
    };
    let label = if prompt.rename {
        "browser-rename-name"
    } else if prompt.duplicate.is_some() {
        "browser-duplicate-name"
    } else {
        "browser-folder-name"
    };
    prompt.original_name = prompt.name.clone();
    if prompt.rename {
        let Some((row, _)) = rows.iter().find(|(_, row)| Some(row.0) == prompt.duplicate) else {
            return;
        };
        let Some((caption, parent)) = labels.iter().find(|(label, _)| {
            parents
                .iter_ancestors(*label)
                .any(|ancestor| ancestor == row)
        }) else {
            return;
        };
        let mut wrapper = Entity::PLACEHOLDER;
        let secondary = texts
            .iter()
            .find(|(entity, owner)| *entity != caption && owner.parent() == parent.parent())
            .map(|(entity, _)| entity);
        commands.entity(parent.parent()).with_children(|host| {
            wrapper = host
                .spawn((
                    InlineRenameEditor,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ))
                .with_children(|host| {
                    let field = crate::feathers::text_input::spawn_text_input(
                        host,
                        &prompt.name,
                        &localizer.text("browser-rename"),
                        NameField,
                    );
                    host.commands().entity(field).insert(PendingFocus);
                    host.commands()
                        .entity(field)
                        .entry::<Node>()
                        .and_modify(|mut node| {
                            node.height = Val::Px(22.0);
                            node.min_width = Val::Px(0.0);
                            node.width = Val::Percent(100.0);
                            node.flex_shrink = 0.0;
                        });
                    host.spawn((
                        NameError,
                        Text::default(),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 10.0.into(),
                            ..default()
                        },
                        TextLayout::no_wrap(),
                        Node {
                            display: Display::None,
                            max_height: Val::Px(16.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                    ));
                    host.spawn((
                        Choice::Create,
                        Node {
                            display: Display::None,
                            ..default()
                        },
                    ));
                })
                .id();
        });
        commands
            .entity(caption)
            .entry::<Node>()
            .and_modify(|mut node| node.display = Display::None);
        prompt.inline_label = Some(caption);
        if let Some(secondary) = secondary {
            commands
                .entity(secondary)
                .entry::<Node>()
                .and_modify(|mut node| node.display = Display::None);
        }
        prompt.inline_secondary = secondary;
        prompt.inline_row = Some(row);
        prompt.return_focus = parents
            .iter_ancestors(row)
            .find(|entity| lists.contains(*entity));
        prompt.overlay = Some(wrapper);
        return;
    }
    let overlay = commands
        .spawn((
            TabGroup::modal(),
            super::panel::BrowserSurface,
            crate::feathers::node_graph::FeathersGraphNavigationBlocker,
            RelativeCursorPosition::default(),
            GlobalZIndex(320),
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: Val::Px(400.0),
                        max_width: Val::Percent(95.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(12.0),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(localizer.text(title)),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 16.0.into(),
                            ..default()
                        },
                    ));
                    panel.spawn((
                        Text::new(localizer.text(label)),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 13.0.into(),
                            ..default()
                        },
                    ));
                    let field = crate::feathers::text_input::spawn_text_input(
                        panel,
                        &prompt.name,
                        &localizer.text(label),
                        NameField,
                    );
                    panel.commands().entity(field).insert(PendingFocus);
                    panel.spawn((
                        NameError,
                        Text::new(localizer.text("browser-folder-exists")),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 13.0.into(),
                            ..default()
                        },
                        Node {
                            display: Display::None,
                            ..default()
                        },
                    ));
                    crate::feathers::button::spawn_action_button(
                        panel,
                        &localizer.text(if prompt.rename {
                            "browser-rename"
                        } else if prompt.duplicate.is_some() {
                            "browser-duplicate"
                        } else {
                            "browser-folder-create"
                        }),
                        Choice::Create,
                        false,
                    );
                    crate::feathers::button::spawn_action_button(
                        panel,
                        &localizer.text("browser-folder-cancel"),
                        Choice::Cancel,
                        false,
                    );
                });
        })
        .id();
    prompt.overlay = Some(overlay);
}
fn change(
    event: On<ValueChange<String>>,
    fields: Query<(), With<NameField>>,
    mut prompt: ResMut<Prompt>,
) {
    if fields.contains(event.source) {
        prompt.name.clone_from(&event.value);
        prompt.failure = None;
    }
}
fn close(commands: &mut Commands, prompt: &mut Prompt) {
    for label in [prompt.inline_label.take(), prompt.inline_secondary.take()]
        .into_iter()
        .flatten()
    {
        commands.queue(move |world: &mut World| {
            if let Some(mut node) = world.get_mut::<Node>(label) {
                node.display = Display::Flex;
            }
        });
    }
    if let Some(list) = prompt.return_focus.take() {
        let row = prompt.inline_row;
        commands.queue(move |world: &mut World| {
            if world.get_entity(list).is_ok()
                && let Some(row) = row.filter(|row| world.get_entity(*row).is_ok())
            {
                world
                    .entity_mut(list)
                    .insert(bevy::ui_widgets::ActiveDescendant(Some(row)));
            }
            if world.get_entity(list).is_ok()
                && let Some(mut focus) = world.get_resource_mut::<InputFocus>()
            {
                focus.set(list, FocusCause::Navigated);
            }
        });
    }
    if let Some(entity) = prompt.overlay.take() {
        commands.entity(entity).try_despawn();
    }
    prompt.target = None;
    prompt.duplicate = None;
    prompt.inline_row = None;
}

fn submit_inline_on_outside_press(
    event: On<Pointer<Press>>,
    parents: Query<&ChildOf>,
    mut prompt: ResMut<Prompt>,
) {
    let Some(wrapper) = prompt.overlay.filter(|_| prompt.inline_row.is_some()) else {
        return;
    };
    if !std::iter::once(event.entity)
        .chain(parents.iter_ancestors(event.entity))
        .any(|entity| entity == wrapper)
    {
        prompt.return_focus = None;
        if !prompt.pending && !prompt.blur_attempted {
            prompt.submit_requested = true;
            prompt.blur_attempted = true;
        }
    }
}

fn submit_inline(
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    inputs: Query<&bevy::text::EditableText>,
    choices: Query<(Entity, &Choice)>,
    localizer: Res<Localizer>,
) {
    if prompt.inline_row.is_none() || !prompt.submit_requested {
        return;
    }
    prompt.submit_requested = false;
    if prompt.pending {
        return;
    }
    let Some(text) = prompt.input.and_then(|input| inputs.get(input).ok()) else {
        return;
    };
    if text.is_composing() {
        return;
    }
    // Native text input queues edits until PostUpdate. Read the committed text,
    // not the previous frame's value or an earlier ValueChange event.
    prompt.name = text.value().to_string();
    if prompt.name.trim().is_empty() {
        prompt.failure = Some(localizer.text("browser-rename-empty"));
        return;
    }
    if prompt.name == prompt.original_name {
        close(&mut commands, &mut prompt);
    } else if let Some((entity, _)) = choices
        .iter()
        .find(|(_, choice)| matches!(choice, Choice::Create))
    {
        commands.trigger(Activate { entity });
    }
}

#[allow(clippy::too_many_arguments)]
fn inline_lifecycle(
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    focus: Option<Res<InputFocus>>,
    parents: Query<&ChildOf>,
    nodes: Query<&Node>,
    pending: Query<(), With<PendingFocus>>,
    state: Option<Res<AssetBrowserState>>,
    catalog: Option<Res<ProjectEffectCatalog>>,
) {
    let (Some(wrapper), Some(row)) = (prompt.overlay, prompt.inline_row) else {
        return;
    };
    if !pending.is_empty() {
        return;
    }
    let focused = focus.as_ref().and_then(|focus| focus.get());
    let inside = focused.is_some_and(|entity| {
        entity == wrapper
            || parents
                .iter_ancestors(entity)
                .any(|ancestor| ancestor == wrapper)
    });
    let hidden = nodes
        .get(row)
        .map_or(true, |node| node.display == Display::None);
    let changed = state.as_ref().is_some_and(|state| state.legacy)
        || catalog.as_ref().is_some_and(|catalog| {
            prompt.target.is_some_and(|(_, version)| {
                version.generation != catalog.content_revision().generation
            })
        });
    if changed || (hidden && !prompt.submit_requested && !prompt.pending) {
        // Focus already moved to another control; never steal it back on blur/navigation.
        prompt.return_focus = None;
        close(&mut commands, &mut prompt);
    } else if inside {
        prompt.blur_attempted = false;
    } else {
        prompt.return_focus = None;
        if !prompt.pending && !prompt.blur_attempted {
            prompt.blur_attempted = true;
            prompt.submit_requested = true;
        }
    }
}
fn escape(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut prompt: ResMut<Prompt>,
    mut commands: Commands,
) {
    if keys.is_some_and(|keys| keys.just_pressed(KeyCode::Escape)) {
        close(&mut commands, &mut prompt);
    }
}
fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut prompt: ResMut<Prompt>,
    mut commands: Commands,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    tasks: Option<Res<io::ProjectIoTasks>>,
) {
    let Ok(choice) = choices.get(event.entity) else {
        return;
    };
    if matches!(choice, Choice::Cancel) {
        close(&mut commands, &mut prompt);
        return;
    }
    if !io::idle(tasks) {
        return;
    }
    if prompt.name.trim().is_empty() || name_collision(&prompt, &catalog) {
        return;
    }
    let Some((parent, version)) = prompt.target else {
        return;
    };
    if version != catalog.content_revision() {
        session.status = "Project changed; reopen the operation prompt".into();
        if prompt.rename {
            prompt.failure = Some(session.status.clone());
            return;
        }
        close(&mut commands, &mut prompt);
        return;
    }
    if let Some(source) = prompt.duplicate {
        let (drafts, complete) = super::inspection::draft_inventory(&catalog, &session);
        if prompt.rename {
            let request = OperationRequest::Rename {
                source,
                name: prompt.name.clone(),
            };
            let guard = IoGuard::capture(&catalog, &session);
            let submitted_target = prompt.target;
            let submitted_name = prompt.name.clone();
            let submitted_generation = prompt.generation;
            prompt.pending = true;
            prompt.failure = None;
            let mut prepared = catalog.clone();
            io::enqueue(&mut commands, guard.clone(), move || {
                let plan = prepared
                    .content()
                    .plan_material_rename(request, &drafts, complete);
                io::completion(move |world| {
                    let current = world.resource::<Prompt>();
                    if current.generation != submitted_generation {
                        return;
                    }
                    if current.target != submitted_target || current.name != submitted_name {
                        world.resource_mut::<Prompt>().pending = false;
                        return;
                    }
                    // A destructive path change must not commit against drafts edited during planning.
                    if !guard.matches_material_reload(
                        world.resource::<ProjectEffectCatalog>(),
                        world.resource::<EditorSession>(),
                    ) {
                        io::set_status(world, "project-operation-queued-cancelled");
                        let message = world.resource::<EditorSession>().status.clone();
                        let mut prompt = world.resource_mut::<Prompt>();
                        prompt.pending = false;
                        prompt.failure = Some(message);
                        return;
                    }
                    let result = plan.and_then(|plan| plan.apply());
                    if result.is_ok() {
                        prepared.refresh();
                    }
                    let status = match &result {
                        Ok(result) => format!("Renamed file: {}", result.destination.display()),
                        Err(error) => format!("Rename failed: {error}"),
                    };
                    if let Ok(result) = &result
                        && let Ok(entry) = prepared.content().unique_source_for_asset(result.asset)
                        && let Some(mut state) = world.get_resource_mut::<AssetBrowserState>()
                    {
                        state.reconcile(prepared.content(), prepared.content_revision());
                        state.locate(prepared.content(), entry.id);
                    }
                    if result.is_ok() {
                        io::publish_catalog(world, prepared);
                        world.resource_scope(|world, mut prompt: Mut<Prompt>| {
                            prompt.pending = false;
                            close(&mut world.commands(), &mut prompt);
                        });
                    } else {
                        let mut prompt = world.resource_mut::<Prompt>();
                        prompt.pending = false;
                        prompt.failure = Some(status.clone());
                    }
                    world.resource_mut::<EditorSession>().status = status;
                })
            });
            return;
        }
        let request = OperationRequest::Duplicate {
            source,
            parent,
            name: prompt.name.clone(),
        };
        let guard = IoGuard::capture(&catalog, &session);
        let mut prepared = catalog.clone();
        io::enqueue(&mut commands, guard.clone(), move || {
            let result = prepared
                .content()
                .plan_saved_material_duplicate(request, &drafts, complete)
                .and_then(|plan| plan.apply());
            prepared.refresh();
            io::completion(move |world| {
                if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
                    return;
                }
                let status = match &result {
                    Ok(result) => format!(
                        "Duplicated saved asset: {}",
                        result.created_source.display()
                    ),
                    Err(error) => format!("Duplicate failed: {error}"),
                };
                if let Ok(result) = &result
                    && let Ok(entry) = prepared.content().unique_source_for_asset(result.asset)
                    && let Some(mut state) = world.get_resource_mut::<AssetBrowserState>()
                {
                    state.reconcile(prepared.content(), prepared.content_revision());
                    state.locate(prepared.content(), entry.id);
                }
                io::publish_catalog(world, prepared);
                world.resource_mut::<EditorSession>().status = status;
            })
        });
        close(&mut commands, &mut prompt);
        return;
    }
    let request = OperationRequest::CreateFolder {
        parent,
        name: prompt.name.clone(),
    };
    let guard = IoGuard::capture(&catalog, &session);
    let mut prepared = catalog.clone();
    io::enqueue(&mut commands, guard.clone(), move || {
        prepared.refresh();
        let result = prepared
            .content()
            .plan_operation(request)
            .and_then(|plan| plan.apply());
        prepared.refresh();
        io::completion(move |world| {
            if !guard.same_project(world.resource::<ProjectEffectCatalog>()) {
                return;
            }
            let status = match &result {
                Ok(result) => format!("Created folder: {}", result.created_directory.display()),
                Err(error) => format!("Folder creation failed: {error}"),
            };
            io::publish_catalog(world, prepared);
            world.resource_mut::<EditorSession>().status = status;
        })
    });
    close(&mut commands, &mut prompt);
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    #[test]
    fn rename_failure_stays_visible_in_prompt_and_can_retry() {
        use aestra_core::material::MaterialProgram;
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let original = root.path().join("original.aestra.material.ron");
        program.save_ron(&original).unwrap();
        let unknown = root.path().join("unknown.wgsl");
        std::fs::write(&unknown, "#import external::material").unwrap();
        let catalog = ProjectEffectCatalog::scan(root.path());
        let source = catalog
            .content()
            .unique_source_for_asset(aestra_project::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        let prompt = Prompt {
            target: Some((
                catalog.content().source_tree().root(),
                catalog.content_revision(),
            )),
            duplicate: Some(source),
            rename: true,
            name: "renamed".into(),
            suffix: ".aestra.material.ron".into(),
            ..default()
        };
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(prompt)
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(choose);
        let button = app.world_mut().spawn(Choice::Create).id();
        let error = app
            .world_mut()
            .spawn((NameError, Text::default(), Node::default()))
            .id();
        app.world_mut().trigger(Activate { entity: button });
        io::drain(app.world_mut());
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(
            app.world()
                .get::<Text>(error)
                .unwrap()
                .0
                .contains("unknown.wgsl")
        );
        assert_eq!(
            app.world().get::<Node>(error).unwrap().display,
            Display::Flex
        );
        assert!(app.world().resource::<Prompt>().target.is_some());
        assert!(!app.world().resource::<Prompt>().pending);
        assert!(original.exists());
        std::fs::remove_file(unknown).unwrap();
        let target = app.world().resource::<Prompt>().target;
        app.world_mut().trigger(Activate { entity: button });
        let mut cancelled = io::prepared_completion(app.world_mut());
        app.world_mut().resource_mut::<Prompt>().target = None;
        cancelled.apply(app.world_mut());
        assert!(original.exists());
        app.world_mut().resource_mut::<Prompt>().target = target;
        app.world_mut().trigger(Activate { entity: button });
        io::drain(app.world_mut());
        assert!(app.world().resource::<Prompt>().target.is_none());
        assert!(root.path().join("renamed.aestra.material.ron").exists());
    }
    #[test]
    fn rename_selects_same_asset_and_cancels_if_drafts_change_during_preflight() {
        use aestra_core::material::MaterialProgram;
        for edit_during_preflight in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let program = MaterialProgram::additive_sprite("Display name");
            let old = directory.path().join("original.aestra.material.ron");
            let new = directory.path().join("renamed.aestra.material.ron");
            program.save_ron(&old).unwrap();
            let catalog = ProjectEffectCatalog::scan(directory.path());
            let asset = aestra_project::ProjectAssetId::MaterialProgram(program.id);
            let source = catalog.content().unique_source_for_asset(asset).unwrap().id;
            let prompt = Prompt {
                target: Some((
                    catalog.content().source_tree().root(),
                    catalog.content_revision(),
                )),
                duplicate: Some(source),
                rename: true,
                name: "renamed".into(),
                suffix: ".aestra.material.ron".into(),
                ..default()
            };
            let session = crate::test_support::session_with_timing_slack();
            let effect = session.effect.clone();
            let mut app = App::new();
            app.insert_resource(catalog)
                .insert_resource(prompt)
                .insert_resource(session)
                .insert_resource(AssetBrowserState::default())
                .insert_resource(Localizer::new("en-US").unwrap())
                .add_observer(choose);
            let button = app.world_mut().spawn(Choice::Create).id();
            app.world_mut().trigger(Activate { entity: button });
            let mut completion = io::prepared_completion(app.world_mut());
            if edit_during_preflight {
                let mut draft = program.clone();
                draft.name = "New draft".into();
                app.world_mut()
                    .resource_mut::<ProjectEffectCatalog>()
                    .replace_material_program(&program, &draft)
                    .unwrap();
            }
            completion.apply(app.world_mut());
            assert_eq!(old.exists(), edit_during_preflight);
            assert_eq!(new.exists(), !edit_during_preflight);
            assert_eq!(app.world().resource::<EditorSession>().effect, effect);
            if !edit_during_preflight {
                let catalog = app.world().resource::<ProjectEffectCatalog>();
                let selected = catalog.content().unique_source_for_asset(asset).unwrap().id;
                assert_eq!(
                    app.world().resource::<AssetBrowserState>().selected,
                    Some(selected)
                );
                assert_eq!(
                    MaterialProgram::load_ron(&new).unwrap().name,
                    "Display name"
                );
            }
        }
    }
    #[test]
    fn duplicate_selects_independent_copy_and_preserves_active_document() {
        use aestra_core::material::MaterialProgram;
        let directory = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let original_path = directory.path().join("original.aestra.material.ron");
        program.save_ron(&original_path).unwrap();
        let original_bytes = std::fs::read(&original_path).unwrap();
        let catalog = ProjectEffectCatalog::scan(directory.path());
        let source = catalog
            .content()
            .unique_source_for_asset(aestra_project::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        let mut state = AssetBrowserState::default();
        state.reconcile(catalog.content(), catalog.content_revision());
        let prompt = Prompt {
            target: Some((
                catalog.content().source_tree().root(),
                catalog.content_revision(),
            )),
            duplicate: Some(source),
            suffix: ".aestra.material.ron".into(),
            name: "copy".into(),
            ..default()
        };
        let session = crate::test_support::session_with_timing_slack();
        let effect = session.effect.clone();
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(session)
            .insert_resource(state)
            .insert_resource(prompt)
            .insert_resource(Localizer::new("en-US").unwrap())
            .add_observer(choose);
        let button = app.world_mut().spawn(Choice::Create).id();
        app.world_mut().trigger(Activate { entity: button });
        // Edits made while saved-copy I/O finishes must survive publication.
        let mut completion = io::prepared_completion(app.world_mut());
        let mut original_draft = program.clone();
        original_draft.name = "Later original edit".into();
        app.world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .replace_material_program(&program, &original_draft)
            .unwrap();
        completion.apply(app.world_mut());
        let copy =
            MaterialProgram::load_ron(directory.path().join("copy.aestra.material.ron")).unwrap();
        assert_ne!(copy.id, program.id);
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        let copy_source = catalog
            .content()
            .unique_source_for_asset(aestra_project::ProjectAssetId::MaterialProgram(copy.id))
            .unwrap()
            .id;
        assert_eq!(
            app.world().resource::<AssetBrowserState>().selected,
            Some(copy_source)
        );
        assert_eq!(app.world().resource::<EditorSession>().effect, effect);
        let mut changed = copy.clone();
        changed.name = "Edited copy".into();
        app.world_mut()
            .resource_mut::<ProjectEffectCatalog>()
            .replace_material_program(&copy, &changed)
            .unwrap();
        assert_eq!(std::fs::read(&original_path).unwrap(), original_bytes);
        let catalog = app.world().resource::<ProjectEffectCatalog>();
        assert_eq!(
            catalog.material_drafts.programs[&program.id]
                .current
                .as_ref(),
            Some(&original_draft.normalized())
        );
        assert!(catalog.material_drafts.programs.contains_key(&copy.id));
    }

    #[test]
    fn duplicate_collision_and_unsaved_source_disable_confirmation() {
        use aestra_core::material::MaterialProgram;
        let directory = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        program
            .save_ron(directory.path().join("original.aestra.material.ron"))
            .unwrap();
        let mut catalog = ProjectEffectCatalog::scan(directory.path());
        let source = catalog
            .content()
            .unique_source_for_asset(aestra_project::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        let prompt = Prompt {
            target: Some((
                catalog.content().source_tree().root(),
                catalog.content_revision(),
            )),
            duplicate: Some(source),
            suffix: ".aestra.material.ron".into(),
            name: "ORIGINAL".into(),
            ..default()
        };
        assert!(name_collision(&prompt, &catalog));
        let mut changed = program.clone();
        changed.name = "Unsaved".into();
        catalog
            .replace_material_program(&program, &changed)
            .unwrap();
        let mut app = App::new();
        app.insert_resource(catalog)
            .insert_resource(prompt)
            .insert_resource(crate::test_support::session_with_timing_slack())
            .insert_resource(Localizer::new("en-US").unwrap());
        app.world_mut().resource_mut::<Prompt>().name = "unused".into();
        let button = app.world_mut().spawn(Choice::Create).id();
        let error = app
            .world_mut()
            .spawn((NameError, Text::default(), Node::default()))
            .id();
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        assert!(
            app.world()
                .get::<Text>(error)
                .unwrap()
                .0
                .contains("Save the source")
        );
        assert!(!directory.path().join("unused.aestra.material.ron").exists());
    }
    #[test]
    fn blank_name_disables_create_and_escape_closes_overlay() {
        let mut app = App::new();
        app.init_resource::<Prompt>()
            .init_resource::<ButtonInput<KeyCode>>();
        let button = app.world_mut().spawn(Choice::Create).id();
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        app.world_mut().resource_mut::<Prompt>().name = "Folder".into();
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_none());
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("Folder")).unwrap();
        let catalog = ProjectEffectCatalog::scan(directory.path());
        app.world_mut().resource_mut::<Prompt>().target = Some((
            catalog.content().source_tree().root(),
            catalog.content_revision(),
        ));
        app.insert_resource(catalog)
            .insert_resource(Localizer::new("en-US").unwrap());
        let error = app
            .world_mut()
            .spawn((NameError, Text::default(), Node::default()))
            .id();
        app.world_mut().resource_mut::<Prompt>().name = "folder".into();
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        assert!(
            app.world()
                .get::<Text>(error)
                .unwrap()
                .0
                .contains("already exists")
        );
        assert_eq!(
            app.world().get::<Node>(error).unwrap().display,
            Display::Flex
        );
        app.world_mut().resource_mut::<Prompt>().name = "Different".into();
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_none());
        assert_eq!(
            app.world().get::<Node>(error).unwrap().display,
            Display::None
        );
        app.world_mut().resource_mut::<Prompt>().name = "  ".into();
        app.world_mut().run_system_once(sync_controls).unwrap();
        assert!(app.world().get::<InteractionDisabled>(button).is_some());
        let overlay = app.world_mut().spawn_empty().id();
        app.world_mut().resource_mut::<Prompt>().overlay = Some(overlay);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.world_mut().run_system_once(escape).unwrap();
        assert!(app.world().resource::<Prompt>().overlay.is_none());
        assert!(app.world().get_entity(overlay).is_err());
    }
    #[test]
    fn create_and_cancel_use_scoped_project_io() {
        for cancel in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let catalog = ProjectEffectCatalog::scan(directory.path());
            let parent = catalog.content().source_tree().root();
            let version = catalog.content_revision();
            let session = crate::test_support::session_with_timing_slack();
            let effect = session.effect.clone();
            let mut app = App::new();
            app.insert_resource(catalog)
                .insert_resource(session)
                .insert_resource(Localizer::new("en-US").unwrap())
                .insert_resource(Prompt {
                    target: Some((parent, version)),
                    overlay: None,
                    name: "Textures".into(),
                    ..default()
                })
                .add_observer(choose);
            let action = app
                .world_mut()
                .spawn(if cancel {
                    Choice::Cancel
                } else {
                    Choice::Create
                })
                .id();
            app.world_mut().trigger(Activate { entity: action });
            io::drain(app.world_mut());
            assert_eq!(directory.path().join("Textures").is_dir(), !cancel);
            assert_eq!(app.world().resource::<EditorSession>().effect, effect);
            assert!(app.world().resource::<Prompt>().target.is_none());
        }
    }
}
