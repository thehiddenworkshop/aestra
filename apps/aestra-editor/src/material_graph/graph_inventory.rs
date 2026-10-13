//! Actual project-wide graph inventory including drafts and built-in effect materials.
use crate::{ProjectEffectCatalog, session::EditorSession};
use aestra_core::material::MaterialProgram;
use std::collections::BTreeMap;

/// Include inactive/shared documents, not just the currently focused effect or function tab.
pub(super) fn programs(
    catalog: &ProjectEffectCatalog,
    session: &EditorSession,
) -> Vec<MaterialProgram> {
    let mut programs = BTreeMap::new();
    for entry in catalog.content().asset_index().material_programs() {
        if let Some(reference) = entry.reference
            && let Ok(program) = catalog.material_program(reference.id())
        {
            programs.insert(program.id, program);
        }
    }
    for (id, draft) in &catalog.material_drafts.programs {
        if let Some(program) = &draft.current {
            programs.insert(*id, program.clone());
        } else {
            programs.remove(id);
        }
    }
    // Built-ins have no project source entry. Keep them even when another effect reference is
    // unresolved, and never let the active effect reintroduce a tombstoned shared draft.
    for instance in &session.effect.material_instances {
        if let Some(program) = MaterialProgram::built_in(instance.program) {
            programs.insert(program.id, program);
        }
    }
    programs.into_values().collect()
}
