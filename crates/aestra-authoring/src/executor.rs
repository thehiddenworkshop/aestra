use crate::{EffectCommand, EffectDiff, EffectTransaction, LockState, SemanticTarget};
use aestra_core::{
    DiagnosticCode, EffectAsset, Emitter, EmitterId, MaterialId, ModuleId, ModuleParameters,
    RendererId, ValidationReport, Value,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("{kind} '{id}' was not found")]
    NotFound { kind: &'static str, id: String },
    #[error("{target} is locked")]
    Locked { target: SemanticTarget },
    #[error("index {index} is outside {collection} with length {len}")]
    IndexOutOfBounds {
        collection: &'static str,
        index: usize,
        len: usize,
    },
    #[error("module parameter '{parameter}' does not exist on this module type")]
    UnknownParameter { parameter: String },
    #[error("module parameter '{parameter}' expected {expected}, got {actual}")]
    ParameterType {
        parameter: String,
        expected: &'static str,
        actual: &'static str,
    },
    #[error("module parameter '{parameter}' cannot be removed from a built-in module")]
    RequiredParameter { parameter: String },
    #[error("module parameter '{parameter}' is not bound")]
    MissingBinding { parameter: String },
    #[error("transaction validation failed: {0}")]
    Validation(#[from] ValidationReport),
    #[error("the document changed after this transaction was previewed")]
    StalePreview,
}

#[derive(Debug, Clone)]
pub struct TransactionOutcome {
    pub inverse: EffectTransaction,
    pub diff: EffectDiff,
}

#[derive(Debug, Clone)]
pub struct TransactionPreview {
    source: EffectAsset,
    candidate: EffectAsset,
    transaction: EffectTransaction,
    diff: EffectDiff,
}

impl TransactionPreview {
    pub fn candidate(&self) -> &EffectAsset {
        &self.candidate
    }

    pub fn transaction(&self) -> &EffectTransaction {
        &self.transaction
    }

    pub fn diff(&self) -> &EffectDiff {
        &self.diff
    }

    pub(crate) fn source_matches(&self, effect: &EffectAsset) -> bool {
        &self.source == effect
    }

    pub(crate) fn into_transaction(self) -> EffectTransaction {
        self.transaction
    }
}

#[derive(Debug, Default)]
pub struct CommandExecutor;

impl CommandExecutor {
    pub fn preview(
        effect: &EffectAsset,
        locks: &LockState,
        transaction: EffectTransaction,
    ) -> Result<TransactionPreview, CommandError> {
        let source = effect.clone();
        let mut candidate = source.clone();
        let outcome = Self::execute(&mut candidate, locks, &transaction)?;
        Ok(TransactionPreview {
            source,
            candidate,
            transaction,
            diff: outcome.diff,
        })
    }

    pub fn execute(
        effect: &mut EffectAsset,
        locks: &LockState,
        transaction: &EffectTransaction,
    ) -> Result<TransactionOutcome, CommandError> {
        let before = effect.clone();
        let mut working = before.clone();
        let mut inverse_commands = Vec::new();

        for command in &transaction.commands {
            if let Some(target) = locks.blocking_target(command, &working) {
                return Err(CommandError::Locked { target });
            }
            let mut command_inverse = apply_command(&mut working, command)?;
            command_inverse.extend(inverse_commands);
            inverse_commands = command_inverse;
        }

        let mut report = working.validation_report();
        report.diagnostics.retain(|diagnostic| {
            !matches!(
                diagnostic.code,
                DiagnosticCode::MissingModule | DiagnosticCode::MissingRenderer
            )
        });
        report.into_result()?;
        let diff = EffectDiff::between(&before, &working);
        *effect = working;
        Ok(TransactionOutcome {
            inverse: EffectTransaction::new(
                format!("Undo {}", transaction.label),
                inverse_commands,
            ),
            diff,
        })
    }
}

fn apply_command(
    effect: &mut EffectAsset,
    command: &EffectCommand,
) -> Result<Vec<EffectCommand>, CommandError> {
    let inverse = match command {
        EffectCommand::SetEffectName { name } => {
            let previous = std::mem::replace(&mut effect.name, name.clone());
            vec![EffectCommand::SetEffectName { name: previous }]
        }
        EffectCommand::SetEffectDuration { duration } => {
            let previous = std::mem::replace(&mut effect.duration, *duration);
            vec![EffectCommand::SetEffectDuration { duration: previous }]
        }
        EffectCommand::SetEffectLooping { looping } => {
            let previous = std::mem::replace(&mut effect.looping, *looping);
            vec![EffectCommand::SetEffectLooping { looping: previous }]
        }
        EffectCommand::AddParameter { parameter, index } => {
            checked_insert(
                &mut effect.parameters,
                *index,
                parameter.clone(),
                "effect parameters",
            )?;
            vec![EffectCommand::RemoveParameter { id: parameter.id }]
        }
        EffectCommand::RemoveParameter { id } => {
            let index = effect
                .parameters
                .iter()
                .position(|item| item.id == *id)
                .ok_or_else(|| not_found("parameter", id))?;
            let parameter = effect.parameters.remove(index);
            vec![EffectCommand::AddParameter { parameter, index }]
        }
        EffectCommand::AddMaterial { material, index } => {
            checked_insert(
                &mut effect.materials,
                *index,
                material.clone(),
                "effect materials",
            )?;
            vec![EffectCommand::RemoveMaterial { id: material.id }]
        }
        EffectCommand::RemoveMaterial { id } => {
            let index = material_index(effect, *id)?;
            let material = effect.materials.remove(index);
            vec![EffectCommand::AddMaterial { material, index }]
        }
        EffectCommand::SetMaterial { id, material } => {
            let index = material_index(effect, *id)?;
            let mut replacement = material.clone();
            replacement.id = *id;
            let previous = std::mem::replace(&mut effect.materials[index], replacement);
            vec![EffectCommand::SetMaterial {
                id: *id,
                material: previous,
            }]
        }
        EffectCommand::AddEmitter { emitter, index } => {
            checked_insert(
                &mut effect.emitters,
                *index,
                emitter.clone(),
                "effect emitters",
            )?;
            vec![EffectCommand::RemoveEmitter { id: emitter.id }]
        }
        EffectCommand::RemoveEmitter { id } => {
            let index = emitter_index(effect, *id)?;
            let emitter = effect.emitters.remove(index);
            let mut removed_events = Vec::new();
            let mut event_index = 0;
            while event_index < effect.events.len() {
                if effect.events[event_index].source == *id
                    || effect.events[event_index].target == *id
                {
                    removed_events.push((event_index, effect.events.remove(event_index)));
                } else {
                    event_index += 1;
                }
            }
            let mut commands = vec![EffectCommand::AddEmitter { emitter, index }];
            commands.extend(
                removed_events
                    .into_iter()
                    .map(|(index, event)| EffectCommand::AddEvent { event, index }),
            );
            commands
        }
        EffectCommand::MoveEmitter { id, index } => {
            let old_index = emitter_index(effect, *id)?;
            checked_move(&mut effect.emitters, old_index, *index, "effect emitters")?;
            vec![EffectCommand::MoveEmitter {
                id: *id,
                index: old_index,
            }]
        }
        EffectCommand::SetEmitterName { id, name } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.name, name.clone());
            vec![EffectCommand::SetEmitterName {
                id: *id,
                name: previous,
            }]
        }
        EffectCommand::SetEmitterEnabled { id, enabled } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.enabled, *enabled);
            vec![EffectCommand::SetEmitterEnabled {
                id: *id,
                enabled: previous,
            }]
        }
        EffectCommand::SetEmitterTiming {
            id,
            start_time,
            duration,
        } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = (emitter.start_time, emitter.duration);
            emitter.start_time = *start_time;
            emitter.duration = *duration;
            vec![EffectCommand::SetEmitterTiming {
                id: *id,
                start_time: previous.0,
                duration: previous.1,
            }]
        }
        EffectCommand::SetEmitterCapacity { id, max_particles } => {
            let emitter = emitter_mut(effect, *id)?;
            let previous = std::mem::replace(&mut emitter.max_particles, *max_particles);
            vec![EffectCommand::SetEmitterCapacity {
                id: *id,
                max_particles: previous,
            }]
        }
        EffectCommand::AddModule {
            emitter,
            module,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            checked_insert(
                &mut target.modules,
                *index,
                module.clone(),
                "emitter modules",
            )?;
            vec![EffectCommand::RemoveModule {
                emitter: *emitter,
                module: module.id,
            }]
        }
        EffectCommand::RemoveModule { emitter, module } => {
            let target = emitter_mut(effect, *emitter)?;
            let index = module_index(target, *module)?;
            let removed = target.modules.remove(index);
            vec![EffectCommand::AddModule {
                emitter: *emitter,
                module: removed,
                index,
            }]
        }
        EffectCommand::MoveModule {
            emitter,
            module,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            let old_index = module_index(target, *module)?;
            checked_move(&mut target.modules, old_index, *index, "emitter modules")?;
            vec![EffectCommand::MoveModule {
                emitter: *emitter,
                module: *module,
                index: old_index,
            }]
        }
        EffectCommand::SetModuleEnabled {
            emitter,
            module,
            enabled,
        } => {
            let module = module_mut(effect, *emitter, *module)?;
            let previous = std::mem::replace(&mut module.enabled, *enabled);
            vec![EffectCommand::SetModuleEnabled {
                emitter: *emitter,
                module: module.id,
                enabled: previous,
            }]
        }
        EffectCommand::SetModuleParameter {
            emitter,
            module,
            parameter,
            value,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let previous = set_module_parameter(module_instance, parameter, value.clone())?;
            match previous {
                Some(value) => vec![EffectCommand::SetModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    value,
                }],
                None => vec![EffectCommand::RemoveModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                }],
            }
        }
        EffectCommand::RemoveModuleParameter {
            emitter,
            module,
            parameter,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let ModuleParameters::Custom(values) = &mut module_instance.parameters else {
                return Err(CommandError::RequiredParameter {
                    parameter: parameter.clone(),
                });
            };
            let value = values
                .remove(parameter)
                .ok_or_else(|| CommandError::UnknownParameter {
                    parameter: parameter.clone(),
                })?;
            vec![EffectCommand::SetModuleParameter {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                value,
            }]
        }
        EffectCommand::BindModuleParameter {
            emitter,
            module,
            parameter,
            source,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let previous = module_instance.bindings.insert(parameter.clone(), *source);
            match previous {
                Some(source) => vec![EffectCommand::BindModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                    source,
                }],
                None => vec![EffectCommand::UnbindModuleParameter {
                    emitter: *emitter,
                    module: *module,
                    parameter: parameter.clone(),
                }],
            }
        }
        EffectCommand::UnbindModuleParameter {
            emitter,
            module,
            parameter,
        } => {
            let module_instance = module_mut(effect, *emitter, *module)?;
            let source = module_instance.bindings.remove(parameter).ok_or_else(|| {
                CommandError::MissingBinding {
                    parameter: parameter.clone(),
                }
            })?;
            vec![EffectCommand::BindModuleParameter {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                source,
            }]
        }
        EffectCommand::AddCurveKey {
            emitter,
            module,
            parameter,
            key,
            index,
        } => {
            let curve = module_curve_mut(effect, *emitter, *module, parameter)?;
            checked_insert(&mut curve.keys, *index, *key, "curve keys")?;
            vec![EffectCommand::RemoveCurveKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
            }]
        }
        EffectCommand::RemoveCurveKey {
            emitter,
            module,
            parameter,
            index,
        } => {
            let curve = module_curve_mut(effect, *emitter, *module, parameter)?;
            let key = checked_remove(&mut curve.keys, *index, "curve keys")?;
            vec![EffectCommand::AddCurveKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                key,
                index: *index,
            }]
        }
        EffectCommand::SetCurveKey {
            emitter,
            module,
            parameter,
            index,
            key,
        } => {
            let curve = module_curve_mut(effect, *emitter, *module, parameter)?;
            let previous = checked_replace(&mut curve.keys, *index, *key, "curve keys")?;
            vec![EffectCommand::SetCurveKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
                key: previous,
            }]
        }
        EffectCommand::AddGradientKey {
            emitter,
            module,
            parameter,
            key,
            index,
        } => {
            let gradient = module_gradient_mut(effect, *emitter, *module, parameter)?;
            checked_insert(&mut gradient.keys, *index, *key, "gradient keys")?;
            vec![EffectCommand::RemoveGradientKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
            }]
        }
        EffectCommand::RemoveGradientKey {
            emitter,
            module,
            parameter,
            index,
        } => {
            let gradient = module_gradient_mut(effect, *emitter, *module, parameter)?;
            let key = checked_remove(&mut gradient.keys, *index, "gradient keys")?;
            vec![EffectCommand::AddGradientKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                key,
                index: *index,
            }]
        }
        EffectCommand::SetGradientKey {
            emitter,
            module,
            parameter,
            index,
            key,
        } => {
            let gradient = module_gradient_mut(effect, *emitter, *module, parameter)?;
            let previous = checked_replace(&mut gradient.keys, *index, *key, "gradient keys")?;
            vec![EffectCommand::SetGradientKey {
                emitter: *emitter,
                module: *module,
                parameter: parameter.clone(),
                index: *index,
                key: previous,
            }]
        }
        EffectCommand::AddRenderer {
            emitter,
            renderer,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            checked_insert(
                &mut target.renderers,
                *index,
                renderer.clone(),
                "emitter renderers",
            )?;
            vec![EffectCommand::RemoveRenderer {
                emitter: *emitter,
                renderer: renderer.id,
            }]
        }
        EffectCommand::RemoveRenderer { emitter, renderer } => {
            let target = emitter_mut(effect, *emitter)?;
            let index = renderer_index(target, *renderer)?;
            let removed = target.renderers.remove(index);
            vec![EffectCommand::AddRenderer {
                emitter: *emitter,
                renderer: removed,
                index,
            }]
        }
        EffectCommand::MoveRenderer {
            emitter,
            renderer,
            index,
        } => {
            let target = emitter_mut(effect, *emitter)?;
            let old_index = renderer_index(target, *renderer)?;
            checked_move(
                &mut target.renderers,
                old_index,
                *index,
                "emitter renderers",
            )?;
            vec![EffectCommand::MoveRenderer {
                emitter: *emitter,
                renderer: *renderer,
                index: old_index,
            }]
        }
        EffectCommand::SetRendererEnabled {
            emitter,
            renderer,
            enabled,
        } => {
            let renderer = renderer_mut(effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.enabled, *enabled);
            vec![EffectCommand::SetRendererEnabled {
                emitter: *emitter,
                renderer: renderer.id,
                enabled: previous,
            }]
        }
        EffectCommand::SetRendererMaterial {
            emitter,
            renderer,
            material,
        } => {
            let renderer = renderer_mut(effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.material, *material);
            vec![EffectCommand::SetRendererMaterial {
                emitter: *emitter,
                renderer: renderer.id,
                material: previous,
            }]
        }
        EffectCommand::SetRendererProperties {
            emitter,
            renderer,
            properties,
        } => {
            let renderer = renderer_mut(effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.properties, properties.clone());
            vec![EffectCommand::SetRendererProperties {
                emitter: *emitter,
                renderer: renderer.id,
                properties: previous,
            }]
        }
        EffectCommand::AddEvent { event, index } => {
            checked_insert(&mut effect.events, *index, event.clone(), "effect events")?;
            vec![EffectCommand::RemoveEvent { id: event.id }]
        }
        EffectCommand::RemoveEvent { id } => {
            let index = effect
                .events
                .iter()
                .position(|event| event.id == *id)
                .ok_or_else(|| not_found("event", id))?;
            let event = effect.events.remove(index);
            vec![EffectCommand::AddEvent { event, index }]
        }
    };
    Ok(inverse)
}

fn set_module_parameter(
    module: &mut aestra_core::ModuleInstance,
    parameter: &str,
    value: Value,
) -> Result<Option<Value>, CommandError> {
    let unknown = || CommandError::UnknownParameter {
        parameter: parameter.into(),
    };
    let mismatch = |expected, actual| CommandError::ParameterType {
        parameter: parameter.into(),
        expected,
        actual,
    };
    let previous = match (&mut module.parameters, value) {
        (ModuleParameters::Emission { spawn_rate, .. }, Value::Scalar(value))
            if parameter == "spawn_rate" =>
        {
            Some(Value::Scalar(std::mem::replace(spawn_rate, value)))
        }
        (ModuleParameters::Emission { burst_count, .. }, Value::U32(value))
            if parameter == "burst_count" =>
        {
            Some(Value::U32(std::mem::replace(burst_count, value)))
        }
        (ModuleParameters::Shape { shape }, Value::Shape(value)) if parameter == "shape" => {
            Some(Value::Shape(std::mem::replace(shape, value)))
        }
        (ModuleParameters::Initialize { lifetime, .. }, Value::Range(value))
            if parameter == "lifetime" =>
        {
            Some(Value::Range(std::mem::replace(lifetime, value)))
        }
        (ModuleParameters::Initialize { speed, .. }, Value::Range(value))
            if parameter == "speed" =>
        {
            Some(Value::Range(std::mem::replace(speed, value)))
        }
        (
            ModuleParameters::Initialize {
                direction_degrees, ..
            },
            Value::Scalar(value),
        ) if parameter == "direction_degrees" => {
            Some(Value::Scalar(std::mem::replace(direction_degrees, value)))
        }
        (ModuleParameters::Initialize { spread_degrees, .. }, Value::Scalar(value))
            if parameter == "spread_degrees" =>
        {
            Some(Value::Scalar(std::mem::replace(spread_degrees, value)))
        }
        (
            ModuleParameters::Initialize {
                angular_velocity, ..
            },
            Value::Range(value),
        ) if parameter == "angular_velocity" => {
            Some(Value::Range(std::mem::replace(angular_velocity, value)))
        }
        (ModuleParameters::Motion { gravity, .. }, Value::Vec2(value))
            if parameter == "gravity" =>
        {
            Some(Value::Vec2(std::mem::replace(gravity, value)))
        }
        (ModuleParameters::Motion { drag, .. }, Value::Scalar(value)) if parameter == "drag" => {
            Some(Value::Scalar(std::mem::replace(drag, value)))
        }
        (ModuleParameters::Motion { turbulence, .. }, Value::Scalar(value))
            if parameter == "turbulence" =>
        {
            Some(Value::Scalar(std::mem::replace(turbulence, value)))
        }
        (ModuleParameters::Appearance { size, .. }, Value::Curve(value)) if parameter == "size" => {
            Some(Value::Curve(std::mem::replace(size, value)))
        }
        (ModuleParameters::Appearance { opacity, .. }, Value::Curve(value))
            if parameter == "opacity" =>
        {
            Some(Value::Curve(std::mem::replace(opacity, value)))
        }
        (ModuleParameters::Appearance { color, .. }, Value::Gradient(value))
            if parameter == "color" =>
        {
            Some(Value::Gradient(std::mem::replace(color, value)))
        }
        (ModuleParameters::Custom(values), value) => values.insert(parameter.into(), value),
        (parameters, value) => {
            let expected = expected_parameter_type(parameters, parameter).ok_or_else(unknown)?;
            return Err(mismatch(expected, value_type(&value)));
        }
    };
    Ok(previous)
}

fn expected_parameter_type(parameters: &ModuleParameters, parameter: &str) -> Option<&'static str> {
    match (parameters, parameter) {
        (ModuleParameters::Emission { .. }, "spawn_rate") => Some("scalar"),
        (ModuleParameters::Emission { .. }, "burst_count") => Some("u32"),
        (ModuleParameters::Shape { .. }, "shape") => Some("shape"),
        (ModuleParameters::Initialize { .. }, "lifetime" | "speed" | "angular_velocity") => {
            Some("range")
        }
        (ModuleParameters::Initialize { .. }, "direction_degrees" | "spread_degrees") => {
            Some("scalar")
        }
        (ModuleParameters::Motion { .. }, "gravity") => Some("vec2"),
        (ModuleParameters::Motion { .. }, "drag" | "turbulence") => Some("scalar"),
        (ModuleParameters::Appearance { .. }, "size" | "opacity") => Some("curve"),
        (ModuleParameters::Appearance { .. }, "color") => Some("gradient"),
        _ => None,
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Bool(_) => "bool",
        Value::U32(_) => "u32",
        Value::Scalar(_) => "scalar",
        Value::Vec2(_) => "vec2",
        Value::Vec3(_) => "vec3",
        Value::Vec4(_) => "vec4",
        Value::Text(_) => "text",
        Value::Range(_) => "range",
        Value::Curve(_) => "curve",
        Value::Gradient(_) => "gradient",
        Value::Shape(_) => "shape",
        Value::Parameter(_) => "parameter",
        Value::Asset(_) => "asset",
        Value::Material(_) => "material",
    }
}

fn emitter_index(effect: &EffectAsset, id: EmitterId) -> Result<usize, CommandError> {
    effect
        .emitters
        .iter()
        .position(|emitter| emitter.id == id)
        .ok_or_else(|| not_found("emitter", &id))
}

fn material_index(effect: &EffectAsset, id: MaterialId) -> Result<usize, CommandError> {
    effect
        .materials
        .iter()
        .position(|material| material.id == id)
        .ok_or_else(|| not_found("material", &id))
}

fn emitter_mut(effect: &mut EffectAsset, id: EmitterId) -> Result<&mut Emitter, CommandError> {
    effect
        .emitters
        .iter_mut()
        .find(|emitter| emitter.id == id)
        .ok_or_else(|| not_found("emitter", &id))
}

fn module_index(emitter: &Emitter, id: ModuleId) -> Result<usize, CommandError> {
    emitter
        .modules
        .iter()
        .position(|module| module.id == id)
        .ok_or_else(|| not_found("module", &id))
}

fn renderer_index(emitter: &Emitter, id: RendererId) -> Result<usize, CommandError> {
    emitter
        .renderers
        .iter()
        .position(|renderer| renderer.id == id)
        .ok_or_else(|| not_found("renderer", &id))
}

fn module_mut(
    effect: &mut EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
) -> Result<&mut aestra_core::ModuleInstance, CommandError> {
    emitter_mut(effect, emitter)?
        .modules
        .iter_mut()
        .find(|item| item.id == module)
        .ok_or_else(|| not_found("module", &module))
}

fn renderer_mut(
    effect: &mut EffectAsset,
    emitter: EmitterId,
    renderer: RendererId,
) -> Result<&mut aestra_core::RendererInstance, CommandError> {
    emitter_mut(effect, emitter)?
        .renderers
        .iter_mut()
        .find(|item| item.id == renderer)
        .ok_or_else(|| not_found("renderer", &renderer))
}

fn module_curve_mut<'a>(
    effect: &'a mut EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
    parameter: &str,
) -> Result<&'a mut aestra_core::Curve, CommandError> {
    let module = module_mut(effect, emitter, module)?;
    match (&mut module.parameters, parameter) {
        (ModuleParameters::Appearance { size, .. }, "size") => Ok(size),
        (ModuleParameters::Appearance { opacity, .. }, "opacity") => Ok(opacity),
        (ModuleParameters::Custom(values), parameter) => match values.get_mut(parameter) {
            Some(Value::Curve(curve)) => Ok(curve),
            Some(value) => Err(CommandError::ParameterType {
                parameter: parameter.into(),
                expected: "curve",
                actual: value_type(value),
            }),
            None => Err(CommandError::UnknownParameter {
                parameter: parameter.into(),
            }),
        },
        _ => Err(CommandError::UnknownParameter {
            parameter: parameter.into(),
        }),
    }
}

fn module_gradient_mut<'a>(
    effect: &'a mut EffectAsset,
    emitter: EmitterId,
    module: ModuleId,
    parameter: &str,
) -> Result<&'a mut aestra_core::Gradient, CommandError> {
    let module = module_mut(effect, emitter, module)?;
    match (&mut module.parameters, parameter) {
        (ModuleParameters::Appearance { color, .. }, "color") => Ok(color),
        (ModuleParameters::Custom(values), parameter) => match values.get_mut(parameter) {
            Some(Value::Gradient(gradient)) => Ok(gradient),
            Some(value) => Err(CommandError::ParameterType {
                parameter: parameter.into(),
                expected: "gradient",
                actual: value_type(value),
            }),
            None => Err(CommandError::UnknownParameter {
                parameter: parameter.into(),
            }),
        },
        _ => Err(CommandError::UnknownParameter {
            parameter: parameter.into(),
        }),
    }
}

fn checked_insert<T>(
    items: &mut Vec<T>,
    index: usize,
    value: T,
    collection: &'static str,
) -> Result<(), CommandError> {
    if index > items.len() {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index,
            len: items.len(),
        });
    }
    items.insert(index, value);
    Ok(())
}

fn checked_remove<T>(
    items: &mut Vec<T>,
    index: usize,
    collection: &'static str,
) -> Result<T, CommandError> {
    if index >= items.len() {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index,
            len: items.len(),
        });
    }
    Ok(items.remove(index))
}

fn checked_replace<T: Copy>(
    items: &mut [T],
    index: usize,
    value: T,
    collection: &'static str,
) -> Result<T, CommandError> {
    let len = items.len();
    let Some(item) = items.get_mut(index) else {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index,
            len,
        });
    };
    Ok(std::mem::replace(item, value))
}

fn checked_move<T>(
    items: &mut Vec<T>,
    old_index: usize,
    new_index: usize,
    collection: &'static str,
) -> Result<(), CommandError> {
    if new_index >= items.len() {
        return Err(CommandError::IndexOutOfBounds {
            collection,
            index: new_index,
            len: items.len(),
        });
    }
    let item = items.remove(old_index);
    items.insert(new_index, item);
    Ok(())
}

fn not_found(kind: &'static str, id: &impl ToString) -> CommandError {
    CommandError::NotFound {
        kind,
        id: id.to_string(),
    }
}
