//! Module discovery, compiler validation, and lowering into Aestra runtime plans.

use aestra_core::{
    Diagnostic, DiagnosticCode, EffectAsset, MODULE_APPEARANCE, MODULE_EMISSION, MODULE_INITIALIZE,
    MODULE_MOTION, MODULE_SHAPE, ModuleInstance, ModuleParameters, ModuleTypeId, RENDERER_SPRITE,
    RendererProperties, StageKind, ValidationReport,
};
use aestra_runtime::{
    CompiledEffect, CompiledEmitter, ExecutionPlan, Instruction, IrLocation, ParticleAttribute,
    ParticleLayout, RendererPlan, RuntimeStage,
};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueType {
    U32,
    Scalar,
    Vec2,
    Range,
    Curve,
    Gradient,
    Shape,
}

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

        let mut source_map = BTreeMap::new();
        let mut live_attributes = BTreeSet::new();
        let mut emitters = Vec::with_capacity(asset.emitters.len());
        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            let mut execution = ExecutionPlan::default();
            for module in emitter.modules.iter().filter(|module| module.enabled) {
                let instruction = lower_module(module).expect("validated built-in module");
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
                let metadata = self
                    .registry
                    .get(&module.module_type)
                    .expect("validated module is registered");
                live_attributes.extend(metadata.reads.iter().copied());
                live_attributes.extend(metadata.writes.iter().copied());
            }

            live_attributes.extend([
                ParticleAttribute::Position,
                ParticleAttribute::Rotation,
                ParticleAttribute::Size,
                ParticleAttribute::Color,
            ]);
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

        Ok(CompiledEffect {
            source: asset.id,
            name: asset.name.clone(),
            duration: asset.duration,
            looping: asset.looping,
            particle_layout: ParticleLayout {
                attributes: live_attributes.into_iter().collect(),
            },
            max_particles: asset
                .emitters
                .iter()
                .map(|emitter| emitter.max_particles as usize)
                .sum(),
            emitters,
            source_map,
        })
    }

    fn validate_compiler_contracts(&self, asset: &EffectAsset, report: &mut ValidationReport) {
        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            let emitter_path = format!("effect.emitters[{emitter_index}]");
            for (module_index, module) in emitter.modules.iter().enumerate() {
                let path = format!("{emitter_path}.modules[{module_index}]");
                let Some(metadata) = self.registry.get(&module.module_type) else {
                    report.push(Diagnostic::error(
                        DiagnosticCode::UnknownModule,
                        format!("{path}.module_type"),
                        format!("module '{}' is not registered", module.module_type.0),
                    ));
                    continue;
                };
                if !metadata.stages.contains(&module.stage) {
                    report.push(Diagnostic::error(
                        DiagnosticCode::StageMismatch,
                        format!("{path}.stage"),
                        format!(
                            "module '{}' cannot execute in stage {:?}",
                            module.module_type.0, module.stage
                        ),
                    ));
                }
                if module.enabled && !parameters_match(module) {
                    report.push(Diagnostic::error(
                        DiagnosticCode::InvalidValue,
                        format!("{path}.parameters"),
                        format!(
                            "module '{}' has parameters that its compiler lowering does not support",
                            module.module_type.0
                        ),
                    ));
                }
            }

            self.validate_attribute_flow(emitter_index, emitter.modules.as_slice(), report);

            let enabled_renderers = emitter
                .renderers
                .iter()
                .filter(|renderer| renderer.enabled)
                .count();
            if enabled_renderers == 0 {
                report.push(Diagnostic::error(
                    DiagnosticCode::MissingRenderer,
                    format!("{emitter_path}.renderers"),
                    "emitter must have at least one enabled renderer",
                ));
            }
            for (renderer_index, renderer) in emitter.renderers.iter().enumerate() {
                if renderer.enabled
                    && (renderer.renderer_type.0 != RENDERER_SPRITE
                        || !matches!(renderer.properties, RendererProperties::Sprite { .. }))
                {
                    report.push(Diagnostic::error(
                        DiagnosticCode::UnsupportedRenderer,
                        format!("{emitter_path}.renderers[{renderer_index}].renderer_type"),
                        format!(
                            "renderer '{}' is not supported by the current runtime",
                            renderer.renderer_type.0
                        ),
                    ));
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
                        report.push(Diagnostic::error(
                            DiagnosticCode::MissingAttribute,
                            format!("effect.emitters[{emitter_index}].modules[{module_index}]"),
                            format!(
                                "module '{}' reads unavailable attribute {attribute:?}",
                                module.module_type.0
                            ),
                        ));
                    }
                }
                available.extend(metadata.writes.iter().copied());
            }
        }
        for required in [
            ParticleAttribute::Position,
            ParticleAttribute::Rotation,
            ParticleAttribute::Size,
            ParticleAttribute::Color,
        ] {
            if !available.contains(&required) {
                report.push(Diagnostic::error(
                    DiagnosticCode::MissingAttribute,
                    format!("effect.emitters[{emitter_index}].renderers"),
                    format!("sprite rendering requires attribute {required:?}"),
                ));
            }
        }
    }
}

fn lower_module(module: &ModuleInstance) -> Option<Instruction> {
    let instruction = match &module.parameters {
        ModuleParameters::Emission {
            spawn_rate,
            burst_count,
        } => Instruction::Emit {
            source: module.id,
            spawn_rate: *spawn_rate,
            burst_count: *burst_count,
        },
        ModuleParameters::Shape { shape } => Instruction::SampleShape {
            source: module.id,
            shape: *shape,
        },
        ModuleParameters::Initialize {
            lifetime,
            speed,
            direction_degrees,
            spread_degrees,
            angular_velocity,
        } => Instruction::Initialize {
            source: module.id,
            lifetime: *lifetime,
            speed: *speed,
            direction_degrees: *direction_degrees,
            spread_degrees: *spread_degrees,
            angular_velocity: *angular_velocity,
        },
        ModuleParameters::Motion {
            gravity,
            drag,
            turbulence,
        } => Instruction::Motion {
            source: module.id,
            gravity: *gravity,
            drag: *drag,
            turbulence: *turbulence,
        },
        ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } => Instruction::Appearance {
            source: module.id,
            size: size.clone(),
            opacity: opacity.clone(),
            color: color.clone(),
        },
        ModuleParameters::Custom(_) => return None,
    };
    Some(instruction)
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
