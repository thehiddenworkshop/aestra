//! Explicit mutation preflight. Document edits and filesystem operations are separate histories.
use super::{ProjectContent, ProjectSourceKind, ProjectSourceTree};
use super::{ProjectRelationStatus, ProjectSourceDocument, ProjectSourceRelation};
use crate::ProjectSourceId;

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
    /// Read-only inventory over this snapshot and explicit host drafts. The host must include
    /// every open/unsaved document and report false when its draft inventory is incomplete.
    /// Fresh disk/source-byte revalidation is still required by the eventual operation planner.
    pub fn reference_preflight(
        &self,
        source: ProjectSourceId,
        drafts: &[(ProjectSourceId, DraftDocument)],
        all_drafts_known: bool,
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
            if entry.error.is_some() || !projected.documents.contains_key(&entry.id) {
                report.incomplete.push(format!(
                    "References not fully known for {}",
                    entry.relative_path.display()
                ));
                continue;
            }
            if matches!(projected.documents.get(&entry.id), Some(ProjectSourceDocument::MaterialFunction(function)) if function.custom_wesl.is_some())
            {
                report.incomplete.push(format!(
                    "Custom WESL include semantics are unknown for {}",
                    entry.relative_path.display()
                ));
            }
            for dependency in projected.source_relations(entry.id).dependencies {
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
