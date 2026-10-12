//! Multi-document registry (Milestone 2).
//!
//! Owns the identity and lifecycle of open authored documents — material programs and functions
//! today, WESL sources and effects later. This is the generic owner the multi-document editor
//! routes opening/saving/closing through, replacing the single global `material_target`.
//!
//! Introduced here as a standalone, tested subsystem; it is wired into the open flow, the material
//! graph, and the save/close/undo lifecycle in Milestones 3–6.
#![allow(dead_code)] // Consumed by Milestones 3–6.

pub(crate) use crate::editor_identity::{DocumentId, DocumentKey};

/// Asset display names shared by editor tabs and document save/close UI.
pub(crate) fn document_display_name(
    key: DocumentKey,
    catalog: &crate::ProjectEffectCatalog,
    wesl: &crate::wesl_document::WeslDocuments,
) -> String {
    match key {
        DocumentKey::MaterialProgram(id) => catalog
            .material_program(id)
            .map(|program| program.name)
            .unwrap_or_else(|_| format!("Material {id}")),
        DocumentKey::MaterialFunction(id) => catalog
            .material_functions()
            .ok()
            .and_then(|functions| functions.into_iter().find(|function| function.id == id))
            .map(|function| function.name)
            .unwrap_or_else(|| format!("Function {id}")),
        DocumentKey::WeslSource(id) => wesl
            .relative_path(id)
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("WESL {id}")),
    }
}

pub(crate) use crate::document_registry::DocumentManager;
