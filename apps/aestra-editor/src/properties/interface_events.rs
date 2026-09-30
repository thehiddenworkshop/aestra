//! Declared event inputs and outputs in the Interface section (event system E1): add, rename and
//! remove the events an effect promises to accept and raise, and edit their typed payloads. A
//! declared input's routes (E3) say what it does: each spawns a burst of an emitter's particles.

use super::*;
use aestra_core::{
    EventDefinition, EventDefinitionId, EventDirection, EventField, EventFieldId, EventFieldType,
    EventRouteId, InputSpawnRoute, MAX_EVENT_LINK_COUNT, RESERVED_EVENT_NAMES,
};

/// A declared-event edit a button or menu carries.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventDeclarationAction {
    Add(EventDirection),
    Remove(EventDefinitionId),
    AddField(EventDefinitionId),
    RemoveField(EventDefinitionId, EventFieldId),
    SetFieldType(EventDefinitionId, EventFieldId, EventFieldType),
    SetFieldRequired(EventDefinitionId, EventFieldId, bool),
    /// Sends a declared input to the preview, with a neutral payload (event system E2).
    Send(EventDefinitionId),
    /// Sends a built-in input to the preview: `stop_emitting` or `kill` (event system E2b).
    SendBuiltIn(BuiltInInput),
    /// Forgets the inputs sent to the preview.
    ClearSent,
    /// Routes a declared input to a burst of the first emitter's particles (event system E3).
    AddRoute(EventDefinitionId),
    RemoveRoute(EventRouteId),
    SetRouteTarget(EventRouteId, EmitterId),
    /// Centers a route's burst on one of its input's `vec3` fields, or on the effect origin.
    SetRoutePosition(EventRouteId, Option<EventFieldId>),
}

/// The particles-per-event input of an input route.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct RouteCountControl(EventRouteId);

/// Particles a new input route spawns per event.
const DEFAULT_ROUTE_COUNT: u32 = 32;

/// A built-in input the preview can be sent (event system E2b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BuiltInInput {
    StopEmitting,
    Kill,
}

impl BuiltInInput {
    pub(super) fn from_name(name: &str) -> Option<Self> {
        match name {
            aestra_runtime::INPUT_STOP_EMITTING => Some(Self::StopEmitting),
            aestra_runtime::INPUT_KILL => Some(Self::Kill),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::StopEmitting => aestra_runtime::INPUT_STOP_EMITTING,
            Self::Kill => aestra_runtime::INPUT_KILL,
        }
    }
}

/// The name input of a declared event, or of one of its fields.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum EventNameControl {
    Event(EventDefinitionId),
    Field(EventDefinitionId, EventFieldId),
}

pub(super) fn register(app: &mut App) {
    app.add_observer(activate)
        .add_observer(rename)
        .add_observer(change_route_count)
        .add_systems(Update, sync_route_counts.in_set(PropertiesSet::Sync));
}

/// The command editing input route `id` as `edit` says, the count kept in range; `None` when the
/// route is gone or the edit changes nothing.
fn route_edit(
    effect: &EffectAsset,
    id: EventRouteId,
    edit: impl FnOnce(&mut InputSpawnRoute),
) -> Option<EffectCommand> {
    let route = effect.input_spawns.iter().find(|route| route.id == id)?;
    let mut changed = route.clone();
    edit(&mut changed);
    changed.count = changed.count.clamp(1, MAX_EVENT_LINK_COUNT);
    (&changed != route).then_some(EffectCommand::SetInputSpawn { id, route: changed })
}

fn definitions(effect: &EffectAsset, direction: EventDirection) -> &[EventDefinition] {
    match direction {
        EventDirection::Input => &effect.event_inputs,
        EventDirection::Output => &effect.event_outputs,
    }
}

/// A declared event and its direction.
fn find(effect: &EffectAsset, id: EventDefinitionId) -> Option<(EventDirection, &EventDefinition)> {
    [EventDirection::Input, EventDirection::Output]
        .into_iter()
        .find_map(|direction| {
            definitions(effect, direction)
                .iter()
                .find(|definition| definition.id == id)
                .map(|definition| (direction, definition))
        })
}

fn unique_name(taken: impl Fn(&str) -> bool, base: &str) -> String {
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|index| format!("{base} {index}"))
        .find(|name| !taken(name))
        .expect("the unbounded numeric suffix always yields a unique name")
}

fn type_label(field_type: EventFieldType) -> &'static str {
    match field_type {
        EventFieldType::Bool => "bool",
        EventFieldType::Int => "int",
        EventFieldType::Float => "float",
        EventFieldType::Vec2 => "vec2",
        EventFieldType::Vec3 => "vec3",
        EventFieldType::Vec4 => "vec4",
        EventFieldType::Color => "color",
        EventFieldType::Binding => "binding",
    }
}

/// Applies a declared-event edit as one undoable step. Returns whether the effect changed.
pub(super) fn apply(
    action: EventDeclarationAction,
    session: &mut EditorSession,
    localizer: &Localizer,
) -> bool {
    match action {
        EventDeclarationAction::Send(id) => {
            if let Some((_, definition)) = find(&session.effect, id) {
                let name = definition.name.clone();
                send(session, &name, localizer);
            }
            return false;
        }
        EventDeclarationAction::SendBuiltIn(input) => {
            send(session, input.name(), localizer);
            return false;
        }
        EventDeclarationAction::ClearSent => {
            session.preview_inputs.clear();
            session.ui_revision += 1;
            return false;
        }
        _ => {}
    }
    let effect = &session.effect;
    let (label, command) = match action {
        EventDeclarationAction::Add(direction) => {
            let base = localizer.text(match direction {
                EventDirection::Input => "interface-event-input-default-name",
                EventDirection::Output => "interface-event-output-default-name",
            });
            let existing = definitions(effect, direction);
            let name = unique_name(
                |name| {
                    existing.iter().any(|definition| definition.name == name)
                        || RESERVED_EVENT_NAMES.contains(&name)
                },
                &base,
            );
            (
                "interface-add-event-command",
                EffectCommand::AddEventDefinition {
                    direction,
                    definition: EventDefinition::new(name),
                    index: existing.len(),
                },
            )
        }
        EventDeclarationAction::Send(_)
        | EventDeclarationAction::SendBuiltIn(_)
        | EventDeclarationAction::ClearSent => {
            unreachable!("handled above")
        }
        EventDeclarationAction::AddRoute(input) => {
            let Some((_, definition)) = find(effect, input) else {
                return false;
            };
            let Some(emitter) = effect.emitters.first() else {
                session.status = localizer.text("interface-status-route-needs-emitter");
                session.ui_revision += 1;
                return false;
            };
            let mut route = InputSpawnRoute::new(input, emitter.id);
            route.count = DEFAULT_ROUTE_COUNT;
            route.position = definition
                .fields
                .iter()
                .find(|field| field.field_type == EventFieldType::Vec3)
                .map(|field| field.id);
            (
                "interface-add-route-command",
                EffectCommand::AddInputSpawn {
                    route,
                    index: effect.input_spawns.len(),
                },
            )
        }
        EventDeclarationAction::RemoveRoute(id) => (
            "interface-remove-route-command",
            EffectCommand::RemoveInputSpawn { id },
        ),
        EventDeclarationAction::SetRouteTarget(id, target) => {
            let Some(command) = route_edit(effect, id, |route| route.target = target) else {
                return false;
            };
            ("interface-edit-route-command", command)
        }
        EventDeclarationAction::SetRoutePosition(id, position) => {
            let Some(command) = route_edit(effect, id, |route| route.position = position) else {
                return false;
            };
            ("interface-edit-route-command", command)
        }
        EventDeclarationAction::Remove(id) => (
            "interface-remove-event-command",
            EffectCommand::RemoveEventDefinition { id },
        ),
        EventDeclarationAction::AddField(id) => {
            let Some((_, definition)) = find(effect, id) else {
                return false;
            };
            let mut changed = definition.clone();
            let name = unique_name(
                |name| changed.fields.iter().any(|field| field.name == name),
                &localizer.text("interface-event-field-default-name"),
            );
            changed
                .fields
                .push(EventField::new(name, EventFieldType::Float));
            (
                "interface-edit-event-command",
                EffectCommand::SetEventDefinition {
                    id,
                    definition: changed,
                },
            )
        }
        EventDeclarationAction::RemoveField(id, field)
        | EventDeclarationAction::SetFieldType(id, field, _)
        | EventDeclarationAction::SetFieldRequired(id, field, _) => {
            let Some((_, definition)) = find(effect, id) else {
                return false;
            };
            let mut changed = definition.clone();
            let Some(index) = changed.fields.iter().position(|item| item.id == field) else {
                return false;
            };
            match action {
                EventDeclarationAction::RemoveField(..) => {
                    changed.fields.remove(index);
                }
                EventDeclarationAction::SetFieldType(_, _, field_type) => {
                    changed.fields[index].field_type = field_type;
                }
                EventDeclarationAction::SetFieldRequired(_, _, required) => {
                    changed.fields[index].required = required;
                }
                _ => unreachable!("matched above"),
            }
            if &changed == definition {
                return false;
            }
            (
                "interface-edit-event-command",
                EffectCommand::SetEventDefinition {
                    id,
                    definition: changed,
                },
            )
        }
    };
    session.execute(localizer.text(label), command, true)
}

/// Sends an input — declared, with a neutral payload, or built in — to the session's preview,
/// recording it for every later seek. It takes effect at the tick after the playhead's: the
/// session's instance can run on a clock of its own (it restarts when the effect recompiles), while
/// the viewport's players follow the playhead.
pub(super) fn send(session: &mut EditorSession, name: &str, localizer: &Localizer) {
    let name = name.to_owned();
    let tick = aestra_runtime::trace_tick(session.simulation_time()) + 1;
    let Some(preview) = session.preview_mut() else {
        session.status = localizer.text("interface-status-send-unavailable");
        return;
    };
    let payload = aestra_runtime::neutral_payload(preview.effect(), &name).unwrap_or_default();
    let result = preview.send_event_at(&name, payload, tick);
    let recorded = preview.received_events().to_vec();
    let mut args = FluentArgs::new();
    args.set("input", name);
    match result {
        Ok(tick) => {
            args.set("tick", tick);
            session.preview_inputs = recorded;
            session.status = localizer.text_with("interface-status-sent", &args);
        }
        Err(error) => {
            args.set("reason", error.to_string());
            session.status = localizer.text_with("interface-status-send-refused", &args);
        }
    }
    session.ui_revision += 1;
}

fn activate(
    event: On<Activate>,
    actions: Query<&EventDeclarationAction>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if let Ok(action) = actions.get(event.entity) {
        apply(*action, &mut session, &localizer);
    }
}

/// Sets an input route's particles per event. Returns whether the effect changed; a value out of
/// range is clamped, and one that changes nothing shows the route's count again.
pub(super) fn set_route_count(
    session: &mut EditorSession,
    id: EventRouteId,
    count: i32,
    localizer: &Localizer,
) -> bool {
    let count = count.max(1) as u32;
    match route_edit(&session.effect, id, |route| route.count = count) {
        Some(command) => session.execute(
            localizer.text("interface-edit-route-command"),
            command,
            true,
        ),
        None => {
            session.ui_revision += 1;
            false
        }
    }
}

fn change_route_count(
    change: On<ValueChange<i32>>,
    controls: Query<&RouteCountControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    if let Ok(RouteCountControl(id)) = controls.get(change.source) {
        set_route_count(&mut session, *id, change.value, &localizer);
    }
}

fn sync_route_counts(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &RouteCountControl), Added<RouteCountControl>>,
) {
    for (entity, RouteCountControl(id)) in &controls {
        if let Some(route) = session
            .effect
            .input_spawns
            .iter()
            .find(|route| route.id == *id)
        {
            commands.trigger(UpdateNumberInput {
                entity,
                value: NumberInputValue::I32(route.count as i32),
            });
        }
    }
}

/// Renames a declared event or one of its fields. Returns whether the effect changed; an invalid
/// name (empty, taken, built-in) is refused with a status.
pub(super) fn rename_declared(
    session: &mut EditorSession,
    target: (EventDefinitionId, Option<EventFieldId>),
    name: &str,
    localizer: &Localizer,
) -> bool {
    let name = name.trim();
    let Some((direction, definition)) = find(&session.effect, target.0) else {
        return false;
    };
    let mut changed = definition.clone();
    let valid = match target.1 {
        None => {
            let taken = definitions(&session.effect, direction)
                .iter()
                .any(|other| other.id != target.0 && other.name == name);
            changed.name = name.to_owned();
            !name.is_empty() && !taken && !RESERVED_EVENT_NAMES.contains(&name)
        }
        Some(field) => {
            let taken = changed
                .fields
                .iter()
                .any(|other| other.id != field && other.name == name);
            let Some(item) = changed.fields.iter_mut().find(|item| item.id == field) else {
                return false;
            };
            item.name = name.to_owned();
            !name.is_empty() && !taken
        }
    };
    let unchanged = &changed == definition;
    if !valid {
        session.status = localizer.text("interface-status-event-name-invalid");
        session.ui_revision += 1;
        return false;
    }
    if unchanged {
        return false;
    }
    session.execute(
        localizer.text("interface-edit-event-command"),
        EffectCommand::SetEventDefinition {
            id: target.0,
            definition: changed,
        },
        true,
    )
}

fn rename(
    change: On<ValueChange<String>>,
    controls: Query<&EventNameControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let target = match *control {
        EventNameControl::Event(id) => (id, None),
        EventNameControl::Field(id, field) => (id, Some(field)),
    };
    rename_declared(&mut session, target, &change.value, &localizer);
}

/// The declared events of one direction, each editable, then the button adding one.
pub(super) fn spawn_declared_events(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    direction: EventDirection,
    localizer: &Localizer,
) {
    for definition in definitions(&session.effect, direction) {
        spawn_declared_event(parent, session, definition, direction, localizer);
    }
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::FlexEnd,
            ..default()
        })
        .with_children(|row| {
            spawn_feathers_action_button(
                row,
                &localizer.text(match direction {
                    EventDirection::Input => "interface-add-event-input",
                    EventDirection::Output => "interface-add-event-output",
                }),
                EventDeclarationAction::Add(direction),
                false,
            );
        });
    if direction == EventDirection::Input && !session.preview_inputs.is_empty() {
        spawn_sent_inputs(parent, session, localizer);
    }
}

/// The inputs sent to the preview, in tick order, and the button forgetting them.
fn spawn_sent_inputs(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    parent
        .spawn_empty()
        .apply_scene(label_dim(localizer.text("interface-sent-inputs")));
    for event in &session.preview_inputs {
        let mut args = FluentArgs::new();
        args.set("tick", event.tick);
        args.set("input", event.input.clone());
        parent.spawn((
            Text::new(localizer.text_with("interface-sent-input", &args)),
            ThemedText,
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
        ));
    }
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::FlexEnd,
            ..default()
        })
        .with_children(|row| {
            spawn_feathers_action_button(
                row,
                &localizer.text("interface-clear-sent-inputs"),
                EventDeclarationAction::ClearSent,
                false,
            );
        });
}

fn spawn_declared_event(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    definition: &EventDefinition,
    direction: EventDirection,
    localizer: &Localizer,
) {
    let routes: Vec<&InputSpawnRoute> = session
        .effect
        .input_spawns
        .iter()
        .filter(|route| route.input == definition.id)
        .collect();
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(6.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_LIGHT),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                })
                .with_children(|name| {
                    spawn_text_input(
                        name,
                        &definition.name,
                        &localizer.text("interface-event-name"),
                        EventNameControl::Event(definition.id),
                    );
                });
                mini_button(row, "×", EventDeclarationAction::Remove(definition.id));
            });
            for field in &definition.fields {
                spawn_field(card, definition.id, field, localizer);
            }
            for route in &routes {
                spawn_route(card, session, definition, route, localizer);
            }
            if direction == EventDirection::Input && routes.is_empty() {
                card.spawn_empty()
                    .apply_scene(label_dim(localizer.text("interface-event-input-unhandled")));
            }
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                if direction == EventDirection::Output {
                    row.spawn_empty()
                        .apply_scene(label_dim(localizer.text("interface-event-output-unraised")));
                } else {
                    row.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|send| {
                        spawn_feathers_action_button(
                            send,
                            &localizer.text("interface-send-event"),
                            EventDeclarationAction::Send(definition.id),
                            true,
                        );
                        send.spawn((
                            Node::default(),
                            EditorTooltip::description(
                                localizer.text("interface-add-route-description"),
                            ),
                        ))
                        .with_children(|add| {
                            spawn_feathers_action_button(
                                add,
                                &localizer.text("interface-add-route"),
                                EventDeclarationAction::AddRoute(definition.id),
                                false,
                            );
                        });
                    });
                }
                spawn_feathers_action_button(
                    row,
                    &localizer.text("interface-add-event-field"),
                    EventDeclarationAction::AddField(definition.id),
                    false,
                );
            });
        });
}

/// One input route: `Spawns [emitter] × [count] at [position] ×`.
fn spawn_route(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    input: &EventDefinition,
    route: &InputSpawnRoute,
    localizer: &Localizer,
) {
    let emitter_name = |id: EmitterId| {
        session
            .effect
            .emitters
            .iter()
            .find(|emitter| emitter.id == id)
            .map_or_else(|| id.to_string(), |emitter| emitter.name.clone())
    };
    let emitters = session
        .effect
        .emitters
        .iter()
        .map(|emitter| ComboOption {
            label: emitter.name.clone(),
            selected: emitter.id == route.target,
            action: EventDeclarationAction::SetRouteTarget(route.id, emitter.id),
        })
        .collect::<Vec<_>>();
    let origin = localizer.text("interface-route-origin");
    let positions = std::iter::once((None, origin.clone()))
        .chain(
            input
                .fields
                .iter()
                .filter(|field| field.field_type == EventFieldType::Vec3)
                .map(|field| (Some(field.id), field.name.clone())),
        )
        .map(|(position, label)| ComboOption {
            label,
            selected: position == route.position,
            action: EventDeclarationAction::SetRoutePosition(route.id, position),
        })
        .collect::<Vec<_>>();
    let position_label = positions
        .iter()
        .find(|option| option.selected)
        .map_or(origin, |option| option.label.clone());
    let mut args = FluentArgs::new();
    args.set("input", input.name.clone());
    let description = localizer.text_with("interface-route-description", &args);
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(5.0),
                row_gap: Val::Px(3.0),
                ..default()
            },
            EditorTooltip::description(description),
        ))
        .with_children(|row| {
            row.spawn_empty()
                .apply_scene(label_dim(localizer.text("interface-route-spawns")));
            spawn_combo_control(
                row,
                &emitter_name(route.target),
                &localizer.text("interface-route-emitter"),
                &emitters,
                110.0,
            );
            let count_title = localizer.text("interface-route-count");
            row.spawn(Node {
                width: Val::Px(58.0),
                ..default()
            })
            .with_children(|count| {
                count
                    .spawn_empty()
                    .apply_scene(ui_shell::feathers_integer_input())
                    .insert((RouteCountControl(route.id), AccessibleLabel(count_title)));
            });
            row.spawn_empty()
                .apply_scene(label_dim(localizer.text("interface-route-at")));
            spawn_combo_control(
                row,
                &position_label,
                &localizer.text("interface-route-position"),
                &positions,
                100.0,
            );
            mini_button(row, "×", EventDeclarationAction::RemoveRoute(route.id));
        });
}

fn spawn_field(
    parent: &mut ChildSpawnerCommands,
    event: EventDefinitionId,
    field: &EventField,
    localizer: &Localizer,
) {
    let types = EventFieldType::ALL.map(|field_type| ComboOption {
        label: type_label(field_type).to_owned(),
        selected: field_type == field.field_type,
        action: EventDeclarationAction::SetFieldType(event, field.id, field_type),
    });
    let requirement_label = |required: bool| {
        localizer.text(if required {
            "interface-required"
        } else {
            "interface-optional"
        })
    };
    let requirements = [true, false].map(|required| ComboOption {
        label: requirement_label(required),
        selected: required == field.required,
        action: EventDeclarationAction::SetFieldRequired(event, field.id, required),
    });
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            })
            .with_children(|name| {
                spawn_text_input(
                    name,
                    &field.name,
                    &localizer.text("interface-event-field-name"),
                    EventNameControl::Field(event, field.id),
                );
            });
            spawn_combo_control(
                row,
                type_label(field.field_type),
                &localizer.text("interface-event-field-type"),
                &types,
                96.0,
            );
            spawn_combo_control(
                row,
                &requirement_label(field.required),
                &localizer.text("interface-requirement"),
                &requirements,
                110.0,
            );
            mini_button(
                row,
                "×",
                EventDeclarationAction::RemoveField(event, field.id),
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn declared_events_are_edited_through_undoable_commands() {
        let mut session = crate::test_support::session_with_timing_slack();
        let localizer = Localizer::new("en-US").unwrap();
        assert!(apply(
            EventDeclarationAction::Add(EventDirection::Output),
            &mut session,
            &localizer
        ));
        assert!(apply(
            EventDeclarationAction::Add(EventDirection::Output),
            &mut session,
            &localizer
        ));
        let names = session
            .effect
            .event_outputs
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Notify", "Notify 2"]);
        let id = session.effect.event_outputs[0].id;

        assert!(rename_declared(
            &mut session,
            (id, None),
            " Exploded ",
            &localizer
        ));
        assert_eq!(session.effect.event_outputs[0].name, "Exploded");
        // Taken, built-in and empty names are refused.
        for name in ["Notify 2", "finished", "  "] {
            assert!(!rename_declared(&mut session, (id, None), name, &localizer));
        }
        assert_eq!(session.effect.event_outputs[0].name, "Exploded");

        assert!(apply(
            EventDeclarationAction::AddField(id),
            &mut session,
            &localizer
        ));
        let field = session.effect.event_outputs[0].fields[0].id;
        assert!(apply(
            EventDeclarationAction::SetFieldType(id, field, EventFieldType::Vec3),
            &mut session,
            &localizer
        ));
        assert!(rename_declared(
            &mut session,
            (id, Some(field)),
            "position",
            &localizer
        ));
        let declared = &session.effect.event_outputs[0].fields[0];
        assert_eq!(
            (declared.name.as_str(), declared.field_type),
            ("position", EventFieldType::Vec3)
        );
        // The compiled interface lists it as a declared output, with its payload.
        let interface = session.preview().unwrap().effect().interface();
        let exploded = interface
            .output_events
            .iter()
            .find(|event| event.kind == "Exploded")
            .expect("the declared output is listed");
        assert_eq!(exploded.channel, aestra_runtime::EventChannel::Declared);
        assert_eq!(exploded.fields[0].name, "position");

        session.undo();
        assert_eq!(session.effect.event_outputs[0].fields[0].name, "value");
        assert!(apply(
            EventDeclarationAction::Remove(id),
            &mut session,
            &localizer
        ));
        assert_eq!(session.effect.event_outputs.len(), 1);
    }

    #[test]
    fn declared_inputs_are_sent_to_the_preview_and_recorded() {
        let mut session = crate::test_support::session_with_timing_slack();
        let localizer = Localizer::new("en-US").unwrap();
        apply(
            EventDeclarationAction::Add(EventDirection::Input),
            &mut session,
            &localizer,
        );
        let id = session.effect.event_inputs[0].id;
        apply(
            EventDeclarationAction::AddField(id),
            &mut session,
            &localizer,
        );
        let history = session.effect.clone();

        apply(EventDeclarationAction::Send(id), &mut session, &localizer);
        assert_eq!(session.preview_inputs.len(), 1);
        assert_eq!(session.preview_inputs[0].input, "Trigger");
        // On the playhead's clock, which the viewport's players follow.
        assert_eq!(
            session.preview_inputs[0].tick,
            aestra_runtime::trace_tick(session.simulation_time()) + 1
        );
        assert_eq!(
            session.preview_inputs[0].payload,
            vec![("value".to_string(), aestra_core::EventValue::Float(0.0))]
        );
        assert_eq!(
            session.preview().unwrap().received_events(),
            session.preview_inputs.as_slice()
        );
        assert_eq!(session.effect, history, "sending is not an edit");

        apply(EventDeclarationAction::ClearSent, &mut session, &localizer);
        assert!(session.preview_inputs.is_empty());
    }

    #[test]
    fn an_input_routes_to_a_burst_the_preview_spawns() {
        let mut session = crate::test_support::session_with_timing_slack();
        let localizer = Localizer::new("en-US").unwrap();
        session.duplicate_selected_layer();
        let (first, second) = (session.effect.emitters[0].id, session.effect.emitters[1].id);
        apply(
            EventDeclarationAction::Add(EventDirection::Input),
            &mut session,
            &localizer,
        );
        let input = session.effect.event_inputs[0].id;
        apply(
            EventDeclarationAction::AddField(input),
            &mut session,
            &localizer,
        );
        let field = session.effect.event_inputs[0].fields[0].id;
        apply(
            EventDeclarationAction::SetFieldType(input, field, EventFieldType::Vec3),
            &mut session,
            &localizer,
        );

        // A new route bursts the first emitter at the input's first vec3 field.
        assert!(apply(
            EventDeclarationAction::AddRoute(input),
            &mut session,
            &localizer
        ));
        let route = session.effect.input_spawns[0].clone();
        assert_eq!(
            (route.input, route.target, route.count, route.position),
            (input, first, DEFAULT_ROUTE_COUNT, Some(field))
        );
        assert!(apply(
            EventDeclarationAction::SetRouteTarget(route.id, second),
            &mut session,
            &localizer
        ));
        assert!(apply(
            EventDeclarationAction::SetRoutePosition(route.id, None),
            &mut session,
            &localizer
        ));
        assert!(set_route_count(&mut session, route.id, 5_000, &localizer));
        let edited = &session.effect.input_spawns[0];
        assert_eq!(
            (edited.target, edited.position, edited.count),
            (second, None, MAX_EVENT_LINK_COUNT)
        );
        assert!(!set_route_count(&mut session, route.id, 5_000, &localizer));

        // The preview compiles the route: the burst's emitter spawns only from it, and a sent
        // input makes a burst of its particles.
        let preview = session.preview().unwrap();
        assert!(preview.effect().is_event_target(1));
        assert!(crate::properties::event_links::is_sub_emitter(
            &session.effect,
            second
        ));
        apply(
            EventDeclarationAction::Send(input),
            &mut session,
            &localizer,
        );
        let bursts = session.preview().unwrap().input_spawn_bursts();
        assert_eq!(bursts.len(), 1);
        assert_eq!(bursts[0].records().count(), MAX_EVENT_LINK_COUNT as usize);

        session.undo();
        assert_eq!(session.effect.input_spawns[0].count, DEFAULT_ROUTE_COUNT);
        assert!(apply(
            EventDeclarationAction::RemoveRoute(route.id),
            &mut session,
            &localizer
        ));
        assert!(session.effect.input_spawns.is_empty());
    }

    #[test]
    fn built_in_inputs_are_sent_to_the_preview() {
        let mut session = crate::test_support::session_with_timing_slack();
        let localizer = Localizer::new("en-US").unwrap();
        apply(
            EventDeclarationAction::SendBuiltIn(BuiltInInput::StopEmitting),
            &mut session,
            &localizer,
        );
        apply(
            EventDeclarationAction::SendBuiltIn(BuiltInInput::Kill),
            &mut session,
            &localizer,
        );
        let sent: Vec<&str> = session
            .preview_inputs
            .iter()
            .map(|event| event.input.as_str())
            .collect();
        assert_eq!(
            sent,
            [
                aestra_runtime::INPUT_STOP_EMITTING,
                aestra_runtime::INPUT_KILL
            ]
        );
        let cutoffs = session.preview().unwrap().emission_cutoffs();
        assert!(cutoffs.stop_tick.is_some() && cutoffs.kill_tick.is_some());
    }
}
