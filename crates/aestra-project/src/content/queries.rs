//! Shared dependency traversal with explicit snapshot versus disk read policies.
//! ProjectContent queries never access the filesystem. ProjectAssetIndex loaders keep their
//! latest-disk policy, including for mutation preflight.
use super::{ProjectContent, ProjectSourceDocument};
use crate::*;

pub(crate) trait ProjectRead {
    fn index(&self) -> &ProjectAssetIndex;
    fn effect(&self, reference: EffectAssetRef) -> Result<EffectAsset, ResolveEffectError>;
    fn material_program(
        &self,
        reference: MaterialProgramRef,
    ) -> Result<MaterialProgram, ResolveMaterialProgramError>;
    fn material_function(
        &self,
        reference: MaterialFunctionRef,
    ) -> Result<MaterialFunction, ResolveMaterialFunctionError>;
}

impl ProjectRead for ProjectAssetIndex {
    fn index(&self) -> &ProjectAssetIndex {
        self
    }
    fn effect(&self, reference: EffectAssetRef) -> Result<EffectAsset, ResolveEffectError> {
        self.load_effect(reference)
    }
    fn material_program(
        &self,
        reference: MaterialProgramRef,
    ) -> Result<MaterialProgram, ResolveMaterialProgramError> {
        self.load_material_program(reference)
    }
    fn material_function(
        &self,
        reference: MaterialFunctionRef,
    ) -> Result<MaterialFunction, ResolveMaterialFunctionError> {
        self.load_material_function(reference)
    }
}

impl ProjectRead for ProjectContent {
    fn index(&self) -> &ProjectAssetIndex {
        self.asset_index()
    }
    fn effect(&self, reference: EffectAssetRef) -> Result<EffectAsset, ResolveEffectError> {
        self.cached_effect(reference)
    }
    fn material_program(
        &self,
        reference: MaterialProgramRef,
    ) -> Result<MaterialProgram, ResolveMaterialProgramError> {
        self.cached_material_program(reference)
    }
    fn material_function(
        &self,
        reference: MaterialFunctionRef,
    ) -> Result<MaterialFunction, ResolveMaterialFunctionError> {
        self.cached_material_function(reference)
    }
}

impl ProjectContent {
    /// Searches authored names and preset metadata in this snapshot without I/O or document clones.
    /// The caller supplies a lowercase query, as with source-name filtering.
    pub fn source_metadata_matches(&self, source: ProjectSourceId, query: &str) -> bool {
        let contains = |text: &str| text.to_lowercase().contains(query);
        match self.documents.get(&source) {
            Some(ProjectSourceDocument::Effect(document)) => contains(&document.name),
            Some(ProjectSourceDocument::MaterialProgram(document)) => contains(&document.name),
            Some(ProjectSourceDocument::MaterialFunction(document)) => contains(&document.name),
            Some(ProjectSourceDocument::MaterialPreset(document)) => {
                contains(&document.display_name)
                    || contains(&document.description)
                    || contains(document.category.display_name())
                    || document.tags.iter().any(|tag| contains(tag))
            }
            _ => false,
        }
    }

    /// Reads the uniquely resolved document captured by this snapshot, never the latest disk bytes.
    pub fn cached_effect(
        &self,
        reference: EffectAssetRef,
    ) -> Result<EffectAsset, ResolveEffectError> {
        let entry = self.asset_index.resolve(reference)?;
        let Some(ProjectSourceDocument::Effect(document)) = self.documents.get(&entry.id) else {
            unreachable!("a resolvable source is indexed from the same parsed document");
        };
        Ok(document.as_ref().clone())
    }

    /// Reads the uniquely resolved document captured by this snapshot, never the latest disk bytes.
    pub fn cached_material_program(
        &self,
        reference: MaterialProgramRef,
    ) -> Result<MaterialProgram, ResolveMaterialProgramError> {
        if let Some(program) = MaterialProgram::built_in(reference) {
            return Ok(program);
        }
        let entry = self.asset_index.resolve_material_program(reference)?;
        let Some(ProjectSourceDocument::MaterialProgram(document)) = self.documents.get(&entry.id)
        else {
            unreachable!("a resolvable source is indexed from the same parsed document");
        };
        Ok(document.as_ref().clone())
    }

    /// Reads the uniquely resolved document captured by this snapshot, never the latest disk bytes.
    pub fn cached_material_function(
        &self,
        reference: MaterialFunctionRef,
    ) -> Result<MaterialFunction, ResolveMaterialFunctionError> {
        let entry = self.asset_index.resolve_material_function(reference)?;
        let Some(ProjectSourceDocument::MaterialFunction(document)) = self.documents.get(&entry.id)
        else {
            unreachable!("a resolvable source is indexed from the same parsed document");
        };
        Ok(document.as_ref().clone())
    }

    /// Reads the uniquely resolved document captured by this snapshot, never the latest disk bytes.
    pub fn cached_material_preset(
        &self,
        preset: MaterialPresetId,
    ) -> Result<MaterialPresetDescriptor, ResolveMaterialPresetError> {
        let entry = self.asset_index.resolve_material_preset(preset)?;
        let Some(ProjectSourceDocument::MaterialPreset(document)) = self.documents.get(&entry.id)
        else {
            unreachable!("a resolvable source is indexed from the same parsed document");
        };
        Ok(document.as_ref().clone())
    }

    pub fn cached_material_functions(
        &self,
    ) -> Result<BTreeMap<MaterialFunctionId, MaterialFunction>, ResolveMaterialFunctionError> {
        self.asset_index
            .material_functions()
            .iter()
            .filter_map(|entry| entry.reference)
            .map(|reference| {
                self.cached_material_function(reference)
                    .map(|function| (function.id, function))
            })
            .collect()
    }

    pub fn cached_material_presets(
        &self,
    ) -> Result<BTreeMap<MaterialPresetId, MaterialPresetDescriptor>, ResolveMaterialPresetError>
    {
        self.asset_index
            .material_presets()
            .iter()
            .filter_map(|entry| entry.preset)
            .map(|id| self.cached_material_preset(id).map(|preset| (id, preset)))
            .collect()
    }

    /// Resolves nested effects and materials from this snapshot, allowing an unsaved root/programs.
    /// This is for presentation/compilation, not authorizing source changes or deletion.
    pub fn cached_effect_project_with_materials(
        &self,
        root: &EffectAsset,
        programs: BTreeMap<MaterialProgramId, MaterialProgram>,
    ) -> Result<ResolvedEffectProject, ProjectDependencyReport> {
        resolve_project(self, root, programs)
    }

    /// Point-in-time relations for inspection. Mutations must use the index's disk-validating API.
    pub fn cached_effect_usage_graph(
        &self,
        reference: EffectAssetRef,
    ) -> Result<ProjectEffectUsageGraph, ResolveEffectError> {
        self.asset_index.resolve(reference)?;
        let edges = effect_relation_edges(self)?;
        Ok(ProjectEffectUsageGraph {
            dependencies: traverse_effect_relations(reference, &edges, false),
            usages: traverse_effect_relations(reference, &edges, true),
        })
    }
}

pub(crate) fn resolve_project(
    reader: &dyn ProjectRead,
    root: &EffectAsset,
    programs: BTreeMap<MaterialProgramId, MaterialProgram>,
) -> Result<ResolvedEffectProject, ProjectDependencyReport> {
    let mut resolver = DependencyResolver {
        reader,
        resolved: BTreeMap::new(),
        material_programs: programs,
        visiting: Vec::new(),
        visited: BTreeSet::new(),
        diagnostics: Vec::new(),
        material_diagnostics: Vec::new(),
    };
    resolver.visit(root);
    if resolver.diagnostics.is_empty() && resolver.material_diagnostics.is_empty() {
        let material_functions = reader
            .index()
            .material_functions
            .iter()
            .filter_map(|entry| entry.reference)
            .filter_map(|reference| reader.material_function(reference).ok())
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

pub(crate) fn effect_relation_edges(
    reader: &dyn ProjectRead,
) -> Result<Vec<ProjectEffectRelation>, ResolveEffectError> {
    let mut edges = Vec::new();
    for entry in reader
        .index()
        .effects
        .iter()
        .filter(|entry| entry.status.is_resolvable())
    {
        let Some(reference) = entry.reference else {
            continue;
        };
        let effect = reader.effect(reference)?;
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
