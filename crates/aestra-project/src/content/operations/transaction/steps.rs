//! Publish replacements with two exclusive renames, never overwrite-in-place.
//! A crash between detaching the original and publishing its replacement is an
//! explicit journal state. Forward apply and reverse recovery traverse the same
//! sequence; exact bytes at every touched path must match one valid prefix.
use super::*;

pub(super) struct Step {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub bytes: Vec<u8>,
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
        .moves
        .iter()
        .map(|item| Step {
            source: item.source.clone(),
            destination: item.destination.clone(),
            bytes: item.bytes.clone(),
        })
        .collect();
    for (index, item) in record.replacements.iter().enumerate() {
        let path = final_path(&item.source, &record.moves).to_owned();
        steps.push(Step {
            source: path.clone(),
            destination: sidecar(record, index, "original"),
            bytes: item.before.clone(),
        });
        steps.push(Step {
            source: sidecar(record, index, "replacement"),
            destination: path,
            bytes: item.after.clone(),
        });
    }
    steps
}

fn initial(record: &Record) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files: BTreeMap<_, _> = record
        .moves
        .iter()
        .map(|item| (item.source.clone(), item.bytes.clone()))
        .collect();
    for (index, item) in record.replacements.iter().enumerate() {
        files.insert(item.source.clone(), item.before.clone());
        files.insert(sidecar(record, index, "replacement"), item.after.clone());
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
        if let Some(bytes) = read_file(root, path)? {
            actual.insert(path.clone(), bytes);
        }
    }
    let mut expected = initial(record);
    for prefix in 0..=steps.len() {
        if actual == expected {
            return Ok(prefix);
        }
        if let Some(step) = steps.get(prefix)
            && (expected.remove(&step.source).as_ref() != Some(&step.bytes)
                || expected
                    .insert(step.destination.clone(), step.bytes.clone())
                    .is_some())
        {
            return Err(blocked("Invalid transaction publication sequence"));
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
