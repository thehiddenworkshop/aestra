//! The packaged-extension host (extensible-stages M12), `aestra_extension::host`.
//!
//! Installs extensions without rebuilding Aestra: a package directory dropped into a search directory
//! is **discovered**, its manifest **version-checked** against the extension API and its dependencies,
//! **registered** through the same [`AestraExtension`] contract linked extensions use, and can be
//! **disabled** by id. Every outcome is reported per package so a host can **diagnose** why an
//! extension is not active.
//!
//! Packages are declarative ([`package`]): descriptors, schemas, Execution IR templates and payload
//! migrations as data. No package code runs on the host; choosing a code-execution boundary (WASM
//! component, process/IPC or stable FFI) is deliberately deferred.

pub mod declarative;
pub mod package;

pub use declarative::DeclarativeExtension;
pub use package::*;

use crate::{AestraExtension, EXTENSION_API_VERSION, ExtensionRegistry};
use aestra_core::ExtensionId;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Why a discovered package is, or is not, active.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageStatus {
    Registered,
    /// Disabled by the user.
    Disabled,
    InvalidManifest(String),
    InvalidContent(String),
    /// The package targets an extension API range this build does not implement.
    IncompatibleApi {
        required: String,
        host: String,
    },
    /// Another package earlier in the search order has the same id.
    DuplicateId {
        first: PathBuf,
    },
    /// An extension with this id is linked into the build; the linked one wins.
    AlreadyLinked,
    MissingDependency {
        id: String,
        requirement: String,
    },
    IncompatibleDependency {
        id: String,
        requirement: String,
        found: String,
    },
    /// The dependency exists but is disabled or failed itself.
    DependencyUnavailable {
        id: String,
    },
    DependencyCycle,
    /// Registration was rejected (namespace, duplicate id, unregistered capability, …).
    RegistryConflict(String),
}

impl PackageStatus {
    pub fn is_registered(&self) -> bool {
        matches!(self, Self::Registered)
    }

    /// A one-line explanation for logs and UI.
    pub fn describe(&self) -> String {
        match self {
            Self::Registered => "registered".into(),
            Self::Disabled => "disabled".into(),
            Self::InvalidManifest(error) => format!("invalid manifest: {error}"),
            Self::InvalidContent(error) => format!("invalid content: {error}"),
            Self::IncompatibleApi { required, host } => {
                format!("targets extension API {required}, but this build implements {host}")
            }
            Self::DuplicateId { first } => {
                format!("duplicate id; the package at {} is used", first.display())
            }
            Self::AlreadyLinked => {
                "an extension with this id is built into this application".into()
            }
            Self::MissingDependency { id, requirement } => {
                format!("requires '{id}' {requirement}, which is not installed")
            }
            Self::IncompatibleDependency {
                id,
                requirement,
                found,
            } => format!("requires '{id}' {requirement}, but {found} is installed"),
            Self::DependencyUnavailable { id } => {
                format!("requires '{id}', which is disabled or failed to load")
            }
            Self::DependencyCycle => "its dependencies form a cycle".into(),
            Self::RegistryConflict(error) => format!("registration rejected: {error}"),
        }
    }
}

/// The outcome for one discovered package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReport {
    pub root: PathBuf,
    pub id: Option<ExtensionId>,
    pub name: String,
    pub version: String,
    /// Permissions the manifest requests (recorded; declarative packages cannot use them).
    pub permissions: Vec<&'static str>,
    pub status: PackageStatus,
}

/// Every discovered package and what happened to it, in discovery order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostReport {
    pub packages: Vec<PackageReport>,
}

impl HostReport {
    pub fn registered(&self) -> impl Iterator<Item = &PackageReport> {
        self.packages
            .iter()
            .filter(|package| package.status.is_registered())
    }

    /// Packages that are neither registered nor deliberately disabled.
    pub fn problems(&self) -> impl Iterator<Item = &PackageReport> {
        self.packages.iter().filter(|package| {
            !matches!(
                package.status,
                PackageStatus::Registered | PackageStatus::Disabled
            )
        })
    }

    pub fn status_of(&self, id: &ExtensionId) -> Option<&PackageStatus> {
        self.packages
            .iter()
            .find(|package| package.id.as_ref() == Some(id))
            .map(|package| &package.status)
    }
}

/// The registered packages, in dependency order, plus the report.
pub struct LoadedPackages {
    pub extensions: Vec<Arc<DeclarativeExtension>>,
    pub report: HostReport,
}

/// Package roots under the search directories: each immediate subdirectory containing
/// [`MANIFEST_FILE`], in search-directory order, then by path. Missing directories are skipped.
pub fn discover(search_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for dir in search_dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.join(MANIFEST_FILE).is_file())
            .collect();
        found.sort();
        roots.extend(found);
    }
    roots
}

fn read_manifest(root: &Path) -> Result<PackageManifest, String> {
    let text = std::fs::read_to_string(root.join(MANIFEST_FILE)).map_err(|e| e.to_string())?;
    let manifest: PackageManifest = ron::from_str(&text).map_err(|e| e.to_string())?;
    semver::Version::parse(&manifest.version)
        .map_err(|e| format!("version '{}': {e}", manifest.version))?;
    semver::VersionReq::parse(&manifest.aestra_api)
        .map_err(|e| format!("aestra_api '{}': {e}", manifest.aestra_api))?;
    for (id, requirement) in &manifest.dependencies {
        semver::VersionReq::parse(requirement)
            .map_err(|e| format!("dependency '{id}' requirement '{requirement}': {e}"))?;
    }
    if manifest.content.contains("..") || Path::new(&manifest.content).is_absolute() {
        return Err(format!(
            "content '{}' must be a path inside the package",
            manifest.content
        ));
    }
    Ok(manifest)
}

fn read_content(root: &Path, manifest: &PackageManifest) -> Result<PackageContent, String> {
    let text = std::fs::read_to_string(root.join(&manifest.content))
        .map_err(|e| format!("{}: {e}", manifest.content))?;
    ron::from_str(&text).map_err(|e| format!("{}: {e}", manifest.content))
}

/// Discovers, checks and registers packages against `base` (the built-ins plus linked extensions).
/// `disabled` ids are reported but not registered. Nothing global is touched — see [`link_packages`].
pub fn load_packages(
    search_dirs: &[PathBuf],
    disabled: &BTreeSet<ExtensionId>,
    base: &ExtensionRegistry,
) -> LoadedPackages {
    let host_api = semver::Version::parse(EXTENSION_API_VERSION).expect("valid API version");
    let mut reports: Vec<PackageReport> = Vec::new();
    let mut candidates: BTreeMap<ExtensionId, (usize, DeclarativeExtension)> = BTreeMap::new();
    let mut first_root: BTreeMap<ExtensionId, PathBuf> = BTreeMap::new();

    for root in discover(search_dirs) {
        let fallback_name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let manifest = match read_manifest(&root) {
            Ok(manifest) => manifest,
            Err(error) => {
                reports.push(PackageReport {
                    root,
                    id: None,
                    name: fallback_name,
                    version: String::new(),
                    permissions: Vec::new(),
                    status: PackageStatus::InvalidManifest(error),
                });
                continue;
            }
        };
        let index = reports.len();
        let mut report = PackageReport {
            root: root.clone(),
            id: Some(manifest.id.clone()),
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            permissions: manifest.permissions.requested(),
            status: PackageStatus::Registered,
        };
        let api = semver::VersionReq::parse(&manifest.aestra_api).expect("checked on read");
        report.status = if let Some(first) = first_root.get(&manifest.id) {
            PackageStatus::DuplicateId {
                first: first.clone(),
            }
        } else if disabled.contains(&manifest.id) {
            PackageStatus::Disabled
        } else if base.installed_manifest(&manifest.id).is_some() {
            PackageStatus::AlreadyLinked
        } else if !api.matches(&host_api) {
            PackageStatus::IncompatibleApi {
                required: manifest.aestra_api.clone(),
                host: EXTENSION_API_VERSION.into(),
            }
        } else {
            match read_content(&root, &manifest)
                .and_then(|content| DeclarativeExtension::new(manifest.clone(), content))
            {
                Ok(extension) => {
                    candidates.insert(manifest.id.clone(), (index, extension));
                    PackageStatus::Registered
                }
                Err(error) => PackageStatus::InvalidContent(error),
            }
        };
        first_root.entry(manifest.id.clone()).or_insert(root);
        reports.push(report);
    }

    // Dependencies: every requirement must be met by a linked extension or another candidate; drop
    // candidates whose requirements fail until nothing changes (a failure can cascade).
    loop {
        let mut failed: Vec<(ExtensionId, PackageStatus)> = Vec::new();
        for (id, (_, extension)) in &candidates {
            for (dependency, requirement) in &extension.package_manifest().dependencies {
                let dependency_id = ExtensionId::new(dependency);
                let version_req = semver::VersionReq::parse(requirement).expect("checked on read");
                let found = base
                    .installed_manifest(&dependency_id)
                    .map(|manifest| manifest.version.clone())
                    .or_else(|| {
                        candidates
                            .get(&dependency_id)
                            .map(|(_, candidate)| candidate.package_manifest().version.clone())
                    });
                let status = match found {
                    Some(found) => {
                        let matches = semver::Version::parse(&found)
                            .is_ok_and(|version| version_req.matches(&version));
                        (!matches).then(|| PackageStatus::IncompatibleDependency {
                            id: dependency.clone(),
                            requirement: requirement.clone(),
                            found,
                        })
                    }
                    None if first_root.contains_key(&dependency_id) => {
                        Some(PackageStatus::DependencyUnavailable {
                            id: dependency.clone(),
                        })
                    }
                    None => Some(PackageStatus::MissingDependency {
                        id: dependency.clone(),
                        requirement: requirement.clone(),
                    }),
                };
                if let Some(status) = status {
                    failed.push((id.clone(), status));
                    break;
                }
            }
        }
        if failed.is_empty() {
            break;
        }
        for (id, status) in failed {
            if let Some((index, _)) = candidates.remove(&id) {
                reports[index].status = status;
            }
        }
    }

    // Registration in dependency order (dependencies first); what cannot be ordered is a cycle.
    let mut order: Vec<ExtensionId> = Vec::new();
    let mut placed: BTreeSet<ExtensionId> = BTreeSet::new();
    loop {
        let ready: Vec<ExtensionId> = candidates
            .iter()
            .filter(|(id, _)| !placed.contains(*id))
            .filter(|(_, (_, extension))| {
                extension
                    .package_manifest()
                    .dependencies
                    .keys()
                    .map(ExtensionId::new)
                    .all(|dependency| {
                        placed.contains(&dependency) || !candidates.contains_key(&dependency)
                    })
            })
            .map(|(id, _)| id.clone())
            .collect();
        if ready.is_empty() {
            break;
        }
        for id in ready {
            placed.insert(id.clone());
            order.push(id);
        }
    }
    for (id, (index, _)) in &candidates {
        if !placed.contains(id) {
            reports[*index].status = PackageStatus::DependencyCycle;
        }
    }

    let mut registry = base.clone();
    let mut registered = Vec::new();
    let mut rejected: BTreeSet<ExtensionId> = BTreeSet::new();
    for id in order {
        let (index, extension) = candidates.remove(&id).expect("ordered candidate");
        if let Some(dependency) = extension
            .package_manifest()
            .dependencies
            .keys()
            .map(ExtensionId::new)
            .find(|dependency| rejected.contains(dependency))
        {
            reports[index].status = PackageStatus::DependencyUnavailable {
                id: dependency.as_str().to_string(),
            };
            rejected.insert(id);
            continue;
        }
        let mut trial = registry.clone();
        let outcome = trial
            .install(&extension)
            .map_err(|e| e.to_string())
            .and_then(|()| {
                match extension
                    .required_capabilities()
                    .iter()
                    .find(|capability| !trial.capabilities.contains(capability))
                {
                    Some(capability) => Err(format!(
                        "a module requires capability '{}', which nothing registers",
                        capability.as_str()
                    )),
                    None => Ok(()),
                }
            });
        match outcome {
            Ok(()) => {
                registry = trial;
                registered.push(Arc::new(extension));
            }
            Err(error) => {
                reports[index].status = PackageStatus::RegistryConflict(error);
                rejected.insert(id);
            }
        }
    }

    LoadedPackages {
        extensions: registered,
        report: HostReport { packages: reports },
    }
}

/// The `extensions/` directory of the project an effect file belongs to: the nearest ancestor of the
/// effect that has one. Empty when there is none.
pub fn project_extension_dirs(effect_path: &Path) -> Vec<PathBuf> {
    effect_path
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("extensions"))
        .find(|dir| dir.is_dir())
        .into_iter()
        .collect()
}

/// Loads packages against the process's linked registry and links every registered one, so
/// `EffectCompiler::default()` and `ExtensionRegistry::linked()` include them from now on.
pub fn link_packages(search_dirs: &[PathBuf], disabled: &BTreeSet<ExtensionId>) -> HostReport {
    let loaded = load_packages(search_dirs, disabled, &ExtensionRegistry::linked());
    let mut report = loaded.report;
    for extension in loaded.extensions {
        let id = extension.package_manifest().id.clone();
        if let Err(error) = crate::link_extension(extension as Arc<dyn AestraExtension>)
            && let Some(package) = report
                .packages
                .iter_mut()
                .find(|package| package.id.as_ref() == Some(&id))
        {
            package.status = PackageStatus::RegistryConflict(error.to_string());
        }
    }
    report
}
