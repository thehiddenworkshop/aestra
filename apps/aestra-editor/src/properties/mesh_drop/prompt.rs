//! Read-only background inspection; only the final selection changes the document.
use super::*;
use crate::project_content::io::{self, IoGuard};
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus, tab_navigation::TabGroup};
#[cfg(test)]
mod tests;

#[derive(Event)]
pub(in crate::properties) struct OpenMeshDrop {
    pub payload: AssetPayload,
    pub target: MeshDropTarget,
}

#[derive(Event)]
pub(in crate::properties) struct OpenMeshRenderer;

struct Pending {
    payload: Option<AssetPayload>,
    target: Target,
    sources: Vec<(String, AssetPayload)>,
    guard: IoGuard,
    overlay: Entity,
    panel: Entity,
    choices: Vec<ProjectMeshPrimitive>,
    focused: bool,
}
#[derive(Resource, Default)]
struct Prompt(Option<Pending>);
#[derive(Component, Clone, Copy)]
enum Choice {
    Source(usize),
    Assign(usize),
    Cancel,
}

pub(in crate::properties) fn register(app: &mut App) {
    app.init_resource::<Prompt>()
        .init_resource::<DocumentProtectionState>()
        .add_observer(open)
        .add_observer(open_renderer)
        .add_observer(choose)
        .add_observer(keyboard)
        .add_systems(Update, sync);
}

fn open(event: On<OpenMeshDrop>, mut commands: Commands) {
    let payload = event.payload.clone();
    let target = event.target;
    commands.queue(move |world: &mut World| start(world, Some(payload), Target::Existing(target)));
}

fn open_renderer(_: On<OpenMeshRenderer>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        if let Some(emitter) = world
            .resource::<EditorSession>()
            .selected_layer()
            .map(|item| item.id)
        {
            start(world, None, Target::Create(emitter));
        }
    });
}

fn start(world: &mut World, payload: Option<AssetPayload>, target: Target) {
    if world.resource::<Prompt>().0.is_some()
        || world.resource::<DocumentProtectionState>().is_open()
        || !io::idle_world(world)
        || crate::asset_drop::cancelled(world.get_resource::<ButtonInput<KeyCode>>())
    {
        return;
    }
    let catalog = world.resource::<ProjectEffectCatalog>().clone();
    let session = world.resource::<EditorSession>();
    if let Err(error) = target.check(session).and_then(|()| {
        payload
            .as_ref()
            .map_or(Ok(()), |payload| payload.mesh_source(&catalog).map(|_| ()))
    }) {
        world.resource_mut::<EditorSession>().status = format!("Mesh drop rejected: {error}");
        return;
    }
    let guard = IoGuard::capture(&catalog, session);
    let mut sources: Vec<_> = catalog
        .content()
        .source_tree()
        .entries()
        .filter_map(|source| {
            let payload = AssetPayload::capture(&catalog, source.id);
            payload.mesh_source(&catalog).ok()?;
            Some((source.relative_path.display().to_string(), payload))
        })
        .collect();
    sources.sort_by(|a, b| a.0.cmp(&b.0));
    let mut panel = Entity::PLACEHOLDER;
    let overlay = world
        .commands()
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
        .with_children(|root| {
            panel = root
                .spawn((
                    Node {
                        width: Val::Px(470.0),
                        max_width: Val::Percent(95.0),
                        max_height: Val::Percent(85.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(10.0),
                        padding: UiRect::all(Val::Px(16.0)),
                        ..default()
                    },
                    BackgroundColor(theme::PANEL),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        Text::new("Reading mesh primitives…"),
                        TextColor(theme::TEXT),
                    ));
                    crate::feathers::button::spawn_action_button(
                        panel,
                        "Cancel",
                        Choice::Cancel,
                        false,
                    );
                })
                .id();
        })
        .id();
    world.resource_mut::<Prompt>().0 = Some(Pending {
        payload: payload.clone(),
        target,
        sources,
        guard: guard.clone(),
        overlay,
        panel,
        choices: Vec::new(),
        focused: false,
    });
    world
        .resource_mut::<DocumentProtectionState>()
        .asset_create_open = true;
    let Some(payload) = payload else {
        show_choices(world);
        return;
    };
    io::enqueue(&mut world.commands(), guard.clone(), move || {
        let result = inspect(&payload, &catalog);
        io::completion(move |world| {
            if world
                .resource::<Prompt>()
                .0
                .as_ref()
                .is_none_or(|pending| pending.overlay != overlay)
            {
                return;
            }
            if !guard.matches(
                world.resource::<ProjectEffectCatalog>(),
                world.resource::<EditorSession>(),
            ) {
                close(
                    world,
                    "Mesh assignment cancelled: document or project changed".into(),
                );
                return;
            }
            match result {
                Err(error) => close(world, format!("Mesh drop rejected: {error}")),
                Ok(choices) => {
                    let single = choices.len() == 1;
                    world.resource_mut::<Prompt>().0.as_mut().unwrap().choices = choices;
                    if single {
                        assign(world, 0);
                    } else {
                        show_choices(world);
                    }
                }
            }
        })
    });
}

fn show_choices(world: &mut World) {
    world.resource_mut::<Prompt>().0.as_mut().unwrap().focused = false;
    let pending = world.resource::<Prompt>().0.as_ref().unwrap();
    let panel = pending.panel;
    let choices = pending.choices.clone();
    let picking_source = pending.payload.is_none();
    let creating = matches!(pending.target, Target::Create(_));
    let title = if picking_source {
        "Add Mesh Renderer"
    } else {
        "Choose mesh primitive"
    };
    let description = if picking_source {
        "Choose a glTF/GLB from the project."
    } else if creating {
        "Creates a mesh renderer with a new local material."
    } else {
        "Assigns geometry only. The renderer material stays unchanged."
    };
    let options: Vec<_> = if picking_source {
        pending
            .sources
            .iter()
            .enumerate()
            .map(|(index, (name, _))| (name.clone(), Choice::Source(index)))
            .collect()
    } else {
        choices
            .iter()
            .enumerate()
            .map(|(index, primitive)| {
                (
                    format!(
                        "Mesh {} · Primitive {} · {} vertices",
                        primitive.mesh + 1,
                        primitive.primitive + 1,
                        primitive.vertices
                    ),
                    Choice::Assign(index),
                )
            })
            .collect()
    };
    world
        .commands()
        .entity(panel)
        .despawn_related::<Children>()
        .with_children(|panel| {
            panel.spawn((Text::new(title), TextColor(theme::TEXT)));
            panel.spawn((
                Text::new(description),
                TextColor(theme::TEXT_MUTED),
            ));
            panel
                .spawn(Node {
                    height: Val::Px(320.0),
                    min_height: Val::Px(0.0),
                    ..default()
                })
                .with_children(|row| {
                    let list = row
                        .spawn((
                            Node {
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                overflow: Overflow::scroll_y(),
                                flex_direction: FlexDirection::Column,
                                row_gap: Val::Px(4.0),
                                scrollbar_width: 0.0,
                                ..default()
                            },
                            bevy::ui_widgets::ScrollArea,
                        ))
                        .with_children(|list| {
                            if options.is_empty() {
                                list.spawn((Text::new("No glTF/GLB meshes found. Add a mesh to the project through Assets, then try again."), TextColor(theme::TEXT_MUTED)));
                            }
                            for (label, choice) in &options {
                                list.spawn(Node {
                                    flex_shrink: 0.0,
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|item| {
                                    crate::feathers::button::spawn_action_button(
                                        item,
                                        label,
                                        *choice,
                                        false,
                                    );
                                });
                            }
                        })
                        .id();
                    crate::feathers::scroll::spawn_vertical_scrollbar(row, list);
                });
            crate::feathers::button::spawn_action_button(panel, "Cancel", Choice::Cancel, false);
        });
}

fn assign(world: &mut World, index: usize) {
    let Some(pending) = &world.resource::<Prompt>().0 else {
        return;
    };
    let catalog = world.resource::<ProjectEffectCatalog>();
    let session = world.resource::<EditorSession>();
    let result = (|| {
        if crate::asset_drop::cancelled(world.get_resource::<ButtonInput<KeyCode>>())
            || !pending.guard.matches(catalog, session)
        {
            return Err("Assignment cancelled: document or project changed".into());
        }
        let mut protection = world.resource::<DocumentProtectionState>().clone();
        protection.asset_create_open = false;
        if protection.is_open() || !io::idle_world(world) {
            return Err("Finish the current document operation before assigning a mesh".into());
        }
        let primitive = pending
            .choices
            .get(index)
            .ok_or("Mesh primitive no longer exists")?;
        let payload = pending
            .payload
            .as_ref()
            .ok_or("Choose a mesh source first")?;
        resource::check_file(payload.mesh_source(catalog)?, catalog)?;
        match pending.target {
            Target::Existing(target) => plan(payload, target, primitive, catalog, session),
            target @ Target::Create(_) => plan_target(payload, target, primitive, catalog, session),
        }
    })();
    match result {
        Err(error) => close(world, format!("Mesh drop rejected: {error}")),
        Ok(assignment) => {
            close(world, assignment.label);
            if let Some(transaction) = assignment.transaction {
                let mut session = world.resource_mut::<EditorSession>();
                let renderer = transaction
                    .commands
                    .iter()
                    .find_map(|command| match command {
                        EffectCommand::AddRenderer { renderer, .. } => Some(renderer.id),
                        _ => None,
                    });
                if session.execute_transaction(transaction, true) {
                    session.material_history_active = false;
                    if let Some(renderer) = renderer {
                        session.selection.primary = SemanticTarget::Renderer(renderer);
                    }
                }
            }
        }
    }
}

fn select_source(world: &mut World, index: usize) {
    let Some(pending) = world.resource::<Prompt>().0.as_ref() else {
        return;
    };
    if pending.payload.is_some() {
        return;
    }
    if !pending.guard.matches(
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    ) {
        close(
            world,
            "Mesh creation cancelled: document or project changed".into(),
        );
        return;
    }
    let Some((_, payload)) = pending.sources.get(index) else {
        return;
    };
    let payload = payload.clone();
    let target = pending.target;
    close(world, "Reading mesh primitives…".into());
    start(world, Some(payload), target);
}

fn close(world: &mut World, status: String) {
    if let Some(pending) = world.resource_mut::<Prompt>().0.take() {
        world.commands().entity(pending.overlay).try_despawn();
        world
            .resource_mut::<DocumentProtectionState>()
            .asset_create_open = false;
        world.resource_mut::<EditorSession>().status = status;
    }
}

fn choose(event: On<Activate>, choices: Query<&Choice>, mut commands: Commands) {
    let Ok(choice) = choices.get(event.entity).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| match choice {
        Choice::Source(index) => select_source(world, index),
        Choice::Cancel => close(world, "Mesh assignment cancelled".into()),
        Choice::Assign(index) => assign(world, index),
    });
}

fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    prompt: Res<Prompt>,
    mut commands: Commands,
) {
    if prompt.0.is_some()
        && event.input.state == ButtonState::Pressed
        && event.input.key_code == KeyCode::Escape
    {
        event.propagate(false);
        commands.queue(|world: &mut World| close(world, "Mesh assignment cancelled".into()));
    }
}

fn sync(world: &mut World) {
    let Some(pending) = &world.resource::<Prompt>().0 else {
        return;
    };
    let cancel = crate::asset_drop::cancelled(world.get_resource::<ButtonInput<KeyCode>>())
        || !pending.guard.matches(
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
        );
    if cancel {
        close(world, "Mesh assignment cancelled".into());
        return;
    }
    let pending = world.resource::<Prompt>().0.as_ref().unwrap();
    if !pending.focused {
        let ready = !pending.choices.is_empty();
        let sources = pending.payload.is_none() && !pending.sources.is_empty();
        let button = world
            .query::<(Entity, &Choice)>()
            .iter(world)
            .find(|(_, choice)| {
                if sources {
                    matches!(choice, Choice::Source(0))
                } else if ready {
                    matches!(choice, Choice::Assign(0))
                } else {
                    matches!(choice, Choice::Cancel)
                }
            })
            .map(|(entity, _)| entity);
        if let Some(button) = button
            && let Some(mut focus) = world.get_resource_mut::<InputFocus>()
        {
            focus.set(button, FocusCause::Navigated);
            world.resource_mut::<Prompt>().0.as_mut().unwrap().focused = true;
        }
    }
}
