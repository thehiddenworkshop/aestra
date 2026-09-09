//! Publish replacements with two exclusive renames, never overwrite-in-place.
//! A crash between detaching the original and publishing its replacement is an
//! explicit journal state. Forward apply and reverse recovery traverse the same
//! sequence; exact bytes at every touched path must match one valid prefix.
use super::*;

pub(super) struct Step {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub bytes: Vec<u8>,
    pub folder: bool,
}

pub(super) fn internal(path: &Path) -> bool {
    path.starts_with(".aestra/asset-transactions")
}

pub(super) fn final_path<'a>(source: &'a Path, moves: &'a [Move]) -> &'a Path {
    moves
        .iter()
        .find(|item| item.source == source)
        .map_or(source, |item| item.destination.as_path())
}

fn sidecar(record: &Record, index: usize, suffix: &str) -> PathBuf {
    Path::new(".aestra/asset-transactions").join(format!("{}-{index}.{suffix}", record.archive))
}

pub(super) fn list(record: &Record) -> Vec<Step> {
    let mut steps: Vec<_> = record
        .folders
        .iter()
        .map(|folder| Step {
            source: folder.source.clone(),
            destination: folder.destination.clone(),
            bytes: Vec::new(),
            folder: true,
        })
        .collect();
    steps.extend(
        record
            .moves
            .iter()
            .filter(|item| {
                !record
                    .folders
                    .iter()
                    .any(|folder| item.source.starts_with(&folder.source))
            })
            .map(|item| Step {
                source: item.source.clone(),
                destination: item.destination.clone(),
                bytes: item.bytes.clone(),
                folder: false,
            }),
    );
    for (index, item) in record.replacements.iter().enumerate() {
        let path = final_path(&item.source, &record.moves).to_owned();
        steps.push(Step {
            source: path.clone(),
            destination: sidecar(record, index, "original"),
            bytes: item.before.clone(),
            folder: false,
        });
        steps.push(Step {
            source: sidecar(record, index, "replacement"),
            destination: path,
            bytes: item.after.clone(),
            folder: false,
        });
    }
    steps
}

fn initial(record: &Record) -> BTreeMap<PathBuf, folders::Node> {
    let mut files: BTreeMap<_, _> = record
        .moves
        .iter()
        .map(|item| (item.source.clone(), folders::Node::File(item.bytes.clone())))
        .collect();
    for folder in &record.folders {
        for path in &folder.directories {
            files.insert(path.clone(), folders::Node::Directory);
        }
    }
    for (index, item) in record.replacements.iter().enumerate() {
        files.insert(
            item.source.clone(),
            folders::Node::File(item.before.clone()),
        );
        files.insert(
            sidecar(record, index, "replacement"),
            folders::Node::File(item.after.clone()),
        );
    }
    files
}

pub(super) fn stage(root: &Path, record: &Record) -> Result<(), OperationError> {
    for (index, item) in record.replacements.iter().enumerate() {
        let path = root.join(sidecar(record, index, "replacement"));
        vacant(path.parent().unwrap(), &path)?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(&item.after)?;
        file.sync_all()?;
    }
    if progress(root, record)? != 0 {
        return Err(blocked("Transaction changed while staging"));
    }
    Ok(())
}

pub(super) fn progress(root: &Path, record: &Record) -> Result<usize, OperationError> {
    let steps = list(record);
    let paths: BTreeSet<_> = steps
        .iter()
        .flat_map(|step| [&step.source, &step.destination])
        .collect();
    let mut actual = BTreeMap::new();
    for path in paths {
        if let Some(node) = folders::read_node(root, path)? {
            actual.insert(path.clone(), node);
        }
    }
    for folder in &record.folders {
        for path in [&folder.source, &folder.destination] {
            actual.extend(folders::scan(root, path)?);
        }
    }
    let mut expected = initial(record);
    for prefix in 0..=steps.len() {
        if actual == expected {
            return Ok(prefix);
        }
        if let Some(step) = steps.get(prefix) {
            if step.folder {
                if expected.get(&step.source) != Some(&folders::Node::Directory) {
                    return Err(blocked("Missing expected folder"));
                }
                shift(&mut expected, &step.source, &step.destination)?;
            } else if expected.remove(&step.source) != Some(folders::Node::File(step.bytes.clone()))
                || expected
                    .insert(
                        step.destination.clone(),
                        folders::Node::File(step.bytes.clone()),
                    )
                    .is_some()
            {
                return Err(blocked("Invalid transaction publication sequence"));
            }
        }
    }
    Err(blocked(
        "Transaction paths changed or are ambiguous; no unknown file will be overwritten",
    ))
}

pub(super) fn advance(
    root: &Path,
    record: &Record,
    index: usize,
    restore: bool,
) -> Result<(), OperationError> {
    if progress(root, record)? != index + usize::from(restore) {
        return Err(blocked("Transaction state changed before publication"));
    }
    let steps = list(record);
    let step = steps
        .get(index)
        .ok_or_else(|| blocked("Invalid transaction step"))?;
    let (source, destination) = if restore {
        (&step.destination, &step.source)
    } else {
        (&step.source, &step.destination)
    };
    rename::rename_exclusive(&root.join(source), &root.join(destination))
}

fn shift<T>(
    map: &mut BTreeMap<PathBuf, T>,
    source: &Path,
    destination: &Path,
) -> Result<(), OperationError> {
    let paths: Vec<_> = map
        .keys()
        .filter(|path| path.starts_with(source))
        .cloned()
        .collect();
    for path in paths {
        let target = destination.join(path.strip_prefix(source).unwrap());
        let value = map.remove(&path).unwrap();
        if map.insert(target, value).is_some() {
            return Err(blocked("Overlapping transaction destination"));
        }
    }
    Ok(())
}

pub(super) fn update_inventory(
    root: &Path,
    step: &Step,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
    directories: &mut BTreeSet<PathBuf>,
) -> Result<(), OperationError> {
    let source = root.join(&step.source);
    let destination = root.join(&step.destination);
    if step.folder {
        shift(files, &source, &destination)?;
        let paths: Vec<_> = directories
            .iter()
            .filter(|path| path.starts_with(&source))
            .cloned()
            .collect();
        for path in paths {
            directories.remove(&path);
            directories.insert(destination.join(path.strip_prefix(&source).unwrap()));
        }
    } else {
        if !internal(&step.source) {
            files.remove(&source);
        }
        if !internal(&step.destination) {
            files.insert(destination, step.bytes.clone());
        }
    }
    Ok(())
}

pub(super) fn moved_files(record: &Record, progress: usize) -> usize {
    list(record)
        .iter()
        .take(progress)
        .map(|step| {
            record
                .moves
                .iter()
                .filter(|item| {
                    if step.folder {
                        item.source.starts_with(&step.source)
                    } else {
                        item.source == step.source
                    }
                })
                .count()
        })
        .sum()
}
