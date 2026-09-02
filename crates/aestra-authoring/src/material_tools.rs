//! UI-independent, high-level material transformations built from baseline commands.

use crate::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialCommandExecutor,
    MaterialDiff, MaterialTransaction,
};
use aestra_compiler::{
    MaterialCompiler, MaterialStackEditError, MaterialStackModifierKind, MaterialStackPresetKind,
    MaterialStackProjection,
};
use aestra_core::{MaterialExpressionId, MaterialProgramId, material::MaterialProgram};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Stable placement for an inserted material operation or preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialInsertionPoint {
    Start,
    Before(MaterialExpressionId),
    After(MaterialExpressionId),
    End,
}

/// A semantic material edit request suitable for editor, CLI, and tool clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialToolCommand {
    InsertMaterialOperation {
        program: MaterialProgramId,
        kind: MaterialStackModifierKind,
        placement: MaterialInsertionPoint,
    },
    ApplyMaterialPreset {
        program: MaterialProgramId,
        preset: MaterialStackPresetKind,
        placement: MaterialInsertionPoint,
    },
}

/// A validated baseline transaction and the semantic changes it will make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialToolPlan {
    pub command: MaterialToolCommand,
    pub transaction: MaterialTransaction,
    pub diff: MaterialDiff,
    pub created_expressions: Vec<MaterialExpressionId>,
}

impl MaterialToolPlan {
    /// Returns the planned replacement for `program`, when the command replaces that program.
    pub fn replacement_program(&self, program: MaterialProgramId) -> Option<&MaterialProgram> {
        self.transaction
            .commands
            .iter()
            .find_map(|command| match command {
                MaterialCommand::ReplaceMaterialProgram {
                    id,
                    program: replacement,
                } if *id == program => Some(replacement),
                _ => None,
            })
    }
}

#[derive(Debug, Error)]
pub enum MaterialToolError {
    #[error("material program '{0}' was not found")]
    ProgramNotFound(MaterialProgramId),
    #[error("material insertion anchor '{0}' is not present in the editable stack")]
    InsertionAnchorNotFound(MaterialExpressionId),
    #[error(transparent)]
    StackEdit(#[from] MaterialStackEditError),
    #[error(transparent)]
    Transaction(#[from] MaterialCommandError),
}

/// Plans high-level material edits without mutating the source document.
#[derive(Debug, Default)]
pub struct MaterialToolPlanner;

impl MaterialToolPlanner {
    pub fn plan(
        document: &MaterialAuthoringDocument,
        command: MaterialToolCommand,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        match command {
            MaterialToolCommand::InsertMaterialOperation {
                program,
                kind,
                placement,
            } => Self::plan_insert_material_operation(document, program, kind, placement),
            MaterialToolCommand::ApplyMaterialPreset {
                program,
                preset,
                placement,
            } => Self::plan_apply_material_preset(document, program, preset, placement),
        }
    }

    fn plan_insert_material_operation(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        kind: MaterialStackModifierKind,
        placement: MaterialInsertionPoint,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let target_index = resolve_insertion_point(program, placement)?;
        let insert_plan = MaterialCompiler.plan_stack_insert(program, kind, target_index)?;
        let command = MaterialToolCommand::InsertMaterialOperation {
            program: program_id,
            kind,
            placement,
        };
        let transaction = MaterialTransaction::single(
            format!("Insert {} material operation", kind.display_name()),
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: insert_plan.replacement,
            },
        );
        validate_plan(document, command, transaction, vec![insert_plan.expression])
    }

    fn plan_apply_material_preset(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        preset: MaterialStackPresetKind,
        placement: MaterialInsertionPoint,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let target_index = resolve_insertion_point(program, placement)?;
        let preset_plan =
            MaterialCompiler.plan_stack_insert_preset(program, preset, target_index)?;
        let command = MaterialToolCommand::ApplyMaterialPreset {
            program: program_id,
            preset,
            placement,
        };
        let transaction = MaterialTransaction::single(
            format!("Apply {} preset", preset.display_name()),
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: preset_plan.replacement,
            },
        );

        validate_plan(document, command, transaction, preset_plan.expressions)
    }
}

fn find_program(
    document: &MaterialAuthoringDocument,
    program_id: MaterialProgramId,
) -> Result<&MaterialProgram, MaterialToolError> {
    document
        .programs
        .iter()
        .find(|program| program.id == program_id)
        .ok_or(MaterialToolError::ProgramNotFound(program_id))
}

fn resolve_insertion_point(
    program: &MaterialProgram,
    placement: MaterialInsertionPoint,
) -> Result<usize, MaterialToolError> {
    let projection = MaterialCompiler
        .project_stack(program)
        .map_err(MaterialStackEditError::from)?;
    let MaterialStackProjection::Stack { entries } = projection else {
        return Err(MaterialStackEditError::Advanced.into());
    };
    match placement {
        MaterialInsertionPoint::Start => Ok(0),
        MaterialInsertionPoint::End => Ok(entries.len()),
        MaterialInsertionPoint::Before(anchor) => entries
            .iter()
            .position(|entry| entry.expression == anchor)
            .ok_or(MaterialToolError::InsertionAnchorNotFound(anchor)),
        MaterialInsertionPoint::After(anchor) => entries
            .iter()
            .position(|entry| entry.expression == anchor)
            .map(|index| index + 1)
            .ok_or(MaterialToolError::InsertionAnchorNotFound(anchor)),
    }
}

fn validate_plan(
    document: &MaterialAuthoringDocument,
    command: MaterialToolCommand,
    transaction: MaterialTransaction,
    created_expressions: Vec<MaterialExpressionId>,
) -> Result<MaterialToolPlan, MaterialToolError> {
    let mut preview = document.clone();
    let outcome = MaterialCommandExecutor::execute(&mut preview, &transaction)?;
    Ok(MaterialToolPlan {
        command,
        transaction,
        diff: outcome.diff,
        created_expressions,
    })
}
