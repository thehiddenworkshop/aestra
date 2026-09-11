//! Standalone WESL source documents (Milestone 7).
//!
//! WESL modules are plain `.wesl` text files, distinct from the semantic material assets. They are
//! identified by their project-relative path (hashed into a stable, serializable [`WeslSourceId`] so
//! it can key a [`crate::document::DocumentKey`]) and their text lives in an in-memory buffer here.
//!
//! Milestone 7-1 opens them read-only; editing, dirty tracking, save, and diagnostics are layered on
//! in later slices.
#![allow(dead_code)] // Editing/save/diagnostics consume the rest from Milestone 7-2+.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Stable identity of a WESL source document: a deterministic hash of its project-relative path.
/// Serializable so it can persist in the editor-document manifest, and `Copy` so [`DocumentKey`]
/// stays `Copy`. Like the source-tree ids, it changes if the file is moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct WeslSourceId {
    // Two u64 halves rather than a u128 so the id round-trips cleanly through RON in the manifest.
    high: u64,
    low: u64,
}

impl WeslSourceId {
    pub(crate) fn for_relative_path(relative: &Path) -> Self {
        // Normalize separators so the same file yields the same id on every platform.
        let normalized = relative.to_string_lossy().replace('\\', "/");
        let hash = |salt: u64| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            salt.hash(&mut hasher);
            normalized.hash(&mut hasher);
            hasher.finish()
        };
        Self {
            high: hash(0),
            low: hash(0xa5a5_5a5a),
        }
    }
}

impl std::fmt::Display for WeslSourceId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:016x}{:016x}", self.high, self.low)
    }
}

/// One open WESL source buffer. `disk` is the last text read from (or written to) the file, so a
/// future editing slice can compare it against `text` for dirty state.
#[derive(Debug, Clone)]
struct WeslBuffer {
    relative_path: PathBuf,
    text: String,
    disk: String,
    revision: u64,
}

/// In-memory registry of open WESL source buffers, keyed by [`WeslSourceId`].
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslDocuments {
    buffers: BTreeMap<WeslSourceId, WeslBuffer>,
}

impl WeslDocuments {
    /// Opens (or refreshes) a WESL buffer for `relative_path`, returning its id. Opening the same
    /// file again refreshes the on-disk baseline without discarding an existing buffer identity.
    pub(crate) fn open(&mut self, relative_path: PathBuf, text: String) -> WeslSourceId {
        let id = WeslSourceId::for_relative_path(&relative_path);
        self.buffers
            .entry(id)
            .and_modify(|buffer| buffer.disk = text.clone())
            .or_insert_with(|| WeslBuffer {
                relative_path,
                text: text.clone(),
                disk: text,
                revision: 0,
            });
        id
    }

    pub(crate) fn text(&self, id: WeslSourceId) -> Option<&str> {
        self.buffers.get(&id).map(|buffer| buffer.text.as_str())
    }

    pub(crate) fn relative_path(&self, id: WeslSourceId) -> Option<&Path> {
        self.buffers
            .get(&id)
            .map(|buffer| buffer.relative_path.as_path())
    }

    pub(crate) fn is_open(&self, id: WeslSourceId) -> bool {
        self.buffers.contains_key(&id)
    }

    /// Replaces the buffer text, advancing its revision when the text actually changed. Returns
    /// whether it changed, so callers can avoid redundant work.
    pub(crate) fn set_text(&mut self, id: WeslSourceId, text: String) -> bool {
        let Some(buffer) = self.buffers.get_mut(&id) else {
            return false;
        };
        if buffer.text == text {
            return false;
        }
        buffer.text = text;
        buffer.revision += 1;
        true
    }

    /// Records that the current buffer text was written to disk, clearing the dirty state.
    pub(crate) fn mark_saved(&mut self, id: WeslSourceId) {
        if let Some(buffer) = self.buffers.get_mut(&id) {
            buffer.disk = buffer.text.clone();
        }
    }

    /// Reverts the buffer to the last on-disk text (the discard action).
    pub(crate) fn revert(&mut self, id: WeslSourceId) {
        if let Some(buffer) = self.buffers.get_mut(&id) {
            buffer.text = buffer.disk.clone();
        }
    }

    pub(crate) fn is_dirty(&self, id: WeslSourceId) -> bool {
        self.buffers
            .get(&id)
            .is_some_and(|buffer| buffer.text != buffer.disk)
    }

    pub(crate) fn revision(&self, id: WeslSourceId) -> Option<u64> {
        self.buffers.get(&id).map(|buffer| buffer.revision)
    }

    /// The open WESL documents that currently have unsaved edits, as (id, relative path).
    pub(crate) fn dirty_documents(&self) -> impl Iterator<Item = (WeslSourceId, &Path)> + '_ {
        self.buffers
            .iter()
            .filter(|(_, buffer)| buffer.text != buffer.disk)
            .map(|(id, buffer)| (*id, buffer.relative_path.as_path()))
    }

    pub(crate) fn close(&mut self, id: WeslSourceId) {
        self.buffers.remove(&id);
    }

    pub(crate) fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Open buffers as (id, relative path, revision, text) — used to recompile changed modules.
    fn iter(&self) -> impl Iterator<Item = (WeslSourceId, &Path, u64, &str)> + '_ {
        self.buffers.iter().map(|(id, buffer)| {
            (
                *id,
                buffer.relative_path.as_path(),
                buffer.revision,
                buffer.text.as_str(),
            )
        })
    }
}

/// The compile state of a WESL module: clean, or the compiler's error message plus the 1-based
/// source line it points at (best-effort, parsed from the message) so the editor can mark it inline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WeslCompileState {
    Ok,
    Error {
        message: String,
        line: Option<usize>,
    },
}

/// Best-effort extraction of a 1-based line number from a WESL/Naga compiler message. Naga formats
/// locations as `wgsl:LINE:COL`; generated WGSL lines align with WESL for modules without imports.
fn error_line(message: &str) -> Option<usize> {
    for token in message.split(|c: char| c.is_whitespace() || c == '│' || c == '┌' || c == '─')
    {
        let digits = token
            .strip_prefix("wgsl:")
            .and_then(|rest| rest.split(':').next());
        if let Some(line) = digits.and_then(|line| line.parse::<usize>().ok()) {
            return Some(line);
        }
    }
    None
}

/// Per-document WESL compile diagnostics, refreshed whenever a buffer's revision changes. Paired
/// with the module's compiled revision so a stale buffer is not reported.
#[derive(Resource, Debug, Default)]
pub(crate) struct WeslDiagnostics {
    entries: BTreeMap<WeslSourceId, (u64, WeslCompileState)>,
}

impl WeslDiagnostics {
    pub(crate) fn state(&self, id: WeslSourceId) -> Option<&WeslCompileState> {
        self.entries.get(&id).map(|(_, state)| state)
    }
}

/// A WESL module name derived from a file stem, sanitized to a valid identifier for the compiler.
fn module_name_for(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("module");
    let mut name: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    {
        name.insert(0, '_');
    }
    name
}

/// Compiles a WESL module (via the WESL compiler and Naga validation) to a clean/error state.
pub(crate) fn compile_wesl_source(module_name: &str, source: &str) -> WeslCompileState {
    match aestra_gpu::shader::compile_wesl(module_name, source, &[]) {
        Ok(_) => WeslCompileState::Ok,
        Err(error) => {
            let message = error.to_string();
            let line = error_line(&message);
            WeslCompileState::Error { message, line }
        }
    }
}

/// Recompiles WESL modules whose buffer changed since their last compile, and drops entries for
/// closed documents. Runs only when the document store changes, and only touches changed revisions.
pub(crate) fn recompile_changed_wesl(
    documents: Res<WeslDocuments>,
    mut diagnostics: ResMut<WeslDiagnostics>,
) {
    if !documents.is_changed() {
        return;
    }
    for (id, path, revision, text) in documents.iter() {
        let up_to_date = diagnostics
            .entries
            .get(&id)
            .is_some_and(|(compiled, _)| *compiled == revision);
        if up_to_date {
            continue;
        }
        let module = module_name_for(path);
        let state = compile_wesl_source(&module, text);
        diagnostics.entries.insert(id, (revision, state));
    }
    diagnostics
        .entries
        .retain(|id, _| documents.buffers.contains_key(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_relative_path_yields_a_stable_id_across_separators() {
        let a = WeslSourceId::for_relative_path(Path::new("shaders/noise.wesl"));
        let b = WeslSourceId::for_relative_path(Path::new("shaders\\noise.wesl"));
        assert_eq!(a, b);
        let other = WeslSourceId::for_relative_path(Path::new("shaders/other.wesl"));
        assert_ne!(a, other);
    }

    #[test]
    fn opening_registers_text_and_is_idempotent_by_path() {
        let mut documents = WeslDocuments::default();
        let id = documents.open(PathBuf::from("shaders/noise.wesl"), "fn main() {}".into());
        assert_eq!(documents.text(id), Some("fn main() {}"));
        assert_eq!(
            documents.relative_path(id),
            Some(Path::new("shaders/noise.wesl"))
        );
        assert!(!documents.is_dirty(id));
        // Reopening the same path returns the same id and refreshes the baseline.
        let again = documents.open(PathBuf::from("shaders/noise.wesl"), "fn main() { }".into());
        assert_eq!(again, id);
        assert_eq!(documents.len(), 1);
    }

    #[test]
    fn compile_reports_ok_for_valid_and_an_error_for_invalid_wesl() {
        assert_eq!(
            compile_wesl_source("noise", "fn add(a: f32, b: f32) -> f32 { return a + b; }"),
            WeslCompileState::Ok
        );
        match compile_wesl_source("noise", "fn broken( {") {
            WeslCompileState::Error { message, .. } => assert!(!message.is_empty()),
            WeslCompileState::Ok => panic!("expected a compile error for malformed WESL"),
        }
    }

    #[test]
    fn round_trips_through_ron_for_the_manifest() {
        let id = WeslSourceId::for_relative_path(Path::new("shaders/noise.wesl"));
        let encoded = ron::to_string(&id).unwrap();
        assert_eq!(ron::from_str::<WeslSourceId>(&encoded).unwrap(), id);
    }
}
