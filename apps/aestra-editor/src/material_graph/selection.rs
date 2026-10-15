//! Actual per-view material/function selection; independent of graph-panel rendering.
use super::palette_state::{MaterialGraphConnection, MaterialSelectionScope};
use crate::feathers::node_graph::{GraphSelectionMode, geometry::GraphNodeKey};
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialProgramId};
use bevy::prelude::*;
use std::collections::BTreeSet;

/// One scope's selection: which program it targets, the selected expression nodes, and the selected
/// connection.
#[derive(Debug, Default, Clone)]
pub(crate) struct ScopeSelection {
    pub(super) program: Option<MaterialProgramId>,
    pub(super) expressions: BTreeSet<MaterialExpressionId>,
    pub(super) connection: Option<MaterialGraphConnection>,
}

#[derive(Debug, Default, Clone)]
struct FunctionScopeSelection {
    function: Option<MaterialFunctionId>,
    pub(super) expressions: BTreeSet<MaterialExpressionId>,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphSelectionState {
    pub(super) scopes: std::collections::HashMap<MaterialSelectionScope, ScopeSelection>,
    function_scopes: std::collections::HashMap<MaterialSelectionScope, FunctionScopeSelection>,
}

impl MaterialGraphSelectionState {
    /// A scope's selection, if it has one.
    pub(super) fn get(&self, scope: MaterialSelectionScope) -> Option<&ScopeSelection> {
        self.scopes.get(&scope)
    }

    /// A scope's selection, creating an empty one if absent (for mutation).
    pub(super) fn entry(&mut self, scope: MaterialSelectionScope) -> &mut ScopeSelection {
        self.scopes.entry(scope).or_default()
    }

    /// The program a scope's selection targets.
    pub(super) fn program(&self, scope: MaterialSelectionScope) -> Option<MaterialProgramId> {
        self.get(scope).and_then(|selection| selection.program)
    }

    /// Whether a specific expression node is selected in a scope.
    pub(super) fn is_expression_selected(
        &self,
        scope: MaterialSelectionScope,
        program: MaterialProgramId,
        expression: MaterialExpressionId,
    ) -> bool {
        self.get(scope).is_some_and(|selection| {
            selection.program == Some(program) && selection.expressions.contains(&expression)
        })
    }

    /// The connection selected in a scope.
    pub(super) fn selected_connection(
        &self,
        scope: MaterialSelectionScope,
    ) -> Option<MaterialGraphConnection> {
        self.get(scope).and_then(|selection| selection.connection)
    }

    pub(super) fn select_expression(
        &mut self,
        scope: MaterialSelectionScope,
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        control: bool,
        shift: bool,
    ) -> Option<MaterialExpressionId> {
        let selection = self.entry(scope);
        if selection.program != Some(program) {
            selection.program = Some(program);
            selection.expressions.clear();
            selection.connection = None;
        }
        if control {
            if !selection.expressions.insert(expression) {
                selection.expressions.remove(&expression);
            }
        } else if shift {
            selection.expressions.insert(expression);
        } else {
            selection.expressions.clear();
            selection.expressions.insert(expression);
        }
        selection.connection = None;
        selection
            .expressions
            .contains(&expression)
            .then_some(expression)
            .or_else(|| selection.expressions.iter().next_back().copied())
    }

    /// Replaces a scope's selection with a single expression node (menu / palette selection).
    pub(super) fn select_single(
        &mut self,
        scope: MaterialSelectionScope,
        program: MaterialProgramId,
        expression: MaterialExpressionId,
    ) {
        let selection = self.entry(scope);
        selection.program = Some(program);
        selection.expressions.clear();
        selection.expressions.insert(expression);
        selection.connection = None;
    }

    pub(crate) fn is_function_expression_selected(
        &self,
        scope: MaterialSelectionScope,
        function: MaterialFunctionId,
        expression: MaterialExpressionId,
    ) -> bool {
        self.function_scopes.get(&scope).is_some_and(|selection| {
            selection.function == Some(function) && selection.expressions.contains(&expression)
        })
    }

    pub(crate) fn select_function_expression(
        &mut self,
        scope: MaterialSelectionScope,
        function: MaterialFunctionId,
        expression: MaterialExpressionId,
        control: bool,
        shift: bool,
    ) {
        let selection = self.function_scopes.entry(scope).or_default();
        if selection.function != Some(function) {
            selection.function = Some(function);
            selection.expressions.clear();
        }
        if control {
            if !selection.expressions.insert(expression) {
                selection.expressions.remove(&expression);
            }
        } else if shift {
            selection.expressions.insert(expression);
        } else {
            selection.expressions.clear();
            selection.expressions.insert(expression);
        }
    }

    pub(crate) fn clear_function_selection(
        &mut self,
        scope: MaterialSelectionScope,
        function: MaterialFunctionId,
    ) -> bool {
        let Some(selection) = self.function_scopes.get_mut(&scope) else {
            return false;
        };
        if selection.function != Some(function) || selection.expressions.is_empty() {
            return false;
        }
        selection.expressions.clear();
        true
    }

    pub(crate) fn function_arrange_seeds(
        &self,
        scope: MaterialSelectionScope,
        function: MaterialFunctionId,
    ) -> BTreeSet<GraphNodeKey> {
        self.function_scopes
            .get(&scope)
            .filter(|selection| selection.function == Some(function))
            .map(|selection| {
                selection
                    .expressions
                    .iter()
                    .copied()
                    .map(GraphNodeKey::Expression)
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn select_material_expressions(
        &mut self,
        scope: MaterialSelectionScope,
        program: MaterialProgramId,
        expressions: &BTreeSet<MaterialExpressionId>,
        mode: GraphSelectionMode,
    ) -> bool {
        let selection = self.scopes.entry(scope).or_default();
        if selection.program != Some(program) {
            selection.program = Some(program);
            selection.expressions.clear();
            selection.connection = None;
        }
        let before = selection.expressions.clone();
        match mode {
            GraphSelectionMode::Replace => selection.expressions.clone_from(expressions),
            GraphSelectionMode::Add => selection.expressions.extend(expressions),
            GraphSelectionMode::Toggle => {
                for expression in expressions {
                    if !selection.expressions.insert(*expression) {
                        selection.expressions.remove(expression);
                    }
                }
            }
        }
        selection.connection = None;
        selection.expressions != before
    }

    pub(crate) fn select_function_expressions(
        &mut self,
        scope: MaterialSelectionScope,
        function: MaterialFunctionId,
        expressions: &BTreeSet<MaterialExpressionId>,
        mode: GraphSelectionMode,
    ) -> bool {
        let selection = self.function_scopes.entry(scope).or_default();
        if selection.function != Some(function) {
            selection.function = Some(function);
            selection.expressions.clear();
        }
        let before = selection.expressions.clone();
        match mode {
            GraphSelectionMode::Replace => selection.expressions.clone_from(expressions),
            GraphSelectionMode::Add => selection.expressions.extend(expressions),
            GraphSelectionMode::Toggle => {
                for expression in expressions {
                    if !selection.expressions.insert(*expression) {
                        selection.expressions.remove(expression);
                    }
                }
            }
        }
        selection.expressions != before
    }
}
