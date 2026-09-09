//! Typed path edits, never textual search/replace. Material texture values are IDs;
//! only effect resource declarations currently contain loader-owned file paths.
use super::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Replacement {
    pub source: PathBuf,
    pub before: Vec<u8>,
    pub after: Vec<u8>,
}

/// Separate a loader-owned subasset label from the physical project file. Bevy's
/// asset paths use the last `#` as the delimiter; keep the suffix opaque and exact
/// (including case and slashes). Only the file portion participates in disk checks.
/// Custom asset sources are still rejected by `key`, not treated as project files.
pub(super) fn resource_parts(path: &str) -> Result<(&str, &str), OperationError> {
    let Some(index) = path.rfind('#') else {
        return Ok((path, ""));
    };
    let (file, suffix) = path.split_at(index);
    if suffix.len() == 1 || suffix.contains("://") {
        return Err(blocked("Invalid resource subasset label"));
    }
    Ok((file, suffix))
}

/// Root-relative lexical key. Accept separators and dot aliases, but never foreign
/// roots, drive prefixes, metadata paths or escaping parents (including on Linux).
pub(super) fn key(path: &str) -> Result<String, OperationError> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') || path.contains(':') {
        return Err(blocked("Resource paths must be relative to the asset root"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| blocked("Resource path escapes asset root"))?;
            }
            part if valid_name(part) => parts.push(part.to_lowercase()),
            _ => return Err(blocked("Unsupported resource path component")),
        }
    }
    if parts.is_empty() {
        return Err(blocked("Empty resource path"));
    }
    Ok(parts.join("/"))
}

fn rewrite(
    bytes: &[u8],
    moves: &[Move],
) -> Result<(aestra_core::EffectAsset, bool), OperationError> {
    let text = std::str::from_utf8(bytes).map_err(|_| blocked("Invalid effect bytes"))?;
    let mut effect = aestra_core::EffectAsset::from_ron(text)
        .map_err(|e| blocked(&format!("Cannot rewrite effect: {e}")))?;
    let mut changed = false;
    for resource in &mut effect.assets {
        let (file, suffix) = resource_parts(&resource.path)?;
        let reference = key(file)?;
        for item in moves {
            if reference == key(&item.source.to_string_lossy())? {
                resource.path = format!(
                    "{}{suffix}",
                    item.destination.to_string_lossy().replace('\\', "/")
                );
                changed = true;
                break;
            }
        }
    }
    Ok((effect, changed))
}

/// Borrow only the loader-owned path tokens from the original RON. This preserves
/// comments, whitespace, unknown fields and strings elsewhere in the document.
/// The typed round-trip below is a second guard against changing the wrong field.
fn patch_paths(
    bytes: &[u8],
    expected: &aestra_core::EffectAsset,
) -> Result<Vec<u8>, OperationError> {
    #[derive(Deserialize)]
    #[serde(rename = "EffectAsset")]
    struct Paths<'a> {
        #[serde(borrow)]
        assets: Vec<Resource<'a>>,
    }
    #[derive(Deserialize)]
    #[serde(rename = "AssetDefinition")]
    struct Resource<'a> {
        #[serde(borrow)]
        path: &'a ron::value::RawValue,
    }
    let text = std::str::from_utf8(bytes).map_err(|_| blocked("Invalid effect bytes"))?;
    let paths: Paths<'_> = ron::from_str(text)
        .map_err(|e| blocked(&format!("Cannot locate resource path tokens: {e}")))?;
    if paths.assets.len() != expected.assets.len() {
        return Err(blocked("Resource field inventory mismatch"));
    }
    let mut edits = Vec::new();
    for (resource, expected) in paths.assets.iter().zip(&expected.assets) {
        let current: String = resource
            .path
            .into_rust()
            .map_err(|e| blocked(&format!("Invalid resource path token: {e}")))?;
        if current == expected.path {
            continue;
        }
        let token = resource.path.trim().get_ron();
        let start = (token.as_ptr() as usize)
            .checked_sub(text.as_ptr() as usize)
            .ok_or_else(|| blocked("Resource token is not in the source document"))?;
        let end = start
            .checked_add(token.len())
            .ok_or_else(|| blocked("Invalid token range"))?;
        if text.get(start..end) != Some(token) {
            return Err(blocked("Invalid resource token range"));
        }
        let value = ron::ser::to_string(&expected.path)
            .map_err(|e| blocked(&format!("Cannot serialize resource path: {e}")))?;
        edits.push((start..end, value));
    }
    let mut result = text.to_owned();
    for (range, value) in edits.into_iter().rev() {
        result.replace_range(range, &value);
    }
    let actual = aestra_core::EffectAsset::from_ron(&result)
        .map_err(|e| blocked(&format!("Invalid rewritten effect: {e}")))?;
    if &actual != expected {
        return Err(blocked(
            "Resource rewrite changed unrelated semantic fields",
        ));
    }
    Ok(result.into_bytes())
}

pub(super) fn plan(
    content: &ProjectContent,
    baseline: &BTreeMap<PathBuf, Vec<u8>>,
    moves: &[Move],
) -> Result<Vec<Replacement>, OperationError> {
    let root = content.source_tree().root_path();
    let mut known = BTreeMap::new();
    for path in baseline.keys() {
        let relative = path
            .strip_prefix(root)
            .map_err(|_| blocked("Source escapes root"))?;
        if known
            .insert(key(&relative.to_string_lossy())?, path.canonicalize()?)
            .is_some()
        {
            return Err(blocked("Case-ambiguous project paths"));
        }
    }
    let mut replacements = Vec::new();
    for (source, document) in &content.documents {
        let ProjectSourceDocument::Effect(effect) = document else {
            continue;
        };
        // Unresolved paths are not evidence of no usages. A full inventory excludes
        // linked/unreadable files before this step, and aliases resolve to that inventory.
        for resource in &effect.assets {
            let (file, _) = resource_parts(&resource.path)?;
            let resolved = root.join(file.replace('\\', "/")).canonicalize();
            if !known
                .get(&key(file)?)
                .is_some_and(|path| resolved.as_ref().ok() == Some(path))
            {
                return Err(blocked(&format!(
                    "Unresolved resource path: {}",
                    resource.path
                )));
            }
        }
        let entry = content
            .source(*source)
            .ok_or_else(|| blocked("Missing reference owner"))?;
        let before = baseline
            .get(&entry.path)
            .ok_or_else(|| blocked("Missing owner bytes"))?;
        let (rewritten, changed) = rewrite(before, moves)?;
        if changed {
            if read_file(root, &entry.relative_path)?.as_ref() != Some(before) {
                return Err(blocked("Reference owner changed"));
            }
            let after = patch_paths(before, &rewritten)?;
            replacements.push(Replacement {
                source: entry.relative_path.clone(),
                before: before.clone(),
                after,
            });
        }
    }
    validate(&replacements, moves)?;
    Ok(replacements)
}

pub(super) fn validate(replacements: &[Replacement], moves: &[Move]) -> Result<(), OperationError> {
    if replacements.len() > MAX_FILES {
        return Err(blocked("A batch may rewrite at most 128 documents"));
    }
    let mut sources = BTreeSet::new();
    for replacement in replacements {
        validate_path(&replacement.source)?;
        let source = key(&replacement.source.to_string_lossy())?;
        if !sources.insert(source.clone())
            || moves
                .iter()
                .any(|item| key(&item.destination.to_string_lossy()).ok().as_ref() == Some(&source))
        {
            return Err(blocked("Overlapping replacement owners"));
        }
        if let Some(item) = moves.iter().find(|item| item.source == replacement.source)
            && item.bytes != replacement.before
        {
            return Err(blocked("Moved owner backup mismatch"));
        }
        let (expected, changed) = rewrite(&replacement.before, moves)?;
        let expected = patch_paths(&replacement.before, &expected)?;
        if !changed || expected != replacement.after {
            return Err(blocked("Replacement is not the planned typed path rewrite"));
        }
    }
    Ok(())
}
