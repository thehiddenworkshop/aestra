//! Presets create a distinct saved program; assigning it uses the normal effect transaction.
use super::*;
use crate::project_content::io::{self, IoGuard};
use aestra_core::material::MaterialPresetDescriptor;
use aestra_project::ProjectSourceId;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus, tab_navigation::TabGroup};
use bevy::text::EditableText;
use bevy::ui_widgets::ValueChange;

#[cfg(test)]
mod tests;

#[derive(Event)]
pub(super) struct OpenPresetDrop {
    pub payload: AssetPayload,
    pub target: RendererDropTarget,
}

pub(super) fn prepare(
    payload: &AssetPayload,
    target: RendererDropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<MaterialProgram, String> {
    let Some(ProjectAssetId::MaterialPreset(id)) = payload.resolve(catalog)? else {
        return Err("Expected a material preset".into());
    };
    let renderer = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| &emitter.renderers)
        .find(|renderer| renderer.id == target.renderer)
        .ok_or("Renderer no longer exists")?;
    let domain = renderer_domain(&renderer.properties).ok_or("Unsupported renderer")?;
    let presets = catalog.material_preset_catalog()?;
    let descriptor = presets.get(id).ok_or("Preset is no longer available")?;
    let base = crate::material_graph::material_preset_base(&descriptor.display_name, domain);
    let insertion = MaterialCompiler
        .stack_preset_targets_with_catalog(&base, &presets)
        .map_err(|error| error.to_string())?
        .into_iter()
        .filter(|target| target.preset == id)
        .max_by_key(|target| target.index)
        .ok_or("This preset cannot create a material for this renderer")?;
    let program = MaterialCompiler
        .plan_stack_insert_preset_with_catalog(&base, &presets, id, insertion.index)
        .map_err(|error| error.to_string())?
        .replacement;
    plan_program(&program, target, catalog, session)?;
    Ok(program)
}

struct Pending {
    payload: AssetPayload,
    target: RendererDropTarget,
    program: MaterialProgram,
    preset: MaterialPresetDescriptor,
    overlay: Entity,
    name: String,
    folder: String,
    failure: Option<String>,
    busy: bool,
    submit: bool,
}
#[derive(Resource, Default)]
struct Prompt(Option<Pending>);
#[derive(Component, Clone, Copy)]
enum Field {
    Name,
    Folder,
}
#[derive(Component)]
struct FocusName;
#[derive(Component)]
struct Description;
#[derive(Component, Clone, Copy)]
enum Choice {
    Create,
    Cancel,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Prompt>()
        .init_resource::<DocumentProtectionState>()
        .add_observer(open)
        .add_observer(change)
        .add_observer(choose)
        .add_observer(keyboard)
        .add_systems(Update, sync)
        .add_systems(PostUpdate, submit.after(bevy::text::EditableTextSystems));
}

fn folder_id(catalog: &ProjectEffectCatalog, folder: &str) -> Result<ProjectSourceId, String> {
    let folder = folder.replace('\\', "/");
    if folder == "." {
        return Ok(catalog.content().source_tree().root());
    }
    if folder.is_empty()
        || folder
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("Choose an existing folder relative to assets; use . for the root".into());
    }
    catalog
        .content()
        .source_tree()
        .at_relative_path(&folder)
        .map(|entry| entry.id)
        .ok_or_else(|| "Folder does not exist in this project".into())
}

fn destination(
    prompt: &Pending,
    catalog: &ProjectEffectCatalog,
) -> Result<(ProjectSourceId, std::path::PathBuf), String> {
    prompt.payload.resolve(catalog)?;
    let parent = folder_id(catalog, &prompt.folder)?;
    let path = catalog
        .content()
        .material_creation_destination(parent, &prompt.name)
        .map_err(|error| error.to_string())?;
    Ok((parent, path))
}

#[allow(clippy::too_many_arguments)]
fn open(
    event: On<OpenPresetDrop>,
    mut commands: Commands,
    mut prompt: ResMut<Prompt>,
    catalog: Res<ProjectEffectCatalog>,
    mut session: ResMut<EditorSession>,
    mut protection: ResMut<DocumentProtectionState>,
    tasks: Option<Res<io::ProjectIoTasks>>,
) {
    if prompt.0.is_some() || protection.is_open() || !io::idle(tasks) {
        return;
    }
    let program = match prepare(&event.payload, event.target, &catalog, &session) {
        Ok(program) => program,
        Err(error) => {
            session.status = error;
            return;
        }
    };
    let Ok(Some(ProjectAssetId::MaterialPreset(id))) = event.payload.resolve(&catalog) else {
        return;
    };
    let Ok(preset) = catalog.content().cached_material_preset(id) else {
        return;
    };
    let folder = catalog
        .content()
        .source_tree()
        .at_relative_path("materials")
        .filter(|entry| entry.kind == aestra_project::ProjectSourceKind::Directory)
        .map_or(".", |_| "materials")
        .to_owned();
    let overlay = commands
        .spawn((
            TabGroup::modal(),
            crate::feathers::node_graph::FeathersGraphNavigationBlocker,
            GlobalZIndex(320),
            RelativeCursorPosition::default(),
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
                        width: Val::Px(470.0),
                        max_width: Val::Percent(95.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new(format!("Create material from {}", preset.display_name)),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 16.0.into(),
                            ..default()
                        },
                    ));
                    panel.spawn((Text::new("Material name"), TextColor(theme::TEXT)));
                    let input = crate::feathers::text_input::spawn_text_input(
                        panel,
                        "",
                        "Material name",
                        Field::Name,
                    );
                    panel.commands().entity(input).insert(FocusName);
                    panel.spawn((
                        Text::new("Folder (relative to assets; . for root)"),
                        TextColor(theme::TEXT),
                    ));
                    crate::feathers::text_input::spawn_text_input(
                        panel,
                        &folder,
                        "Project folder",
                        Field::Folder,
                    );
                    panel.spawn((
                        Description,
                        Text::new("Enter a material name"),
                        TextColor(theme::TEXT),
                        TextFont {
                            font_size: 12.0.into(),
                            ..default()
                        },
                    ));
                    panel.spawn((
                        Text::new(
                            "Undo restores the renderer. The new material remains in Assets.",
                        ),
                        TextColor(theme::TEXT_MUTED),
                        TextFont {
                            font_size: 12.0.into(),
                            ..default()
                        },
                    ));
                    panel
                        .spawn_empty()
                        .apply_scene(crate::feathers::scenes::feathers_primary_button())
                        .insert((
                            Choice::Create,
                            InteractionDisabled,
                            crate::feathers::button::FeathersActionButton,
                            AccessibleLabel("Create & Assign".into()),
                        ))
                        .with_child((
                            Text::new("Create & Assign"),
                            bevy::feathers::theme::ThemedText,
                            Pickable::IGNORE,
                        ));
                    crate::feathers::button::spawn_action_button(
                        panel,
                        "Cancel",
                        Choice::Cancel,
                        false,
                    );
                });
        })
        .id();
    prompt.0 = Some(Pending {
        payload: event.payload.clone(),
        target: event.target,
        program,
        preset,
        overlay,
        name: String::new(),
        folder,
        failure: None,
        busy: false,
        submit: false,
    });
    protection.asset_create_open = true;
}

fn close(commands: &mut Commands, prompt: &mut Prompt, protection: &mut DocumentProtectionState) {
    if let Some(pending) = prompt.0.take() {
        commands.entity(pending.overlay).try_despawn();
    }
    protection.asset_create_open = false;
}

fn change(event: On<ValueChange<String>>, fields: Query<&Field>, mut prompt: ResMut<Prompt>) {
    let Some(prompt) = &mut prompt.0 else {
        return;
    };
    if prompt.busy {
        return;
    }
    if let Ok(field) = fields.get(event.source) {
        match field {
            Field::Name => prompt.name.clone_from(&event.value),
            Field::Folder => prompt.folder.clone_from(&event.value),
        }
        prompt.failure = None;
    }
}
fn choose(
    event: On<Activate>,
    choices: Query<&Choice>,
    mut prompt: ResMut<Prompt>,
    mut protection: ResMut<DocumentProtectionState>,
    mut commands: Commands,
) {
    let Some(pending) = &mut prompt.0 else {
        return;
    };
    if pending.busy {
        return;
    }
    match choices.get(event.entity) {
        Ok(Choice::Create) => pending.submit = true,
        Ok(Choice::Cancel) => close(&mut commands, &mut prompt, &mut protection),
        _ => {}
    }
}
fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    mut prompt: ResMut<Prompt>,
    mut protection: ResMut<DocumentProtectionState>,
    mut commands: Commands,
) {
    let Some(pending) = &mut prompt.0 else {
        return;
    };
    if event.input.state != ButtonState::Pressed || event.input.repeat {
        return;
    }
    match event.input.key_code {
        KeyCode::Escape => {
            event.propagate(false);
            if !pending.busy {
                close(&mut commands, &mut prompt, &mut protection);
            }
        }
        KeyCode::Enter => {
            event.propagate(false);
            if !pending.busy {
                pending.submit = true;
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn sync(
    mut prompt: ResMut<Prompt>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    session: Option<Res<EditorSession>>,
    mut protection: ResMut<DocumentProtectionState>,
    mut commands: Commands,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut labels: Query<&mut Text, With<Description>>,
    buttons: Query<(Entity, &Choice)>,
    pending_focus: Query<(Entity, &Children), With<FocusName>>,
    inputs: Query<(), With<EditableText>>,
    mut focus: Option<ResMut<InputFocus>>,
    tasks: Option<Res<io::ProjectIoTasks>>,
) {
    let Some(pending) = &mut prompt.0 else {
        return;
    };
    if pending.busy && io::idle(tasks) {
        pending.busy = false;
        pending.failure =
            Some("Document changed before creation started; check and try again".into());
    }
    if !pending.busy
        && keys
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
    {
        close(&mut commands, &mut prompt, &mut protection);
        return;
    }
    let (Some(catalog), Some(session)) = (catalog, session) else {
        return;
    };
    let validation = if pending.target.effect != session.effect.id {
        Err("The effect changed; cancel and drag the preset again".into())
    } else {
        destination(pending, &catalog)
    };
    let valid = validation.is_ok() && !pending.busy && pending.failure.is_none();
    let text = if pending.busy {
        "Creating material…".into()
    } else {
        pending.failure.clone().unwrap_or_else(|| {
            validation.map_or_else(|error| error, |(_, path)| path.display().to_string())
        })
    };
    for mut label in &mut labels {
        label.0.clone_from(&text);
    }
    for (entity, choice) in &buttons {
        let enabled = match choice {
            Choice::Create => valid,
            Choice::Cancel => !pending.busy,
        };
        if enabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for (wrapper, children) in &pending_focus {
        if let Some(input) = children.iter().find(|child| inputs.contains(*child)) {
            if let Some(focus) = focus.as_deref_mut() {
                focus.set(input, FocusCause::Navigated);
            }
            commands.entity(wrapper).remove::<FocusName>();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn submit(
    mut prompt: ResMut<Prompt>,
    catalog: Option<Res<ProjectEffectCatalog>>,
    session: Option<Res<EditorSession>>,
    protection: Res<DocumentProtectionState>,
    tasks: Option<Res<io::ProjectIoTasks>>,
    mut commands: Commands,
    fields: Query<(&Field, &Children)>,
    inputs: Query<&EditableText>,
) {
    let Some(pending) = &mut prompt.0 else {
        return;
    };
    if pending.busy || !std::mem::take(&mut pending.submit) {
        return;
    }
    let (Some(catalog), Some(session)) = (catalog, session) else {
        return;
    };
    let mut other_protection = protection.clone();
    other_protection.asset_create_open = false;
    if other_protection.is_open() || !io::idle(tasks) {
        pending.failure = Some("Finish the current document operation first".into());
        return;
    }
    // Consume native text after queued edits, including Enter in the same frame as typing.
    for (field, children) in &fields {
        if let Some(text) = children.iter().find_map(|child| inputs.get(child).ok()) {
            if text.is_composing() {
                return;
            }
            match field {
                Field::Name => pending.name = text.value().to_string(),
                Field::Folder => pending.folder = text.value().to_string(),
            }
        }
    }
    let (parent, _) = match destination(pending, &catalog) {
        Ok(value) => value,
        Err(error) => {
            pending.failure = Some(error);
            return;
        }
    };
    let mut program = pending.program.clone();
    program.name.clone_from(&pending.name);
    let assignment = match plan_program(&program, pending.target, &catalog, &session) {
        Ok(value) => value,
        Err(error) => {
            pending.failure = Some(error);
            return;
        }
    };
    let Some(transaction) = assignment.transaction else {
        return;
    };
    let guard = IoGuard::capture(&catalog, &session);
    let mut prepared = catalog.clone();
    let name = pending.name.clone();
    let expected_preset = pending.preset.clone();
    let effect = session.effect.clone();
    let locks = session.locks.clone();
    pending.busy = true;
    io::enqueue(&mut commands, guard.clone(), move || {
        prepared.refresh();
        let result = (|| -> Result<std::path::PathBuf, String> {
            let current = prepared
                .content()
                .cached_material_preset(expected_preset.id)
                .map_err(|e| e.to_string())?;
            if current != expected_preset {
                return Err("Preset changed on disk; cancel and drag it again".into());
            }
            MaterialCompiler
                .compile_with_functions(&program, &prepared.material_function_library()?)
                .map_err(|error| error.to_string())?;
            let mut candidate = effect.clone();
            aestra_authoring::CommandExecutor::execute(&mut candidate, &locks, &transaction)
                .map_err(|error| error.to_string())?;
            let mut programs = prepared.material_programs_for_effect(&effect)?;
            programs.push(program.clone());
            let mut document = MaterialAuthoringDocument::new(candidate, programs);
            document.material_functions = prepared.material_functions()?;
            document
                .validate()
                .map_err(|error| format!("Material assignment is invalid: {error:?}"))?;
            prepared
                .content()
                .plan_material_creation(parent, &name, &program)
                .map_err(|error| error.to_string())?
                .apply()
                .map_err(|error| error.to_string())
        })();
        if result.is_ok() {
            prepared.refresh();
        }
        io::completion(move |world| {
            let matches = guard.matches_material_reload(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            );
            match result {
                Err(error) => {
                    if let Some(pending) = &mut world.resource_mut::<Prompt>().0 {
                        pending.busy = false;
                        pending.failure = Some(error.clone());
                    }
                    world.resource_mut::<EditorSession>().status =
                        format!("Material creation failed: {error}");
                }
                Ok(path) => {
                    if guard.same_project(world.resource::<ProjectEffectCatalog>()) {
                        io::publish_catalog(world, prepared);
                    }
                    let mut session = world.resource_mut::<EditorSession>();
                    if matches && session.execute_transaction(transaction, true) {
                        session.material_history_active = false;
                        session.status = format!("Created and assigned {}", path.display());
                    } else {
                        session.status = format!(
                            "Created {}; document changed, so it was not assigned",
                            path.display()
                        );
                    }
                    if let Some(pending) = world.resource_mut::<Prompt>().0.take() {
                        world.despawn(pending.overlay);
                    }
                    world
                        .resource_mut::<DocumentProtectionState>()
                        .asset_create_open = false;
                }
            }
        })
    });
}
