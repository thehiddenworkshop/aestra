//! Explicit mutation preflight. Document edits and filesystem operations are separate histories.
mod duplicate;
mod move_journal;
mod rename;
mod transaction;
use super::{ProjectContent, ProjectSourceKind, ProjectSourceTree};
use super::{ProjectRelationStatus, ProjectSourceDocument, ProjectSourceRelation};
use crate::ProjectSourceId;
pub use duplicate::{DuplicatePlan, DuplicateResult};
pub use rename::{RenamePlan, RenameResult};
pub use transaction::{
    AssetMoveBatchPlan, AssetMoveBatchResult, DeletePlan, DeletedSource, PendingAssetMoveBatch,
    SourceRelocation,
};

/// Host-supplied current documents, replacing saved references rather than adding stale ones.
#[derive(Debug, Clone)]
pub enum DraftDocument {
    Effect(Box<aestra_core::EffectAsset>),
    Program(Box<aestra_core::material::MaterialProgram>),
    Function(Box<aestra_core::material::MaterialFunction>),
}

#[derive(Debug, Default)]
pub struct ReferencePreflight {
    pub affected_sources: Vec<ProjectSourceId>,
    pub usages: Vec<ProjectSourceRelation>,
    /// Any reason here blocks authorization. Empty is not authorization to mutate disk.
    pub incomplete: Vec<String>,
}

impl ProjectContent {
    /// Snapshot-only Move/Rename affordance. Empty suffix means a directory; compound
    /// semantic suffixes and resource extensions are preserved. This is not authority
    /// to mutate: `plan_content_relocations` must validate bytes, references and drafts.
    pub fn source_relocation_suffix(&self, source: ProjectSourceId) -> Option<String> {
        let entry = self.source(source)?;
        if entry.relative_path.as_os_str().is_empty()
            || entry.error.is_some()
            || entry
                .metadata
                .as_ref()
                .is_some_and(|metadata| metadata.readonly)
        {
            return None;
        }
        if entry.kind == ProjectSourceKind::Directory {
            return Some(String::new());
        }
        if !matches!(entry.kind, ProjectSourceKind::File(_)) {
            return None;
        }
        if let Some(suffix) = self.asset_operation_suffix(source) {
            return Some(suffix.into());
        }
        if matches!(
            self.documents.get(&source),
            Some(ProjectSourceDocument::MaterialPreset(_))
        ) {
            return Some(".aestra.material-preset.ron".into());
        }
        if !rename::resource_relocation_format(&entry.path) {
            return None;
        }
        entry
            .path
            .extension()?
            .to_str()
            .map(|extension| format!(".{extension}"))
    }

    /// One capability/format boundary for filename operations. UI code should not
    /// duplicate the supported-type list. A suffix is not authorization: planners
    /// still check identity, drafts, references and the current filesystem.
    pub fn asset_operation_suffix(&self, source: ProjectSourceId) -> Option<&'static str> {
        match self.documents.get(&source)? {
            ProjectSourceDocument::Effect(_) => {
                if self
                    .source(source)?
                    .name
                    .to_string_lossy()
                    .ends_with(".aestra.ron")
                {
                    Some(".aestra.ron")
                } else {
                    Some(".ron")
                }
            }
            ProjectSourceDocument::MaterialProgram(_) => Some(".aestra.material.ron"),
            ProjectSourceDocument::MaterialFunction(_) => Some(".aestra.material-function.ron"),
            ProjectSourceDocument::MaterialPreset(_) => None,
        }
    }

    /// Read-only inventory over this snapshot and explicit host drafts. The host must include
    /// every open/unsaved document and report false when its draft inventory is incomplete.
    /// Fresh disk/source-byte revalidation is still required by the eventual operation planner.
    pub fn reference_preflight(
        &self,
        source: ProjectSourceId,
        drafts: &[(ProjectSourceId, DraftDocument)],
        all_drafts_known: bool,
    ) -> ReferencePreflight {
        self.reference_preflight_inner(source, drafts, all_drafts_known, false, false)
    }

    fn reference_preflight_inner(
        &self,
        source: ProjectSourceId,
        drafts: &[(ProjectSourceId, DraftDocument)],
        all_drafts_known: bool,
        rename_only: bool,
        rewrite_paths: bool,
    ) -> ReferencePreflight {
        let mut report = ReferencePreflight::default();
        let Some(target) = self.source(source) else {
            report.incomplete.push("Source is no longer indexed".into());
            return report;
        };
        if !all_drafts_known {
            report
                .incomplete
                .push("Host draft inventory is incomplete".into());
        }
        let mut projected = self.clone();
        let mut seen = std::collections::BTreeSet::new();
        for (owner, draft) in drafts {
            let (asset, document) = match draft {
                DraftDocument::Effect(value) => (
                    crate::ProjectAssetId::Effect(value.id),
                    ProjectSourceDocument::Effect(value.clone()),
                ),
                DraftDocument::Program(value) => (
                    crate::ProjectAssetId::MaterialProgram(value.id),
                    ProjectSourceDocument::MaterialProgram(value.clone()),
                ),
                DraftDocument::Function(value) => (
                    crate::ProjectAssetId::MaterialFunction(value.id),
                    ProjectSourceDocument::MaterialFunction(value.clone()),
                ),
            };
            if !seen.insert(*owner) || self.asset_for_source(*owner) != Some(asset) {
                report
                    .incomplete
                    .push("Draft source is missing, duplicated or has a different identity".into());
                continue;
            }
            projected.documents.insert(*owner, document);
        }
        projected.relations = super::relations::RelationIndex::build(&projected.documents);
        for entry in self.source_tree().entries() {
            if entry.id == source
                || (target.kind == ProjectSourceKind::Directory
                    && entry.relative_path.starts_with(&target.relative_path))
            {
                report.affected_sources.push(entry.id);
                for usage in projected.source_relations(entry.id).usages {
                    if !report.usages.contains(&usage) {
                        report.usages.push(usage);
                    }
                }
            }
            if entry.kind == ProjectSourceKind::Directory && entry.error.is_none() {
                continue;
            }
            if rename_only && entry.error.is_none() && rename::non_referencing_asset(&entry.path) {
                continue;
            }
            if entry.error.is_some() || !projected.documents.contains_key(&entry.id) {
                report.incomplete.push(format!(
                    "References not fully known for {}",
                    entry.relative_path.display()
                ));
                continue;
            }
            if matches!(projected.documents.get(&entry.id), Some(ProjectSourceDocument::MaterialFunction(function))
                if function.custom_wesl.as_ref().is_some_and(|wesl|
                    !rename_only || !rename::non_referencing_shader(&wesl.source)))
            {
                report.incomplete.push(format!(
                    "Custom WESL include semantics are unknown for {}",
                    entry.relative_path.display()
                ));
            }
            let expressions = match projected.documents.get(&entry.id) {
                Some(ProjectSourceDocument::MaterialProgram(value)) => value.expressions.as_slice(),
                Some(ProjectSourceDocument::MaterialFunction(value)) => {
                    value.expressions.as_slice()
                }
                _ => &[],
            };
            // Calls use stable function IDs. For filename-only rename the function's
            // actual source is checked above, including any current draft overlay.
            if !rename_only
                && expressions.iter().any(|expression| {
                    matches!(
                        expression.kind,
                        aestra_core::material::MaterialExpressionKind::CustomWeslCall { .. }
                    )
                })
            {
                report.incomplete.push(format!(
                    "Custom WESL call semantics are unknown for {}",
                    entry.relative_path.display()
                ));
            }
            for dependency in projected.source_relations(entry.id).dependencies {
                if rename_only {
                    if let super::ProjectRelationTarget::File(path) = &dependency.target {
                        match rename::path_targets(
                            self.source_tree().root_path(),
                            &target.path,
                            path,
                        ) {
                            Ok(false) => {}
                            Ok(true) if rewrite_paths => {}
                            Ok(true) => report.incomplete.push(format!(
                                "{} references this filename ({}) and requires a path rewrite",
                                entry.relative_path.display(),
                                path.display()
                            )),
                            Err(reason) => report.incomplete.push(format!(
                                "Cannot resolve path in {}: {reason}",
                                entry.relative_path.display()
                            )),
                        }
                    }
                    // Typed IDs, including contextual texture IDs, do not change on rename.
                    continue;
                }
                if !matches!(
                    projected.relation_status(&dependency.target),
                    ProjectRelationStatus::Available | ProjectRelationStatus::BuiltIn
                ) {
                    report.incomplete.push(format!(
                        "Unresolved or contextual reference in {}",
                        entry.relative_path.display()
                    ));
                }
            }
        }
        if !self.asset_index().diagnostics().is_empty()
            || !self.source_tree().diagnostics().is_empty()
        {
            report
                .incomplete
                .push("Project discovery or parsing has diagnostics".into());
        }
        report
    }
}
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub enum OperationRequest {
    CreateFolder {
        parent: ProjectSourceId,
        name: String,
    },
    /// Filename only; never an authored display-name edit.
    Rename {
        source: ProjectSourceId,
        name: String,
    },
    Move {
        source: ProjectSourceId,
        parent: ProjectSourceId,
    },
    Duplicate {
        source: ProjectSourceId,
        parent: ProjectSourceId,
        name: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OperationError {
    #[error("{0}")]
    Blocked(String),
    #[error("Filesystem operation failed: {0}")]
    Io(String),
}
impl From<std::io::Error> for OperationError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Opaque plan: callers cannot substitute unchecked paths after preflight.
#[derive(Debug)]
pub struct OperationPlan {
    root: PathBuf,
    parent: PathBuf,
    destination: PathBuf,
}

#[derive(Debug)]
pub struct OperationResult {
    pub created_directory: PathBuf,
}

fn blocked(message: &str) -> OperationError {
    OperationError::Blocked(message.into())
}

fn valid_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    !name.is_empty()
        && name.trim() == name
        && !name.ends_with('.')
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

// Check every existing component before canonicalization can erase link provenance.
fn checked_parent(root: &Path, parent: &Path) -> Result<(), OperationError> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| blocked("Destination is outside the project"))?;
    let mut path = root.to_owned();
    ProjectSourceTree::validate_root(&path).map_err(OperationError::Blocked)?;
    for component in relative.components() {
        let std::path::Component::Normal(part) = component else {
            return Err(blocked("Invalid destination path"));
        };
        path.push(part);
        ProjectSourceTree::validate_root(&path).map_err(OperationError::Blocked)?;
    }
    if fs::metadata(parent)?.permissions().readonly() {
        return Err(blocked("Destination is read-only"));
    }
    Ok(())
}

fn vacant(parent: &Path, destination: &Path) -> Result<(), OperationError> {
    let name = destination
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    for entry in fs::read_dir(parent)? {
        if entry?.file_name().to_string_lossy().to_lowercase() == name {
            return Err(blocked(
                "Destination already exists (including case-only collisions)",
            ));
        }
    }
    Ok(())
}

impl ProjectContent {
    pub fn plan_operation(
        &self,
        request: OperationRequest,
    ) -> Result<OperationPlan, OperationError> {
        let OperationRequest::CreateFolder { parent, name } = request else {
            return Err(blocked(
                "Rename, move and duplicate require draft-aware reference preflight; not available yet",
            ));
        };
        if !valid_name(&name) {
            return Err(blocked(
                "Use a non-hidden portable folder name without separators or reserved characters",
            ));
        }
        let entry = self
            .source(parent)
            .ok_or_else(|| blocked("Destination no longer exists"))?;
        if entry.kind != ProjectSourceKind::Directory || entry.error.is_some() {
            return Err(blocked(
                "Destination must be a readable directory, not a link",
            ));
        }
        let root = self.source_tree().root_path();
        transaction::ensure_idle(root)?;
        checked_parent(root, &entry.path)?;
        let canonical_root = root.canonicalize()?;
        let canonical_parent = entry.path.canonicalize()?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(blocked("Destination is outside the project"));
        }
        let destination = entry.path.join(name);
        vacant(&entry.path, &destination)?;
        Ok(OperationPlan {
            root: root.to_owned(),
            parent: entry.path.clone(),
            destination,
        })
    }
}

impl OperationPlan {
    /// Revalidate immediately before mutation. No overwrite, recursive creation, or Ctrl+Z claim.
    pub fn apply(self) -> Result<OperationResult, OperationError> {
        transaction::ensure_idle(&self.root)?;
        checked_parent(&self.root, &self.parent)?;
        vacant(&self.parent, &self.destination)?;
        fs::create_dir(&self.destination)?;
        Ok(OperationResult {
            created_directory: self.destination,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relocation_capability_preserves_extensions_without_authorizing_unsafe_bytes() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("folder.with.dots")).unwrap();
        fs::write(root.path().join("image.PNG"), b"texture").unwrap();
        fs::write(root.path().join("unknown.bin"), b"unknown").unwrap();
        fs::write(root.path().join("include.wgsl"), "#import unsafe").unwrap();
        let content = ProjectContent::scan(root.path());
        let id = |name| content.source_tree().at_relative_path(name).unwrap().id;
        assert_eq!(
            content.source_relocation_suffix(content.source_tree().root()),
            None
        );
        assert_eq!(
            content.source_relocation_suffix(id("folder.with.dots")),
            Some("".into())
        );
        assert_eq!(
            content.source_relocation_suffix(id("image.PNG")),
            Some(".PNG".into())
        );
        assert_eq!(content.source_relocation_suffix(id("unknown.bin")), None);
        assert_eq!(
            content.source_relocation_suffix(id("include.wgsl")),
            Some(".wgsl".into())
        );
        assert!(
            content
                .plan_content_relocations(
                    vec![OperationRequest::Rename {
                        source: id("include.wgsl"),
                        name: "renamed".into(),
                    }],
                    &[],
                    true
                )
                .is_err(),
            "format eligibility is not reference proof"
        );
        assert!(root.path().join("include.wgsl").exists());
    }

    #[test]
    fn draft_references_replace_saved_references_and_unknown_files_block() {
        use aestra_core::material::*;
        let root = tempfile::tempdir().unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let mut program = MaterialProgram::additive_sprite("Caller");
        let content_path = root.path().join("program.aestra.material.ron");
        program.save_ron(&content_path).unwrap();
        let content = ProjectContent::scan(root.path());
        let target = content
            .unique_source_for_asset(crate::ProjectAssetId::MaterialFunction(function.id))
            .unwrap()
            .id;
        let owner = content
            .unique_source_for_asset(crate::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        program.expressions[0].kind = MaterialExpressionKind::FunctionCall {
            function: MaterialFunctionRef::Project(function.id),
            output: function.outputs[0].id,
            arguments: Default::default(),
        };
        let drafts = [(owner, DraftDocument::Program(Box::new(program)))];
        let report = content.reference_preflight(target, &drafts, true);
        assert_eq!(report.usages.len(), 1);
        assert_eq!(report.usages[0].owner, owner);
        assert!(
            content
                .reference_preflight(target, &[], true)
                .usages
                .is_empty()
        );
        assert!(
            !content
                .reference_preflight(target, &drafts, false)
                .incomplete
                .is_empty()
        );
        fs::write(root.path().join("unknown.wgsl"), "// includes not analyzed").unwrap();
        let content = ProjectContent::scan(root.path());
        assert!(
            !content
                .reference_preflight(target, &drafts, true)
                .incomplete
                .is_empty()
        );
        let folder = content.reference_preflight(content.source_tree().root(), &drafts, true);
        assert!(folder.affected_sources.contains(&owner));
        assert!(folder.affected_sources.contains(&target));
    }
    #[test]
    fn folder_creation_and_collision_revalidation() {
        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        let request = || OperationRequest::CreateFolder {
            parent: content.source_tree().root(),
            name: "Textures".into(),
        };
        let first = content.plan_operation(request()).unwrap();
        let stale = content.plan_operation(request()).unwrap();
        assert_eq!(
            first.apply().unwrap().created_directory,
            root.path().join("Textures")
        );
        assert!(stale.apply().is_err());
        assert!(
            content
                .plan_operation(OperationRequest::CreateFolder {
                    parent: content.source_tree().root(),
                    name: "textures".into()
                })
                .is_err()
        );
    }
    #[test]
    fn unsafe_names_and_unimplemented_mutations_are_blocked() {
        let root = tempfile::tempdir().unwrap();
        let content = ProjectContent::scan(root.path());
        for name in [
            "",
            "..",
            "../outside",
            "a/b",
            "a\\b",
            "CON",
            "NUL.txt",
            "COM1",
            "x.",
            ".aestra",
            " a",
        ] {
            assert!(
                content
                    .plan_operation(OperationRequest::CreateFolder {
                        parent: content.source_tree().root(),
                        name: name.into()
                    })
                    .is_err(),
                "{name}"
            );
        }
        assert!(
            content
                .plan_operation(OperationRequest::Rename {
                    source: content.source_tree().root(),
                    name: "Other".into()
                })
                .is_err()
        );
    }
    #[test]
    fn removed_parent_does_not_get_recreated() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent");
        fs::create_dir(&parent).unwrap();
        let content = ProjectContent::scan(root.path());
        let id = content
            .source_tree()
            .entries()
            .find(|e| e.path == parent)
            .unwrap()
            .id;
        let plan = content
            .plan_operation(OperationRequest::CreateFolder {
                parent: id,
                name: "child".into(),
            })
            .unwrap();
        fs::remove_dir(&parent).unwrap();
        assert!(plan.apply().is_err());
        assert!(!parent.exists());
    }
}
