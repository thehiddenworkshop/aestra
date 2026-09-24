//! The linked-extension SDK (extensible-stages M10; plan §23–25, Phase A).
//!
//! A plugin is an ordinary Rust crate linked into an Aestra build. It implements [`AestraExtension`]:
//! a manifest naming its [`PluginId`], and a `register` call that adds its stage types, module types,
//! renderer types, domains, resource types and capabilities to the [`ExtensionRegistry`] — the same
//! surface Aestra's built-ins register through — together with the lowerers that turn its authored
//! stages and modules into portable Execution IR. No built-in stage/module/renderer `match` changes to
//! add one.
//!
//! Identity governance (§5, §9.1): every id a plugin registers must live under its own namespace,
//! `{plugin_id}::…` (e.g. `org.example.aestra::module/vortex`). The `aestra.*` namespace belongs to
//! core. [`ExtensionRegistry::install`] rejects anything else.
//!
//! Linking: an application calls [`link_extension`] once at startup. From then on
//! [`ExtensionRegistry::linked`] — and therefore `EffectCompiler::default()` — includes the plugin, so
//! every compile site in the editor, viewer and game picks it up without threading a registry through
//! each call. [`ExtensionRegistry::builtin`] stays plugin-free. Dynamic loading (Phase B) is a separate
//! problem and deliberately not designed here.

use crate::{
    CapabilitySet, ExtensionRegistry, LifecycleRole, RegistryConflict, StageTypeDescriptor,
};
use aestra_core::{
    DomainTypeId, ModuleInstance, ModuleTypeId, PluginId, PropertyBag, ResourceTypeId, StageId,
    StageTypeId,
};
use aestra_runtime::{ExecutionBlock, ExtensionModulePlan, ResourceLifetime};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

/// Who a linked extension is (§23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionManifest {
    pub plugin: PluginId,
    pub display_name: String,
    pub version: String,
}

/// A linked Aestra extension (§23). `register` adds the plugin's descriptors and lowerers to the
/// registry; it must only register ids under the manifest's plugin namespace.
pub trait AestraExtension: Send + Sync {
    fn manifest(&self) -> ExtensionManifest;
    fn register(&self, registry: &mut ExtensionRegistry) -> Result<(), RegistryConflict>;
}

/// The registered stage types (§6.3): the built-in lifecycle stages, the generic simulation stage, and
/// plugin stages.
#[derive(Debug, Clone, Default)]
pub struct StageRegistry {
    stages: BTreeMap<StageTypeId, StageTypeDescriptor>,
}

impl StageRegistry {
    /// The six lifecycle stages plus the generic simulation stage (which provides no capabilities).
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for role in [
            LifecycleRole::EffectSpawn,
            LifecycleRole::EffectUpdate,
            LifecycleRole::EmitterSpawn,
            LifecycleRole::EmitterUpdate,
            LifecycleRole::ParticleSpawn,
            LifecycleRole::ParticleUpdate,
        ] {
            let descriptor = StageTypeDescriptor::lifecycle(role);
            registry
                .stages
                .insert(descriptor.type_id.clone(), descriptor);
        }
        let generic = StageTypeDescriptor {
            type_id: StageTypeId::new(aestra_core::AESTRA_STAGE_SIMULATION),
            display_name: "Simulation".into(),
            role: None,
            provides: CapabilitySet::default(),
        };
        registry.stages.insert(generic.type_id.clone(), generic);
        registry
    }

    pub fn register(&mut self, descriptor: StageTypeDescriptor) -> Result<(), RegistryConflict> {
        if self.stages.contains_key(&descriptor.type_id) {
            return Err(RegistryConflict::DuplicateStage(descriptor.type_id));
        }
        self.stages.insert(descriptor.type_id.clone(), descriptor);
        Ok(())
    }

    pub fn get(&self, type_id: &StageTypeId) -> Option<&StageTypeDescriptor> {
        self.stages.get(type_id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &StageTypeDescriptor> {
        self.stages.values()
    }
}

/// An execution/data domain (§10) — a registered topology such as particles or a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainDescriptor {
    pub type_id: DomainTypeId,
    pub display_name: String,
}

/// The registered domains (§10): built-in particles and strips, plus plugin domains.
#[derive(Debug, Clone, Default)]
pub struct DomainRegistry {
    domains: BTreeMap<DomainTypeId, DomainDescriptor>,
}

impl DomainRegistry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        for (type_id, name) in [
            (aestra_core::AESTRA_DOMAIN_PARTICLES, "Particles"),
            (aestra_core::AESTRA_DOMAIN_STRIP, "Strip"),
        ] {
            let descriptor = DomainDescriptor {
                type_id: DomainTypeId::new(type_id),
                display_name: name.into(),
            };
            registry
                .domains
                .insert(descriptor.type_id.clone(), descriptor);
        }
        registry
    }

    pub fn register(&mut self, descriptor: DomainDescriptor) -> Result<(), RegistryConflict> {
        if self.domains.contains_key(&descriptor.type_id) {
            return Err(RegistryConflict::DuplicateDomain(descriptor.type_id));
        }
        self.domains.insert(descriptor.type_id.clone(), descriptor);
        Ok(())
    }

    pub fn get(&self, type_id: &DomainTypeId) -> Option<&DomainDescriptor> {
        self.domains.get(type_id)
    }
}

/// A resource type (§11): a typed buffer in some domain, persistent across ticks or transient scratch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceTypeDescriptor {
    pub type_id: ResourceTypeId,
    pub display_name: String,
    pub domain: DomainTypeId,
    pub lifetime: ResourceLifetime,
}

/// The registered resource types (§11): the built-in particle buffer, plus plugin resources.
#[derive(Debug, Clone, Default)]
pub struct ResourceTypeRegistry {
    resources: BTreeMap<ResourceTypeId, ResourceTypeDescriptor>,
}

impl ResourceTypeRegistry {
    pub fn builtin() -> Self {
        let mut registry = Self::default();
        let particles = ResourceTypeDescriptor {
            type_id: ResourceTypeId::new(aestra_runtime::AESTRA_RESOURCE_PARTICLES),
            display_name: "Particles".into(),
            domain: DomainTypeId::new(aestra_core::AESTRA_DOMAIN_PARTICLES),
            lifetime: ResourceLifetime::Persistent,
        };
        registry
            .resources
            .insert(particles.type_id.clone(), particles);
        registry
    }

    pub fn register(&mut self, descriptor: ResourceTypeDescriptor) -> Result<(), RegistryConflict> {
        if self.resources.contains_key(&descriptor.type_id) {
            return Err(RegistryConflict::DuplicateResource(descriptor.type_id));
        }
        self.resources
            .insert(descriptor.type_id.clone(), descriptor);
        Ok(())
    }

    pub fn get(&self, type_id: &ResourceTypeId) -> Option<&ResourceTypeDescriptor> {
        self.resources.get(type_id)
    }
}

/// Lowers one authored plugin module (§8.3 `ModuleLowerer`). `payload` is the module's authored
/// property bag with the module's schema defaults already filled in.
pub trait ModuleLowerer: Send + Sync {
    fn lower(
        &self,
        module: &ModuleInstance,
        payload: &PropertyBag,
    ) -> Result<ExtensionModulePlan, String>;
}

/// What a stage lowerer receives: the authored stage and its already-lowered modules, in order.
pub struct StageLoweringInput<'a> {
    pub stage: StageId,
    pub stage_type: &'a StageTypeId,
    pub name: &'a str,
    pub particle_capacity: u32,
    pub modules: &'a [ExtensionModulePlan],
}

/// Lowers an authored plugin stage into a portable [`ExecutionBlock`] (§6.3 `StageLowerer`) — one or
/// many passes, barriers and repeats over declared resources.
pub trait StageLowerer: Send + Sync {
    fn lower(&self, input: &StageLoweringInput<'_>) -> Result<ExecutionBlock, String>;
}

/// The registered lowerers, keyed by the type they lower.
#[derive(Clone, Default)]
pub struct LoweringRegistry {
    modules: BTreeMap<ModuleTypeId, Arc<dyn ModuleLowerer>>,
    stages: BTreeMap<StageTypeId, Arc<dyn StageLowerer>>,
}

impl std::fmt::Debug for LoweringRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoweringRegistry")
            .field("modules", &self.modules.keys().collect::<Vec<_>>())
            .field("stages", &self.stages.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl LoweringRegistry {
    pub fn register_module(
        &mut self,
        module_type: ModuleTypeId,
        lowerer: Arc<dyn ModuleLowerer>,
    ) -> Result<(), RegistryConflict> {
        if self.modules.contains_key(&module_type) {
            return Err(RegistryConflict::DuplicateLowerer(module_type.0));
        }
        self.modules.insert(module_type, lowerer);
        Ok(())
    }

    pub fn register_stage(
        &mut self,
        stage_type: StageTypeId,
        lowerer: Arc<dyn StageLowerer>,
    ) -> Result<(), RegistryConflict> {
        if self.stages.contains_key(&stage_type) {
            return Err(RegistryConflict::DuplicateLowerer(stage_type.0));
        }
        self.stages.insert(stage_type, lowerer);
        Ok(())
    }

    pub fn module(&self, module_type: &ModuleTypeId) -> Option<&Arc<dyn ModuleLowerer>> {
        self.modules.get(module_type)
    }

    pub fn stage(&self, stage_type: &StageTypeId) -> Option<&Arc<dyn StageLowerer>> {
        self.stages.get(stage_type)
    }
}

/// Every id the registry currently holds, for the namespace check in [`ExtensionRegistry::install`].
fn registered_ids(registry: &ExtensionRegistry) -> std::collections::BTreeSet<String> {
    let mut ids = std::collections::BTreeSet::new();
    ids.extend(registry.modules.iter().map(|m| m.type_id.0.clone()));
    ids.extend(registry.renderers.iter().map(|r| r.type_id.0.clone()));
    ids.extend(registry.stages.iter().map(|s| s.type_id.0.clone()));
    ids.extend(registry.domains.domains.keys().map(|d| d.0.clone()));
    ids.extend(registry.resources.resources.keys().map(|r| r.0.clone()));
    ids.extend(registry.capabilities.iter().map(|c| c.0.clone()));
    ids.extend(
        registry
            .lowering
            .modules
            .keys()
            .map(|m| format!("lowering:{}", m.0)),
    );
    ids.extend(
        registry
            .lowering
            .stages
            .keys()
            .map(|s| format!("lowering:{}", s.0)),
    );
    ids
}

impl ExtensionRegistry {
    /// Installs a linked extension (§23): runs its `register`, then checks every id it added lives
    /// under its plugin namespace. On any conflict the registry is left unchanged.
    pub fn install(&mut self, extension: &dyn AestraExtension) -> Result<(), RegistryConflict> {
        let manifest = extension.manifest();
        if self
            .installed
            .iter()
            .any(|installed| installed.plugin == manifest.plugin)
        {
            return Err(RegistryConflict::DuplicateExtension(manifest.plugin));
        }
        let mut candidate = self.clone();
        let before = registered_ids(&candidate);
        extension.register(&mut candidate)?;
        let namespace = format!("{}::", manifest.plugin.as_str());
        for id in registered_ids(&candidate).difference(&before) {
            let id = id.strip_prefix("lowering:").unwrap_or(id);
            if !id.starts_with(&namespace) {
                return Err(RegistryConflict::OutsideNamespace {
                    plugin: manifest.plugin.clone(),
                    id: id.to_string(),
                });
            }
        }
        candidate.installed.push(manifest);
        *self = candidate;
        Ok(())
    }

    /// The manifests of the extensions installed in this registry, in install order.
    pub fn installed(&self) -> &[ExtensionManifest] {
        &self.installed
    }

    /// The built-in registry plus every extension linked into this process with [`link_extension`].
    /// This is what `EffectCompiler::default()` uses.
    pub fn linked() -> Self {
        let mut registry = Self::builtin();
        for extension in linked_list().read().expect("linked extensions lock").iter() {
            // Each was installed successfully against the built-ins when it was linked.
            registry
                .install(extension.as_ref())
                .expect("a linked extension installs cleanly");
        }
        registry
    }
}

fn linked_list() -> &'static RwLock<Vec<Arc<dyn AestraExtension>>> {
    static LINKED: OnceLock<RwLock<Vec<Arc<dyn AestraExtension>>>> = OnceLock::new();
    LINKED.get_or_init(|| RwLock::new(Vec::new()))
}

/// Links an extension into this process (Phase A linked extensions): from now on
/// [`ExtensionRegistry::linked`] and `EffectCompiler::default()` include it. Validated immediately
/// against the built-ins and every previously linked extension; linking the same plugin twice is a
/// no-op, so startup code can call this idempotently.
pub fn link_extension(extension: Arc<dyn AestraExtension>) -> Result<(), RegistryConflict> {
    let mut linked = linked_list().write().expect("linked extensions lock");
    let plugin = extension.manifest().plugin;
    if linked
        .iter()
        .any(|existing| existing.manifest().plugin == plugin)
    {
        return Ok(());
    }
    let mut trial = ExtensionRegistry::builtin();
    for existing in linked.iter() {
        trial.install(existing.as_ref())?;
    }
    trial.install(extension.as_ref())?;
    linked.push(extension);
    Ok(())
}

/// The manifests of the extensions linked into this process.
pub fn linked_extensions() -> Vec<ExtensionManifest> {
    linked_list()
        .read()
        .expect("linked extensions lock")
        .iter()
        .map(|extension| extension.manifest())
        .collect()
}
