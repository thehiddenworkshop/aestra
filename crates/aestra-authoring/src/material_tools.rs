//! UI-independent, high-level material transformations built from baseline commands.

use crate::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialCommandExecutor,
    MaterialDiff, MaterialExpressionInput, MaterialOutputSocket, MaterialTransaction,
    material_authoring::{material_expression_input_source, rewire_expression},
};
use aestra_compiler::{
    MaterialCompiler, MaterialStackEditError, MaterialStackModifierKind, MaterialStackPresetKind,
    MaterialStackProjection,
};
use aestra_core::{
    MaterialExpressionId, MaterialProgramId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialProgram},
};
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

/// Stable destination for a material expression connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialConnectionTarget {
    ExpressionInput {
        expression: MaterialExpressionId,
        input: MaterialExpressionInput,
    },
    ProgramOutput(MaterialOutputSocket),
}

/// A semantic material edit request suitable for editor, CLI, and tool clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialToolCommand {
    ReplaceMaterialExpression {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        replacement: MaterialExpressionKind,
    },
    WrapMaterialExpression {
        program: MaterialProgramId,
        target: MaterialConnectionTarget,
        kind: MaterialStackModifierKind,
    },
    ConnectMaterialExpression {
        program: MaterialProgramId,
        source: MaterialExpressionId,
        target: MaterialConnectionTarget,
    },
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
    #[error("source material expression '{0}' was not found")]
    SourceExpressionNotFound(MaterialExpressionId),
    #[error("destination material expression '{0}' was not found")]
    DestinationExpressionNotFound(MaterialExpressionId),
    #[error("{kind:?} cannot wrap {target:?} without changing graph meaning")]
    IncompatibleWrap {
        kind: MaterialStackModifierKind,
        target: MaterialConnectionTarget,
    },
    #[error("{kind:?} has more than one valid way to wrap {target:?}")]
    AmbiguousWrap {
        kind: MaterialStackModifierKind,
        target: MaterialConnectionTarget,
    },
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
            MaterialToolCommand::ReplaceMaterialExpression {
                program,
                expression,
                replacement,
            } => Self::plan_replace_material_expression(document, program, expression, replacement),
            MaterialToolCommand::WrapMaterialExpression {
                program,
                target,
                kind,
            } => Self::plan_wrap_material_expression(document, program, target, kind),
            MaterialToolCommand::ConnectMaterialExpression {
                program,
                source,
                target,
            } => Self::plan_connect_material_expression(document, program, source, target),
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

    fn plan_replace_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        expression_id: MaterialExpressionId,
        replacement: MaterialExpressionKind,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == expression_id)
        {
            return Err(MaterialToolError::DestinationExpressionNotFound(
                expression_id,
            ));
        }

        let command = MaterialToolCommand::ReplaceMaterialExpression {
            program: program_id,
            expression: expression_id,
            replacement: replacement.clone(),
        };
        let transaction = MaterialTransaction::single(
            "Replace material expression",
            MaterialCommand::ReplaceMaterialExpression {
                program: program_id,
                expression: expression_id,
                replacement: MaterialExpression {
                    id: expression_id,
                    kind: replacement,
                },
            },
        );
        validate_plan(document, command, transaction, Vec::new())
    }

    fn plan_wrap_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        target: MaterialConnectionTarget,
        kind: MaterialStackModifierKind,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let previous_source = connection_source(program, target)?;
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == previous_source)
        {
            return Err(MaterialToolError::SourceExpressionNotFound(previous_source));
        }

        let mut matched = None;
        for insertion in MaterialCompiler
            .stack_insert_targets(program)?
            .into_iter()
            .filter(|insertion| insertion.kind == kind)
        {
            let candidate = MaterialCompiler.plan_stack_insert(program, kind, insertion.index)?;
            let wraps_requested_edge = connection_source(&candidate.replacement, target)
                .is_ok_and(|source| source == candidate.expression);
            let consumes_previous_source = candidate
                .replacement
                .expressions
                .iter()
                .find(|expression| expression.id == candidate.expression)
                .and_then(|expression| expression.kind.bypass_input())
                == Some(previous_source);
            let changes_only_requested_edge = wrap_changes_only_requested_edge(
                program,
                &candidate.replacement,
                target,
                candidate.expression,
            );
            if !wraps_requested_edge || !consumes_previous_source || !changes_only_requested_edge {
                continue;
            }
            if matched.is_some() {
                return Err(MaterialToolError::AmbiguousWrap { kind, target });
            }
            matched = Some(candidate);
        }
        let wrap = matched.ok_or(MaterialToolError::IncompatibleWrap { kind, target })?;
        let command = MaterialToolCommand::WrapMaterialExpression {
            program: program_id,
            target,
            kind,
        };
        let transaction = MaterialTransaction::new(
            format!("Wrap material expression with {}", kind.display_name()),
            wrap_transaction_commands(program, &wrap.replacement, target, wrap.expression),
        );
        validate_plan(document, command, transaction, vec![wrap.expression])
    }

    fn plan_connect_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        source: MaterialExpressionId,
        target: MaterialConnectionTarget,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == source)
        {
            return Err(MaterialToolError::SourceExpressionNotFound(source));
        }
        connection_source(program, target)?;
        let baseline = connection_command(program_id, target, source);
        let command = MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source,
            target,
        };
        let transaction = MaterialTransaction::single("Connect material expression", baseline);
        validate_plan(document, command, transaction, Vec::new())
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

fn wrap_changes_only_requested_edge(
    before: &MaterialProgram,
    after: &MaterialProgram,
    target: MaterialConnectionTarget,
    wrapper: MaterialExpressionId,
) -> bool {
    if after.expressions.len() <= before.expressions.len()
        || after.expressions[..before.expressions.len()]
            .iter()
            .map(|expression| expression.id)
            .ne(before.expressions.iter().map(|expression| expression.id))
    {
        return false;
    }
    let mut expected = before.clone();
    expected
        .expressions
        .extend_from_slice(&after.expressions[before.expressions.len()..]);
    if apply_connection(&mut expected, target, wrapper).is_err() {
        return false;
    }
    expected == *after
}

fn wrap_transaction_commands(
    before: &MaterialProgram,
    after: &MaterialProgram,
    target: MaterialConnectionTarget,
    wrapper: MaterialExpressionId,
) -> Vec<MaterialCommand> {
    let mut commands = after.expressions[before.expressions.len()..]
        .iter()
        .cloned()
        .enumerate()
        .map(
            |(offset, expression)| MaterialCommand::AddMaterialExpression {
                program: before.id,
                expression,
                index: before.expressions.len() + offset,
            },
        )
        .collect::<Vec<_>>();
    commands.push(connection_command(before.id, target, wrapper));
    commands
}

fn apply_connection(
    program: &mut MaterialProgram,
    target: MaterialConnectionTarget,
    source: MaterialExpressionId,
) -> Result<(), MaterialCommandError> {
    match target {
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color) => {
            program.outputs.color = source;
        }
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha) => {
            program.outputs.alpha = source;
        }
        MaterialConnectionTarget::ExpressionInput { expression, input } => {
            let destination = program
                .expressions
                .iter_mut()
                .find(|candidate| candidate.id == expression)
                .ok_or_else(|| MaterialCommandError::NotFound {
                    kind: "material expression",
                    id: expression.to_string(),
                })?;
            rewire_expression(&mut destination.kind, input, source)?;
        }
    }
    Ok(())
}

fn connection_command(
    program: MaterialProgramId,
    target: MaterialConnectionTarget,
    source: MaterialExpressionId,
) -> MaterialCommand {
    match target {
        MaterialConnectionTarget::ExpressionInput { expression, input } => {
            MaterialCommand::RewireMaterialExpressionInput {
                program,
                expression,
                input,
                source,
            }
        }
        MaterialConnectionTarget::ProgramOutput(output) => MaterialCommand::SetMaterialOutput {
            program,
            output,
            expression: source,
        },
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

fn connection_source(
    program: &MaterialProgram,
    target: MaterialConnectionTarget,
) -> Result<MaterialExpressionId, MaterialToolError> {
    match target {
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color) => {
            Ok(program.outputs.color)
        }
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha) => {
            Ok(program.outputs.alpha)
        }
        MaterialConnectionTarget::ExpressionInput { expression, input } => {
            let destination = program
                .expressions
                .iter()
                .find(|candidate| candidate.id == expression)
                .ok_or(MaterialToolError::DestinationExpressionNotFound(expression))?;
            material_expression_input_source(&destination.kind, input)
                .map_err(MaterialToolError::Transaction)
        }
    }
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
