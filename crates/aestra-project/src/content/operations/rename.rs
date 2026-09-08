//! Filename-only, same-directory rename for fully understood semantic projects.
use super::*;
use crate::ProjectAssetId;
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct RenamePlan {
    destination: OperationPlan,
    source: PathBuf,
    inventory: BTreeMap<PathBuf, Vec<u8>>,
    asset: ProjectAssetId,
}

#[derive(Debug)]
pub struct RenameResult {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub asset: ProjectAssetId,
}

fn inventory(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>, OperationError> {
    let tree = ProjectSourceTree::scan(root);
    let mut result = BTreeMap::new();
    for entry in tree.entries() {
        if entry.error.is_some() {
            return Err(blocked("Project discovery is incomplete"));
        }
        if entry.kind == ProjectSourceKind::Directory {
            continue;
        }
        checked_parent(
            root,
            entry
                .path
                .parent()
                .ok_or_else(|| blocked("Missing parent"))?,
        )?;
        let metadata = fs::symlink_metadata(&entry.path)?;
        if !metadata.is_file() || super::super::source_tree::is_link(&metadata) {
            return Err(blocked("Linked or unreadable sources block rename"));
        }
        result.insert(entry.path.clone(), fs::read(&entry.path)?);
    }
    Ok(result)
}

impl ProjectContent {
    /// Host must recheck its draft/session guard immediately before apply. No path rewrites
    /// are supported: unknown references and known filename references block this operation.
    pub fn plan_material_rename(
        &self,
        request: OperationRequest,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<RenamePlan, OperationError> {
        let OperationRequest::Rename { source, name } = request else {
            return Err(blocked("Expected a rename request"));
        };
        if !complete || drafts.iter().any(|(owner, _)| *owner == source) {
            return Err(blocked(
                "Save the source and resolve pending drafts before renaming",
            ));
        }
        let asset = self
            .asset_for_source(source)
            .ok_or_else(|| blocked("Unsupported source"))?;
        let suffix = match asset {
            ProjectAssetId::MaterialProgram(_) => ".aestra.material.ron",
            ProjectAssetId::MaterialFunction(_) => ".aestra.material-function.ron",
            _ => {
                return Err(blocked(
                    "Only materials and graph functions support filename Rename",
                ));
            }
        };
        if !valid_name(&name) {
            return Err(blocked("Use a portable filename stem"));
        }
        let entry = self
            .unique_source_for_asset(asset)
            .map_err(|_| blocked("Source identity is ambiguous"))?;
        let root = self.source_tree().root_path();
        let baseline = inventory(root)?;
        let fresh = ProjectContent::scan(root);
        // Reject stale selected documents instead of silently renaming their replacements.
        let fresh_entry = fresh
            .unique_source_for_asset(asset)
            .map_err(|_| blocked("Source changed; refresh first"))?;
        let same = match (fresh.documents.get(&source), self.documents.get(&source)) {
            (
                Some(ProjectSourceDocument::MaterialProgram(a)),
                Some(ProjectSourceDocument::MaterialProgram(b)),
            ) => a == b,
            (
                Some(ProjectSourceDocument::MaterialFunction(a)),
                Some(ProjectSourceDocument::MaterialFunction(b)),
            ) => a == b,
            _ => false,
        };
        if fresh_entry.id != source || !same {
            return Err(blocked("Source changed; refresh first"));
        }
        let report = fresh.reference_preflight(source, drafts, complete);
        if let Some(reason) = report.incomplete.first() {
            return Err(blocked(&format!("Rename blocked: {reason}")));
        }
        // Path aliases/case rules are not yet audited by the relation index. Do not infer
        // safety from an absent exact lexical reverse match when path-backed assets exist.
        let path_backed = fresh.documents.values().any(|document| matches!(document, ProjectSourceDocument::Effect(effect) if !effect.assets.is_empty()))
            || drafts.iter().any(|(_, draft)| matches!(draft, DraftDocument::Effect(effect) if !effect.assets.is_empty()));
        if path_backed
            || report
                .usages
                .iter()
                .any(|usage| matches!(usage.target, super::super::ProjectRelationTarget::File(_)))
        {
            return Err(blocked(
                "Filename references require a reference-rewrite transaction",
            ));
        }
        if fs::metadata(&entry.path)?.permissions().readonly() {
            return Err(blocked("Source is read-only"));
        }
        let destination = fresh.plan_operation(OperationRequest::CreateFolder {
            parent: entry
                .parent
                .ok_or_else(|| blocked("Missing source parent"))?,
            name: format!("{name}{suffix}"),
        })?;
        if inventory(root)? != baseline {
            return Err(blocked("Project changed during preflight"));
        }
        Ok(RenamePlan {
            destination,
            source: entry.path.clone(),
            inventory: baseline,
            asset,
        })
    }
}

impl RenamePlan {
    pub fn apply(self) -> Result<RenameResult, OperationError> {
        let plan = self.destination;
        checked_parent(&plan.root, &plan.parent)?;
        if inventory(&plan.root)? != self.inventory {
            return Err(blocked("Project files changed; rename cancelled"));
        }
        if fs::metadata(&self.source)?.permissions().readonly() {
            return Err(blocked("Source is read-only"));
        }
        vacant(&plan.parent, &plan.destination)?;
        rename_exclusive(&self.source, &plan.destination)?;
        Ok(RenameResult {
            source: self.source,
            destination: plan.destination,
            asset: self.asset,
        })
    }
}

#[cfg(windows)]
fn rename_exclusive(source: &Path, destination: &Path) -> Result<(), OperationError> {
    use std::os::windows::ffi::OsStrExt;
    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: both pointers reference live NUL-terminated UTF-16 buffers. Zero flags means
    // no replacement and no copy fallback; same-directory publication is a single rename.
    if unsafe {
        windows_sys::Win32::Storage::FileSystem::MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            0,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn rename_exclusive(source: &Path, destination: &Path) -> Result<(), OperationError> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        destination,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|e| OperationError::Io(e.to_string()))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn rename_exclusive(_: &Path, _: &Path) -> Result<(), OperationError> {
    Err(blocked(
        "Exclusive rename is not implemented on this platform",
    ))
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;
    use aestra_core::{EffectAsset, MaterialId, material::*};

    fn request(content: &ProjectContent, asset: ProjectAssetId, name: &str) -> OperationRequest {
        OperationRequest::Rename {
            source: content.unique_source_for_asset(asset).unwrap().id,
            name: name.into(),
        }
    }

    #[test]
    fn filename_rename_preserves_bytes_identity_and_effect_resolution() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Display name");
        let original = root.path().join("original.aestra.material.ron");
        program.save_ron(&original).unwrap();
        let bytes = fs::read(&original).unwrap();
        let mut effect = EffectAsset::new("User", 1.0);
        effect.material_instances.push(MaterialInstance {
            id: MaterialId::new(),
            program: MaterialProgramRef::Project(program.id),
            values: Default::default(),
            render_state: MaterialRenderState::additive_sprite(),
        });
        effect
            .save_ron(root.path().join("user.aestra.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let result = content
            .plan_material_rename(
                request(
                    &content,
                    ProjectAssetId::MaterialProgram(program.id),
                    "renamed",
                ),
                &[],
                true,
            )
            .unwrap()
            .apply()
            .unwrap();
        assert!(!original.exists());
        assert_eq!(fs::read(&result.destination).unwrap(), bytes);
        assert_eq!(
            MaterialProgram::load_ron(&result.destination).unwrap().name,
            program.name
        );
        let fresh = ProjectContent::scan(root.path());
        assert!(fresh.asset_index().resolve_effect_project(&effect).is_ok());
        assert_eq!(
            fresh.unique_source_for_asset(result.asset).unwrap().path,
            result.destination
        );
    }

    #[test]
    fn function_identity_and_all_signatures_survive_rename() {
        let root = tempfile::tempdir().unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("function.aestra.material-function.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let result = content
            .plan_material_rename(
                request(
                    &content,
                    ProjectAssetId::MaterialFunction(function.id),
                    "renamed",
                ),
                &[],
                true,
            )
            .unwrap()
            .apply()
            .unwrap();
        assert_eq!(
            MaterialFunction::load_ron(result.destination).unwrap(),
            function
        );
    }

    #[test]
    fn stale_dirty_unknown_and_colliding_renames_preserve_original() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let original = root.path().join("original.aestra.material.ron");
        program.save_ron(&original).unwrap();
        let content = ProjectContent::scan(root.path());
        let asset = ProjectAssetId::MaterialProgram(program.id);
        let source = content.unique_source_for_asset(asset).unwrap().id;
        for name in ["original", "ORIGINAL", "../escape", "NUL", ""] {
            assert!(
                content
                    .plan_material_rename(request(&content, asset, name), &[], true)
                    .is_err()
            );
        }
        assert!(
            content
                .plan_material_rename(request(&content, asset, "renamed"), &[], false)
                .is_err()
        );
        assert!(
            content
                .plan_material_rename(
                    request(&content, asset, "renamed"),
                    &[(source, DraftDocument::Program(Box::new(program)))],
                    true
                )
                .is_err()
        );
        let plan = content
            .plan_material_rename(request(&content, asset, "renamed"), &[], true)
            .unwrap();
        fs::write(root.path().join("unknown.wgsl"), "unknown includes").unwrap();
        assert!(plan.apply().is_err());
        assert!(
            content
                .plan_material_rename(request(&content, asset, "renamed"), &[], true)
                .is_err()
        );
        assert!(original.exists());
    }

    #[test]
    fn exclusive_primitive_never_replaces_destination() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::write(&source, "original").unwrap();
        fs::write(&destination, "existing").unwrap();
        assert!(rename_exclusive(&source, &destination).is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), "original");
        assert_eq!(fs::read_to_string(destination).unwrap(), "existing");
    }
}
