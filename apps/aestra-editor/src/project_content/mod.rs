//! Editor adapter for one coherent project snapshot and unsaved shared-source drafts.
pub(crate) mod io;
mod plugin;
pub(crate) use plugin::{EditorProjectContentPlugin, ProjectContentSet};
mod refresh;
use crate::*;
use aestra_compiler::{
    EffectCompiler, MaterialFunctionLibrary, MaterialPresetCatalog, ProjectCompileError,
};
use aestra_core::material::{MaterialFunction, MaterialProgram};
use aestra_core::{Diagnostic, EffectAssetRef, EffectClipId, ValidationReport};
use aestra_project::*;
use aestra_runtime::CompiledEffectProject;
#[cfg(test)]
pub(crate) use refresh::apply_project_effect_catalog_refresh;
pub(crate) use refresh::{ProjectEffectWatchState, poll_project_effect_catalog};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
#[derive(Resource, Clone)]
pub(crate) struct EditorProjectContent {
    snapshot: ProjectContentSnapshot,
    version: ProjectContentVersion,
    prepared: Option<PreparedProject>,
    #[cfg(test)]
    test_index: Option<ProjectAssetIndex>,
    effect_root: PathBuf,
    pub(crate) material_drafts: crate::material_drafts::MaterialDrafts,
}

impl Default for EditorProjectContent {
    fn default() -> Self {
        // The editor opens the curated sample project at the repo root by default. Editor UI assets
        // live under `assets/`; workspace test fixtures live under `assets/test/`.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
        let root = root.canonicalize().unwrap_or(root);
        Self::scan_project(&root, root.join("effects"))
    }
}

impl EditorProjectContent {
    pub(crate) fn try_scan_project(
        project_root: impl AsRef<Path>,
        effect_root: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let catalog = Self::scan_project(project_root, effect_root);
        if let ProjectAssetIndexAvailability::Unavailable { message, .. } =
            catalog.index().availability()
        {
            return Err(message.clone());
        }
        Ok(catalog)
    }
    #[cfg(test)]
    pub(crate) fn scan(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self::scan_project(root, root)
    }

    pub(crate) fn scan_project(
        project_root: impl AsRef<Path>,
        effect_root: impl AsRef<Path>,
    ) -> Self {
        Self {
            snapshot: ProjectContentSnapshot::scan(project_root.as_ref()),
            version: next_version(),
            prepared: None,
            #[cfg(test)]
            test_index: None,
            effect_root: effect_root.as_ref().to_owned(),
            material_drafts: default(),
        }
    }

    pub(crate) fn entries(&self) -> &[ProjectEffectEntry] {
        self.index().effects()
    }

    pub(crate) fn root(&self) -> &Path {
        self.index().root()
    }

    pub(crate) fn effect_root(&self) -> &Path {
        &self.effect_root
    }

    pub(crate) fn refresh(&mut self) {
        self.snapshot = ProjectContentSnapshot::scan(self.root());
        self.version.revision = self.version.revision.wrapping_add(1);
        self.prepared = None;
    }

    pub(crate) fn create_effect_source(
        &mut self,
        effect: &EffectAsset,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        if self.effect_root == self.index().root() {
            return self.edit_index(|index| index.create_effect_source(effect));
        }
        ProjectAssetIndex::scan(&self.effect_root).create_effect_source(effect)?;
        let reference = EffectAssetRef::new(effect.id);
        self.refresh();
        self.index().resolve(reference).cloned().map_err(|error| {
            ProjectAssetOperationError::Refresh {
                reference,
                message: error.to_string(),
            }
        })
    }

    pub(crate) fn entry(&self, id: ProjectEffectEntryId) -> Option<&ProjectEffectEntry> {
        self.index().entry(id)
    }

    pub(crate) fn openable_path(&self, reference: EffectAssetRef) -> Option<&Path> {
        self.index()
            .resolve(reference)
            .ok()
            .map(|entry| entry.path.as_path())
    }

    pub(crate) fn effect_for_placement(
        &self,
        owner: &EffectAsset,
        reference: EffectAssetRef,
    ) -> Result<EffectAsset, String> {
        if reference.id == owner.id {
            return Err("an effect cannot reference itself".into());
        }
        let source = self.cached_effect(reference)?;
        let project = self
            .resolve_project(&source)
            .map_err(|error| error.to_string())?;
        if project.effect(owner.id).is_some() {
            return Err("placing this effect would create a reference cycle".into());
        }
        Ok(source)
    }

    pub(crate) fn load_effect(&self, reference: EffectAssetRef) -> Result<EffectAsset, String> {
        self.index()
            .load_effect(reference)
            .map_err(|error| error.to_string())
    }

    /// Presentation/authoring query at the published content revision, without disk I/O.
    pub(crate) fn cached_effect(&self, reference: EffectAssetRef) -> Result<EffectAsset, String> {
        self.snapshot
            .content
            .cached_effect(reference)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn material_programs_for_effect(
        &self,
        effect: &EffectAsset,
    ) -> Result<Vec<aestra_core::material::MaterialProgram>, String> {
        let mut programs = Vec::new();
        for instance in &effect.material_instances {
            if programs
                .iter()
                .any(|program: &aestra_core::material::MaterialProgram| {
                    program.id == instance.program.id()
                })
            {
                continue;
            }
            if matches!(
                instance.program,
                aestra_core::material::MaterialProgramRef::BuiltIn(_)
            ) {
                if let Some(program) = MaterialProgram::built_in(instance.program) {
                    programs.push(program);
                }
                continue;
            }
            programs.push(
                match self
                    .material_drafts
                    .programs
                    .get(&instance.program.id())
                    .and_then(|draft| draft.current.clone())
                {
                    Some(program) => program,
                    None => self
                        .snapshot
                        .content
                        .cached_material_program(instance.program)
                        .map_err(|error| error.to_string())?,
                },
            );
        }
        Ok(programs)
    }

    /// Published shared source with its working draft, without an effect prerequisite.
    pub(crate) fn material_program(
        &self,
        id: aestra_core::MaterialProgramId,
    ) -> Result<MaterialProgram, String> {
        let saved = self
            .content()
            .cached_material_program(aestra_core::material::MaterialProgramRef::Project(id))
            .map_err(|error| error.to_string())?;
        match self.material_drafts.programs.get(&id) {
            Some(draft) => draft
                .current
                .clone()
                .ok_or_else(|| format!("Material program {id} is deleted")),
            None => Ok(saved),
        }
    }

    pub(crate) fn material_functions(&self) -> Result<Vec<MaterialFunction>, String> {
        let mut functions = self
            .snapshot
            .content
            .cached_material_functions()
            .map_err(|error| error.to_string())?;
        for (id, draft) in &self.material_drafts.functions {
            if let Some(function) = &draft.current {
                functions.insert(*id, function.clone());
            } else {
                functions.remove(id);
            }
        }
        Ok(functions.into_values().collect())
    }

    pub(crate) fn material_function_library(&self) -> Result<MaterialFunctionLibrary, String> {
        self.material_functions().map(MaterialFunctionLibrary::new)
    }

    /// Whether a shared material program is definitively absent from the project index — its source
    /// was moved or deleted — as opposed to present or merely ambiguous. Workspace restore uses this
    /// to safely drop editor tabs whose asset is gone, while leaving ambiguous (recoverable) ones.
    pub(crate) fn material_program_missing(&self, id: aestra_core::MaterialProgramId) -> bool {
        matches!(
            self.content()
                .cached_material_program(aestra_core::material::MaterialProgramRef::Project(id)),
            Err(ResolveMaterialProgramError::Missing { .. })
        )
    }

    /// Function counterpart to [`Self::material_program_missing`].
    pub(crate) fn material_function_missing(&self, id: aestra_core::MaterialFunctionId) -> bool {
        matches!(
            self.content()
                .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(id)),
            Err(ResolveMaterialFunctionError::Missing { .. })
        )
    }

    pub(crate) fn material_preset_catalog(&self) -> Result<MaterialPresetCatalog, String> {
        let presets = self
            .snapshot
            .content
            .cached_material_presets()
            .map_err(|error| error.to_string())?;
        MaterialPresetCatalog::with_project_presets(presets.into_values())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn replace_material_function(
        &mut self,
        expected: &MaterialFunction,
        replacement: &MaterialFunction,
    ) -> Result<(), String> {
        self.material_drafts.replace_function(
            self.snapshot.content.asset_index(),
            expected,
            replacement,
        )
    }

    pub(crate) fn create_material_function(
        &mut self,
        function: &MaterialFunction,
    ) -> Result<(), String> {
        self.material_drafts
            .create_function(self.snapshot.content.asset_index(), function)
    }

    pub(crate) fn delete_material_function(
        &mut self,
        function: &MaterialFunction,
    ) -> Result<(), String> {
        self.material_drafts
            .delete_function(self.snapshot.content.asset_index(), function)
    }

    pub(crate) fn next_material_function_name(&self, base: &str) -> String {
        let functions = self.material_functions().unwrap_or_default();
        let names = functions
            .iter()
            .map(|function| function.name.as_str())
            .collect::<BTreeSet<_>>();
        if !names.contains(base) {
            return base.to_owned();
        }
        (2..)
            .map(|suffix| format!("{base} {suffix}"))
            .find(|candidate| !names.contains(candidate.as_str()))
            .expect("the finite project index cannot exhaust function names")
    }

    pub(crate) fn replace_material_program(
        &mut self,
        expected: &MaterialProgram,
        replacement: &MaterialProgram,
    ) -> Result<(), String> {
        self.material_drafts.replace_program(
            self.snapshot.content.asset_index(),
            expected,
            replacement,
        )
    }

    fn resolve_project(
        &self,
        root: &EffectAsset,
    ) -> Result<aestra_project::ResolvedEffectProject, aestra_project::ProjectDependencyReport>
    {
        let overrides = self
            .material_drafts
            .programs
            .iter()
            .filter_map(|(id, draft)| draft.current.clone().map(|program| (*id, program)))
            .collect();
        let mut resolved = self
            .snapshot
            .content
            .cached_effect_project_with_materials(root, overrides)?;
        for (id, draft) in &self.material_drafts.functions {
            if let Some(function) = &draft.current {
                resolved.material_functions.insert(*id, function.clone());
            } else {
                resolved.material_functions.remove(id);
            }
        }
        Ok(resolved)
    }

    pub(crate) fn save_material_drafts(&mut self) -> Result<(), String> {
        let result = self.material_drafts.save();
        self.refresh();
        result
    }

    pub(crate) fn compile_project(
        &self,
        root: &EffectAsset,
    ) -> Result<CompiledEffectProject, String> {
        if let Some(prepared) = &self.prepared
            && prepared.effect == *root
            && prepared.drafts == self.material_drafts
        {
            return prepared.compiled.clone();
        }
        self.resolve_project(root)
            .map_err(ProjectCompileError::Dependencies)
            .and_then(|resolved| EffectCompiler::default().compile_resolved_project(&resolved))
            .map_err(|error| match error {
                ProjectCompileError::Dependencies(report) => report
                    .diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .chain(
                        report
                            .material_diagnostics
                            .into_iter()
                            .map(|diagnostic| diagnostic.message),
                    )
                    .collect::<Vec<_>>()
                    .join("; "),
                error => error.to_string(),
            })
    }

    pub(crate) fn dependency_validation_report(&self, effect: &EffectAsset) -> ValidationReport {
        let mut validation = ValidationReport::default();
        let Err(report) = self.resolve_project(effect) else {
            return validation;
        };
        for diagnostic in report
            .diagnostics
            .into_iter()
            .filter(|diagnostic| diagnostic.owner == effect.id)
        {
            let Some(index) = effect
                .effect_clips
                .iter()
                .position(|clip| clip.id == diagnostic.clip)
            else {
                continue;
            };
            let code = match diagnostic.code {
                ProjectDependencyDiagnosticCode::InvalidTiming => DiagnosticCode::InvalidTiming,
                ProjectDependencyDiagnosticCode::Cycle => DiagnosticCode::ReferenceCycle,
                ProjectDependencyDiagnosticCode::Missing
                | ProjectDependencyDiagnosticCode::Duplicate
                | ProjectDependencyDiagnosticCode::Unresolvable
                | ProjectDependencyDiagnosticCode::IndexUnavailable
                | ProjectDependencyDiagnosticCode::SourceChanged => {
                    DiagnosticCode::InvalidReference
                }
            };
            validation.push(Diagnostic::error(
                code,
                format!("effect.effect_clips[{index}].source"),
                diagnostic.message,
            ));
        }
        for diagnostic in report.material_diagnostics {
            validation.push(Diagnostic::error(
                DiagnosticCode::InvalidReference,
                diagnostic.path,
                diagnostic.message,
            ));
        }
        validation
    }

    pub(crate) fn effect_clip_dependency_error(
        &self,
        effect: &EffectAsset,
        clip: EffectClipId,
    ) -> Option<String> {
        self.dependency_validation_report(effect)
            .diagnostics
            .into_iter()
            .find(|diagnostic| {
                effect
                    .effect_clips
                    .iter()
                    .position(|candidate| candidate.id == clip)
                    .is_some_and(|index| {
                        diagnostic.path == format!("effect.effect_clips[{index}].source")
                    })
            })
            .map(|diagnostic| diagnostic.message)
    }

    pub(crate) fn availability(&self) -> &ProjectAssetIndexAvailability {
        self.index().availability()
    }

    pub(crate) fn rename_effect_source(
        &mut self,
        source: ProjectEffectEntryId,
        name: &str,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        self.edit_index(|index| index.rename_effect_source(source, name))
    }

    pub(crate) fn move_effect_source(
        &mut self,
        source: ProjectEffectEntryId,
        destination: &Path,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        if !destination.is_dir() {
            return Err(ProjectAssetOperationError::InvalidDestination {
                path: destination.to_owned(),
            });
        }
        let effect_root = fs::canonicalize(&self.effect_root).map_err(|error| {
            ProjectAssetOperationError::FileSystem {
                operation: "resolve project effect root",
                path: self.effect_root.clone(),
                message: error.to_string(),
            }
        })?;
        let destination = fs::canonicalize(destination).map_err(|error| {
            ProjectAssetOperationError::FileSystem {
                operation: "resolve destination",
                path: destination.to_owned(),
                message: error.to_string(),
            }
        })?;
        if !destination.starts_with(&effect_root) {
            return Err(ProjectAssetOperationError::DestinationOutsideRoot {
                destination,
                root: effect_root,
            });
        }
        self.edit_index(|index| index.move_effect_source(source, destination))
    }

    pub(crate) fn effect_usage_graph(
        &self,
        reference: EffectAssetRef,
    ) -> Result<ProjectEffectUsageGraph, String> {
        self.index()
            .effect_usage_graph(reference)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn cached_effect_usage_graph(
        &self,
        reference: EffectAssetRef,
    ) -> Result<ProjectEffectUsageGraph, String> {
        self.snapshot
            .content
            .cached_effect_usage_graph(reference)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn delete_effect_source(
        &mut self,
        source: ProjectEffectEntryId,
    ) -> Result<ProjectEffectEntry, ProjectAssetOperationError> {
        self.edit_index(|index| {
            index.delete_effect_source(source, ProjectEffectDeletePolicy::AllowReferenced)
        })
    }

    pub(crate) fn effect_name(&self, reference: EffectAssetRef) -> String {
        self.index().resolve(reference).map_or_else(
            |_| reference.to_string(),
            |entry| entry.display_name.clone(),
        )
    }

    #[cfg(test)]
    // Metadata-only UI fixtures. Semantic query tests should scan a temporary project so
    // the source tree, index availability and parsed documents agree as they do in production.
    pub(crate) fn from_entries(entries: Vec<ProjectEffectEntry>) -> Self {
        Self {
            snapshot: ProjectContentSnapshot::scan(Path::new("virtual")),
            version: next_version(),
            prepared: None,
            test_index: Some(ProjectAssetIndex::from_entries("virtual", entries)),
            effect_root: PathBuf::from("virtual"),
            material_drafts: default(),
        }
    }
}

#[derive(Clone)]
struct PreparedProject {
    effect: EffectAsset,
    drafts: crate::material_drafts::MaterialDrafts,
    compiled: Result<CompiledEffectProject, String>,
}

fn next_version() -> ProjectContentVersion {
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    ProjectContentVersion {
        generation: GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        revision: 0,
    }
}

impl EditorProjectContent {
    fn index(&self) -> &ProjectAssetIndex {
        #[cfg(test)]
        if let Some(index) = &self.test_index {
            return index;
        }
        self.snapshot.content.asset_index()
    }

    fn edit_index<T>(&mut self, edit: impl FnOnce(&mut ProjectAssetIndex) -> T) -> T {
        // Existing explicit file operations retain their audited preflight behavior. Their
        // temporary index is not published; the coherent tree/index is replaced afterwards.
        let mut index = self.index().clone();
        let result = edit(&mut index);
        self.refresh();
        result
    }

    pub(crate) fn content_revision(&self) -> ProjectContentVersion {
        self.version
    }

    /// Published discovery and semantic index; browsing must never scan the filesystem.
    pub(crate) fn content(&self) -> &aestra_project::ProjectContent {
        &self.snapshot.content
    }

    pub(crate) fn prepare_preview(&mut self, effect: &EffectAsset) -> Result<(), String> {
        let compiled = self.compile_project(effect);
        self.prepared = Some(PreparedProject {
            effect: effect.clone(),
            drafts: self.material_drafts.clone(),
            compiled: compiled.clone(),
        });
        compiled.map(|_| ())
    }

    pub(crate) fn snapshot_is_current(&self) -> bool {
        ProjectTreeStamp::scan(self.root()) == self.snapshot.stamp
    }
}
