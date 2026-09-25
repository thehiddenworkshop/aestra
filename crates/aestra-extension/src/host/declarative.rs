//! A packaged extension turned into an [`AestraExtension`] (extensible-stages M12): its declarations
//! become the same descriptors, lowerers and migrations a linked Rust extension registers, so the
//! compiler, editor and every other consumer cannot tell the two apart.

use super::package::{
    AccessModeDecl, DispatchDecl, LifetimeDecl, MigrationOp, MigrationStep, OpTemplate,
    PackageContent, PackageManifest, RequiresDecl, StageTemplate,
};
use crate::{
    AestraExtension, BindingFieldDescriptor, BindingKindDescriptor, CapabilityExpression,
    CapabilitySet, DomainDescriptor, ExtensionManifest, ExtensionRegistry, InputControl,
    InputMetadata, ModuleLowerer, ModuleMetadata, PayloadMigration, RegistryConflict,
    RendererDescriptor, ResourceTypeDescriptor, StageLowerer, StageLoweringInput,
    StageTypeDescriptor,
};
use aestra_core::{
    BindingFieldId, BindingKindId, CapabilityId, DomainTypeId, ModuleInstance, ModuleTypeId,
    PropertyBag, PropertyControl, PropertyDescriptor, PropertySchema, PropertySource,
    RendererTypeId, ResourceTypeId, StageTypeId, Value,
};
use aestra_runtime::{
    ComputeOp, CopyOp, ExecutionBlock, ExecutionOp, ExtensionModulePlan, RepeatPolicy,
    ResourceAccess, ResourceAccessMode, ResourceDescriptor, ResourceLifetime, StagedDispatch,
};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A validated, ready-to-register packaged extension. Built once when the package is loaded; every
/// `register` call (the linked registry is rebuilt on demand) only clones prepared descriptors.
pub struct DeclarativeExtension {
    manifest: PackageManifest,
    capabilities: Vec<CapabilityId>,
    domains: Vec<DomainDescriptor>,
    resources: Vec<ResourceTypeDescriptor>,
    stages: Vec<(StageTypeDescriptor, Option<Arc<dyn StageLowerer>>)>,
    modules: Vec<PreparedModule>,
    renderers: Vec<(RendererDescriptor, Option<Arc<dyn PayloadMigration>>)>,
    binding_kinds: Vec<BindingKindDescriptor>,
    required_capabilities: Vec<CapabilityId>,
}

/// A package module ready to register: its descriptor, lowerer and optional payload migration.
struct PreparedModule {
    metadata: ModuleMetadata,
    lowerer: Arc<dyn ModuleLowerer>,
    migration: Option<Arc<dyn PayloadMigration>>,
}

impl std::fmt::Debug for DeclarativeExtension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeclarativeExtension")
            .field("id", &self.manifest.id)
            .field("version", &self.manifest.version)
            .finish_non_exhaustive()
    }
}

/// `ModuleMetadata`/`InputMetadata` carry `&'static str` labels, as the built-in catalog does. A
/// package's strings are leaked once, when the package is loaded — bounded by the installed packages,
/// never per registration.
fn leak(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}

fn lifetime(lifetime: LifetimeDecl) -> ResourceLifetime {
    match lifetime {
        LifetimeDecl::Persistent => ResourceLifetime::Persistent,
        LifetimeDecl::Transient => ResourceLifetime::Transient,
    }
}

fn capability_set(ids: &[String]) -> CapabilitySet {
    CapabilitySet::new(ids.iter().map(CapabilityId::new))
}

fn input_control(control: &PropertyControl) -> InputControl {
    match control {
        PropertyControl::Toggle => InputControl::Toggle,
        PropertyControl::Number { step, min, max } => InputControl::Number {
            step: *step,
            min: *min,
            max: *max,
        },
        PropertyControl::Vector { step, min, max } => InputControl::Vector {
            step: *step,
            min: *min,
            max: *max,
        },
        PropertyControl::Range { step, min, max } => InputControl::Range {
            step: *step,
            min: *min,
            max: *max,
        },
        PropertyControl::Choice { .. } => InputControl::Choice,
        PropertyControl::Curve { step, min, max } => InputControl::Curve {
            step: *step,
            min: *min,
            max: *max,
        },
        PropertyControl::Gradient => InputControl::Gradient,
        PropertyControl::Reference => InputControl::Reference,
    }
}

fn input_metadata(descriptor: &PropertyDescriptor) -> InputMetadata {
    InputMetadata {
        name: leak(&descriptor.name),
        display_name: leak(&descriptor.label),
        description: leak(&descriptor.description),
        value_type: descriptor.value_type,
        default_value: descriptor.default.clone(),
        unit: descriptor.unit.as_deref().map(leak),
        control: input_control(&descriptor.control),
        sources: if descriptor.sources.is_empty() {
            vec![PropertySource::Constant]
        } else {
            descriptor.sources.clone()
        },
    }
}

/// Checks a schema's own defaults validate, and its migrations are consistent with its version.
fn check_schema(
    owner: &str,
    schema: &PropertySchema,
    migrations: &[MigrationStep],
) -> Result<(), String> {
    let issues = schema.validate(&schema.default_bag());
    if let Some(issue) = issues.first() {
        return Err(format!(
            "{owner}: the default of '{}' is invalid ({:?})",
            issue.property, issue.problem
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for step in migrations {
        if step.from == 0 || step.from >= schema.schema_version {
            return Err(format!(
                "{owner}: migration from v{} is outside schema versions 1..{}",
                step.from, schema.schema_version
            ));
        }
        if !seen.insert(step.from) {
            return Err(format!(
                "{owner}: two migrations from schema v{}",
                step.from
            ));
        }
    }
    Ok(())
}

fn migration(steps: &[MigrationStep]) -> Option<Arc<dyn PayloadMigration>> {
    (!steps.is_empty()).then(|| {
        Arc::new(TemplateMigration {
            steps: steps
                .iter()
                .map(|step| (step.from, step.ops.clone()))
                .collect(),
        }) as Arc<dyn PayloadMigration>
    })
}

impl DeclarativeExtension {
    /// Validates a package's content and prepares its descriptors.
    pub fn new(manifest: PackageManifest, content: PackageContent) -> Result<Self, String> {
        let mut required_capabilities = Vec::new();
        let stages = content
            .stages
            .iter()
            .map(|stage| {
                if let Some(template) = &stage.lowering {
                    check_template(&stage.id, &template.ops, false)?;
                }
                Ok((
                    StageTypeDescriptor {
                        type_id: StageTypeId::new(&stage.id),
                        display_name: stage.name.clone(),
                        role: None,
                        provides: capability_set(&stage.provides),
                    },
                    stage.lowering.clone().map(|template| {
                        Arc::new(TemplateStageLowerer { template }) as Arc<dyn StageLowerer>
                    }),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let modules = content
            .modules
            .iter()
            .map(|module| {
                if module.entry_point.trim().is_empty() {
                    return Err(format!("module '{}' has no entry point", module.id));
                }
                let schema = PropertySchema::new(module.schema_version, module.inputs.clone());
                check_schema(&module.id, &schema, &module.migrations)?;
                let requires = match &module.requires {
                    RequiresDecl::AnyOf(ids) => {
                        required_capabilities.extend(ids.iter().map(CapabilityId::new));
                        CapabilityExpression::AnyOf(capability_set(ids))
                    }
                    RequiresDecl::AllOf(ids) => {
                        required_capabilities.extend(ids.iter().map(CapabilityId::new));
                        CapabilityExpression::AllOf(capability_set(ids))
                    }
                    RequiresDecl::Unconstrained => CapabilityExpression::Unconstrained,
                };
                let metadata = ModuleMetadata::extension(
                    ModuleTypeId::new(&module.id),
                    leak(&module.name),
                    leak(&module.description),
                    leak(if module.category.is_empty() {
                        "Extensions"
                    } else {
                        &module.category
                    }),
                    requires,
                )
                .with_inputs(module.inputs.iter().map(input_metadata).collect())
                .with_schema_version(module.schema_version);
                Ok(PreparedModule {
                    metadata,
                    lowerer: Arc::new(TemplateModuleLowerer {
                        entry_point: module.entry_point.clone(),
                    }),
                    migration: migration(&module.migrations),
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let renderers = content
            .renderers
            .iter()
            .map(|renderer| {
                check_schema(&renderer.id, &renderer.schema, &renderer.migrations)?;
                Ok((
                    RendererDescriptor::extension(
                        RendererTypeId::new(&renderer.id),
                        renderer.name.clone(),
                        renderer.schema.clone(),
                    ),
                    migration(&renderer.migrations),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            capabilities: content.capabilities.iter().map(CapabilityId::new).collect(),
            domains: content
                .domains
                .iter()
                .map(|domain| DomainDescriptor {
                    type_id: DomainTypeId::new(&domain.id),
                    display_name: domain.name.clone(),
                })
                .collect(),
            resources: content
                .resources
                .iter()
                .map(|resource| ResourceTypeDescriptor {
                    type_id: ResourceTypeId::new(&resource.id),
                    display_name: resource.name.clone(),
                    domain: DomainTypeId::new(&resource.domain),
                    lifetime: lifetime(resource.lifetime),
                })
                .collect(),
            stages,
            modules,
            renderers,
            binding_kinds: content
                .binding_kinds
                .iter()
                .map(|kind| BindingKindDescriptor {
                    type_id: BindingKindId::new(&kind.id),
                    display_name: kind.name.clone(),
                    fields: kind
                        .fields
                        .iter()
                        .map(|field| BindingFieldDescriptor {
                            id: BindingFieldId::new(&field.id),
                            display_name: field.name.clone(),
                            value_type: field.value_type,
                        })
                        .collect(),
                })
                .collect(),
            required_capabilities,
            manifest,
        })
    }

    pub fn package_manifest(&self) -> &PackageManifest {
        &self.manifest
    }

    /// Capabilities its modules require from host stages; each must be registered (by core, this
    /// package, or a dependency) for the package to be usable.
    pub fn required_capabilities(&self) -> &[CapabilityId] {
        &self.required_capabilities
    }
}

impl AestraExtension for DeclarativeExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            plugin: self.manifest.id.clone(),
            display_name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
        }
    }

    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict> {
        for capability in &self.capabilities {
            registry.register_capability(capability.clone())?;
        }
        for domain in &self.domains {
            registry.domains.register(domain.clone())?;
        }
        for resource in &self.resources {
            registry.resources.register(resource.clone())?;
        }
        for (stage, lowerer) in &self.stages {
            registry.register_stage(stage.clone())?;
            if let Some(lowerer) = lowerer {
                registry
                    .lowering
                    .register_stage(stage.type_id.clone(), lowerer.clone())?;
            }
        }
        for PreparedModule {
            metadata,
            lowerer,
            migration,
        } in &self.modules
        {
            registry.register_module(metadata.clone())?;
            registry
                .lowering
                .register_module(metadata.type_id.clone(), lowerer.clone())?;
            if let Some(migration) = migration {
                registry
                    .migrations
                    .register_module(metadata.type_id.clone(), migration.clone())?;
            }
        }
        for kind in &self.binding_kinds {
            registry.register_binding_kind(kind.clone())?;
        }
        for (renderer, migration) in &self.renderers {
            registry.register_renderer(renderer.clone())?;
            if let Some(migration) = migration {
                registry
                    .migrations
                    .register_renderer(renderer.type_id.clone(), migration.clone())?;
            }
        }
        Ok(())
    }
}

/// Rejects templates that use `{module}` outside `ForEachModule`, or nest `ForEachModule`.
fn check_template(stage: &str, ops: &[OpTemplate], in_module: bool) -> Result<(), String> {
    for op in ops {
        match op {
            OpTemplate::Compute(compute) => {
                if !in_module
                    && (compute.name.contains("{module}")
                        || compute.entry_point.contains("{module}"))
                {
                    return Err(format!(
                        "stage '{stage}': '{{module}}' is only available inside ForEachModule"
                    ));
                }
            }
            OpTemplate::Repeat { body, .. } => check_template(stage, body, in_module)?,
            OpTemplate::ForEachModule(body) => {
                if in_module {
                    return Err(format!("stage '{stage}': ForEachModule cannot be nested"));
                }
                check_template(stage, body, true)?;
            }
            OpTemplate::Barrier | OpTemplate::Copy { .. } => {}
        }
    }
    Ok(())
}

/// Lowers a packaged module: its resolved payload plus the entry point it declared.
struct TemplateModuleLowerer {
    entry_point: String,
}

impl ModuleLowerer for TemplateModuleLowerer {
    fn lower(
        &self,
        module: &ModuleInstance,
        payload: &PropertyBag,
    ) -> Result<ExtensionModulePlan, String> {
        Ok(ExtensionModulePlan {
            source: module.id,
            module_type: module.module_type.clone(),
            entry_point: self.entry_point.clone(),
            parameters: payload.clone(),
        })
    }
}

/// Lowers a packaged stage by instantiating its Execution IR template.
struct TemplateStageLowerer {
    template: StageTemplate,
}

impl StageLowerer for TemplateStageLowerer {
    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String> {
        Ok(ExecutionBlock {
            resources: self
                .template
                .resources
                .iter()
                .map(|resource| ResourceDescriptor {
                    id: ResourceTypeId::new(&resource.id),
                    bytes: resource.bytes,
                    lifetime: lifetime(resource.lifetime),
                })
                .collect(),
            ops: expand(&self.template.ops, input, None),
        })
    }
}

fn substitute(
    text: &str,
    input: &StageLoweringInput<'_>,
    module: Option<&ExtensionModulePlan>,
) -> String {
    let text = text.replace("{stage}", input.name);
    match module {
        Some(module) => text.replace("{module}", &module.entry_point),
        None => text,
    }
}

fn expand(
    ops: &[OpTemplate],
    input: &StageLoweringInput<'_>,
    module: Option<&ExtensionModulePlan>,
) -> Vec<ExecutionOp> {
    let mut expanded = Vec::new();
    for op in ops {
        match op {
            OpTemplate::Compute(compute) => expanded.push(ExecutionOp::Compute(ComputeOp {
                name: substitute(&compute.name, input, module),
                entry_point: substitute(&compute.entry_point, input, module),
                accesses: compute
                    .accesses
                    .iter()
                    .map(|access| ResourceAccess {
                        resource: ResourceTypeId::new(&access.resource),
                        mode: match access.mode {
                            AccessModeDecl::Read => ResourceAccessMode::Read,
                            AccessModeDecl::Write => ResourceAccessMode::Write,
                            AccessModeDecl::ReadWrite => ResourceAccessMode::ReadWrite,
                        },
                    })
                    .collect(),
                dispatch: match compute.dispatch {
                    DispatchDecl::Fixed(x, y, z) => StagedDispatch { x, y, z },
                    DispatchDecl::Particles(workgroup) => StagedDispatch {
                        x: input.particle_capacity.div_ceil(workgroup.max(1)).max(1),
                        y: 1,
                        z: 1,
                    },
                },
            })),
            OpTemplate::Barrier => expanded.push(ExecutionOp::Barrier),
            OpTemplate::Copy { from, to } => expanded.push(ExecutionOp::Copy(CopyOp {
                from: ResourceTypeId::new(from),
                to: ResourceTypeId::new(to),
            })),
            OpTemplate::Repeat { count, body } => expanded.push(ExecutionOp::Repeat {
                policy: RepeatPolicy::FixedCount(*count),
                body: expand(body, input, module),
            }),
            OpTemplate::ForEachModule(body) => {
                for plan in input.modules {
                    expanded.extend(expand(body, input, Some(plan)));
                }
            }
        }
    }
    expanded
}

/// Applies a package's declared payload migration steps (§35).
struct TemplateMigration {
    steps: BTreeMap<u32, Vec<MigrationOp>>,
}

impl PayloadMigration for TemplateMigration {
    fn migrate(&self, from: u32, payload: &mut BTreeMap<String, Value>) -> Result<(), String> {
        let ops = self
            .steps
            .get(&from)
            .ok_or_else(|| format!("the package declares no migration from schema v{from}"))?;
        for op in ops {
            match op {
                MigrationOp::Rename { from, to } => {
                    if let Some(value) = payload.remove(from) {
                        payload.insert(to.clone(), value);
                    }
                }
                MigrationOp::Remove(name) => {
                    payload.remove(name);
                }
                MigrationOp::Insert { name, value } => {
                    payload.entry(name.clone()).or_insert_with(|| value.clone());
                }
            }
        }
        Ok(())
    }
}
