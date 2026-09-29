//! The effect's Interface section (host bindings HB11): what a game sets, binds, hears and supplies
//! to play the effect — its parameters, bindings, output events and world requirements — in the
//! words of the effect, never of an engine. Also the binding-field picker of host-bound module
//! inputs.

use super::*;
use crate::preview_mocks::PreviewMock;
use aestra_compiler::BindingKindRegistry;
use aestra_core::{
    BindingFieldId, BindingId, BindingUpdateMode, EffectBinding, HostFieldRef,
    HostInputAvailability, ModuleInstance, RESERVED_SELF_BINDING,
};
use std::sync::LazyLock;

/// The binding kinds of the built-in registry and every linked extension.
static BINDING_KINDS: LazyLock<BindingKindRegistry> =
    LazyLock::new(|| aestra_compiler::ExtensionRegistry::linked().bindings);

/// An Interface edit. Field indices are small and `Copy`, so menus can carry them.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub(super) enum InterfaceAction {
    AddBinding,
    RemoveBinding(BindingId),
    SetUpdateMode(BindingId, BindingUpdateMode),
    SetRequired(BindingId, bool),
    /// Declares the `field`-th field of the binding's kind, as optional.
    AddField {
        binding: BindingId,
        field: u8,
    },
    /// `field` indexes the binding's declared fields, required first.
    SetFieldRequired {
        binding: BindingId,
        field: u8,
        required: bool,
    },
    RemoveField {
        binding: BindingId,
        field: u8,
    },
    /// Makes a module input read a binding field: `binding` `None` declares a new spatial binding;
    /// `field` indexes the binding kind's fields.
    BindInput {
        module: ModuleId,
        input: u8,
        binding: Option<BindingId>,
        field: u8,
    },
    /// Gives a binding an editor-only preview stand-in, or none (host bindings HB11c).
    SetPreviewMock(BindingId, Option<PreviewMock>),
}

/// The name input of a binding.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BindingNameControl(BindingId);

pub(super) fn register(app: &mut App) {
    app.add_observer(activate).add_observer(rename);
}

fn activate(
    event: On<Activate>,
    actions: Query<&InterfaceAction>,
    mut session: ResMut<EditorSession>,
    registry: Res<EditorModuleRegistry>,
    localizer: Res<Localizer>,
) {
    if let Ok(action) = actions.get(event.entity) {
        apply(*action, &mut session, &registry.0, &localizer);
    }
}

fn rename(
    change: On<ValueChange<String>>,
    controls: Query<&BindingNameControl>,
    mut session: ResMut<EditorSession>,
    localizer: Res<Localizer>,
) {
    if !change.is_final {
        return;
    }
    let Ok(BindingNameControl(id)) = controls.get(change.source) else {
        return;
    };
    let name = change.value.trim();
    let Some(binding) = binding(&session.effect, *id) else {
        return;
    };
    if binding.name == name {
        return;
    }
    let taken = session
        .effect
        .bindings
        .iter()
        .any(|other| other.id != *id && other.name == name);
    if name.is_empty() || name == RESERVED_SELF_BINDING || taken {
        session.status = localizer.text("interface-status-binding-name-invalid");
        session.ui_revision += 1;
        return;
    }
    let mut renamed = binding.clone();
    renamed.name = name.to_owned();
    session.execute(
        localizer.text("interface-rename-binding-command"),
        EffectCommand::SetBinding {
            id: *id,
            binding: renamed,
        },
        true,
    );
}

fn binding(effect: &EffectAsset, id: BindingId) -> Option<&EffectBinding> {
    effect.bindings.iter().find(|binding| binding.id == id)
}

/// The binding kind's field ids and value types, in the kind's order.
fn kind_fields(binding: &EffectBinding) -> Vec<(BindingFieldId, ValueType)> {
    BINDING_KINDS
        .get(&binding.kind)
        .map(|kind| {
            kind.fields
                .iter()
                .map(|field| (field.id.clone(), field.value_type))
                .collect()
        })
        .unwrap_or_default()
}

/// A readable field name: its kind's display name, or its id's words.
fn field_name(field: &BindingFieldId) -> String {
    BINDING_KINDS
        .iter()
        .find_map(|kind| kind.field(field))
        .map(|field| field.display_name.clone())
        .unwrap_or_else(|| aestra_runtime::field_label(field.as_str()))
}

fn unique_binding_name(effect: &EffectAsset, base: &str) -> String {
    let taken = |name: &str| effect.bindings.iter().any(|binding| binding.name == name);
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|index| format!("{base} {index}"))
        .find(|name| !taken(name))
        .expect("the unbounded numeric suffix always yields a unique binding name")
}

/// Every module with its scope: the effect's own stages, then each emitter's.
fn scoped_modules(effect: &EffectAsset) -> impl Iterator<Item = (EmitterId, &ModuleInstance)> {
    effect
        .simulation_stages
        .iter()
        .flat_map(|stage| &stage.modules)
        .map(|module| (EmitterId::EFFECT_SCOPE, module))
        .chain(effect.emitters.iter().flat_map(|emitter| {
            emitter
                .modules
                .iter()
                .map(move |module| (emitter.id, module))
        }))
}

/// Whether something other than a module input needs the binding: an emitter attached to it, or a
/// nested effect forwarding it.
fn binding_held(effect: &EffectAsset, id: BindingId) -> bool {
    effect.emitters.iter().any(|emitter| {
        emitter
            .attachment
            .is_some_and(|attachment| attachment.binding == id)
    }) || effect
        .effect_clips
        .iter()
        .any(|clip| clip.binding_forwards.values().any(|parent| *parent == id))
}

/// Whether something reads this field of the binding.
fn field_read(effect: &EffectAsset, id: BindingId, field: &BindingFieldId) -> bool {
    let attached = field.as_str() == aestra_core::AESTRA_FIELD_POSITION
        && effect.emitters.iter().any(|emitter| {
            emitter
                .attachment
                .is_some_and(|attachment| attachment.binding == id)
        });
    attached
        || scoped_modules(effect).any(|(_, module)| {
            module
                .host_bindings
                .values()
                .any(|reference| reference.binding == id && &reference.field == field)
        })
}

/// Applies an Interface edit as one undoable transaction. Returns whether the effect changed. A
/// preview stand-in is session state only: it changes neither the effect nor its history.
pub(super) fn apply(
    action: InterfaceAction,
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    localizer: &Localizer,
) -> bool {
    if let InterfaceAction::SetPreviewMock(id, mock) = action {
        match mock {
            Some(mock) => session.preview_mocks.insert(id, mock),
            None => session.preview_mocks.remove(&id),
        };
        session.ui_revision += 1;
        return false;
    }
    let effect = &session.effect;
    let (label, commands) = match action {
        InterfaceAction::AddBinding => {
            let name =
                unique_binding_name(effect, &localizer.text("interface-binding-default-name"));
            (
                "interface-add-binding-command",
                vec![EffectCommand::AddBinding {
                    binding: EffectBinding::spatial(name, BindingUpdateMode::Live),
                    index: effect.bindings.len(),
                }],
            )
        }
        InterfaceAction::RemoveBinding(id) => {
            if binding_held(effect, id) {
                session.status = localizer.text("interface-status-binding-in-use");
                return false;
            }
            // Inputs reading it go back to their authored value.
            let mut commands = Vec::new();
            for (emitter, module) in scoped_modules(effect) {
                for (parameter, reference) in &module.host_bindings {
                    if reference.binding != id {
                        continue;
                    }
                    if module.property_source(parameter) == Some(PropertySourceKind::HostBinding) {
                        commands.push(EffectCommand::SetModulePropertySource {
                            emitter,
                            module: module.id,
                            parameter: parameter.clone(),
                            source: PropertySourceKind::Constant,
                        });
                    }
                    commands.push(EffectCommand::SetModuleHostBinding {
                        emitter,
                        module: module.id,
                        parameter: parameter.clone(),
                        field: None,
                    });
                }
            }
            commands.push(EffectCommand::RemoveBinding { id });
            ("interface-remove-binding-command", commands)
        }
        InterfaceAction::SetUpdateMode(id, mode) => {
            let Some(mut changed) = binding(effect, id).cloned() else {
                return false;
            };
            changed.update_mode = mode;
            (
                "interface-edit-binding-command",
                vec![EffectCommand::SetBinding {
                    id,
                    binding: changed,
                }],
            )
        }
        InterfaceAction::SetRequired(id, required) => {
            let Some(mut changed) = binding(effect, id).cloned() else {
                return false;
            };
            changed.required = required;
            (
                "interface-edit-binding-command",
                vec![EffectCommand::SetBinding {
                    id,
                    binding: changed,
                }],
            )
        }
        InterfaceAction::AddField { binding: id, field } => {
            let Some(mut changed) = binding(effect, id).cloned() else {
                return false;
            };
            let Some((field, _)) = kind_fields(&changed).into_iter().nth(field as usize) else {
                return false;
            };
            if changed.fields().any(|declared| *declared == field) {
                return false;
            }
            changed.optional_fields.insert(field);
            (
                "interface-edit-binding-command",
                vec![EffectCommand::SetBinding {
                    id,
                    binding: changed,
                }],
            )
        }
        InterfaceAction::SetFieldRequired {
            binding: id,
            field,
            required,
        } => {
            let Some(mut changed) = binding(effect, id).cloned() else {
                return false;
            };
            let Some(field) = changed.fields().nth(field as usize).cloned() else {
                return false;
            };
            changed.required_fields.remove(&field);
            changed.optional_fields.remove(&field);
            if required {
                changed.required_fields.insert(field);
            } else {
                changed.optional_fields.insert(field);
            }
            (
                "interface-edit-binding-command",
                vec![EffectCommand::SetBinding {
                    id,
                    binding: changed,
                }],
            )
        }
        InterfaceAction::RemoveField { binding: id, field } => {
            let Some(mut changed) = binding(effect, id).cloned() else {
                return false;
            };
            let Some(field) = changed.fields().nth(field as usize).cloned() else {
                return false;
            };
            if field_read(effect, id, &field) {
                session.status = localizer.text("interface-status-field-in-use");
                return false;
            }
            changed.required_fields.remove(&field);
            changed.optional_fields.remove(&field);
            (
                "interface-edit-binding-command",
                vec![EffectCommand::SetBinding {
                    id,
                    binding: changed,
                }],
            )
        }
        InterfaceAction::SetPreviewMock(..) => unreachable!("handled above"),
        InterfaceAction::BindInput {
            module,
            input,
            binding: target,
            field,
        } => {
            let Some((emitter, instance)) = session.owned_module(module) else {
                return false;
            };
            let Some(input) = registry
                .get(&instance.module_type)
                .and_then(|metadata| metadata.inputs.get(input as usize))
            else {
                return false;
            };
            let mut commands = Vec::with_capacity(2);
            let reference = match target {
                Some(id) => {
                    let Some(existing) = binding(effect, id) else {
                        return false;
                    };
                    let Some((field, _)) = kind_fields(existing).into_iter().nth(field as usize)
                    else {
                        return false;
                    };
                    if !existing.fields().any(|declared| *declared == field) {
                        let mut changed = existing.clone();
                        changed.optional_fields.insert(field.clone());
                        commands.push(EffectCommand::SetBinding {
                            id,
                            binding: changed,
                        });
                    }
                    HostFieldRef { binding: id, field }
                }
                None => {
                    let mut created = EffectBinding::spatial(
                        unique_binding_name(
                            effect,
                            &localizer.text("interface-binding-default-name"),
                        ),
                        BindingUpdateMode::Live,
                    );
                    let Some((field, _)) = kind_fields(&created).into_iter().nth(field as usize)
                    else {
                        return false;
                    };
                    created.required_fields = [field.clone()].into();
                    let reference = HostFieldRef {
                        binding: created.id,
                        field,
                    };
                    commands.push(EffectCommand::AddBinding {
                        binding: created,
                        index: effect.bindings.len(),
                    });
                    reference
                }
            };
            if instance.host_bindings.get(input.name) == Some(&reference) {
                return false;
            }
            commands.push(EffectCommand::SetModuleHostBinding {
                emitter,
                module,
                parameter: input.name.to_owned(),
                field: Some(reference),
            });
            ("interface-bind-input-command", commands)
        }
    };
    session.execute_transaction(
        EffectTransaction::new(localizer.text(label), commands),
        true,
    )
}

/// The commands giving a module input a binding field of its type when it switches to the Host
/// Binding source: the first declared field that fits, else a fitting field of a declared binding's
/// kind, else a new spatial binding. `None` when no binding kind has a field of that type.
pub(super) fn default_host_field(
    effect: &EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
    input: &InputMetadata,
    localizer: &Localizer,
) -> Option<Vec<EffectCommand>> {
    let set = |reference: HostFieldRef| EffectCommand::SetModuleHostBinding {
        emitter,
        module,
        parameter: input.name.to_owned(),
        field: Some(reference),
    };
    let fits = |binding: &EffectBinding| {
        kind_fields(binding)
            .into_iter()
            .filter(|(_, value_type)| *value_type == input.value_type)
            .map(|(field, _)| field)
            .collect::<Vec<_>>()
    };
    for binding in &effect.bindings {
        if let Some(field) = fits(binding)
            .into_iter()
            .find(|field| binding.fields().any(|declared| declared == field))
        {
            return Some(vec![set(HostFieldRef {
                binding: binding.id,
                field,
            })]);
        }
    }
    for binding in &effect.bindings {
        if let Some(field) = fits(binding).into_iter().next() {
            let mut changed = binding.clone();
            changed.optional_fields.insert(field.clone());
            return Some(vec![
                EffectCommand::SetBinding {
                    id: binding.id,
                    binding: changed,
                },
                set(HostFieldRef {
                    binding: binding.id,
                    field,
                }),
            ]);
        }
    }
    let mut created = EffectBinding::spatial(
        unique_binding_name(effect, &localizer.text("interface-binding-default-name")),
        BindingUpdateMode::Live,
    );
    let field = fits(&created).into_iter().next()?;
    created.required_fields = [field.clone()].into();
    let reference = HostFieldRef {
        binding: created.id,
        field,
    };
    Some(vec![
        EffectCommand::AddBinding {
            binding: created,
            index: effect.bindings.len(),
        },
        set(reference),
    ])
}

/// The "Bound to" row of a host-bound module input: every binding field of the input's type, plus a
/// new binding.
pub(super) fn spawn_host_field_picker(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let current = module.host_bindings.get(input.name);
    let mut options = Vec::new();
    let mut current_label = localizer.text("interface-unbound");
    for binding in &session.effect.bindings {
        for (index, (field, value_type)) in kind_fields(binding).into_iter().enumerate() {
            if value_type != input.value_type {
                continue;
            }
            let label = format!("{} › {}", binding.name, field_name(&field));
            let selected = current
                .is_some_and(|current| current.binding == binding.id && current.field == field);
            if selected {
                current_label.clone_from(&label);
            }
            options.push(ComboOption {
                label,
                selected,
                action: InterfaceAction::BindInput {
                    module: module.id,
                    input: input_index,
                    binding: Some(binding.id),
                    field: index as u8,
                },
            });
        }
    }
    let spatial = EffectBinding::spatial("", BindingUpdateMode::Live);
    if let Some((index, (field, _))) = kind_fields(&spatial)
        .into_iter()
        .enumerate()
        .find(|(_, (_, value_type))| *value_type == input.value_type)
    {
        let mut args = FluentArgs::new();
        args.set("field", field_name(&field));
        options.push(ComboOption {
            label: localizer.text_with("interface-bind-new", &args),
            selected: false,
            action: InterfaceAction::BindInput {
                module: module.id,
                input: input_index,
                binding: None,
                field: index as u8,
            },
        });
    }
    let title = localizer.text("interface-bound-to");
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new(&title).with_control_min_width(150.0),
        EditorTooltip::description(localizer.text("interface-bound-to-description")),
        |controls| {
            spawn_combo_control(controls, &current_label, &title, &options, 190.0);
        },
    );
}

fn heading(parent: &mut ChildSpawnerCommands, text: String) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.0),
            ..default()
        },
        TextColor(theme::ACCENT),
        Node {
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        },
    ));
}

fn line(parent: &mut ChildSpawnerCommands, text: String) {
    parent.spawn((
        Text::new(text),
        ThemedText,
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
    ));
}

fn availability_label(availability: HostInputAvailability, localizer: &Localizer) -> String {
    localizer.text(match availability {
        HostInputAvailability::TimeAddressable => "interface-availability-time-addressable",
        HostInputAvailability::Recordable => "interface-availability-recordable",
        HostInputAvailability::Checkpointed => "interface-availability-checkpointed",
        HostInputAvailability::ForwardOnly => "interface-availability-forward-only",
    })
}

/// The Interface card, under the effect's details: parameters, bindings (editable), output events,
/// world requirements and input events.
pub(super) fn spawn_interface(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let interface = session
        .preview()
        .map(|preview| preview.effect().interface())
        .unwrap_or_default();
    parent
        .spawn((
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
                Text::new(localizer.text("interface")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            card.spawn_empty()
                .apply_scene(label_dim(localizer.text("interface-description")));

            heading(card, localizer.text("interface-parameters"));
            if interface.parameters.is_empty() {
                card.spawn_empty()
                    .apply_scene(label_dim(localizer.text("interface-parameters-empty")));
            }
            for parameter in &interface.parameters {
                line(
                    card,
                    format!("{} · {:?}", parameter.name, parameter.value_type),
                );
            }

            heading(card, localizer.text("interface-bindings"));
            if session.effect.bindings.is_empty() {
                card.spawn_empty()
                    .apply_scene(label_dim(localizer.text("interface-bindings-empty")));
            }
            for binding in &session.effect.bindings {
                let read = interface
                    .bindings
                    .iter()
                    .any(|candidate| candidate.name == binding.name && candidate.read);
                let mock = session.preview_mocks.get(&binding.id).copied();
                spawn_binding(card, binding, read, mock, localizer);
            }
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|row| {
                spawn_feathers_action_button(
                    row,
                    &localizer.text("interface-add-binding"),
                    InterfaceAction::AddBinding,
                    false,
                );
            });

            heading(card, localizer.text("interface-output-events"));
            if interface.output_events.is_empty() {
                card.spawn_empty()
                    .apply_scene(label_dim(localizer.text("interface-output-events-empty")));
            }
            for event in &interface.output_events {
                let mut args = FluentArgs::new();
                args.set("kind", event.kind.clone());
                args.set("source", event.raised_by.clone());
                let message = match event.channel {
                    aestra_runtime::EventChannel::Runtime => "interface-output-event",
                    aestra_runtime::EventChannel::Timeline => "interface-output-cue",
                };
                line(card, localizer.text_with(message, &args));
            }

            heading(card, localizer.text("interface-world"));
            if interface.world.is_empty() {
                card.spawn_empty()
                    .apply_scene(label_dim(localizer.text("interface-world-empty")));
            }
            for requirement in &interface.world {
                line(
                    card,
                    format!(
                        "{} · {}",
                        requirement.label,
                        availability_label(requirement.availability, localizer)
                    ),
                );
            }

            heading(card, localizer.text("interface-input-events"));
            for input in &interface.input_events {
                let description = match input.as_str() {
                    aestra_runtime::INPUT_RESTART => localizer.text("interface-input-restart"),
                    _ => String::new(),
                };
                line(card, format!("{input} — {description}"));
            }
            card.spawn_empty()
                .apply_scene(label_dim(localizer.text("interface-input-events-playback")));
        });
}

fn spawn_binding(
    parent: &mut ChildSpawnerCommands,
    binding: &EffectBinding,
    read: bool,
    mock: Option<PreviewMock>,
    localizer: &Localizer,
) {
    let kind = BINDING_KINDS
        .get(&binding.kind)
        .map(|kind| kind.display_name.clone())
        .unwrap_or_else(|| aestra_runtime::field_label(binding.kind.as_str()));
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
                        &binding.name,
                        &localizer.text("interface-binding-name"),
                        BindingNameControl(binding.id),
                    );
                });
                row.spawn((
                    Text::new(kind),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(theme::TEXT_MUTED),
                ));
                mini_button(row, "×", InterfaceAction::RemoveBinding(binding.id));
            });

            let mode_label = |mode| {
                localizer.text(match mode {
                    BindingUpdateMode::Live => "interface-mode-live",
                    BindingUpdateMode::SnapshotOnSpawn => "interface-mode-snapshot",
                })
            };
            let modes = [BindingUpdateMode::Live, BindingUpdateMode::SnapshotOnSpawn].map(|mode| {
                ComboOption {
                    label: mode_label(mode),
                    selected: mode == binding.update_mode,
                    action: InterfaceAction::SetUpdateMode(binding.id, mode),
                }
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
                selected: required == binding.required,
                action: InterfaceAction::SetRequired(binding.id, required),
            });
            card.spawn(Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|row| {
                spawn_combo_control(
                    row,
                    &mode_label(binding.update_mode),
                    &localizer.text("interface-mode"),
                    &modes,
                    130.0,
                );
                spawn_combo_control(
                    row,
                    &requirement_label(binding.required),
                    &localizer.text("interface-requirement"),
                    &requirements,
                    110.0,
                );
            });

            // A spatial binding can have a stand-in in the editor's preview.
            if binding.kind.as_str() == aestra_core::AESTRA_BINDING_SPATIAL {
                let mock_label = |mock: Option<PreviewMock>| {
                    localizer.text(mock.map_or("interface-mock-none", PreviewMock::message_id))
                };
                let mocks = std::iter::once(None)
                    .chain(PreviewMock::ALL.map(Some))
                    .map(|choice| ComboOption {
                        label: mock_label(choice),
                        selected: choice == mock,
                        action: InterfaceAction::SetPreviewMock(binding.id, choice),
                    })
                    .collect::<Vec<_>>();
                let title = localizer.text("interface-mock");
                crate::feathers::field_row::spawn_field_row(
                    card,
                    crate::feathers::field_row::FieldRowProps::new(&title)
                        .with_control_min_width(130.0),
                    EditorTooltip::description(localizer.text("interface-mock-description")),
                    |controls| {
                        spawn_combo_control(controls, &mock_label(mock), &title, &mocks, 130.0);
                    },
                );
            }
            for (index, field) in binding.fields().enumerate() {
                let required = binding.required_fields.contains(field);
                let options = [
                    ComboOption {
                        label: localizer.text("interface-required"),
                        selected: required,
                        action: InterfaceAction::SetFieldRequired {
                            binding: binding.id,
                            field: index as u8,
                            required: true,
                        },
                    },
                    ComboOption {
                        label: localizer.text("interface-optional"),
                        selected: !required,
                        action: InterfaceAction::SetFieldRequired {
                            binding: binding.id,
                            field: index as u8,
                            required: false,
                        },
                    },
                    ComboOption {
                        label: localizer.text("interface-remove-field"),
                        selected: false,
                        action: InterfaceAction::RemoveField {
                            binding: binding.id,
                            field: index as u8,
                        },
                    },
                ];
                let name = field_name(field);
                crate::feathers::field_row::spawn_field_row(
                    card,
                    crate::feathers::field_row::FieldRowProps::new(&name)
                        .with_control_min_width(110.0),
                    EditorTooltip::description(localizer.text("interface-field-description")),
                    |controls| {
                        spawn_combo_control(
                            controls,
                            &requirement_label(required),
                            &name,
                            &options,
                            110.0,
                        );
                    },
                );
            }

            let addable = kind_fields(binding)
                .into_iter()
                .enumerate()
                .filter(|(_, (field, _))| !binding.fields().any(|declared| declared == field))
                .map(|(index, (field, _))| ComboOption {
                    label: field_name(&field),
                    selected: false,
                    action: InterfaceAction::AddField {
                        binding: binding.id,
                        field: index as u8,
                    },
                })
                .collect::<Vec<_>>();
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|row| {
                if read {
                    row.spawn(Node::default());
                } else {
                    row.spawn_empty()
                        .apply_scene(label_dim(localizer.text("interface-binding-unread")));
                }
                if !addable.is_empty() {
                    let label = localizer.text("interface-add-field");
                    spawn_combo_control(row, &label, &label, &addable, 130.0);
                }
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A session whose Initialize `direction` (a Vec3 input that accepts host bindings) exists.
    fn direction_input() -> (EditorSession, ModuleRegistry, ModuleId, u8) {
        let session = crate::test_support::session_with_timing_slack();
        let registry = ModuleRegistry::builtin();
        let module = session
            .selected_layer()
            .and_then(|layer| layer.module_by_type(aestra_core::MODULE_INITIALIZE))
            .expect("the test effect has an Initialize module");
        let input = registry
            .get(&module.module_type)
            .and_then(|metadata| {
                metadata
                    .inputs
                    .iter()
                    .position(|input| input.name == "direction")
            })
            .expect("Initialize has a direction input") as u8;
        let module = module.id;
        (session, registry, module, input)
    }

    fn host_field(session: &EditorSession, module: ModuleId) -> Option<HostFieldRef> {
        session
            .owned_module(module)
            .and_then(|(_, module)| module.host_bindings.get("direction").cloned())
    }

    #[test]
    fn switching_an_input_to_a_host_binding_declares_and_binds_a_field() {
        let (mut session, registry, module, input) = direction_input();
        let localizer = Localizer::new("en-US").unwrap();
        assert!(set_module_input_source(
            &mut session,
            &registry,
            module,
            input,
            PropertySourceKind::HostBinding,
            &localizer,
        ));
        assert_eq!(session.effect.bindings.len(), 1);
        let target = &session.effect.bindings[0];
        assert_eq!(target.name, "Target");
        assert_eq!(
            host_field(&session, module),
            Some(HostFieldRef::new(
                target.id,
                aestra_core::AESTRA_FIELD_POSITION
            ))
        );
        let interface = session
            .preview()
            .expect("the bound effect compiles")
            .effect()
            .interface();
        assert!(interface.bindings[0].read);

        // The picker rebinds it to the velocity, declaring that field on the binding.
        let velocity = kind_fields(target)
            .iter()
            .position(|(field, _)| field.as_str() == aestra_core::AESTRA_FIELD_LINEAR_VELOCITY)
            .unwrap() as u8;
        let id = target.id;
        assert!(apply(
            InterfaceAction::BindInput {
                module,
                input,
                binding: Some(id),
                field: velocity,
            },
            &mut session,
            &registry,
            &localizer,
        ));
        assert_eq!(
            host_field(&session, module),
            Some(HostFieldRef::new(
                id,
                aestra_core::AESTRA_FIELD_LINEAR_VELOCITY
            ))
        );
        assert!(session.preview().is_some());

        // Removing the binding returns the input to its authored value, in one undoable step.
        assert!(apply(
            InterfaceAction::RemoveBinding(id),
            &mut session,
            &registry,
            &localizer,
        ));
        assert!(session.effect.bindings.is_empty());
        assert_eq!(host_field(&session, module), None);
        assert_eq!(
            session
                .owned_module(module)
                .and_then(|(_, module)| module.property_source("direction")),
            Some(PropertySourceKind::Constant)
        );
        assert!(session.preview().is_some());
        session.undo();
        assert_eq!(session.effect.bindings.len(), 1);
        assert!(host_field(&session, module).is_some());
    }

    #[test]
    fn bindings_are_named_uniquely_and_fields_in_use_stay() {
        let (mut session, registry, _, _) = direction_input();
        let localizer = Localizer::new("en-US").unwrap();
        for _ in 0..2 {
            assert!(apply(
                InterfaceAction::AddBinding,
                &mut session,
                &registry,
                &localizer,
            ));
        }
        let names = session
            .effect
            .bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["Target", "Target 2"]);
        let id = session.effect.bindings[0].id;

        // An emitter attached to the binding holds it and its position.
        session.effect.emitters[0].attachment = Some(aestra_core::EmitterAttachment {
            binding: id,
            inherit: Default::default(),
        });
        let position = InterfaceAction::RemoveField {
            binding: id,
            field: 0,
        };
        assert!(!apply(position, &mut session, &registry, &localizer));
        assert!(!apply(
            InterfaceAction::RemoveBinding(id),
            &mut session,
            &registry,
            &localizer,
        ));

        assert!(apply(
            InterfaceAction::AddField {
                binding: id,
                field: 2,
            },
            &mut session,
            &registry,
            &localizer,
        ));
        assert_eq!(session.effect.bindings[0].optional_fields.len(), 1);
        assert!(apply(
            InterfaceAction::SetFieldRequired {
                binding: id,
                field: 1,
                required: true,
            },
            &mut session,
            &registry,
            &localizer,
        ));
        assert_eq!(session.effect.bindings[0].required_fields.len(), 2);
        assert!(apply(
            InterfaceAction::SetUpdateMode(id, BindingUpdateMode::SnapshotOnSpawn),
            &mut session,
            &registry,
            &localizer,
        ));
        assert_eq!(
            session.effect.bindings[0].update_mode,
            BindingUpdateMode::SnapshotOnSpawn
        );

        // A preview stand-in is session state: the effect and its history are untouched.
        let before = session.effect.clone();
        assert!(!apply(
            InterfaceAction::SetPreviewMock(id, Some(PreviewMock::FlyBy)),
            &mut session,
            &registry,
            &localizer,
        ));
        assert_eq!(session.preview_mocks.get(&id), Some(&PreviewMock::FlyBy));
        assert_eq!(session.effect, before);
        session.undo();
        assert_eq!(
            session.effect.bindings[0].update_mode,
            BindingUpdateMode::Live,
            "undo skips the stand-in and reverts the last edit"
        );
        apply(
            InterfaceAction::SetPreviewMock(id, None),
            &mut session,
            &registry,
            &localizer,
        );
        assert!(session.preview_mocks.is_empty());
    }
}
