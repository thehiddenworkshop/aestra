//! Engine-independent discovery and resolution of project-level Aestra assets.
//!
//! Source paths are locations, not semantic identity. Valid effect and material-program references
//! use persisted semantic IDs, so moving or renaming a file cannot break a reference.
//! [`ProjectSourceId`] exists only to identify rows and diagnostics for source files that may be
//! invalid and therefore have no readable semantic ID.

mod editor_layout;

pub use editor_layout::*;

pub use aestra_core::EffectAssetRef;
use aestra_core::{
    AssetError, EffectAsset, EffectClipId, EffectId, MaterialFunctionId, MaterialId,
    MaterialPresetId, MaterialProgramId,
    material::{
        MaterialFunction, MaterialFunctionError, MaterialFunctionRef, MaterialPresetDescriptor,
        MaterialPresetError, MaterialProgram, MaterialProgramError, MaterialProgramRef,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    path::{Path, PathBuf},
};
use thiserror::Error;

/// The stable identity of any project-level asset.
///
/// The enum leaves room for typed project mesh, texture, and flipbook assets without weakening the
/// typed reference APIs used by consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProjectAssetId {
    Effect(EffectId),
    MaterialProgram(MaterialProgramId),
    MaterialFunction(MaterialFunctionId),
    MaterialPreset(MaterialPresetId),
}

impl From<EffectAssetRef> for ProjectAssetId {
    fn from(reference: EffectAssetRef) -> Self {
        Self::Effect(reference.id)
    }
}

impl From<MaterialProgramRef> for ProjectAssetId {
    fn from(reference: MaterialProgramRef) -> Self {
        Self::MaterialProgram(reference.id())
    }
}

impl From<MaterialFunctionRef> for ProjectAssetId {
    fn from(reference: MaterialFunctionRef) -> Self {
        Self::MaterialFunction(reference.id())
    }
}

impl From<MaterialPresetId> for ProjectAssetId {
    fn from(id: MaterialPresetId) -> Self {
        Self::MaterialPreset(id)
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
pub enum ProjectMaterialProgramStatus {
    Valid,
    DuplicateId {
        reference: MaterialProgramRef,
        sources: Vec<PathBuf>,
    },
    Invalid {
        message: String,
    },
}

impl ProjectMaterialProgramStatus {
    pub fn is_resolvable(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMaterialProgramEntry {
    pub id: ProjectSourceId,
    pub reference: Option<MaterialProgramRef>,
    pub display_name: String,
    pub path: PathBuf,
    pub status: ProjectMaterialProgramStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMaterialFunctionStatus {
    Valid,
    DuplicateId {
        reference: MaterialFunctionRef,
        sources: Vec<PathBuf>,
    },
    Invalid {
        message: String,
    },
}

impl ProjectMaterialFunctionStatus {
    pub fn is_resolvable(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMaterialFunctionEntry {
    pub id: ProjectSourceId,
    pub reference: Option<MaterialFunctionRef>,
    pub display_name: String,
    pub path: PathBuf,
    pub status: ProjectMaterialFunctionStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMaterialPresetStatus {
    Valid,
    DuplicateId {
        preset: MaterialPresetId,
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

impl ProjectMaterialPresetStatus {
    pub fn is_resolvable(&self) -> bool {
        matches!(self, Self::Valid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMaterialPresetEntry {
    pub id: ProjectSourceId,
    pub preset: Option<MaterialPresetId>,
    pub display_name: String,
    pub path: PathBuf,
    pub status: ProjectMaterialPresetStatus,
}

/// One authored effect-clip relationship in the project dependency graph.
///
/// `depth` is one for a direct relationship and increases for every traversed effect. Keeping the
/// owning clip identity makes reverse usages actionable in editor clients instead of reducing the
/// graph to a list of filenames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEffectRelation {
    pub owner: EffectAssetRef,
    pub owner_source: ProjectSourceId,
    pub clip: EffectClipId,
    pub dependency: EffectAssetRef,
    pub depth: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectEffectUsageGraph {
    pub dependencies: Vec<ProjectEffectRelation>,
    pub usages: Vec<ProjectEffectRelation>,
}

impl ProjectEffectUsageGraph {
    pub fn direct_dependencies(&self) -> impl Iterator<Item = &ProjectEffectRelation> {
        self.dependencies
            .iter()
            .filter(|relation| relation.depth == 1)
    }

    pub fn transitive_dependencies(&self) -> impl Iterator<Item = &ProjectEffectRelation> {
        self.dependencies
            .iter()
            .filter(|relation| relation.depth > 1)
    }

    pub fn direct_usages(&self) -> impl Iterator<Item = &ProjectEffectRelation> {
        self.usages.iter().filter(|relation| relation.depth == 1)
    }

    pub fn transitive_usages(&self) -> impl Iterator<Item = &ProjectEffectRelation> {
        self.usages.iter().filter(|relation| relation.depth > 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectEffectDeletePolicy {
    RejectReferenced,
    AllowReferenced,
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
    material_programs: Vec<ProjectMaterialProgramEntry>,
    material_functions: Vec<ProjectMaterialFunctionEntry>,
    material_presets: Vec<ProjectMaterialPresetEntry>,
    availability: ProjectAssetIndexAvailability,
    diagnostics: Vec<ProjectAssetDiagnostic>,
    effect_sources: BTreeMap<EffectId, Vec<usize>>,
    material_program_sources: BTreeMap<MaterialProgramId, Vec<usize>>,
    material_function_sources: BTreeMap<MaterialFunctionId, Vec<usize>>,
    material_preset_sources: BTreeMap<MaterialPresetId, Vec<usize>>,
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
                material_programs: Vec::new(),
                material_functions: Vec::new(),
                material_presets: Vec::new(),
                availability: ProjectAssetIndexAvailability::Unavailable { root, message },
                diagnostics,
                effect_sources: BTreeMap::new(),
                material_program_sources: BTreeMap::new(),
                material_function_sources: BTreeMap::new(),
                material_preset_sources: BTreeMap::new(),
            };
        }
        paths.sort();
        paths.dedup();

        let mut material_paths = Vec::new();
        let mut function_paths = Vec::new();
        let mut preset_paths = Vec::new();
        let mut effect_paths = Vec::new();
        for path in paths {
            if is_material_preset_source(&path) {
                preset_paths.push(path);
            } else if is_material_function_source(&path) {
                function_paths.push(path);
            } else if is_material_program_source(&path) {
                material_paths.push(path);
            } else {
                effect_paths.push(path);
            }
        }
        let effects = effect_paths
            .into_iter()
            .map(|path| index_effect_source(&root, path, &mut diagnostics))
            .collect();
        let material_programs = material_paths
            .into_iter()
            .map(|path| index_material_program_source(&root, path, &mut diagnostics))
            .collect();
        let material_functions = function_paths
            .into_iter()
            .map(|path| index_material_function_source(&root, path, &mut diagnostics))
            .collect();
        let material_presets = preset_paths
            .into_iter()
            .map(|path| index_material_preset_source(&root, path, &mut diagnostics))
            .collect();
        Self::from_parts(
            root,
            effects,
            material_programs,
            material_functions,
            material_presets,
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
            Vec::new(),
            Vec::new(),
            Vec::new(),
            ProjectAssetIndexAvailability::Ready,
            Vec::new(),
        )
    }

    fn from_parts(
        root: PathBuf,
        mut effects: Vec<ProjectEffectEntry>,
        mut material_programs: Vec<ProjectMaterialProgramEntry>,
        mut material_functions: Vec<ProjectMaterialFunctionEntry>,
        mut material_presets: Vec<ProjectMaterialPresetEntry>,
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
        for entry in &mut material_programs {
            while !source_ids.insert(entry.id) {
                entry.id.0 = entry.id.0.wrapping_add(1);
            }
        }
        material_programs.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        for entry in &mut material_functions {
            while !source_ids.insert(entry.id) {
                entry.id.0 = entry.id.0.wrapping_add(1);
            }
        }
        material_functions.sort_by(|left, right| {
            left.display_name
                .to_lowercase()
                .cmp(&right.display_name.to_lowercase())
                .then_with(|| left.path.cmp(&right.path))
        });
        for entry in &mut material_presets {
            while !source_ids.insert(entry.id) {
                entry.id.0 = entry.id.0.wrapping_add(1);
            }
        }
        material_presets.sort_by(|left, right| {
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

        let mut material_program_sources = BTreeMap::<MaterialProgramId, Vec<usize>>::new();
        for (index, entry) in material_programs.iter().enumerate() {
            if let Some(reference) = entry.reference {
                material_program_sources
                    .entry(reference.id())
                    .or_default()
                    .push(index);
            }
        }
        for (id, indexes) in &material_program_sources {
            if indexes.len() <= 1 {
                continue;
            }
            let reference = MaterialProgramRef::Project(*id);
            let sources = indexes
                .iter()
                .map(|index| material_programs[*index].path.clone())
                .collect::<Vec<_>>();
            for index in indexes {
                material_programs[*index].status = ProjectMaterialProgramStatus::DuplicateId {
                    reference,
                    sources: sources.clone(),
                };
            }
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::DuplicateId,
                path: None,
                message: format!(
                    "material program ID {} is declared by {} project sources",
                    id,
                    sources.len()
                ),
            });
        }

        let mut material_function_sources = BTreeMap::<MaterialFunctionId, Vec<usize>>::new();
        for (index, entry) in material_functions.iter().enumerate() {
            if let Some(reference) = entry.reference {
                material_function_sources
                    .entry(reference.id())
                    .or_default()
                    .push(index);
            }
        }
        for (id, indexes) in &material_function_sources {
            if indexes.len() <= 1 {
                continue;
            }
            let reference = MaterialFunctionRef::Project(*id);
            let sources = indexes
                .iter()
                .map(|index| material_functions[*index].path.clone())
                .collect::<Vec<_>>();
            for index in indexes {
                material_functions[*index].status = ProjectMaterialFunctionStatus::DuplicateId {
                    reference,
                    sources: sources.clone(),
                };
            }
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::DuplicateId,
                path: None,
                message: format!(
                    "material function ID {} is declared by {} project sources",
                    id,
                    sources.len()
                ),
            });
        }

        let mut material_preset_sources = BTreeMap::<MaterialPresetId, Vec<usize>>::new();
        for (index, entry) in material_presets.iter().enumerate() {
            if let Some(preset) = entry.preset {
                material_preset_sources
                    .entry(preset)
                    .or_default()
                    .push(index);
            }
        }
        for (preset, indexes) in &material_preset_sources {
            if indexes.len() <= 1 {
                continue;
            }
            let sources = indexes
                .iter()
                .map(|index| material_presets[*index].path.clone())
                .collect::<Vec<_>>();
            for index in indexes {
                material_presets[*index].status = ProjectMaterialPresetStatus::DuplicateId {
                    preset: *preset,
                    sources: sources.clone(),
                };
            }
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::DuplicateId,
                path: None,
                message: format!(
                    "material preset ID {} is declared by {} project sources",
                    preset,
                    sources.len()
                ),
            });
        }

        Self {
            root,
            effects,
            material_programs,
            material_functions,
            material_presets,
            availability,
            diagnostics,
            effect_sources,
            material_program_sources,
            material_function_sources,
            material_preset_sources,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn effects(&self) -> &[ProjectEffectEntry] {
        &self.effects
    }

    pub fn material_programs(&self) -> &[ProjectMaterialProgramEntry] {
        &self.material_programs
    }

    pub fn material_functions(&self) -> &[ProjectMaterialFunctionEntry] {
        &self.material_functions
    }

    pub fn material_presets(&self) -> &[ProjectMaterialPresetEntry] {
        &self.material_presets
    }

    pub fn entry(&self, id: ProjectSourceId) -> Option<&ProjectEffectEntry> {
        self.effects.iter().find(|entry| entry.id == id)
    }

    pub fn material_program_entry(
        &self,
        id: ProjectSourceId,
    ) -> Option<&ProjectMaterialProgramEntry> {
        self.material_programs.iter().find(|entry| entry.id == id)
    }

    pub fn material_function_entry(
        &self,
        id: ProjectSourceId,
    ) -> Option<&ProjectMaterialFunctionEntry> {
        self.material_functions.iter().find(|entry| entry.id == id)
    }

    pub fn material_preset_entry(
        &self,
        id: ProjectSourceId,
    ) -> Option<&ProjectMaterialPresetEntry> {
        self.material_presets.iter().find(|entry| entry.id == id)
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

    pub fn resolve_material_program(
        &self,
        reference: MaterialProgramRef,
    ) -> Result<&ProjectMaterialProgramEntry, ResolveMaterialProgramError> {
        if let ProjectAssetIndexAvailability::Unavailable { root, message } = &self.availability {
            return Err(ResolveMaterialProgramError::IndexUnavailable {
                root: root.clone(),
                message: message.clone(),
            });
        }
        let MaterialProgramRef::Project(id) = reference else {
            return Err(ResolveMaterialProgramError::BuiltInNotIndexed { reference });
        };
        let Some(indexes) = self.material_program_sources.get(&id) else {
            return Err(ResolveMaterialProgramError::Missing { reference });
        };
        if indexes.len() != 1 {
            return Err(ResolveMaterialProgramError::Duplicate {
                reference,
                sources: indexes
                    .iter()
                    .map(|index| self.material_programs[*index].path.clone())
                    .collect(),
            });
        }
        let entry = &self.material_programs[indexes[0]];
        if entry.status.is_resolvable() {
            Ok(entry)
        } else {
            Err(ResolveMaterialProgramError::Unresolvable {
                reference,
                path: entry.path.clone(),
            })
        }
    }

    pub fn resolve_material_function(
        &self,
        reference: MaterialFunctionRef,
    ) -> Result<&ProjectMaterialFunctionEntry, ResolveMaterialFunctionError> {
        if let ProjectAssetIndexAvailability::Unavailable { root, message } = &self.availability {
            return Err(ResolveMaterialFunctionError::IndexUnavailable {
                root: root.clone(),
                message: message.clone(),
            });
        }
        let MaterialFunctionRef::Project(id) = reference else {
            return Err(ResolveMaterialFunctionError::BuiltInNotIndexed { reference });
        };
        let Some(indexes) = self.material_function_sources.get(&id) else {
            return Err(ResolveMaterialFunctionError::Missing { reference });
        };
        if indexes.len() != 1 {
            return Err(ResolveMaterialFunctionError::Duplicate {
                reference,
                sources: indexes
                    .iter()
                    .map(|index| self.material_functions[*index].path.clone())
                    .collect(),
            });
        }
        let entry = &self.material_functions[indexes[0]];
        if entry.status.is_resolvable() {
            Ok(entry)
        } else {
            Err(ResolveMaterialFunctionError::Unresolvable {
                reference,
                path: entry.path.clone(),
            })
        }
    }

    pub fn resolve_material_preset(
        &self,
        preset: MaterialPresetId,
    ) -> Result<&ProjectMaterialPresetEntry, ResolveMaterialPresetError> {
        if let ProjectAssetIndexAvailability::Unavailable { root, message } = &self.availability {
            return Err(ResolveMaterialPresetError::IndexUnavailable {
                root: root.clone(),
                message: message.clone(),
            });
        }
        let Some(indexes) = self.material_preset_sources.get(&preset) else {
            return Err(ResolveMaterialPresetError::Missing { preset });
        };
        if indexes.len() != 1 {
            return Err(ResolveMaterialPresetError::Duplicate {
                preset,
                sources: indexes
                    .iter()
                    .map(|index| self.material_presets[*index].path.clone())
                    .collect(),
            });
        }
        let entry = &self.material_presets[indexes[0]];
        if entry.status.is_resolvable() {
            Ok(entry)
        } else {
            Err(ResolveMaterialPresetError::Unresolvable {
                preset,
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

    pub fn load_material_program(
        &self,
        reference: MaterialProgramRef,
    ) -> Result<MaterialProgram, ResolveMaterialProgramError> {
        let entry = self.resolve_material_program(reference)?;
        let program = MaterialProgram::load_ron(&entry.path).map_err(|error| {
            ResolveMaterialProgramError::SourceChanged {
                reference,
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if program.id != reference.id() {
            return Err(ResolveMaterialProgramError::SourceChanged {
                reference,
                path: entry.path.clone(),
                message: format!(
                    "indexed material program ID {} was replaced by {}",
                    reference.id(),
                    program.id
                ),
            });
        }
        Ok(program)
    }

    pub fn load_material_function(
        &self,
        reference: MaterialFunctionRef,
    ) -> Result<MaterialFunction, ResolveMaterialFunctionError> {
        let entry = self.resolve_material_function(reference)?;
        let function = MaterialFunction::load_ron(&entry.path).map_err(|error| {
            ResolveMaterialFunctionError::SourceChanged {
                reference,
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if function.id != reference.id() {
            return Err(ResolveMaterialFunctionError::SourceChanged {
                reference,
                path: entry.path.clone(),
                message: format!(
                    "indexed material function ID {} was replaced by {}",
                    reference.id(),
                    function.id
                ),
            });
        }
        Ok(function)
    }

    pub fn load_material_preset(
        &self,
        preset: MaterialPresetId,
    ) -> Result<MaterialPresetDescriptor, ResolveMaterialPresetError> {
        let entry = self.resolve_material_preset(preset)?;
        let descriptor = MaterialPresetDescriptor::load_ron(&entry.path).map_err(|error| {
            ResolveMaterialPresetError::SourceChanged {
                preset,
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if descriptor.id != preset {
            return Err(ResolveMaterialPresetError::SourceChanged {
                preset,
                path: entry.path.clone(),
                message: format!(
                    "indexed material preset ID {} was replaced by {}",
                    preset, descriptor.id
                ),
            });
        }
        Ok(descriptor)
    }

    /// Loads every uniquely identified project-local material function.
    pub fn load_material_functions(
        &self,
    ) -> Result<BTreeMap<MaterialFunctionId, MaterialFunction>, ResolveMaterialFunctionError> {
        let mut functions = BTreeMap::new();
        for entry in &self.material_functions {
            let Some(reference) = entry.reference else {
                continue;
            };
            if functions.contains_key(&reference.id()) {
                continue;
            }
            let function = self.load_material_function(reference)?;
            functions.insert(function.id, function);
        }
        Ok(functions)
    }

    /// Loads every uniquely identified project-local material preset.
    pub fn load_material_presets(
        &self,
    ) -> Result<BTreeMap<MaterialPresetId, MaterialPresetDescriptor>, ResolveMaterialPresetError>
    {
        let mut presets = BTreeMap::new();
        for entry in &self.material_presets {
            let Some(preset) = entry.preset else {
                continue;
            };
            if presets.contains_key(&preset) {
                continue;
            }
            presets.insert(preset, self.load_material_preset(preset)?);
        }
        Ok(presets)
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
            material_programs: BTreeMap::new(),
            visiting: Vec::new(),
            visited: BTreeSet::new(),
            diagnostics: Vec::new(),
            material_diagnostics: Vec::new(),
        };
        resolver.visit(root);
        if resolver.diagnostics.is_empty() && resolver.material_diagnostics.is_empty() {
            let material_functions = self
                .material_functions
                .iter()
                .filter_map(|entry| entry.reference)
                .filter_map(|reference| self.load_material_function(reference).ok())
                .map(|function| (function.id, function))
                .collect();
            Ok(ResolvedEffectProject {
                root: root.clone(),
                dependencies: resolver.resolved,
                material_programs: resolver.material_programs,
                material_functions,
            })
        } else {
            Err(ProjectDependencyReport {
                diagnostics: resolver.diagnostics,
                material_diagnostics: resolver.material_diagnostics,
            })
        }
    }

    /// Builds the forward dependency and reverse usage graph for one project effect.
    ///
    /// All valid indexed effects are reloaded so external source changes cannot produce a stale
    /// deletion decision. Relations are deterministic and preserve every authored clip, even when
    /// an owner instantiates the same dependency more than once.
    pub fn effect_usage_graph(
        &self,
        reference: EffectAssetRef,
    ) -> Result<ProjectEffectUsageGraph, ResolveEffectError> {
        self.resolve(reference)?;
        let edges = self.effect_relation_edges()?;
        Ok(ProjectEffectUsageGraph {
            dependencies: traverse_effect_relations(reference, &edges, false),
            usages: traverse_effect_relations(reference, &edges, true),
        })
    }

    pub fn refresh(&mut self) {
        *self = Self::scan(self.root.clone());
    }

    /// Creates a new effect source directly inside the indexed project root.
    ///
    /// The display name determines the normalized filename. Existing files are never replaced,
    /// and the index is refreshed before the newly created stable reference is returned.
    pub fn create_effect_source(
        &mut self,
        effect: &EffectAsset,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        let name = effect.name.trim();
        let Some(stem) = effect_source_stem(name) else {
            return Err(ProjectAssetOperationError::InvalidName);
        };
        let destination = self.root.join(format!("{stem}.aestra.ron"));
        if destination.exists() {
            return Err(ProjectAssetOperationError::DestinationExists { path: destination });
        }
        effect
            .save_ron(&destination)
            .map_err(|error| ProjectAssetOperationError::Asset {
                path: destination.clone(),
                message: error.to_string(),
            })?;

        let reference = EffectAssetRef::new(effect.id);
        self.refresh();
        self.resolve(reference)
            .cloned()
            .map_err(|error| ProjectAssetOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    pub fn create_material_program_source(
        &mut self,
        program: &MaterialProgram,
    ) -> Result<ProjectMaterialProgramEntry, ProjectMaterialProgramOperationError> {
        let name = program.name.trim();
        let Some(stem) = effect_source_stem(name) else {
            return Err(ProjectMaterialProgramOperationError::InvalidName);
        };
        let destination = self.root.join(format!("{stem}.aestra.material.ron"));
        if destination.exists() {
            return Err(ProjectMaterialProgramOperationError::DestinationExists {
                path: destination,
            });
        }
        program.save_ron(&destination).map_err(|error| {
            ProjectMaterialProgramOperationError::Asset {
                path: destination.clone(),
                message: error.to_string(),
            }
        })?;

        let reference = MaterialProgramRef::Project(program.id);
        self.refresh();
        self.resolve_material_program(reference)
            .cloned()
            .map_err(|error| ProjectMaterialProgramOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    /// Creates one project-local reusable material function.
    pub fn create_material_function_source(
        &mut self,
        function: &MaterialFunction,
    ) -> Result<ProjectMaterialFunctionEntry, ProjectMaterialFunctionOperationError> {
        let name = function.name.trim();
        let Some(stem) = effect_source_stem(name) else {
            return Err(ProjectMaterialFunctionOperationError::InvalidName);
        };
        let reference = MaterialFunctionRef::Project(function.id);
        if self
            .material_functions
            .iter()
            .any(|entry| entry.reference.is_some_and(|item| item.id() == function.id))
        {
            return Err(ProjectMaterialFunctionOperationError::IdentityExists { reference });
        }
        let destination = self
            .root
            .join(format!("{stem}.aestra.material-function.ron"));
        if destination.exists() {
            return Err(ProjectMaterialFunctionOperationError::DestinationExists {
                path: destination,
            });
        }
        function.save_ron(&destination).map_err(|error| {
            ProjectMaterialFunctionOperationError::Asset {
                path: destination.clone(),
                message: error.to_string(),
            }
        })?;

        self.refresh();
        match self.resolve_material_function(reference).cloned() {
            Ok(entry) => Ok(entry),
            Err(error) => {
                let rollback = fs::remove_file(&destination)
                    .err()
                    .map(|error| format!("; removing the incomplete source also failed: {error}"))
                    .unwrap_or_default();
                self.refresh();
                Err(ProjectMaterialFunctionOperationError::Refresh {
                    reference,
                    message: format!("{error}{rollback}"),
                })
            }
        }
    }

    /// Deletes a material-function source only when it still matches the expected authored value.
    pub fn delete_material_function_source(
        &mut self,
        reference: MaterialFunctionRef,
        expected: &MaterialFunction,
    ) -> Result<(), ProjectMaterialFunctionOperationError> {
        let entry = self
            .resolve_material_function(reference)
            .cloned()
            .map_err(|error| ProjectMaterialFunctionOperationError::Resolve {
                reference,
                message: error.to_string(),
            })?;
        let current = MaterialFunction::load_ron(&entry.path).map_err(|error| {
            ProjectMaterialFunctionOperationError::Asset {
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if current != *expected {
            return Err(ProjectMaterialFunctionOperationError::SourceConflict {
                reference,
                path: entry.path,
            });
        }
        fs::remove_file(&entry.path).map_err(|error| {
            ProjectMaterialFunctionOperationError::FileSystem {
                operation: "delete",
                path: entry.path,
                message: error.to_string(),
            }
        })?;
        self.refresh();
        Ok(())
    }

    /// Atomically replaces one material-program source after verifying that it still contains
    /// the exact program the caller edited.
    ///
    /// The expected-value check prevents an editor transaction from silently overwriting an
    /// external change made after the program was loaded.
    pub fn replace_material_program_source(
        &mut self,
        source: ProjectSourceId,
        expected: &MaterialProgram,
        replacement: &MaterialProgram,
    ) -> Result<ProjectMaterialProgramEntry, ProjectMaterialProgramOperationError> {
        let expected = expected.normalized();
        let replacement = replacement.normalized();
        let entry = self.material_program_source_for_operation(source)?;
        let reference = entry
            .reference
            .expect("a resolvable material program source has a semantic reference");
        if expected.id != reference.id() {
            return Err(ProjectMaterialProgramOperationError::IdentityChanged {
                reference,
                path: entry.path,
            });
        }
        if replacement.id != expected.id {
            return Err(
                ProjectMaterialProgramOperationError::ReplacementIdentityChanged {
                    expected: expected.id,
                    actual: replacement.id,
                },
            );
        }
        replacement
            .validate()
            .map_err(|error| ProjectMaterialProgramOperationError::Asset {
                path: entry.path.clone(),
                message: error.to_string(),
            })?;
        let current = MaterialProgram::load_ron(&entry.path).map_err(|error| {
            ProjectMaterialProgramOperationError::Asset {
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if current != expected {
            return Err(ProjectMaterialProgramOperationError::SourceConflict {
                reference,
                path: entry.path,
            });
        }
        replacement.save_ron(&entry.path).map_err(|error| {
            ProjectMaterialProgramOperationError::Asset {
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;

        self.refresh();
        self.resolve_material_program(reference)
            .cloned()
            .map_err(|error| ProjectMaterialProgramOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    pub fn rename_material_program_source(
        &mut self,
        source: ProjectSourceId,
        new_name: &str,
    ) -> Result<ProjectMaterialProgramEntry, ProjectMaterialProgramOperationError> {
        let entry = self.material_program_source_for_operation(source)?;
        let reference = entry
            .reference
            .expect("a resolvable material program source has a semantic reference");
        let new_name = new_name.trim();
        let Some(stem) = effect_source_stem(new_name) else {
            return Err(ProjectMaterialProgramOperationError::InvalidName);
        };
        let Some(parent) = entry.path.parent() else {
            return Err(ProjectMaterialProgramOperationError::InvalidSource {
                path: entry.path.clone(),
            });
        };
        let destination = parent.join(format!("{stem}.aestra.material.ron"));
        let same_file = destination.exists()
            && fs::canonicalize(&entry.path).ok() == fs::canonicalize(&destination).ok();
        let moved = destination != entry.path && !same_file;
        if destination.exists() && !same_file {
            return Err(ProjectMaterialProgramOperationError::DestinationExists {
                path: destination,
            });
        }

        let mut program = MaterialProgram::load_ron(&entry.path).map_err(|error| {
            ProjectMaterialProgramOperationError::Asset {
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if program.id != reference.id() {
            return Err(ProjectMaterialProgramOperationError::IdentityChanged {
                reference,
                path: entry.path,
            });
        }

        if moved {
            fs::rename(&entry.path, &destination).map_err(|error| {
                ProjectMaterialProgramOperationError::FileSystem {
                    operation: "rename",
                    path: entry.path.clone(),
                    message: error.to_string(),
                }
            })?;
        }
        program.name = new_name.to_owned();
        if let Err(error) = program.save_ron(&destination) {
            let rollback = moved
                .then(|| fs::rename(&destination, &entry.path))
                .and_then(Result::err)
                .map(|error| format!("; restoring the original source also failed: {error}"))
                .unwrap_or_default();
            return Err(ProjectMaterialProgramOperationError::Asset {
                path: destination,
                message: format!("{error}{rollback}"),
            });
        }

        self.refresh();
        self.resolve_material_program(reference)
            .cloned()
            .map_err(|error| ProjectMaterialProgramOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    pub fn move_material_program_source(
        &mut self,
        source: ProjectSourceId,
        destination_directory: impl AsRef<Path>,
    ) -> Result<ProjectMaterialProgramEntry, ProjectMaterialProgramOperationError> {
        let entry = self.material_program_source_for_operation(source)?;
        let reference = entry
            .reference
            .expect("a resolvable material program source has a semantic reference");
        let destination_directory = destination_directory.as_ref();
        if !destination_directory.is_dir() {
            return Err(ProjectMaterialProgramOperationError::InvalidDestination {
                path: destination_directory.to_owned(),
            });
        }
        let root = fs::canonicalize(&self.root).map_err(|error| {
            ProjectMaterialProgramOperationError::FileSystem {
                operation: "resolve project root",
                path: self.root.clone(),
                message: error.to_string(),
            }
        })?;
        let destination_directory = fs::canonicalize(destination_directory).map_err(|error| {
            ProjectMaterialProgramOperationError::FileSystem {
                operation: "resolve destination",
                path: destination_directory.to_owned(),
                message: error.to_string(),
            }
        })?;
        if !destination_directory.starts_with(&root) {
            return Err(
                ProjectMaterialProgramOperationError::DestinationOutsideRoot {
                    destination: destination_directory,
                    root,
                },
            );
        }
        let Some(file_name) = entry.path.file_name() else {
            return Err(ProjectMaterialProgramOperationError::InvalidSource {
                path: entry.path.clone(),
            });
        };
        let destination = destination_directory.join(file_name);
        if destination.exists() {
            let source_path = fs::canonicalize(&entry.path).ok();
            let destination_path = fs::canonicalize(&destination).ok();
            if source_path == destination_path {
                return Ok(entry);
            }
            return Err(ProjectMaterialProgramOperationError::DestinationExists {
                path: destination,
            });
        }
        fs::rename(&entry.path, &destination).map_err(|error| {
            ProjectMaterialProgramOperationError::FileSystem {
                operation: "move",
                path: entry.path,
                message: error.to_string(),
            }
        })?;

        self.refresh();
        self.resolve_material_program(reference)
            .cloned()
            .map_err(|error| ProjectMaterialProgramOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    /// Renames one resolvable effect source and its authored display name.
    ///
    /// The persisted [`EffectId`] is left untouched, so every existing [`EffectAssetRef`] keeps
    /// resolving after the source filename changes.
    pub fn rename_effect_source(
        &mut self,
        source: ProjectSourceId,
        new_name: &str,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        let entry = self.effect_source_for_operation(source)?;
        let reference = entry
            .reference
            .expect("a resolvable effect source has a semantic reference");
        let new_name = new_name.trim();
        let Some(stem) = effect_source_stem(new_name) else {
            return Err(ProjectAssetOperationError::InvalidName);
        };
        let Some(parent) = entry.path.parent() else {
            return Err(ProjectAssetOperationError::InvalidSource {
                path: entry.path.clone(),
            });
        };
        let destination = parent.join(format!("{stem}.aestra.ron"));
        let same_file = destination.exists()
            && fs::canonicalize(&entry.path).ok() == fs::canonicalize(&destination).ok();
        let moved = destination != entry.path && !same_file;
        if destination.exists() && !same_file {
            return Err(ProjectAssetOperationError::DestinationExists { path: destination });
        }

        let mut effect = EffectAsset::load_ron(&entry.path).map_err(|error| {
            ProjectAssetOperationError::Asset {
                path: entry.path.clone(),
                message: error.to_string(),
            }
        })?;
        if effect.id != reference.id {
            return Err(ProjectAssetOperationError::IdentityChanged {
                reference,
                path: entry.path,
            });
        }

        if moved {
            fs::rename(&entry.path, &destination).map_err(|error| {
                ProjectAssetOperationError::FileSystem {
                    operation: "rename",
                    path: entry.path.clone(),
                    message: error.to_string(),
                }
            })?;
        }
        effect.name = new_name.to_owned();
        if let Err(error) = effect.save_ron(&destination) {
            let rollback = moved
                .then(|| fs::rename(&destination, &entry.path))
                .and_then(Result::err)
                .map(|error| format!("; restoring the original source also failed: {error}"))
                .unwrap_or_default();
            return Err(ProjectAssetOperationError::Asset {
                path: destination,
                message: format!("{error}{rollback}"),
            });
        }

        self.refresh();
        self.resolve(reference)
            .cloned()
            .map_err(|error| ProjectAssetOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    /// Moves one resolvable effect source to another directory inside the indexed root.
    ///
    /// Moving outside the project root is rejected because the source would immediately become
    /// unavailable to all references in this index.
    pub fn move_effect_source(
        &mut self,
        source: ProjectSourceId,
        destination_directory: impl AsRef<Path>,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        let entry = self.effect_source_for_operation(source)?;
        let reference = entry
            .reference
            .expect("a resolvable effect source has a semantic reference");
        let destination_directory = destination_directory.as_ref();
        if !destination_directory.is_dir() {
            return Err(ProjectAssetOperationError::InvalidDestination {
                path: destination_directory.to_owned(),
            });
        }
        let root = fs::canonicalize(&self.root).map_err(|error| {
            ProjectAssetOperationError::FileSystem {
                operation: "resolve project root",
                path: self.root.clone(),
                message: error.to_string(),
            }
        })?;
        let destination_directory = fs::canonicalize(destination_directory).map_err(|error| {
            ProjectAssetOperationError::FileSystem {
                operation: "resolve destination",
                path: destination_directory.to_owned(),
                message: error.to_string(),
            }
        })?;
        if !destination_directory.starts_with(&root) {
            return Err(ProjectAssetOperationError::DestinationOutsideRoot {
                destination: destination_directory,
                root,
            });
        }
        let Some(file_name) = entry.path.file_name() else {
            return Err(ProjectAssetOperationError::InvalidSource {
                path: entry.path.clone(),
            });
        };
        let destination = destination_directory.join(file_name);
        if destination.exists() {
            let source_path = fs::canonicalize(&entry.path).ok();
            let destination_path = fs::canonicalize(&destination).ok();
            if source_path == destination_path {
                return Ok(entry);
            }
            return Err(ProjectAssetOperationError::DestinationExists { path: destination });
        }
        fs::rename(&entry.path, &destination).map_err(|error| {
            ProjectAssetOperationError::FileSystem {
                operation: "move",
                path: entry.path,
                message: error.to_string(),
            }
        })?;

        self.refresh();
        self.resolve(reference)
            .cloned()
            .map_err(|error| ProjectAssetOperationError::Refresh {
                reference,
                message: error.to_string(),
            })
    }

    /// Deletes one resolvable effect source after applying an explicit reference policy.
    ///
    /// Callers must deliberately opt into breaking references. This keeps non-UI consumers safe
    /// while allowing an editor confirmation surface to perform the requested destructive action.
    pub fn delete_effect_source(
        &mut self,
        source: ProjectSourceId,
        policy: ProjectEffectDeletePolicy,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        let entry = self.effect_source_for_operation(source)?;
        let reference = entry
            .reference
            .expect("a resolvable effect source has a semantic reference");
        if policy == ProjectEffectDeletePolicy::RejectReferenced {
            let graph = self.effect_usage_graph(reference).map_err(|error| {
                ProjectAssetOperationError::UsageInspection {
                    reference,
                    message: error.to_string(),
                }
            })?;
            let usage_count = graph.direct_usages().count();
            if usage_count > 0 {
                return Err(ProjectAssetOperationError::Referenced {
                    reference,
                    usage_count,
                });
            }
        }
        fs::remove_file(&entry.path).map_err(|error| ProjectAssetOperationError::FileSystem {
            operation: "delete",
            path: entry.path.clone(),
            message: error.to_string(),
        })?;
        self.refresh();
        Ok(entry)
    }

    fn effect_relation_edges(&self) -> Result<Vec<ProjectEffectRelation>, ResolveEffectError> {
        let mut edges = Vec::new();
        for entry in self
            .effects
            .iter()
            .filter(|entry| entry.status.is_resolvable())
        {
            let Some(reference) = entry.reference else {
                continue;
            };
            let effect = self.load_effect(reference)?;
            edges.extend(
                effect
                    .effect_clips
                    .into_iter()
                    .map(|clip| ProjectEffectRelation {
                        owner: reference,
                        owner_source: entry.id,
                        clip: clip.id,
                        dependency: clip.source,
                        depth: 1,
                    }),
            );
        }
        Ok(edges)
    }

    fn effect_source_for_operation(
        &self,
        source: ProjectSourceId,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        let entry = self
            .entry(source)
            .cloned()
            .ok_or(ProjectAssetOperationError::SourceMissing { id: source })?;
        if !entry.status.is_resolvable() || entry.reference.is_none() {
            return Err(ProjectAssetOperationError::SourceNotResolvable {
                id: source,
                path: entry.path,
            });
        }
        Ok(entry)
    }

    fn material_program_source_for_operation(
        &self,
        source: ProjectSourceId,
    ) -> Result<ProjectMaterialProgramEntry, ProjectMaterialProgramOperationError> {
        let entry = self
            .material_program_entry(source)
            .cloned()
            .ok_or(ProjectMaterialProgramOperationError::SourceMissing { id: source })?;
        if !entry.status.is_resolvable() || entry.reference.is_none() {
            return Err(ProjectMaterialProgramOperationError::SourceNotResolvable {
                id: source,
                path: entry.path,
            });
        }
        Ok(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProjectAssetOperationError {
    #[error("project source {id:?} is no longer present in the asset index")]
    SourceMissing { id: ProjectSourceId },
    #[error("project source {id:?} at {path} is not a resolvable effect")]
    SourceNotResolvable { id: ProjectSourceId, path: PathBuf },
    #[error("effect name must contain at least one letter or number")]
    InvalidName,
    #[error("effect source path {path} has no valid parent or filename")]
    InvalidSource { path: PathBuf },
    #[error("destination {path} is not an existing directory")]
    InvalidDestination { path: PathBuf },
    #[error("destination {destination} is outside the project effect root {root}")]
    DestinationOutsideRoot { destination: PathBuf, root: PathBuf },
    #[error("destination source already exists at {path}")]
    DestinationExists { path: PathBuf },
    #[error("effect {reference} is used by {usage_count} project clip(s)")]
    Referenced {
        reference: EffectAssetRef,
        usage_count: usize,
    },
    #[error("could not inspect usages of effect {reference}: {message}")]
    UsageInspection {
        reference: EffectAssetRef,
        message: String,
    },
    #[error("effect source at {path} changed semantic identity from {reference}")]
    IdentityChanged {
        reference: EffectAssetRef,
        path: PathBuf,
    },
    #[error("could not read or save effect source at {path}: {message}")]
    Asset { path: PathBuf, message: String },
    #[error("could not {operation} {path}: {message}")]
    FileSystem {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    #[error("effect {reference} no longer resolves after the asset operation: {message}")]
    Refresh {
        reference: EffectAssetRef,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProjectMaterialProgramOperationError {
    #[error("project source {id:?} is no longer present in the material-program index")]
    SourceMissing { id: ProjectSourceId },
    #[error("project source {id:?} at {path} is not a resolvable material program")]
    SourceNotResolvable { id: ProjectSourceId, path: PathBuf },
    #[error("material program name must contain at least one letter or number")]
    InvalidName,
    #[error("material program source path {path} has no valid parent or filename")]
    InvalidSource { path: PathBuf },
    #[error("destination {path} is not an existing directory")]
    InvalidDestination { path: PathBuf },
    #[error("destination {destination} is outside the project root {root}")]
    DestinationOutsideRoot { destination: PathBuf, root: PathBuf },
    #[error("destination source already exists at {path}")]
    DestinationExists { path: PathBuf },
    #[error("material program source at {path} changed semantic identity from {reference:?}")]
    IdentityChanged {
        reference: MaterialProgramRef,
        path: PathBuf,
    },
    #[error("replacement material program must preserve identity {expected}, received {actual}")]
    ReplacementIdentityChanged {
        expected: MaterialProgramId,
        actual: MaterialProgramId,
    },
    #[error("material program {reference:?} changed on disk at {path}; reload before editing")]
    SourceConflict {
        reference: MaterialProgramRef,
        path: PathBuf,
    },
    #[error("could not read or save material program source at {path}: {message}")]
    Asset { path: PathBuf, message: String },
    #[error("could not {operation} {path}: {message}")]
    FileSystem {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    #[error(
        "material program {reference:?} no longer resolves after the asset operation: {message}"
    )]
    Refresh {
        reference: MaterialProgramRef,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProjectMaterialFunctionOperationError {
    #[error("material function name must contain at least one letter or number")]
    InvalidName,
    #[error("material function {reference:?} already exists in the project")]
    IdentityExists { reference: MaterialFunctionRef },
    #[error("destination source already exists at {path}")]
    DestinationExists { path: PathBuf },
    #[error("material function {reference:?} could not be resolved: {message}")]
    Resolve {
        reference: MaterialFunctionRef,
        message: String,
    },
    #[error("material function {reference:?} changed on disk at {path}; reload before editing")]
    SourceConflict {
        reference: MaterialFunctionRef,
        path: PathBuf,
    },
    #[error("could not read or save material function source at {path}: {message}")]
    Asset { path: PathBuf, message: String },
    #[error("could not {operation} {path}: {message}")]
    FileSystem {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
    #[error("material function {reference:?} did not resolve after the asset operation: {message}")]
    Refresh {
        reference: MaterialFunctionRef,
        message: String,
    },
}

fn traverse_effect_relations(
    reference: EffectAssetRef,
    edges: &[ProjectEffectRelation],
    reverse: bool,
) -> Vec<ProjectEffectRelation> {
    let mut relations = Vec::new();
    let mut queue = VecDeque::from([(reference.id, 0_usize)]);
    let mut visited = BTreeSet::from([reference.id]);
    while let Some((current, depth)) = queue.pop_front() {
        for edge in edges.iter().filter(|edge| {
            if reverse {
                edge.dependency.id == current
            } else {
                edge.owner.id == current
            }
        }) {
            let next = if reverse {
                edge.owner.id
            } else {
                edge.dependency.id
            };
            let mut relation = edge.clone();
            relation.depth = depth + 1;
            relations.push(relation);
            if visited.insert(next) {
                queue.push_back((next, depth + 1));
            }
        }
    }
    relations.sort_by_key(|relation| (relation.depth, relation.owner_source, relation.clip));
    relations
}

fn effect_source_stem(name: &str) -> Option<String> {
    let mut stem = String::new();
    let mut separator_pending = false;
    for character in name.chars() {
        if character.is_alphanumeric() {
            if separator_pending && !stem.is_empty() {
                stem.push('_');
            }
            stem.extend(character.to_lowercase());
            separator_pending = false;
        } else {
            separator_pending = true;
        }
    }
    if stem.is_empty() {
        return None;
    }
    if matches!(
        stem.as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    ) {
        stem.push_str("_effect");
    }
    Some(stem)
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

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveMaterialProgramError {
    #[error("project asset index at {root} is unavailable: {message}")]
    IndexUnavailable { root: PathBuf, message: String },
    #[error("built-in material program {reference:?} is not owned by the project index")]
    BuiltInNotIndexed { reference: MaterialProgramRef },
    #[error("material program {reference:?} is missing from the project index")]
    Missing { reference: MaterialProgramRef },
    #[error("material program {reference:?} is declared by multiple sources: {sources:?}")]
    Duplicate {
        reference: MaterialProgramRef,
        sources: Vec<PathBuf>,
    },
    #[error("material program {reference:?} at {path} is not resolvable")]
    Unresolvable {
        reference: MaterialProgramRef,
        path: PathBuf,
    },
    #[error("material program {reference:?} changed after indexing at {path}: {message}")]
    SourceChanged {
        reference: MaterialProgramRef,
        path: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveMaterialFunctionError {
    #[error("project asset index at {root} is unavailable: {message}")]
    IndexUnavailable { root: PathBuf, message: String },
    #[error("built-in material function {reference:?} is not owned by the project index")]
    BuiltInNotIndexed { reference: MaterialFunctionRef },
    #[error("material function {reference:?} is missing from the project index")]
    Missing { reference: MaterialFunctionRef },
    #[error("material function {reference:?} is declared by multiple sources: {sources:?}")]
    Duplicate {
        reference: MaterialFunctionRef,
        sources: Vec<PathBuf>,
    },
    #[error("material function {reference:?} at {path} is not resolvable")]
    Unresolvable {
        reference: MaterialFunctionRef,
        path: PathBuf,
    },
    #[error("material function {reference:?} changed after indexing at {path}: {message}")]
    SourceChanged {
        reference: MaterialFunctionRef,
        path: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResolveMaterialPresetError {
    #[error("project asset index at {root} is unavailable: {message}")]
    IndexUnavailable { root: PathBuf, message: String },
    #[error("material preset {preset} is missing from the project index")]
    Missing { preset: MaterialPresetId },
    #[error("material preset {preset} is declared by multiple sources: {sources:?}")]
    Duplicate {
        preset: MaterialPresetId,
        sources: Vec<PathBuf>,
    },
    #[error("material preset {preset} at {path} is not resolvable")]
    Unresolvable {
        preset: MaterialPresetId,
        path: PathBuf,
    },
    #[error("material preset {preset} changed after indexing at {path}: {message}")]
    SourceChanged {
        preset: MaterialPresetId,
        path: PathBuf,
        message: String,
    },
}

/// A root effect plus every unique reusable effect it transitively references.
#[derive(Debug, Clone)]
pub struct ResolvedEffectProject {
    pub root: EffectAsset,
    pub dependencies: BTreeMap<EffectId, EffectAsset>,
    pub material_programs: BTreeMap<MaterialProgramId, MaterialProgram>,
    pub material_functions: BTreeMap<MaterialFunctionId, MaterialFunction>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectMaterialDependencyDiagnosticCode {
    Missing,
    Duplicate,
    Unresolvable,
    IndexUnavailable,
    SourceChanged,
    InvalidInstance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMaterialDependencyDiagnostic {
    pub code: ProjectMaterialDependencyDiagnosticCode,
    pub owner: EffectId,
    pub material: MaterialId,
    pub reference: MaterialProgramRef,
    pub path: String,
    pub message: String,
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
#[error(
    "project dependency resolution failed with {count} diagnostic(s)",
    count = .diagnostics.len() + .material_diagnostics.len()
)]
pub struct ProjectDependencyReport {
    pub diagnostics: Vec<ProjectDependencyDiagnostic>,
    pub material_diagnostics: Vec<ProjectMaterialDependencyDiagnostic>,
}

struct DependencyResolver<'a> {
    index: &'a ProjectAssetIndex,
    resolved: BTreeMap<EffectId, EffectAsset>,
    material_programs: BTreeMap<MaterialProgramId, MaterialProgram>,
    visiting: Vec<EffectId>,
    visited: BTreeSet<EffectId>,
    diagnostics: Vec<ProjectDependencyDiagnostic>,
    material_diagnostics: Vec<ProjectMaterialDependencyDiagnostic>,
}

impl DependencyResolver<'_> {
    fn visit(&mut self, effect: &EffectAsset) {
        if self.visited.contains(&effect.id) {
            return;
        }
        self.visiting.push(effect.id);
        for instance in &effect.material_instances {
            let reference = instance.program;
            let MaterialProgramRef::Project(program_id) = reference else {
                continue;
            };
            let program = if let Some(program) = self.material_programs.get(&program_id) {
                program.clone()
            } else {
                match self.index.load_material_program(reference) {
                    Ok(program) => {
                        self.material_programs.insert(program.id, program.clone());
                        program
                    }
                    Err(error) => {
                        self.material_diagnostics
                            .push(ProjectMaterialDependencyDiagnostic {
                                code: material_dependency_code(&error),
                                owner: effect.id,
                                material: instance.id,
                                reference,
                                path: "program".into(),
                                message: error.to_string(),
                            });
                        continue;
                    }
                }
            };
            for diagnostic in instance.validate_against(&program).diagnostics {
                if diagnostic.severity != aestra_core::DiagnosticSeverity::Error {
                    continue;
                }
                self.material_diagnostics
                    .push(ProjectMaterialDependencyDiagnostic {
                        code: ProjectMaterialDependencyDiagnosticCode::InvalidInstance,
                        owner: effect.id,
                        material: instance.id,
                        reference,
                        path: diagnostic.path,
                        message: diagnostic.message,
                    });
            }
        }
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
            if !child.playback_mode.is_looping()
                && clip.source_offset + clip.duration > child.duration
            {
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

fn material_dependency_code(
    error: &ResolveMaterialProgramError,
) -> ProjectMaterialDependencyDiagnosticCode {
    match error {
        ResolveMaterialProgramError::IndexUnavailable { .. } => {
            ProjectMaterialDependencyDiagnosticCode::IndexUnavailable
        }
        ResolveMaterialProgramError::BuiltInNotIndexed { .. }
        | ResolveMaterialProgramError::Missing { .. } => {
            ProjectMaterialDependencyDiagnosticCode::Missing
        }
        ResolveMaterialProgramError::Duplicate { .. } => {
            ProjectMaterialDependencyDiagnosticCode::Duplicate
        }
        ResolveMaterialProgramError::Unresolvable { .. } => {
            ProjectMaterialDependencyDiagnosticCode::Unresolvable
        }
        ResolveMaterialProgramError::SourceChanged { .. } => {
            ProjectMaterialDependencyDiagnosticCode::SourceChanged
        }
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
            Ok(kind) if kind.is_dir() && path.file_name().is_some_and(|name| name == ".aestra") => {
            }
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

fn is_material_program_source(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_lowercase().ends_with(".aestra.material.ron"))
}

fn is_material_function_source(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.to_lowercase()
                .ends_with(".aestra.material-function.ron")
        })
}

fn is_material_preset_source(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.to_lowercase().ends_with(".aestra.material-preset.ron"))
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

fn index_material_program_source(
    root: &Path,
    path: PathBuf,
    diagnostics: &mut Vec<ProjectAssetDiagnostic>,
) -> ProjectMaterialProgramEntry {
    let id = source_id(root, &path);
    let fallback_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unnamed material")
        .trim_end_matches(".ron")
        .trim_end_matches(".material")
        .trim_end_matches(".aestra")
        .replace(['_', '-'], " ");
    match MaterialProgram::load_ron(&path) {
        Ok(program) => ProjectMaterialProgramEntry {
            id,
            reference: Some(MaterialProgramRef::Project(program.id)),
            display_name: program.name,
            path,
            status: ProjectMaterialProgramStatus::Valid,
        },
        Err(error @ MaterialProgramError::Validation(_))
        | Err(error @ MaterialProgramError::Parse(_))
        | Err(error @ MaterialProgramError::Io(_))
        | Err(error @ MaterialProgramError::Serialize(_)) => {
            let message = error.to_string();
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::InvalidAsset,
                path: Some(path.clone()),
                message: message.clone(),
            });
            ProjectMaterialProgramEntry {
                id,
                reference: None,
                display_name: fallback_name,
                path,
                status: ProjectMaterialProgramStatus::Invalid { message },
            }
        }
    }
}

fn index_material_function_source(
    root: &Path,
    path: PathBuf,
    diagnostics: &mut Vec<ProjectAssetDiagnostic>,
) -> ProjectMaterialFunctionEntry {
    let id = source_id(root, &path);
    let fallback_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unnamed material function")
        .trim_end_matches(".ron")
        .trim_end_matches(".material-function")
        .trim_end_matches(".aestra")
        .replace(['_', '-'], " ");
    match MaterialFunction::load_ron(&path) {
        Ok(function) => ProjectMaterialFunctionEntry {
            id,
            reference: Some(MaterialFunctionRef::Project(function.id)),
            display_name: function.name,
            path,
            status: ProjectMaterialFunctionStatus::Valid,
        },
        Err(error @ MaterialFunctionError::Validation(_))
        | Err(error @ MaterialFunctionError::Parse(_))
        | Err(error @ MaterialFunctionError::Io(_))
        | Err(error @ MaterialFunctionError::Serialize(_)) => {
            let message = error.to_string();
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::InvalidAsset,
                path: Some(path.clone()),
                message: message.clone(),
            });
            ProjectMaterialFunctionEntry {
                id,
                reference: None,
                display_name: fallback_name,
                path,
                status: ProjectMaterialFunctionStatus::Invalid { message },
            }
        }
    }
}

fn index_material_preset_source(
    root: &Path,
    path: PathBuf,
    diagnostics: &mut Vec<ProjectAssetDiagnostic>,
) -> ProjectMaterialPresetEntry {
    let id = source_id(root, &path);
    let fallback_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unnamed material preset")
        .trim_end_matches(".ron")
        .trim_end_matches(".material-preset")
        .trim_end_matches(".aestra")
        .replace(['_', '-'], " ");
    match MaterialPresetDescriptor::load_ron(&path) {
        Ok(preset) => ProjectMaterialPresetEntry {
            id,
            preset: Some(preset.id),
            display_name: preset.display_name,
            path,
            status: ProjectMaterialPresetStatus::Valid,
        },
        Err(MaterialPresetError::UnsupportedFormat { found, current }) => {
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::UnsupportedFormat,
                path: Some(path.clone()),
                message: format!(
                    "material preset format version {found} is unsupported; expected {current}"
                ),
            });
            ProjectMaterialPresetEntry {
                id,
                preset: None,
                display_name: fallback_name,
                path,
                status: ProjectMaterialPresetStatus::Unsupported { found, current },
            }
        }
        Err(error) => {
            let message = error.to_string();
            diagnostics.push(ProjectAssetDiagnostic {
                code: ProjectAssetDiagnosticCode::InvalidAsset,
                path: Some(path.clone()),
                message: message.clone(),
            });
            ProjectMaterialPresetEntry {
                id,
                preset: None,
                display_name: fallback_name,
                path,
                status: ProjectMaterialPresetStatus::Invalid { message },
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
