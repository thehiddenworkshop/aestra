//! Project location and document-boundary operations, independent of file dialogs and widgets.

use crate::project_content::EditorProjectContent as ProjectEffectCatalog;
use crate::session::EditorSession;
use std::path::{Path, PathBuf};

/// Creation never merges with an existing project or copies the bundled examples.
#[derive(Clone, Debug)]
pub(crate) struct CreateProjectRequest {
    pub(crate) name: String,
    pub(crate) parent: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CreateProjectError {
    InvalidName,
    InvalidLocation,
    DestinationExists,
    Io(String),
}

impl CreateProjectRequest {
    pub(crate) fn destination(&self) -> PathBuf {
        self.parent.join(&self.name)
    }

    pub(crate) fn validate(&self) -> Result<(), CreateProjectError> {
        let stem = self
            .name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if self.name.is_empty()
            || self.name.trim() != self.name
            || self.name.starts_with('.')
            || self.name.ends_with('.')
            || self
                .name
                .chars()
                .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(CreateProjectError::InvalidName);
        }
        if !self.parent.is_absolute() {
            return Err(CreateProjectError::InvalidLocation);
        }
        Ok(())
    }
}

pub(crate) fn create_project(
    request: &CreateProjectRequest,
) -> Result<ProjectEffectCatalog, CreateProjectError> {
    create_project_with(request, |path| std::fs::create_dir(path))
}

fn create_project_with(
    request: &CreateProjectRequest,
    mut mkdir: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<ProjectEffectCatalog, CreateProjectError> {
    use std::fs;
    request.validate()?;
    aestra_project::ProjectSourceTree::validate_root(&request.parent)
        .map_err(|_| CreateProjectError::InvalidLocation)?;
    let parent = request
        .parent
        .canonicalize()
        .map_err(|_| CreateProjectError::InvalidLocation)?;
    if !parent.is_dir() {
        return Err(CreateProjectError::InvalidLocation);
    }
    let destination = parent.join(&request.name);
    let io_error = |error: std::io::Error| CreateProjectError::Io(error.to_string());
    for entry in fs::read_dir(&parent).map_err(io_error)? {
        if entry
            .map_err(io_error)?
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            == request.name.to_lowercase()
        {
            return Err(CreateProjectError::DestinationExists);
        }
    }
    // create_dir atomically rejects late collisions, including an empty destination folder.
    aestra_project::ProjectSourceTree::validate_root(&parent).map_err(CreateProjectError::Io)?;
    if parent.canonicalize().map_err(io_error)? != parent {
        return Err(CreateProjectError::InvalidLocation);
    }
    mkdir(&destination).map_err(|error| {
        if error.kind() == std::io::ErrorKind::AlreadyExists {
            CreateProjectError::DestinationExists
        } else {
            io_error(error)
        }
    })?;
    let mut created = vec![destination.clone()];
    let result = (|| {
        for relative in [
            "assets",
            "assets/effects",
            "assets/materials",
            "assets/meshes",
            "assets/shaders",
            "assets/textures",
        ] {
            let path = destination.join(relative);
            let owner = path.parent().expect("project child has a parent");
            aestra_project::ProjectSourceTree::validate_root(owner)
                .map_err(CreateProjectError::Io)?;
            if owner.canonicalize().map_err(io_error)? != owner {
                return Err(CreateProjectError::Io(
                    "Project destination changed during creation".into(),
                ));
            }
            mkdir(&path).map_err(io_error)?;
            created.push(path);
        }
        catalog_for_folder(&destination).map_err(CreateProjectError::Io)
    })();
    if result.is_err() {
        // Only remove the empty directories created by this operation. Never remove a tree,
        // an existing destination, or files someone added during creation.
        for path in created.iter().rev() {
            if aestra_project::ProjectSourceTree::validate_root(path).is_ok()
                && path
                    .canonicalize()
                    .is_ok_and(|canonical| canonical == *path)
            {
                let _ = fs::remove_dir(path);
            }
        }
    }
    result
}

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

    fn request(parent: &Path, name: &str) -> CreateProjectRequest {
        CreateProjectRequest {
            name: name.into(),
            parent: parent.to_owned(),
        }
    }

    #[test]
    fn creation_is_empty_conventional_and_reopenable() {
        let parent = tempfile::tempdir().unwrap();
        let request = request(parent.path(), "My VFX");
        let catalog = create_project(&request).unwrap();
        let root = request.destination().canonicalize().unwrap();
        assert_eq!(catalog.root(), root.join("assets"));
        assert_eq!(catalog.effect_root(), root.join("assets/effects"));
        for folder in ["effects", "materials", "meshes", "shaders", "textures"] {
            assert!(root.join("assets").join(folder).is_dir());
        }
        assert!(catalog.entries().is_empty());
        assert_eq!(catalog_for_folder(&root).unwrap().root(), catalog.root());
    }

    #[test]
    fn invalid_names_and_locations_never_write() {
        let parent = tempfile::tempdir().unwrap();
        for name in [
            "",
            ".",
            "..",
            "../escape",
            "nested/name",
            "bad\\name",
            " name",
            "name ",
            "name.",
            ".hidden",
            "CON",
            "nul.txt",
            "LPT9",
            "COM1.foo",
            "a:b",
            "a?b",
            "a\nb",
        ] {
            assert!(
                matches!(
                    create_project(&request(parent.path(), name)),
                    Err(CreateProjectError::InvalidName)
                ),
                "{name}"
            );
        }
        assert!(std::fs::read_dir(parent.path()).unwrap().next().is_none());
        assert!(matches!(
            create_project(&request(Path::new("relative"), "Project")),
            Err(CreateProjectError::InvalidLocation)
        ));
        assert!(matches!(
            create_project(&request(&parent.path().join("missing"), "Project")),
            Err(CreateProjectError::InvalidLocation)
        ));
        std::fs::write(parent.path().join("file"), "untouched").unwrap();
        assert!(matches!(
            create_project(&request(&parent.path().join("file"), "Project")),
            Err(CreateProjectError::InvalidLocation)
        ));
    }

    #[test]
    fn existing_and_case_only_destinations_are_never_merged() {
        let parent = tempfile::tempdir().unwrap();
        let existing = parent.path().join("Existing");
        std::fs::create_dir(&existing).unwrap();
        std::fs::write(existing.join("keep.txt"), "keep").unwrap();
        for name in ["Existing", "existing"] {
            assert!(matches!(
                create_project(&request(parent.path(), name)),
                Err(CreateProjectError::DestinationExists)
            ));
        }
        assert_eq!(
            std::fs::read_to_string(existing.join("keep.txt")).unwrap(),
            "keep"
        );
        assert!(!existing.join("assets").exists());
        std::fs::write(parent.path().join("File"), "keep").unwrap();
        assert!(matches!(
            create_project(&request(parent.path(), "File")),
            Err(CreateProjectError::DestinationExists)
        ));
    }

    #[test]
    fn late_collision_and_failed_creation_preserve_foreign_content() {
        let parent = tempfile::tempdir().unwrap();
        let request = request(parent.path(), "Project");
        let result = create_project_with(&request, |path| {
            if path.file_name().unwrap() == "Project" {
                std::fs::create_dir(path)?;
                std::fs::write(path.join("keep.txt"), "keep")?;
            }
            std::fs::create_dir(path)
        });
        assert!(matches!(result, Err(CreateProjectError::DestinationExists)));
        assert_eq!(
            std::fs::read_to_string(request.destination().join("keep.txt")).unwrap(),
            "keep"
        );
        let other = CreateProjectRequest {
            name: "Failed".into(),
            ..request.clone()
        };
        let result = create_project_with(&other, |path| {
            if path.file_name().unwrap() == "materials" {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected write failure",
                ));
            }
            std::fs::create_dir(path)
        });
        assert!(matches!(result, Err(CreateProjectError::Io(_))));
        assert!(!other.destination().exists());
        let result = create_project_with(&other, |path| {
            if path.file_name().unwrap() == "materials" {
                std::fs::write(path.parent().unwrap().join("keep.txt"), "foreign")?;
                return Err(std::io::Error::other("injected write failure"));
            }
            std::fs::create_dir(path)
        });
        assert!(matches!(result, Err(CreateProjectError::Io(_))));
        assert_eq!(
            std::fs::read_to_string(other.destination().join("assets/keep.txt")).unwrap(),
            "foreign"
        );
        assert!(!other.destination().join("assets/effects").exists());
    }

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
