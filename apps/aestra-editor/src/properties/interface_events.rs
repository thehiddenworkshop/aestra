//! Declared event inputs and outputs in the Interface section (event system E1): add, rename and
//! remove the events an effect promises to accept and raise, and edit their typed payloads.

use super::*;
use aestra_core::{
    EventDefinition, EventDefinitionId, EventDirection, EventField, EventFieldId, EventFieldType,
    RESERVED_EVENT_NAMES,
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
}

/// The name input of a declared event, or of one of its fields.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum EventNameControl {
    Event(EventDefinitionId),
    Field(EventDefinitionId, EventFieldId),
}

pub(super) fn register(app: &mut App) {
    app.add_observer(activate).add_observer(rename);
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
        spawn_declared_event(parent, definition, direction, localizer);
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
}

fn spawn_declared_event(
    parent: &mut ChildSpawnerCommands,
    definition: &EventDefinition,
    direction: EventDirection,
    localizer: &Localizer,
) {
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
                    row.spawn_empty()
                        .apply_scene(label_dim(localizer.text("interface-event-input-unhandled")));
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
}
