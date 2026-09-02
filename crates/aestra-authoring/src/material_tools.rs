//! UI-independent, high-level material transformations built from baseline commands.

use crate::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialCommandExecutor,
    MaterialDiff, MaterialTransaction,
};
use aestra_compiler::{MaterialCompiler, MaterialStackEditError, MaterialStackPresetKind};
use aestra_core::{MaterialExpressionId, MaterialProgramId, material::MaterialProgram};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A semantic material edit request suitable for editor, CLI, and tool clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialToolCommand {
    ApplyMaterialPreset {
        program: MaterialProgramId,
        preset: MaterialStackPresetKind,
        target_index: usize,
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
            MaterialToolCommand::ApplyMaterialPreset {
                program,
                preset,
                target_index,
            } => Self::plan_apply_material_preset(document, program, preset, target_index),
        }
    }

    fn plan_apply_material_preset(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        preset: MaterialStackPresetKind,
        target_index: usize,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = document
            .programs
            .iter()
            .find(|program| program.id == program_id)
            .ok_or(MaterialToolError::ProgramNotFound(program_id))?;
        let preset_plan =
            MaterialCompiler.plan_stack_insert_preset(program, preset, target_index)?;
        let command = MaterialToolCommand::ApplyMaterialPreset {
            program: program_id,
            preset,
            target_index,
        };
        let transaction = MaterialTransaction::single(
            format!("Apply {} preset", preset.display_name()),
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: preset_plan.replacement,
            },
        );

        let mut preview = document.clone();
        let outcome = MaterialCommandExecutor::execute(&mut preview, &transaction)?;
        Ok(MaterialToolPlan {
            command,
            transaction,
            diff: outcome.diff,
            created_expressions: preset_plan.expressions,
        })
    }
}
