//! Editor-view registry (Milestone 3).
//!
//! Connects [`DocumentId`]s to the dynamic `DockTab::Editor` instances introduced in Milestone 1.
//! A document may back several views (split / open-in-new), and each view is an independent
//! dockable tab. This is the seam that keeps document identity separate from dock identity.
//!
//! Standalone and tested here; the open flow, material graph, and dock wiring consume it from
//! Milestone 4 onward.
#![allow(dead_code)] // Consumed by Milestones 4+.

use crate::docking::EditorViewId;
use crate::document::{DocumentId, DocumentKey, DocumentManager};
use bevy::prelude::*;
use std::collections::BTreeMap;

/// The kind of asset editor a view renders. The document supplies the content; the kind selects
/// which editor UI is docked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EditorViewKind {
    MaterialGraph,
    MaterialFunctionGraph,
    // Added in later milestones: WeslSource, Effect, Texture, Mesh, Flipbook.
}

/// One dockable asset-editor instance: a view of a document. Several views may target the same
/// document (split views), each carrying independent view-local state (migrated in Milestone 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditorView {
    pub(crate) id: EditorViewId,
    pub(crate) document: DocumentId,
    pub(crate) kind: EditorViewKind,
}

/// Registry connecting documents to their dockable editor views, and the authority that allocates
/// [`EditorViewId`]s.
#[derive(Resource, Debug, Default)]
pub(crate) struct EditorViewManager {
    views: BTreeMap<EditorViewId, EditorView>,
    next_id: u64,
}

impl EditorViewManager {
    fn allocate_id(&mut self) -> EditorViewId {
        let id = EditorViewId(self.next_id);
        self.next_id += 1;
        id
    }

    /// Creates a fresh view of `document`, always allocating a new id. Used for split / open-in-new;
    /// the open flow uses [`Self::open_default_view`] instead so it does not duplicate views.
    pub(crate) fn create_view(
        &mut self,
        document: DocumentId,
        kind: EditorViewKind,
    ) -> EditorViewId {
        let id = self.allocate_id();
        self.views.insert(id, EditorView { id, document, kind });
        id
    }

    /// The open-flow entry point: returns an existing view of `document` (focus) or creates the
    /// default view. Opening an already-open asset therefore focuses it rather than replacing
    /// another asset's view.
    pub(crate) fn open_default_view(
        &mut self,
        document: DocumentId,
        kind: EditorViewKind,
    ) -> EditorViewId {
        if let Some(view) = self.views.values().find(|view| view.document == document) {
            return view.id;
        }
        self.create_view(document, kind)
    }

    pub(crate) fn view(&self, id: EditorViewId) -> Option<EditorView> {
        self.views.get(&id).copied()
    }

    pub(crate) fn document_of(&self, id: EditorViewId) -> Option<DocumentId> {
        self.views.get(&id).map(|view| view.document)
    }

    pub(crate) fn views_for_document(
        &self,
        document: DocumentId,
    ) -> impl Iterator<Item = EditorViewId> + '_ {
        self.views
            .values()
            .filter(move |view| view.document == document)
            .map(|view| view.id)
    }

    /// Removes a view. Returns the removed view and whether its document now has no remaining views
    /// (so the caller can decide whether to close the document too).
    pub(crate) fn close_view(&mut self, id: EditorViewId) -> Option<(EditorView, bool)> {
        let view = self.views.remove(&id)?;
        let document_now_orphaned = !self
            .views
            .values()
            .any(|other| other.document == view.document);
        Some((view, document_now_orphaned))
    }

    pub(crate) fn len(&self) -> usize {
        self.views.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.views.is_empty()
    }
}

/// The editor view (and its document) that contextual tool panels follow — updated whenever an
/// asset is opened or focused. Milestone 10 extends the tool panels to read this; for now it simply
/// records what the user last brought forward.
#[derive(Resource, Debug, Default)]
pub(crate) struct ActiveEditorContext {
    pub(crate) active_view: Option<EditorViewId>,
    pub(crate) active_document: Option<DocumentId>,
}

/// Opens (or focuses) the document for `key`, ensures it has a default editor view of `kind`, and
/// marks that view active. Returns the view. This is the single entry point the asset browser and
/// other open paths route through, so opening an already-open asset focuses it rather than
/// replacing another.
pub(crate) fn open_document_view(
    documents: &mut DocumentManager,
    views: &mut EditorViewManager,
    active: &mut ActiveEditorContext,
    key: DocumentKey,
    kind: EditorViewKind,
) -> EditorViewId {
    let document = documents.open(key);
    let view = views.open_default_view(document, kind);
    active.active_document = Some(document);
    active.active_view = Some(view);
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(seed: u64) -> DocumentId {
        DocumentId(seed)
    }

    #[test]
    fn open_default_view_focuses_an_existing_view_of_the_document() {
        let mut manager = EditorViewManager::default();
        let doc = document(1);
        let first = manager.open_default_view(doc, EditorViewKind::MaterialGraph);
        let again = manager.open_default_view(doc, EditorViewKind::MaterialGraph);
        // Opening the same document again returns its existing view rather than a new one.
        assert_eq!(first, again);
        assert_eq!(manager.len(), 1);
        assert_eq!(manager.document_of(first), Some(doc));
    }

    #[test]
    fn distinct_documents_get_distinct_views() {
        let mut manager = EditorViewManager::default();
        let a = manager.open_default_view(document(1), EditorViewKind::MaterialGraph);
        let b = manager.open_default_view(document(2), EditorViewKind::MaterialFunctionGraph);
        assert_ne!(a, b);
        assert_eq!(manager.len(), 2);
        assert_eq!(
            manager.view(b).unwrap().kind,
            EditorViewKind::MaterialFunctionGraph
        );
    }

    #[test]
    fn a_document_can_back_several_split_views() {
        let mut manager = EditorViewManager::default();
        let doc = document(1);
        let a = manager.create_view(doc, EditorViewKind::MaterialGraph);
        let b = manager.create_view(doc, EditorViewKind::MaterialGraph);
        assert_ne!(a, b);
        let mut views: Vec<_> = manager.views_for_document(doc).collect();
        views.sort();
        assert_eq!(views, vec![a, b]);
    }

    #[test]
    fn open_document_view_routes_documents_views_and_active_context() {
        use crate::document::DocumentKey;
        use aestra_core::MaterialProgramId;

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        let key_a = DocumentKey::MaterialProgram(MaterialProgramId::from_u128(0xa));
        let key_b = DocumentKey::MaterialProgram(MaterialProgramId::from_u128(0xb));

        let view_a = open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            key_a,
            EditorViewKind::MaterialGraph,
        );
        let view_b = open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            key_b,
            EditorViewKind::MaterialGraph,
        );
        // Opening a second asset does not replace the first: two documents, two views.
        assert_ne!(view_a, view_b);
        assert_eq!(documents.len(), 2);
        assert_eq!(views.len(), 2);
        assert_eq!(active.active_view, Some(view_b));

        // Reopening the first asset focuses its existing view and makes it active again.
        let refocus = open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            key_a,
            EditorViewKind::MaterialGraph,
        );
        assert_eq!(refocus, view_a);
        assert_eq!(documents.len(), 2);
        assert_eq!(views.len(), 2);
        assert_eq!(active.active_view, Some(view_a));
    }

    #[test]
    fn closing_reports_when_the_document_loses_its_last_view() {
        let mut manager = EditorViewManager::default();
        let doc = document(1);
        let a = manager.create_view(doc, EditorViewKind::MaterialGraph);
        let b = manager.create_view(doc, EditorViewKind::MaterialGraph);

        // Closing one of two views leaves the document with a view.
        let (closed_a, orphaned) = manager.close_view(a).unwrap();
        assert_eq!(closed_a.id, a);
        assert!(!orphaned);

        // Closing the last view orphans the document.
        let (_, orphaned) = manager.close_view(b).unwrap();
        assert!(orphaned);
        assert!(manager.is_empty());
        assert!(manager.close_view(a).is_none());
    }
}
