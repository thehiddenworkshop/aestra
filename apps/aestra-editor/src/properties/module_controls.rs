use super::*;

pub(crate) type PropertySourceKind = InputSourceKind;

pub(super) fn handle_module_action(
    action: PropertiesAction,
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    palette: &mut ModulePaletteState,
    workspace: &mut CurvesState,
    layout: &mut WorkspaceLayout,
    localizer: &Localizer,
) -> bool {
    match action {
        PropertiesAction::OpenModulePalette(stage) => {
            palette.open = true;
            palette.stage = stage;
            palette.query.clear();
            session.ui_revision += 1;
        }
        PropertiesAction::CloseModulePalette => {
            palette.open = false;
            session.ui_revision += 1;
        }
        PropertiesAction::AddModule(index) => {
            let module = registry
                .iter()
                .nth(index)
                .and_then(|metadata| registry.instantiate(&metadata.type_id));
            if let Some(module) = module {
                session.add_module(module);
                palette.open = false;
            } else {
                set_properties_status(
                    session,
                    localizer,
                    PropertiesStatus::ModuleRegistryUnavailable,
                );
            }
        }
        PropertiesAction::SetModuleChoice {
            module,
            input,
            choice,
        } => set_module_choice(session, registry, module, input, choice, localizer),
        PropertiesAction::MoveModule(id, direction) => {
            session.move_module(id, direction);
        }
        PropertiesAction::DuplicateModule(id) => session.duplicate_module(id),
        PropertiesAction::DeleteModule(id) => {
            if preview_module_deletion(session, id) {
                reveal_dock_panel(layout, session, DockPanel::Changes);
                workspace.clear();
            }
        }
        PropertiesAction::ToggleModuleInputPublic { module, input } => {
            toggle_module_input_public(session, registry, module, input, localizer);
        }
        PropertiesAction::SetModuleInputSource {
            module,
            input,
            source,
        } => {
            set_module_input_source(session, registry, module, input, source, localizer);
        }
        _ => return false,
    }
    true
}

pub(super) fn preview_module_deletion(session: &mut EditorSession, module: ModuleId) -> bool {
    let emitter = session.selected_layer().id;
    session.preview_transaction(EffectTransaction::single(
        "Delete module",
        EffectCommand::RemoveModule { emitter, module },
    ))
}

fn unique_effect_parameter_name_from_base(effect: &EffectAsset, base: &str) -> String {
    if !effect
        .parameters
        .iter()
        .any(|parameter| parameter.name == base)
    {
        return base.to_owned();
    }
    (2..)
        .map(|index| format!("{base} {index}"))
        .find(|name| {
            !effect
                .parameters
                .iter()
                .any(|parameter| &parameter.name == name)
        })
        .expect("the unbounded numeric suffix always yields a unique parameter name")
}

pub(super) fn toggle_module_input_public(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    localizer: &Localizer,
) -> bool {
    let Some((_, input_name)) =
        properties_module_input_target(session, registry, module_id, input_index)
    else {
        return false;
    };
    let binding = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|module| module.id == module_id)
        .and_then(|module| module.bindings.get(input_name))
        .copied();
    let Some(parameter_id) = binding else {
        return expose_module_input(session, registry, module_id, input_index, localizer);
    };
    update_effect_parameter(session, localizer, parameter_id, |parameter| {
        parameter.exposed = !parameter.exposed;
    })
}

pub(super) fn expose_module_input(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module_id: ModuleId,
    input_index: u8,
    localizer: &Localizer,
) -> bool {
    let Some((emitter, input_name)) =
        properties_module_input_target(session, registry, module_id, input_index)
    else {
        return false;
    };
    let Some(module) = session
        .effect
        .emitters
        .iter()
        .find(|candidate| candidate.id == emitter)
        .and_then(|emitter| {
            emitter
                .modules
                .iter()
                .find(|candidate| candidate.id == module_id)
        })
    else {
        return false;
    };
    if module.bindings.contains_key(input_name) {
        return false;
    }
    let Some(mut default) = module_parameter(module, input_name) else {
        return false;
    };
    default.regenerate_ids();
    let metadata = registry.get(&module.module_type);
    let display_name = metadata
        .and_then(|metadata| metadata.inputs.get(input_index as usize))
        .map_or_else(
            || input_name.to_owned(),
            |input| localized_properties_input(localizer, input.name, input.display_name, false),
        );
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: unique_effect_parameter_name_from_base(&session.effect, &display_name),
        default,
        exposed: true,
    };
    let parameter_id = parameter.id;
    let parameter_index = session.effect.parameters.len();
    session.execute_transaction(
        EffectTransaction::new(
            localizer.text("properties-expose-module-input-command"),
            vec![
                EffectCommand::AddParameter {
                    parameter,
                    index: parameter_index,
                },
                EffectCommand::BindModuleParameter {
                    emitter,
                    module: module_id,
                    parameter: input_name.to_owned(),
                    source: parameter_id,
                },
            ],
        ),
        true,
    )
}

fn properties_module_input_target<'a>(
    session: &EditorSession,
    registry: &'a ModuleRegistry,
    module: ModuleId,
    input: u8,
) -> Option<(EmitterId, &'a str)> {
    let (emitter, module) = session.effect.emitters.iter().find_map(|emitter| {
        emitter
            .modules
            .iter()
            .find(|candidate| candidate.id == module)
            .map(|module| (emitter.id, module))
    })?;
    let metadata = registry.get(&module.module_type)?;
    metadata
        .inputs
        .get(input as usize)
        .map(|input| (emitter, input.name))
}

pub(super) fn set_module_input_source(
    session: &mut EditorSession,
    registry: &ModuleRegistry,
    module: ModuleId,
    input_index: u8,
    source: PropertySourceKind,
    localizer: &Localizer,
) -> bool {
    let Some((emitter, parameter)) =
        properties_module_input_target(session, registry, module, input_index)
    else {
        return false;
    };
    let Some((module_instance, input)) = session
        .effect
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .find(|candidate| candidate.id == module)
        .and_then(|module| {
            registry
                .get(&module.module_type)
                .and_then(|metadata| metadata.inputs.get(input_index as usize))
                .map(|input| (module, input))
        })
    else {
        return false;
    };
    if !input.sources.contains(&source) {
        return false;
    }
    if module_instance.property_source(parameter) == Some(source) {
        return false;
    }
    let Some(current) = properties_module_parameter(session, module, parameter) else {
        return false;
    };
    let mut commands = Vec::with_capacity(4);
    let Some(active_source) = module_instance.property_source(parameter) else {
        return false;
    };
    let active_has_stored_value = module_instance
        .property_source_values
        .get(parameter)
        .is_some_and(|values| {
            values
                .iter()
                .any(|candidate| candidate.source == active_source)
        });
    if active_source != PropertySourceKind::Constant
        && (module_instance.bindings.contains_key(parameter) || !active_has_stored_value)
    {
        commands.push(EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter: parameter.to_owned(),
            source: active_source,
            value: current.clone(),
        });
    }
    let target_value = if source == PropertySourceKind::Constant {
        module_instance.parameter_value(parameter)
    } else {
        module_instance
            .property_value_for_source(parameter, source)
            .or_else(|| initial_property_source_value(input, &current, source))
    };
    if source != PropertySourceKind::Constant
        && module_instance
            .property_value_for_source(parameter, source)
            .is_none()
    {
        let Some(value) = target_value.clone() else {
            return false;
        };
        commands.push(EffectCommand::SetModulePropertySourceValue {
            emitter,
            module,
            parameter: parameter.to_owned(),
            source,
            value,
        });
    }
    if let Some(parameter_id) = module_instance.bindings.get(parameter) {
        let Some(mut effect_parameter) = session
            .effect
            .parameters
            .iter()
            .find(|candidate| candidate.id == *parameter_id)
            .cloned()
        else {
            return false;
        };
        let Some(value) = target_value else {
            return false;
        };
        effect_parameter.default = detached_property_value(value);
        commands.push(EffectCommand::SetParameter {
            id: *parameter_id,
            parameter: effect_parameter,
        });
    } else if source == PropertySourceKind::Constant
        && active_source != PropertySourceKind::Constant
        && !active_has_stored_value
    {
        let Some(value) = target_value else {
            return false;
        };
        commands.push(EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter: parameter.to_owned(),
            value: detached_property_value(value),
        });
    }
    commands.push(EffectCommand::SetModulePropertySource {
        emitter,
        module,
        parameter: parameter.to_owned(),
        source,
    });
    session.execute_transaction(
        EffectTransaction::new(localizer.text("properties-change-source-command"), commands),
        true,
    )
}

fn detached_property_value(mut value: Value) -> Value {
    value.regenerate_ids();
    value
}

fn initial_property_source_value(
    input: &InputMetadata,
    current: &Value,
    source: PropertySourceKind,
) -> Option<Value> {
    let scalar = match current {
        Value::Scalar(value) => Some(*value),
        Value::Range(range) => Some((range.min + range.max) * 0.5),
        Value::Curve(curve) => Some(curve.sample(0.0)),
        _ => None,
    };
    let vector = match current {
        Value::Vec3(value) => Some(*value),
        Value::Vec3Range(range) => Some(std::array::from_fn(|axis| {
            (range.min[axis] + range.max[axis]) * 0.5
        })),
        Value::Vec3Curve(curves) => Some(curves.sample(0.0)),
        _ => None,
    };
    match source {
        PropertySourceKind::RandomRange => {
            let (step, min, max) = numeric_source_limits(&input.control)?;
            if let Some(value) = vector {
                let low = value
                    .map(|value| min.map_or(value - step, |minimum| (value - step).max(minimum)));
                let high = value
                    .map(|value| max.map_or(value + step, |maximum| (value + step).min(maximum)));
                return Some(Value::Vec3Range(Vec3Range::new(low, high)));
            }
            let value = scalar?;
            let low = min.map_or(value - step, |minimum| (value - step).max(minimum));
            let high = max.map_or(value + step, |maximum| (value + step).min(maximum));
            Some(Value::Range(ScalarRange::new(low.min(high), high.max(low))))
        }
        PropertySourceKind::Curve(_) => {
            if let Some(value) = vector {
                return Some(Value::Vec3Curve(Vec3Curve::constant(value)));
            }
            let value = scalar?;
            Some(Value::Curve(Curve::normalized(
                vec![CurveKey::new(0.0, 0.0), CurveKey::new(1.0, 0.0)],
                ScalarRange::new(value, value),
            )))
        }
        PropertySourceKind::Gradient(_) => {
            let Value::Gradient(gradient) = current else {
                return None;
            };
            let color = gradient.sample(0.0);
            Some(Value::Gradient(Gradient::new(vec![
                ColorKey::new(0.0, color),
                ColorKey::new(1.0, color),
            ])))
        }
        PropertySourceKind::Constant => None,
    }
}

pub(super) fn numeric_source_limits(
    control: &InputControl,
) -> Option<(f32, Option<f32>, Option<f32>)> {
    match control {
        InputControl::Number { step, min, max }
        | InputControl::Range { step, min, max }
        | InputControl::Vector { step, min, max } => Some((*step, *min, *max)),
        InputControl::Curve { step, min, max } => Some((*step, Some(*min), Some(*max))),
        _ => None,
    }
}

pub(super) fn properties_curve_limits(
    input: &InputMetadata,
    _curve: &Curve,
) -> Option<(f32, Option<f32>, Option<f32>)> {
    numeric_source_limits(&input.control)
}

pub(super) fn properties_module_collapsed(
    settings: &EditorSettings,
    module: &ModuleInstance,
) -> bool {
    properties_module_card_memory(module).collapsed(&settings.properties.section_expansion)
}

pub(super) fn properties_module_card_memory(module: &ModuleInstance) -> RememberedPanelCard {
    RememberedPanelCard::new(
        properties_module_key(module),
        !matches!(module.stage, StageKind::ParticleUpdate),
    )
}

pub(super) fn properties_module_key(module: &ModuleInstance) -> String {
    format!("module/{}", module.module_type.0)
}

pub(super) fn spawn_module_card(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    metadata: Option<&ModuleMetadata>,
    diagnostic_path: &str,
    session: &EditorSession,
    localizer: &Localizer,
    collapsed: bool,
    asset_server: &AssetServer,
) {
    let display_name = metadata.map_or(module.module_type.0.as_str(), |item| item.display_name);
    let help = metadata.map_or(
        "This module is not available in the current registry.",
        |item| item.description,
    );
    let base_border = if session
        .diagnostics
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.path.starts_with(diagnostic_path))
    {
        Color::srgb(0.82, 0.28, 0.24)
    } else if session.selection.primary == SemanticTarget::Module(module.id) {
        theme::ACCENT_DIM
    } else {
        theme::BORDER
    };
    spawn_remembered_panel_card(
        parent,
        PanelCardProps::new(display_name, collapsed)
            .with_memory_key(properties_module_key(module))
            .with_help(help)
            .with_enabled(module.enabled)
            .with_background(if module.enabled {
                theme::PANEL_LIGHT
            } else {
                theme::PANEL_DARK
            })
            .with_border(base_border),
        PropertiesSemanticTarget {
            target: SemanticTarget::Module(module.id),
            base_border,
        },
        PropertiesSelectionTarget(SemanticTarget::Module(module.id)),
        PropertiesAction::ToggleSection(PropertiesSection::Module(module.id)),
        |header| {
            let mut enabled = header.spawn_empty();
            enabled.apply_scene(ui_shell::feathers_checkbox()).insert((
                ModuleEnabledControl(module.id),
                AccessibleLabel(format!("Enable {display_name}")),
            ));
            if module.enabled {
                enabled.insert(Checked);
            }
            spawn_action_menu(
                header,
                &format!("{display_name} actions"),
                &[
                    ComboOption {
                        label: "Move up".into(),
                        selected: false,
                        action: PropertiesAction::MoveModule(module.id, -1),
                    },
                    ComboOption {
                        label: "Move down".into(),
                        selected: false,
                        action: PropertiesAction::MoveModule(module.id, 1),
                    },
                    ComboOption {
                        label: "Duplicate".into(),
                        selected: false,
                        action: PropertiesAction::DuplicateModule(module.id),
                    },
                    ComboOption {
                        label: "Delete…".into(),
                        selected: false,
                        action: PropertiesAction::DeleteModule(module.id),
                    },
                ],
            );
        },
        |card| {
            if let Some(metadata) = metadata {
                for (input_index, input) in metadata.inputs.iter().enumerate() {
                    spawn_input_control(
                        card,
                        module,
                        input,
                        input_index as u8,
                        session,
                        localizer,
                        asset_server,
                    );
                }
            }
            spawn_inline_diagnostics(card, diagnostic_path, session);
        },
    );
}

fn spawn_input_control(
    parent: &mut ChildSpawnerCommands,
    module: &ModuleInstance,
    input: &InputMetadata,
    input_index: u8,
    session: &EditorSession,
    localizer: &Localizer,
    asset_server: &AssetServer,
) {
    let display_name = localized_properties_input(localizer, input.name, input.display_name, false);
    let description = localized_properties_input(localizer, input.name, input.description, true);
    let Some(value) = properties_module_parameter(session, module.id, input.name) else {
        spawn_properties_read_only_control(parent, &display_name, "Missing authored value");
        return;
    };
    let public =
        public_module_input_control(session, module, input, input_index, &value, localizer);
    let source = property_source_for_input(module, input, &value);
    if input.sources.len() > 1
        && matches!(&input.control, InputControl::Vector { .. })
        && matches!(
            &value,
            Value::Vec3(_) | Value::Vec3Range(_) | Value::Vec3Curve(_)
        )
    {
        spawn_properties_vector_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if input.sources.len() > 1
        && matches!(&input.control, InputControl::Number { .. })
        && matches!(&value, Value::Scalar(_))
    {
        spawn_properties_scalar_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if source == PropertySourceKind::RandomRange && matches!(&value, Value::Range(_)) {
        spawn_properties_range_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            public,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    if matches!(source, PropertySourceKind::Curve(_))
        && let Value::Curve(curve) = &value
    {
        spawn_properties_curve_source_control(
            parent,
            module.id,
            input,
            input_index,
            &display_name,
            property_tooltip(&description, input.unit, localizer),
            curve,
            source,
            asset_server,
            localizer,
        );
        return;
    }
    match (&input.control, value) {
        (InputControl::Curve { .. }, Value::Curve(curve)) => {
            spawn_properties_curve_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                &curve,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Gradient, Value::Gradient(gradient)) => {
            spawn_properties_gradient_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                &description,
                &gradient,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Toggle, Value::Bool(value)) => {
            spawn_properties_toggle_control(
                parent,
                module.id,
                input,
                &display_name,
                &description,
                value,
                public,
            );
        }
        (InputControl::Number { .. }, Value::U32(_)) => {
            spawn_properties_integer_control(
                parent,
                module.id,
                input,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                public,
            );
        }
        (InputControl::Number { step, min, max }, Value::Scalar(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Scalar,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("", value, 0)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec2(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec3(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[("X", value[0], 0), ("Y", value[1], 1), ("Z", value[2], 2)],
                public,
            );
        }
        (InputControl::Vector { step, min, max }, Value::Vec4(value)) => {
            spawn_properties_number_controls(
                parent,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                PropertiesNumberControl {
                    module: module.id,
                    parameter: input.name,
                    component: 0,
                    kind: PropertiesNumberKind::Vector,
                    step: *step,
                    min: *min,
                    max: *max,
                },
                &[
                    ("X", value[0], 0),
                    ("Y", value[1], 1),
                    ("Z", value[2], 2),
                    ("W", value[3], 3),
                ],
                public,
            );
        }
        (InputControl::Range { .. }, Value::Range(_)) => {
            spawn_properties_range_source_control(
                parent,
                module.id,
                input,
                input_index,
                &display_name,
                property_tooltip(&description, input.unit, localizer),
                public,
                source,
                asset_server,
                localizer,
            );
        }
        (InputControl::Choice, value) => spawn_properties_choice_control(
            parent,
            module.id,
            input_index,
            &display_name,
            &description,
            &value,
        ),
        (_, value) => {
            spawn_properties_read_only_control(parent, &display_name, &format_value(value));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_limits_follow_numeric_control_metadata() {
        let number = InputControl::Number {
            step: 0.25,
            min: Some(0.0),
            max: Some(2.0),
        };
        assert_eq!(
            numeric_source_limits(&number),
            Some((0.25, Some(0.0), Some(2.0)))
        );
        assert_eq!(numeric_source_limits(&InputControl::Toggle), None);
    }
}
