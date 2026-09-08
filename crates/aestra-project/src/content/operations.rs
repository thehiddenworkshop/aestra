//! Explicit mutation preflight. Document edits and filesystem operations are separate histories.
use super::{ProjectContent, ProjectSourceKind, ProjectSourceTree};
use crate::ProjectSourceId;
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
