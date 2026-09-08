//! Recover source identity and drafts before publishing any part of the restored session.
use super::*;
use crate::material_document::MaterialEditingTarget;

pub(super) fn restore_candidate(
    session: &mut EditorSession,
    persistence: &mut RecoveryPersistence,
    candidate: &RecoveryCandidate,
    catalog: &mut ProjectEffectCatalog,
) -> Result<Vec<String>, String> {
    let mut drafts = candidate.material_drafts().clone();
    let mut target = candidate.material_target().clone();
    let target_root = match &target {
        MaterialEditingTarget::EffectInstance => None,
        MaterialEditingTarget::Function { root, id } => {
            if id.is_nil() {
                return Err("Recovered function has an invalid identity".into());
            }
            Some(root.as_path())
        }
        MaterialEditingTarget::Program { root, id } => {
            if id.is_nil() {
                return Err("Recovered material has an invalid identity".into());
            }
            Some(root.as_path())
        }
    };
    let root = if !drafts.is_empty() {
        Some(
            drafts
                .root
                .as_deref()
                .ok_or("Recovery has no material project root")?,
        )
    } else {
        target_root
    };
    let mut prepared = if let Some(root) = root {
        aestra_project::ProjectSourceTree::validate_root(root)?;
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        if let Some(target_root) = target_root
            && target_root
                .canonicalize()
                .map_err(|error| error.to_string())?
                != root
        {
            return Err("Recovered material target and drafts belong to different projects".into());
        }
        // This is the recorded asset root, not a directory to search for a nested assets folder.
        let effects = if root.join("effects").is_dir() {
            root.join("effects")
        } else {
            root.clone()
        };
        ProjectEffectCatalog::try_scan_project(root, effects)?
    } else {
        catalog.clone()
    };
    let mut warnings = if drafts.is_empty() {
        Vec::new()
    } else {
        drafts.recover_paths(prepared.content().asset_index())?
    };
    prepared.material_drafts = drafts.clone();
    if let MaterialEditingTarget::Program { root, id } = &mut target {
        *root = prepared.root().to_owned();
        // Keep a missing/ambiguous target explicit. The graph reports the error; no fallback
        // selects another asset or drops the draft. Save/Reload retain their normal guards.
        if let Err(error) = prepared.material_program(*id) {
            warnings.push(format!(
                "Material target unavailable; recovered drafts were kept: {error}"
            ));
        }
    }
    if let MaterialEditingTarget::Function { root, id } = &mut target {
        *root = prepared.root().to_owned();
        if let Err(error) = prepared
            .content()
            .cached_material_function(aestra_core::material::MaterialFunctionRef::Project(*id))
        {
            warnings.push(format!(
                "Function target unavailable; recovered drafts were kept: {error}"
            ));
        }
    }
    session.restore_recovery(
        candidate.effect().clone(),
        candidate.source_path().map(Path::to_owned),
    );
    session.set_material_drafts(drafts);
    session.material_history_active = matches!(
        target,
        MaterialEditingTarget::Program { .. } | MaterialEditingTarget::Function { .. }
    );
    session.material_target = target;
    if let Ok(project) = prepared.compile_project(&session.effect) {
        let _ = session.install_compiled_project_root(project.root);
    }
    *catalog = prepared;
    persistence.activate(candidate);
    Ok(warnings)
}

#[cfg(test)]
mod tests;
