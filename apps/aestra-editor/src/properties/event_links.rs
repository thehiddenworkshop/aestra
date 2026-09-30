//! Particle event links in the Properties panel (event system E0): an emitter's outgoing and
//! incoming links, the link inspector (trigger, target, spawn count, inherited velocity), and the
//! Module Stack's note on emitters that spawn only from their links.

use super::*;
use crate::session::EventLinkError;
use aestra_core::{EventId, EventLink, MAX_EVENT_LINK_COUNT, MODULE_EMISSION, MODULE_SHAPE};

/// A link edit a menu carries.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(super) enum EventLinkAction {
    SetTrigger(EventId, EventTrigger),
    SetTarget(EventId, EmitterId),
}

/// A numeric link field: the spawn count, or the inherited velocity in percent.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventLinkNumberControl {
    Count(EventId),
    InheritPercent(EventId),
}

pub(super) fn register(app: &mut App) {
    app.add_observer(activate)
        .add_observer(change_number)
        .add_systems(Update, sync_number_inputs.in_set(PropertiesSet::Sync));
}

const TRIGGERS: [EventTrigger; 3] = [
    EventTrigger::OnSpawn,
    EventTrigger::OnDeath,
    EventTrigger::OnCollision,
];

/// Whether a module does not run on an emitter that spawns only from its event links: its rate and
/// bursts, and its spawn shape (a child starts where its event happened).
pub(super) fn unused_on_sub_emitter(module: &ModuleInstance) -> bool {
    matches!(
        module.module_type.0.as_str(),
        MODULE_EMISSION | MODULE_SHAPE
    )
}

/// Whether some link or input route (event system E3) targets `emitter`, which then spawns only
/// from them.
pub(super) fn is_sub_emitter(effect: &EffectAsset, emitter: EmitterId) -> bool {
    effect.events.iter().any(|event| event.target == emitter)
        || effect
            .input_spawns
            .iter()
            .any(|route| route.target == emitter)
}

/// `Input Detonate ×48`: the input routes spawning `emitter`.
fn incoming_route_labels(
    effect: &EffectAsset,
    emitter: EmitterId,
    localizer: &Localizer,
) -> Vec<String> {
    effect
        .input_spawns
        .iter()
        .filter(|route| route.target == emitter)
        .map(|route| {
            let mut args = FluentArgs::new();
            args.set(
                "input",
                effect
                    .event_inputs
                    .iter()
                    .find(|input| input.id == route.input)
                    .map_or_else(|| route.input.to_string(), |input| input.name.clone()),
            );
            format!(
                "{} ×{}",
                localizer.text_with("properties-input-route-incoming", &args),
                route.count
            )
        })
        .collect()
}

fn spawn_route_label(parent: &mut ChildSpawnerCommands, label: String) {
    parent.spawn((
        Text::new(label),
        ThemedText,
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        Node {
            margin: UiRect::horizontal(Val::Px(8.0)),
            ..default()
        },
    ));
}

fn emitter_name(session: &EditorSession, id: EmitterId) -> String {
    session
        .effect
        .emitters
        .iter()
        .find(|emitter| emitter.id == id)
        .map_or_else(|| id.to_string(), |emitter| emitter.name.clone())
}

/// `On death → Stars ×48`, seen from the source.
fn outgoing_label(session: &EditorSession, link: &EventLink, localizer: &Localizer) -> String {
    let mut args = FluentArgs::new();
    args.set("trigger", localized_event_trigger(localizer, link.trigger));
    args.set("target", emitter_name(session, link.target));
    format!(
        "{} ×{}",
        localizer.text_with("properties-event-link", &args),
        link.count
    )
}

/// `Rocket · On death ×48`, seen from the target.
fn incoming_label(session: &EditorSession, link: &EventLink, localizer: &Localizer) -> String {
    let mut args = FluentArgs::new();
    args.set("source", emitter_name(session, link.source));
    args.set("trigger", localized_event_trigger(localizer, link.trigger));
    format!(
        "{} ×{}",
        localizer.text_with("properties-event-link-incoming", &args),
        link.count
    )
}

fn drops_label(dropped: u64, localizer: &Localizer) -> String {
    let mut args = FluentArgs::new();
    args.set("dropped", dropped);
    localizer.text_with("properties-event-link-drops", &args)
}

fn section_heading(parent: &mut ChildSpawnerCommands, text: String) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
            ..default()
        },
    ));
}

/// A selectable link row: clicking it opens the link inspector.
fn spawn_link_row(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    link: &EventLink,
    label: String,
    removable: bool,
    localizer: &Localizer,
) {
    let target = SemanticTarget::Event(link.id);
    let selected = session.selection.primary == target;
    let dropped = session.event_link_drops.get(&link.id).copied();
    parent
        .spawn((
            PropertiesSemanticTarget {
                target,
                base_border: theme::BORDER,
            },
            PropertiesSelectionTarget(target),
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(30.0),
                padding: UiRect::horizontal(Val::Px(8.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(if selected {
                theme::SELECTION
            } else {
                theme::PANEL_LIGHT
            }),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            if let Some(dropped) = dropped {
                row.spawn((
                    Text::new(drops_label(dropped, localizer)),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::PLAYHEAD),
                    Pickable::IGNORE,
                ));
            }
            if removable {
                mini_button(row, "×", PropertiesAction::DeleteEventLink(link.id));
            }
        });
}

/// The emitter's event links: those it raises (with the menu adding more) and those that spawn it.
pub(super) fn spawn_event_links(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let Some(selected_layer) = session.selected_layer() else {
        return;
    };
    let source = selected_layer.id;
    section_heading(parent, localizer.text("properties-events"));
    let outgoing = session
        .effect
        .events
        .iter()
        .filter(|event| event.source == source)
        .collect::<Vec<_>>();
    if outgoing.is_empty() {
        parent
            .spawn_empty()
            .apply_scene(label_dim(localizer.text("properties-events-empty")));
    }
    for link in outgoing {
        let label = outgoing_label(session, link, localizer);
        spawn_link_row(parent, session, link, label, true, localizer);
    }

    let mut options = Vec::new();
    for target in session
        .effect
        .emitters
        .iter()
        .filter(|emitter| emitter.id != source)
    {
        for trigger in TRIGGERS {
            if session.effect.events.iter().any(|event| {
                event.source == source && event.target == target.id && event.trigger == trigger
            }) {
                continue;
            }
            let mut args = FluentArgs::new();
            args.set("trigger", localized_event_trigger(localizer, trigger));
            args.set("target", target.name.clone());
            options.push(ComboOption {
                label: localizer.text_with("properties-event-link", &args),
                selected: false,
                action: PropertiesAction::AddEventLink {
                    trigger,
                    target: target.id,
                },
            });
        }
    }
    if options.is_empty() {
        parent
            .spawn_empty()
            .apply_scene(label_dim(localizer.text("properties-events-no-targets")));
    } else {
        parent
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::all(Val::Px(5.0)),
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                },
                EditorTooltip::description(localizer.text("properties-events-add-description")),
            ))
            .with_children(|row| {
                spawn_combo_control(
                    row,
                    &localizer.text("properties-events-add"),
                    &localizer.text("properties-events-add-description"),
                    &options,
                    230.0,
                );
            });
    }

    let incoming = session
        .effect
        .events
        .iter()
        .filter(|event| event.target == source)
        .collect::<Vec<_>>();
    let routes = incoming_route_labels(&session.effect, source, localizer);
    if !incoming.is_empty() || !routes.is_empty() {
        section_heading(parent, localizer.text("properties-events-incoming"));
        parent.spawn_empty().apply_scene(label_dim(
            localizer.text("properties-sub-emitter-description"),
        ));
        for link in incoming {
            let label = incoming_label(session, link, localizer);
            spawn_link_row(parent, session, link, label, false, localizer);
        }
        for label in routes {
            spawn_route_label(parent, label);
        }
    }
}

/// The Module Stack's note under an emitter that spawns only from its links, listing them.
pub(super) fn spawn_sub_emitter_note(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    emitter: EmitterId,
    localizer: &Localizer,
) {
    let incoming = session
        .effect
        .events
        .iter()
        .filter(|event| event.target == emitter)
        .collect::<Vec<_>>();
    let routes = incoming_route_labels(&session.effect, emitter, localizer);
    if incoming.is_empty() && routes.is_empty() {
        return;
    }
    parent
        .spawn((
            Node {
                width: Val::Auto,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::all(Val::Px(6.0)),
                margin: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(4.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL_DARK),
            BorderColor::all(theme::ACCENT_DIM),
            EditorTooltip::description(localizer.text("properties-sub-emitter-description")),
        ))
        .with_children(|note| {
            note.spawn((
                Text::new(localizer.text("properties-sub-emitter")),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(theme::ACCENT),
            ));
            for link in incoming {
                let label = incoming_label(session, link, localizer);
                spawn_link_row(note, session, link, label, false, localizer);
            }
            for label in routes {
                spawn_route_label(note, label);
            }
        });
}

/// The Event Link inspector, shown when a link is selected.
pub(super) fn spawn_event_link_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    link: &EventLink,
    localizer: &Localizer,
) {
    let trigger_options = TRIGGERS.map(|trigger| ComboOption {
        label: localized_event_trigger(localizer, trigger),
        selected: trigger == link.trigger,
        action: EventLinkAction::SetTrigger(link.id, trigger),
    });
    let target_options = session
        .effect
        .emitters
        .iter()
        .filter(|emitter| emitter.id != link.source)
        .map(|emitter| ComboOption {
            label: emitter.name.clone(),
            selected: emitter.id == link.target,
            action: EventLinkAction::SetTarget(link.id, emitter.id),
        })
        .collect::<Vec<_>>();
    parent
        .spawn((
            PropertiesSemanticTarget {
                target: SemanticTarget::Event(link.id),
                base_border: theme::BORDER,
            },
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(7.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(theme::PANEL),
            BorderColor::all(theme::BORDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(localizer.text("properties-event-link-inspector")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            let row = |card: &mut ChildSpawnerCommands,
                       title: String,
                       description: String,
                       content: &mut dyn FnMut(&mut ChildSpawnerCommands)| {
                crate::feathers::field_row::spawn_field_row(
                    card,
                    crate::feathers::field_row::FieldRowProps::new(&title)
                        .with_control_min_width(150.0),
                    EditorTooltip::description(description),
                    |controls| content(controls),
                );
            };
            row(
                card,
                localizer.text("properties-event-link-source"),
                localizer.text("properties-event-link-source-description"),
                &mut |controls| {
                    controls.spawn((
                        Text::new(emitter_name(session, link.source)),
                        ThemedText,
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                    ));
                },
            );
            let trigger_title = localizer.text("properties-event-link-trigger");
            row(
                card,
                trigger_title.clone(),
                localizer.text("properties-event-link-trigger-description"),
                &mut |controls| {
                    spawn_combo_control(
                        controls,
                        &localized_event_trigger(localizer, link.trigger),
                        &trigger_title,
                        &trigger_options,
                        150.0,
                    );
                },
            );
            let target_title = localizer.text("properties-event-link-target");
            row(
                card,
                target_title.clone(),
                localizer.text("properties-event-link-target-description"),
                &mut |controls| {
                    spawn_combo_control(
                        controls,
                        &emitter_name(session, link.target),
                        &target_title,
                        &target_options,
                        150.0,
                    );
                },
            );
            for (title, description, control) in [
                (
                    "properties-event-link-count",
                    "properties-event-link-count-description",
                    EventLinkNumberControl::Count(link.id),
                ),
                (
                    "properties-event-link-inherit",
                    "properties-event-link-inherit-description",
                    EventLinkNumberControl::InheritPercent(link.id),
                ),
            ] {
                let title = localizer.text(title);
                let mut args = FluentArgs::new();
                args.set("max", MAX_EVENT_LINK_COUNT);
                let description = localizer.text_with(description, &args);
                row(card, title.clone(), description, &mut |controls| {
                    controls
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_integer_input())
                        .insert((control, AccessibleLabel(title.clone())));
                });
            }
            if let Some(&dropped) = session.event_link_drops.get(&link.id) {
                card.spawn((
                    Text::new(format!(
                        "{} — {}",
                        drops_label(dropped, localizer),
                        localizer.text("properties-event-link-drops-description")
                    )),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(theme::PLAYHEAD),
                ));
            }
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|row| {
                spawn_feathers_action_button(
                    row,
                    &localizer.text("properties-event-link-delete"),
                    PropertiesAction::DeleteEventLink(link.id),
                    false,
                );
            });
        });
}

fn report(
    session: &mut EditorSession,
    localizer: &Localizer,
    result: Result<bool, EventLinkError>,
) {
    let status = match result {
        Ok(true) => PropertiesStatus::Updated(localizer.text("properties-event-link-inspector")),
        Ok(false) => {
            // A clamped or unchanged value: show the link's actual value again.
            session.ui_revision += 1;
            return;
        }
        Err(EventLinkError::SameEmitter) => PropertiesStatus::EventSelfTarget,
        Err(EventLinkError::Duplicate) => PropertiesStatus::EventDuplicate,
        Err(EventLinkError::TargetMissing) => PropertiesStatus::EventTargetMissing,
    };
    set_properties_status(session, localizer, status);
}

fn activate(
    event: On<Activate>,
    actions: Query<&EventLinkAction>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    let Ok(action) = actions.get(event.entity) else {
        return;
    };
    let result = match *action {
        EventLinkAction::SetTrigger(id, trigger) => {
            session.set_event_link(id, |link| link.trigger = trigger)
        }
        EventLinkAction::SetTarget(id, target) => {
            session.set_event_link(id, |link| link.target = target)
        }
    };
    report(&mut session, &localizer, result);
}

fn change_number(
    change: On<ValueChange<i32>>,
    controls: Query<&EventLinkNumberControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let value = change.value;
    let result = match *control {
        EventLinkNumberControl::Count(id) => {
            session.set_event_link(id, |link| link.count = value.max(1) as u32)
        }
        EventLinkNumberControl::InheritPercent(id) => {
            session.set_event_link(id, |link| link.inherit_velocity = value as f32 / 100.0)
        }
    };
    report(&mut session, &localizer, result);
}

fn sync_number_inputs(
    mut commands: Commands,
    session: Res<EditorSession>,
    controls: Query<(Entity, &EventLinkNumberControl), Added<EventLinkNumberControl>>,
) {
    for (entity, control) in &controls {
        let (EventLinkNumberControl::Count(id) | EventLinkNumberControl::InheritPercent(id)) =
            *control;
        let Some(link) = session.effect.events.iter().find(|event| event.id == id) else {
            continue;
        };
        let value = match control {
            EventLinkNumberControl::Count(_) => link.count as i32,
            EventLinkNumberControl::InheritPercent(_) => {
                (link.inherit_velocity * 100.0).round() as i32
            }
        };
        commands.trigger(UpdateNumberInput {
            entity,
            value: NumberInputValue::I32(value),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with_link() -> (EditorSession, EventId, EmitterId, EmitterId) {
        let mut session = crate::test_support::session_with_timing_slack();
        session.duplicate_selected_layer();
        let rocket = session.effect.emitters[0].id;
        let stars = session.effect.emitters[1].id;
        session.selection.primary = SemanticTarget::Emitter(rocket);
        let id = session
            .add_event_link(EventTrigger::OnDeath, stars)
            .expect("the link is added");
        (session, id, rocket, stars)
    }

    #[test]
    fn a_link_is_edited_in_place_and_undone() {
        let (mut session, id, _, _) = session_with_link();
        assert_eq!(
            session.set_event_link(id, |link| {
                link.count = 48;
                link.inherit_velocity = 0.2;
            }),
            Ok(true)
        );
        let link = session.effect.events[0].clone();
        assert_eq!((link.count, link.inherit_velocity), (48, 0.2));
        assert_eq!(link.id, id);
        // Clamped to the authored range; unchanged is not an edit.
        assert_eq!(
            session.set_event_link(id, |link| link.count = 1_000),
            Ok(true)
        );
        assert_eq!(session.effect.events[0].count, MAX_EVENT_LINK_COUNT);
        assert_eq!(
            session.set_event_link(id, |link| link.count = 1_000),
            Ok(false)
        );
        session.undo();
        assert_eq!(session.effect.events[0].count, 48);
    }

    #[test]
    fn invalid_edits_are_refused() {
        let (mut session, id, rocket, stars) = session_with_link();
        assert_eq!(
            session.set_event_link(id, |link| link.target = rocket),
            Err(EventLinkError::SameEmitter)
        );
        session.selection.primary = SemanticTarget::Emitter(rocket);
        session
            .add_event_link(EventTrigger::OnCollision, stars)
            .unwrap();
        assert_eq!(
            session.set_event_link(id, |link| link.trigger = EventTrigger::OnCollision),
            Err(EventLinkError::Duplicate)
        );
        assert_eq!(session.effect.events[0].trigger, EventTrigger::OnDeath);
    }

    #[test]
    fn a_selected_link_keeps_its_source_emitter_and_marks_the_target() {
        let (mut session, id, rocket, stars) = session_with_link();
        session.selection.primary = SemanticTarget::Event(id);
        assert_eq!(session.selected_layer().map(|layer| layer.id), Some(rocket));
        assert!(is_sub_emitter(&session.effect, stars));
        assert!(!is_sub_emitter(&session.effect, rocket));
        let unused = session.effect.emitters[1]
            .modules
            .iter()
            .filter(|module| unused_on_sub_emitter(module))
            .count();
        assert!(unused > 0, "the rate and shape modules do not run");
    }

    #[test]
    fn dropped_children_become_navigable_warnings() {
        let (mut session, id, _, _) = session_with_link();
        assert!(session.preview_runtime_report().diagnostics.is_empty());
        session.event_link_drops.insert(id, 3072);
        let report = session.preview_runtime_report();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].path, "effect.events[0].count");
        assert!(report.diagnostics[0].message.contains("3072"));
    }
}
