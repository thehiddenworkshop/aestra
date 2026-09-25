//! The linked-extension SDK (extensible-stages M10; plan §23–25, Phase A).
//!
//! A plugin is an ordinary Rust crate linked into an Aestra build. It implements [`AestraExtension`]:
//! a manifest naming its [`ExtensionId`], and a `register` call that adds its stage types, module types,
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
    DomainTypeId, EffectAsset, ExtensionId, ExtensionRequirement, ModuleInstance, ModuleParameters,
    ModuleTypeId, PropertyBag, RendererProperties, RendererTypeId, ResourceTypeId, StageId,
    StageTypeId, Value, plugin_of,
};
use aestra_runtime::{ExecutionBlock, ExtensionModulePlan, ResourceLifetime};
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

/// The version of the public extension contract this build implements (extensible-stages M12). A
/// packaged extension declares the range it targets (`aestra_api: "^0.1"`) and is only registered when
/// this version satisfies it. Bump the minor version for additive contract changes and the major (or,
/// before 1.0, the minor) version for breaking ones.
pub const EXTENSION_API_VERSION: &str = "0.1.0";

/// Who a linked extension is (§23).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionManifest {
    pub plugin: ExtensionId,
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
    ids.extend(
        registry
            .migrations
            .keys()
            .map(|id| format!("migration:{id}")),
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
        if semver::Version::parse(&manifest.version).is_err() {
            return Err(RegistryConflict::InvalidVersion {
                plugin: manifest.plugin,
                version: manifest.version,
            });
        }
        let mut candidate = self.clone();
        let before = registered_ids(&candidate);
        extension.register(&mut candidate)?;
        let namespace = format!("{}::", manifest.plugin.as_str());
        for id in registered_ids(&candidate).difference(&before) {
            let id = id
                .strip_prefix("lowering:")
                .or_else(|| id.strip_prefix("migration:"))
                .unwrap_or(id);
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

/// Upgrades one plugin type's stored payload by a single schema version (extensible-stages M11, §35).
/// A plugin registers one per type whose schema it has bumped; Aestra chains the steps
/// (`v1 → v2 → v3`) until the payload reaches the installed schema. Migrations are per type, never
/// per plugin release, so one release can migrate one of several types.
pub trait PayloadMigration: Send + Sync {
    /// Rewrites `payload` from schema version `from` to `from + 1`.
    fn migrate(&self, from: u32, payload: &mut BTreeMap<String, Value>) -> Result<(), String>;
}

/// The registered payload migrations, keyed by the module or renderer type they upgrade.
#[derive(Clone, Default)]
pub struct MigrationRegistry {
    migrations: BTreeMap<String, Arc<dyn PayloadMigration>>,
}

impl std::fmt::Debug for MigrationRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.migrations.keys()).finish()
    }
}

impl MigrationRegistry {
    pub fn register_module(
        &mut self,
        module_type: ModuleTypeId,
        migration: Arc<dyn PayloadMigration>,
    ) -> Result<(), RegistryConflict> {
        self.register(module_type.0, migration)
    }

    pub fn register_renderer(
        &mut self,
        renderer_type: RendererTypeId,
        migration: Arc<dyn PayloadMigration>,
    ) -> Result<(), RegistryConflict> {
        self.register(renderer_type.0, migration)
    }

    fn register(
        &mut self,
        type_id: String,
        migration: Arc<dyn PayloadMigration>,
    ) -> Result<(), RegistryConflict> {
        if self.migrations.contains_key(&type_id) {
            return Err(RegistryConflict::DuplicateMigration(type_id));
        }
        self.migrations.insert(type_id, migration);
        Ok(())
    }

    pub fn get(&self, type_id: &str) -> Option<&Arc<dyn PayloadMigration>> {
        self.migrations.get(type_id)
    }

    fn keys(&self) -> impl Iterator<Item = &String> {
        self.migrations.keys()
    }
}

/// One payload upgraded to the installed schema by [`ExtensionRegistry::migrate_effect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratedPayload {
    pub path: String,
    pub type_id: String,
    pub from: u32,
    pub to: u32,
}

/// A payload that could not be upgraded; it is left exactly as authored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFailure {
    pub path: String,
    pub type_id: String,
    pub from: u32,
    pub to: u32,
    pub message: String,
}

/// What [`ExtensionRegistry::migrate_effect`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PayloadMigrationReport {
    pub upgraded: Vec<MigratedPayload>,
    pub failed: Vec<MigrationFailure>,
}

/// How a stored plugin payload's schema version relates to the installed descriptor's (§35).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchemaStatus {
    Current,
    /// Authored against an older schema: migratable when the plugin registers a migration.
    Older {
        stored: u32,
        current: u32,
    },
    /// Authored by a newer plugin: preserved untouched, read-only and not compilable (§35).
    Newer {
        stored: u32,
        current: u32,
    },
}

impl SchemaStatus {
    pub fn of(stored: Option<u32>, current: u32) -> Self {
        let stored = stored.unwrap_or(1);
        match stored.cmp(&current) {
            std::cmp::Ordering::Equal => Self::Current,
            std::cmp::Ordering::Less => Self::Older { stored, current },
            std::cmp::Ordering::Greater => Self::Newer { stored, current },
        }
    }
}

/// Whether an installed extension satisfies an effect's [`ExtensionRequirement`] (§21).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementStatus<'a> {
    Satisfied(&'a ExtensionManifest),
    /// The plugin is not installed in this build.
    Missing,
    /// The plugin is installed, but its version does not match the requirement.
    Incompatible(&'a ExtensionManifest),
    /// The requirement is not a valid semver requirement.
    Invalid(String),
}

/// Chains a type's single-step migrations from `stored` up to `current` on a copy, so a failure part
/// way leaves the authored payload untouched.
fn run_migration(
    migration: Option<&Arc<dyn PayloadMigration>>,
    payload: &BTreeMap<String, Value>,
    stored: u32,
    current: u32,
) -> Result<BTreeMap<String, Value>, String> {
    let migration = migration
        .ok_or_else(|| format!("the plugin registers no migration from schema v{stored}"))?;
    let mut working = payload.clone();
    for from in stored..current {
        migration
            .migrate(from, &mut working)
            .map_err(|error| format!("migrating schema v{from} → v{}: {error}", from + 1))?;
    }
    Ok(working)
}

impl ExtensionRegistry {
    /// The installed extension that owns a namespaced type id, if any.
    pub fn provider_of(&self, type_id: &str) -> Option<&ExtensionManifest> {
        let plugin = plugin_of(type_id)?;
        self.installed_manifest(&plugin)
    }

    /// The manifest of an installed plugin.
    pub fn installed_manifest(&self, plugin: &ExtensionId) -> Option<&ExtensionManifest> {
        self.installed
            .iter()
            .find(|manifest| &manifest.plugin == plugin)
    }

    /// Matches a recorded requirement against the installed extensions.
    pub fn requirement_status(&self, requirement: &ExtensionRequirement) -> RequirementStatus<'_> {
        let version_req = match semver::VersionReq::parse(&requirement.version) {
            Ok(version_req) => version_req,
            Err(error) => return RequirementStatus::Invalid(error.to_string()),
        };
        let Some(manifest) = self.installed_manifest(&requirement.plugin) else {
            return RequirementStatus::Missing;
        };
        // Installed manifests are validated as semver on install.
        match semver::Version::parse(&manifest.version) {
            Ok(version) if version_req.matches(&version) => RequirementStatus::Satisfied(manifest),
            _ => RequirementStatus::Incompatible(manifest),
        }
    }

    /// The requirements an effect should record on save (§21): one per referenced plugin — `^version`
    /// of the installed plugin, or the effect's existing entry (else `*`) for a plugin that is missing.
    /// An existing requirement the installed plugin does not satisfy is kept, so saving never hides
    /// an incompatibility.
    pub fn derive_requirements(&self, asset: &EffectAsset) -> Vec<ExtensionRequirement> {
        asset
            .referenced_plugins()
            .into_iter()
            .map(|plugin| {
                let existing = asset.extension_requirement(&plugin);
                match self.installed_manifest(&plugin) {
                    Some(manifest)
                        if existing.is_none_or(|existing| {
                            matches!(
                                self.requirement_status(existing),
                                RequirementStatus::Satisfied(_)
                            )
                        }) =>
                    {
                        ExtensionRequirement {
                            plugin,
                            version: format!("^{}", manifest.version),
                        }
                    }
                    _ => existing.cloned().unwrap_or(ExtensionRequirement {
                        plugin,
                        version: "*".into(),
                    }),
                }
            })
            .collect()
    }

    /// The schema status of a plugin module's stored payload, or `None` for a built-in or unregistered
    /// module (nothing to compare against).
    pub fn module_schema_status(&self, module: &ModuleInstance) -> Option<SchemaStatus> {
        if !matches!(module.parameters, ModuleParameters::Custom(_)) {
            return None;
        }
        let metadata = self.modules.get(&module.module_type)?;
        Some(SchemaStatus::of(
            module.schema_version,
            metadata.schema_version,
        ))
    }

    /// The schema status of an extension renderer's stored payload.
    pub fn renderer_schema_status(
        &self,
        renderer: &aestra_core::RendererInstance,
    ) -> Option<SchemaStatus> {
        if !matches!(renderer.properties, RendererProperties::Custom(_)) {
            return None;
        }
        let descriptor = self.renderers.get(&renderer.renderer_type)?;
        Some(SchemaStatus::of(
            renderer.schema_version,
            descriptor.property_schema.schema_version,
        ))
    }

    /// Whether any plugin payload in the effect is older than its installed schema.
    pub fn needs_migration(&self, asset: &EffectAsset) -> bool {
        asset.emitters.iter().any(|emitter| {
            emitter.modules.iter().any(|module| {
                matches!(
                    self.module_schema_status(module),
                    Some(SchemaStatus::Older { .. })
                )
            }) || emitter.renderers.iter().any(|renderer| {
                matches!(
                    self.renderer_schema_status(renderer),
                    Some(SchemaStatus::Older { .. })
                )
            })
        })
    }

    /// Upgrades every older plugin payload in the effect to the installed schema through the plugins'
    /// registered migrations (§35). A payload with no migration path, or whose migration fails, is left
    /// exactly as authored and reported; newer payloads are never touched.
    pub fn migrate_effect(&self, asset: &mut EffectAsset) -> PayloadMigrationReport {
        let mut report = PayloadMigrationReport::default();
        for (emitter_index, emitter) in asset.emitters.iter_mut().enumerate() {
            for (module_index, module) in emitter.modules.iter_mut().enumerate() {
                let Some(SchemaStatus::Older { stored, current }) =
                    self.module_schema_status(module)
                else {
                    continue;
                };
                let path = format!("effect.emitters[{emitter_index}].modules[{module_index}]");
                let type_id = module.module_type.0.clone();
                let ModuleParameters::Custom(values) = &mut module.parameters else {
                    continue;
                };
                match run_migration(self.migrations.get(&type_id), values, stored, current) {
                    Ok(migrated) => {
                        *values = migrated;
                        module.schema_version = Some(current);
                        report.upgraded.push(MigratedPayload {
                            path,
                            type_id,
                            from: stored,
                            to: current,
                        });
                    }
                    Err(message) => report.failed.push(MigrationFailure {
                        path,
                        type_id,
                        from: stored,
                        to: current,
                        message,
                    }),
                }
            }
            for (renderer_index, renderer) in emitter.renderers.iter_mut().enumerate() {
                let Some(SchemaStatus::Older { stored, current }) =
                    self.renderer_schema_status(renderer)
                else {
                    continue;
                };
                let path = format!("effect.emitters[{emitter_index}].renderers[{renderer_index}]");
                let type_id = renderer.renderer_type.0.clone();
                let RendererProperties::Custom(values) = &mut renderer.properties else {
                    continue;
                };
                match run_migration(self.migrations.get(&type_id), values, stored, current) {
                    Ok(migrated) => {
                        *values = migrated;
                        renderer.schema_version = Some(current);
                        report.upgraded.push(MigratedPayload {
                            path,
                            type_id,
                            from: stored,
                            to: current,
                        });
                    }
                    Err(message) => report.failed.push(MigrationFailure {
                        path,
                        type_id,
                        from: stored,
                        to: current,
                        message,
                    }),
                }
            }
        }
        report
    }
}
