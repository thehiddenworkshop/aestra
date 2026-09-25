//! The Properties panel's selection inspector (extensible-stages M9, §28.1): effect, emitter, module
//! and renderer details for whatever the Module Stack has selected, plus instance labels (§28.5).

use super::*;

/// Renders the inspector body for the current selection (extensible-stages M9, §28.1): effect / emitter
/// settings when an Effect or Emitter stack item is selected, the selected module's or renderer's full
/// controls otherwise, and the emitter's settings as a sensible default. The panel rebuilds on selection
/// change, so this follows the selection made in the Module Stack panel.
#[allow(clippy::too_many_arguments)]
pub(super) fn spawn_selection_inspector(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    registry: &EditorModuleRegistry,
    catalog: &ProjectEffectCatalog,
    material_stack_inspector: &MaterialStackInspectorState,
    asset_server: &AssetServer,
    localizer: &Localizer,
) {
    let layer = session.selected_layer();
    let emitter_index = session.selected_layer_index();
    match session.selection.primary {
        SemanticTarget::Effect(_) => {
            spawn_effect_details(parent, session, localizer);
            return;
        }
        SemanticTarget::Module(id) => {
            // A module of the effect's own simulation stages (fluid F2).
            if let Some((path, module)) = session
                .effect
                .simulation_stages
                .iter()
                .enumerate()
                .find_map(|(stage_index, stage)| {
                    stage
                        .modules
                        .iter()
                        .enumerate()
                        .find(|(_, module)| module.id == id)
                        .map(|(module_index, module)| {
                            (
                                format!(
                                    "effect.simulation_stages[{stage_index}].modules[{module_index}]"
                                ),
                                module,
                            )
                        })
                })
            {
                spawn_module_inspector(
                    parent,
                    module,
                    registry.0.get(&module.module_type),
                    &path,
                    session,
                    localizer,
                    asset_server,
                );
                return;
            }
            if let (Some(layer), Some(emitter_index)) = (layer, emitter_index)
                && let Some((module_index, module)) = layer
                    .modules
                    .iter()
                    .enumerate()
                    .find(|(_, module)| module.id == id)
            {
                spawn_module_inspector(
                    parent,
                    module,
                    registry.0.get(&module.module_type),
                    &format!("effect.emitters[{emitter_index}].modules[{module_index}]"),
                    session,
                    localizer,
                    asset_server,
                );
                return;
            }
        }
        SemanticTarget::Renderer(id) => {
            if let (Some(layer), Some(emitter_index)) = (layer, emitter_index)
                && let Some((renderer_index, renderer)) = layer
                    .renderers
                    .iter()
                    .enumerate()
                    .find(|(_, renderer)| renderer.id == id)
            {
                spawn_renderer_card(
                    parent,
                    renderer,
                    &format!("effect.emitters[{emitter_index}].renderers[{renderer_index}]"),
                    session,
                    catalog,
                    false,
                    material_stack_inspector,
                    asset_server,
                    localizer,
                );
                spawn_instance_label_field(
                    parent,
                    renderer.label.as_deref(),
                    InstanceLabelControl::Renderer(renderer.id),
                );
                return;
            }
        }
        // Emitter selected, or a selection not tied to this emitter: show the emitter's settings.
        _ => {}
    }
    if layer.is_some() {
        spawn_emitter_details(parent, session, localizer);
    } else {
        spawn_effect_details(parent, session, localizer);
    }
}

/// The Effect-item inspector details (extensible-stages M9, §28.1): the effect's name. Shown in the
/// Properties panel when the Effect item is selected in the Module Stack.
pub(super) fn spawn_effect_details(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
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
                Text::new(localizer.text("properties-effect")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_text_field(
                card,
                &localizer.text("properties-effect-name"),
                &localizer.text("properties-effect-name-description"),
                &session.effect.name,
                DocumentTextControl::Effect,
            );
        });
}

/// The Emitter-item inspector details (extensible-stages M9, §28.1): name, enabled, capacity, transform,
/// timing, and event links. Shown in the Properties panel when the Emitter item (or nothing more
/// specific) is selected in the Module Stack.
pub(super) fn spawn_emitter_details(
    parent: &mut ChildSpawnerCommands,
    session: &EditorSession,
    localizer: &Localizer,
) {
    let Some(emitter) = session.selected_layer() else {
        return;
    };
    parent
        .spawn((
            PropertiesSemanticTarget {
                target: SemanticTarget::Emitter(emitter.id),
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
                Text::new(localizer.text("properties-emitter")),
                ThemedText,
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
            ));
            spawn_text_field(
                card,
                &localizer.text("properties-emitter-name"),
                &localizer.text("properties-emitter-name-description"),
                &emitter.name,
                DocumentTextControl::Emitter,
            );
            spawn_document_toggle(
                card,
                &localizer.text("properties-emitter-enabled"),
                &localizer.text("properties-emitter-enabled-description"),
                emitter.enabled,
                DocumentToggleControl::EmitterEnabled,
            );
            crate::feathers::field_row::spawn_field_row(
                card,
                crate::feathers::field_row::FieldRowProps::new(
                    localizer.text("properties-emitter-capacity"),
                )
                .with_control_min_width(150.0),
                EditorTooltip::description(
                    localizer.text("properties-emitter-capacity-description"),
                ),
                |controls| {
                    controls
                        .spawn_empty()
                        .apply_scene(ui_shell::feathers_integer_input())
                        .insert((
                            EmitterCapacityControl,
                            AccessibleLabel(localizer.text("properties-emitter-capacity")),
                        ));
                },
            );
        });
    spawn_emitter_transform_controls(parent);
    spawn_emitter_timing_controls(parent, session, localizer);
    spawn_event_links(parent, session, localizer);
}

/// Which instance an instance-label text field edits (§28.5).
#[derive(Component, Debug, Clone, Copy)]
pub(super) enum InstanceLabelControl {
    Module(ModuleId),
    Renderer(RendererId),
}

/// The optional instance label field (§28.5), shown in the inspector for modules and renderers.
/// Clearing it removes the label; the semantic type is unaffected.
pub(super) fn spawn_instance_label_field(
    parent: &mut ChildSpawnerCommands,
    label: Option<&str>,
    control: InstanceLabelControl,
) {
    crate::feathers::field_row::spawn_field_row(
        parent,
        crate::feathers::field_row::FieldRowProps::new("Label").with_control_min_width(150.0),
        EditorTooltip::description(
            "Optional name telling repeated instances of this type apart, e.g. Collision \
             \"Ground\". Leave empty for none."
                .to_owned(),
        ),
        |inputs| {
            spawn_text_input(inputs, label.unwrap_or_default(), "Instance label", control);
        },
    );
}

/// Commits an edited instance label as one undoable command (§28.5).
pub(super) fn handle_instance_label_change(
    change: On<ValueChange<String>>,
    controls: Query<&InstanceLabelControl>,
    mut session: ResMut<EditorSession>,
) {
    if !change.is_final {
        return;
    }
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let label = aestra_core::normalize_instance_label(Some(&change.value));
    let command = match *control {
        InstanceLabelControl::Module(module) => {
            // An emitter's module, or one in the effect's own simulation stages (fluid F2).
            let Some((emitter, current)) = session.owned_module(module) else {
                return;
            };
            if current.label == label {
                return;
            }
            EffectCommand::SetModuleLabel {
                emitter,
                module,
                label,
            }
        }
        InstanceLabelControl::Renderer(renderer) => {
            let Some(layer) = session.selected_layer() else {
                return;
            };
            let emitter = layer.id;
            let current = layer
                .renderers
                .iter()
                .find(|candidate| candidate.id == renderer)
                .map(|candidate| candidate.label.clone());
            if current.is_none() || current == Some(label.clone()) {
                return;
            }
            EffectCommand::SetRendererLabel {
                emitter,
                renderer,
                label,
            }
        }
    };
    session.execute("Renamed instance", command, true);
}
