//! Actual function graph context-menu state, independent of canvas rendering.
use aestra_authoring::MaterialExpressionInput;
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialFunctionOutputId};
use bevy::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    Input(MaterialExpressionId, MaterialExpressionInput),
    Output(MaterialFunctionOutputId),
}

#[derive(Debug, Clone)]
pub(super) enum FunctionGraphMenuKind {
    Node(MaterialExpressionId),
    Connections(Vec<(MaterialExpressionId, Target)>),
}

#[derive(Debug, Clone)]
pub(super) struct FunctionGraphMenuOpen {
    pub(super) owner: MaterialFunctionId,
    pub(super) scope: crate::material_graph::MaterialSelectionScope,
    pub(super) position: Vec2,
    pub(super) kind: FunctionGraphMenuKind,
}

#[derive(Resource, Default)]
pub(crate) struct FunctionGraphMenuState {
    pub(super) open: Option<FunctionGraphMenuOpen>,
}

impl FunctionGraphMenuState {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

#[cfg(test)]
impl FunctionGraphMenuState {
    pub(crate) fn blocking_states_for_test() -> Vec<Self> {
        let expression = MaterialExpressionId::new();
        [
            FunctionGraphMenuKind::Node(expression),
            FunctionGraphMenuKind::Connections(vec![(
                expression,
                Target::Output(MaterialFunctionOutputId::new()),
            )]),
        ]
        .into_iter()
        .map(|kind| Self {
            open: Some(FunctionGraphMenuOpen {
                owner: MaterialFunctionId::new(),
                scope: None,
                position: Vec2::ONE,
                kind,
            }),
        })
        .collect()
    }
}
