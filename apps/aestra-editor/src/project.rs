//! Project location and document-boundary operations, independent of file dialogs and widgets.

use crate::project_content::EditorProjectContent as ProjectEffectCatalog;
use crate::session::EditorSession;
use std::path::{Path, PathBuf};

/// Accept either an asset directory or a conventional project containing `assets/`.
pub(crate) fn catalog_for_folder(folder: &Path) -> Result<ProjectEffectCatalog, String> {
    aestra_project::ProjectSourceTree::validate_root(folder)?;
    if folder.join("assets").is_dir() {
        aestra_project::ProjectSourceTree::validate_root(folder.join("assets"))?;
    }
    let folder = folder.canonicalize().map_err(|error| error.to_string())?;
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let root = if folder.join("assets").is_dir() {
        folder.join("assets")
    } else {
        folder
    };
    let effects = if root.join("effects").is_dir() {
        root.join("effects")
    } else {
        root.clone()
    };
    ProjectEffectCatalog::try_scan_project(root, effects)
}

pub(crate) fn contains_source(catalog: &ProjectEffectCatalog, source: &Path) -> bool {
    match (catalog.root().canonicalize(), source.canonicalize()) {
        (Ok(root), Ok(source)) => source.starts_with(root),
        _ => false,
    }
}

pub(crate) fn open_folder(
    session: &mut EditorSession,
    catalog: &mut ProjectEffectCatalog,
    folder: &Path,
) -> Result<(), String> {
    let candidate = catalog_for_folder(folder)?;
    session.new_effect();
    *catalog = candidate;
    Ok(())
}

/// External files use the nearest conventional asset root, otherwise their own directory.
/// Arbitrary ancestors are never scanned implicitly; users can choose a wider project explicitly.
pub(crate) fn folder_for_source(source: &Path) -> Result<PathBuf, String> {
    let source = source.canonicalize().map_err(|error| error.to_string())?;
    let parent = source.parent().ok_or("Effect has no parent folder")?;
    if let Some(assets) = parent.ancestors().find(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("assets"))
    }) {
        return Ok(assets.to_owned());
    }
    if parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("effects"))
    {
        return Ok(parent.parent().unwrap_or(parent).to_owned());
    }
    Ok(parent.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_a_project_resets_the_document_and_invalid_folders_preserve_it() {
        let temporary = tempfile::tempdir().unwrap();
        let mut session = crate::test_support::session_with_timing_slack();
        let mut catalog = ProjectEffectCatalog::default();
        let original = session.effect.clone();
        let root = catalog.root().to_owned();
        assert!(
            open_folder(
                &mut session,
                &mut catalog,
                &temporary.path().join("missing")
            )
            .is_err()
        );
        assert_eq!(session.effect, original);
        assert_eq!(catalog.root(), root);
        open_folder(&mut session, &mut catalog, temporary.path()).unwrap();
        assert!(session.source_path.is_none());
        assert!(session.dirty);
        assert_ne!(session.effect.id, original.id);
        assert_eq!(catalog.root(), temporary.path().canonicalize().unwrap());
    }

    #[test]
    fn project_folders_and_external_effects_use_the_same_asset_root() {
        let directory = tempfile::tempdir().unwrap();
        let effects = directory.path().join("assets/effects/nested");
        std::fs::create_dir_all(&effects).unwrap();
        let source = effects.join("effect.aestra.ron");
        std::fs::write(&source, "placeholder").unwrap();
        let catalog = catalog_for_folder(directory.path()).unwrap();
        assert_eq!(folder_for_source(&source).unwrap(), catalog.root());
        assert!(contains_source(&catalog, &source));
        assert_eq!(catalog.effect_root(), catalog.root().join("effects"));
        assert!(catalog_for_folder(&source).is_err());
    }
}
