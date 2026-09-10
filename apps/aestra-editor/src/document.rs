//! Multi-document registry (Milestone 2).
//!
//! Owns the identity and lifecycle of open authored documents — material programs and functions
//! today, WESL sources and effects later. This is the generic owner the multi-document editor
//! routes opening/saving/closing through, replacing the single global `material_target`.
//!
//! Introduced here as a standalone, tested subsystem; it is wired into the open flow, the material
//! graph, and the save/close/undo lifecycle in Milestones 3–6.
#![allow(dead_code)] // Consumed by Milestones 3–6.

use aestra_core::{MaterialFunctionId, MaterialProgramId};
use bevy::prelude::*;
use std::collections::BTreeMap;

/// Stable identity of one open document within an editor session. Distinct from the asset it
/// authors ([`DocumentKey`]) and from the dock tabs / editor views that display it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct DocumentId(pub(crate) u64);

/// Monotonic per-document edit counter. Async work (compilation, previews) is tagged with the
/// revision it started from, so a stale result can be discarded once the document changes again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DocumentRevision(pub(crate) u64);

/// Identifies the project asset a document authors. One asset corresponds to one open document by
/// default, so opening the same asset twice never creates two independent drafts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum DocumentKey {
    MaterialProgram(MaterialProgramId),
    MaterialFunction(MaterialFunctionId),
}

/// An open authored document: its identity, the asset it edits, and its dirty/revision state.
///
/// Authored draft bytes continue to live in the project draft store for now; ownership of the draft
/// itself moves onto the document in the save/undo milestone.
#[derive(Debug, Clone)]
pub(crate) struct OpenDocument {
    pub(crate) id: DocumentId,
    pub(crate) key: DocumentKey,
    pub(crate) dirty: bool,
    pub(crate) revision: DocumentRevision,
}

/// Registry of open documents, keyed both by identity and by asset. Guarantees one document per
/// asset and tracks per-document dirty/revision state independently.
#[derive(Resource, Debug, Default)]
pub(crate) struct DocumentManager {
    documents: BTreeMap<DocumentId, OpenDocument>,
    by_key: BTreeMap<DocumentKey, DocumentId>,
    next_id: u64,
}

impl DocumentManager {
    fn allocate_id(&mut self) -> DocumentId {
        let id = DocumentId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Returns the document for `key`, creating it if one is not already open. Idempotent: opening
    /// the same asset again returns the existing document rather than a second independent draft.
    pub(crate) fn open(&mut self, key: DocumentKey) -> DocumentId {
        if let Some(id) = self.by_key.get(&key) {
            return *id;
        }
        let id = self.allocate_id();
        self.documents.insert(
            id,
            OpenDocument {
                id,
                key,
                dirty: false,
                revision: DocumentRevision::default(),
            },
        );
        self.by_key.insert(key, id);
        id
    }

    pub(crate) fn find(&self, key: &DocumentKey) -> Option<DocumentId> {
        self.by_key.get(key).copied()
    }

    pub(crate) fn document(&self, id: DocumentId) -> Option<&OpenDocument> {
        self.documents.get(&id)
    }

    pub(crate) fn is_open(&self, id: DocumentId) -> bool {
        self.documents.contains_key(&id)
    }

    pub(crate) fn is_dirty(&self, id: DocumentId) -> bool {
        self.documents
            .get(&id)
            .is_some_and(|document| document.dirty)
    }

    pub(crate) fn revision(&self, id: DocumentId) -> Option<DocumentRevision> {
        self.documents.get(&id).map(|document| document.revision)
    }

    /// Records an edit: marks the document dirty and advances its revision. Returns the new
    /// revision, or `None` if the document is not open.
    pub(crate) fn mark_edited(&mut self, id: DocumentId) -> Option<DocumentRevision> {
        let document = self.documents.get_mut(&id)?;
        document.dirty = true;
        document.revision.0 += 1;
        Some(document.revision)
    }

    /// Clears the dirty flag after a successful save. The revision is intentionally not reset, so
    /// async work already tagged with an older revision still resolves against a stable timeline.
    pub(crate) fn mark_saved(&mut self, id: DocumentId) {
        if let Some(document) = self.documents.get_mut(&id) {
            document.dirty = false;
        }
    }

    /// Removes a document from the registry and returns it. Callers resolve save/discard first; the
    /// asset key is freed so a later open creates a fresh document.
    pub(crate) fn close(&mut self, id: DocumentId) -> Option<OpenDocument> {
        let document = self.documents.remove(&id)?;
        self.by_key.remove(&document.key);
        Some(document)
    }

    pub(crate) fn len(&self) -> usize {
        self.documents.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    pub(crate) fn open_documents(&self) -> impl Iterator<Item = &OpenDocument> {
        self.documents.values()
    }

    pub(crate) fn dirty_documents(&self) -> impl Iterator<Item = &OpenDocument> {
        self.documents.values().filter(|document| document.dirty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn program_key(seed: u128) -> DocumentKey {
        DocumentKey::MaterialProgram(MaterialProgramId::from_u128(seed))
    }

    #[test]
    fn opening_the_same_asset_returns_the_same_document() {
        let mut manager = DocumentManager::default();
        let key = program_key(0x1);
        let first = manager.open(key);
        let second = manager.open(key);
        assert_eq!(first, second);
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.find(&key), Some(first));
    }

    #[test]
    fn distinct_assets_produce_independent_documents() {
        let mut manager = DocumentManager::default();
        let a = manager.open(program_key(0xa));
        let b = manager.open(program_key(0xb));
        assert_ne!(a, b);
        assert_eq!(manager.len(), 2);

        // A material program and a material function with the same underlying uuid are still
        // distinct documents, because the key carries the asset kind.
        let program = manager.open(DocumentKey::MaterialProgram(MaterialProgramId::from_u128(
            0xc,
        )));
        let function = manager.open(DocumentKey::MaterialFunction(
            MaterialFunctionId::from_u128(0xc),
        ));
        assert_ne!(program, function);
        assert_eq!(manager.len(), 4);
    }

    #[test]
    fn dirty_and_save_state_are_per_document() {
        let mut manager = DocumentManager::default();
        let a = manager.open(program_key(0xa));
        let b = manager.open(program_key(0xb));

        assert!(!manager.is_dirty(a));
        assert_eq!(manager.mark_edited(a), Some(DocumentRevision(1)));
        assert!(manager.is_dirty(a));
        // Editing A must not dirty B.
        assert!(!manager.is_dirty(b));

        assert_eq!(manager.mark_edited(a), Some(DocumentRevision(2)));
        manager.mark_edited(b);
        // Saving A leaves B dirty.
        manager.mark_saved(a);
        assert!(!manager.is_dirty(a));
        assert!(manager.is_dirty(b));
        // The revision survives the save so stale async results can still be discarded.
        assert_eq!(manager.revision(a), Some(DocumentRevision(2)));

        let dirty: Vec<_> = manager
            .dirty_documents()
            .map(|document| document.id)
            .collect();
        assert_eq!(dirty, vec![b]);
    }

    #[test]
    fn closing_frees_the_asset_key_for_a_fresh_document() {
        let mut manager = DocumentManager::default();
        let key = program_key(0x1);
        let first = manager.open(key);
        manager.mark_edited(first);

        let closed = manager.close(first).unwrap();
        assert_eq!(closed.key, key);
        assert!(!manager.is_open(first));
        assert_eq!(manager.find(&key), None);

        // Reopening the same asset yields a new, clean document.
        let second = manager.open(key);
        assert_ne!(first, second);
        assert!(!manager.is_dirty(second));
    }
}
