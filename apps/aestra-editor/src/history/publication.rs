//! Production material/function publication and compensating rollback.
//! The real catalog owns source/draft validation; only session preview installation is supplied.
use crate::project_content::EditorProjectContent;
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandExecutor, MaterialTransaction,
};
use aestra_core::material::{MaterialFunction, MaterialProgram};
use aestra_runtime::CompiledEffect;
use std::sync::Arc;

pub(crate) fn replace_program(
    catalog: &mut EditorProjectContent,
    mut document: MaterialAuthoringDocument,
    label: &str,
    expected: &MaterialProgram,
    replacement: &MaterialProgram,
    install: impl FnOnce(Arc<CompiledEffect>) -> Result<(), String>,
) -> Result<(), String> {
    MaterialCommandExecutor::execute(
        &mut document,
        &MaterialTransaction::single(
            label,
            MaterialCommand::ReplaceMaterialProgram {
                id: expected.id,
                program: replacement.clone(),
            },
        ),
    )
    .map_err(|error| error.to_string())?;
    let replacement = document
        .programs
        .into_iter()
        .find(|program| program.id == expected.id)
        .ok_or_else(|| format!("material program {} disappeared", expected.id))?;
    catalog.replace_material_program(expected, &replacement)?;
    let Some(effect) = document.effect else {
        // Standalone source edits do not compile or mutate an unrelated active effect.
        return Ok(());
    };
    let compiled = match catalog.compile_project(&effect) {
        Ok(project) => project.root,
        Err(error) => return Err(rollback_program(catalog, &replacement, expected, error)),
    };
    if let Err(error) = install(compiled) {
        return Err(rollback_program(catalog, &replacement, expected, error));
    }
    Ok(())
}

fn rollback_program(
    catalog: &mut EditorProjectContent,
    current: &MaterialProgram,
    previous: &MaterialProgram,
    error: String,
) -> String {
    match catalog.replace_material_program(current, previous) {
        Ok(()) => error,
        Err(rollback) => {
            format!("{error}; restoring the previous material also failed: {rollback}")
        }
    }
}

pub(crate) fn extract_function(
    catalog: &mut EditorProjectContent,
    function: &MaterialFunction,
    publish_program: impl FnOnce(&mut EditorProjectContent) -> Result<(), String>,
) -> Result<(), String> {
    catalog.create_material_function(function)?;
    if let Err(error) = publish_program(catalog) {
        return Err(match catalog.delete_material_function(function) {
            Ok(()) => error,
            Err(rollback) => {
                format!("{error}; removing the extracted function also failed: {rollback}")
            }
        });
    }
    Ok(())
}

pub(crate) fn undo_extraction(
    catalog: &mut EditorProjectContent,
    label: &str,
    before: &MaterialProgram,
    after: &MaterialProgram,
    function: &MaterialFunction,
    mut publish_program: impl FnMut(
        &mut EditorProjectContent,
        &str,
        &MaterialProgram,
        &MaterialProgram,
    ) -> Result<(), String>,
) -> Result<(), String> {
    publish_program(catalog, &format!("Undo {label}"), after, before)?;
    if let Err(error) = catalog.delete_material_function(function) {
        return Err(
            match publish_program(
                catalog,
                &format!("Restore {label} after failed undo"),
                before,
                after,
            ) {
                Ok(()) => error,
                Err(rollback) => format!(
                    "{error}; restoring the material after the failed function removal also failed: {rollback}"
                ),
            },
        );
    }
    Ok(())
}

pub(crate) fn replace_function(
    catalog: &mut EditorProjectContent,
    current: &MaterialFunction,
    before: &MaterialFunction,
    after: &MaterialFunction,
) -> Result<String, String> {
    if current.normalized() != before.normalized() {
        return Err("Function changed since this edit was prepared".into());
    }
    // Validate every indexed program/draft and function, not just active effect consumers.
    let programs = catalog
        .content()
        .asset_index()
        .material_programs()
        .iter()
        .filter_map(|entry| entry.reference.map(|reference| reference.id()))
        .map(|id| catalog.material_program(id))
        .collect::<Result<Vec<_>, _>>()?;
    let document = MaterialAuthoringDocument::standalone(programs)
        .with_material_functions(catalog.material_functions()?);
    let plan = document
        .plan_function_edit(before.id, after.clone())
        .map_err(|error| error.to_string())?;
    let report = format!(
        "{} direct call sites checked (indexed programs and functions).\n{}",
        plan.call_sites.len(),
        plan.diagnostics
    );
    if !plan.diagnostics.is_valid() {
        return Err(report);
    }
    catalog.replace_material_function(before, after)?;
    Ok(report)
}

#[cfg(test)]
#[path = "tests/publication.rs"]
mod tests;
