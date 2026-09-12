//! Compatible asset selection for Properties. Source assignment is shared with drops.
use super::asset_drop::{AssignAsset, Assignment, DropTarget};
use super::*;
use crate::asset_drop::AssetPayload;
use crate::project_content::io::{self, IoGuard};
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus, tab_navigation::TabGroup};
use bevy::text::EditableText;

#[cfg(test)]
mod tests;

#[derive(Component, Clone, Copy)]
pub(super) struct PickerField(pub DropTarget);
#[derive(Clone)]
enum Selection {
    Source(AssetPayload),
    Texture(Option<aestra_core::AssetId>),
}
#[derive(Clone)]
struct Entry {
    label: String,
    selection: Selection,
}
struct Pending {
    target: DropTarget,
    guard: IoGuard,
    overlay: Entity,
    search: Entity,
    list: Entity,
    error: Entity,
    entries: Vec<Entry>,
    query: String,
    focused: bool,
    return_focus: Option<Entity>,
}
#[derive(Resource, Default)]
struct Picker(Option<Pending>);
#[derive(Component)]
struct Search;
#[derive(Component, Clone, Copy)]
enum Choice {
    Select(usize),
    Cancel,
}

pub(super) fn register(app: &mut App) {
    app.init_resource::<Picker>()
        .add_observer(open)
        .add_observer(choose)
        .add_observer(search)
        .add_observer(keyboard)
        .add_observer(outside)
        .add_systems(Update, sync);
}

pub(super) fn row(
    parent: &mut ChildSpawnerCommands,
    target: DropTarget,
    label: &str,
    current: &str,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(27.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            },
            PickerField(target),
            EditorTooltip::description("Click to browse compatible assets, or drop an asset here."),
        ))
        .with_children(|row| {
            spawn_property_label(row, label);
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|value| {
                crate::feathers::button::spawn_action_button(
                    value,
                    current,
                    PickerField(target),
                    false,
                )
            });
        });
}

fn entries(
    target: DropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Vec<Entry> {
    let mut entries = Vec::new();
    if let DropTarget::Texture(texture) = target
        && super::texture_drop::allows_procedural(texture)
    {
        entries.push(Entry {
            label: "Procedural · No texture".into(),
            selection: Selection::Texture(None),
        });
    }
    for built_in in [false, true] {
        for entry in crate::asset_drop::virtual_sources::entries(catalog, session, built_in) {
            let payload = AssetPayload::capture_virtual(catalog, session, entry.asset);
            if super::asset_drop::plan_target(&payload, target, catalog, session).is_ok() {
                entries.push(Entry {
                    label: format!("{} · {}", entry.name, entry.kind),
                    selection: Selection::Source(payload),
                });
            }
        }
    }
    for source in catalog.content().source_tree().entries() {
        let payload = AssetPayload::capture(catalog, source.id);
        let compatible = match target {
            DropTarget::Texture(_) => payload.texture_source(catalog).is_ok(),
            DropTarget::Mesh(_) => payload.mesh_source(catalog).is_ok(),
            DropTarget::Material(_) => {
                super::asset_drop::plan_target(&payload, target, catalog, session).is_ok()
            }
            DropTarget::Renderer(_) => false,
        };
        if compatible {
            entries.push(Entry {
                label: source.relative_path.display().to_string(),
                selection: Selection::Source(payload),
            });
        }
    }
    entries.sort_by_key(|entry| entry.label.to_lowercase());
    entries
}

fn open(event: On<Activate>, fields: Query<&PickerField>, mut commands: Commands) {
    if let Ok(field) = fields.get(event.entity) {
        let target = field.0;
        commands.queue(move |world: &mut World| start(world, target));
    }
}

fn start(world: &mut World, target: DropTarget) {
    if world.resource::<Picker>().0.is_some()
        || world.resource::<DocumentProtectionState>().is_open()
        || !io::idle_world(world)
        || crate::asset_drop::cancelled(world.get_resource::<ButtonInput<KeyCode>>())
    {
        return;
    }
    let catalog = world.resource::<ProjectEffectCatalog>();
    let session = world.resource::<EditorSession>();
    let entries = entries(target, catalog, session);
    let guard = IoGuard::capture(catalog, session);
    let return_focus = world.get_resource::<InputFocus>().and_then(InputFocus::get);
    let title = match target {
        DropTarget::Texture(_) => "Choose Texture",
        DropTarget::Mesh(_) => "Choose Mesh",
        _ => "Choose Material",
    };
    let mut search = Entity::PLACEHOLDER;
    let mut list = Entity::PLACEHOLDER;
    let mut error = Entity::PLACEHOLDER;
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
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: Val::Px(470.0),
                    max_width: Val::Percent(95.0),
                    max_height: Val::Percent(85.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(8.0),
                    padding: UiRect::all(Val::Px(12.0)),
                    ..default()
                },
                BackgroundColor(theme::PANEL),
            ))
            .with_children(|panel| {
                panel.spawn((Text::new(title), TextColor(theme::TEXT)));
                search = crate::feathers::text_input::spawn_text_input(
                    panel,
                    "",
                    "Search compatible assets",
                    Search,
                );
                panel
                    .commands()
                    .entity(search)
                    .entry::<Node>()
                    .and_modify(|mut node| {
                        node.width = Val::Percent(100.0);
                        node.min_height = Val::Px(28.0);
                        node.flex_shrink = 0.0;
                    });
                panel
                    .spawn(Node {
                        height: Val::Px(280.0),
                        min_height: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|row| {
                        list = row
                            .spawn((
                                Node {
                                    flex_grow: 1.0,
                                    min_width: Val::Px(0.0),
                                    overflow: Overflow::scroll_y(),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: Val::Px(2.0),
                                    scrollbar_width: 0.0,
                                    ..default()
                                },
                                bevy::ui_widgets::ScrollArea,
                            ))
                            .id();
                        crate::feathers::scroll::spawn_vertical_scrollbar(row, list);
                    });
                error = panel
                    .spawn((Text::default(), TextColor(theme::TEXT_MUTED)))
                    .id();
                crate::feathers::button::spawn_action_button(
                    panel,
                    "Cancel",
                    Choice::Cancel,
                    false,
                );
            });
        })
        .id();
    world.resource_mut::<Picker>().0 = Some(Pending {
        target,
        guard,
        overlay,
        search,
        list,
        error,
        entries,
        query: String::new(),
        focused: false,
        return_focus,
    });
    world
        .resource_mut::<DocumentProtectionState>()
        .asset_create_open = true;
    render_results(world);
}

fn matching(pending: &Pending) -> Vec<usize> {
    let query = pending.query.trim().to_lowercase();
    pending
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.label.to_lowercase().contains(&query))
        .map(|(i, _)| i)
        .collect()
}

fn render_results(world: &mut World) {
    let pending = world.resource::<Picker>().0.as_ref().unwrap();
    let entries: Vec<_> = matching(pending)
        .into_iter()
        .map(|i| (i, pending.entries[i].label.clone()))
        .collect();
    let list = pending.list;
    world
        .commands()
        .entity(list)
        .despawn_related::<Children>()
        .with_children(|list| {
            if entries.is_empty() {
                list.spawn((
                    Text::new("No compatible assets match this search."),
                    TextColor(theme::TEXT_MUTED),
                ));
            }
            for (index, label) in entries {
                list.spawn(Node {
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|row| {
                    crate::feathers::button::spawn_action_button(
                        row,
                        &label,
                        Choice::Select(index),
                        false,
                    );
                });
            }
        });
}

fn search(
    change: On<ValueChange<String>>,
    fields: Query<(), With<Search>>,
    mut commands: Commands,
) {
    if !fields.contains(change.source) {
        return;
    }
    let value = change.value.clone();
    commands.queue(move |world: &mut World| {
        let mut picker = world.resource_mut::<Picker>();
        let Some(pending) = picker.0.as_mut() else {
            return;
        };
        if pending.query != value {
            pending.query = value;
            render_results(world);
        }
    });
}

fn close(world: &mut World) {
    if let Some(pending) = world.resource_mut::<Picker>().0.take() {
        world.commands().entity(pending.overlay).try_despawn();
        world
            .resource_mut::<DocumentProtectionState>()
            .asset_create_open = false;
        if let Some(entity) = pending
            .return_focus
            .filter(|entity| world.get_entity(*entity).is_ok())
            && let Some(mut focus) = world.get_resource_mut::<InputFocus>()
        {
            focus.set(entity, FocusCause::Navigated);
        }
    }
}

fn local_plan(
    selection: &Selection,
    target: DropTarget,
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Result<Assignment, String> {
    match (selection, target) {
        (Selection::Source(payload), target) => {
            super::asset_drop::plan_target(payload, target, catalog, session)
        }
        (Selection::Texture(asset), DropTarget::Texture(target)) => {
            super::texture_drop::plan_local(target, *asset, catalog, session)
        }
        _ => Err("Selection is incompatible with this field".into()),
    }
}

fn select(world: &mut World, index: usize) {
    let Some(pending) = world.resource::<Picker>().0.as_ref() else {
        return;
    };
    if crate::asset_drop::cancelled(world.get_resource::<ButtonInput<KeyCode>>())
        || !pending.guard.matches(
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
        )
    {
        close(world);
        return;
    }
    let Some(entry) = pending.entries.get(index) else {
        return;
    };
    let selection = entry.selection.clone();
    let target = pending.target;
    let error = pending.error;
    let mut protection = world.resource::<DocumentProtectionState>().clone();
    protection.asset_create_open = false;
    if protection.is_open() || !io::idle_world(world) {
        close(world);
        return;
    }
    let plan = local_plan(
        &selection,
        target,
        world.resource::<ProjectEffectCatalog>(),
        world.resource::<EditorSession>(),
    );
    match plan {
        Err(message) => {
            if let Some(mut text) = world.get_mut::<Text>(error) {
                text.0 = message;
            }
        }
        Ok(plan) => {
            close(world);
            if let Selection::Source(payload) = selection {
                world.commands().trigger(AssignAsset { payload, target });
            } else {
                let mut session = world.resource_mut::<EditorSession>();
                if let Some(transaction) = plan.transaction {
                    if session.execute_transaction(transaction, true) {
                        session.material_history_active = false;
                    }
                } else {
                    session.status = plan.label;
                }
            }
        }
    }
}

fn choose(event: On<Activate>, choices: Query<&Choice>, mut commands: Commands) {
    if let Ok(choice) = choices.get(event.entity).copied() {
        commands.queue(move |world: &mut World| match choice {
            Choice::Cancel => close(world),
            Choice::Select(index) => select(world, index),
        });
    }
}

fn keyboard(
    mut event: On<FocusedInput<KeyboardInput>>,
    picker: Res<Picker>,
    parents: Query<&ChildOf>,
    mut commands: Commands,
) {
    let Some(pending) = picker.0.as_ref() else {
        return;
    };
    if event.input.state != ButtonState::Pressed {
        return;
    }
    if event.input.key_code == KeyCode::Escape {
        event.propagate(false);
        commands.queue(close);
    } else if event.input.key_code == KeyCode::Enter
        && parents
            .iter_ancestors(event.event_target())
            .any(|e| e == pending.search)
    {
        event.propagate(false);
        commands.queue(|world: &mut World| {
            if let Some(index) = world
                .resource::<Picker>()
                .0
                .as_ref()
                .and_then(|p| matching(p).first().copied())
            {
                select(world, index);
            }
        });
    }
}

fn outside(event: On<Pointer<Click>>, picker: Res<Picker>, mut commands: Commands) {
    if event.button == PointerButton::Primary
        && picker
            .0
            .as_ref()
            .is_some_and(|p| p.overlay == event.original_event_target())
    {
        commands.queue(close);
    }
}

fn sync(world: &mut World) {
    let Some(pending) = world.resource::<Picker>().0.as_ref() else {
        return;
    };
    if crate::asset_drop::cancelled(world.get_resource::<ButtonInput<KeyCode>>())
        || !pending.guard.matches(
            world.resource::<ProjectEffectCatalog>(),
            world.resource::<EditorSession>(),
        )
    {
        close(world);
        return;
    }
    if !pending.focused {
        let search = pending.search;
        let input = world
            .query::<(Entity, &ChildOf, &EditableText)>()
            .iter(world)
            .find(|(_, parent, _)| parent.parent() == search)
            .map(|(e, _, _)| e);
        if let Some(input) = input
            && let Some(mut focus) = world.get_resource_mut::<InputFocus>()
        {
            focus.set(input, FocusCause::Navigated);
            world.resource_mut::<Picker>().0.as_mut().unwrap().focused = true;
        }
    }
}
