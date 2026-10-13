//! Actual active/proposed reference checks and complete shared-draft inventory.
use crate::{ProjectEffectCatalog, session::EditorSession};
use aestra_project::{ProjectAssetId, ProjectSourceId};

/// Opening a source is not a usage; references from a surviving effect still block.
pub(super) fn active_block(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    source: ProjectSourceId,
) -> bool {
    let Some(entry) = catalog.content().source(source) else {
        return true;
    };
    let Ok(selected_path) = entry.path.canonicalize() else {
        return true;
    };
    if session
        .source_path
        .as_ref()
        .and_then(|path| path.canonicalize().ok())
        .is_some_and(|path| path.starts_with(&selected_path))
    {
        return false;
    }
    let ids: Vec<_> = catalog
        .content()
        .source_tree()
        .entries()
        .filter(|e| e.relative_path.starts_with(&entry.relative_path))
        .filter_map(|e| catalog.content().asset_for_source(e.id))
        .collect();
    let used_by_effect = |effect: &aestra_core::EffectAsset| {
        effect
            .effect_clips
            .iter()
            .any(|clip| ids.contains(&clip.source.into()))
            || effect
                .material_instances
                .iter()
                .any(|instance| match instance.program {
                    aestra_core::material::MaterialProgramRef::Project(id) => {
                        ids.contains(&aestra_project::ProjectAssetId::MaterialProgram(id))
                    }
                    _ => false,
                })
            || effect.assets.iter().any(|asset| {
                let file = asset
                    .path
                    .rsplit_once('#')
                    .map_or(asset.path.as_str(), |(file, _)| file);
                catalog
                    .root()
                    .join(file.replace('\\', "/"))
                    .canonicalize()
                    .ok()
                    .is_some_and(|path| path.starts_with(&selected_path))
            })
    };
    used_by_effect(&session.effect)
        || session
            .pending_change
            .as_ref()
            .is_some_and(|pending| used_by_effect(pending.preview.candidate()))
}

pub(crate) fn draft_inventory(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> (
    Vec<(
        ProjectSourceId,
        aestra_project::content::operations::DraftDocument,
    )>,
    bool,
) {
    collect_drafts(catalog, session, true)
}

/// Deletion checks the active effect's current references separately, including
/// untitled effects which have no indexed source to attach a draft to.
pub(crate) fn deletion_draft_inventory(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> (
    Vec<(
        ProjectSourceId,
        aestra_project::content::operations::DraftDocument,
    )>,
    bool,
) {
    collect_drafts(catalog, session, false)
}

fn collect_drafts(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
    include_effect: bool,
) -> (
    Vec<(
        ProjectSourceId,
        aestra_project::content::operations::DraftDocument,
    )>,
    bool,
) {
    use aestra_project::content::operations::DraftDocument;
    let content = catalog.content();
    let mut drafts = Vec::new();
    // The deletion coordinator checks both active and proposed effect references.
    let mut complete = !include_effect || session.pending_change.is_none();
    if include_effect && session.effect_is_dirty() {
        if let Ok(entry) =
            content.unique_source_for_asset(ProjectAssetId::Effect(session.effect.id))
        {
            drafts.push((
                entry.id,
                DraftDocument::Effect(Box::new(session.effect.clone())),
            ));
        } else {
            complete = false;
        }
    }
    for (id, draft) in &catalog.material_drafts.programs {
        if let (Ok(entry), Some(current)) = (
            content.unique_source_for_asset(ProjectAssetId::MaterialProgram(*id)),
            &draft.current,
        ) {
            drafts.push((entry.id, DraftDocument::Program(Box::new(current.clone()))));
        } else {
            complete = false;
        }
    }
    for (id, draft) in &catalog.material_drafts.functions {
        if let (Ok(entry), Some(current)) = (
            content.unique_source_for_asset(ProjectAssetId::MaterialFunction(*id)),
            &draft.current,
        ) {
            drafts.push((entry.id, DraftDocument::Function(Box::new(current.clone()))));
        } else {
            complete = false;
        }
    }
    if !catalog.material_drafts.is_empty()
        && catalog.material_drafts.root.as_deref() != Some(catalog.root())
    {
        complete = false;
    }
    (drafts, complete)
}
