//! Non-mutating preflight for shared function edits, before committing a transaction.
use crate::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialTransaction,
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialProgramId, ValidationReport,
    material::{MaterialExpressionKind, MaterialFunction},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialFunctionCallOwner {
    Program(MaterialProgramId),
    Function(MaterialFunctionId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialFunctionCallSite {
    pub owner: MaterialFunctionCallOwner,
    pub expression: MaterialExpressionId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialFunctionEditPlan {
    pub transaction: MaterialTransaction,
    /// Direct callers in the supplied document, not a claim of a complete project inventory.
    pub call_sites: Vec<MaterialFunctionCallSite>,
    /// Candidate diagnostics include transitive callers and cycles. No links are repaired or removed.
    pub diagnostics: ValidationReport,
}

impl MaterialAuthoringDocument {
    /// Preview a function signature/body replacement without modifying drafts or history.
    /// Include all known dependent programs/functions to obtain their diagnostics.
    pub fn plan_function_edit(
        &self,
        id: MaterialFunctionId,
        function: MaterialFunction,
    ) -> Result<MaterialFunctionEditPlan, MaterialCommandError> {
        let transaction = MaterialTransaction::single(
            "Edit material function",
            MaterialCommand::ReplaceMaterialFunction { id, function },
        );
        let mut candidate = self.clone();
        // Reuse identity/existence checks, but retain invalid candidate diagnostics for the UI.
        crate::material_authoring::apply_command(&mut candidate, &transaction.commands[0])?;
        let mut call_sites = Vec::new();
        for (owner, expressions) in self
            .programs
            .iter()
            .map(|program| {
                (
                    MaterialFunctionCallOwner::Program(program.id),
                    &program.expressions,
                )
            })
            .chain(self.material_functions.iter().map(|function| {
                (
                    MaterialFunctionCallOwner::Function(function.id),
                    &function.expressions,
                )
            }))
        {
            for expression in expressions {
                if matches!(&expression.kind, MaterialExpressionKind::FunctionCall { function, .. } if function.id() == id)
                {
                    call_sites.push(MaterialFunctionCallSite {
                        owner,
                        expression: expression.id,
                    });
                }
            }
        }
        Ok(MaterialFunctionEditPlan {
            transaction,
            call_sites,
            diagnostics: candidate.validation_report(),
        })
    }
}
