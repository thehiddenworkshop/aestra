//! Single saved-asset duplication. This never rewrites a source or its users.
use super::*;
use aestra_core::{EffectAsset, EffectId, MaterialFunctionId, MaterialProgramId, material::*};
use std::io::Write;

#[derive(Debug)]
pub struct DuplicatePlan {
    destination: OperationPlan,
    source: PathBuf,
    baseline: String,
    serialized: String,
    asset: crate::ProjectAssetId,
}

#[derive(Debug)]
pub struct DuplicateResult {
    pub created_source: PathBuf,
    pub asset: crate::ProjectAssetId,
}

fn read_source(root: &Path, source: &Path) -> Result<String, OperationError> {
    checked_parent(
        root,
        source
            .parent()
            .ok_or_else(|| blocked("Missing source parent"))?,
    )?;
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_file() || super::super::source_tree::is_link(&metadata) {
        return Err(blocked("Source must be a regular file, not a link"));
    }
    Ok(fs::read_to_string(source)?)
}

impl ProjectContent {
    /// Prepare a saved semantic asset copy. `name` is a filename stem, not a display name.
    /// The host must provide every current draft at submission and explicitly present this as
    /// a saved-copy operation: later edits are not included and must not be discarded when
    /// publishing the result. Dirty target sources must be saved first. Other drafts/reverse
    /// references do not block this additive operation:
    /// no existing identity, path or dependency is rewritten. This is not rename/move preflight.
    pub fn plan_saved_asset_duplicate(
        &self,
        request: OperationRequest,
        drafts: &[(ProjectSourceId, DraftDocument)],
        all_drafts_known: bool,
    ) -> Result<DuplicatePlan, OperationError> {
        let OperationRequest::Duplicate {
            source,
            parent,
            name,
        } = request
        else {
            return Err(blocked("Expected a duplicate request"));
        };
        if !all_drafts_known || drafts.iter().any(|(owner, _)| *owner == source) {
            return Err(blocked(
                "Save the source first and provide a complete draft inventory",
            ));
        }
        if !valid_name(&name) {
            return Err(blocked("Use a portable non-hidden filename stem"));
        }
        let entry = self
            .source(source)
            .ok_or_else(|| blocked("Source is no longer indexed"))?;
        let original = self
            .asset_for_source(source)
            .ok_or_else(|| blocked("Source is not a supported semantic asset"))?;
        if self
            .unique_source_for_asset(original)
            .ok()
            .map(|entry| entry.id)
            != Some(source)
        {
            return Err(blocked("Source identity is ambiguous"));
        }
        let baseline = read_source(self.source_tree().root_path(), &entry.path)?;
        let suffix = self
            .asset_operation_suffix(source)
            .ok_or_else(|| blocked("This asset does not support saved duplication"))?;
        let (serialized, asset) = match self.documents.get(&source) {
            Some(ProjectSourceDocument::Effect(saved)) => {
                let mut copy =
                    EffectAsset::from_ron(&baseline).map_err(|e| blocked(&e.to_string()))?;
                if &copy != saved.as_ref() {
                    return Err(blocked("Source changed; refresh before duplicating"));
                }
                // The document owns its emitters, regions, events and resources. Preserve
                // these local IDs and their wiring; project effect/material links stay shared.
                // Only the project-level identity changes when copying the entire owner.
                copy.id = EffectId::new();
                (
                    copy.to_pretty_ron().map_err(|e| blocked(&e.to_string()))?,
                    crate::ProjectAssetId::Effect(copy.id),
                )
            }
            Some(ProjectSourceDocument::MaterialProgram(saved)) => {
                let mut copy =
                    MaterialProgram::from_ron(&baseline).map_err(|e| blocked(&e.to_string()))?;
                if copy != saved.normalized() {
                    return Err(blocked("Source changed; refresh before duplicating"));
                }
                if copy
                    .expressions
                    .iter()
                    .any(|e| matches!(e.kind, MaterialExpressionKind::CustomWeslCall { .. }))
                {
                    return Err(blocked("Custom WESL duplication is not supported yet"));
                }
                copy.id = MaterialProgramId::new();
                (
                    copy.to_pretty_ron().map_err(|e| blocked(&e.to_string()))?,
                    crate::ProjectAssetId::MaterialProgram(copy.id),
                )
            }
            Some(ProjectSourceDocument::MaterialFunction(saved)) => {
                let mut copy =
                    MaterialFunction::from_ron(&baseline).map_err(|e| blocked(&e.to_string()))?;
                if copy != saved.normalized() {
                    return Err(blocked("Source changed; refresh before duplicating"));
                }
                if copy.custom_wesl.is_some()
                    || copy
                        .expressions
                        .iter()
                        .any(|e| matches!(e.kind, MaterialExpressionKind::CustomWeslCall { .. }))
                {
                    return Err(blocked("Custom WESL duplication is not supported yet"));
                }
                copy.id = MaterialFunctionId::new();
                (
                    copy.to_pretty_ron().map_err(|e| blocked(&e.to_string()))?,
                    crate::ProjectAssetId::MaterialFunction(copy.id),
                )
            }
            _ => {
                return Err(blocked("This asset does not support saved duplication"));
            }
        };
        // Expression, parameter and signature IDs are owner-local. Keeping them preserves all
        // internal wiring; project function/texture references intentionally remain shared.
        // Reuse folder planning's path/name/collision checks, but never its directory apply.
        let destination = self.plan_operation(OperationRequest::CreateFolder {
            parent,
            name: format!("{name}{suffix}"),
        })?;
        Ok(DuplicatePlan {
            destination,
            source: entry.path.clone(),
            baseline,
            serialized,
            asset,
        })
    }
}

impl DuplicatePlan {
    /// Stage in the destination directory and publish without replacing any existing file.
    /// Recheck source bytes and destination immediately before publication. No document Undo.
    pub fn apply(self) -> Result<DuplicateResult, OperationError> {
        super::transaction::ensure_idle(&self.destination.root)?;
        let plan = &self.destination;
        checked_parent(&plan.root, &plan.parent)?;
        vacant(&plan.parent, &plan.destination)?;
        let mut staged = tempfile::NamedTempFile::new_in(&plan.parent)?;
        staged.write_all(self.serialized.as_bytes())?;
        staged.as_file().sync_all()?;
        checked_parent(&plan.root, &plan.parent)?;
        if read_source(&plan.root, &self.source)? != self.baseline {
            return Err(blocked(
                "Source changed after preflight; duplicate cancelled",
            ));
        }
        vacant(&plan.parent, &plan.destination)?;
        staged
            .persist_noclobber(&plan.destination)
            .map_err(|e| OperationError::Io(e.error.to_string()))?;
        Ok(DuplicateResult {
            created_source: plan.destination.clone(),
            asset: self.asset,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(content: &ProjectContent, source: ProjectSourceId, name: &str) -> OperationRequest {
        OperationRequest::Duplicate {
            source,
            parent: content.source_tree().root(),
            name: name.into(),
        }
    }

    #[test]
    fn effect_copy_has_a_new_owner_but_preserves_all_local_wiring_and_shared_links() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original.aestra.ron");
        let effect = EffectAsset::from_ron(include_str!(
            "../../../../../assets/effects/prism_bloom.aestra.ron"
        ))
        .unwrap();
        effect.save_ron(&original).unwrap();
        let bytes = fs::read(&original).unwrap();
        let content = ProjectContent::scan(root.path());
        let source = content
            .unique_source_for_asset(crate::ProjectAssetId::Effect(effect.id))
            .unwrap()
            .id;
        assert!(
            content
                .plan_saved_asset_duplicate(
                    request(&content, source, "copy"),
                    &[(source, DraftDocument::Effect(Box::new(effect.clone())))],
                    true
                )
                .is_err()
        );
        let result = content
            .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
            .unwrap()
            .apply()
            .unwrap();
        let mut copy = EffectAsset::load_ron(&result.created_source).unwrap();
        let new_id = copy.id;
        assert_ne!(new_id, effect.id);
        assert_eq!(result.asset, crate::ProjectAssetId::Effect(new_id));
        copy.id = effect.id;
        assert_eq!(copy, effect); // Includes resources, events, clips, curves and renderer wiring.
        copy.id = new_id;
        copy.name = "Independent copy".into();
        copy.save_ron(&result.created_source).unwrap();
        assert_eq!(fs::read(original).unwrap(), bytes);
        let fresh = ProjectContent::scan(root.path());
        assert!(
            fresh
                .unique_source_for_asset(crate::ProjectAssetId::Effect(effect.id))
                .is_ok()
        );
        assert!(fresh.unique_source_for_asset(result.asset).is_ok());
    }

    #[test]
    fn effect_duplicate_revalidates_source_and_never_overwrites() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original.aestra.ron");
        let mut effect = EffectAsset::new("Original", 2.0);
        effect.save_ron(&original).unwrap();
        let content = ProjectContent::scan(root.path());
        let source = content
            .unique_source_for_asset(crate::ProjectAssetId::Effect(effect.id))
            .unwrap()
            .id;
        let plan = content
            .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
            .unwrap();
        effect.name = "External edit".into();
        effect.save_ron(&original).unwrap();
        assert!(plan.apply().is_err());
        assert!(!root.path().join("copy.aestra.ron").exists());
        let content = ProjectContent::scan(root.path());
        let plan = content
            .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
            .unwrap();
        let destination = root.path().join("copy.aestra.ron");
        fs::write(&destination, "existing bytes").unwrap();
        assert!(plan.apply().is_err());
        assert_eq!(fs::read_to_string(destination).unwrap(), "existing bytes");
    }

    #[test]
    fn copies_have_fresh_identity_but_keep_wiring_names_and_shared_references() {
        let root = tempfile::tempdir().unwrap();
        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/materials/dissolve_edge.aestra.material-function.ron"
        ))
        .unwrap();
        let function_path = root.path().join("function.aestra.material-function.ron");
        function.save_ron(&function_path).unwrap();
        let mut program = MaterialProgram::additive_sprite("Original display name");
        // A shared resource reference is data, not a path to relocate when duplicating.
        program.expressions.push(MaterialExpression {
            id: aestra_core::MaterialExpressionId::new(),
            kind: MaterialExpressionKind::Constant(MaterialValue::Texture2D(
                aestra_core::AssetId::new(),
            )),
        });
        let program_path = root.path().join("program.aestra.material.ron");
        program.save_ron(&program_path).unwrap();
        let content = ProjectContent::scan(root.path());
        for (asset, path) in [
            (
                crate::ProjectAssetId::MaterialProgram(program.id),
                program_path,
            ),
            (
                crate::ProjectAssetId::MaterialFunction(function.id),
                function_path,
            ),
        ] {
            let source = content.unique_source_for_asset(asset).unwrap().id;
            let before = fs::read(&path).unwrap();
            let result = content
                .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
                .unwrap()
                .apply()
                .unwrap();
            assert_ne!(result.asset, asset);
            assert_eq!(fs::read(&path).unwrap(), before);
            match asset {
                crate::ProjectAssetId::MaterialProgram(_) => {
                    let mut copy = MaterialProgram::load_ron(&result.created_source).unwrap();
                    copy.id = program.id;
                    assert_eq!(copy, program.normalized());
                }
                crate::ProjectAssetId::MaterialFunction(_) => {
                    let mut copy = MaterialFunction::load_ron(&result.created_source).unwrap();
                    copy.id = function.id;
                    assert_eq!(copy, function.normalized());
                }
                _ => unreachable!(),
            }
            let refreshed = ProjectContent::scan(root.path());
            assert!(refreshed.unique_source_for_asset(result.asset).is_ok());
            assert!(refreshed.unique_source_for_asset(asset).is_ok());
        }
    }

    #[test]
    fn dirty_unknown_stale_and_colliding_sources_never_write() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let path = root.path().join("program.aestra.material.ron");
        program.save_ron(&path).unwrap();
        let content = ProjectContent::scan(root.path());
        let source = content
            .unique_source_for_asset(crate::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        let plan =
            || content.plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true);
        assert!(
            content
                .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], false)
                .is_err()
        );
        assert!(
            content
                .plan_saved_asset_duplicate(
                    request(&content, source, "copy"),
                    &[(source, DraftDocument::Program(Box::new(program.clone())))],
                    true
                )
                .is_err()
        );
        let stale = plan().unwrap();
        let mut changed = program.clone();
        changed.name = "External edit".into();
        changed.save_ron(&path).unwrap();
        assert!(stale.apply().is_err());
        assert!(plan().is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        program.save_ron(&path).unwrap();
        let collision = plan().unwrap();
        let destination = root.path().join("COPY.aestra.material.ron");
        fs::write(&destination, "do not overwrite").unwrap();
        assert!(collision.apply().is_err());
        assert_eq!(
            fs::read_to_string(&destination).unwrap(),
            "do not overwrite"
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn removed_source_and_custom_wesl_are_blocked() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        let path = root.path().join("program.aestra.material.ron");
        program.save_ron(&path).unwrap();
        let content = ProjectContent::scan(root.path());
        let source = content
            .unique_source_for_asset(crate::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        let plan = content
            .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
            .unwrap();
        fs::remove_file(&path).unwrap();
        assert!(plan.apply().is_err());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);

        let function = MaterialFunction::from_ron(include_str!(
            "../../../../../assets/materials/pulse_wave.aestra.material-function.ron"
        ))
        .unwrap();
        function
            .save_ron(root.path().join("custom.aestra.material-function.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let source = content
            .unique_source_for_asset(crate::ProjectAssetId::MaterialFunction(function.id))
            .unwrap()
            .id;
        assert!(
            content
                .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
                .is_err()
        );
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn invalid_destinations_unsupported_assets_and_ambiguous_ids_are_blocked() {
        let root = tempfile::tempdir().unwrap();
        let program = MaterialProgram::additive_sprite("Original");
        program
            .save_ron(root.path().join("program.aestra.material.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        let source = content
            .unique_source_for_asset(crate::ProjectAssetId::MaterialProgram(program.id))
            .unwrap()
            .id;
        for name in ["", "../outside", "CON", "with/slash", " trailing "] {
            assert!(
                content
                    .plan_saved_asset_duplicate(request(&content, source, name), &[], true)
                    .is_err()
            );
        }
        assert!(
            content
                .plan_saved_asset_duplicate(
                    request(&content, content.source_tree().root(), "copy"),
                    &[],
                    true
                )
                .is_err()
        );
        assert!(
            content
                .plan_saved_asset_duplicate(
                    OperationRequest::Duplicate {
                        source,
                        parent: source,
                        name: "copy".into()
                    },
                    &[],
                    true
                )
                .is_err()
        );
        program
            .save_ron(root.path().join("ambiguous.aestra.material.ron"))
            .unwrap();
        let content = ProjectContent::scan(root.path());
        assert!(
            content
                .plan_saved_asset_duplicate(request(&content, source, "copy"), &[], true)
                .is_err()
        );
    }
}
