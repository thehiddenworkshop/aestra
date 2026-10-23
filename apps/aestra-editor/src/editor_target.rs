//! View/document-to-asset routing; no session, catalog, docking or filesystem adapter.
use crate::{
    document_registry::DocumentManager,
    editor_identity::{DocumentKey, EditorViewId, WeslSourceId},
    editor_view_registry::{
        ActiveEditorContext, EditorViewKind, EditorViewManager, open_document_view,
    },
    material_target::MaterialEditingTarget,
};

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

/// The shared-material document currently focused in an editor view. Unlike the session's
/// compatibility target, this follows the view/document registry even if another operation last
/// changed `material_target`.
pub(crate) fn active_material_target(
    active: &ActiveEditorContext,
    views: &EditorViewManager,
    documents: &DocumentManager,
    root: &std::path::Path,
) -> Option<MaterialEditingTarget> {
    let view = active.active_view?;
    let document = active.active_document?;
    (views.document_of(view) == Some(document))
        .then(|| view_editing_target(view, views, documents, root))
        .flatten()
}

/// The editor kind a view renders (material graph vs WESL source), so the dock can pick the pane.
pub(crate) fn view_kind(view: EditorViewId, views: &EditorViewManager) -> Option<EditorViewKind> {
    views.view(view).map(|view| view.kind)
}

/// The editor view showing a given WESL source, if one is open — so the diagnostics panel can
/// reveal the tab for a shader error.
pub(crate) fn view_for_wesl_source(
    id: WeslSourceId,
    views: &EditorViewManager,
    documents: &DocumentManager,
) -> Option<EditorViewId> {
    views.iter().find_map(|view| {
        let key = documents.document(view.document)?.key;
        (key == DocumentKey::WeslSource(id)).then_some(view.id)
    })
}

/// The WESL source id an editor view edits, if it is a WESL document view.
pub(crate) fn view_wesl_source(
    view: EditorViewId,
    views: &EditorViewManager,
    documents: &DocumentManager,
) -> Option<WeslSourceId> {
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
            // Returning to the effect material clears the active shared-material document, but a
            // focused WESL module has no material target, so it must stay active.
            let active_is_wesl = active
                .active_document
                .and_then(|id| documents.document(id))
                .is_some_and(|document| matches!(document.key, DocumentKey::WeslSource(_)));
            if !active_is_wesl {
                active.active_view = None;
                active.active_document = None;
            }
        }
    }
}

/// Resolves the document key an editor view edits, if the view is still open.
pub(crate) fn view_document_key(
    view: EditorViewId,
    views: &EditorViewManager,
    documents: &DocumentManager,
) -> Option<DocumentKey> {
    documents
        .document(views.document_of(view)?)
        .map(|open| open.key)
}

#[cfg(test)]
#[path = "tests/editor_target.rs"]
mod tests;
