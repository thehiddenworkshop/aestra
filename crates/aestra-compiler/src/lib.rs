//! Module discovery, compiler validation, optimization, and typed lowering.

mod material_function;
mod material_function_graph;
mod material_graph;
mod material_ir;
mod material_reflection;
mod material_stack;
mod module_stack;
mod normal_map;
pub use normal_map::evaluate_normal_map;

pub use aestra_extension::sdk::*;
pub use material_function::*;
pub use material_function_graph::*;
pub use material_graph::*;
pub use material_ir::*;
pub use material_reflection::*;
pub use material_stack::*;
pub use module_stack::*;

use aestra_core::{
    Collider, ColorKey, Curve, CurveKey, Diagnostic, DiagnosticCode, EffectAsset, EffectParameter,
    Emitter, EmitterId, Gradient, MODULE_APPEARANCE, MODULE_COLLISION, MODULE_EMISSION,
    MODULE_INITIALIZE, MODULE_MOTION, MODULE_PERSISTENT, MODULE_SHAPE, MaterialInput,
    MaterialProgramId, MaterialProperties, ModuleInstance, ModuleParameters, ModuleTypeId,
    ParameterId, RENDERER_FLIPBOOK, RENDERER_MESH, RENDERER_RIBBON, RENDERER_SPRITE,
    RENDERER_TRAIL, RendererProperties, ScalarRange, SpriteColorSource, StageKind,
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
    SimulationClass, SimulationSeekMode, VectorSource,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;

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

/// The derived simulation classification of one authored emitter (hybrid roadmap M2). Produced by
/// analysis from the emitter's module requirements; not yet persisted in the compiled effect (the
/// island representation and its artifact support arrive in a later milestone). `promoted_by` names
/// the first module that lifted the emitter above `Analytic`, for promotion diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitterSimulationClass {
    pub emitter: EmitterId,
    pub name: String,
    pub class: SimulationClass,
    pub promoted_by: Option<ModuleTypeId>,
}

/// Frontend that validates authored semantics and emits immutable runtime plans.
#[derive(Debug, Clone)]
pub struct EffectCompiler {
    registry: ExtensionRegistry,
}

impl Default for EffectCompiler {
    /// The built-ins plus every extension linked into this process ([`link_extension`]).
    fn default() -> Self {
        Self::with_extensions(ExtensionRegistry::linked())
    }
}

impl EffectCompiler {
    /// Builds a compiler from a module registry, wrapping it in the unified extension registry with
    /// the built-in capability vocabulary. Module resolution flows through the unified registry.
    pub fn new(registry: ModuleRegistry) -> Self {
        Self::with_extensions(ExtensionRegistry::from_modules(registry))
    }

    /// Builds a compiler directly from a unified extension registry.
    pub fn with_extensions(registry: ExtensionRegistry) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &ModuleRegistry {
        &self.registry.modules
    }

    /// The unified extension registry backing this compiler.
    pub fn extensions(&self) -> &ExtensionRegistry {
        &self.registry
    }

    /// Classifies each enabled authored emitter by its derived [`SimulationClass`] (hybrid roadmap
    /// M2). The class aggregates the emitter's enabled modules' declared simulation requirements;
    /// `promoted_by` names the first module that lifted it above `Analytic`. Every current built-in
    /// module is analytic, so existing effects classify entirely as `Analytic`.
    pub fn classify_simulation(&self, asset: &EffectAsset) -> Vec<EmitterSimulationClass> {
        asset
            .emitters
            .iter()
            .filter(|emitter| emitter.enabled)
            .map(|emitter| {
                let (class, promoted_by) = self.emitter_simulation(emitter);
                EmitterSimulationClass {
                    emitter: emitter.id,
                    name: emitter.name.clone(),
                    class,
                    promoted_by,
                }
            })
            .collect()
    }

    /// Derives one emitter's simulation class by aggregating its enabled modules' requirements, and
    /// the first module that promoted it above `Analytic`. Shared by classification and lowering.
    fn emitter_simulation(&self, emitter: &Emitter) -> (SimulationClass, Option<ModuleTypeId>) {
        let mut requirements = SimulationRequirements::ANALYTIC;
        let mut promoted_by = None;
        for module in emitter.modules.iter().filter(|module| module.enabled) {
            let Some(metadata) = self.registry.modules.get(&module.module_type) else {
                continue;
            };
            let before = requirements.derived_class();
            requirements = requirements.max(metadata.simulation);
            if promoted_by.is_none() && requirements.derived_class() > before {
                promoted_by = Some(module.module_type.clone());
            }
        }
        (requirements.derived_class(), promoted_by)
    }

    /// Lowers an emitter's plugin simulation stages (extensible-stages M10): every enabled module in a
    /// stage whose type has a registered [`StageLowerer`] goes through its [`ModuleLowerer`], then the
    /// stage lowerer builds the stage's [`aestra_runtime::ExecutionBlock`], which must validate and use
    /// only registered resource types. Generic simulation stages (no lowerer) produce nothing.
    fn lower_extension_stages(
        &self,
        asset: &EffectAsset,
        bindings: &[aestra_runtime::CompiledBinding],
        emitter_index: usize,
        emitter: &Emitter,
    ) -> Result<Vec<aestra_runtime::CompiledExtensionStage>, Vec<Diagnostic>> {
        let mut names: Vec<&str> = Vec::new();
        for module in &emitter.modules {
            if let StageKind::Simulation(name) = &module.stage
                && !names.contains(&name.as_str())
            {
                names.push(name);
            }
        }
        let mut stages = Vec::new();
        let mut diagnostics = Vec::new();
        for name in names {
            let stage_type = emitter.simulation_stage_type(name);
            let Some(stage_lowerer) = self.registry.lowering.stage(&stage_type) else {
                continue;
            };
            let mut plans = Vec::new();
            for (module_index, module) in emitter.modules.iter().enumerate() {
                if !module.enabled
                    || !matches!(&module.stage, StageKind::Simulation(other) if other == name)
                {
                    continue;
                }
                let path = format!("effect.emitters[{emitter_index}].modules[{module_index}]");
                let failed = |message: String| {
                    Diagnostic::error(DiagnosticCode::LoweringFailed, path.clone(), message)
                };
                let (Some(lowerer), ModuleParameters::Custom(values)) = (
                    self.registry.lowering.module(&module.module_type),
                    &module.parameters,
                ) else {
                    diagnostics.push(failed(format!(
                        "module '{}' has no lowering for stage type '{}'",
                        module.module_type.0, stage_type.0
                    )));
                    continue;
                };
                let mut payload: aestra_core::PropertyBag = values
                    .iter()
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                if let Some(metadata) = self.registry.modules.get(&module.module_type) {
                    metadata.property_schema().apply_defaults(&mut payload);
                }
                match lowerer.lower(module, &payload) {
                    Ok(mut plan) => {
                        // Host-bound inputs keep their authored payload value as the fallback;
                        // the stage lowerer finds the field through `host_fields` (HB4).
                        plan.host_fields = module
                            .host_bindings
                            .iter()
                            .filter(|(input, _)| {
                                module.property_source(input) == Some(InputSourceKind::HostBinding)
                            })
                            .filter_map(|(input, reference)| {
                                Some((input.clone(), host_field_ref(asset, bindings, reference)?))
                            })
                            .collect();
                        plans.push(plan);
                    }
                    Err(message) => diagnostics.push(failed(message)),
                }
            }
            let stage_id = aestra_core::StageId::for_name(name);
            let block = match stage_lowerer.lower(&StageLoweringInput {
                stage: stage_id,
                stage_type: &stage_type,
                name,
                particle_capacity: emitter.max_particles,
                modules: &plans,
            }) {
                Ok(block) => block,
                Err(message) => {
                    diagnostics.push(Diagnostic::error(
                        DiagnosticCode::LoweringFailed,
                        format!("effect.emitters[{emitter_index}].simulation_stages.{name}"),
                        message,
                    ));
                    continue;
                }
            };
            let stage_path = format!("effect.emitters[{emitter_index}].simulation_stages.{name}");
            if let Err(error) = block.validate() {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::LoweringFailed,
                    stage_path.clone(),
                    format!(
                        "stage '{}' lowered to invalid Execution IR: {error}",
                        stage_type.0
                    ),
                ));
                continue;
            }
            if let Some(unknown) = block
                .resources
                .iter()
                .find(|resource| self.registry.resources.get(&resource.id).is_none())
            {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::LoweringFailed,
                    stage_path,
                    format!(
                        "stage '{}' declares unregistered resource type '{}'",
                        stage_type.0,
                        unknown.id.as_str()
                    ),
                ));
                continue;
            }
            if let Some(problem) = unresolved_program(&self.registry, &block.ops) {
                diagnostics.push(Diagnostic::error(
                    DiagnosticCode::LoweringFailed,
                    stage_path,
                    format!("stage '{}' {problem}", stage_type.0),
                ));
                continue;
            }
            let cpu_reference = self
                .registry
                .stages
                .get(&stage_type)
                .is_some_and(|descriptor| descriptor.backend.has_cpu_reference());
            stages.push(aestra_runtime::CompiledExtensionStage {
                id: stage_id,
                stage_type,
                name: name.to_string(),
                modules: plans,
                block,
                cpu_reference,
            });
        }
        if diagnostics.is_empty() {
            Ok(stages)
        } else {
            Err(diagnostics)
        }
    }

    /// The effect's aggregate simulation class — the strongest over its enabled emitters, `Analytic`
    /// when it has none. This is what determines the resolved seek strategy.
    fn aggregate_simulation_class(&self, asset: &EffectAsset) -> SimulationClass {
        self.classify_simulation(asset)
            .into_iter()
            .map(|classified| classified.class)
            .max()
            .unwrap_or(SimulationClass::Analytic)
    }

    /// Resolves and compiles a root effect together with all transitive reusable effects.
    pub fn compile_project(
        &self,
        root: &EffectAsset,
        index: &ProjectAssetIndex,
    ) -> Result<CompiledEffectProject, ProjectCompileError> {
        let resolved = index.resolve_effect_project(root)?;
        self.compile_resolved_project(&resolved)
    }

    /// Compile a project already resolved and dependency-validated by ProjectAssetIndex.
    /// Hosts may migrate material representations in memory before this step.
    pub fn compile_resolved_project(
        &self,
        resolved: &ResolvedEffectProject,
    ) -> Result<CompiledEffectProject, ProjectCompileError> {
        self.validate_project_parameter_overrides(resolved)?;
        self.validate_project_binding_forwards(resolved)?;
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
        populate_project_parameter_overrides(resolved, &mut project);
        populate_project_binding_forwards(resolved, &mut project);
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
        // Plugin payloads authored against an older schema compile through the plugin's migrations
        // (extensible-stages M11, §35) — on a copy, so compiling never rewrites the caller's document.
        let migrated;
        let (asset, migration) = if self.registry.needs_migration(asset) {
            let mut copy = asset.clone();
            let migration = self.registry.migrate_effect(&mut copy);
            migrated = copy;
            (&migrated, migration)
        } else {
            (asset, PayloadMigrationReport::default())
        };
        let mut report = asset.validation_report();
        for failure in migration.failed {
            push_unique(
                &mut report,
                Diagnostic::error(
                    DiagnosticCode::IncompatibleExtension,
                    failure.path,
                    format!(
                        "'{}' was authored against schema v{}, but the installed plugin needs v{}: {}",
                        failure.type_id, failure.from, failure.to, failure.message
                    ),
                ),
            );
        }
        self.validate_compiler_contracts(asset, &mut report);
        let mut expanded_programs = BTreeMap::new();
        let mut function_expansions = BTreeMap::new();
        let built_ins: BTreeMap<_, _> = asset
            .material_instances
            .iter()
            .filter_map(|instance| MaterialProgram::built_in(instance.program))
            .map(|program| (program.id, program))
            .collect();
        for program in material_programs
            .values()
            .filter(|program| !built_ins.contains_key(&program.id))
            .chain(built_ins.values())
        {
            let id = program.id;
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
                    let expected = if matches!(
                        renderer.properties,
                        RendererProperties::Ribbon { .. } | RendererProperties::Trail { .. }
                    ) {
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
        let bindings = self.compile_bindings(asset);
        let (host_fields, host_field_slots) =
            compile_host_fields(asset, &bindings, parameters.len());
        let context = LoweringContext {
            parameters: &parameter_lookup,
            slots: &parameter_slots,
            host_fields: &host_field_slots,
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
                // Marker modules (e.g. the Persistent solver) contribute no analytic instruction —
                // their effect is on the emitter's simulation class, resolved separately. Every other
                // validated built-in module lowers to an instruction.
                let Some(instruction) = lower_module(module, &context) else {
                    continue;
                };
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

            // Extensible-stages M8: extension (plugin) renderers lower generically into a separate
            // list, so no core `RendererPlanKind` variant is needed; built-ins lower through the typed
            // path below.
            let mut extension_renderers: Vec<aestra_runtime::CompiledExtensionRenderer> =
                Vec::new();
            let renderers: Vec<RendererPlan> = emitter
                .renderers
                .iter()
                .filter(|renderer| renderer.enabled)
                .filter_map(|renderer| {
                    Some(match &renderer.properties {
                        RendererProperties::Custom(values)
                            if self
                                .registry
                                .renderers
                                .is_extension(&renderer.renderer_type) =>
                        {
                            extension_renderers.push(aestra_runtime::CompiledExtensionRenderer {
                                source: renderer.id,
                                renderer_type: renderer.renderer_type.clone(),
                                material: renderer.material,
                                payload: values
                                    .iter()
                                    .map(|(name, value)| (name.clone(), value.clone()))
                                    .collect(),
                            });
                            return None;
                        }
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
                        RendererProperties::Ribbon {
                            width,
                            strand_count,
                        } => RendererPlan {
                            source: renderer.id,
                            material: renderer.material,
                            kind: RendererPlanKind::Ribbon {
                                width: *width,
                                strand_count: *strand_count,
                            },
                        },
                        RendererProperties::Trail {
                            width,
                            sample_interval,
                            lifetime,
                            max_points,
                            max_trails,
                            sampling,
                            sample_distance,
                            curve_tolerance,
                            uv_mode,
                            tile_length,
                            end_cap,
                        } => RendererPlan {
                            source: renderer.id,
                            material: renderer.material,
                            kind: RendererPlanKind::Trail {
                                width: *width,
                                sample_interval: *sample_interval,
                                sampling: *sampling,
                                sample_distance: *sample_distance,
                                curve_tolerance: *curve_tolerance,
                                uv_mode: *uv_mode,
                                tile_length: *tile_length,
                                end_cap: *end_cap,
                                lifetime: *lifetime,
                                max_points: *max_points,
                                max_trails: if *max_trails == 0 {
                                    emitter.max_particles
                                } else {
                                    *max_trails
                                },
                            },
                        },
                        RendererProperties::Mesh { asset } => RendererPlan {
                            source: renderer.id,
                            material: renderer.material,
                            kind: RendererPlanKind::Mesh { asset: *asset },
                        },
                        _ => unreachable!("compiler validation rejects unsupported renderers"),
                    })
                })
                .collect();
            // Every region of an emitter shares its modules, hence its simulation class (hybrid M3).
            let simulation_class = self.emitter_simulation(emitter).0;
            // Collision colliders (hybrid roadmap M10): gathered from the emitter's enabled collision
            // modules in order, so the stateful backend resolves them after each tick. Their presence
            // is also what promoted the emitter to a stateful class above.
            let colliders: Vec<Collider> = emitter
                .modules
                .iter()
                .filter(|module| module.enabled)
                .filter_map(|module| match &module.parameters {
                    ModuleParameters::Collision { colliders } => Some(colliders.iter().copied()),
                    _ => None,
                })
                .flatten()
                .collect();
            // Plugin simulation stages (extensible-stages M10) lower through their registered
            // lowerers into portable Execution IR, carried alongside the built-in lifecycle stages.
            let extension_stages = if emitter.enabled {
                self.lower_extension_stages(asset, &bindings, emitter_index, emitter)
                    .map_err(|diagnostics| {
                        let mut report = ValidationReport::default();
                        for diagnostic in diagnostics {
                            report.push(diagnostic);
                        }
                        CompileError::Validation(report)
                    })?
            } else {
                Vec::new()
            };
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
                    simulation_class,
                    colliders: colliders.clone(),
                    stages: aestra_runtime::CompiledLifecycleStages::from_execution_plan(
                        &execution, emitter.id,
                    ),
                    execution: execution.clone(),
                    renderers: renderers.clone(),
                    extension_renderers: extension_renderers.clone(),
                    extension_stages: extension_stages.clone(),
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
            host_transform_track: asset.host_transform_track.clone().map(|track| {
                std::sync::Arc::new(
                    aestra_runtime::CompiledHostTransformTrack::new(track)
                        .expect("validated host transform track"),
                )
            }),
            // Derived from the effect's simulation class (hybrid roadmap M2), replacing the former
            // unconditional StatelessDirect. History-dependent effects replay; choosing checkpoint
            // vs restart is a backend-capability decision (hybrid §5.3) made once a checkpoint-capable
            // backend exists. Every current effect is analytic, so this stays StatelessDirect.
            seek_mode: match self.aggregate_simulation_class(asset) {
                SimulationClass::Analytic => SimulationSeekMode::StatelessDirect,
                SimulationClass::Stateful | SimulationClass::Staged => {
                    SimulationSeekMode::RestartReplay
                }
            },
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
            binding_slots: bindings
                .iter()
                .enumerate()
                .map(|(index, binding)| (binding.source, aestra_runtime::BindingSlot(index)))
                .collect(),
            bindings,
            host_fields,
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
                    // Resolved against the child effect at project compilation (host bindings HB2).
                    binding_forwards: Vec::new(),
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

    /// Compiles the effect's host bindings into dense slots in declaration order (host bindings HB2).
    /// Each binding's layout packs its declared fields in the kind's field order, so the same
    /// declaration always yields the same record regardless of how the author listed its fields.
    fn compile_bindings(&self, asset: &EffectAsset) -> Vec<aestra_runtime::CompiledBinding> {
        asset
            .bindings
            .iter()
            .map(|binding| {
                let kind = self
                    .registry
                    .bindings
                    .get(&binding.kind)
                    .expect("validation rejects unregistered binding kinds");
                let declared: BTreeSet<_> = binding.fields().collect();
                let layout = aestra_runtime::BindingLayout::pack(
                    kind.fields
                        .iter()
                        .filter(|field| declared.contains(&field.id))
                        .map(|field| (field.id.clone(), field.value_type)),
                )
                .expect("registered binding kinds only declare packable fields");
                aestra_runtime::CompiledBinding {
                    source: binding.id,
                    name: binding.name.clone(),
                    kind: binding.kind.clone(),
                    update_mode: binding.update_mode,
                    required: binding.required,
                    required_fields: binding.required_fields.clone(),
                    optional_fields: binding.optional_fields.clone(),
                    layout,
                }
            })
            .collect()
    }

    /// A diagnostic for a type id no registered descriptor provides (extensible-stages M11). When the id
    /// is namespaced by a plugin that is not installed, it names the plugin and its recorded version
    /// requirement (`MissingExtension`); otherwise it is the ordinary unknown-type diagnostic.
    fn unregistered_type_diagnostic(
        &self,
        asset: &EffectAsset,
        type_id: &str,
        kind: &str,
        path: String,
        code: DiagnosticCode,
    ) -> Diagnostic {
        let Some(plugin) = aestra_core::plugin_of(type_id) else {
            return Diagnostic::error(code, path, format!("{kind} '{type_id}' is not registered"));
        };
        match self.registry.installed_manifest(&plugin) {
            None => {
                let requirement = asset
                    .extension_requirement(&plugin)
                    .map(|requirement| format!(" {}", requirement.version))
                    .unwrap_or_default();
                Diagnostic::error(
                    DiagnosticCode::MissingExtension,
                    path,
                    format!(
                        "{kind} '{type_id}' comes from plugin '{}{requirement}', which is not \
                         installed. Its authored data is preserved; install the plugin to compile it.",
                        plugin.as_str()
                    ),
                )
            }
            Some(manifest) => Diagnostic::error(
                code,
                path,
                format!(
                    "{kind} '{type_id}' is not provided by the installed plugin '{}' {}",
                    plugin.as_str(),
                    manifest.version
                ),
            ),
        }
    }

    /// "plugin 'id' version" for a type's installed provider, or "the installed registry".
    fn provider_label(&self, type_id: &str) -> String {
        self.registry.provider_of(type_id).map_or_else(
            || "the installed registry".to_string(),
            |manifest| format!("plugin '{}' {}", manifest.plugin.as_str(), manifest.version),
        )
    }

    fn validate_compiler_contracts(&self, asset: &EffectAsset, report: &mut ValidationReport) {
        // Recorded plugin requirements (extensible-stages M11, §21). A missing plugin is reported per
        // type it provides (below), which names exactly what is unavailable; here only a present but
        // incompatible plugin, or a malformed requirement, is reported.
        let referenced = asset.referenced_plugins();
        for (index, requirement) in asset.extensions.iter().enumerate() {
            let path = format!("effect.extensions[{index}]");
            match self.registry.requirement_status(requirement) {
                RequirementStatus::Invalid(error) => push_unique(
                    report,
                    Diagnostic::error(
                        DiagnosticCode::InvalidValue,
                        format!("{path}.version"),
                        format!(
                            "'{}' is not a valid version requirement: {error}",
                            requirement.version
                        ),
                    ),
                ),
                RequirementStatus::Incompatible(manifest)
                    if referenced.contains(&requirement.plugin) =>
                {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::IncompatibleExtension,
                            path,
                            format!(
                                "the effect requires plugin '{}' {}, but version {} is installed",
                                requirement.plugin.as_str(),
                                requirement.version,
                                manifest.version
                            ),
                        ),
                    )
                }
                _ => {}
            }
        }
        // Host binding declarations (host bindings HB1): the kind must be registered — a plugin kind
        // whose extension is missing is a `MissingExtension`, with the declaration preserved — and
        // every declared field must be one the kind supplies.
        for (index, binding) in asset.bindings.iter().enumerate() {
            let path = format!("effect.bindings[{index}]");
            let Some(kind) = self.registry.bindings.get(&binding.kind) else {
                push_unique(
                    report,
                    self.unregistered_type_diagnostic(
                        asset,
                        binding.kind.as_str(),
                        "binding kind",
                        format!("{path}.kind"),
                        DiagnosticCode::InvalidReference,
                    ),
                );
                continue;
            };
            for field in binding.fields() {
                if kind.field(field).is_none() {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            format!("{path}.fields.{}", field.as_str()),
                            format!(
                                "binding '{}': kind '{}' does not supply field '{}'",
                                binding.name,
                                kind.type_id.as_str(),
                                field.as_str()
                            ),
                        ),
                    );
                }
            }
        }
        for (emitter_index, emitter) in asset.emitters.iter().enumerate() {
            let emitter_path = format!("effect.emitters[{emitter_index}]");
            for (module_index, module) in emitter.modules.iter().enumerate() {
                let path = format!("{emitter_path}.modules[{module_index}]");
                // The host stage's provided capabilities come from its registered stage type
                // (extensible-stages M10), so a plugin stage hosts exactly what it declares.
                let stage_type = emitter.stage_type_of(&module.stage);
                let stage = self.registry.stages.get(&stage_type);
                if stage.is_none() {
                    push_unique(
                        report,
                        self.unregistered_type_diagnostic(
                            asset,
                            &stage_type.0,
                            "stage type",
                            format!("{path}.stage"),
                            DiagnosticCode::UnknownStage,
                        ),
                    );
                }
                let Some(metadata) = self.registry.modules.get(&module.module_type) else {
                    push_unique(
                        report,
                        self.unregistered_type_diagnostic(
                            asset,
                            &module.module_type.0,
                            "module",
                            format!("{path}.module_type"),
                            DiagnosticCode::UnknownModule,
                        ),
                    );
                    continue;
                };
                let Some(stage) = stage else {
                    continue;
                };
                // A plugin payload must match the installed schema (extensible-stages M11, §35): a
                // newer one is preserved but not compiled; an older one that could not be migrated
                // is reported (compilation already tried the plugin's migrations).
                match self.registry.module_schema_status(module) {
                    Some(SchemaStatus::Newer { stored, current }) => {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::IncompatibleExtension,
                                path.clone(),
                                format!(
                                    "module '{}' was saved with schema v{stored}, but {} provides v{current}. \
                                     Its data is preserved; update the plugin to edit or compile it.",
                                    module.module_type.0,
                                    self.provider_label(&module.module_type.0)
                                ),
                            ),
                        );
                        continue;
                    }
                    Some(SchemaStatus::Older { stored, current }) => {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::IncompatibleExtension,
                                path.clone(),
                                format!(
                                    "module '{}' was saved with schema v{stored} and cannot be migrated \
                                     to v{current}",
                                    module.module_type.0
                                ),
                            ),
                        );
                        continue;
                    }
                    _ => {}
                }
                // Capability-based compatibility (extensible-stages M4): the module is valid here when
                // its required capabilities are satisfied by what its host stage provides — never a
                // hardcoded stage-type check. For built-ins this is equivalent to the old
                // `stages.contains(module.stage)`, and it also lets third-party stages host standard
                // modules by providing the right capabilities.
                if !stage.hosts(&metadata.required_capabilities()) {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::StageMismatch,
                            format!("{path}.stage"),
                            format!(
                                "module '{}' cannot execute in stage {:?}: its required capabilities \
                                 are not provided there",
                                module.module_type.0, module.stage
                            ),
                        ),
                    );
                }
                if module.enabled
                    && let ModuleParameters::Custom(values) = &module.parameters
                    && !is_builtin_module(&module.module_type)
                {
                    // A plugin module's payload is validated against its declared schema (M10).
                    let bag: aestra_core::PropertyBag = values
                        .iter()
                        .map(|(name, value)| (name.clone(), value.clone()))
                        .collect();
                    for issue in metadata.property_schema().validate(&bag) {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::InvalidValue,
                                format!("{path}.parameters.{}", issue.property),
                                format!(
                                    "module '{}' input '{}': {:?}",
                                    module.module_type.0, issue.property, issue.problem
                                ),
                            ),
                        );
                    }
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
                // Host-bound inputs (host bindings HB4): the input accepts the source, the binding is
                // declared with the field, and the field's type matches the input's.
                for (input_name, reference) in &module.host_bindings {
                    let host_path = format!("{path}.host_bindings.{input_name}");
                    let Some(input) = metadata
                        .inputs
                        .iter()
                        .find(|input| input.name == input_name)
                    else {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::UnknownParameter,
                                host_path,
                                format!(
                                    "module '{}' has no registered input named '{input_name}'",
                                    module.module_type.0
                                ),
                            ),
                        );
                        continue;
                    };
                    if !input.sources.contains(&InputSourceKind::HostBinding) {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::InvalidValue,
                                host_path.clone(),
                                format!("input '{input_name}' does not accept host bindings"),
                            ),
                        );
                    }
                    let Some(binding) = asset
                        .bindings
                        .iter()
                        .find(|binding| binding.id == reference.binding)
                    else {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::InvalidReference,
                                host_path,
                                format!("binding {} is not declared", reference.binding),
                            ),
                        );
                        continue;
                    };
                    if !binding.fields().any(|field| *field == reference.field) {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::InvalidReference,
                                host_path,
                                format!(
                                    "binding '{}' does not declare field '{}'",
                                    binding.name,
                                    reference.field.as_str()
                                ),
                            ),
                        );
                        continue;
                    }
                    if let Some(field_type) = self.registry.bindings.field_type(&reference.field)
                        && field_type != input.value_type
                    {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::ParameterTypeMismatch,
                                host_path,
                                format!(
                                    "input '{input_name}' is {:?} but field '{}' is {field_type:?}",
                                    input.value_type,
                                    reference.field.as_str()
                                ),
                            ),
                        );
                    }
                }
                for input in &metadata.inputs {
                    if module.enabled
                        && module.property_source(input.name) == Some(InputSourceKind::HostBinding)
                        && !module.host_bindings.contains_key(input.name)
                    {
                        push_unique(
                            report,
                            Diagnostic::error(
                                DiagnosticCode::InvalidValue,
                                format!("{path}.host_bindings.{}", input.name),
                                format!(
                                    "input '{}' reads a host binding but names no binding field",
                                    input.name
                                ),
                            ),
                        );
                    }
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

            // Singleton validation (extensible-stages M4): a `Single`-multiplicity module may appear
            // at most once per stage. Report once, at the first offending occurrence.
            for (module_index, module) in emitter.modules.iter().enumerate() {
                let Some(metadata) = self.registry.modules.get(&module.module_type) else {
                    continue;
                };
                if metadata.multiplicity != ModuleMultiplicity::Single {
                    continue;
                }
                // Only report from the first occurrence of this type+stage.
                let first = emitter.modules.iter().position(|other| {
                    other.module_type == module.module_type && other.stage == module.stage
                });
                if first != Some(module_index) {
                    continue;
                }
                let count = emitter
                    .modules
                    .iter()
                    .filter(|other| {
                        other.module_type == module.module_type && other.stage == module.stage
                    })
                    .count();
                if count > 1 {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::InvalidValue,
                            format!("{emitter_path}.modules[{module_index}]"),
                            format!(
                                "module '{}' is a singleton but appears more than once in stage {:?}",
                                module.module_type.0, module.stage
                            ),
                        ),
                    );
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
                    && let RendererProperties::Ribbon { strand_count, .. } = renderer.properties
                    && emitter.renderers.iter().any(|other| {
                        other.enabled
                            && matches!(other.properties,
                            RendererProperties::Ribbon { strand_count: other_count, .. }
                            if other_count != strand_count)
                    })
                {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::UnsupportedRenderer,
                            format!(
                                "{emitter_path}.renderers[{renderer_index}].properties.strand_count"
                            ),
                            "Ribbon renderers on the same emitter must use the same strand count because they share particle links",
                        ),
                    );
                }
                if let RendererProperties::Trail { max_trails, .. } = renderer.properties
                    && max_trails != 0
                    && max_trails < emitter.max_particles
                {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::InvalidValue,
                            format!(
                                "{emitter_path}.renderers[{renderer_index}].properties.max_trails"
                            ),
                            "Maximum Trails must cover every live parent; increase it above particle capacity to retain retired tails",
                        ),
                    );
                }
                if renderer.enabled
                    && matches!(renderer.properties, RendererProperties::Trail { .. })
                    && (emitter.max_particles > 256
                        || emitter
                            .renderers
                            .iter()
                            .filter(|r| {
                                r.enabled
                                    && matches!(r.properties, RendererProperties::Trail { .. })
                            })
                            .count()
                            > 1)
                {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::UnsupportedRenderer,
                            format!("{emitter_path}.renderers[{renderer_index}]"),
                            "trail history currently supports one Trail renderer per emitter and at most 256 parent particles",
                        ),
                    );
                }
                let builtin_supported = matches!(
                    (&renderer.properties, renderer.renderer_type.0.as_str()),
                    (RendererProperties::Sprite, RENDERER_SPRITE)
                        | (RendererProperties::Flipbook { .. }, RENDERER_FLIPBOOK)
                        | (RendererProperties::Mesh { .. }, RENDERER_MESH)
                        | (RendererProperties::Trail { .. }, RENDERER_TRAIL)
                        | (RendererProperties::Ribbon { .. }, RENDERER_RIBBON)
                );
                // Extensible-stages M8: a registered plugin renderer (a Custom payload whose type id is
                // a registered extension renderer) is supported — validated through the registry, not a
                // hardcoded type check.
                let extension_supported =
                    matches!(renderer.properties, RendererProperties::Custom(_))
                        && self
                            .registry
                            .renderers
                            .is_extension(&renderer.renderer_type);
                let supported = builtin_supported || extension_supported;
                let renderer_path = format!("{emitter_path}.renderers[{renderer_index}]");
                if renderer.enabled
                    && !supported
                    && aestra_core::plugin_of(&renderer.renderer_type.0).is_some()
                    && self
                        .registry
                        .renderers
                        .get(&renderer.renderer_type)
                        .is_none()
                {
                    // A plugin renderer whose plugin is missing (extensible-stages M11).
                    push_unique(
                        report,
                        self.unregistered_type_diagnostic(
                            asset,
                            &renderer.renderer_type.0,
                            "renderer",
                            format!("{renderer_path}.renderer_type"),
                            DiagnosticCode::UnsupportedRenderer,
                        ),
                    );
                } else if renderer.enabled && !supported {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::UnsupportedRenderer,
                            format!("{renderer_path}.renderer_type"),
                            format!(
                                "renderer '{}' is not supported by the current runtime",
                                renderer.renderer_type.0
                            ),
                        ),
                    );
                }
                if renderer.enabled
                    && let Some(
                        SchemaStatus::Newer { stored, current }
                        | SchemaStatus::Older { stored, current },
                    ) = self.registry.renderer_schema_status(renderer)
                {
                    push_unique(
                        report,
                        Diagnostic::error(
                            DiagnosticCode::IncompatibleExtension,
                            renderer_path,
                            format!(
                                "renderer '{}' was saved with schema v{stored}, but {} provides \
                                 v{current} and no migration applies. Its data is preserved.",
                                renderer.renderer_type.0,
                                self.provider_label(&renderer.renderer_type.0)
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

    /// Validates child-effect binding forwards (host bindings HB2, roadmap §9.6): every forward names
    /// a child binding of the same kind whose required fields the parent binding also requires, and
    /// every required child binding is forwarded — otherwise the host, which binds only the root,
    /// could never fill it.
    fn validate_project_binding_forwards(
        &self,
        project: &ResolvedEffectProject,
    ) -> Result<(), ProjectCompileError> {
        for owner in std::iter::once(&project.root).chain(project.dependencies.values()) {
            let mut report = ValidationReport::default();
            for (clip_index, clip) in owner.effect_clips.iter().enumerate() {
                let Some(child) = project.effect(clip.source.id) else {
                    continue;
                };
                let clip_path = format!("effect.effect_clips[{clip_index}]");
                for (child_id, parent_id) in &clip.binding_forwards {
                    let path = format!("{clip_path}.binding_forwards.{child_id}");
                    let Some(child_binding) = child
                        .bindings
                        .iter()
                        .find(|binding| binding.id == *child_id)
                    else {
                        report.push(Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            path,
                            format!(
                                "child effect '{}' declares no binding {child_id}",
                                child.name
                            ),
                        ));
                        continue;
                    };
                    // A missing parent binding is reported by the owner's own validation.
                    let Some(parent_binding) = owner
                        .bindings
                        .iter()
                        .find(|binding| binding.id == *parent_id)
                    else {
                        continue;
                    };
                    if child_binding.kind != parent_binding.kind {
                        report.push(Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            path,
                            format!(
                                "child binding '{}' is {} but parent binding '{}' is {}",
                                child_binding.name,
                                child_binding.kind.as_str(),
                                parent_binding.name,
                                parent_binding.kind.as_str()
                            ),
                        ));
                        continue;
                    }
                    if let Some(field) = child_binding
                        .required_fields
                        .iter()
                        .find(|field| !parent_binding.required_fields.contains(*field))
                    {
                        report.push(Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            path,
                            format!(
                                "child binding '{}' requires field '{}', which parent binding '{}' does not require",
                                child_binding.name,
                                field.as_str(),
                                parent_binding.name
                            ),
                        ));
                    }
                }
                for binding in child.bindings.iter().filter(|binding| binding.required) {
                    if !clip.binding_forwards.contains_key(&binding.id) {
                        report.push(Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            format!("{clip_path}.binding_forwards"),
                            format!(
                                "child effect '{}' requires binding '{}'; forward one of this effect's bindings to it",
                                child.name, binding.name
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
                let Some(metadata) = self.registry.modules.get(&module.module_type) else {
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
            if let Some(metadata) = self.registry.modules.get(&module.module_type) {
                discovered.extend(metadata.reads.iter().copied());
                discovered.extend(metadata.writes.iter().copied());
            }
        }

        for stage in [StageKind::ParticleUpdate, StageKind::ParticleSpawn] {
            for module in modules.iter().rev() {
                if !module.enabled || module.stage != stage {
                    continue;
                }
                let Some(metadata) = self.registry.modules.get(&module.module_type) else {
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
            RendererPlanKind::Ribbon { .. } | RendererPlanKind::Trail { .. } => {
                RendererCapability::RibbonParticles
            }
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

/// Resolves every clip's binding forwards into child and parent slots (host bindings HB2).
fn populate_project_binding_forwards(
    authored: &ResolvedEffectProject,
    compiled: &mut CompiledEffectProject,
) {
    let owners: Vec<&EffectAsset> = std::iter::once(&authored.root)
        .chain(authored.dependencies.values())
        .collect();
    for owner in owners {
        let forwards: BTreeMap<
            aestra_core::EffectClipId,
            Vec<aestra_runtime::CompiledBindingForward>,
        > = owner
            .effect_clips
            .iter()
            .map(|clip| {
                let child = compiled
                    .effect(clip.source.id)
                    .expect("project resolution guarantees the child artifact exists");
                let parent = compiled
                    .effect(owner.id)
                    .expect("every owner in the project is compiled");
                let forwards = clip
                    .binding_forwards
                    .iter()
                    .filter_map(|(child_id, parent_id)| {
                        Some(aestra_runtime::CompiledBindingForward {
                            child: *child_id,
                            child_slot: *child.binding_slots.get(child_id)?,
                            parent_slot: *parent.binding_slots.get(parent_id)?,
                        })
                    })
                    .collect();
                (clip.id, forwards)
            })
            .collect();
        let effect = if compiled.root.source == owner.id {
            Arc::make_mut(&mut compiled.root)
        } else {
            Arc::make_mut(
                compiled
                    .dependencies
                    .get_mut(&owner.id)
                    .expect("resolved dependency must have a compiled artifact"),
            )
        };
        for clip in &mut effect.effect_clips {
            clip.binding_forwards = forwards.get(&clip.source_clip).cloned().unwrap_or_default();
        }
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
    /// Built-in module inputs read from host binding fields (host bindings HB4).
    host_fields: &'a BTreeMap<(aestra_core::ModuleId, String), aestra_runtime::HostFieldSlot>,
}

/// Where a host field lives in its compiled binding, or `None` if the reference is not resolvable
/// (validation reports why).
fn host_field_ref(
    asset: &EffectAsset,
    bindings: &[aestra_runtime::CompiledBinding],
    reference: &aestra_core::HostFieldRef,
) -> Option<aestra_runtime::CompiledHostFieldRef> {
    let slot = asset
        .bindings
        .iter()
        .position(|binding| binding.id == reference.binding)?;
    let (index, packed) = bindings.get(slot)?.layout.field(&reference.field)?;
    Some(aestra_runtime::CompiledHostFieldRef {
        binding: aestra_runtime::BindingSlot(slot),
        field: reference.field.clone(),
        value_type: packed.value_type,
        field_index: index as u32,
        offset: packed.offset,
    })
}

/// Assigns an input-table slot to every enabled built-in module input whose active source is
/// `HostBinding` (host bindings HB4). Slots follow the parameters; each keeps the input's authored
/// value as its fallback. Plugin modules get their host fields through `ExtensionModulePlan`.
fn compile_host_fields(
    asset: &EffectAsset,
    bindings: &[aestra_runtime::CompiledBinding],
    parameter_count: usize,
) -> (
    Vec<aestra_runtime::CompiledHostField>,
    BTreeMap<(aestra_core::ModuleId, String), aestra_runtime::HostFieldSlot>,
) {
    let mut fields = Vec::new();
    let mut slots = BTreeMap::new();
    for module in asset
        .emitters
        .iter()
        .flat_map(|emitter| emitter.modules.iter())
        .filter(|module| module.enabled && is_builtin_module(&module.module_type))
    {
        for (input, reference) in &module.host_bindings {
            if module.property_source(input) != Some(InputSourceKind::HostBinding) {
                continue;
            }
            let (Some(source), Some(fallback)) = (
                host_field_ref(asset, bindings, reference),
                module
                    .parameter_value(input)
                    .as_ref()
                    .and_then(RuntimeValue::compile),
            ) else {
                continue;
            };
            slots.insert(
                (module.id, input.clone()),
                aestra_runtime::HostFieldSlot(parameter_count + fields.len()),
            );
            fields.push(aestra_runtime::CompiledHostField { source, fallback });
        }
    }
    (fields, slots)
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
        // The persistent-state solver contributes no analytic instruction: its effect is to promote
        // the emitter's simulation class (handled in classification), and the stateful GPU backend
        // drives the dynamics from the emitter's other modules.
        ModuleParameters::Persistent {} => return None,
        // Collision, like the persistent solver, contributes no analytic instruction: it promotes the
        // emitter's class (handled in classification) and its colliders are gathered onto the compiled
        // emitter for the stateful GPU backend to resolve after each tick (hybrid roadmap M10).
        ModuleParameters::Collision { .. } => return None,
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
        InputSourceKind::Constant | InputSourceKind::HostBinding => Some(ScalarSource::Constant(
            expression(module, input, fallback, context),
        )),
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
        InputSourceKind::Constant | InputSourceKind::HostBinding => Some(VectorSource::Constant(
            expression(module, input, fallback, context),
        )),
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
    if module.property_source(input) == Some(InputSourceKind::HostBinding)
        && let Some(slot) = context.host_fields.get(&(module.id, input.to_string()))
    {
        return Expression::HostField(*slot);
    }
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
            Expression::Parameter(_) | Expression::HostField(_) => (0, 1),
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
        Expression::Parameter(_) | Expression::HostField(_) => (0, 1),
    }
}

fn add_expression_counts(left: (usize, usize), right: (usize, usize)) -> (usize, usize) {
    (left.0 + right.0, left.1 + right.1)
}

/// Whether the type id is one of the core modules the compiler lowers through typed parameters.
fn is_builtin_module(type_id: &ModuleTypeId) -> bool {
    matches!(
        type_id.0.as_str(),
        MODULE_EMISSION
            | MODULE_SHAPE
            | MODULE_INITIALIZE
            | MODULE_MOTION
            | MODULE_PERSISTENT
            | MODULE_COLLISION
            | MODULE_APPEARANCE
    )
}

fn parameters_match(module: &ModuleInstance) -> bool {
    // A plugin module carries a generic payload validated against its schema (M10).
    if !is_builtin_module(&module.module_type) {
        return matches!(module.parameters, ModuleParameters::Custom(_));
    }
    matches!(
        (&*module.module_type.0, &module.parameters),
        (MODULE_EMISSION, ModuleParameters::Emission { .. })
            | (MODULE_SHAPE, ModuleParameters::Shape { .. })
            | (MODULE_INITIALIZE, ModuleParameters::Initialize { .. })
            | (MODULE_MOTION, ModuleParameters::Motion { .. })
            | (MODULE_PERSISTENT, ModuleParameters::Persistent {})
            | (MODULE_COLLISION, ModuleParameters::Collision { .. })
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

/// The first compute op naming a program that is not registered, or an entry point its program does
/// not declare (extensible-stages M13, §13.2). Recurses into repeat bodies.
fn unresolved_program(
    registry: &ExtensionRegistry,
    ops: &[aestra_runtime::ExecutionOp],
) -> Option<String> {
    ops.iter().find_map(|op| match op {
        aestra_runtime::ExecutionOp::Compute(compute) => {
            let program_id = compute.program.as_ref()?;
            let Some(program) = registry.programs.get(program_id) else {
                return Some(format!(
                    "references unregistered compute program '{}'",
                    program_id.as_str()
                ));
            };
            (!program.entry_points.contains(&compute.entry_point)).then(|| {
                format!(
                    "calls entry point '{}', which program '{}' does not declare",
                    compute.entry_point,
                    program_id.as_str()
                )
            })
        }
        aestra_runtime::ExecutionOp::Repeat { body, .. } => unresolved_program(registry, body),
        _ => None,
    })
}
