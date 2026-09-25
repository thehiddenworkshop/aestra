//! The on-disk format of a packaged extension (extensible-stages M12, plan §22).
//!
//! A package is a directory:
//!
//! ```text
//! org.example.aestra-wind/
//!   extension.ron     manifest: identity, version, API range, dependencies, permissions
//!   content.ron       declarations: capabilities, domains, resources, stages, modules, renderers,
//!                     binding kinds
//! ```
//!
//! Packages are **declarative**: they carry descriptors, property schemas, Execution IR templates and
//! payload migrations as data, and no code runs on the host. Both files are RON, like every other
//! Aestra asset.

use aestra_core::{ExtensionId, PropertyDescriptor, PropertySchema, Value, ValueType};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The manifest file every package root contains.
pub const MANIFEST_FILE: &str = "extension.ron";
/// The content file used when the manifest does not name one.
pub const DEFAULT_CONTENT_FILE: &str = "content.ron";

fn default_content_file() -> String {
    DEFAULT_CONTENT_FILE.into()
}

fn one() -> u32 {
    1
}

/// `extension.ron`: who the package is and what it needs (§22).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageManifest {
    /// The extension id; also the namespace of every id it declares (`{id}::…`).
    pub id: ExtensionId,
    pub name: String,
    /// The package's own version (semver).
    pub version: String,
    /// The range of the Aestra extension API it targets, e.g. `^0.1`.
    pub aestra_api: String,
    #[serde(default)]
    pub description: String,
    /// Other extensions (linked or packaged) this one builds on: id → version requirement.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
    #[serde(default)]
    pub permissions: PackagePermissions,
    /// The content file, relative to the package root.
    #[serde(default = "default_content_file")]
    pub content: String,
}

/// Capabilities a package asks the host for (§22). Declarative packages run no code, so nothing can
/// exercise these yet; they are recorded and reported so the contract exists before code hosting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackagePermissions {
    pub filesystem_project: bool,
    pub network: bool,
}

impl PackagePermissions {
    /// The names of the permissions requested.
    pub fn requested(&self) -> Vec<&'static str> {
        let mut requested = Vec::new();
        if self.filesystem_project {
            requested.push("filesystem_project");
        }
        if self.network {
            requested.push("network");
        }
        requested
    }
}

/// `content.ron`: everything the package registers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PackageContent {
    pub capabilities: Vec<String>,
    pub domains: Vec<DomainDecl>,
    pub resources: Vec<ResourceDecl>,
    pub stages: Vec<StageDecl>,
    pub modules: Vec<ModuleDecl>,
    pub renderers: Vec<RendererDecl>,
    /// Host binding kinds (host bindings HB1).
    pub binding_kinds: Vec<BindingKindDecl>,
}

/// A host binding kind and its typed fields. Fields may reuse already-registered ids (e.g.
/// `aestra.field.position`, with the same value type) or declare new ones under the package namespace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingKindDecl {
    pub id: String,
    pub name: String,
    pub fields: Vec<BindingFieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingFieldDecl {
    pub id: String,
    pub name: String,
    pub value_type: ValueType,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DomainDecl {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifetimeDecl {
    Persistent,
    Transient,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceDecl {
    pub id: String,
    pub name: String,
    pub domain: String,
    pub lifetime: LifetimeDecl,
}

/// A stage type. With `lowering`, authored stages of this type lower through the template; without,
/// the stage only hosts modules (like the generic simulation stage).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageDecl {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub lowering: Option<StageTemplate>,
}

/// Which host stages a module may run in, as a capability expression (§9).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequiresDecl {
    AnyOf(Vec<String>),
    AllOf(Vec<String>),
    Unconstrained,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleDecl {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    pub requires: RequiresDecl,
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default)]
    pub inputs: Vec<PropertyDescriptor>,
    /// The kernel entry point this module contributes to its host stage.
    pub entry_point: String,
    #[serde(default)]
    pub migrations: Vec<MigrationStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RendererDecl {
    pub id: String,
    pub name: String,
    pub schema: PropertySchema,
    #[serde(default)]
    pub migrations: Vec<MigrationStep>,
}

/// A stage's Execution IR as data. Strings may use `{stage}` (the authored stage name) and, inside
/// `ForEachModule`, `{module}` (each module's entry point).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageTemplate {
    #[serde(default)]
    pub resources: Vec<ResourceUse>,
    pub ops: Vec<OpTemplate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceUse {
    pub id: String,
    #[serde(default)]
    pub bytes: u64,
    pub lifetime: LifetimeDecl,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpTemplate {
    Compute(ComputeTemplate),
    Barrier,
    Copy {
        from: String,
        to: String,
    },
    Repeat {
        count: u32,
        body: Vec<OpTemplate>,
    },
    /// The body once per enabled module of the stage, in authored order.
    ForEachModule(Vec<OpTemplate>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeTemplate {
    pub name: String,
    pub entry_point: String,
    pub accesses: Vec<AccessDecl>,
    pub dispatch: DispatchDecl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccessModeDecl {
    Read,
    Write,
    ReadWrite,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessDecl {
    pub resource: String,
    pub mode: AccessModeDecl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DispatchDecl {
    /// A fixed workgroup grid.
    Fixed(u32, u32, u32),
    /// One thread per particle of the emitter's capacity, in workgroups of this size.
    Particles(u32),
}

/// Upgrades a payload from schema `from` to `from + 1` (§35).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationStep {
    pub from: u32,
    pub ops: Vec<MigrationOp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationOp {
    Rename {
        from: String,
        to: String,
    },
    Remove(String),
    /// Adds a property when the payload lacks it.
    Insert {
        name: String,
        value: Value,
    },
}
