//! Extension descriptors and registries (extensible plan Â§6â€“Â§16, Â§23): what a module, stage,
//! renderer, domain, resource or capability *is*, and the registries every one of them â€” built-in or
//! from an extension â€” is registered in.

use crate::catalog::builtin_modules;
use crate::*;
use aestra_core::*;
use aestra_runtime::{ParticleAttribute, SimulationClass, TemporalSemantics};
use std::collections::{BTreeMap, BTreeSet};
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

/// The property-schema version built-in modules publish (extensibility redesign M2). Built-ins are v1.
pub const BUILTIN_PROPERTY_SCHEMA_VERSION: u32 = 1;

impl InputControl {
    /// The owned, serializable [`PropertyControl`] equivalent, so built-in controls render and validate
    /// through the same schema-driven path as plugin controls (extensibility redesign M2, §19.1).
    pub fn to_property_control(self) -> PropertyControl {
        match self {
            Self::Toggle => PropertyControl::Toggle,
            Self::Number { step, min, max } => PropertyControl::Number { step, min, max },
            Self::Vector { step, min, max } => PropertyControl::Vector { step, min, max },
            Self::Range { step, min, max } => PropertyControl::Range { step, min, max },
            // Built-in choices draw their options from the host (e.g. the shape/blend enums), so the
            // schema carries no explicit options; a plugin choice would list them.
            Self::Choice => PropertyControl::Choice {
                options: Vec::new(),
            },
            Self::Curve { step, min, max } => PropertyControl::Curve { step, min, max },
            Self::Gradient => PropertyControl::Gradient,
            Self::Reference => PropertyControl::Reference,
        }
    }
}

impl InputMetadata {
    /// Expresses this built-in input as a [`PropertyDescriptor`] — the generalization the schema-driven
    /// inspector and plugin properties share (extensibility redesign M2, §19.1).
    pub fn to_property_descriptor(&self) -> PropertyDescriptor {
        PropertyDescriptor {
            name: self.name.to_string(),
            label: self.display_name.to_string(),
            description: self.description.to_string(),
            value_type: self.value_type,
            default: self.default_value.clone(),
            unit: self.unit.map(str::to_string),
            control: self.control.to_property_control(),
            sources: self.sources.clone(),
        }
    }
}

impl ModuleMetadata {
    /// This module's inputs expressed as a [`PropertySchema`] (extensibility redesign M2, §19.1): the
    /// generalization of `ModuleMetadata.inputs` that built-ins and plugins both render/validate through.
    pub fn property_schema(&self) -> PropertySchema {
        PropertySchema::new(
            self.schema_version,
            self.inputs
                .iter()
                .map(InputMetadata::to_property_descriptor)
                .collect(),
        )
    }
}

/// Whether a module's state at time `t` is a closed-form function of `t`, or depends on the previous
/// tick. Part of the single `ModuleMetadata` simulation-requirement extension (shared-foundation
/// S1-D1); the compiler derives a [`SimulationClass`] from these — the class is never authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum TemporalRequirement {
    /// `state(t) = f(seed, params, t)` — no previous tick required (analytic).
    #[default]
    Direct,
    /// The next state depends on the previous state (stateful).
    PreviousState,
}

/// Whether a module needs ordered or iterative multi-pass execution. Variants are ordered
/// weakest-to-strongest so aggregating takes the maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SynchronizationRequirement {
    #[default]
    None,
    /// One ordered pass with a barrier before dependents run.
    OrderedPass,
    /// Repeated passes (e.g. a solver iteration loop).
    Iterative,
}

/// Whether a module reads neighbouring elements. Variants are ordered weakest-to-strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum NeighborhoodRequirement {
    #[default]
    None,
    /// Reads other particles (e.g. particle-particle collision).
    Particles,
    /// Reads a grid/field domain.
    Grid,
}

/// A module's declared simulation requirements — co-designed with its capabilities / reads / writes
/// as the one requirement extension both architecture tracks consume (shared-foundation S1-D1). The
/// compiler derives an execution class from these; artists never author the class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimulationRequirements {
    pub temporal: TemporalRequirement,
    pub synchronization: SynchronizationRequirement,
    pub neighborhood: NeighborhoodRequirement,
}

impl SimulationRequirements {
    /// The analytic default: direct in time, no synchronization, no neighbourhood. Every current
    /// built-in module uses this, so existing effects stay analytic.
    pub const ANALYTIC: Self = Self {
        temporal: TemporalRequirement::Direct,
        synchronization: SynchronizationRequirement::None,
        neighborhood: NeighborhoodRequirement::None,
    };

    /// Derives the execution class (shared-foundation S1-D2): ordered/iterative synchronization or
    /// any neighbourhood promotes to [`SimulationClass::Staged`]; otherwise a previous-state
    /// dependency promotes to [`SimulationClass::Stateful`]; otherwise [`SimulationClass::Analytic`].
    pub fn derived_class(self) -> SimulationClass {
        if self.synchronization != SynchronizationRequirement::None
            || self.neighborhood != NeighborhoodRequirement::None
        {
            SimulationClass::Staged
        } else if self.temporal == TemporalRequirement::PreviousState {
            SimulationClass::Stateful
        } else {
            SimulationClass::Analytic
        }
    }

    /// The temporal semantics implied by these requirements: analytic is [`TemporalSemantics::Direct`],
    /// stateful and staged are [`TemporalSemantics::HistoryDependent`].
    pub fn temporal_semantics(self) -> TemporalSemantics {
        match self.derived_class() {
            SimulationClass::Analytic => TemporalSemantics::Direct,
            SimulationClass::Stateful | SimulationClass::Staged => {
                TemporalSemantics::HistoryDependent
            }
        }
    }

    /// Combines two requirement sets, taking the stronger of each axis. Used to aggregate a stage's
    /// or island's modules into one class (the aggregate is the max over its parts).
    pub fn max(self, other: Self) -> Self {
        Self {
            temporal: self.temporal.max(other.temporal),
            synchronization: self.synchronization.max(other.synchronization),
            neighborhood: self.neighborhood.max(other.neighborhood),
        }
    }
}

/// How strongly a descriptor supports a backend (extensible plan §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SupportLevel {
    /// The descriptor only runs on this backend.
    Required,
    /// The descriptor runs on this backend.
    #[default]
    Supported,
    /// The descriptor cannot run on this backend.
    Unavailable,
}

/// The backend support a stage / module / renderer descriptor *declares* — the complement of
/// `aestra_runtime::BackendCapabilities`, which is what a backend *offers* (§44.4). Shape agreed in
/// the shared foundation (S1-E); no backend consumes it yet, and the hybrid roadmap's
/// checkpoint/staged-dispatch axes will extend the backend-offers side, not this one.
///
/// Built-ins keep a real CPU reference; a community compute stage that ships no CPU evaluator sets
/// `cpu_reference = Unavailable` (there is no WESL-on-CPU interpreter — extensible plan §13.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendSupport {
    pub cpu_reference: SupportLevel,
    pub gpu_compute: SupportLevel,
}

impl Default for BackendSupport {
    /// Built-in default: fully supported on both the CPU reference and GPU compute backends.
    fn default() -> Self {
        Self {
            cpu_reference: SupportLevel::Supported,
            gpu_compute: SupportLevel::Supported,
        }
    }
}

/// A lifecycle role — the fixed effect/emitter/particle spawn/update slots (extensible-stages M4).
/// Maps 1:1 with the non-`Simulation` [`StageKind`] variants; each carries the capability a stage of
/// that role **provides** and a module of that role **requires**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleRole {
    EffectSpawn,
    EffectUpdate,
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
}

impl LifecycleRole {
    /// The capability a stage of this role provides / a module of this role requires.
    pub fn capability(self) -> CapabilityId {
        CapabilityId::new(match self {
            Self::EffectSpawn => aestra_core::CAPABILITY_HOSTS_EFFECT_SPAWN,
            Self::EffectUpdate => aestra_core::CAPABILITY_HOSTS_EFFECT_UPDATE,
            Self::EmitterSpawn => aestra_core::CAPABILITY_HOSTS_EMITTER_SPAWN,
            Self::EmitterUpdate => aestra_core::CAPABILITY_HOSTS_EMITTER_UPDATE,
            Self::ParticleSpawn => aestra_core::CAPABILITY_HOSTS_PARTICLE_SPAWN,
            Self::ParticleUpdate => aestra_core::CAPABILITY_HOSTS_PARTICLE_UPDATE,
        })
    }

    /// The lifecycle role of a stage, or `None` for a `Simulation(name)` stage (not a fixed role).
    pub fn from_stage(stage: &StageKind) -> Option<Self> {
        match stage {
            StageKind::EffectSpawn => Some(Self::EffectSpawn),
            StageKind::EffectUpdate => Some(Self::EffectUpdate),
            StageKind::EmitterSpawn => Some(Self::EmitterSpawn),
            StageKind::EmitterUpdate => Some(Self::EmitterUpdate),
            StageKind::ParticleSpawn => Some(Self::ParticleSpawn),
            StageKind::ParticleUpdate => Some(Self::ParticleUpdate),
            StageKind::Simulation(_) => None,
        }
    }
}

/// A set of capabilities (extensible-stages M4, §9). A stage provides one; a module requirement is
/// tested against it. Membership decides compatibility, never execution order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    ids: BTreeSet<CapabilityId>,
}

impl CapabilitySet {
    pub fn new(ids: impl IntoIterator<Item = CapabilityId>) -> Self {
        Self {
            ids: ids.into_iter().collect(),
        }
    }

    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.ids.contains(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CapabilityId> {
        self.ids.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// A module's requirement over the capabilities its host stage provides (extensible-stages M4). The
/// compatibility contract is capability satisfaction — never a concrete stage-type check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityExpression {
    /// Satisfied by a stage providing **any** of these — the common "runs in one of these roles".
    AnyOf(CapabilitySet),
    /// Satisfied only by a stage providing **all** of these.
    AllOf(CapabilitySet),
    /// No requirement — compatible with any stage.
    Unconstrained,
}

impl CapabilityExpression {
    /// Whether a stage that provides `provided` satisfies this requirement.
    pub fn is_satisfied_by(&self, provided: &CapabilitySet) -> bool {
        match self {
            Self::AnyOf(set) => set.iter().any(|id| provided.contains(id)),
            Self::AllOf(set) => !set.is_empty() && set.iter().all(|id| provided.contains(id)),
            Self::Unconstrained => true,
        }
    }
}

/// How many instances of a module a single stage may host (extensible-stages M4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModuleMultiplicity {
    /// At most one per stage (e.g. a solver marker); duplicates are a validation error.
    Single,
    /// Any number (the default — repeated same-type modules are allowed, §8.1).
    #[default]
    Multiple,
}

/// Describes a stage type (extensible-stages M4, §6.3): its identity, optional lifecycle role, and the
/// capabilities it **provides** to the modules it hosts. A built-in lifecycle stage provides exactly
/// its role capability; a plugin stage provides whatever capabilities it declares. Module/stage
/// compatibility is [`StageTypeDescriptor::hosts`], which never inspects the stage's concrete type id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageTypeDescriptor {
    pub type_id: StageTypeId,
    pub display_name: String,
    pub role: Option<LifecycleRole>,
    pub provides: CapabilitySet,
}

impl StageTypeDescriptor {
    /// A built-in lifecycle stage descriptor: it provides exactly its role's capability.
    pub fn lifecycle(role: LifecycleRole) -> Self {
        let type_id = StageTypeId::new(match role {
            LifecycleRole::EffectSpawn => aestra_core::AESTRA_STAGE_EFFECT_SPAWN,
            LifecycleRole::EffectUpdate => aestra_core::AESTRA_STAGE_EFFECT_UPDATE,
            LifecycleRole::EmitterSpawn => aestra_core::AESTRA_STAGE_EMITTER_SPAWN,
            LifecycleRole::EmitterUpdate => aestra_core::AESTRA_STAGE_EMITTER_UPDATE,
            LifecycleRole::ParticleSpawn => aestra_core::AESTRA_STAGE_PARTICLE_SPAWN,
            LifecycleRole::ParticleUpdate => aestra_core::AESTRA_STAGE_PARTICLE_UPDATE,
        });
        Self {
            type_id,
            display_name: format!("{role:?}"),
            role: Some(role),
            provides: CapabilitySet::new([role.capability()]),
        }
    }

    /// Whether this stage can host a module with the given capability requirement — the M4
    /// compatibility contract. It inspects only what the stage *provides*, so a third-party stage hosts
    /// standard modules without either side knowing the other's concrete type id.
    pub fn hosts(&self, requires: &CapabilityExpression) -> bool {
        requires.is_satisfied_by(&self.provides)
    }
}

/// Describes a renderer type (extensible-stages M8, §16): its identity, display name, the
/// [`PropertySchema`] its authored payload follows, and whether it is a core built-in or a plugin
/// **extension** renderer. Extension renderers lower generically (payload + type id) so they need no
/// core `RendererPlanKind` variant; `material` and asset references stay structural on the
/// `RendererInstance`, never in the schema, so a missing-plugin renderer still preserves its bindings.
#[derive(Debug, Clone, PartialEq)]
pub struct RendererDescriptor {
    pub type_id: RendererTypeId,
    pub display_name: String,
    pub property_schema: PropertySchema,
    /// True for a community/plugin renderer lowered generically; false for a core built-in.
    pub extension: bool,
}

impl RendererDescriptor {
    /// A core built-in renderer type (lowered by the compiler's typed renderer path).
    pub fn builtin(type_id: &str, display_name: &str) -> Self {
        Self {
            type_id: RendererTypeId::new(type_id),
            display_name: display_name.to_string(),
            property_schema: PropertySchema::new(BUILTIN_PROPERTY_SCHEMA_VERSION, Vec::new()),
            extension: false,
        }
    }

    /// A plugin extension renderer type, lowered generically through its payload.
    pub fn extension(
        type_id: RendererTypeId,
        display_name: impl Into<String>,
        schema: PropertySchema,
    ) -> Self {
        Self {
            type_id,
            display_name: display_name.into(),
            property_schema: schema,
            extension: true,
        }
    }
}

/// The registry of renderer types (extensible-stages M8): built-ins plus registered plugin renderers,
/// so renderer creation, validation, and lowering route through one catalog rather than hardcoded type
/// checks.
#[derive(Debug, Clone, Default)]
pub struct RendererRegistry {
    renderers: BTreeMap<RendererTypeId, RendererDescriptor>,
}

impl RendererRegistry {
    /// The five core renderer types.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for (type_id, name) in [
            (RENDERER_SPRITE, "Sprite"),
            (RENDERER_FLIPBOOK, "Flipbook"),
            (RENDERER_RIBBON, "Ribbon"),
            (RENDERER_TRAIL, "Trail"),
            (RENDERER_MESH, "Mesh"),
        ] {
            let descriptor = RendererDescriptor::builtin(type_id, name);
            registry
                .renderers
                .insert(descriptor.type_id.clone(), descriptor);
        }
        registry
    }

    /// Registers a renderer descriptor; errors if its type id is already registered.
    pub fn register(&mut self, descriptor: RendererDescriptor) -> Result<(), RegistryConflict> {
        let type_id = descriptor.type_id.clone();
        match self.renderers.insert(type_id.clone(), descriptor) {
            None => Ok(()),
            Some(_) => Err(RegistryConflict::DuplicateRenderer(type_id)),
        }
    }

    pub fn get(&self, type_id: &RendererTypeId) -> Option<&RendererDescriptor> {
        self.renderers.get(type_id)
    }

    /// Whether the type id is a registered *extension* (plugin) renderer.
    pub fn is_extension(&self, type_id: &RendererTypeId) -> bool {
        self.get(type_id)
            .is_some_and(|descriptor| descriptor.extension)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RendererDescriptor> {
        self.renderers.values()
    }
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
    pub capabilities: Vec<CapabilityId>,
    /// Declared simulation requirements; the compiler derives an execution class from these
    /// (shared-foundation S1-D1). Defaults to analytic for every current built-in.
    pub simulation: SimulationRequirements,
    /// How many of this module a single stage may host (extensible-stages M4). Defaults to `Multiple`.
    pub multiplicity: ModuleMultiplicity,
    pub approximate_cost: u32,
    /// An explicit host-stage requirement (extensible-stages M10). Plugin modules that run in a plugin
    /// stage declare the capability that stage provides; `None` derives it from `stages`.
    pub requires: Option<CapabilityExpression>,
    /// The version of this module's property schema (extensible-stages M11, §35). Built-ins are
    /// [BUILTIN_PROPERTY_SCHEMA_VERSION]; a plugin bumps it when its payload shape changes and
    /// registers a [PayloadMigration] from the previous version.
    pub schema_version: u32,
}

impl ModuleMetadata {
    /// The capabilities this module requires from its host stage (extensible-stages M4). An explicit
    /// [`requires`](Self::requires) wins; otherwise it is derived from the lifecycle roles of its
    /// declared stages: a module runs in a stage that provides **any** of its roles' capabilities.
    /// This is the capability-based replacement for the old concrete `stages.contains(module.stage)`
    /// check — a stage providing the right capability hosts the module regardless of its concrete
    /// type id. A module declaring only `Simulation` stages and no requirement is `Unconstrained`.
    pub fn required_capabilities(&self) -> CapabilityExpression {
        if let Some(requires) = &self.requires {
            return requires.clone();
        }
        let roles: Vec<CapabilityId> = self
            .stages
            .iter()
            .filter_map(LifecycleRole::from_stage)
            .map(LifecycleRole::capability)
            .collect();
        if roles.is_empty() {
            CapabilityExpression::Unconstrained
        } else {
            CapabilityExpression::AnyOf(CapabilitySet::new(roles))
        }
    }
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
        let metadata = self.get(type_id)?;
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
            MODULE_PERSISTENT => Some(ModuleInstance::persistent()),
            MODULE_COLLISION => Some(ModuleInstance::collision(vec![Collider {
                shape: aestra_core::ColliderShape::Plane {
                    normal: [0.0, 1.0, 0.0],
                    distance: 0.0,
                },
                restitution: 0.5,
                friction: 0.2,
                kill: false,
            }])),
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
            // A plugin module (extensible-stages M10): a generic payload seeded from its schema
            // defaults, placed in the first declared stage (the caller moves it to its host stage).
            _ => Some(ModuleInstance {
                id: aestra_core::ModuleId::new(),
                module_type: type_id.clone(),
                stage: metadata
                    .stages
                    .first()
                    .cloned()
                    .unwrap_or(StageKind::ParticleUpdate),
                enabled: true,
                parameters: ModuleParameters::Custom(
                    metadata
                        .inputs
                        .iter()
                        .map(|input| (input.name.to_string(), input.instantiate_default()))
                        .collect(),
                ),
                property_sources: BTreeMap::new(),
                property_source_values: BTreeMap::new(),
                bindings: BTreeMap::new(),
                host_bindings: BTreeMap::new(),
                label: None,
                // Version 1 is the implicit default and is not written to files.
                schema_version: (metadata.schema_version > 1).then_some(metadata.schema_version),
            }),
        }
    }
}

/// The governed set of registered capability identities (shared-foundation S1-A2/§9.1). Built-in and
/// plugin capabilities share this one namespaced vocabulary.
#[derive(Debug, Clone, Default)]
pub struct CapabilityRegistry {
    capabilities: std::collections::BTreeSet<CapabilityId>,
}

impl CapabilityRegistry {
    /// The core-owned `aestra.*` capabilities every built-in module may declare.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        registry
            .register(CapabilityId::new(CAPABILITY_CPU_REFERENCE))
            .expect("built-in capabilities are unique");
        registry
            .register(CapabilityId::new(CAPABILITY_PARTICLE_SIMULATION))
            .expect("built-in capabilities are unique");
        registry
    }

    /// Registers a capability; errors if the identity was already registered.
    pub fn register(&mut self, id: CapabilityId) -> Result<(), RegistryConflict> {
        if self.capabilities.insert(id.clone()) {
            Ok(())
        } else {
            Err(RegistryConflict::DuplicateCapability(id))
        }
    }

    pub fn contains(&self, id: &CapabilityId) -> bool {
        self.capabilities.contains(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &CapabilityId> {
        self.capabilities.iter()
    }
}

/// A registry integrity problem, surfaced as a distinct diagnostic (shared-foundation S1-B2). None of
/// these can occur for the hand-curated built-ins; they exist for plugin registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryConflict {
    /// Two module descriptors share a type ID.
    DuplicateModule(ModuleTypeId),
    /// Two renderer descriptors share a type ID (extensible-stages M8).
    DuplicateRenderer(RendererTypeId),
    /// Two capability declarations share an ID.
    DuplicateCapability(CapabilityId),
    /// A module descriptor references a capability that is not registered.
    UnknownModuleCapability {
        module: ModuleTypeId,
        capability: CapabilityId,
    },
    /// Two stage descriptors share a type ID (extensible-stages M10).
    DuplicateStage(StageTypeId),
    /// Two domain descriptors share a type ID.
    DuplicateDomain(aestra_core::DomainTypeId),
    /// Two resource descriptors share a type ID.
    DuplicateResource(aestra_core::ResourceTypeId),
    /// Two lowerers were registered for the same stage or module type.
    DuplicateLowerer(String),
    /// A plugin registered an id outside its own `{plugin_id}::` namespace (§5, §9.1).
    OutsideNamespace {
        plugin: aestra_core::ExtensionId,
        id: String,
    },
    /// The same plugin was installed twice.
    DuplicateExtension(aestra_core::ExtensionId),
    /// A plugin's manifest version is not valid semver (extensible-stages M11).
    InvalidVersion {
        plugin: aestra_core::ExtensionId,
        version: String,
    },
    /// Two payload migrations were registered for the same type.
    DuplicateMigration(String),
    /// Two binding kind descriptors share a type id (host bindings HB1).
    DuplicateBindingKind(BindingKindId),
    /// A binding kind lists the same field twice.
    DuplicateBindingField {
        kind: BindingKindId,
        field: BindingFieldId,
    },
    /// A binding kind declares a field whose type cannot be packed into a snapshot (curves, text…),
    /// or more fields than a snapshot can mark (host bindings HB2).
    UnsupportedBindingField {
        kind: BindingKindId,
        message: String,
    },
    /// A binding field id is declared with different value types by different kinds.
    BindingFieldTypeMismatch {
        field: BindingFieldId,
        registered: ValueType,
        declared: ValueType,
    },
}

impl std::fmt::Display for RegistryConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateModule(id) => write!(f, "module type '{}' is registered twice", id.0),
            Self::DuplicateRenderer(id) => {
                write!(f, "renderer type '{}' is registered twice", id.0)
            }
            Self::DuplicateCapability(id) => write!(f, "capability '{}' is registered twice", id.0),
            Self::UnknownModuleCapability { module, capability } => write!(
                f,
                "module '{}' references unregistered capability '{}'",
                module.0, capability.0
            ),
            Self::DuplicateStage(id) => write!(f, "stage type '{}' is registered twice", id.0),
            Self::DuplicateDomain(id) => write!(f, "domain '{}' is registered twice", id.0),
            Self::DuplicateResource(id) => {
                write!(f, "resource type '{}' is registered twice", id.0)
            }
            Self::DuplicateLowerer(id) => write!(f, "'{id}' has two lowerers"),
            Self::OutsideNamespace { plugin, id } => write!(
                f,
                "plugin '{}' registered '{id}' outside its '{}::' namespace",
                plugin.0, plugin.0
            ),
            Self::DuplicateExtension(plugin) => {
                write!(f, "extension '{}' is installed twice", plugin.0)
            }
            Self::InvalidVersion { plugin, version } => write!(
                f,
                "extension '{}' has version '{version}', which is not valid semver",
                plugin.0
            ),
            Self::DuplicateMigration(id) => write!(f, "'{id}' has two payload migrations"),
            Self::DuplicateBindingKind(id) => {
                write!(f, "binding kind '{}' is registered twice", id.0)
            }
            Self::DuplicateBindingField { kind, field } => write!(
                f,
                "binding kind '{}' lists field '{}' twice",
                kind.0, field.0
            ),
            Self::UnsupportedBindingField { kind, message } => {
                write!(f, "binding kind '{}': {message}", kind.0)
            }
            Self::BindingFieldTypeMismatch {
                field,
                registered,
                declared,
            } => write!(
                f,
                "binding field '{}' is {registered:?} elsewhere but declared {declared:?}",
                field.0
            ),
        }
    }
}

impl std::error::Error for RegistryConflict {}

/// The unified extension registry (extensible plan §23): modules, capabilities, renderers, stage
/// types, domains and resource types, plus the lowerers that turn plugin stages and modules into
/// Execution IR. Built-in Aestra functionality registers through this same surface — there is no
/// privileged path — and linked plugins are added with [`ExtensionRegistry::install`].
#[derive(Debug, Clone, Default)]
pub struct ExtensionRegistry {
    pub modules: ModuleRegistry,
    pub capabilities: CapabilityRegistry,
    /// Renderer type catalog (extensible-stages M8): built-ins plus registered plugin renderers.
    pub renderers: RendererRegistry,
    /// Stage type catalog (extensible-stages M10): lifecycle stages, the generic simulation stage,
    /// and plugin stages.
    pub stages: StageRegistry,
    pub domains: DomainRegistry,
    pub resources: ResourceTypeRegistry,
    /// Plugin stage/module lowerers into Execution IR (extensible-stages M10).
    pub lowering: LoweringRegistry,
    /// Host binding kinds and their typed fields (host bindings HB1).
    pub bindings: BindingKindRegistry,
    /// Plugin payload schema migrations (extensible-stages M11, §35).
    pub migrations: MigrationRegistry,
    pub(crate) installed: Vec<ExtensionManifest>,
}

impl ExtensionRegistry {
    /// The built-in registry: every core module, renderer, stage type, domain, resource type, and the
    /// governed capability vocabulary. Never includes linked plugins — see [`Self::linked`].
    pub fn builtin() -> Self {
        Self::from_modules(ModuleRegistry::builtin())
    }

    /// Wraps a module registry with the built-in capability, renderer, stage, domain and resource
    /// vocabulary. Keeps the legacy module-only construction path working while resolution flows
    /// through the unified registry.
    pub fn from_modules(modules: ModuleRegistry) -> Self {
        let mut capabilities = CapabilityRegistry::builtin();
        // Lifecycle-role capabilities are provided by the built-in stages, so they are part of the
        // governed vocabulary a plugin may reference.
        for role in [
            LifecycleRole::EffectSpawn,
            LifecycleRole::EffectUpdate,
            LifecycleRole::EmitterSpawn,
            LifecycleRole::EmitterUpdate,
            LifecycleRole::ParticleSpawn,
            LifecycleRole::ParticleUpdate,
        ] {
            capabilities
                .register(role.capability())
                .expect("built-in capabilities are unique");
        }
        Self {
            modules,
            capabilities,
            renderers: RendererRegistry::builtin(),
            stages: StageRegistry::builtin(),
            domains: DomainRegistry::builtin(),
            resources: ResourceTypeRegistry::builtin(),
            lowering: LoweringRegistry::default(),
            bindings: BindingKindRegistry::builtin(),
            migrations: MigrationRegistry::default(),
            installed: Vec::new(),
        }
    }

    /// Registers a host binding kind (host bindings HB1); errors on a duplicate id or a field whose
    /// value type disagrees with another kind's.
    pub fn register_binding_kind(
        &mut self,
        descriptor: BindingKindDescriptor,
    ) -> Result<(), RegistryConflict> {
        self.bindings.register(descriptor)
    }

    /// Registers a stage type descriptor (extensible-stages M10); errors on a duplicate type id.
    pub fn register_stage(
        &mut self,
        descriptor: StageTypeDescriptor,
    ) -> Result<(), RegistryConflict> {
        self.stages.register(descriptor)
    }

    /// Registers a capability identity; errors if it is already registered.
    pub fn register_capability(&mut self, id: CapabilityId) -> Result<(), RegistryConflict> {
        self.capabilities.register(id)
    }

    /// Registers a module descriptor; errors if its type ID is already registered.
    pub fn register_module(&mut self, metadata: ModuleMetadata) -> Result<(), RegistryConflict> {
        let type_id = metadata.type_id.clone();
        match self.modules.register(metadata) {
            None => Ok(()),
            Some(_) => Err(RegistryConflict::DuplicateModule(type_id)),
        }
    }

    /// Registers a renderer descriptor (extensible-stages M8); errors on a duplicate type id.
    pub fn register_renderer(
        &mut self,
        descriptor: RendererDescriptor,
    ) -> Result<(), RegistryConflict> {
        self.renderers.register(descriptor)
    }

    /// Checks that every module descriptor only references registered capabilities — the
    /// "invalid descriptor" diagnostic. Returns every conflict found (empty when consistent).
    pub fn validate(&self) -> Vec<RegistryConflict> {
        let mut conflicts = Vec::new();
        for metadata in self.modules.iter() {
            for capability in &metadata.capabilities {
                if !self.capabilities.contains(capability) {
                    conflicts.push(RegistryConflict::UnknownModuleCapability {
                        module: metadata.type_id.clone(),
                        capability: capability.clone(),
                    });
                }
            }
        }
        conflicts
    }
}

/// One typed field a binding kind supplies (host bindings HB1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingFieldDescriptor {
    pub id: BindingFieldId,
    pub display_name: String,
    pub value_type: ValueType,
}

impl BindingFieldDescriptor {
    pub fn new(id: &str, display_name: &str, value_type: ValueType) -> Self {
        Self {
            id: BindingFieldId::new(id),
            display_name: display_name.into(),
            value_type,
        }
    }
}

/// A host binding kind: what sort of host object fills a binding slot, and the typed fields it
/// supplies. Drives validation now, and the snapshot layout, GPU record and editor source picker in
/// later milestones (host bindings HB1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingKindDescriptor {
    pub type_id: BindingKindId,
    pub display_name: String,
    pub fields: Vec<BindingFieldDescriptor>,
}

impl BindingKindDescriptor {
    /// The built-in spatial kind: any host object with a world transform.
    pub fn spatial() -> Self {
        Self {
            type_id: BindingKindId::new(AESTRA_BINDING_SPATIAL),
            display_name: "Spatial".into(),
            fields: vec![
                BindingFieldDescriptor::new(AESTRA_FIELD_POSITION, "Position", ValueType::Vec3),
                // A unit quaternion, xyzw; Aestra has no dedicated quaternion value type.
                BindingFieldDescriptor::new(AESTRA_FIELD_ROTATION, "Rotation", ValueType::Vec4),
                BindingFieldDescriptor::new(AESTRA_FIELD_SCALE, "Scale", ValueType::Vec3),
                BindingFieldDescriptor::new(
                    AESTRA_FIELD_LINEAR_VELOCITY,
                    "Linear Velocity",
                    ValueType::Vec3,
                ),
            ],
        }
    }

    pub fn field(&self, id: &BindingFieldId) -> Option<&BindingFieldDescriptor> {
        self.fields.iter().find(|field| &field.id == id)
    }
}

/// The registered host binding kinds (host bindings HB1). A field id means the same thing wherever
/// it appears, so every kind listing it must agree on its value type.
#[derive(Debug, Clone, Default)]
pub struct BindingKindRegistry {
    kinds: BTreeMap<BindingKindId, BindingKindDescriptor>,
}

impl BindingKindRegistry {
    /// The built-in kinds: `aestra.binding.spatial`.
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        registry
            .register(BindingKindDescriptor::spatial())
            .expect("built-in binding kinds are consistent");
        registry
    }

    pub fn register(&mut self, descriptor: BindingKindDescriptor) -> Result<(), RegistryConflict> {
        if self.kinds.contains_key(&descriptor.type_id) {
            return Err(RegistryConflict::DuplicateBindingKind(descriptor.type_id));
        }
        // Every field must pack into a snapshot record (host bindings HB2).
        if let Err(message) = aestra_runtime::BindingLayout::pack(
            descriptor
                .fields
                .iter()
                .map(|field| (field.id.clone(), field.value_type)),
        ) {
            return Err(RegistryConflict::UnsupportedBindingField {
                kind: descriptor.type_id.clone(),
                message,
            });
        }
        for (index, field) in descriptor.fields.iter().enumerate() {
            if descriptor.fields[..index]
                .iter()
                .any(|earlier| earlier.id == field.id)
            {
                return Err(RegistryConflict::DuplicateBindingField {
                    kind: descriptor.type_id.clone(),
                    field: field.id.clone(),
                });
            }
            if let Some(registered) = self.field_type(&field.id)
                && registered != field.value_type
            {
                return Err(RegistryConflict::BindingFieldTypeMismatch {
                    field: field.id.clone(),
                    registered,
                    declared: field.value_type,
                });
            }
        }
        self.kinds.insert(descriptor.type_id.clone(), descriptor);
        Ok(())
    }

    pub fn get(&self, type_id: &BindingKindId) -> Option<&BindingKindDescriptor> {
        self.kinds.get(type_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &BindingKindDescriptor> {
        self.kinds.values()
    }

    /// The value type of a field id, as declared by any registered kind.
    pub fn field_type(&self, id: &BindingFieldId) -> Option<ValueType> {
        self.kinds
            .values()
            .find_map(|kind| kind.field(id))
            .map(|field| field.value_type)
    }
}
