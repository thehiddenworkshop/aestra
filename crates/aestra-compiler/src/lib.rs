//! Module discovery, compiler validation, optimization, and typed lowering.

mod material_function;
mod material_graph;
mod material_ir;
mod material_reflection;
mod material_stack;
mod normal_map;
pub use normal_map::evaluate_normal_map;

pub use material_function::*;
pub use material_graph::*;
pub use material_ir::*;
pub use material_reflection::*;
pub use material_stack::*;

pub use aestra_core::{
    PropertyEvaluationDomain as InputEvaluationDomain, PropertySource as InputSourceKind, ValueType,
};

use aestra_core::{
    ColorKey, Curve, CurveId, CurveKey, Diagnostic, DiagnosticCode, EffectAsset, EffectParameter,
    EmitterShape, Gradient, GradientId, MODULE_APPEARANCE, MODULE_EMISSION, MODULE_INITIALIZE,
    MODULE_MOTION, MODULE_SHAPE, MaterialInput, MaterialProgramId, MaterialProperties,
    ModuleInstance, ModuleParameters, ModuleTypeId, ParameterId, RENDERER_FLIPBOOK, RENDERER_MESH,
    RENDERER_SPRITE, RendererProperties, ScalarRange, SpriteColorSource, StageKind,
    ValidationReport, Value,
    material::{MaterialParameterValue, MaterialProgram},
};
use aestra_project::{ProjectAssetIndex, ProjectDependencyReport, ResolvedEffectProject};
use aestra_runtime::{
    CompiledAsset, CompiledChoreographyEvent, CompiledCurve, CompiledEffect, CompiledEffectClip,
    CompiledEffectProject, CompiledEmitter, CompiledFlipbook, CompiledGradient, CompiledMaterial,
    CompiledParameter, CompiledParameterOverride, CompiledVec3Curve, EffectRequirements,
    ExecutionPlan, Expression, Instruction, IrLocation, MaterialColorPlan, OptimizationStats,
    ParameterSlot, ParticleAttribute, ParticleLayout, RendererCapability, RendererPlan,
    RendererPlanKind, RuntimeParameterValue, RuntimeStage, RuntimeValue, ScalarSource,
    SimulationSeekMode, VectorSource,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct InputMetadata {
    pub name: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub value_type: ValueType,
    pub default_value: aestra_core::Value,
    pub unit: Option<&'static str>,
    pub control: InputControl,
    pub sources: Vec<InputSourceKind>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputControl {
    Toggle,
    Number {
        step: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    Vector {
        step: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    Range {
        step: f32,
        min: Option<f32>,
        max: Option<f32>,
    },
    Choice,
    Curve {
        step: f32,
        min: f32,
        max: f32,
    },
    Gradient,
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    CpuReference,
    ParticleSimulation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleMetadata {
    pub type_id: ModuleTypeId,
    pub display_name: &'static str,
    pub description: &'static str,
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

    /// Creates an authored instance using the catalog's production-ready defaults.
    pub fn instantiate(&self, type_id: &ModuleTypeId) -> Option<ModuleInstance> {
        self.get(type_id)?;
        match type_id.0.as_str() {
            MODULE_EMISSION => Some(ModuleInstance::emission(24.0, 0)),
            MODULE_SHAPE => Some(ModuleInstance::shape(EmitterShape::Point)),
            MODULE_INITIALIZE => Some(ModuleInstance::initialize(
                ScalarRange::new(0.8, 1.4),
                ScalarRange::new(35.0, 70.0),
                [0.0, 1.0, 0.0],
                30.0,
                ScalarRange::new(-1.0, 1.0),
            )),
            MODULE_MOTION => Some(ModuleInstance::motion([0.0, -18.0, 0.0], 0.6, 4.0)),
            MODULE_APPEARANCE => Some(ModuleInstance::appearance(
                Curve::new(vec![
                    CurveKey::new(0.0, 4.0),
                    CurveKey::new(0.35, 10.0),
                    CurveKey::new(1.0, 1.0),
                ]),
                Curve::new(vec![
                    CurveKey::new(0.0, 0.0),
                    CurveKey::new(0.12, 1.0),
                    CurveKey::new(1.0, 0.0),
                ]),
                Gradient::new(vec![
                    ColorKey::new(0.0, [0.35, 0.75, 1.0, 1.0]),
                    ColorKey::new(0.5, [0.62, 0.3, 1.0, 1.0]),
                    ColorKey::new(1.0, [0.15, 0.05, 0.4, 0.0]),
                ]),
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum CompileError {
    #[error("effect compilation failed: {0}")]
    Validation(ValidationReport),
}

#[derive(Debug, Error)]
pub enum ProjectCompileError {
    #[error(transparent)]
    Dependencies(#[from] ProjectDependencyReport),
    #[error("failed to compile effect {effect}: {source}")]
    Effect {
        effect: aestra_core::EffectId,
        #[source]
        source: CompileError,
    },
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

    /// Resolves and compiles a root effect together with all transitive reusable effects.
    pub fn compile_project(
        &self,
        root: &EffectAsset,
        index: &ProjectAssetIndex,
    ) -> Result<CompiledEffectProject, ProjectCompileError> {
        let resolved = index.resolve_effect_project(root)?;
        self.validate_project_parameter_overrides(&resolved)?;
        let function_library =
            MaterialFunctionLibrary::new(resolved.material_functions.values().cloned());
        let compiled_root = Arc::new(
            self.compile_with_material_programs_and_functions(
                &resolved.root,
                &resolved.material_programs,
                &function_library,
            )
            .map_err(|source| ProjectCompileError::Effect {
                effect: resolved.root.id,
                source,
            })?,
        );
        let mut dependencies = BTreeMap::new();
        for (&id, effect) in &resolved.dependencies {
            let compiled = self
                .compile_with_material_programs_and_functions(
                    effect,
                    &resolved.material_programs,
                    &function_library,
                )
                .map_err(|source| ProjectCompileError::Effect { effect: id, source })?;
            dependencies.insert(id, Arc::new(compiled));
        }
        let mut project = CompiledEffectProject {
            root: compiled_root,
            dependencies,
        };
        populate_project_parameter_overrides(&resolved, &mut project);
        Ok(project)
    }

    pub fn compile(&self, asset: &EffectAsset) -> Result<CompiledEffect, CompileError> {
        self.compile_with_material_programs(asset, &BTreeMap::new())
    }

    /// Compiles one effect with the project material programs resolved for its semantic instances.
    pub fn compile_with_material_programs(
        &self,
        asset: &EffectAsset,
        material_programs: &BTreeMap<MaterialProgramId, MaterialProgram>,
    ) -> Result<CompiledEffect, CompileError> {
        self.compile_with_material_programs_and_functions(
            asset,
            material_programs,
            &MaterialFunctionLibrary::default(),
        )
    }

    pub fn compile_with_material_programs_and_functions(
        &self,
        asset: &EffectAsset,
        material_programs: &BTreeMap<MaterialProgramId, MaterialProgram>,
        functions: &MaterialFunctionLibrary,
    ) -> Result<CompiledEffect, CompileError> {
        let mut report = asset.validation_report();
        self.validate_compiler_contracts(asset, &mut report);
        let mut expanded_programs = BTreeMap::new();
        let mut function_expansions = BTreeMap::new();
        for (&id, program) in material_programs {
            match material_function::inline_material_functions(program, functions) {
                Ok(expansion) => {
                    expanded_programs.insert(id, expansion.program.clone());
                    function_expansions.insert(id, expansion);
                }
                Err(error) => {
                    for mut diagnostic in error.report().diagnostics.clone() {
                        diagnostic.path = format!("material_programs[{id}].{}", diagnostic.path);
                        push_unique(&mut report, diagnostic);
                    }
                }
            }
        }
        let material_programs = &expanded_programs;
        for (index, instance) in asset.material_instances.iter().enumerate() {
            let Some(program) = material_programs.get(&instance.program.id()) else {
                push_unique(
                    &mut report,
                    Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        format!("effect.material_instances[{index}].program"),
                        format!(
                            "semantic material program {} is not available to the compiler",
                            instance.program.id()
                        ),
                    ),
                );
                continue;
            };
            for mut diagnostic in program.validation_report().diagnostics {
                diagnostic.path = format!(
                    "effect.material_instances[{index}].program.{}",
                    diagnostic.path
                );
                push_unique(&mut report, diagnostic);
            }
            for mut diagnostic in instance.validate_against(program).diagnostics {
                diagnostic.path = format!("effect.material_instances[{index}].{}", diagnostic.path);
                push_unique(&mut report, diagnostic);
            }
        }
        if !report.is_valid() {
            return Err(CompileError::Validation(report));
        }

        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            for (renderer_index, renderer) in emitter
                .renderers
                .iter()
                .enumerate()
                .filter(|(_, renderer)| renderer.enabled)
            {
                let path = format!("effect.emitters[{emitter_index}].renderers[{renderer_index}]");
                if let RendererProperties::Mesh { asset: mesh } = renderer.properties
                    && !asset
                        .assets
                        .iter()
                        .any(|asset| asset.id == mesh && asset.kind == aestra_core::AssetKind::Mesh)
                {
                    report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        format!("{path}.properties.asset"),
                        "mesh renderer requires a registered Mesh asset",
                    ));
                }
                if let Some(instance) = asset
                    .material_instances
                    .iter()
                    .find(|instance| instance.id == renderer.material)
                    && let Some(program) = material_programs.get(&instance.program.id())
                {
                    let expected =
                        if matches!(renderer.properties, RendererProperties::Ribbon { .. }) {
                            aestra_core::material::MaterialDomain::Ribbon
                        } else if matches!(renderer.properties, RendererProperties::Mesh { .. }) {
                            aestra_core::material::MaterialDomain::Mesh
                        } else {
                            aestra_core::material::MaterialDomain::Sprite
                        };
                    if program.domain != expected {
                        report.push(Diagnostic::error(
                            DiagnosticCode::UnsupportedMaterialDomain,
                            format!("{path}.material"),
                            format!(
                                "renderer requires a {expected:?} material, received {:?}",
                                program.domain
                            ),
                        ));
                    }
                }
            }
        }
        if !report.is_valid() {
            return Err(CompileError::Validation(report));
        }

        let parameter_lookup = asset
            .parameters
            .iter()
            .map(|parameter| (parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let mut referenced_parameters = asset
            .emitters
            .iter()
            .flat_map(|emitter| emitter.modules.iter())
            .filter(|module| module.enabled)
            .flat_map(|module| module.bindings.values().copied())
            .collect::<BTreeSet<_>>();
        for material in &asset.materials {
            let MaterialProperties::Sprite {
                softness, color, ..
            } = &material.properties;
            collect_material_parameter(softness, &mut referenced_parameters);
            if let SpriteColorSource::Value(input) = color {
                collect_material_parameter(input, &mut referenced_parameters);
            }
        }
        referenced_parameters.extend(asset.material_instances.iter().flat_map(|instance| {
            instance.values.values().filter_map(|value| match value {
                MaterialParameterValue::EffectParameter(parameter)
                | MaterialParameterValue::EmitterParameter(parameter) => Some(*parameter),
                MaterialParameterValue::Constant(_)
                | MaterialParameterValue::RandomRange { .. } => None,
            })
        }));
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
        for id in asset
            .material_instances
            .iter()
            .map(|instance| instance.program.id())
            .collect::<BTreeSet<_>>()
        {
            let Some(expansion) = function_expansions.get(&id) else {
                continue;
            };
            let stats = MaterialCompiler
                .compile_function_expansion(expansion)
                .map_err(|error| CompileError::Validation(error.report().clone()))?
                .optimizations;
            optimizations.material_common_subexpressions += stats.common_subexpressions;
            optimizations.material_specialized_parameter_reads += stats.specialized_parameter_reads;
            optimizations.material_pruned_static_branches += stats.pruned_static_branches;
            optimizations.material_pruned_features += stats.pruned_features;
            optimizations.material_texture_samples_authored += stats.texture_samples_authored;
            optimizations.material_texture_samples_eliminated += stats.texture_samples_eliminated;
            optimizations.material_texture_samples_live += stats.texture_samples_live;
            optimizations.material_function_calls_authored += stats.function_calls_authored;
            optimizations.material_function_calls_eliminated += stats.function_calls_eliminated;
            optimizations.material_function_calls_live += stats.function_calls_live;
        }
        let materials = asset
            .materials
            .iter()
            .map(|material| {
                let MaterialProperties::Sprite {
                    softness,
                    color,
                    texture,
                    uv,
                } = &material.properties;
                let softness = material_expression(softness, &context);
                let color = match color {
                    SpriteColorSource::ParticleColor => MaterialColorPlan::ParticleColor,
                    SpriteColorSource::Value(input) => {
                        MaterialColorPlan::Value(material_expression(input, &context))
                    }
                };
                CompiledMaterial {
                    source: material.id,
                    name: material.name.clone(),
                    blend: material.blend,
                    softness,
                    color,
                    texture: *texture,
                    uv: *uv,
                }
            })
            .collect::<Vec<_>>();
        for material in &materials {
            let mut counts = expression_count(&material.softness);
            if let MaterialColorPlan::Value(color) = &material.color {
                counts = add_expression_counts(counts, expression_count(color));
            }
            optimizations.constant_expressions += counts.0;
            optimizations.runtime_parameter_reads += counts.1;
        }

        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            let compiled_emitter_index = emitters.len();
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
                        emitter_index: compiled_emitter_index,
                        stage,
                        instruction_index,
                    },
                );
            }

            let renderers: Vec<RendererPlan> = emitter
                .renderers
                .iter()
                .filter(|renderer| renderer.enabled)
                .map(|renderer| match &renderer.properties {
                    RendererProperties::Sprite => RendererPlan {
                        source: renderer.id,
                        material: renderer.material,
                        kind: RendererPlanKind::Sprite,
                    },
                    RendererProperties::Flipbook {
                        flipbook,
                        time_source,
                        playback,
                        random_start,
                    } => RendererPlan {
                        source: renderer.id,
                        material: renderer.material,
                        kind: RendererPlanKind::Flipbook {
                            flipbook: *flipbook,
                            time_source: *time_source,
                            playback: *playback,
                            random_start: *random_start,
                        },
                    },
                    RendererProperties::Ribbon { width } => RendererPlan {
                        source: renderer.id,
                        material: renderer.material,
                        kind: RendererPlanKind::Ribbon { width: *width },
                    },
                    RendererProperties::Mesh { asset } => RendererPlan {
                        source: renderer.id,
                        material: renderer.material,
                        kind: RendererPlanKind::Mesh { asset: *asset },
                    },
                    _ => unreachable!("compiler validation rejects unsupported renderers"),
                })
                .collect();
            for region in emitter.timeline_regions() {
                emitters.push(CompiledEmitter {
                    source: emitter.id,
                    region: region.id,
                    name: emitter.name.clone(),
                    enabled: emitter.enabled,
                    transform: emitter.transform,
                    start_time: region.start_time,
                    source_offset: region.source_offset,
                    source_duration: emitter.duration,
                    duration: region.duration,
                    seed_index: emitter_index as u32,
                    max_particles: emitter.max_particles,
                    execution: execution.clone(),
                    renderers: renderers.clone(),
                });
            }
        }

        optimizations.eliminated_attributes =
            discovered_attributes.difference(&stored_attributes).count();
        let requirements = derive_effect_requirements(&emitters);

        Ok(CompiledEffect {
            source: asset.id,
            name: asset.name.clone(),
            duration: asset.duration,
            playback_mode: asset.playback_mode,
            seek_mode: SimulationSeekMode::StatelessDirect,
            assets: asset
                .assets
                .iter()
                .map(|entry| CompiledAsset {
                    source: entry.id,
                    name: entry.name.clone(),
                    kind: entry.kind,
                    path: entry.path.clone(),
                })
                .collect(),
            flipbooks: asset
                .flipbooks
                .iter()
                .map(|flipbook| CompiledFlipbook {
                    source: flipbook.id,
                    name: flipbook.name.clone(),
                    texture: flipbook.texture,
                    frames: flipbook.frames.clone(),
                    frame_rate: flipbook.frame_rate,
                    looping: flipbook.looping,
                })
                .collect(),
            materials,
            material_programs: asset
                .material_instances
                .iter()
                .map(|instance| instance.program.id())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter_map(|id| material_programs.get(&id).map(MaterialProgram::normalized))
                .collect(),
            material_instances: asset.material_instances.clone(),
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
            effect_clips: asset
                .effect_clips
                .iter()
                .map(|clip| CompiledEffectClip {
                    source_clip: clip.id,
                    source: clip.source,
                    start_time: clip.start_time,
                    source_offset: clip.source_offset,
                    duration: clip.duration,
                    transform: clip.transform,
                    seed: clip.seed,
                    parameter_overrides: Vec::new(),
                })
                .collect(),
            choreography_events: {
                let mut events = asset
                    .choreography_events
                    .iter()
                    .map(|event| CompiledChoreographyEvent {
                        source: event.id,
                        name: event.name.clone(),
                        time: event.time,
                        payload: event.payload.clone(),
                    })
                    .collect::<Vec<_>>();
                events.sort_by(|left, right| {
                    left.time
                        .total_cmp(&right.time)
                        .then_with(|| left.source.cmp(&right.source))
                });
                events
            },
            requirements,
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
                for (input_name, source) in &module.property_sources {
                    let source_path = format!("{path}.property_sources.{input_name}");
                    let Some(input) = metadata
                        .inputs
                        .iter()
                        .find(|input| input.name == input_name)
                    else {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::UnknownParameter,
                                source_path,
                                format!(
                                    "module '{}' has no registered input named '{input_name}'",
                                    module.module_type.0
                                ),
                            ),
                        );
                        continue;
                    };
                    if !input.sources.contains(source) {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::InvalidValue,
                                source_path,
                                format!("input '{input_name}' does not support source {source:?}"),
                            ),
                        );
                    }
                }
                for (input_name, values) in &module.property_source_values {
                    let source_path = format!("{path}.property_source_values.{input_name}");
                    let Some(input) = metadata
                        .inputs
                        .iter()
                        .find(|input| input.name == input_name)
                    else {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::UnknownParameter,
                                source_path,
                                format!(
                                    "module '{}' has no registered input named '{input_name}'",
                                    module.module_type.0
                                ),
                            ),
                        );
                        continue;
                    };
                    for (value_index, value) in values.iter().enumerate() {
                        if !input.sources.contains(&value.source) {
                            push_unique(
                                report,
                                Diagnostic::error(
                                    DiagnosticCode::InvalidValue,
                                    format!("{source_path}[{value_index}].source"),
                                    format!(
                                        "input '{input_name}' does not support stored source {:?}",
                                        value.source
                                    ),
                                ),
                            );
                        }
                    }
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
                        let expected = module
                            .active_parameter_value(input_name)
                            .map_or(input.value_type, |value| value.value_type());
                        let actual = parameter.default.value_type();
                        if actual != expected {
                            push_unique(
                                report,
                                Diagnostic::error(
                                    DiagnosticCode::ParameterTypeMismatch,
                                    binding_path,
                                    format!(
                                        "input '{input_name}' expects {:?}, but parameter '{}' is {actual:?}",
                                        expected, parameter.name
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
                let supported = matches!(
                    (&renderer.properties, renderer.renderer_type.0.as_str()),
                    (RendererProperties::Sprite, RENDERER_SPRITE)
                        | (RendererProperties::Flipbook { .. }, RENDERER_FLIPBOOK)
                        | (RendererProperties::Mesh { .. }, RENDERER_MESH)
                        | (
                            RendererProperties::Ribbon { .. },
                            aestra_core::RENDERER_RIBBON
                        )
                );
                if renderer.enabled && !supported {
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

    fn validate_project_parameter_overrides(
        &self,
        project: &ResolvedEffectProject,
    ) -> Result<(), ProjectCompileError> {
        for owner in std::iter::once(&project.root).chain(project.dependencies.values()) {
            let mut report = ValidationReport::default();
            for (clip_index, clip) in owner.effect_clips.iter().enumerate() {
                let Some(source) = project.effect(clip.source.id) else {
                    continue;
                };
                for (parameter_id, value) in &clip.parameter_overrides {
                    let path = format!(
                        "effect.effect_clips[{clip_index}].parameter_overrides.{parameter_id}"
                    );
                    let Some(parameter) = source
                        .parameters
                        .iter()
                        .find(|parameter| parameter.id == *parameter_id)
                    else {
                        report.push(Diagnostic::error(
                            DiagnosticCode::UnknownParameter,
                            path,
                            format!(
                                "effect clip override references missing source parameter {parameter_id}"
                            ),
                        ));
                        continue;
                    };
                    if !parameter.exposed {
                        report.push(Diagnostic::error(
                            DiagnosticCode::UnknownParameter,
                            path,
                            format!("source parameter '{}' is not exposed", parameter.name),
                        ));
                        continue;
                    }
                    let expected = parameter.default.value_type();
                    let actual = value.value_type();
                    if expected != actual {
                        report.push(Diagnostic::error(
                            DiagnosticCode::ParameterTypeMismatch,
                            path,
                            format!(
                                "source parameter '{}' expects {expected:?}, found {actual:?}",
                                parameter.name
                            ),
                        ));
                    }
                }
            }
            if !report.is_valid() {
                return Err(ProjectCompileError::Effect {
                    effect: owner.id,
                    source: CompileError::Validation(report),
                });
            }
        }
        Ok(())
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

fn derive_effect_requirements(emitters: &[CompiledEmitter]) -> EffectRequirements {
    let enabled = emitters.iter().filter(|emitter| emitter.enabled);
    let max_particles = enabled.clone().fold(0_usize, |total, emitter| {
        total.saturating_add(emitter.max_particles as usize)
    });
    let mut renderers = BTreeSet::new();
    for renderer in enabled.flat_map(|emitter| &emitter.renderers) {
        renderers.insert(match renderer.kind {
            RendererPlanKind::Sprite => RendererCapability::SpriteParticles,
            RendererPlanKind::Mesh { .. } => RendererCapability::MeshParticles,
            RendererPlanKind::Ribbon { .. } => RendererCapability::RibbonParticles,
            RendererPlanKind::Flipbook { .. } => RendererCapability::FlipbookParticles,
        });
    }
    EffectRequirements {
        max_particles,
        gpu_simulation: max_particles > 0,
        native_gpu_presentation: !renderers.is_empty(),
        renderers,
    }
}

fn populate_project_parameter_overrides(
    authored: &ResolvedEffectProject,
    compiled: &mut CompiledEffectProject,
) {
    let root_overrides = compile_parameter_overrides(&authored.root, compiled);
    let dependency_overrides = authored
        .dependencies
        .iter()
        .map(|(&id, effect)| (id, compile_parameter_overrides(effect, compiled)))
        .collect::<BTreeMap<_, _>>();

    apply_parameter_overrides(Arc::make_mut(&mut compiled.root), &root_overrides);
    for (id, overrides) in dependency_overrides {
        let effect = compiled
            .dependencies
            .get_mut(&id)
            .expect("resolved dependency must have a compiled artifact");
        apply_parameter_overrides(Arc::make_mut(effect), &overrides);
    }
}

fn compile_parameter_overrides(
    owner: &EffectAsset,
    project: &CompiledEffectProject,
) -> BTreeMap<aestra_core::EffectClipId, Vec<CompiledParameterOverride>> {
    owner
        .effect_clips
        .iter()
        .map(|clip| {
            let child = project
                .effect(clip.source.id)
                .expect("project resolution guarantees the child artifact exists");
            let overrides = clip
                .parameter_overrides
                .iter()
                .filter_map(|(parameter, value)| {
                    let slot = child.parameter_slots.get(parameter).copied()?;
                    Some(CompiledParameterOverride {
                        source: *parameter,
                        slot,
                        value: RuntimeValue::compile(value)
                            .expect("validated clip overrides are concrete runtime values"),
                    })
                })
                .collect();
            (clip.id, overrides)
        })
        .collect()
}

fn apply_parameter_overrides(
    effect: &mut CompiledEffect,
    overrides: &BTreeMap<aestra_core::EffectClipId, Vec<CompiledParameterOverride>>,
) {
    for clip in &mut effect.effect_clips {
        clip.parameter_overrides = overrides
            .get(&clip.source_clip)
            .cloned()
            .unwrap_or_default();
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
            spawn_rate: scalar_source(module, "spawn_rate", *spawn_rate, context)?,
            burst_count: expression(module, "burst_count", *burst_count, context),
        },
        ModuleParameters::Shape { shape } => Instruction::SampleShape {
            source: module.id,
            shape: expression(module, "shape", *shape, context),
        },
        ModuleParameters::Initialize {
            lifetime,
            speed,
            direction,
            spread_degrees,
            angular_velocity,
        } => Instruction::Initialize {
            source: module.id,
            lifetime: expression(
                module,
                "lifetime",
                sourced_range(module, "lifetime", *lifetime),
                context,
            ),
            speed: expression(
                module,
                "speed",
                sourced_range(module, "speed", *speed),
                context,
            ),
            direction: expression(module, "direction", *direction, context),
            spread_degrees: expression(module, "spread_degrees", *spread_degrees, context),
            angular_velocity: expression(
                module,
                "angular_velocity",
                sourced_range(module, "angular_velocity", *angular_velocity),
                context,
            ),
        },
        ModuleParameters::Motion {
            gravity,
            drag,
            turbulence,
        } => Instruction::Motion {
            source: module.id,
            gravity: vector_source(module, "gravity", *gravity, context)?,
            drag: scalar_source(module, "drag", *drag, context)?,
            turbulence: scalar_source(module, "turbulence", *turbulence, context)?,
        },
        ModuleParameters::Appearance {
            size,
            opacity,
            color,
        } => Instruction::Appearance {
            source: module.id,
            size: expression(module, "size", sourced_curve(module, "size", size), context),
            opacity: expression(
                module,
                "opacity",
                sourced_curve(module, "opacity", opacity),
                context,
            ),
            color: expression(
                module,
                "color",
                sourced_gradient(module, "color", color),
                context,
            ),
        },
        ModuleParameters::Custom(_) => return None,
    };
    Some(instruction)
}

fn sourced_range(module: &ModuleInstance, input: &str, range: ScalarRange) -> ScalarRange {
    match module.property_source(input) {
        Some(InputSourceKind::Constant) => {
            let value = (range.min + range.max) * 0.5;
            ScalarRange::new(value, value)
        }
        Some(InputSourceKind::RandomRange) => {
            let Some(Value::Range(active)) = module.active_parameter_value(input) else {
                return range;
            };
            active
        }
        _ => range,
    }
}

fn scalar_source(
    module: &ModuleInstance,
    input: &str,
    fallback: f32,
    context: &LoweringContext<'_>,
) -> Option<ScalarSource> {
    match module.property_source(input)? {
        InputSourceKind::Constant => Some(ScalarSource::Constant(expression(
            module, input, fallback, context,
        ))),
        InputSourceKind::RandomRange => {
            let aestra_core::Value::Range(range) = module.active_parameter_value(input)? else {
                return None;
            };
            Some(ScalarSource::RandomRange(expression(
                module, input, range, context,
            )))
        }
        InputSourceKind::Curve(domain) => {
            let aestra_core::Value::Curve(curve) = module.active_parameter_value(input)? else {
                return None;
            };
            Some(ScalarSource::Curve {
                value: expression(module, input, CompiledCurve::compile(&curve), context),
                domain,
            })
        }
        InputSourceKind::Gradient(_) => None,
    }
}

fn vector_source(
    module: &ModuleInstance,
    input: &str,
    fallback: [f32; 3],
    context: &LoweringContext<'_>,
) -> Option<VectorSource> {
    match module.property_source(input)? {
        InputSourceKind::Constant => Some(VectorSource::Constant(expression(
            module, input, fallback, context,
        ))),
        InputSourceKind::RandomRange => {
            let aestra_core::Value::Vec3Range(range) = module.active_parameter_value(input)? else {
                return None;
            };
            Some(VectorSource::RandomRange(expression(
                module, input, range, context,
            )))
        }
        InputSourceKind::Curve(domain) => {
            let aestra_core::Value::Vec3Curve(curve) = module.active_parameter_value(input)? else {
                return None;
            };
            Some(VectorSource::Curve {
                value: expression(module, input, CompiledVec3Curve::compile(&curve), context),
                domain,
            })
        }
        InputSourceKind::Gradient(_) => None,
    }
}

fn sourced_curve(module: &ModuleInstance, input: &str, curve: &Curve) -> CompiledCurve {
    match module.property_source(input) {
        Some(InputSourceKind::Constant) => {
            let constant = Curve::new(vec![CurveKey::new(0.0, curve.sample(0.0))]);
            CompiledCurve::compile(&constant)
        }
        Some(InputSourceKind::Curve(_)) => {
            let Some(Value::Curve(active)) = module.active_parameter_value(input) else {
                return CompiledCurve::compile(curve);
            };
            CompiledCurve::compile(&active)
        }
        _ => CompiledCurve::compile(curve),
    }
}

fn sourced_gradient(module: &ModuleInstance, input: &str, gradient: &Gradient) -> CompiledGradient {
    match module.property_source(input) {
        Some(InputSourceKind::Constant) => {
            let constant = Gradient::new(vec![ColorKey::new(0.0, gradient.sample(0.0))]);
            CompiledGradient::compile(&constant)
        }
        Some(InputSourceKind::Gradient(_)) => {
            let Some(Value::Gradient(active)) = module.active_parameter_value(input) else {
                return CompiledGradient::compile(gradient);
            };
            CompiledGradient::compile(&active)
        }
        _ => CompiledGradient::compile(gradient),
    }
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

fn collect_material_parameter<T>(input: &MaterialInput<T>, referenced: &mut BTreeSet<ParameterId>) {
    if let MaterialInput::Parameter(parameter) = input {
        referenced.insert(*parameter);
    }
}

fn material_expression<T>(input: &MaterialInput<T>, context: &LoweringContext<'_>) -> Expression<T>
where
    T: RuntimeParameterValue + Clone,
{
    match input {
        MaterialInput::Constant(value) => Expression::constant(value.clone()),
        MaterialInput::Parameter(parameter_id) => {
            if let Some(slot) = context.slots.get(parameter_id) {
                return Expression::parameter(*slot);
            }
            let parameter = context
                .parameters
                .get(parameter_id)
                .expect("validated material binding references an existing parameter");
            let runtime = RuntimeValue::compile(&parameter.default)
                .expect("validated material parameter has a concrete default");
            Expression::constant(
                T::from_runtime(&runtime)
                    .expect("validated material binding type matches its input")
                    .clone(),
            )
        }
    }
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
    fn scalar(source: &ScalarSource) -> (usize, usize) {
        match source {
            ScalarSource::Constant(value) => one(value),
            ScalarSource::RandomRange(value) => one(value),
            ScalarSource::Curve { value, .. } => one(value),
        }
    }
    fn vector(source: &VectorSource) -> (usize, usize) {
        match source {
            VectorSource::Constant(value) => one(value),
            VectorSource::RandomRange(value) => one(value),
            VectorSource::Curve { value, .. } => one(value),
        }
    }
    match instruction {
        Instruction::Emit {
            spawn_rate,
            burst_count,
            ..
        } => sum([scalar(spawn_rate), one(burst_count)]),
        Instruction::SampleShape { shape, .. } => one(shape),
        Instruction::Initialize {
            lifetime,
            speed,
            direction,
            spread_degrees,
            angular_velocity,
            ..
        } => sum([
            one(lifetime),
            one(speed),
            one(direction),
            one(spread_degrees),
            one(angular_velocity),
        ]),
        Instruction::Motion {
            gravity,
            drag,
            turbulence,
            ..
        } => sum([vector(gravity), scalar(drag), scalar(turbulence)]),
        Instruction::Appearance {
            size,
            opacity,
            color,
            ..
        } => sum([one(size), one(opacity), one(color)]),
    }
}

fn expression_count<T>(expression: &Expression<T>) -> (usize, usize) {
    match expression {
        Expression::Constant(_) => (1, 0),
        Expression::Parameter(_) => (0, 1),
    }
}

fn add_expression_counts(left: (usize, usize), right: (usize, usize)) -> (usize, usize) {
    (left.0 + right.0, left.1 + right.1)
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

fn input(
    name: &'static str,
    display_name: &'static str,
    description: &'static str,
    default_value: aestra_core::Value,
    control: InputControl,
) -> InputMetadata {
    let sources = match control {
        InputControl::Range { .. } => {
            vec![InputSourceKind::Constant, InputSourceKind::RandomRange]
        }
        InputControl::Curve { .. } => vec![
            InputSourceKind::Constant,
            InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
        ],
        InputControl::Gradient => vec![
            InputSourceKind::Constant,
            InputSourceKind::Gradient(InputEvaluationDomain::ParticleLife),
        ],
        _ => vec![InputSourceKind::Constant],
    };
    InputMetadata {
        name,
        display_name,
        description,
        value_type: default_value.value_type(),
        default_value,
        unit: None,
        control,
        sources,
    }
}

impl InputMetadata {
    /// Clones the catalog default while assigning fresh IDs to nested authored data.
    pub fn instantiate_default(&self) -> aestra_core::Value {
        let mut value = self.default_value.clone();
        match &mut value {
            aestra_core::Value::Curve(curve) => curve.id = CurveId::new(),
            aestra_core::Value::Vec3Curve(curve) => {
                for axis in &mut curve.curves {
                    axis.id = CurveId::new();
                }
            }
            aestra_core::Value::Gradient(gradient) => gradient.id = GradientId::new(),
            _ => {}
        }
        value
    }

    fn with_unit(mut self, unit: &'static str) -> Self {
        self.unit = Some(unit);
        self
    }

    fn with_sources(mut self, sources: Vec<InputSourceKind>) -> Self {
        self.sources = sources;
        self
    }
}

fn metadata(
    type_id: &'static str,
    display_name: &'static str,
    description: &'static str,
    category: &'static str,
    stage: StageKind,
) -> ModuleMetadata {
    ModuleMetadata {
        type_id: ModuleTypeId::new(type_id),
        display_name,
        description,
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
            "Controls continuous spawning and the initial particle burst.",
            "Emitter",
            StageKind::EmitterUpdate,
        )
        .with_inputs(vec![
            input(
                "spawn_rate",
                "Spawn Rate",
                "Particles emitted per second.",
                aestra_core::Value::Scalar(24.0),
                InputControl::Number {
                    step: 5.0,
                    min: Some(0.0),
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::EmitterTime),
            ])
            .with_unit("particles/s"),
            input(
                "burst_count",
                "Burst Count",
                "Particles emitted when the emitter starts.",
                aestra_core::Value::U32(0),
                InputControl::Number {
                    step: 4.0,
                    min: Some(0.0),
                    max: None,
                },
            ),
        ])
        .with_flow(vec![], vec![])
        .with_tags(vec!["spawn", "rate", "burst"])
        .with_cost(1),
        metadata(
            MODULE_SHAPE,
            "Shape",
            "Defines where newly spawned particles are placed.",
            "Spawn",
            StageKind::ParticleSpawn,
        )
        .with_inputs(vec![input(
            "shape",
            "Shape",
            "Volume used to place newly spawned particles.",
            aestra_core::Value::Shape(EmitterShape::Point),
            InputControl::Choice,
        )])
        .with_flow(vec![], vec![A::Position])
        .with_tags(vec!["spawn", "position"])
        .with_cost(2),
        metadata(
            MODULE_INITIALIZE,
            "Initialize Particle",
            "Sets lifetime, velocity, direction, and rotation for new particles.",
            "Spawn",
            StageKind::ParticleSpawn,
        )
        .with_inputs(vec![
            input(
                "lifetime",
                "Lifetime",
                "Minimum and maximum particle lifetime.",
                aestra_core::Value::Range(ScalarRange::new(0.8, 1.4)),
                InputControl::Range {
                    step: 0.1,
                    min: Some(0.05),
                    max: None,
                },
            )
            .with_unit("s"),
            input(
                "speed",
                "Speed",
                "Minimum and maximum initial particle speed.",
                aestra_core::Value::Range(ScalarRange::new(35.0, 70.0)),
                InputControl::Range {
                    step: 5.0,
                    min: Some(0.0),
                    max: None,
                },
            )
            .with_unit("units/s"),
            input(
                "direction",
                "Direction",
                "Central 3D launch direction.",
                aestra_core::Value::Vec3([0.0, 1.0, 0.0]),
                InputControl::Vector {
                    step: 0.1,
                    min: None,
                    max: None,
                },
            ),
            input(
                "spread_degrees",
                "Spread",
                "Angular launch cone around the central direction.",
                aestra_core::Value::Scalar(30.0),
                InputControl::Number {
                    step: 5.0,
                    min: Some(0.0),
                    max: Some(360.0),
                },
            )
            .with_unit("°"),
            input(
                "angular_velocity",
                "Angular Velocity",
                "Minimum and maximum particle spin.",
                aestra_core::Value::Range(ScalarRange::new(-1.0, 1.0)),
                InputControl::Range {
                    step: 0.1,
                    min: None,
                    max: None,
                },
            )
            .with_unit("rad/s"),
        ])
        .with_flow(
            vec![],
            vec![A::Velocity, A::Lifetime, A::Rotation, A::AngularVelocity],
        )
        .with_tags(vec!["spawn", "velocity", "lifetime"])
        .with_cost(4),
        metadata(
            MODULE_MOTION,
            "Motion",
            "Updates particle movement using gravity, drag, and procedural turbulence.",
            "Forces",
            StageKind::ParticleUpdate,
        )
        .with_inputs(vec![
            input(
                "gravity",
                "Gravity",
                "Constant acceleration applied to particle velocity.",
                aestra_core::Value::Vec3([0.0, -18.0, 0.0]),
                InputControl::Vector {
                    step: 5.0,
                    min: None,
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
            ])
            .with_unit("units/s²"),
            input(
                "drag",
                "Drag",
                "Velocity damping applied over time.",
                aestra_core::Value::Scalar(0.6),
                InputControl::Number {
                    step: 0.1,
                    min: Some(0.0),
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
            ]),
            input(
                "turbulence",
                "Turbulence",
                "Strength of deterministic procedural motion.",
                aestra_core::Value::Scalar(4.0),
                InputControl::Number {
                    step: 0.5,
                    min: None,
                    max: None,
                },
            )
            .with_sources(vec![
                InputSourceKind::Constant,
                InputSourceKind::RandomRange,
                InputSourceKind::Curve(InputEvaluationDomain::ParticleLife),
            ]),
        ])
        .with_flow(
            vec![A::Position, A::Velocity, A::Age],
            vec![A::Position, A::Velocity],
        )
        .with_tags(vec!["update", "force", "motion"])
        .with_cost(6),
        metadata(
            MODULE_APPEARANCE,
            "Appearance",
            "Controls particle size, opacity, and color.",
            "Appearance",
            StageKind::ParticleUpdate,
        )
        .with_inputs(vec![
            input(
                "size",
                "Size",
                "Particle size. The selected source controls how it varies.",
                aestra_core::Value::Curve(Curve {
                    id: CurveId::from_u128(0),
                    keys: vec![
                        CurveKey::new(0.0, 4.0),
                        CurveKey::new(0.35, 10.0),
                        CurveKey::new(1.0, 1.0),
                    ],
                    output_range: None,
                }),
                InputControl::Curve {
                    step: 0.5,
                    min: 0.0,
                    max: 32.0,
                },
            ),
            input(
                "opacity",
                "Opacity",
                "Particle opacity. The selected source controls how it varies.",
                aestra_core::Value::Curve(Curve {
                    id: CurveId::from_u128(0),
                    keys: vec![
                        CurveKey::new(0.0, 0.0),
                        CurveKey::new(0.12, 1.0),
                        CurveKey::new(1.0, 0.0),
                    ],
                    output_range: None,
                }),
                InputControl::Curve {
                    step: 0.05,
                    min: 0.0,
                    max: 1.0,
                },
            ),
            input(
                "color",
                "Color",
                "Particle color and alpha. The selected source controls how they vary.",
                aestra_core::Value::Gradient(Gradient {
                    id: GradientId::from_u128(0),
                    keys: vec![
                        ColorKey::new(0.0, [0.35, 0.75, 1.0, 1.0]),
                        ColorKey::new(0.5, [0.62, 0.3, 1.0, 1.0]),
                        ColorKey::new(1.0, [0.15, 0.05, 0.4, 0.0]),
                    ],
                }),
                InputControl::Gradient,
            ),
        ])
        .with_flow(vec![A::NormalizedAge], vec![A::Size, A::Color])
        .with_tags(vec!["update", "color", "size"])
        .with_cost(5),
    ]
}
