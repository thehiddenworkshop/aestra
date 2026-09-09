//! One exclusive file/folder rename into retained project recovery storage. The
//! immutable, synced journal precedes publication; restore never overwrites or purges.
use super::*;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Backup {
    version: u32,
    root: PathBuf,
    source: PathBuf,
    files: BTreeMap<PathBuf, Vec<u8>>,
    directories: BTreeSet<PathBuf>,
    assets: BTreeMap<PathBuf, ProjectAssetId>,
    required_assets: BTreeSet<ProjectAssetId>,
    required_files: BTreeSet<String>,
    /// Opaque host document state, retained atomically with the deleted files.
    #[serde(default)]
    recovery_data: Vec<u8>,
}

#[derive(Debug)]
pub struct DeletePlan {
    backup: Backup,
    inventory: BTreeMap<PathBuf, Vec<u8>>,
    directories: BTreeSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct DeletedSource {
    backup: Backup,
    journal: PathBuf,
    /// Original root-relative source, including filename extension.
    pub original: PathBuf,
    pub file_count: usize,
    pub folder_count: usize,
    /// False means interruption before removal (or after restoration); Restore
    /// verifies the original and archives the journal without removing anything.
    pub removed: bool,
    /// A collision or external edit blocks this entry, not the entire recovery list.
    pub blocked_reason: Option<String>,
}

impl DeletePlan {
    pub fn with_recovery_data(mut self, data: Vec<u8>) -> Self {
        self.backup.recovery_data = data;
        self
    }
    pub fn original(&self) -> &Path {
        &self.backup.source
    }
    pub fn file_count(&self) -> usize {
        self.backup.files.len()
    }
    pub fn folder_count(&self) -> usize {
        self.backup.directories.len()
    }
    pub fn apply(self) -> Result<DeletedSource, OperationError> {
        self.apply_with(|_| Ok(()))
    }
    fn apply_with(
        self,
        mut checkpoint: impl FnMut(usize) -> Result<(), OperationError>,
    ) -> Result<DeletedSource, OperationError> {
        ensure_idle(&self.backup.root)?;
        if rename::inventory(&self.backup.root)? != self.inventory
            || folders::inventory(&self.backup.root)? != self.directories
        {
            return Err(blocked("Project changed; deletion cancelled"));
        }
        let bytes = serde_json::to_vec(&self.backup).map_err(|e| blocked(&e.to_string()))?;
        if bytes.len() as u64 > MAX_JOURNAL_BYTES {
            return Err(blocked("Deletion backup exceeds 64 MiB"));
        }
        let directory = storage(&self.backup.root, true)?;
        let entry = tempfile::Builder::new()
            .prefix("entry-")
            .tempdir_in(directory)?
            .keep();
        let journal = entry.join("record.json");
        let mut staged = tempfile::NamedTempFile::new_in(&entry)?;
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged
            .persist_noclobber(&journal)
            .map_err(|e| OperationError::Io(e.to_string()))?;
        // Retain the journal even when publication fails. Its exact state is inspectable.
        let item = DeletedSource::read(&self.backup.root, &journal)?;
        item.check()?;
        checkpoint(0)?;
        if rename::inventory(&self.backup.root)? != self.inventory
            || folders::inventory(&self.backup.root)? != self.directories
        {
            return Err(blocked(
                "Project changed before deletion; inspect Deleted Items",
            ));
        }
        rename::rename_exclusive(&self.backup.root.join(&self.backup.source), &item.payload())
            .map_err(|e| {
                blocked(&format!(
                    "Delete interrupted: {e}. Recovery journal: {}",
                    journal.display()
                ))
            })?;
        checkpoint(1)?;
        let published = DeletedSource::read(&self.backup.root, &journal)?;
        published.check()?;
        Ok(published)
    }
}

impl ProjectContent {
    /// Preflight only. No files are removed until the opaque plan is applied.
    pub fn plan_delete_source(
        &self,
        source: ProjectSourceId,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<DeletePlan, OperationError> {
        if !complete {
            return Err(blocked(
                "Draft inventory is incomplete; deletion cannot be checked safely",
            ));
        }
        let root = self.source_tree().root_path().to_owned();
        if self.deleted_sources()?.len() >= 256 {
            return Err(blocked(
                "Restore deleted items before adding more recovery entries (limit 256)",
            ));
        }
        ensure_idle(&root)?;
        super::super::move_journal::recover(&root)?;
        let inventory = rename::inventory(&root)?;
        let directories = folders::inventory(&root)?;
        let fresh = ProjectContent::scan(&root);
        if fresh.source_relocation_suffix(source).is_none() {
            return Err(blocked("This source cannot be deleted safely"));
        }
        let entry = fresh
            .source(source)
            .ok_or_else(|| blocked("Missing source"))?;
        validate_path(&entry.relative_path)?;
        let report = fresh.reference_preflight_inner(source, drafts, true, true, true);
        if let Some(reason) = report.incomplete.first() {
            return Err(blocked(reason));
        }
        // Also prove physical resource references, including loader-owned #labels.
        references::plan(&fresh, &inventory, &[])?;
        let nodes = folders::scan(&root, &entry.relative_path)?;
        let mut backup = Backup {
            version: 1,
            root: root.clone(),
            source: entry.relative_path.clone(),
            files: BTreeMap::new(),
            directories: BTreeSet::new(),
            assets: BTreeMap::new(),
            required_assets: BTreeSet::new(),
            required_files: BTreeSet::new(),
            recovery_data: Vec::new(),
        };
        for (path, node) in nodes {
            match node {
                folders::Node::Directory => {
                    backup.directories.insert(path);
                }
                folders::Node::File(bytes) => {
                    let child = fresh
                        .source_tree()
                        .at_relative_path(&path)
                        .ok_or_else(|| blocked("Unindexed descendant"))?;
                    if let Some(asset) = fresh.asset_for_source(child.id) {
                        fresh
                            .unique_source_for_asset(asset)
                            .map_err(|_| blocked("Ambiguous source identity"))?;
                        backup.assets.insert(path.clone(), asset);
                    } else if !rename::non_referencing_bytes(&path, &bytes) {
                        return Err(blocked(
                            "Unsupported descendant or external include semantics",
                        ));
                    }
                    backup.files.insert(path, bytes);
                }
            }
        }
        validate(&backup)?;
        let projected = check_drafts(&fresh, drafts, &backup)?;
        let deleted: BTreeSet<_> = backup.assets.values().copied().collect();
        let file_keys: BTreeSet<_> = backup
            .files
            .keys()
            .map(|path| references::key(&path.to_string_lossy()))
            .collect::<Result<_, _>>()?;
        for owner in fresh.source_tree().entries() {
            if owner.relative_path.starts_with(&backup.source) {
                for dependency in fresh.source_relations(owner.id).dependencies {
                    match dependency.target {
                        crate::ProjectRelationTarget::Asset(id) if !deleted.contains(&id) => {
                            backup.required_assets.insert(id);
                        }
                        crate::ProjectRelationTarget::File(path) => {
                            let path = path.to_string_lossy();
                            let (file, _) = references::resource_parts(&path)?;
                            let key = references::key(file)?;
                            if !file_keys.contains(&key) {
                                backup.required_files.insert(key);
                            }
                        }
                        _ => {}
                    }
                }
                continue;
            }
            // Keep saved references safe as well as current unsaved references:
            // a draft must not authorize breaking the document still on disk.
            for dependency in fresh
                .source_relations(owner.id)
                .dependencies
                .into_iter()
                .chain(projected.source_relations(owner.id).dependencies)
            {
                let used = match dependency.target {
                    crate::ProjectRelationTarget::Asset(id) => deleted.contains(&id),
                    crate::ProjectRelationTarget::File(path) => {
                        let path = path.to_string_lossy();
                        let (file, _) = references::resource_parts(&path)?;
                        file_keys.contains(&references::key(file)?)
                    }
                    _ => false,
                };
                if used {
                    return Err(blocked(&format!(
                        "Delete blocked: referenced by {}. Repair or remove that reference first.",
                        owner.relative_path.display()
                    )));
                }
            }
        }
        if rename::inventory(&root)? != inventory || folders::inventory(&root)? != directories {
            return Err(blocked("Project changed during delete preflight"));
        }
        Ok(DeletePlan {
            backup,
            inventory,
            directories,
        })
    }

    /// Read-only recovery inventory, including interrupted pre-publication entries.
    pub fn deleted_sources(&self) -> Result<Vec<DeletedSource>, OperationError> {
        let root = self.source_tree().root_path();
        let directory = storage(root, false)?;
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            checked_parent(root, &path)?;
            let journal = path.join("record.json");
            if fs::symlink_metadata(&journal).is_ok() {
                result.push(DeletedSource::read(root, &journal)?);
            }
            if result.len() > 256 {
                return Err(blocked("Recovery inventory exceeds 256 entries"));
            }
        }
        result.sort_by(|a, b| a.original.cmp(&b.original));
        Ok(result)
    }
}

impl DeletedSource {
    pub fn recovery_data(&self) -> &[u8] {
        &self.backup.recovery_data
    }
    /// Prepare Redo only for the exact original restored by this recovery record.
    /// Fresh dependency/draft checks prevent deleting a replacement or a new usage.
    pub fn plan_redo(
        &self,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<DeletePlan, OperationError> {
        let archived = self.journal.with_file_name("record.restored");
        let restored = Self::read(&self.backup.root, &archived)?;
        if restored.check()? || restored.backup != self.backup {
            return Err(blocked(
                "Redo blocked: restored source or recovery record changed",
            ));
        }
        let content = ProjectContent::scan(&self.backup.root);
        let source = content
            .source_tree()
            .at_relative_path(&self.original)
            .ok_or_else(|| blocked("Redo blocked: source moved or disappeared"))?
            .id;
        let plan = content.plan_delete_source(source, drafts, complete)?;
        if plan.backup.files != self.backup.files
            || plan.backup.directories != self.backup.directories
            || plan.backup.assets != self.backup.assets
        {
            return Err(blocked("Redo blocked: restored contents changed"));
        }
        Ok(plan.with_recovery_data(self.backup.recovery_data.clone()))
    }

    pub fn journal(&self) -> &Path {
        &self.journal
    }
    fn payload(&self) -> PathBuf {
        self.journal.parent().unwrap().join("payload")
    }
    fn read(root: &Path, journal: &Path) -> Result<Self, OperationError> {
        let entry = journal
            .parent()
            .ok_or_else(|| blocked("Invalid recovery entry"))?;
        if entry.parent() != Some(storage(root, false)?.as_path())
            || !matches!(
                journal.file_name().and_then(|v| v.to_str()),
                Some("record.json" | "record.restored")
            )
            || !entry
                .file_name()
                .and_then(|v| v.to_str())
                .is_some_and(|v| v.starts_with("entry-") && valid_name(v))
        {
            return Err(blocked("Invalid recovery location"));
        }
        checked_parent(root, entry)?;
        let metadata = fs::symlink_metadata(journal)?;
        if !metadata.is_file()
            || super::super::super::source_tree::is_link(&metadata)
            || metadata.len() > MAX_JOURNAL_BYTES
        {
            return Err(blocked("Invalid recovery journal"));
        }
        let mut backup: Backup = serde_json::from_slice(&fs::read(journal)?)
            .map_err(|e| blocked(&format!("Unreadable recovery journal: {e}")))?;
        if backup.root.canonicalize()? != root.canonicalize()? {
            return Err(blocked("Recovery belongs to another project"));
        }
        // Operations use the trusted caller's spelling, not a path supplied by the
        // journal. Windows canonicalization adds a verbatim prefix to that path.
        backup.root = root.to_owned();
        validate(&backup)?;
        let mut result = Self {
            original: backup.source.clone(),
            file_count: backup.files.len(),
            folder_count: backup.directories.len(),
            removed: false,
            blocked_reason: None,
            backup,
            journal: journal.into(),
        };
        match result.check() {
            Ok(removed) => result.removed = removed,
            Err(error) => {
                result.removed = fs::symlink_metadata(result.payload()).is_ok();
                result.blocked_reason = Some(error.to_string());
            }
        }
        Ok(result)
    }

    /// Inspect both locations; never replace collisions or accept edited recovery bytes.
    fn check(&self) -> Result<bool, OperationError> {
        let original = self.backup.root.join(&self.original);
        let payload = self.payload();
        checked_parent(&self.backup.root, original.parent().unwrap())?;
        checked_parent(&self.backup.root, payload.parent().unwrap())?;
        let source_exists = fs::symlink_metadata(&original).is_ok();
        let payload_exists = fs::symlink_metadata(&payload).is_ok();
        if source_exists == payload_exists {
            return Err(blocked(
                "Restore blocked: collision or missing recovery payload",
            ));
        }
        if !source_exists {
            vacant(original.parent().unwrap(), &original)?;
        }
        if !payload_exists {
            vacant(payload.parent().unwrap(), &payload)?;
        }
        let actual = scan_payload(
            &self.backup.root,
            if payload_exists { &payload } else { &original },
            &self.original,
        )?;
        let expected: BTreeMap<_, _> = self
            .backup
            .files
            .iter()
            .map(|(path, bytes)| (path.clone(), folders::Node::File(bytes.clone())))
            .chain(
                self.backup
                    .directories
                    .iter()
                    .map(|path| (path.clone(), folders::Node::Directory)),
            )
            .collect();
        if actual != expected {
            return Err(blocked(
                "Recovery contents changed; nothing was overwritten",
            ));
        }
        Ok(payload_exists)
    }

    pub fn restore(
        self,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<PathBuf, OperationError> {
        if !complete {
            return Err(blocked(
                "Draft inventory is incomplete; restoration cannot be checked safely",
            ));
        }
        ensure_idle(&self.backup.root)?;
        let fresh = Self::read(&self.backup.root, &self.journal)?;
        fresh.check()?;
        if fresh.backup != self.backup {
            return Err(blocked("Recovery journal changed"));
        }
        check_drafts(
            &ProjectContent::scan(&self.backup.root),
            drafts,
            &self.backup,
        )?;
        vacant(
            self.journal.parent().unwrap(),
            &self.journal.with_file_name("record.restored"),
        )?;
        if fresh.removed {
            let content = ProjectContent::scan(&self.backup.root);
            for asset in &self.backup.required_assets {
                if content.unique_source_for_asset(*asset).is_err() {
                    return Err(blocked(
                        "Restore blocked: restore missing or ambiguous dependencies first",
                    ));
                }
            }
            let inventory = rename::inventory(&self.backup.root)?;
            let files: BTreeSet<_> = inventory
                .keys()
                .map(|path| {
                    let relative = path
                        .strip_prefix(&self.backup.root)
                        .map_err(|_| blocked("Foreign dependency path"))?;
                    references::key(&relative.to_string_lossy())
                })
                .collect::<Result<_, OperationError>>()?;
            if !self.backup.required_files.is_subset(&files) {
                return Err(blocked(
                    "Restore blocked: a required resource was removed or moved. Restore its original location first",
                ));
            }
            for asset in self.backup.assets.values() {
                if content
                    .source_tree()
                    .entries()
                    .any(|entry| content.asset_for_source(entry.id) == Some(*asset))
                {
                    return Err(blocked(
                        "Restore blocked: an asset with this identity already exists",
                    ));
                }
            }
            rename::rename_exclusive(&self.payload(), &self.backup.root.join(&self.original))?;
        }
        if self.check()? {
            return Err(blocked("Incomplete restore"));
        }
        rename::rename_exclusive(
            &self.journal,
            &self.journal.with_file_name("record.restored"),
        )?;
        Ok(self.backup.root.join(self.original))
    }
}

/// Preserve unrelated drafts; reject drafts owned by the deleted/restored source.
/// The projected relations additionally expose references introduced only in memory.
fn check_drafts(
    content: &ProjectContent,
    drafts: &[(ProjectSourceId, DraftDocument)],
    backup: &Backup,
) -> Result<ProjectContent, OperationError> {
    let mut projected = content.clone();
    let mut seen = BTreeSet::new();
    for (owner, draft) in drafts {
        let (asset, document) = match draft {
            DraftDocument::Effect(value) => (
                ProjectAssetId::Effect(value.id),
                ProjectSourceDocument::Effect(value.clone()),
            ),
            DraftDocument::Program(value) => (
                ProjectAssetId::MaterialProgram(value.id),
                ProjectSourceDocument::MaterialProgram(value.clone()),
            ),
            DraftDocument::Function(value) => (
                ProjectAssetId::MaterialFunction(value.id),
                ProjectSourceDocument::MaterialFunction(value.clone()),
            ),
        };
        if backup.assets.values().any(|id| *id == asset) {
            return Err(blocked(
                "This asset (or a folder item) has unsaved edits. Save or discard that draft first.",
            ));
        }
        let entry = content
            .source(*owner)
            .ok_or_else(|| blocked("Draft source is missing"))?;
        if entry.relative_path.starts_with(&backup.source) {
            return Err(blocked(
                "This folder contains unsaved edits. Save or discard those drafts first.",
            ));
        }
        if !seen.insert(*owner) || content.asset_for_source(*owner) != Some(asset) {
            return Err(blocked(
                "Draft source is duplicated or has a different identity",
            ));
        }
        projected.documents.insert(*owner, document);
    }
    projected.relations = crate::content::relations::RelationIndex::build(&projected.documents);
    Ok(projected)
}

fn storage(root: &Path, create: bool) -> Result<PathBuf, OperationError> {
    checked_parent(root, root)?;
    let mut path = root.to_owned();
    for part in [".aestra", "deleted"] {
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
                return Ok(root.join(".aestra/deleted"));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}

fn validate(backup: &Backup) -> Result<(), OperationError> {
    validate_path(&backup.source)?;
    for file in &backup.required_files {
        if references::key(file)? != *file {
            return Err(blocked("Invalid recovery dependency path"));
        }
    }
    if backup.version != 1
        || backup.files.len() > MAX_FILES
        || backup.directories.len() > MAX_FILES
        || (!backup.files.contains_key(&backup.source)
            && !backup.directories.contains(&backup.source))
    {
        return Err(blocked("Invalid deletion inventory"));
    }
    let mut ids = BTreeSet::new();
    for path in backup.directories.iter().chain(backup.files.keys()) {
        validate_path(path)?;
        if !path.starts_with(&backup.source)
            || backup
                .files
                .keys()
                .any(|file| path != file && path.starts_with(file))
        {
            return Err(blocked("Invalid deletion descendant"));
        }
        if path != &backup.source && !backup.directories.contains(path.parent().unwrap()) {
            return Err(blocked("Missing deletion parent"));
        }
    }
    for (path, bytes) in &backup.files {
        if backup.directories.contains(path) {
            return Err(blocked("Conflicting deletion entry"));
        }
        let asset = backup.assets.get(path).copied();
        if asset.is_some_and(|id| !ids.insert(id)) {
            return Err(blocked("Duplicate backup identity"));
        }
        validate_moves(&[Move {
            source: path.clone(),
            destination: PathBuf::from("restored").join(path),
            asset,
            bytes: bytes.clone(),
        }])?;
    }
    if backup
        .assets
        .keys()
        .any(|path| !backup.files.contains_key(path))
    {
        return Err(blocked("Unbacked asset identity"));
    }
    Ok(())
}

fn scan_payload(
    root: &Path,
    base: &Path,
    original: &Path,
) -> Result<BTreeMap<PathBuf, folders::Node>, OperationError> {
    let mut pending = vec![(base.to_owned(), original.to_owned())];
    let mut result = BTreeMap::new();
    let mut bytes = 0;
    while let Some((path, relative)) = pending.pop() {
        validate_path(&relative)?;
        checked_parent(root, path.parent().unwrap())?;
        let metadata = fs::symlink_metadata(&path)?;
        if super::super::super::source_tree::is_link(&metadata) || metadata.permissions().readonly()
        {
            return Err(blocked("Linked or read-only recovery content"));
        }
        let node = if metadata.is_dir() {
            for child in fs::read_dir(&path)? {
                let child = child?;
                pending.push((child.path(), relative.join(child.file_name())));
            }
            folders::Node::Directory
        } else if metadata.is_file() {
            bytes += metadata.len();
            if bytes > MAX_JOURNAL_BYTES {
                return Err(blocked("Recovery payload exceeds size limit"));
            }
            folders::Node::File(fs::read(&path)?)
        } else {
            return Err(blocked("Unsupported recovery content"));
        };
        result.insert(relative, node);
        if result.len() + pending.len() > MAX_FILES * 2 {
            return Err(blocked("Recovery payload exceeds entry limit"));
        }
    }
    Ok(result)
}
