//! Shared path identity and release checks for file-backed resource assignments.
use crate::*;
/// Compare lexical loader paths without converting case-sensitive file identities on Unix.
pub(crate) fn path_key(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    if path.starts_with('/') || path.contains([':', '#']) {
        return None;
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => return None,
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let key = parts.join("/");
    Some(if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    })
}

/// Check the disk entry again on release, without decoding images or doing I/O on hover.
/// Image decoding stays with the asynchronous renderer loader and its diagnostics.
pub(crate) fn check_file(
    source: &aestra_project::ProjectSourceEntry,
    catalog: &ProjectEffectCatalog,
) -> Result<(), String> {
    let mut directory = source
        .path
        .parent()
        .ok_or("Resource has no parent folder")?;
    loop {
        if !directory.starts_with(catalog.root()) {
            return Err("Resource is outside the project".into());
        }
        aestra_project::ProjectSourceTree::validate_root(directory)?;
        if directory == catalog.root() {
            break;
        }
        directory = directory
            .parent()
            .ok_or("Resource is outside the project")?;
    }
    let metadata = std::fs::symlink_metadata(&source.path)
        .map_err(|error| format!("Resource unavailable: {error}"))?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked || !metadata.is_file() || metadata.len() == 0 {
        return Err("Resource must be a non-empty regular file, not a link".into());
    }
    if source.metadata.as_ref().is_none_or(|saved| {
        saved.bytes != metadata.len() || saved.modified != metadata.modified().ok()
    }) {
        return Err("Resource changed on disk; refresh Assets and drag it again".into());
    }
    Ok(())
}
