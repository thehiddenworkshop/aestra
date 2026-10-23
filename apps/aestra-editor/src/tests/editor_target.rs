use super::*;
use crate::editor_view_registry::open_document_view_in_new_tab;
use aestra_core::{MaterialFunctionId, MaterialProgramId};
use std::path::{Path, PathBuf};

fn program(seed: u128) -> DocumentKey {
    DocumentKey::MaterialProgram(MaterialProgramId::from_u128(seed))
}

#[test]
fn program_and_function_views_resolve_real_scoped_targets_and_kinds() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    let root = Path::new("project-a");
    let id = MaterialProgramId::from_u128(1);
    let view = open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        DocumentKey::MaterialProgram(id),
        EditorViewKind::MaterialGraph,
    );
    let target = active_material_target(&active, &views, &documents, root).unwrap();
    assert_eq!(
        target,
        MaterialEditingTarget::Program {
            root: root.into(),
            id
        }
    );
    assert_eq!(target.program(), Some(id));
    assert_eq!(target.function(), None);
    assert_eq!(view_kind(view, &views), Some(EditorViewKind::MaterialGraph));
    let id = MaterialFunctionId::from_u128(1);
    let view = open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        DocumentKey::MaterialFunction(id),
        EditorViewKind::MaterialFunctionGraph,
    );
    let target = active_material_target(&active, &views, &documents, root).unwrap();
    assert_eq!(
        target,
        MaterialEditingTarget::Function {
            root: root.into(),
            id
        }
    );
    assert_eq!(target.function(), Some(id));
    assert_eq!(target.program(), None);
    assert_eq!(
        view_kind(view, &views),
        Some(EditorViewKind::MaterialFunctionGraph)
    );
    assert_eq!(documents.len(), 2);
}

#[test]
fn focused_view_not_a_compatibility_target_controls_routing() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    let a = open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        program(1),
        EditorViewKind::MaterialGraph,
    );
    open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        program(2),
        EditorViewKind::MaterialGraph,
    );
    assert_eq!(
        active_material_target(&active, &views, &documents, Path::new("root"))
            .unwrap()
            .program(),
        Some(MaterialProgramId::from_u128(2))
    );
    active.active_view = Some(a);
    active.active_document = views.document_of(a);
    assert_eq!(
        active_material_target(&active, &views, &documents, Path::new("root"))
            .unwrap()
            .program(),
        Some(MaterialProgramId::from_u128(1))
    );
}

#[test]
fn mismatched_or_incomplete_active_context_cannot_route_an_action() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let a = documents.open(program(1));
    let b = documents.open(program(2));
    let view = views.create_view(a, EditorViewKind::MaterialGraph);
    for active in [
        ActiveEditorContext {
            active_view: Some(view),
            active_document: Some(b),
        },
        ActiveEditorContext {
            active_view: Some(view),
            active_document: None,
        },
        ActiveEditorContext {
            active_view: None,
            active_document: Some(a),
        },
    ] {
        assert!(active_material_target(&active, &views, &documents, Path::new("root")).is_none());
    }
}

#[test]
fn closed_views_and_closed_documents_do_not_resolve() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let document = documents.open(program(1));
    let view = views.create_view(document, EditorViewKind::MaterialGraph);
    documents.close(document);
    assert_eq!(view_document_key(view, &views, &documents), None);
    assert_eq!(
        view_editing_target(view, &views, &documents, Path::new("root")),
        None
    );
    views.close_view(view);
    assert_eq!(view_kind(view, &views), None);
    assert_eq!(view_document_key(view, &views, &documents), None);
}

#[test]
fn split_focus_is_preserved_when_reconciling_the_same_target() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        program(1),
        EditorViewKind::MaterialGraph,
    );
    let split = open_document_view_in_new_tab(
        &mut documents,
        &mut views,
        &mut active,
        program(1),
        EditorViewKind::MaterialGraph,
    );
    let target = MaterialEditingTarget::Program {
        root: "root".into(),
        id: MaterialProgramId::from_u128(1),
    };
    reconcile_active_from_target(&target, &mut documents, &mut views, &mut active);
    assert_eq!(active.active_view, Some(split));
    assert_eq!(documents.len(), 1);
    assert_eq!(views.len(), 2);
}

#[test]
fn reconciliation_opens_and_refocuses_assets_then_clears_inline_context() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    let a = MaterialEditingTarget::Program {
        root: "root".into(),
        id: MaterialProgramId::from_u128(1),
    };
    let b = MaterialEditingTarget::Function {
        root: "root".into(),
        id: MaterialFunctionId::from_u128(2),
    };
    reconcile_active_from_target(&a, &mut documents, &mut views, &mut active);
    let first_view = active.active_view;
    reconcile_active_from_target(&b, &mut documents, &mut views, &mut active);
    assert_ne!(active.active_view, first_view);
    reconcile_active_from_target(&a, &mut documents, &mut views, &mut active);
    assert_eq!(active.active_view, first_view);
    reconcile_active_from_target(
        &MaterialEditingTarget::EffectInstance,
        &mut documents,
        &mut views,
        &mut active,
    );
    assert!(active.active_view.is_none());
    assert!(active.active_document.is_none());
    assert_eq!(documents.len(), 2);
    assert_eq!(views.len(), 2);
}

#[test]
fn wesl_focus_is_not_a_material_target_and_survives_inline_reconciliation() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let mut active = ActiveEditorContext::default();
    let id = WeslSourceId::for_relative_path(Path::new("shaders/smoke.wesl"));
    let view = open_document_view(
        &mut documents,
        &mut views,
        &mut active,
        DocumentKey::WeslSource(id),
        EditorViewKind::WeslSource,
    );
    assert_eq!(view_wesl_source(view, &views, &documents), Some(id));
    assert_eq!(view_for_wesl_source(id, &views, &documents), Some(view));
    assert_eq!(
        active_material_target(&active, &views, &documents, Path::new("root")),
        None
    );
    reconcile_active_from_target(
        &MaterialEditingTarget::EffectInstance,
        &mut documents,
        &mut views,
        &mut active,
    );
    assert_eq!(active.active_view, Some(view));
    views.close_view(view);
    assert_eq!(view_for_wesl_source(id, &views, &documents), None);
    assert_eq!(view_wesl_source(view, &views, &documents), None);
}

#[test]
fn target_roots_follow_current_project_without_mutating_document_identity() {
    let mut documents = DocumentManager::default();
    let mut views = EditorViewManager::default();
    let document = documents.open(program(1));
    let view = views.create_view(document, EditorViewKind::MaterialGraph);
    for root in [PathBuf::from("project-a"), PathBuf::from("project-b")] {
        assert_eq!(
            view_editing_target(view, &views, &documents, &root),
            Some(MaterialEditingTarget::Program {
                root,
                id: MaterialProgramId::from_u128(1)
            })
        );
    }
    assert_eq!(
        view_document_key(view, &views, &documents),
        Some(program(1))
    );
    assert_eq!(documents.len(), 1);
    assert_eq!(MaterialEditingTarget::default().program(), None);
    assert_eq!(MaterialEditingTarget::default().function(), None);
}
