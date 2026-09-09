//! One publication/reconciliation path for browser Move, inline Rename and recovery.
use crate::{project_content::io, *};
use aestra_project::content::operations::AssetMoveBatchResult;
use std::path::{Path, PathBuf};

/// Call only after the captured I/O guard and complete, draft-free inventory pass.
/// Reload failure is a post-publication warning, not permission to repeat a move.
pub(super) fn publish(
    world: &mut World,
    mut catalog: ProjectEffectCatalog,
    result: AssetMoveBatchResult,
    selected_source: &Path,
) -> (PathBuf, Option<String>) {
    let destination = result
        .folders
        .iter()
        .chain(&result.moves)
        .find(|item| item.source == selected_source)
        .map(|item| item.destination.clone())
        .expect("a planned single-source relocation returns its destination");
    let before = catalog.content().clone();
    catalog.refresh();
    let warning = reconcile_document(&catalog, &mut world.resource_mut::<EditorSession>()).err();
    if let Some(mut state) = world.get_resource_mut::<super::AssetBrowserState>() {
        state.reconcile_relocations(&before, catalog.content(), &result);
        state.reconcile(catalog.content(), catalog.content_revision());
        if let Ok(relative) = destination.strip_prefix(catalog.root())
            && let Some(entry) = catalog.content().source_tree().at_relative_path(relative)
        {
            state.locate(catalog.content(), entry.id);
        }
    }
    io::publish_catalog(world, catalog);
    (destination, warning)
}

pub(super) fn failed(world: &mut World) {
    // Inspection, never automatic rollback. Also handles an existing pending batch.
    world.trigger(super::relocation_recovery::CheckRecovery);
}

/// Material/function targets resolve by ID in the new catalog. Effects also retain
/// a path and exact-byte Save baseline. Never adopt an ambiguous or unrelated source.
pub(super) fn reconcile_document(
    catalog: &ProjectEffectCatalog,
    session: &mut EditorSession,
) -> Result<(), String> {
    let Some(source) = &session.source_path else {
        return Ok(());
    };
    if !inside_project(source, catalog.root()) {
        return Ok(());
    }
    let path = catalog.openable_path(session.effect.id.into())
        .ok_or("Locations changed, but the open effect is unavailable or ambiguous. Reopen it before saving.")?;
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let effect =
        EffectAsset::from_ron(std::str::from_utf8(&bytes).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    if effect.id != session.effect.id {
        return Err("Source identity changed; reopen it before saving.".into());
    }
    if effect == session.effect {
        session.accept_relocated_source(path.to_owned(), bytes);
    } else {
        let compiled = catalog.compile_project(&effect)
            .map_err(|error| format!("Locations changed, but the open effect could not reload: {error}. Reopen it before saving."))?;
        let target = session.material_target.clone();
        let material_history_active = session.material_history_active;
        session.open_refreshed_effect(path, effect, compiled.root, bytes);
        session.material_target = target;
        session.material_history_active = material_history_active;
    }
    Ok(())
}

fn inside_project(path: &Path, root: &Path) -> bool {
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return false;
    }
    fn normalized(path: &Path) -> String {
        let path = path.to_string_lossy().replace('\\', "/");
        let path = path.strip_prefix("//?/").unwrap_or(&path);
        if cfg!(windows) {
            path.to_lowercase()
        } else {
            path.to_owned()
        }
    }
    normalized(path).starts_with(&format!("{}/", normalized(root).trim_end_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_project::content::operations::OperationRequest;

    #[test]
    fn folder_relocation_retargets_history_expansion_and_inspection() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("pack/child")).unwrap();
        std::fs::create_dir(root.path().join("destination")).unwrap();
        std::fs::write(root.path().join("pack/child/image.png"), b"texture").unwrap();
        let mut catalog = ProjectEffectCatalog::scan(root.path());
        let before = catalog.content().clone();
        let folder = before.source_tree().at_relative_path("pack").unwrap().id;
        let child = before
            .source_tree()
            .at_relative_path("pack/child")
            .unwrap()
            .id;
        let image = before
            .source_tree()
            .at_relative_path("pack/child/image.png")
            .unwrap()
            .id;
        let mut state = super::super::AssetBrowserState::default();
        state.reconcile(&before, catalog.content_revision());
        state.folder = "pack/child".into();
        state.back = vec!["pack".into()];
        state.forward = vec!["pack/child".into()];
        state.expanded.extend([folder, child]);
        state.selected = Some(image);
        state.inspected = Some(image);
        let result = before
            .plan_content_relocations(
                vec![OperationRequest::Move {
                    source: folder,
                    parent: before
                        .source_tree()
                        .at_relative_path("destination")
                        .unwrap()
                        .id,
                }],
                &[],
                true,
            )
            .unwrap()
            .apply()
            .unwrap();
        catalog.refresh();
        state.reconcile_relocations(&before, catalog.content(), &result);
        state.reconcile(catalog.content(), catalog.content_revision());
        assert_eq!(state.folder, Path::new("destination/pack/child"));
        assert_eq!(state.back, vec![PathBuf::from("destination/pack")]);
        assert_eq!(state.forward, vec![PathBuf::from("destination/pack/child")]);
        let id = |name| {
            catalog
                .content()
                .source_tree()
                .at_relative_path(name)
                .unwrap()
                .id
        };
        assert!(state.expanded.contains(&id("destination/pack/child")));
        assert_eq!(state.selected, Some(id("destination/pack/child/image.png")));
        assert_eq!(state.inspected, state.selected);
    }
}
