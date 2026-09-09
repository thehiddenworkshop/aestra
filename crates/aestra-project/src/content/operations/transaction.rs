//! Journaled source relocations and typed resource-path edits. Callers use source
//! IDs and shared preflight; no unchecked arbitrary-path mutation API is exposed.
//! Pending batches are inspected read-only; restart rollback is an explicit action.
use super::*;
use crate::ProjectAssetId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
mod folders;
mod planning;
mod references;
mod steps;
use references::Replacement;

const MAX_FILES: usize = 128;
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Move {
    source: PathBuf,
    destination: PathBuf,
    asset: Option<ProjectAssetId>,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    root: PathBuf,
    archive: String,
    moves: Vec<Move>,
    #[serde(default)]
    replacements: Vec<Replacement>,
    #[serde(default)]
    folders: Vec<folders::FolderMove>,
}

#[derive(Debug)]
pub struct AssetMoveBatchPlan {
    root: PathBuf,
    moves: Vec<Move>,
    inventory: BTreeMap<PathBuf, Vec<u8>>,
    replacements: Vec<Replacement>,
    folders: Vec<folders::FolderMove>,
    directories: BTreeSet<PathBuf>,
}

/// Location changes are shared by semantic assets, resource files and directories.
/// File-backed resources do not acquire invented semantic identities.
#[derive(Debug)]
pub struct SourceRelocation {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub asset: Option<ProjectAssetId>,
}

#[derive(Debug)]
pub struct AssetMoveBatchResult {
    pub moves: Vec<SourceRelocation>,
    pub folders: Vec<SourceRelocation>,
    /// Retained journal includes exact original bytes. Not semantic Ctrl+Z history.
    pub journal: PathBuf,
    /// Final paths of saved effects whose resource paths were rewritten. Hosts must
    /// reload these documents and refresh their source guards after publication.
    pub rewritten_sources: Vec<PathBuf>,
}

/// Opaque read-only inspection. The host must gather fresh drafts and recheck its
/// session guard before explicitly requesting rollback. No automatic destructive replay.
#[derive(Debug, Clone)]
pub struct PendingAssetMoveBatch {
    root: PathBuf,
    record: Record,
    pub journal: PathBuf,
    pub moved_files: usize,
    pub moved_folders: usize,
}

impl ProjectContent {
    /// Plans a bounded batch of saved semantic files using the single-asset planner.
    /// Destinations must already exist. Known effect resource paths are rewritten
    /// in the same transaction. For folders/resources/renames use
    /// `plan_content_relocations`. Hosts must recheck draft/session guards before apply.
    pub fn plan_asset_moves(
        &self,
        requests: Vec<OperationRequest>,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<AssetMoveBatchPlan, OperationError> {
        for request in &requests {
            let OperationRequest::Move { source, .. } = request else {
                return Err(blocked("Expected a semantic asset move request"));
            };
            if self.asset_operation_suffix(*source).is_none()
                || self.asset_for_source(*source).is_none()
            {
                return Err(blocked("Expected a supported semantic source"));
            }
        }
        self.plan_content_relocations(requests, drafts, complete)
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
        let progress = steps::progress(&root, &record)?;
        let moved_files = steps::moved_files(&record, progress);
        let moved_folders = progress.min(record.folders.len());
        Ok(Some(PendingAssetMoveBatch {
            root,
            record,
            journal,
            moved_files,
            moved_folders,
        }))
    }
}

impl AssetMoveBatchPlan {
    /// Saved documents affected by typed path edits, at their final locations.
    pub fn rewritten_sources(&self) -> Vec<PathBuf> {
        self.replacements
            .iter()
            .map(|item| self.root.join(steps::final_path(&item.source, &self.moves)))
            .collect()
    }

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
        if folders::inventory(&self.root)? != self.directories {
            return Err(blocked("Project directories changed; batch cancelled"));
        }
        validate_moves(&self.moves)?;
        folders::validate(&self.folders, &self.moves)?;
        references::validate(&self.replacements, &self.moves)?;
        let rewritten_sources = self.rewritten_sources();
        let (journal, record) = prepare(&self.root, self.moves, self.replacements, self.folders)?;
        let mut expected = self.inventory;
        let mut expected_directories = self.directories;
        let result = (|| {
            checkpoint(0)?;
            for (index, step) in steps::list(&record).iter().enumerate() {
                if rename::inventory(&self.root)? != expected
                    || folders::inventory(&self.root)? != expected_directories
                {
                    return Err(blocked("Project changed during batch"));
                }
                steps::advance(&self.root, &record, index, false)?;
                steps::update_inventory(
                    &self.root,
                    step,
                    &mut expected,
                    &mut expected_directories,
                )?;
                checkpoint(index + 1)?;
            }
            if rename::inventory(&self.root)? != expected
                || folders::inventory(&self.root)? != expected_directories
            {
                return Err(blocked("Project changed before batch commit"));
            }
            if steps::progress(&self.root, &record)? != steps::list(&record).len() {
                return Err(blocked("Incomplete transaction publication"));
            }
            finish(&self.root, &journal, &record, "complete")
        })();
        match result {
            Ok(journal) => Ok(AssetMoveBatchResult {
                moves: record
                    .moves
                    .into_iter()
                    .map(|item| SourceRelocation {
                        source: self.root.join(item.source),
                        destination: self.root.join(item.destination),
                        asset: item.asset,
                    })
                    .collect(),
                folders: record
                    .folders
                    .into_iter()
                    .map(|item| SourceRelocation {
                        source: self.root.join(item.source),
                        destination: self.root.join(item.destination),
                        asset: None,
                    })
                    .collect(),
                journal,
                rewritten_sources,
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
    if moves.len() > MAX_FILES {
        return Err(blocked("Invalid batch size"));
    }
    let mut paths = BTreeSet::new();
    let mut assets = BTreeSet::new();
    for item in moves {
        if let Some(asset) = item.asset
            && !assets.insert(asset)
        {
            return Err(blocked("An asset occurs more than once in this batch"));
        }
        for path in [&item.source, &item.destination] {
            validate_path(path)?;
            if !paths.insert(path.to_string_lossy().replace('\\', "/").to_lowercase()) {
                return Err(blocked(
                    "Overlapping sources/destinations or case-only collision",
                ));
            }
        }
        let Some(asset) = item.asset else {
            if !rename::non_referencing_bytes(&item.source, &item.bytes)
                || !rename::non_referencing_bytes(&item.destination, &item.bytes)
                || item.source.extension() != item.destination.extension()
            {
                return Err(blocked(
                    "Unsupported resource backup or changed resource format",
                ));
            }
            continue;
        };
        let text =
            std::str::from_utf8(&item.bytes).map_err(|_| blocked("Invalid source backup"))?;
        let actual = match asset {
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
            ProjectAssetId::MaterialPreset(_) => {
                aestra_core::material::MaterialPresetDescriptor::from_ron(text)
                    .map(|asset| ProjectAssetId::MaterialPreset(asset.id))
                    .map_err(|error| error.to_string())
            }
        }
        .map_err(|error| blocked(&format!("Invalid transaction backup: {error}")))?;
        if actual != asset {
            return Err(blocked("Transaction backup identity mismatch"));
        }
    }
    Ok(())
}

fn validate_path(path: &Path) -> Result<(), OperationError> {
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

fn prepare(
    root: &Path,
    moves: Vec<Move>,
    replacements: Vec<Replacement>,
    folders: Vec<folders::FolderMove>,
) -> Result<(PathBuf, Record), OperationError> {
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
        version: 3,
        root: root.canonicalize()?,
        archive,
        moves,
        replacements,
        folders,
    };
    let bytes = serde_json::to_vec(&record).map_err(|e| OperationError::Io(e.to_string()))?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(blocked("Transaction backup exceeds 64 MiB"));
    }
    staging.write_all(&bytes)?;
    staging.as_file().sync_all()?;
    // Stage and sync every rewritten document before the journal authorizes any
    // source mutation. Abandoned pre-journal staging files are harmless retained data.
    steps::stage(root, &record)?;
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
    if !matches!(record.version, 1..=3)
        || (record.version == 1 && !record.replacements.is_empty())
        || (record.version < 3
            && (!record.folders.is_empty() || record.moves.iter().any(|item| item.asset.is_none())))
        || record.root != root.canonicalize()?
        || !record.archive.starts_with("transaction-")
        || !valid_name(&record.archive)
    {
        return Err(blocked("Unsupported or relocated transaction journal"));
    }
    validate_moves(&record.moves)?;
    folders::validate(&record.folders, &record.moves)?;
    references::validate(&record.replacements, &record.moves)?;
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
    let before = steps::progress(root, record)?;
    for index in (0..before).rev() {
        steps::advance(root, record, index, true)?;
        checkpoint(index)?;
    }
    if steps::progress(root, record)? != 0 {
        return Err(blocked("Incomplete rollback"));
    }
    finish(root, journal, record, "rolled-back")
}

#[cfg(all(test, any(windows, target_os = "linux")))]
mod folder_tests;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod rewrite_tests;
#[cfg(all(test, any(windows, target_os = "linux")))]
mod tests;
