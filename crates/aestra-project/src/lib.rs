//! Engine-independent discovery and resolution of project-level Aestra assets.
//!
//! Source paths are locations, not semantic identity. Valid effect references use the persisted
//! [`EffectId`] stored inside each effect asset, so moving or renaming a file cannot break a
//! reference. [`ProjectSourceId`] exists only to identify rows and diagnostics for source files
//! that may be invalid and therefore have no readable semantic ID.

pub use aestra_core::EffectAssetRef;
use aestra_core::{AssetError, EffectAsset, EffectClipId, EffectId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// The stable identity of any project-level asset.
///
/// The enum leaves room for typed project mesh, material, and flipbook assets without weakening
/// the typed reference APIs used by consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProjectAssetId {
    Effect(EffectId),
}

impl From<EffectAssetRef> for ProjectAssetId {
    fn from(reference: EffectAssetRef) -> Self {
        Self::Effect(reference.id)
    }
}

/// A deterministic identity for one source location inside an index.
///
/// This is deliberately not serializable and must never be stored as an effect dependency. Its
/// only purpose is to keep UI rows and invalid-file diagnostics addressable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProjectSourceId(u64);

impl ProjectSourceId {
    pub const fn from_u64(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectEffectStatus {
    Valid,
    DuplicateId {
        reference: EffectAssetRef,
        sources: Vec<PathBuf>,
    },
    Invalid {
        message: String,
    },
    Unsupported {
        found: u32,
        current: u32,
    },
}

impl ProjectEffectStatus {
    pub fn is_resolvable(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEffectEntry {
    pub id: ProjectSourceId,
    pub reference: Option<EffectAssetRef>,
    pub display_name: String,
    pub path: PathBuf,
    pub status: ProjectEffectStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectAssetIndexAvailability {
    Ready,
    Unavailable { root: PathBuf, message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectAssetDiagnosticCode {
    SourceUnavailable,
    InvalidAsset,
    UnsupportedFormat,
    DuplicateId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectAssetDiagnostic {
    pub code: ProjectAssetDiagnosticCode,
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ProjectAssetIndex {
    root: PathBuf,
    effects: Vec<ProjectEffectEntry>,
    availability: ProjectAssetIndexAvailability,
    diagnostics: Vec<ProjectAssetDiagnostic>,
    effect_sources: BTreeMap<EffectId, Vec<usize>>,
}

impl ProjectAssetIndex {
    pub fn scan(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_owned();
        let mut paths = Vec::new();
        let mut diagnostics = Vec::new();
        if let Err(error) = collect_effect_sources(&root, &mut paths, &mut diagnostics, true) {
            let message = error.to_string();
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::SourceUnavailable,
                path: Some(root.clone()),
                message: message.clone(),
            });
            return Self {
                root: root.clone(),
                effects: Vec::new(),
                availability: ProjectAssetIndexAvailability::Unavailable { root, message },
                diagnostics,
                effect_sources: BTreeMap::new(),
            };
        }
        paths.sort();
        paths.dedup();

        let effects = paths
            .into_iter()
            .map(|path| index_effect_source(&root, path, &mut diagnostics))
            .collect();
        Self::from_parts(
            root,
            effects,
            ProjectAssetIndexAvailability::Ready,
            diagnostics,
        )
    }

    /// Builds an index from entries supplied by another discovery provider.
    ///
    /// Duplicate semantic IDs are normalized exactly as they are for a filesystem scan.
    pub fn from_entries(root: impl Into<PathBuf>, entries: Vec<ProjectEffectEntry>) -> Self {
        Self::from_parts(
            root.into(),
            entries,
            ProjectAssetIndexAvailability::Ready,
            Vec::new(),
        )
    }

    fn from_parts(
        root: PathBuf,
        mut effects: Vec<ProjectEffectEntry>,
        availability: ProjectAssetIndexAvailability,
        mut diagnostics: Vec<ProjectAssetDiagnostic>,
    ) -> Self {
        let mut source_ids = BTreeSet::new();
        for entry in &mut effects {
            while !source_ids.insert(entry.id) {
                entry.id.0 = entry.id.0.wrapping_add(1);
            }
        }
        effects.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });

        let mut effect_sources = BTreeMap::<EffectId, Vec<usize>>::new();
        for (index, entry) in effects.iter().enumerate() {
            if let Some(reference) = entry.reference {
                effect_sources.entry(reference.id).or_default().push(index);
            }
        }
        for (id, indexes) in &effect_sources {
            if indexes.len() <= 1 {
                continue;
            }
            let reference = EffectAssetRef::new(*id);
            let sources = indexes
                .iter()
                .map(|index| effects[*index].path.clone())
                .collect::<Vec<_>>();
            for index in indexes {
                effects[*index].status = ProjectEffectStatus::DuplicateId {
                    reference,
                    sources: sources.clone(),
                };
            }
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::DuplicateId,
                path: None,
                message: format!(
                    "effect ID {} is declared by {} project sources",
                    id,
                    sources.len()
                ),
            });
        }

        Self {
            root,
            effects,
            availability,
            diagnostics,
            effect_sources,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn effects(&self) -> &[ProjectEffectEntry] {
        &self.effects
    }

    pub fn entry(&self, id: ProjectSourceId) -> Option<&ProjectEffectEntry> {
        self.effects.iter().find(|entry| entry.id == id)
    }

    pub fn availability(&self) -> &ProjectAssetIndexAvailability {
        &self.availability
    }

    pub fn diagnostics(&self) -> &[ProjectAssetDiagnostic] {
        &self.diagnostics
    }

    pub fn resolve(
        &self,
        reference: EffectAssetRef,
    ) -> Result<&ProjectEffectEntry, ResolveEffectError> {
        if let ProjectAssetIndexAvailability::Unavailable { root, message } = &self.availability {
            return Err(ResolveEffectError::IndexUnavailable {
                root: root.clone(),
                message: message.clone(),
            });
        }
        let Some(indexes) = self.effect_sources.get(&reference.id) else {
            return Err(ResolveEffectError::Missing { reference });
        };
        if indexes.len() != 1 {
            return Err(ResolveEffectError::Duplicate {
                reference,
                sources: indexes
                    .iter()
                    .map(|index| self.effects[*index].path.clone())
                    .collect(),
            });
        }
        let entry = &self.effects[indexes[0]];
        if entry.status.is_resolvable() {
            Ok(entry)
        } else {
            Err(ResolveEffectError::Unresolvable {
                reference,
                path: entry.path.clone(),
            })
        }
    }

    /// Resolves and loads the latest source contents for one reusable effect reference.
    pub fn load_effect(
        &self,
        reference: EffectAssetRef,
    ) -> Result<EffectAsset, ResolveEffectError> {
        let entry = self.resolve(reference)?;
        let effect = EffectAsset::load_ron(&entry.path).map_err(|error| {
            ResolveEffectError::SourceChanged {
                reference,
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if effect.id != reference.id {
            return Err(ResolveEffectError::SourceChanged {
                reference,
                path: entry.path.clone(),
                message: format!(
                    "indexed effect ID {} was replaced by {}",
                    reference.id, effect.id
                ),
            });
        }
        Ok(effect)
    }

    /// Resolves the complete transitive dependency set for an authored root effect.
    ///
    /// The root may contain unsaved edits and therefore does not need to be the same bytes as the
    /// indexed source with the same ID. Every referenced child is loaded through the stable index.
    pub fn resolve_effect_project(
        &self,
        root: &EffectAsset,
    ) -> Result<ResolvedEffectProject, ProjectDependencyReport> {
        let mut resolver = DependencyResolver {
            index: self,
            resolved: BTreeMap::new(),
            visiting: Vec::new(),
            visited: BTreeSet::new(),
            diagnostics: Vec::new(),
        };
        resolver.visit(root);
        if resolver.diagnostics.is_empty() {
            Ok(ResolvedEffectProject {
                root: root.clone(),
                dependencies: resolver.resolved,
            })
        } else {
            Err(ProjectDependencyReport {
                diagnostics: resolver.diagnostics,
            })
        }
    }

    pub fn refresh(&mut self) {
        *self = Self::scan(self.root.clone());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveEffectError {
    #[error("project asset index at {root} is unavailable: {message}")]
    IndexUnavailable { root: PathBuf, message: String },
    #[error("effect asset {reference} is missing from the project index")]
    Missing { reference: EffectAssetRef },
    #[error("effect asset {reference} is declared by multiple sources: {sources:?}")]
    Duplicate {
        reference: EffectAssetRef,
        sources: Vec<PathBuf>,
    },
    #[error("effect asset {reference} at {path} is not resolvable")]
    Unresolvable {
        reference: EffectAssetRef,
        path: PathBuf,
    },
    #[error("effect asset {reference} changed after indexing at {path}: {message}")]
    SourceChanged {
        reference: EffectAssetRef,
        path: PathBuf,
        message: String,
    },
}

/// A root effect plus every unique reusable effect it transitively references.
#[derive(Debug, Clone)]
pub struct ResolvedEffectProject {
    pub root: EffectAsset,
    pub dependencies: BTreeMap<EffectId, EffectAsset>,
}

impl ResolvedEffectProject {
    pub fn effect(&self, id: EffectId) -> Option<&EffectAsset> {
        if self.root.id == id {
            Some(&self.root)
        } else {
            self.dependencies.get(&id)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectDependencyDiagnosticCode {
    Missing,
    Duplicate,
    Unresolvable,
    IndexUnavailable,
    SourceChanged,
    InvalidTiming,
    Cycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDependencyDiagnostic {
    pub code: ProjectDependencyDiagnosticCode,
    pub owner: EffectId,
    pub clip: EffectClipId,
    pub reference: EffectAssetRef,
    pub cycle: Vec<EffectId>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("effect dependency resolution failed with {count} diagnostic(s)", count = .diagnostics.len())]
pub struct ProjectDependencyReport {
    pub diagnostics: Vec<ProjectDependencyDiagnostic>,
}

struct DependencyResolver<'a> {
    index: &'a ProjectAssetIndex,
    resolved: BTreeMap<EffectId, EffectAsset>,
    visiting: Vec<EffectId>,
    visited: BTreeSet<EffectId>,
    diagnostics: Vec<ProjectDependencyDiagnostic>,
}

impl DependencyResolver<'_> {
    fn visit(&mut self, effect: &EffectAsset) {
        if self.visited.contains(&effect.id) {
            return;
        }
        self.visiting.push(effect.id);
        for clip in &effect.effect_clips {
            let reference = clip.source;
            if let Some(cycle_start) = self
                .visiting
                .iter()
                .position(|candidate| *candidate == reference.id)
            {
                let mut cycle = self.visiting[cycle_start..].to_vec();
                cycle.push(reference.id);
                self.diagnostics.push(ProjectDependencyDiagnostic {
                    code: ProjectDependencyDiagnosticCode::Cycle,
                    owner: effect.id,
                    clip: clip.id,
                    reference,
                    message: format!("effect reference cycle detected: {cycle:?}"),
                    cycle,
                });
                continue;
            }

            let child = match self.index.load_effect(reference) {
                Ok(child) => child,
                Err(error) => {
                    self.diagnostics.push(ProjectDependencyDiagnostic {
                        code: dependency_code(&error),
                        owner: effect.id,
                        clip: clip.id,
                        reference,
                        cycle: Vec::new(),
                        message: error.to_string(),
                    });
                    continue;
                }
            };
            if !child.looping && clip.source_offset + clip.duration > child.duration {
                self.diagnostics.push(ProjectDependencyDiagnostic {
                    code: ProjectDependencyDiagnosticCode::InvalidTiming,
                    owner: effect.id,
                    clip: clip.id,
                    reference,
                    cycle: Vec::new(),
                    message: format!(
                        "effect clip source window {}..{} exceeds non-looping effect duration {}",
                        clip.source_offset,
                        clip.source_offset + clip.duration,
                        child.duration
                    ),
                });
                continue;
            }
            self.resolved
                .entry(child.id)
                .or_insert_with(|| child.clone());
            self.visit(&child);
        }
        self.visiting.pop();
        self.visited.insert(effect.id);
    }
}

fn dependency_code(error: &ResolveEffectError) -> ProjectDependencyDiagnosticCode {
    match error {
        ResolveEffectError::IndexUnavailable { .. } => {
            ProjectDependencyDiagnosticCode::IndexUnavailable
        }
        ResolveEffectError::Missing { .. } => ProjectDependencyDiagnosticCode::Missing,
        ResolveEffectError::Duplicate { .. } => ProjectDependencyDiagnosticCode::Duplicate,
        ResolveEffectError::Unresolvable { .. } => ProjectDependencyDiagnosticCode::Unresolvable,
        ResolveEffectError::SourceChanged { .. } => ProjectDependencyDiagnosticCode::SourceChanged,
    }
}

fn collect_effect_sources(
    directory: &Path,
    paths: &mut Vec<PathBuf>,
    diagnostics: &mut Vec<ProjectAssetDiagnostic>,
    root: bool,
) -> std::io::Result<()> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if !root => {
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::SourceUnavailable,
                path: Some(directory.to_owned()),
                message: error.to_string(),
            });
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                diagnostics.push(ProjectAssetDiagnostic {
                    code: ProjectAssetDiagnosticCode::SourceUnavailable,
                    path: Some(directory.to_owned()),
                    message: error.to_string(),
                });
                continue;
            }
        };
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => {
                collect_effect_sources(&path, paths, diagnostics, false)?;
            }
            Ok(kind) if kind.is_file() && is_ron_source(&path) => paths.push(path),
            Ok(_) => {}
            Err(error) => diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::SourceUnavailable,
                path: Some(path),
                message: error.to_string(),
            }),
        }
    }
    Ok(())
}

fn is_ron_source(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ron"))
}

fn index_effect_source(
    root: &Path,
    path: PathBuf,
    diagnostics: &mut Vec<ProjectAssetDiagnostic>,
) -> ProjectEffectEntry {
    let id = source_id(root, &path);
    let fallback_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unnamed effect")
        .trim_end_matches(".aestra")
        .replace(['_', '-'], " ");
    match EffectAsset::load_ron(&path) {
        Ok(effect) => ProjectEffectEntry {
            id,
            reference: Some(effect.id.into()),
            display_name: effect.name,
            path,
            status: ProjectEffectStatus::Valid,
        },
        Err(AssetError::UnsupportedFormat { found, current }) => {
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::UnsupportedFormat,
                path: Some(path.clone()),
                message: format!(
                    "effect format version {found} is unsupported; expected {current}"
                ),
            });
            ProjectEffectEntry {
                id,
                reference: None,
                display_name: fallback_name,
                path,
                status: ProjectEffectStatus::Unsupported { found, current },
            }
        }
        Err(error) => {
            let message = error.to_string();
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::InvalidAsset,
                path: Some(path.clone()),
                message: message.clone(),
            });
            ProjectEffectEntry {
                id,
                reference: None,
                display_name: fallback_name,
                path,
                status: ProjectEffectStatus::Invalid { message },
            }
        }
    }
}

fn source_id(root: &Path, path: &Path) -> ProjectSourceId {
    let relative = path.strip_prefix(root).unwrap_or(path);
    let normalized = relative.to_string_lossy().replace('\\', "/").to_lowercase();
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalized.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    ProjectSourceId(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_effect_reference_exposes_the_generic_project_identity() {
        let id = EffectId::from_u128(42);
        let reference = EffectAssetRef::new(id);

        assert_eq!(ProjectAssetId::from(reference), ProjectAssetId::Effect(id));
    }

    #[test]
    fn source_ids_are_location_keys_not_effect_references() {
        let root = Path::new("assets/effects");
        let first = source_id(root, Path::new("assets/effects/one.aestra.ron"));
        let second = source_id(root, Path::new("assets/effects/two.aestra.ron"));

        assert_ne!(first, second);
    }

    #[test]
    fn current_format_constant_matches_the_loader_contract() {
        assert_eq!(aestra_core::CURRENT_FORMAT_VERSION, 3);
    }
}
