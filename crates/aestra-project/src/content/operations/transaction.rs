//! Journaled batches of already-authorized semantic file moves. No folder or
//! arbitrary-path API: callers use source IDs and the shared relocation preflight.
//! Pending batches are inspected read-only; restart rollback is an explicit action.
use super::*;
use crate::ProjectAssetId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const MAX_FILES: usize = 128;
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    source: PathBuf,
    destination: PathBuf,
    asset: ProjectAssetId,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    root: PathBuf,
    archive: String,
    moves: Vec<Move>,
}

#[derive(Debug)]
pub struct AssetMoveBatchPlan {
    root: PathBuf,
    moves: Vec<Move>,
    inventory: BTreeMap<PathBuf, Vec<u8>>,
}

#[derive(Debug)]
pub struct AssetMoveBatchResult {
    pub moves: Vec<RenameResult>,
    /// Retained journal includes exact original bytes. Not semantic Ctrl+Z history.
    pub journal: PathBuf,
}

/// Opaque read-only inspection. The host must gather fresh drafts and recheck its
/// session guard before explicitly requesting rollback. No automatic destructive replay.
#[derive(Debug)]
pub struct PendingAssetMoveBatch {
    root: PathBuf,
    record: Record,
    pub journal: PathBuf,
    pub moved_files: usize,
}

impl ProjectContent {
    /// Plans a bounded batch of saved semantic files using the single-asset planner.
    /// Destinations must already exist. Renames, folder moves and path rewrites are
    /// intentionally not accepted yet. No asset files are changed by planning.
    pub fn plan_asset_moves(
        &self,
        requests: Vec<OperationRequest>,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<AssetMoveBatchPlan, OperationError> {
        if requests.is_empty() || requests.len() > MAX_FILES {
            return Err(blocked("A move batch must contain 1 to 128 assets"));
        }
        let root = self.source_tree().root_path().to_owned();
        ensure_idle(&root)?;
        let baseline = rename::inventory(&root)?;
        let mut moves = Vec::new();
        for request in requests {
            let plan = self.plan_asset_move(request, drafts, complete)?;
            if plan.inventory != baseline {
                return Err(blocked("Project changed during batch preflight"));
            }
            moves.push(Move {
                source: plan
                    .source
                    .strip_prefix(&root)
                    .map_err(|_| blocked("Source escapes root"))?
                    .into(),
                destination: plan
                    .destination
                    .destination
                    .strip_prefix(&root)
                    .map_err(|_| blocked("Destination escapes root"))?
                    .into(),
                asset: plan.asset,
                bytes: baseline
                    .get(&plan.source)
                    .ok_or_else(|| blocked("Missing source bytes"))?
                    .clone(),
            });
        }
        validate_moves(&moves)?;
        Ok(AssetMoveBatchPlan {
            root,
            moves,
            inventory: baseline,
        })
    }

    /// Inspect an interrupted batch without moving, replacing or deleting files.
    /// Malformed and ambiguous journals return an actionable error and remain intact.
    pub fn pending_asset_move_batch(
        &self,
    ) -> Result<Option<PendingAssetMoveBatch>, OperationError> {
        let root = self.source_tree().root_path().to_owned();
        let journal = directory(&root, false)?.join("active.pending");
        match fs::symlink_metadata(&journal) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let record = read_record(&root, &journal)?;
        let moved_files = states(&root, &record)?
            .into_iter()
            .filter(|moved| *moved)
            .count();
        Ok(Some(PendingAssetMoveBatch {
            root,
            record,
            journal,
            moved_files,
        }))
    }
}

impl AssetMoveBatchPlan {
    pub fn apply(self) -> Result<AssetMoveBatchResult, OperationError> {
        self.apply_with(|_| Ok(()))
    }

    fn apply_with(
        self,
        mut checkpoint: impl FnMut(usize) -> Result<(), OperationError>,
    ) -> Result<AssetMoveBatchResult, OperationError> {
        ensure_idle(&self.root)?;
        if rename::inventory(&self.root)? != self.inventory {
            return Err(blocked("Project changed; batch cancelled"));
        }
        validate_moves(&self.moves)?;
        for item in &self.moves {
            if state(&self.root, item)? {
                return Err(blocked("Source already moved; refresh first"));
            }
        }
        let (journal, record) = prepare(&self.root, self.moves)?;
        let mut expected = self.inventory;
        let result = (|| {
            checkpoint(0)?;
            for (index, item) in record.moves.iter().enumerate() {
                if rename::inventory(&self.root)? != expected {
                    return Err(blocked("Project changed during batch"));
                }
                move_one(&self.root, item, false)?;
                expected.remove(&self.root.join(&item.source));
                expected.insert(self.root.join(&item.destination), item.bytes.clone());
                checkpoint(index + 1)?;
            }
            if rename::inventory(&self.root)? != expected {
                return Err(blocked("Project changed before batch commit"));
            }
            finish(&self.root, &journal, &record, "complete")
        })();
        match result {
            Ok(journal) => Ok(AssetMoveBatchResult {
                moves: record
                    .moves
                    .into_iter()
                    .map(|item| RenameResult {
                        source: self.root.join(item.source),
                        destination: self.root.join(item.destination),
                        asset: item.asset,
                    })
                    .collect(),
                journal,
            }),
            Err(error) => match rollback(&self.root, &journal, &record, |_| Ok(())) {
                Ok(_) => Err(blocked(&format!(
                    "Batch cancelled and rolled back: {error}"
                ))),
                Err(recovery) => Err(blocked(&format!(
                    "Batch interrupted: {error}. Recovery required: {recovery}. Backup journal: {}",
                    journal.display()
                ))),
            },
        }
    }
}

impl PendingAssetMoveBatch {
    /// Explicitly restore originals only when *all* transaction paths still match
    /// known before/after states. An edit, collision, missing file or link blocks
    /// the entire attempt. Repeated recovery after an interruption is safe.
    /// For now all drafts must be saved/discarded before restart rollback.
    pub fn rollback(
        self,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<PathBuf, OperationError> {
        if !complete || !drafts.is_empty() {
            return Err(blocked(
                "Save or discard all drafts before restoring a move batch",
            ));
        }
        rollback(&self.root, &self.journal, &self.record, |_| Ok(()))
    }
}

fn validate_moves(moves: &[Move]) -> Result<(), OperationError> {
    if moves.is_empty() || moves.len() > MAX_FILES {
        return Err(blocked("Invalid batch size"));
    }
    let mut paths = BTreeSet::new();
    let mut assets = BTreeSet::new();
    for item in moves {
        if !assets.insert(item.asset) {
            return Err(blocked("An asset occurs more than once in this batch"));
        }
        for path in [&item.source, &item.destination] {
            if path.as_os_str().is_empty() {
                return Err(blocked("Empty transaction path"));
            }
            for component in path.components() {
                let std::path::Component::Normal(name) = component else {
                    return Err(blocked(
                        "Transaction paths must stay relative to the asset root",
                    ));
                };
                if !name.to_str().is_some_and(valid_name) {
                    return Err(blocked("Unsupported transaction path component"));
                }
            }
            if !paths.insert(path.to_string_lossy().replace('\\', "/").to_lowercase()) {
                return Err(blocked(
                    "Overlapping sources/destinations or case-only collision",
                ));
            }
        }
        let text =
            std::str::from_utf8(&item.bytes).map_err(|_| blocked("Invalid source backup"))?;
        let actual = match item.asset {
            ProjectAssetId::Effect(_) => aestra_core::EffectAsset::from_ron(text)
                .map(|asset| ProjectAssetId::Effect(asset.id))
                .map_err(|error| error.to_string()),
            ProjectAssetId::MaterialProgram(_) => {
                aestra_core::material::MaterialProgram::from_ron(text)
                    .map(|asset| ProjectAssetId::MaterialProgram(asset.id))
                    .map_err(|error| error.to_string())
            }
            ProjectAssetId::MaterialFunction(_) => {
                aestra_core::material::MaterialFunction::from_ron(text)
                    .map(|asset| ProjectAssetId::MaterialFunction(asset.id))
                    .map_err(|error| error.to_string())
            }
            _ => return Err(blocked("Unsupported transaction asset")),
        }
        .map_err(|error| blocked(&format!("Invalid transaction backup: {error}")))?;
        if actual != item.asset {
            return Err(blocked("Transaction backup identity mismatch"));
        }
    }
    Ok(())
}

fn directory(root: &Path, create: bool) -> Result<PathBuf, OperationError> {
    checked_parent(root, root)?;
    let mut path = root.to_owned();
    for part in [".aestra", "asset-transactions"] {
        path.push(part);
        if create {
            match fs::create_dir(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        match fs::symlink_metadata(&path) {
            Ok(_) => checked_parent(root, &path)?,
            Err(e) if !create && e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(root.join(".aestra/asset-transactions"));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}

pub(super) fn ensure_idle(root: &Path) -> Result<(), OperationError> {
    let pending = directory(root, false)?.join("active.pending");
    match fs::symlink_metadata(&pending) {
        Ok(_) => Err(blocked(&format!(
            "An interrupted asset transaction requires explicit recovery before more file operations: {}",
            pending.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn prepare(root: &Path, moves: Vec<Move>) -> Result<(PathBuf, Record), OperationError> {
    let directory = directory(root, true)?;
    let mut staging = tempfile::NamedTempFile::new_in(&directory)?;
    let archive = format!(
        "transaction-{}",
        staging
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .trim_start_matches('.')
    );
    let record = Record {
        version: 1,
        root: root.canonicalize()?,
        archive,
        moves,
    };
    let bytes = serde_json::to_vec(&record).map_err(|e| OperationError::Io(e.to_string()))?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(blocked("Transaction backup exceeds 64 MiB"));
    }
    staging.write_all(&bytes)?;
    staging.as_file().sync_all()?;
    let journal = directory.join("active.pending");
    staging
        .persist_noclobber(&journal)
        .map_err(|e| OperationError::Io(e.to_string()))?;
    Ok((journal, record))
}

fn read_record(root: &Path, journal: &Path) -> Result<Record, OperationError> {
    if journal != directory(root, false)?.join("active.pending") {
        return Err(blocked("Invalid journal location"));
    }
    let metadata = fs::symlink_metadata(journal)?;
    if !metadata.is_file()
        || super::super::source_tree::is_link(&metadata)
        || metadata.len() > MAX_JOURNAL_BYTES
    {
        return Err(blocked("Invalid or oversized transaction journal"));
    }
    let record: Record = serde_json::from_slice(&fs::read(journal)?)
        .map_err(|error| blocked(&format!("Unreadable transaction journal: {error}")))?;
    if record.version != 1
        || record.root != root.canonicalize()?
        || !record.archive.starts_with("transaction-")
        || !valid_name(&record.archive)
    {
        return Err(blocked("Unsupported or relocated transaction journal"));
    }
    validate_moves(&record.moves)?;
    Ok(record)
}

fn read_file(root: &Path, relative: &Path) -> Result<Option<Vec<u8>>, OperationError> {
    let path = root.join(relative);
    checked_parent(
        root,
        path.parent()
            .ok_or_else(|| blocked("Missing file parent"))?,
    )?;
    match fs::symlink_metadata(&path) {
        Ok(metadata)
            if metadata.is_file()
                && !super::super::source_tree::is_link(&metadata)
                && !metadata.permissions().readonly() =>
        {
            Ok(Some(fs::read(path)?))
        }
        Ok(_) => Err(blocked(
            "Transaction path is linked, read-only or not a regular file",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            vacant(path.parent().unwrap(), &path)?;
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

fn state(root: &Path, item: &Move) -> Result<bool, OperationError> {
    match (
        read_file(root, &item.source)?,
        read_file(root, &item.destination)?,
    ) {
        (Some(bytes), None) if bytes == item.bytes => Ok(false),
        (None, Some(bytes)) if bytes == item.bytes => Ok(true),
        _ => Err(blocked(&format!(
            "Transaction path changed or is ambiguous: {} → {}. No unknown file will be overwritten",
            item.source.display(),
            item.destination.display()
        ))),
    }
}

fn states(root: &Path, record: &Record) -> Result<Vec<bool>, OperationError> {
    record.moves.iter().map(|item| state(root, item)).collect()
}

fn move_one(root: &Path, item: &Move, restore: bool) -> Result<(), OperationError> {
    if state(root, item)? != restore {
        return Err(blocked("Transaction state changed before rename"));
    }
    let (source, destination) = if restore {
        (&item.destination, &item.source)
    } else {
        (&item.source, &item.destination)
    };
    rename::rename_exclusive(&root.join(source), &root.join(destination))
}

fn finish(
    root: &Path,
    journal: &Path,
    record: &Record,
    suffix: &str,
) -> Result<PathBuf, OperationError> {
    if read_record(root, journal)? != *record {
        return Err(blocked("Transaction journal changed"));
    }
    let destination = journal
        .parent()
        .unwrap()
        .join(format!("{}.{suffix}", record.archive));
    rename::rename_exclusive(journal, &destination)?;
    Ok(destination)
}

fn rollback(
    root: &Path,
    journal: &Path,
    record: &Record,
    mut checkpoint: impl FnMut(usize) -> Result<(), OperationError>,
) -> Result<PathBuf, OperationError> {
    if read_record(root, journal)? != *record {
        return Err(blocked("Transaction journal changed; rollback refused"));
    }
    // Validate every path before restoring even the first file. Never overwrite an
    // external edit to make the transaction look successful.
    let before = states(root, record)?;
    for (index, (item, moved)) in record.moves.iter().zip(before).enumerate().rev() {
        if moved {
            move_one(root, item, true)?;
            checkpoint(index)?;
        }
    }
    if states(root, record)?.into_iter().any(|moved| moved) {
        return Err(blocked("Incomplete rollback"));
    }
    finish(root, journal, record, "rolled-back")
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests;
