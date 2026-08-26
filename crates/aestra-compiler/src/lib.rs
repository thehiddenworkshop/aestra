//! Module discovery, compiler validation, optimization, and typed lowering.

pub use aestra_core::ValueType;

use aestra_core::{
    Diagnostic, DiagnosticCode, EffectAsset, EffectParameter, MODULE_APPEARANCE, MODULE_EMISSION,
    MODULE_INITIALIZE, MODULE_MOTION, MODULE_SHAPE, ModuleInstance, ModuleParameters, ModuleTypeId,
    ParameterId, RENDERER_SPRITE, RendererProperties, StageKind, ValidationReport,
};
use aestra_runtime::{
    CompiledCurve, CompiledEffect, CompiledEmitter, CompiledGradient, CompiledParameter,
    ExecutionPlan, Expression, Instruction, IrLocation, OptimizationStats, ParameterSlot,
    ParticleAttribute, ParticleLayout, RendererPlan, RuntimeParameterValue, RuntimeStage,
    RuntimeValue,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputMetadata {
    pub name: &'static str,
    pub value_type: ValueType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    CpuReference,
    ParticleSimulation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleMetadata {
    pub type_id: ModuleTypeId,
    pub display_name: &'static str,
    pub category: &'static str,
    pub stages: Vec<StageKind>,
    pub inputs: Vec<InputMetadata>,
    pub reads: Vec<ParticleAttribute>,
    pub writes: Vec<ParticleAttribute>,
    pub tags: Vec<&'static str>,
    pub capabilities: Vec<Capability>,
    pub approximate_cost: u32,
}

/// Extensible catalog used by validation, authoring UI, and lowering.
#[derive(Debug, Clone, Default)]
pub struct ModuleRegistry {
    modules: BTreeMap<ModuleTypeId, ModuleMetadata>,
}

impl ModuleRegistry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for metadata in builtin_modules() {
            registry.register(metadata);
        }
        registry
    }

    pub fn register(&mut self, metadata: ModuleMetadata) -> Option<ModuleMetadata> {
        self.modules.insert(metadata.type_id.clone(), metadata)
    }

    pub fn get(&self, type_id: &ModuleTypeId) -> Option<&ModuleMetadata> {
        self.modules.get(type_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &ModuleMetadata> {
        self.modules.values()
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }
}

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("effect compilation failed: {0}")]
    Validation(ValidationReport),
}

impl CompileError {
    pub fn report(&self) -> &ValidationReport {
        match self {
            Self::Validation(report) => report,
        }
    }
}

/// Frontend that validates authored semantics and emits immutable runtime plans.
#[derive(Debug, Clone)]
pub struct EffectCompiler {
    registry: ModuleRegistry,
}

impl Default for EffectCompiler {
    fn default() -> Self {
        Self::new(ModuleRegistry::builtin())
    }
}

impl EffectCompiler {
    pub fn new(registry: ModuleRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ModuleRegistry {
        &self.registry
    }

    pub fn compile(&self, asset: &EffectAsset) -> Result<CompiledEffect, CompileError> {
        let mut report = asset.validation_report();
        self.validate_compiler_contracts(asset, &mut report);
        if !report.is_valid() {
            return Err(CompileError::Validation(report));
        }

        let parameter_lookup = asset
            .parameters
            .iter()
            .map(|parameter| (parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let referenced_parameters = asset
            .emitters
            .iter()
            .flat_map(|emitter| emitter.modules.iter())
            .filter(|module| module.enabled)
            .flat_map(|module| module.bindings.values().copied())
            .collect::<BTreeSet<_>>();
        let mut parameters = Vec::new();
        let mut parameter_slots = BTreeMap::new();
        for parameter in asset
            .parameters
            .iter()
            .filter(|parameter| parameter.exposed && referenced_parameters.contains(&parameter.id))
        {
            let slot = ParameterSlot(parameters.len());
            parameter_slots.insert(parameter.id, slot);
            parameters.push(CompiledParameter {
                source: parameter.id,
                name: parameter.name.clone(),
                value_type: parameter.default.value_type(),
                default: RuntimeValue::compile(&parameter.default)
                    .expect("validated runtime parameter has a concrete default"),
            });
        }
        let context = LoweringContext {
            parameters: &parameter_lookup,
            slots: &parameter_slots,
        };

        let mut source_map = BTreeMap::new();
        let mut stored_attributes = BTreeSet::new();
        let mut transient_attributes = BTreeSet::new();
        let mut discovered_attributes = BTreeSet::new();
        let mut emitters = Vec::with_capacity(asset.emitters.len());
        let mut optimizations = OptimizationStats::default();

        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            let liveness = self.analyze_liveness(&emitter.modules);
            stored_attributes.extend(liveness.stored);
            transient_attributes.extend(liveness.transient);
            discovered_attributes.extend(liveness.discovered);

            let mut execution = ExecutionPlan::default();
            for module in emitter.modules.iter().filter(|module| module.enabled) {
                let instruction = lower_module(module, &context)
                    .expect("validated built-in module must have a lowering");
                let (constants, parameters) = expression_counts(&instruction);
                optimizations.constant_expressions += constants;
                optimizations.runtime_parameter_reads += parameters;
                let (stage, instructions) = match module.stage {
                    StageKind::EmitterUpdate => {
                        (RuntimeStage::EmitterUpdate, &mut execution.emitter_update)
                    }
                    StageKind::ParticleSpawn => {
                        (RuntimeStage::ParticleSpawn, &mut execution.particle_spawn)
                    }
                    StageKind::ParticleUpdate => {
                        (RuntimeStage::ParticleUpdate, &mut execution.particle_update)
                    }
                    _ => unreachable!("compiler validation rejects unsupported stages"),
                };
                let instruction_index = instructions.len();
                instructions.push(instruction);
                source_map.insert(
                    module.id,
                    IrLocation {
                        emitter_index,
                        stage,
                        instruction_index,
                    },
                );
            }

            let renderers = emitter
                .renderers
                .iter()
                .filter(|renderer| renderer.enabled)
                .map(|renderer| match renderer.properties {
                    RendererProperties::Sprite { softness } => RendererPlan {
                        source: renderer.id,
                        blend: renderer.blend,
                        softness,
                    },
                    _ => unreachable!("compiler validation rejects unsupported renderers"),
                })
                .collect();
            emitters.push(CompiledEmitter {
                source: emitter.id,
                name: emitter.name.clone(),
                enabled: emitter.enabled,
                start_time: emitter.start_time,
                duration: emitter.duration,
                max_particles: emitter.max_particles,
                execution,
                renderers,
            });
        }

        optimizations.eliminated_attributes =
            discovered_attributes.difference(&stored_attributes).count();

        Ok(CompiledEffect {
            source: asset.id,
            name: asset.name.clone(),
            duration: asset.duration,
            looping: asset.looping,
            parameters,
            parameter_slots,
            particle_layout: ParticleLayout {
                attributes: stored_attributes.into_iter().collect(),
                transient_attributes: transient_attributes.into_iter().collect(),
            },
            max_particles: asset
                .emitters
                .iter()
                .map(|emitter| emitter.max_particles as usize)
                .sum(),
            emitters,
            source_map,
            optimizations,
        })
    }

    fn validate_compiler_contracts(&self, asset: &EffectAsset, report: &mut ValidationReport) {
        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            let emitter_path = format!("effect.emitters[{emitter_index}]");
            for (module_index, module) in emitter.modules.iter().enumerate() {
                let path = format!("{emitter_path}.modules[{module_index}]");
                let Some(metadata) = self.registry.get(&module.module_type) else {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::UnknownModule,
                            format!("{path}.module_type"),
                            format!("module '{}' is not registered", module.module_type.0),
                        ),
                    );
                    continue;
                };
                if !metadata.stages.contains(&module.stage) {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::StageMismatch,
                            format!("{path}.stage"),
                            format!(
                                "module '{}' cannot execute in stage {:?}",
                                module.module_type.0, module.stage
                            ),
                        ),
                    );
                }
                if module.enabled && !parameters_match(module) {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::InvalidValue,
                            format!("{path}.parameters"),
                            format!(
                                "module '{}' has parameters that its compiler lowering does not support",
                                module.module_type.0
                            ),
                        ),
                    );
                }
                for (input_name, parameter_id) in &module.bindings {
                    let binding_path = format!("{path}.bindings.{input_name}");
                    let Some(input) = metadata
                        .inputs
                        .iter()
                        .find(|input| input.name == input_name)
                    else {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::UnknownParameter,
                                binding_path,
                                format!(
                                    "module '{}' has no registered input named '{input_name}'",
                                    module.module_type.0
                                ),
                            ),
                        );
                        continue;
                    };
                    if let Some(parameter) = asset
                        .parameters
                        .iter()
                        .find(|parameter| parameter.id == *parameter_id)
                    {
                        let actual = parameter.default.value_type();
                        if actual != input.value_type {
                            push_unique(
                                report,
                                Diagnostic::error(
                                    DiagnosticCode::ParameterTypeMismatch,
                                    binding_path,
                                    format!(
                                        "input '{input_name}' expects {:?}, but parameter '{}' is {actual:?}",
                                        input.value_type, parameter.name
                                    ),
                                ),
                            );
                        }
                    }
                }
            }

            self.validate_attribute_flow(emitter_index, emitter.modules.as_slice(), report);

            let enabled_renderers = emitter
                .renderers
                .iter()
                .filter(|renderer| renderer.enabled)
                .count();
            if enabled_renderers == 0 {
                push_unique(
                    report,
                    Diagnostic::error(
                        DiagnosticCode::MissingRenderer,
                        format!("{emitter_path}.renderers"),
                        "emitter must have at least one enabled renderer",
                    ),
                );
            }
            for (renderer_index, renderer) in emitter.renderers.iter().enumerate() {
                if renderer.enabled
                    && (renderer.renderer_type.0 != RENDERER_SPRITE
                        || !matches!(renderer.properties, RendererProperties::Sprite { .. }))
                {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::UnsupportedRenderer,
                            format!("{emitter_path}.renderers[{renderer_index}].renderer_type"),
                            format!(
                                "renderer '{}' is not supported by the current runtime",
                                renderer.renderer_type.0
                            ),
                        ),
                    );
                }
            }
        }
    }

    fn validate_attribute_flow(
        &self,
        emitter_index: usize,
        modules: &[ModuleInstance],
        report: &mut ValidationReport,
    ) {
        let mut available =
            BTreeSet::from([ParticleAttribute::Age, ParticleAttribute::NormalizedAge]);
        for stage in [StageKind::ParticleSpawn, StageKind::ParticleUpdate] {
            for (module_index, module) in modules.iter().enumerate() {
                if !module.enabled || module.stage != stage {
                    continue;
                }
                let Some(metadata) = self.registry.get(&module.module_type) else {
                    continue;
                };
                for attribute in &metadata.reads {
                    if !available.contains(attribute) {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::MissingAttribute,
                                format!("effect.emitters[{emitter_index}].modules[{module_index}]"),
                                format!(
                                    "module '{}' reads unavailable attribute {attribute:?}",
                                    module.module_type.0
                                ),
                            ),
                        );
                    }
                }
                available.extend(metadata.writes.iter().copied());
            }
        }
        for required in renderer_attributes() {
            if !available.contains(&required) {
                push_unique(
                    report,
                    Diagnostic::error(
                        DiagnosticCode::MissingAttribute,
                        format!("effect.emitters[{emitter_index}].renderers"),
                        format!("sprite rendering requires attribute {required:?}"),
                    ),
                );
            }
        }
    }

    fn analyze_liveness(&self, modules: &[ModuleInstance]) -> Liveness {
        let mut live = BTreeSet::from([
            ParticleAttribute::Position,
            ParticleAttribute::Rotation,
            ParticleAttribute::Size,
            ParticleAttribute::Color,
            ParticleAttribute::Age,
            ParticleAttribute::Lifetime,
            ParticleAttribute::AngularVelocity,
        ]);
        let mut stored = live.clone();
        let mut discovered = live.clone();

        for module in modules.iter().filter(|module| module.enabled) {
            if let Some(metadata) = self.registry.get(&module.module_type) {
                discovered.extend(metadata.reads.iter().copied());
                discovered.extend(metadata.writes.iter().copied());
            }
        }

        for stage in [StageKind::ParticleUpdate, StageKind::ParticleSpawn] {
            for module in modules.iter().rev() {
                if !module.enabled || module.stage != stage {
                    continue;
                }
                let Some(metadata) = self.registry.get(&module.module_type) else {
                    continue;
                };
                if metadata
                    .writes
                    .iter()
                    .any(|attribute| live.contains(attribute))
                {
                    for attribute in &metadata.writes {
                        live.remove(attribute);
                    }
                    live.extend(metadata.reads.iter().copied());
                    stored.extend(live.iter().copied());
                }
            }
        }

        let transient = stored
            .iter()
            .copied()
            .filter(|attribute| matches!(attribute, ParticleAttribute::NormalizedAge))
            .collect::<BTreeSet<_>>();
        for attribute in &transient {
            stored.remove(attribute);
        }
        Liveness {
            stored,
            transient,
            discovered,
        }
    }
}

struct Liveness {
    stored: BTreeSet<ParticleAttribute>,
    transient: BTreeSet<ParticleAttribute>,
    discovered: BTreeSet<ParticleAttribute>,
}

struct LoweringContext<'a> {
    parameters: &'a BTreeMap<ParameterId, &'a EffectParameter>,
    slots: &'a BTreeMap<ParameterId, ParameterSlot>,
}

fn lower_module(module: &ModuleInstance, context: &LoweringContext<'_>) -> Option<Instruction> {
    let instruction = match &module.parameters {
        ModuleParameters::Emission {
            spawn_rate,
            burst_count,
        } => Instruction::Emit {
            source: module.id,
            spawn_rate: expression(module, "spawn_rate", *spawn_rate, context),
            burst_count: expression(module, "burst_count", *burst_count, context),
        },
        ModuleParameters::Shape { shape } => Instruction::SampleShape {
            source: module.id,
            shape: expression(module, "shape", *shape, context),
        },
        ModuleParameters::Initialize {
            lifetime,
            speed,
            direction_degrees,
            spread_degrees,
            angular_velocity,
        } => Instruction::Initialize {
            source: module.id,
            lifetime: expression(module, "lifetime", *lifetime, context),
            speed: expression(module, "speed", *speed, context),
            direction_degrees: expression(module, "direction_degrees", *direction_degrees, context),
            spread_degrees: expression(module, "spread_degrees", *spread_degrees, context),
            angular_velocity: expression(module, "angular_velocity", *angular_velocity, context),
        },
        ModuleParameters::Motion {
            gravity,
            drag,
            turbulence,
        } => Instruction::Motion {
            source: module.id,
            gravity: expression(module, "gravity", *gravity, context),
            drag: expression(module, "drag", *drag, context),
            turbulence: expression(module, "turbulence", *turbulence, context),
        },
        ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } => Instruction::Appearance {
            source: module.id,
            size: expression(module, "size", CompiledCurve::compile(size), context),
            opacity: expression(module, "opacity", CompiledCurve::compile(opacity), context),
            color: expression(module, "color", CompiledGradient::compile(color), context),
        },
        ModuleParameters::Custom(_) => return None,
    };
    Some(instruction)
}

fn expression<T>(
    module: &ModuleInstance,
    input: &str,
    fallback: T,
    context: &LoweringContext<'_>,
) -> Expression<T>
where
    T: RuntimeParameterValue + Clone,
{
    let Some(parameter_id) = module.bindings.get(input) else {
        return Expression::constant(fallback);
    };
    if let Some(slot) = context.slots.get(parameter_id) {
        return Expression::parameter(*slot);
    }
    let parameter = context
        .parameters
        .get(parameter_id)
        .expect("validated binding references an existing parameter");
    let runtime = RuntimeValue::compile(&parameter.default)
        .expect("validated bound parameter has a concrete default");
    Expression::constant(
        T::from_runtime(&runtime)
            .expect("validated binding type matches its module input")
            .clone(),
    )
}

fn expression_counts(instruction: &Instruction) -> (usize, usize) {
    fn one<T>(expression: &Expression<T>) -> (usize, usize) {
        match expression {
            Expression::Constant(_) => (1, 0),
            Expression::Parameter(_) => (0, 1),
        }
    }
    fn sum(values: impl IntoIterator<Item = (usize, usize)>) -> (usize, usize) {
        values.into_iter().fold((0, 0), |total, value| {
            (total.0 + value.0, total.1 + value.1)
        })
    }
    match instruction {
        Instruction::Emit {
            spawn_rate,
            burst_count,
            ..
        } => sum([one(spawn_rate), one(burst_count)]),
        Instruction::SampleShape { shape, .. } => one(shape),
        Instruction::Initialize {
            lifetime,
            speed,
            direction_degrees,
            spread_degrees,
            angular_velocity,
            ..
        } => sum([
            one(lifetime),
            one(speed),
            one(direction_degrees),
            one(spread_degrees),
            one(angular_velocity),
        ]),
        Instruction::Motion {
            gravity,
            drag,
            turbulence,
            ..
        } => sum([one(gravity), one(drag), one(turbulence)]),
        Instruction::Appearance {
            size,
            opacity,
            color,
            ..
        } => sum([one(size), one(opacity), one(color)]),
    }
}

fn parameters_match(module: &ModuleInstance) -> bool {
    matches!(
        (&*module.module_type.0, &module.parameters),
        (MODULE_EMISSION, ModuleParameters::Emission { .. })
            | (MODULE_SHAPE, ModuleParameters::Shape { .. })
            | (MODULE_INITIALIZE, ModuleParameters::Initialize { .. })
            | (MODULE_MOTION, ModuleParameters::Motion { .. })
            | (MODULE_APPEARANCE, ModuleParameters::Appearance { .. })
    )
}

fn renderer_attributes() -> [ParticleAttribute; 4] {
    [
        ParticleAttribute::Position,
        ParticleAttribute::Rotation,
        ParticleAttribute::Size,
        ParticleAttribute::Color,
    ]
}

fn push_unique(report: &mut ValidationReport, diagnostic: Diagnostic) {
    if !report
        .diagnostics
        .iter()
        .any(|existing| existing.code == diagnostic.code && existing.path == diagnostic.path)
    {
        report.push(diagnostic);
    }
}

fn input(name: &'static str, value_type: ValueType) -> InputMetadata {
    InputMetadata { name, value_type }
}

fn metadata(
    type_id: &'static str,
    display_name: &'static str,
    category: &'static str,
    stage: StageKind,
) -> ModuleMetadata {
    ModuleMetadata {
        type_id: ModuleTypeId::new(type_id),
        display_name,
        category,
        stages: vec![stage],
        inputs: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        tags: Vec::new(),
        capabilities: vec![Capability::CpuReference, Capability::ParticleSimulation],
        approximate_cost: 0,
    }
}

impl ModuleMetadata {
    fn with_inputs(mut self, inputs: Vec<InputMetadata>) -> Self {
        self.inputs = inputs;
        self
    }

    fn with_flow(mut self, reads: Vec<ParticleAttribute>, writes: Vec<ParticleAttribute>) -> Self {
        self.reads = reads;
        self.writes = writes;
        self
    }

    fn with_tags(mut self, tags: Vec<&'static str>) -> Self {
        self.tags = tags;
        self
    }

    fn with_cost(mut self, approximate_cost: u32) -> Self {
        self.approximate_cost = approximate_cost;
        self
    }
}

fn builtin_modules() -> Vec<ModuleMetadata> {
    use ParticleAttribute as A;
    vec![
        metadata(
            MODULE_EMISSION,
            "Emission",
            "Emitter",
            StageKind::EmitterUpdate,
        )
        .with_inputs(vec![
            input("spawn_rate", ValueType::Scalar),
            input("burst_count", ValueType::U32),
        ])
        .with_flow(vec![], vec![])
        .with_tags(vec!["spawn", "rate", "burst"])
        .with_cost(1),
        metadata(MODULE_SHAPE, "Shape", "Spawn", StageKind::ParticleSpawn)
            .with_inputs(vec![input("shape", ValueType::Shape)])
            .with_flow(vec![], vec![A::Position])
            .with_tags(vec!["spawn", "position"])
            .with_cost(2),
        metadata(
            MODULE_INITIALIZE,
            "Initialize Particle",
            "Spawn",
            StageKind::ParticleSpawn,
        )
        .with_inputs(vec![
            input("lifetime", ValueType::Range),
            input("speed", ValueType::Range),
            input("direction_degrees", ValueType::Scalar),
            input("spread_degrees", ValueType::Scalar),
            input("angular_velocity", ValueType::Range),
        ])
        .with_flow(
            vec![],
            vec![A::Velocity, A::Lifetime, A::Rotation, A::AngularVelocity],
        )
        .with_tags(vec!["spawn", "velocity", "lifetime"])
        .with_cost(4),
        metadata(MODULE_MOTION, "Motion", "Forces", StageKind::ParticleUpdate)
            .with_inputs(vec![
                input("gravity", ValueType::Vec2),
                input("drag", ValueType::Scalar),
                input("turbulence", ValueType::Scalar),
            ])
            .with_flow(
                vec![A::Position, A::Velocity, A::Age],
                vec![A::Position, A::Velocity],
            )
            .with_tags(vec!["update", "force", "motion"])
            .with_cost(6),
        metadata(
            MODULE_APPEARANCE,
            "Appearance Over Life",
            "Appearance",
            StageKind::ParticleUpdate,
        )
        .with_inputs(vec![
            input("size", ValueType::Curve),
            input("opacity", ValueType::Curve),
            input("color", ValueType::Gradient),
        ])
        .with_flow(vec![A::NormalizedAge], vec![A::Size, A::Color])
        .with_tags(vec!["update", "color", "size"])
        .with_cost(5),
    ]
}
