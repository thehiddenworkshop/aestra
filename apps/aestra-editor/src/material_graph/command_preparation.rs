//! Semantic preparation used by the editor before host draft/history publication.
//! Planning and validation operate on a detached document; failure cannot partially publish edits.
use aestra_authoring::{
    MaterialAuthoringDocument, MaterialCommandExecutor, MaterialToolCommand, MaterialToolPlan,
    MaterialToolPlanner,
};
use aestra_core::{MaterialProgramId, material::MaterialProgram};

pub(super) struct PreparedEdit {
    pub(super) plan: MaterialToolPlan,
    pub(super) before: MaterialProgram,
    pub(super) after: MaterialProgram,
}

pub(super) fn prepare(
    document: MaterialAuthoringDocument,
    program: MaterialProgramId,
    command: MaterialToolCommand,
) -> Result<PreparedEdit, String> {
    let before = document
        .programs
        .iter()
        .find(|candidate| candidate.id == program)
        .cloned()
        .ok_or_else(|| format!("Material program {program} is unavailable"))?;
    let plan = MaterialToolPlanner::plan(&document, command).map_err(|error| error.to_string())?;
    let mut preview = document;
    MaterialCommandExecutor::execute(&mut preview, &plan.transaction)
        .map_err(|error| error.to_string())?;
    let after = preview
        .programs
        .into_iter()
        .find(|candidate| candidate.id == program)
        .ok_or_else(|| format!("material tool plan removed program {program}"))?;
    Ok(PreparedEdit {
        plan,
        before,
        after,
    })
}

#[cfg(test)]
#[path = "tests/command_preparation.rs"]
mod tests;
