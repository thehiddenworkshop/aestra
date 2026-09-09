//! Exact directory inventories for exclusive folder relocation. No recursive delete,
//! merge, cross-project moves or silently skipped metadata/hidden descendants.
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct FolderMove {
    pub source: PathBuf,
    pub destination: PathBuf,
    /// Original root-relative directory paths, including the selected root and empties.
    pub directories: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Node {
    Directory,
    File(Vec<u8>),
}

pub(super) fn key(path: &Path) -> PathBuf {
    path.to_string_lossy()
        .replace('\\', "/")
        .to_lowercase()
        .into()
}

pub(super) fn inventory(root: &Path) -> Result<BTreeSet<PathBuf>, OperationError> {
    let tree = ProjectSourceTree::scan(root);
    let mut directories = BTreeSet::new();
    for entry in tree.entries() {
        if entry.error.is_some() {
            return Err(blocked("Directory inventory is incomplete"));
        }
        if entry.kind == ProjectSourceKind::Directory {
            directories.insert(entry.path.clone());
        }
    }
    Ok(directories)
}

/// Missing descendants of a relocated folder are valid; existing ancestors are
/// checked individually so a link or case-only collision is never treated as absence.
pub(super) fn read_node(root: &Path, relative: &Path) -> Result<Option<Node>, OperationError> {
    checked_parent(root, root)?;
    let mut path = root.to_owned();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(blocked("Invalid transaction path"));
        };
        path.push(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                vacant(path.parent().unwrap(), &path)?;
                return Ok(None);
            }
            Err(e) => return Err(e.into()),
        };
        if super::super::super::source_tree::is_link(&metadata) || metadata.permissions().readonly()
        {
            return Err(blocked("Transaction path is linked or read-only"));
        }
        if components.peek().is_some() {
            if !metadata.is_dir() {
                return Err(blocked("Transaction parent is not a directory"));
            }
        } else if metadata.is_dir() {
            return Ok(Some(Node::Directory));
        } else if metadata.is_file() {
            return Ok(Some(Node::File(fs::read(&path)?)));
        } else {
            return Err(blocked("Unsupported transaction filesystem entry"));
        }
    }
    Err(blocked("Cannot relocate the project root"))
}

/// Unlike project discovery, this deliberately sees excluded and hidden entries.
/// A folder containing any such entry cannot be authorized by a partial inventory.
pub(super) fn scan(root: &Path, base: &Path) -> Result<BTreeMap<PathBuf, Node>, OperationError> {
    let mut pending = vec![base.to_owned()];
    let mut result = BTreeMap::new();
    while let Some(path) = pending.pop() {
        validate_path(&path)?;
        let Some(node) = read_node(root, &path)? else {
            continue;
        };
        if node == Node::Directory {
            for entry in fs::read_dir(root.join(&path))? {
                pending.push(path.join(entry?.file_name()));
            }
        }
        result.insert(path, node);
        if result.len() + pending.len() > MAX_FILES * 2 {
            return Err(blocked("Folder inventory exceeds 256 entries"));
        }
    }
    Ok(result)
}

pub(super) fn validate(folders: &[FolderMove], moves: &[Move]) -> Result<(), OperationError> {
    if folders.len() > MAX_FILES || (folders.is_empty() && moves.is_empty()) {
        return Err(blocked("Invalid relocation batch size"));
    }
    let mut roots = Vec::new();
    let mut directory_count = 0;
    for folder in folders {
        for path in [&folder.source, &folder.destination] {
            validate_path(path)?;
            let path = key(path);
            if roots
                .iter()
                .any(|other: &PathBuf| path.starts_with(other) || other.starts_with(&path))
            {
                return Err(blocked(
                    "Overlapping folders, case-only rename or self-descendant destination",
                ));
            }
            roots.push(path);
        }
        let mut directories = BTreeSet::new();
        for path in &folder.directories {
            validate_path(path)?;
            if !path.starts_with(&folder.source) || !directories.insert(key(path)) {
                return Err(blocked("Invalid folder directory inventory"));
            }
        }
        if !directories.contains(&key(&folder.source)) {
            return Err(blocked("Missing folder root"));
        }
        for path in &folder.directories {
            if path != &folder.source && !directories.contains(&key(path.parent().unwrap())) {
                return Err(blocked("Missing folder parent"));
            }
        }
        directory_count += directories.len();
        for item in moves
            .iter()
            .filter(|item| key(&item.source).starts_with(key(&folder.source)))
        {
            let suffix = item
                .source
                .strip_prefix(&folder.source)
                .map_err(|_| blocked("Case-ambiguous folder mapping"))?;
            if suffix.as_os_str().is_empty()
                || item.destination != folder.destination.join(suffix)
                || !directories.contains(&key(item.source.parent().unwrap()))
                || directories.contains(&key(&item.source))
            {
                return Err(blocked("File is not a valid folder descendant relocation"));
            }
        }
    }
    if directory_count > MAX_FILES {
        return Err(blocked("At most 128 directories can be relocated"));
    }
    for item in moves {
        let containing = folders
            .iter()
            .find(|folder| item.source.starts_with(&folder.source));
        if containing.is_none() {
            for path in [&item.source, &item.destination] {
                if roots
                    .iter()
                    .any(|root| key(path).starts_with(root) || root.starts_with(key(path)))
                {
                    return Err(blocked("File selection overlaps a folder relocation"));
                }
            }
        }
    }
    Ok(())
}
