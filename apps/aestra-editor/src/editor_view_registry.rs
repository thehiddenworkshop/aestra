//! Shared document/view authority, independent of docking persistence and panel composition.
#![allow(dead_code)] // Same registry API policy as the original editor_view module.
use crate::document_registry::DocumentManager;
use crate::editor_identity::{DocumentId, DocumentKey, EditorViewId};
use bevy::prelude::*;
use std::collections::BTreeMap;
/// The kind of asset editor a view renders. The document supplies the content; the kind selects
/// which editor UI is docked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum EditorViewKind {
    MaterialGraph,
    MaterialFunctionGraph,
    WeslSource,
    // Added in later milestones: Effect, Texture, Mesh, Flipbook.
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

    /// Reinserts a view under a specific, previously-allocated id (used by workspace restore so the
    /// persisted `DockTab::Editor` references resolve). Keeps `next_id` ahead of every restored id so
    /// later `create_view` calls never collide with a restored one.
    pub(crate) fn restore_view(
        &mut self,
        id: EditorViewId,
        document: DocumentId,
        kind: EditorViewKind,
    ) {
        self.views.insert(id, EditorView { id, document, kind });
        self.next_id = self.next_id.max(id.0 + 1);
    }

    pub(crate) fn view(&self, id: EditorViewId) -> Option<EditorView> {
        self.views.get(&id).copied()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &EditorView> + '_ {
        self.views.values()
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

/// Opens the document for `key` in a *new* editor view, always allocating a fresh view even when the
/// document is already open elsewhere (the "Open in New Tab" / split path). The new view is made
/// active. Both views share the document — its draft, dirty state, and buffer — so this never forks a
/// second conflicting draft; only the per-view state (pan / zoom / caret) is independent.
pub(crate) fn open_document_view_in_new_tab(
    documents: &mut DocumentManager,
    views: &mut EditorViewManager,
    active: &mut ActiveEditorContext,
    key: DocumentKey,
    kind: EditorViewKind,
) -> EditorViewId {
    let document = documents.open(key);
    let view = views.create_view(document, kind);
    active.active_document = Some(document);
    active.active_view = Some(view);
    view
}
