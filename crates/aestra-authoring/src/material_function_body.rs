//! Function-native graph mutations. Validation and history are owned by the transaction executor.
use crate::{MaterialCommandError, MaterialExpressionInput};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialFunction},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialFunctionBodyCommand {
    Add {
        expression: MaterialExpression,
        index: usize,
    },
    Remove {
        expression: MaterialExpressionId,
    },
    Replace {
        expression: MaterialExpressionId,
        replacement: MaterialExpression,
    },
    Rewire {
        expression: MaterialExpressionId,
        input: MaterialExpressionInput,
        source: MaterialExpressionId,
    },
    SetOutput {
        output: MaterialFunctionOutputId,
        source: MaterialExpressionId,
    },
    /// None disconnects an argument and restores its declared default. Required inputs
    /// still fail transaction validation unless repaired within the same transaction.
    SetArgument {
        expression: MaterialExpressionId,
        input: MaterialFunctionInputId,
        source: Option<MaterialExpressionId>,
    },
}

pub(crate) fn apply(
    function: &mut MaterialFunction,
    edit: &MaterialFunctionBodyCommand,
) -> Result<(), MaterialCommandError> {
    if function.custom_wesl.is_some() {
        return Err(MaterialCommandError::CustomWeslBodyReadOnly);
    }
    let index = |id| {
        function
            .expressions
            .iter()
            .position(|expression| expression.id == id)
            .ok_or_else(|| MaterialCommandError::NotFound {
                kind: "function expression",
                id: format!("{id}"),
            })
    };
    match edit {
        MaterialFunctionBodyCommand::Add { expression, index } => {
            if *index > function.expressions.len() {
                return Err(MaterialCommandError::IndexOutOfBounds {
                    collection: "function expressions",
                    index: *index,
                    len: function.expressions.len(),
                });
            }
            function.expressions.insert(*index, expression.clone());
        }
        MaterialFunctionBodyCommand::Remove { expression } => {
            let index = index(*expression)?;
            function.expressions.remove(index);
        }
        MaterialFunctionBodyCommand::Replace {
            expression,
            replacement,
        } => {
            if *expression != replacement.id {
                return Err(MaterialCommandError::IdentityChanged {
                    kind: "function expression",
                    expected: expression.to_string(),
                    actual: replacement.id.to_string(),
                });
            }
            let index = index(*expression)?;
            function.expressions[index] = replacement.clone();
        }
        MaterialFunctionBodyCommand::Rewire {
            expression,
            input,
            source,
        } => {
            let index = index(*expression)?;
            crate::material_authoring::rewire_expression(
                &mut function.expressions[index].kind,
                *input,
                *source,
            )?;
        }
        MaterialFunctionBodyCommand::SetOutput { output, source } => {
            let output = function
                .outputs
                .iter_mut()
                .find(|candidate| candidate.id == *output)
                .ok_or_else(|| MaterialCommandError::NotFound {
                    kind: "function output",
                    id: output.to_string(),
                })?;
            output.expression = *source;
        }
        MaterialFunctionBodyCommand::SetArgument {
            expression,
            input,
            source,
        } => {
            let index = index(*expression)?;
            let MaterialExpressionKind::FunctionCall { arguments, .. } =
                &mut function.expressions[index].kind
            else {
                return Err(MaterialCommandError::InvalidExpressionInput {
                    input: MaterialExpressionInput::FunctionArgument(*input),
                });
            };
            if let Some(source) = source {
                arguments.insert(*input, *source);
            } else {
                arguments.remove(input);
            }
        }
    }
    Ok(())
}
