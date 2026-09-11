//! Editor-view registry (Milestone 3).
//!
//! Connects [`DocumentId`]s to the dynamic `DockTab::Editor` instances introduced in Milestone 1.
//! A document may back several views (split / open-in-new), and each view is an independent
//! dockable tab. This is the seam that keeps document identity separate from dock identity.
//!
//! Standalone and tested here; the open flow, material graph, and dock wiring consume it from
//! Milestone 4 onward.
#![allow(dead_code)] // Consumed by Milestones 4+.

use crate::docking::{EditorViewId, WorkspaceLayout, workspace_config_path};
use crate::document::{DocumentId, DocumentKey, DocumentManager};
use crate::material_document::MaterialEditingTarget;
use crate::session::EditorSession;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::{fs, io, path::PathBuf};

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

/// Resolves the editing target an editor view renders, by following view → document → asset key.
/// Returns `None` for a stale view (e.g. a persisted editor tab whose view no longer exists), so
/// the dock can drop it gracefully. `root` is the current project root the target is scoped to.
pub(crate) fn view_editing_target(
    view: EditorViewId,
    views: &EditorViewManager,
    documents: &DocumentManager,
    root: &std::path::Path,
) -> Option<MaterialEditingTarget> {
    let document_id = views.document_of(view)?;
    let key = documents.document(document_id)?.key;
    match key {
        DocumentKey::MaterialProgram(id) => Some(MaterialEditingTarget::Program {
            root: root.to_owned(),
            id,
        }),
        DocumentKey::MaterialFunction(id) => Some(MaterialEditingTarget::Function {
            root: root.to_owned(),
            id,
        }),
        // WESL documents are not material targets; their view renders text, not a material graph.
        DocumentKey::WeslSource(_) => None,
    }
}

/// The editor kind a view renders (material graph vs WESL source), so the dock can pick the pane.
pub(crate) fn view_kind(view: EditorViewId, views: &EditorViewManager) -> Option<EditorViewKind> {
    views.view(view).map(|view| view.kind)
}

/// The WESL source id an editor view edits, if it is a WESL document view.
pub(crate) fn view_wesl_source(
    view: EditorViewId,
    views: &EditorViewManager,
    documents: &DocumentManager,
) -> Option<crate::wesl_document::WeslSourceId> {
    match view_document_key(view, views, documents)? {
        DocumentKey::WeslSource(id) => Some(id),
        _ => None,
    }
}

/// Maps a material editing target to the document key and default view kind it corresponds to, or
/// `None` for the effect's inline material (which is not a shared-asset document).
fn document_key_for_target(
    target: &MaterialEditingTarget,
) -> Option<(DocumentKey, EditorViewKind)> {
    match target {
        MaterialEditingTarget::Program { id, .. } => Some((
            DocumentKey::MaterialProgram(*id),
            EditorViewKind::MaterialGraph,
        )),
        MaterialEditingTarget::Function { id, .. } => Some((
            DocumentKey::MaterialFunction(*id),
            EditorViewKind::MaterialFunctionGraph,
        )),
        MaterialEditingTarget::EffectInstance => None,
    }
}

/// Reconciles the document/view model and active context with the material target, whatever path
/// changed it (asset browser, menus, recovery, undo, return-to-effect). While `material_target`
/// remains authoritative this keeps the view model it feeds correct from every entry point; per-view
/// rendering (M4c) and target removal (M12) build on the model maintained here.
pub(crate) fn reconcile_active_from_target(
    target: &MaterialEditingTarget,
    documents: &mut DocumentManager,
    views: &mut EditorViewManager,
    active: &mut ActiveEditorContext,
) {
    match document_key_for_target(target) {
        Some((key, kind)) => {
            let already = active
                .active_document
                .and_then(|id| documents.document(id))
                .map(|document| document.key)
                == Some(key);
            if !already {
                open_document_view(documents, views, active, key, kind);
            }
        }
        None => {
            active.active_view = None;
            active.active_document = None;
        }
    }
}

/// System wrapper: reconciles the view model from the session material target on change.
pub(crate) fn sync_active_document_from_target(
    session: Res<EditorSession>,
    mut documents: ResMut<DocumentManager>,
    mut views: ResMut<EditorViewManager>,
    mut active: ResMut<ActiveEditorContext>,
) {
    if !session.is_changed() {
        return;
    }
    reconcile_active_from_target(
        &session.material_target,
        &mut documents,
        &mut views,
        &mut active,
    );
}

// ---------------------------------------------------------------------------------------------
// Workspace persistence (Milestone 11)
//
// The dock tree persists `DockTab::Editor(EditorViewId)`, but the documents and views those ids
// resolve to live only in memory. Without a companion manifest, restored editor tabs point at
// views that no longer exist and render nothing. The manifest records, for every open editor view,
// the asset it edits and its editor kind, so both managers can be rebuilt on restart and the
// persisted tab references resolve to the same assets.
// ---------------------------------------------------------------------------------------------

/// One persisted editor view: its stable id, the asset it edits, and the editor kind. `DocumentId`
/// is intentionally not persisted — it is reallocated on restore, since nothing outside the managers
/// references it. Only `EditorViewId` appears in the persisted dock tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PersistedEditorView {
    id: EditorViewId,
    document: DocumentKey,
    kind: EditorViewKind,
}

/// The serializable companion to the workspace layout: the open editor views the layout's editor
/// tabs refer to. Saved beside `editor-layout.ron` as `editor-documents.ron`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct EditorWorkspaceManifest {
    views: Vec<PersistedEditorView>,
}

impl EditorWorkspaceManifest {
    /// Snapshots the currently open editor views by joining each view to its document's asset key.
    fn from_managers(views: &EditorViewManager, documents: &DocumentManager) -> Self {
        let mut entries: Vec<PersistedEditorView> = views
            .iter()
            .filter_map(|view| {
                let key = documents.document(view.document)?.key;
                Some(PersistedEditorView {
                    id: view.id,
                    document: key,
                    kind: view.kind,
                })
            })
            .collect();
        entries.sort_by_key(|entry| entry.id.0);
        Self { views: entries }
    }

    fn load() -> Self {
        fs::read_to_string(manifest_path())
            .ok()
            .and_then(|source| ron::from_str(&source).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> io::Result<()> {
        let path = manifest_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let source = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(io::Error::other)?;
        fs::write(path, source)
    }
}

fn manifest_path() -> PathBuf {
    workspace_config_path("editor-documents.ron")
}

/// Rebuilds the document and view managers from a persisted manifest. Documents are reopened by
/// asset key (idempotent, so several views of one asset share a document), and each view is
/// reinserted under its persisted id so the dock tree's editor tabs resolve.
fn rebuild_managers(manifest: &EditorWorkspaceManifest) -> (DocumentManager, EditorViewManager) {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    for entry in &manifest.views {
        let document = documents.open(entry.document);
        views.restore_view(entry.id, document, entry.kind);
    }
    (documents, views)
}

/// Startup: reconstructs the open documents/views the persisted dock tree refers to, and prunes any
/// editor tab whose view could not be restored (a lost or corrupt manifest entry). Never fails the
/// launch — a missing or unreadable manifest simply restores an empty set.
pub(crate) fn restore_editor_workspace(
    mut layout: ResMut<WorkspaceLayout>,
    mut documents: ResMut<DocumentManager>,
    mut views: ResMut<EditorViewManager>,
) {
    let manifest = EditorWorkspaceManifest::load();
    let (restored_documents, restored_views) = rebuild_managers(&manifest);
    *documents = restored_documents;
    *views = restored_views;

    let known: HashSet<EditorViewId> = views.iter().map(|view| view.id).collect();
    if layout.prune_editor_views(&known) {
        // The dock tree lost tabs it referenced but could not restore; persist the pruned layout so
        // the discrepancy does not resurface on the next launch.
        if let Err(error) = layout.save() {
            warn!("failed to persist pruned workspace layout: {error}");
        }
    }
}

/// Keeps `editor-documents.ron` in step with the open editor views whenever they change (open,
/// and — from Milestone 6 — close). Paired with the persisted dock tree so a restart restores both.
pub(crate) fn persist_editor_workspace(
    views: Res<EditorViewManager>,
    documents: Res<DocumentManager>,
) {
    if !views.is_changed() {
        return;
    }
    let manifest = EditorWorkspaceManifest::from_managers(&views, &documents);
    if let Err(error) = manifest.save() {
        warn!("failed to persist editor-document manifest: {error}");
    }
}

/// Selects the restored editor views whose backing asset is definitively gone. Split out from the
/// reconcile system so the decision can be tested without a live project catalog.
fn views_with_missing_assets(
    views: &EditorViewManager,
    documents: &DocumentManager,
    is_missing: impl Fn(DocumentKey) -> bool,
) -> Vec<EditorViewId> {
    views
        .iter()
        .filter_map(|view| {
            let key = documents.document(view.document)?.key;
            is_missing(key).then_some(view.id)
        })
        .collect()
}

/// Once the project index is fully scanned, drops restored editor tabs whose asset was moved or
/// deleted while the editor was closed — the plan's "skip safely" branch of missing-asset handling.
/// Runs a single time; assets that are merely ambiguous (recoverable) or present are left untouched,
/// and the manifest is rewritten by [`persist_editor_workspace`] because the view set changed.
pub(crate) fn reconcile_restored_documents_against_catalog(
    catalog: Res<crate::ProjectEffectCatalog>,
    mut done: Local<bool>,
    mut layout: ResMut<WorkspaceLayout>,
    mut documents: ResMut<DocumentManager>,
    mut views: ResMut<EditorViewManager>,
    mut active: ResMut<ActiveEditorContext>,
) {
    if *done {
        return;
    }
    // Wait for a fully-scanned index: a mid-scan empty catalog must never prune live tabs.
    if !matches!(
        catalog.availability(),
        aestra_project::ProjectAssetIndexAvailability::Ready
    ) {
        return;
    }
    *done = true;
    if views.is_empty() {
        return;
    }
    let missing = views_with_missing_assets(&views, &documents, |key| match key {
        DocumentKey::MaterialProgram(id) => catalog.material_program_missing(id),
        DocumentKey::MaterialFunction(id) => catalog.material_function_missing(id),
        // WESL restore/pruning is handled by its own store (Milestone 7); never prune here.
        DocumentKey::WeslSource(_) => false,
    });
    let mut layout_changed = false;
    for view in missing {
        layout_changed |= layout.close_editor(view);
        if let Some((removed, orphaned)) = views.close_view(view) {
            if orphaned {
                documents.close(removed.document);
            }
            if active.active_view == Some(view) {
                active.active_view = None;
                active.active_document = None;
            }
        }
    }
    if layout_changed && let Err(error) = layout.save() {
        warn!("failed to persist workspace layout after dropping missing documents: {error}");
    }
}

/// Fired to close a single editor view — from the tab's close button or Ctrl+W. When `force` is
/// false the close is guarded: a document with unsaved edits opens the dirty-close prompt instead of
/// closing. The prompt's Save/Discard resolutions re-fire this with `force` set.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct CloseEditorView {
    pub(crate) view: EditorViewId,
    pub(crate) force: bool,
}

impl CloseEditorView {
    pub(crate) fn requested(view: EditorViewId) -> Self {
        Self { view, force: false }
    }
}

/// Save the view's document, then close it (the prompt's Save resolution). The save is queued; the
/// view closes immediately while it proceeds, since the draft lives in the catalog, not the view.
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct SaveAndCloseEditorView(pub(crate) EditorViewId);

/// Discard the view's unsaved draft, then close it (the prompt's Discard resolution).
#[derive(Event, Debug, Clone, Copy)]
pub(crate) struct DiscardAndCloseEditorView(pub(crate) EditorViewId);

/// Whether a material editing target edits the asset a document key names.
fn target_edits_key(target: &MaterialEditingTarget, key: DocumentKey) -> bool {
    match key {
        DocumentKey::MaterialProgram(id) => target.program() == Some(id),
        DocumentKey::MaterialFunction(id) => target.function() == Some(id),
        DocumentKey::WeslSource(_) => false,
    }
}

/// Whether the document `key` names has unsaved edits. WESL modules are read-only in Milestone 7-1,
/// so they are never dirty yet.
fn document_key_is_dirty(key: DocumentKey, catalog: &crate::ProjectEffectCatalog) -> bool {
    match key {
        DocumentKey::MaterialProgram(id) => catalog.material_drafts.programs.contains_key(&id),
        DocumentKey::MaterialFunction(id) => catalog.material_drafts.functions.contains_key(&id),
        DocumentKey::WeslSource(_) => false,
    }
}

/// Resolves the document key an editor view edits, if the view is still open.
fn view_document_key(
    view: EditorViewId,
    views: &EditorViewManager,
    documents: &DocumentManager,
) -> Option<DocumentKey> {
    documents
        .document(views.document_of(view)?)
        .map(|open| open.key)
}

/// Closes an editor view: removes its dock tab, drops the view, closes its document when it was the
/// last view of it, and clears the active context if it pointed there. While the singleton target
/// still drives the shared material tools (pre-M12), closing the view that steers it falls back to
/// the effect material so the graph and tools do not keep pointing at a closed document.
///
/// Unless `force`, a document with unsaved edits opens the dirty-close prompt instead of closing.
/// The manifest is rewritten by [`persist_editor_workspace`] because the view set changed.
pub(crate) fn close_editor_view(
    close: On<CloseEditorView>,
    mut layout: ResMut<WorkspaceLayout>,
    mut documents: ResMut<DocumentManager>,
    mut views: ResMut<EditorViewManager>,
    mut active: ResMut<ActiveEditorContext>,
    mut session: ResMut<EditorSession>,
    catalog: Res<crate::ProjectEffectCatalog>,
    mut protection: ResMut<crate::persistence::DocumentProtectionState>,
) {
    let CloseEditorView { view, force } = *close;
    let closed_key = view_document_key(view, &views, &documents);
    if !force
        && let Some(key) = closed_key
        && document_key_is_dirty(key, &catalog)
    {
        // Defer to the dirty-close prompt: Save / Discard / Cancel.
        protection.pending_editor_close = Some(view);
        return;
    }
    let mut changed = layout.close_editor(view);
    if let Some((removed, orphaned)) = views.close_view(view) {
        if orphaned {
            documents.close(removed.document);
        }
        if active.active_view == Some(view) {
            active.active_view = None;
            active.active_document = None;
        }
        changed = true;
    }
    if let Some(key) = closed_key
        && target_edits_key(&session.material_target, key)
    {
        session.return_to_effect_material();
        changed = true;
    }
    if changed {
        session.ui_revision += 1;
        if let Err(error) = layout.save() {
            warn!("failed to persist workspace layout after closing an editor view: {error}");
        }
    }
}

/// Save resolution of the dirty-close prompt: queue a save of the view's document, then force-close.
pub(crate) fn save_and_close_editor_view(
    event: On<SaveAndCloseEditorView>,
    mut commands: Commands,
    session: Res<EditorSession>,
    catalog: Res<crate::ProjectEffectCatalog>,
    documents: Res<DocumentManager>,
    views: Res<EditorViewManager>,
) {
    let view = event.0;
    if let Some(key) = view_document_key(view, &views, &documents) {
        let target = match key {
            DocumentKey::MaterialProgram(id) => Some(MaterialEditingTarget::Program {
                root: catalog.root().to_owned(),
                id,
            }),
            DocumentKey::MaterialFunction(id) => Some(MaterialEditingTarget::Function {
                root: catalog.root().to_owned(),
                id,
            }),
            // WESL save lands in a later slice; read-only WESL never reaches the dirty-close prompt.
            DocumentKey::WeslSource(_) => None,
        };
        if let Some(target) = target {
            crate::persistence::queue_save_target(&mut commands, &session, &catalog, target);
        }
    }
    commands.trigger(CloseEditorView { view, force: true });
}

/// Discard resolution of the dirty-close prompt: drop the view's unsaved draft (and its edit
/// history), then force-close.
pub(crate) fn discard_and_close_editor_view(
    event: On<DiscardAndCloseEditorView>,
    mut commands: Commands,
    mut session: ResMut<EditorSession>,
    mut catalog: ResMut<crate::ProjectEffectCatalog>,
    documents: Res<DocumentManager>,
    views: Res<EditorViewManager>,
    mut program_history: Option<ResMut<crate::history::MaterialProgramEditHistory>>,
    mut function_editor: Option<ResMut<crate::material_function_editor::FunctionEditor>>,
) {
    let view = event.0;
    if let Some(key) = view_document_key(view, &views, &documents) {
        let root = catalog.root().to_owned();
        match key {
            DocumentKey::MaterialProgram(id) => {
                catalog.material_drafts.programs.remove(&id);
                if let Some(history) = program_history.as_mut() {
                    history.clear_program(&root, id);
                }
            }
            DocumentKey::MaterialFunction(id) => {
                catalog.material_drafts.functions.remove(&id);
                if let Some(editor) = function_editor.as_mut() {
                    editor.clear_function(&root, id);
                }
            }
            // WESL discard lands in a later slice; read-only WESL never reaches the prompt.
            DocumentKey::WeslSource(_) => {}
        }
        catalog.refresh();
        let drafts = catalog.material_drafts.clone();
        session.set_material_drafts(drafts);
    }
    commands.trigger(CloseEditorView { view, force: true });
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
    fn reconcile_tracks_the_active_document_across_target_changes() {
        use aestra_core::MaterialProgramId;
        use std::path::PathBuf;

        fn program_target(seed: u128) -> MaterialEditingTarget {
            MaterialEditingTarget::Program {
                root: PathBuf::from("project"),
                id: MaterialProgramId::from_u128(seed),
            }
        }

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();

        reconcile_active_from_target(
            &program_target(0xa),
            &mut documents,
            &mut views,
            &mut active,
        );
        let doc_a = active.active_document.unwrap();
        reconcile_active_from_target(
            &program_target(0xb),
            &mut documents,
            &mut views,
            &mut active,
        );
        let doc_b = active.active_document.unwrap();
        assert_ne!(doc_a, doc_b);
        assert_eq!(documents.len(), 2);
        assert_eq!(views.len(), 2);

        // Switching back to A focuses the existing document/view rather than creating a new one.
        reconcile_active_from_target(
            &program_target(0xa),
            &mut documents,
            &mut views,
            &mut active,
        );
        assert_eq!(active.active_document, Some(doc_a));
        assert_eq!(documents.len(), 2);
        assert_eq!(views.len(), 2);

        // Returning to the effect's inline material clears the active shared-document context.
        reconcile_active_from_target(
            &MaterialEditingTarget::EffectInstance,
            &mut documents,
            &mut views,
            &mut active,
        );
        assert_eq!(active.active_document, None);
        assert_eq!(active.active_view, None);
    }

    #[test]
    fn view_editing_target_resolves_program_views_and_ignores_stale_ones() {
        use crate::document::DocumentKey;
        use aestra_core::MaterialProgramId;
        use std::path::Path;

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        let key = DocumentKey::MaterialProgram(MaterialProgramId::from_u128(0x7));
        let view = open_document_view(
            &mut documents,
            &mut views,
            &mut active,
            key,
            EditorViewKind::MaterialGraph,
        );

        let target = view_editing_target(view, &views, &documents, Path::new("project")).unwrap();
        assert_eq!(target.program(), Some(MaterialProgramId::from_u128(0x7)));

        // A view id with no backing view resolves to nothing, so the dock can drop it.
        assert!(
            view_editing_target(EditorViewId(999), &views, &documents, Path::new("project"))
                .is_none()
        );
    }

    #[test]
    fn manifest_captures_shared_documents_and_view_kinds_across_a_rebuild() {
        use crate::document::DocumentKey;
        use aestra_core::{MaterialFunctionId, MaterialProgramId};

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let prog = documents.open(DocumentKey::MaterialProgram(MaterialProgramId::from_u128(
            0x1,
        )));
        let func = documents.open(DocumentKey::MaterialFunction(
            MaterialFunctionId::from_u128(0x2),
        ));
        let v1 = views.create_view(prog, EditorViewKind::MaterialGraph);
        // A second view of the same program (split): must share one document after restore.
        let v2 = views.create_view(prog, EditorViewKind::MaterialGraph);
        let v3 = views.create_view(func, EditorViewKind::MaterialFunctionGraph);

        let manifest = EditorWorkspaceManifest::from_managers(&views, &documents);
        assert_eq!(manifest.views.len(), 3);
        // Round-trips through RON unchanged.
        let encoded = ron::to_string(&manifest).unwrap();
        assert_eq!(
            ron::from_str::<EditorWorkspaceManifest>(&encoded).unwrap(),
            manifest
        );

        let (docs2, mut views2) = rebuild_managers(&manifest);
        assert_eq!(views2.len(), 3);
        assert_eq!(docs2.len(), 2);
        assert_eq!(views2.document_of(v1), views2.document_of(v2));
        assert_ne!(views2.document_of(v1), views2.document_of(v3));
        assert_eq!(
            views2.view(v3).unwrap().kind,
            EditorViewKind::MaterialFunctionGraph
        );
        // A freshly created view never reuses a restored id.
        let doc = views2.document_of(v1).unwrap();
        let fresh = views2.create_view(doc, EditorViewKind::MaterialGraph);
        assert!(fresh.0 > v3.0);
    }

    #[test]
    fn restore_prunes_editor_tabs_without_a_restored_view() {
        use crate::docking::ToolPanel;

        let mut layout = WorkspaceLayout::default();
        layout.show(ToolPanel::MaterialGraph);
        layout.show_editor(EditorViewId(7));
        layout.show_editor(EditorViewId(9));
        assert_eq!(layout.editor_views().len(), 2);

        // Only view 7 was restored from the manifest; the orphaned tab for 9 is dropped.
        let known: HashSet<EditorViewId> = [EditorViewId(7)].into_iter().collect();
        assert!(layout.prune_editor_views(&known));
        assert_eq!(layout.editor_views(), vec![EditorViewId(7)]);
        // Pruning again with the same set is a no-op.
        assert!(!layout.prune_editor_views(&known));
    }

    #[test]
    fn workspace_survives_restart_restoring_both_editor_tabs() {
        use crate::docking::ToolPanel;
        use crate::document::DocumentKey;
        use aestra_core::MaterialProgramId;
        use std::path::Path;

        // Session 1: open two materials; each docks its own editor tab.
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
        let mut layout = WorkspaceLayout::default();
        layout.show(ToolPanel::MaterialGraph);
        assert!(layout.show_editor(view_a));
        assert!(layout.show_editor(view_b));

        // Persist and restart: the manifest is written, then read back into fresh managers, and the
        // persisted dock tree (stood in for by a clone) is reconciled against them.
        let manifest = EditorWorkspaceManifest::from_managers(&views, &documents);
        let restored: EditorWorkspaceManifest =
            ron::from_str(&ron::to_string(&manifest).unwrap()).unwrap();
        let (documents, views) = rebuild_managers(&restored);
        let mut layout = layout.clone();

        // Nothing is pruned — both editor tabs still resolve to their programs.
        let known: HashSet<EditorViewId> = views.iter().map(|view| view.id).collect();
        assert!(!layout.prune_editor_views(&known));
        assert_eq!(layout.editor_views().len(), 2);
        assert_eq!(
            view_editing_target(view_a, &views, &documents, Path::new("project"))
                .unwrap()
                .program(),
            Some(MaterialProgramId::from_u128(0xa))
        );
        assert_eq!(
            view_editing_target(view_b, &views, &documents, Path::new("project"))
                .unwrap()
                .program(),
            Some(MaterialProgramId::from_u128(0xb))
        );
    }

    fn program_view_app(ids: &[aestra_core::MaterialProgramId]) -> (App, Vec<EditorViewId>) {
        use crate::docking::ToolPanel;
        use crate::document::DocumentKey;

        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let mut active = ActiveEditorContext::default();
        let mut layout = WorkspaceLayout::default();
        layout.show(ToolPanel::MaterialGraph);
        let mut view_ids = Vec::new();
        for id in ids {
            let view = open_document_view(
                &mut documents,
                &mut views,
                &mut active,
                DocumentKey::MaterialProgram(*id),
                EditorViewKind::MaterialGraph,
            );
            layout.show_editor(view);
            view_ids.push(view);
        }
        let mut app = App::new();
        app.add_observer(close_editor_view);
        app.insert_resource(layout);
        app.insert_resource(documents);
        app.insert_resource(views);
        app.insert_resource(active);
        app.insert_resource(crate::test_support::session_with_timing_slack());
        // The close observer reads the catalog (dirty check) and the protection dialog state; an
        // empty catalog has no drafts, so these views close without a dirty-close prompt.
        app.insert_resource(crate::ProjectEffectCatalog::from_entries(Vec::new()));
        app.init_resource::<crate::persistence::DocumentProtectionState>();
        (app, view_ids)
    }

    #[test]
    fn closing_the_active_editor_drops_its_document_and_returns_to_the_effect() {
        use crate::document::DocumentKey;
        use aestra_core::MaterialProgramId;
        use std::path::PathBuf;

        let id_a = MaterialProgramId::from_u128(0xa);
        let id_b = MaterialProgramId::from_u128(0xb);
        let (mut app, views) = program_view_app(&[id_a, id_b]);
        let (view_a, view_b) = (views[0], views[1]);
        // The second view is active and steers the material target.
        app.world_mut()
            .resource_mut::<EditorSession>()
            .material_target = MaterialEditingTarget::Program {
            root: PathBuf::from("project"),
            id: id_b,
        };

        app.world_mut().trigger(CloseEditorView::requested(view_b));
        app.update();

        assert_eq!(
            app.world().resource::<WorkspaceLayout>().editor_views(),
            vec![view_a]
        );
        assert_eq!(app.world().resource::<EditorViewManager>().len(), 1);
        let documents = app.world().resource::<DocumentManager>();
        assert!(
            documents
                .find(&DocumentKey::MaterialProgram(id_b))
                .is_none()
        );
        assert!(
            documents
                .find(&DocumentKey::MaterialProgram(id_a))
                .is_some()
        );
        assert_eq!(
            app.world().resource::<ActiveEditorContext>().active_view,
            None
        );
        // The closed view was the target, so editing returns to the effect material.
        assert!(
            app.world()
                .resource::<EditorSession>()
                .standalone_material()
                .is_none()
        );
    }

    #[test]
    fn closing_a_non_target_editor_leaves_the_current_target_untouched() {
        use aestra_core::MaterialProgramId;
        use std::path::PathBuf;

        let id_a = MaterialProgramId::from_u128(0xa);
        let id_b = MaterialProgramId::from_u128(0xb);
        let (mut app, views) = program_view_app(&[id_a, id_b]);
        let view_a = views[0];
        // Target is B; close A (not the target).
        app.world_mut()
            .resource_mut::<EditorSession>()
            .material_target = MaterialEditingTarget::Program {
            root: PathBuf::from("project"),
            id: id_b,
        };

        app.world_mut().trigger(CloseEditorView::requested(view_a));
        app.update();

        assert_eq!(app.world().resource::<EditorViewManager>().len(), 1);
        // The target still edits B.
        assert_eq!(
            app.world()
                .resource::<EditorSession>()
                .standalone_material(),
            Some(id_b)
        );
    }

    #[test]
    fn missing_asset_restore_drops_only_the_gone_document() {
        use crate::docking::ToolPanel;
        use crate::document::DocumentKey;
        use aestra_core::MaterialProgramId;

        // Restore two materials; the first asset was deleted while the editor was closed.
        let present = MaterialProgramId::from_u128(0xa);
        let gone = MaterialProgramId::from_u128(0xb);
        let mut documents = DocumentManager::default();
        let mut views = EditorViewManager::default();
        let doc_present = documents.open(DocumentKey::MaterialProgram(present));
        let doc_gone = documents.open(DocumentKey::MaterialProgram(gone));
        let view_present = views.create_view(doc_present, EditorViewKind::MaterialGraph);
        let view_gone = views.create_view(doc_gone, EditorViewKind::MaterialGraph);

        let missing = views_with_missing_assets(&views, &documents, |key| {
            key == DocumentKey::MaterialProgram(gone)
        });
        assert_eq!(missing, vec![view_gone]);

        // Applying the decision drops the gone tab/view/document and keeps the present one.
        let mut layout = WorkspaceLayout::default();
        layout.show(ToolPanel::MaterialGraph);
        layout.show_editor(view_present);
        layout.show_editor(view_gone);
        for view in missing {
            assert!(layout.close_editor(view));
            let (_, orphaned) = views.close_view(view).unwrap();
            assert!(orphaned);
            documents.close(doc_gone);
        }
        assert_eq!(layout.editor_views(), vec![view_present]);
        assert_eq!(views.len(), 1);
        assert!(
            documents
                .find(&DocumentKey::MaterialProgram(gone))
                .is_none()
        );
        assert!(
            documents
                .find(&DocumentKey::MaterialProgram(present))
                .is_some()
        );
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
