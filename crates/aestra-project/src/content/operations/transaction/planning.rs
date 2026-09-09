//! Source-ID-based folder/resource relocation plans. Publication is owned by the
//! same journal and typed reference rewriter as semantic file batches.
use super::*;

impl ProjectContent {
    /// Plans same-project Move/Rename requests for folders, saved semantic documents
    /// and proven non-referencing resource files. Rename names are filename stems
    /// (full names for directories). Destinations must exist and must not merge.
    /// Requires complete saved/discarded drafts; the host must recheck its session
    /// guard before apply and reload returned rewritten owners after publication.
    pub fn plan_content_relocations(
        &self,
        requests: Vec<OperationRequest>,
        drafts: &[(ProjectSourceId, DraftDocument)],
        complete: bool,
    ) -> Result<AssetMoveBatchPlan, OperationError> {
        if requests.is_empty() || requests.len() > MAX_FILES || !complete || !drafts.is_empty() {
            return Err(blocked(
                "Select 1 to 128 sources and save/discard all drafts before relocation",
            ));
        }
        let root = self.source_tree().root_path().to_owned();
        ensure_idle(&root)?;
        super::super::move_journal::recover(&root)?;
        let baseline = rename::inventory(&root)?;
        let directories = folders::inventory(&root)?;
        let fresh = ProjectContent::scan(&root);
        let mut moves = Vec::new();
        let mut folders = Vec::new();
        for request in requests {
            let (source, parent, name) = match request {
                OperationRequest::Move { source, parent } => (source, Some(parent), None),
                OperationRequest::Rename { source, name } => (source, None, Some(name)),
                _ => return Err(blocked("Expected a move or rename request")),
            };
            let entry = fresh
                .source(source)
                .ok_or_else(|| blocked("Source no longer exists"))?;
            if entry.relative_path.as_os_str().is_empty() {
                return Err(blocked("Cannot relocate the project root"));
            }
            validate_path(&entry.relative_path)?;
            let report = fresh.reference_preflight_inner(source, &[], true, true, true);
            if let Some(reason) = report.incomplete.first() {
                return Err(blocked(&format!("Relocation blocked: {reason}")));
            }
            let is_directory = entry.kind == ProjectSourceKind::Directory;
            let name = match name {
                Some(name) if valid_name(&name) => {
                    if is_directory {
                        name
                    } else {
                        format!("{name}{}", suffix(&fresh, source)?)
                    }
                }
                Some(_) => return Err(blocked("Use a portable filename stem")),
                None => entry
                    .name
                    .to_str()
                    .ok_or_else(|| blocked("Non-Unicode filename"))?
                    .to_owned(),
            };
            let destination = fresh
                .plan_operation(OperationRequest::CreateFolder {
                    parent: parent
                        .or(entry.parent)
                        .ok_or_else(|| blocked("Missing source parent"))?,
                    name,
                })?
                .destination;
            let destination = destination
                .strip_prefix(&root)
                .map_err(|_| blocked("Destination escapes root"))?
                .to_owned();
            if is_directory
                && (folders::key(&destination).starts_with(folders::key(&entry.relative_path))
                    || folders::key(&entry.relative_path).starts_with(folders::key(&destination)))
            {
                return Err(blocked("A folder cannot move into itself or an ancestor"));
            }
            let physical = folders::scan(&root, &entry.relative_path)?;
            let mut child_directories = Vec::new();
            for (path, node) in physical {
                let child = fresh
                    .source_tree()
                    .at_relative_path(&path)
                    .ok_or_else(|| blocked("Unindexed folder contents block relocation"))?;
                let previous = self
                    .source(child.id)
                    .ok_or_else(|| blocked("Source changed; refresh first"))?;
                if previous != child
                    || self.documents.get(&child.id) != fresh.documents.get(&child.id)
                {
                    return Err(blocked("Source changed; refresh first"));
                }
                match node {
                    folders::Node::Directory => child_directories.push(path),
                    folders::Node::File(bytes) => {
                        let asset = fresh.asset_for_source(child.id);
                        if let Some(asset) = asset {
                            fresh
                                .unique_source_for_asset(asset)
                                .map_err(|_| blocked("Ambiguous source identity"))?;
                        } else if !rename::non_referencing_bytes(&path, &bytes) {
                            return Err(blocked(
                                "Resource format or external references are not understood",
                            ));
                        }
                        let target = if is_directory {
                            destination.join(path.strip_prefix(&entry.relative_path).unwrap())
                        } else {
                            destination.clone()
                        };
                        moves.push(Move {
                            source: path,
                            destination: target,
                            asset,
                            bytes,
                        });
                    }
                }
            }
            if is_directory {
                folders.push(folders::FolderMove {
                    source: entry.relative_path.clone(),
                    destination,
                    directories: child_directories,
                });
            }
        }
        validate_moves(&moves)?;
        folders::validate(&folders, &moves)?;
        let replacements = references::plan(&fresh, &baseline, &moves)?;
        if rename::inventory(&root)? != baseline || folders::inventory(&root)? != directories {
            return Err(blocked("Project changed during relocation preflight"));
        }
        Ok(AssetMoveBatchPlan {
            root,
            moves,
            inventory: baseline,
            replacements,
            folders,
            directories,
        })
    }
}

fn suffix(content: &ProjectContent, source: ProjectSourceId) -> Result<String, OperationError> {
    if let Some(suffix) = content.asset_operation_suffix(source) {
        return Ok(suffix.into());
    }
    if matches!(
        content.documents.get(&source),
        Some(ProjectSourceDocument::MaterialPreset(_))
    ) {
        return Ok(".aestra.material-preset.ron".into());
    }
    content
        .source(source)
        .and_then(|entry| entry.path.extension())
        .and_then(|ext| ext.to_str())
        .map(|extension| format!(".{extension}"))
        .ok_or_else(|| blocked("Unsupported source suffix"))
}
