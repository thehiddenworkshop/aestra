//! Explicit target projection shared with restored/split material workspaces.
use crate::{EditorSession, ProjectEffectCatalog, material_document::MaterialEditingTarget};
use aestra_authoring::SemanticTarget;
use aestra_compiler::{MaterialCompiler, MaterialGraphProjection};
use aestra_core::{MaterialId, material::MaterialProgram};

/// Projects the graph for an explicit editing target, independent of the session's current target.
/// This is the per-view rendering primitive (M4c): an editor tab projects its own document's target
/// through this, while `selected_projection` delegates with the session target.
pub(super) fn selected_projection_for(
    target: &MaterialEditingTarget,
    session: &EditorSession,
    catalog: &ProjectEffectCatalog,
) -> Result<
    (
        String,
        MaterialGraphProjection,
        Option<MaterialId>,
        MaterialProgram,
    ),
    String,
> {
    if target.function().is_some() {
        return Err("Function inspection is active".into());
    }
    if target.program().is_some() {
        let document = session.graph_authoring_document_for(target, catalog)?;
        let program = &document.programs[0];
        let functions = document.material_function_library();
        let compiler = MaterialCompiler;
        let ir = compiler.compile_with_functions(program, &functions).ok();
        return Ok((
            program.name.clone(),
            compiler.project_graph_with_functions(program, ir.as_ref(), &functions),
            None,
            program.clone(),
        ));
    }
    let selected_renderer = match session.selection.primary {
        SemanticTarget::Renderer(id) => session
            .effect
            .emitters
            .iter()
            .flat_map(|emitter| &emitter.renderers)
            .find(|renderer| renderer.id == id),
        _ => session
            .selection
            .emitter(&session.effect)
            .and_then(|id| {
                session
                    .effect
                    .emitters
                    .iter()
                    .find(|emitter| emitter.id == id)
            })
            .and_then(|emitter| {
                emitter
                    .renderers
                    .iter()
                    .find(|renderer| renderer.enabled)
                    .or_else(|| emitter.renderers.first())
            }),
    }
    .ok_or_else(|| "selected emitter has no renderer".to_owned())?;
    let instance = session
        .effect
        .material_instances
        .iter()
        .find(|instance| instance.id == selected_renderer.material)
        .ok_or_else(|| "selected renderer does not use a semantic material".to_owned())?;
    let programs = session.graph_material_programs_for(target, catalog)?;
    let program = programs
        .iter()
        .find(|program| program.id == instance.program.id())
        .ok_or_else(|| "selected material program is unavailable".to_owned())?;
    let functions = catalog.material_function_library()?;
    let compiler = MaterialCompiler;
    let ir = compiler.compile_with_functions(program, &functions).ok();
    Ok((
        program.name.clone(),
        compiler.project_graph_with_functions(program, ir.as_ref(), &functions),
        Some(instance.id),
        program.clone(),
    ))
}
