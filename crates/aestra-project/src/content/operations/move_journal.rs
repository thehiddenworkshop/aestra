//! Recovery for a single no-replace, same-filesystem rename. We never replay a
//! destructive instruction from a journal: inspect the two atomic outcomes, then
//! acknowledge them. Ambiguous outcomes keep their byte backup for manual recovery.
use super::*;
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Serialize, Deserialize)]
struct Record {
    version: u32,
    source: PathBuf,
    destination: PathBuf,
    bytes: Vec<u8>,
}

fn directory(root: &Path, create: bool) -> Result<PathBuf, OperationError> {
    let mut path = root.to_owned();
    checked_parent(root, &path)?;
    for part in [".aestra", "asset-moves"] {
        path.push(part);
        if create {
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        if !create && !path.try_exists()? {
            return Ok(path);
        }
        checked_parent(root, &path)?;
    }
    Ok(path)
}

pub(super) fn prepare(
    root: &Path,
    source: &Path,
    destination: &Path,
) -> Result<PathBuf, OperationError> {
    let directory = directory(root, true)?;
    let record = Record {
        version: 1,
        source: source
            .strip_prefix(root)
            .map_err(|_| blocked("Source escapes root"))?
            .into(),
        destination: destination
            .strip_prefix(root)
            .map_err(|_| blocked("Destination escapes root"))?
            .into(),
        bytes: fs::read(source)?,
    };
    let mut staging = tempfile::NamedTempFile::new_in(&directory)?;
    serde_json::to_writer(staging.as_file_mut(), &record)
        .map_err(|e| OperationError::Io(e.to_string()))?;
    staging.flush()?;
    staging.as_file().sync_all()?;
    let pending = staging.path().with_extension("pending");
    staging
        .persist_noclobber(&pending)
        .map_err(|e| OperationError::Io(e.to_string()))?;
    Ok(pending)
}

pub(super) fn finish(pending: &Path) -> Result<(), OperationError> {
    super::rename::rename_exclusive(pending, &pending.with_extension("complete"))
}

fn read_candidate(root: &Path, relative: &Path) -> Result<Option<Vec<u8>>, OperationError> {
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str().eq_ignore_ascii_case(".aestra"))
    {
        return Err(blocked("Invalid recovery path; journal retained"));
    }
    let path = root.join(relative);
    checked_parent(
        root,
        path.parent()
            .ok_or_else(|| blocked("Missing recovery parent"))?,
    )?;
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !super::super::source_tree::is_link(&metadata) => {
            Ok(Some(fs::read(path)?))
        }
        Ok(_) => Err(blocked(
            "Recovery path is not a regular file; journal retained",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(super) fn recover(root: &Path) -> Result<usize, OperationError> {
    let directory = directory(root, false)?;
    if !directory.try_exists()? {
        return Ok(0);
    }
    let mut recovered = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "pending")
        {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if !metadata.is_file() || super::super::source_tree::is_link(&metadata) {
            return Err(blocked("Linked move journal; recovery blocked"));
        }
        let record: Record = serde_json::from_slice(&fs::read(entry.path())?)
            .map_err(|e| blocked(&format!("Unreadable move journal: {e}")))?;
        if record.version != 1 || record.source == record.destination {
            return Err(blocked("Unsupported move journal; recovery blocked"));
        }
        let source = read_candidate(root, &record.source)?;
        let destination = read_candidate(root, &record.destination)?;
        if !matches!((&source, &destination), (Some(bytes), None) | (None, Some(bytes)) if *bytes == record.bytes)
        {
            return Err(blocked(&format!(
                "Move recovery needs attention: {}. Files were not changed; backup retained at {}",
                record.source.display(),
                entry.path().display()
            )));
        }
        finish(&entry.path())?;
        recovered += 1;
    }
    Ok(recovered)
}

impl ProjectContent {
    /// Reconcile interrupted single-file moves without modifying asset files.
    /// Ambiguous/externally edited outcomes stay blocked, with their backup retained.
    pub fn recover_asset_moves(&self) -> Result<usize, OperationError> {
        super::transaction::ensure_idle(self.source_tree().root_path())?;
        recover(self.source_tree().root_path())
    }
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn interrupted_atomic_move_recovers_on_either_side_of_publication() {
        for publish in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = root.path().join("a.ron");
            let destination = root.path().join("b.ron");
            fs::write(&source, b"exact original bytes").unwrap();
            let journal = prepare(root.path(), &source, &destination).unwrap();
            if publish {
                super::super::rename::rename_exclusive(&source, &destination).unwrap();
            }
            assert_eq!(recover(root.path()).unwrap(), 1);
            assert!(!journal.exists());
            assert!(journal.with_extension("complete").exists());
            assert_eq!(source.exists(), !publish);
            assert_eq!(destination.exists(), publish);
            assert_eq!(recover(root.path()).unwrap(), 0);
        }
    }

    #[test]
    fn ambiguous_recovery_preserves_every_file_and_backup() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("a.ron");
        let destination = root.path().join("b.ron");
        fs::write(&source, b"original").unwrap();
        let journal = prepare(root.path(), &source, &destination).unwrap();
        fs::write(&destination, b"collision").unwrap();
        assert!(recover(root.path()).is_err());
        assert!(journal.exists());
        assert_eq!(fs::read(source).unwrap(), b"original");
        assert_eq!(fs::read(destination).unwrap(), b"collision");
    }

    #[test]
    fn untrusted_recovery_paths_cannot_escape_root() {
        let root = tempfile::tempdir().unwrap();
        for path in ["../escape", ".aestra/secret", "a/../b"] {
            assert!(read_candidate(root.path(), Path::new(path)).is_err());
        }
    }
}
