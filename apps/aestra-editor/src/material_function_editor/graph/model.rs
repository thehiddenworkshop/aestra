//! Shared production function-canvas components; input consumers remain in the native host.
use crate::feathers::node_graph::GraphSocketSide;
use crate::material_function_editor::graph::menu_state::Target;
use aestra_core::{MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId};
use bevy::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SocketKind {
    Source(MaterialExpressionId),
    Target(Target),
}

impl SocketKind {
    pub(super) fn side(self) -> GraphSocketSide {
        match self {
            Self::Source(_) => GraphSocketSide::Output,
            Self::Target(_) => GraphSocketSide::Input,
        }
    }
}

#[derive(Component, Clone, Copy)]
pub(super) struct Socket {
    pub(super) owner: MaterialFunctionId,
    pub(super) kind: SocketKind,
    pub(super) node: Entity,
}
#[derive(Component, Default)]
pub(super) struct Anchor(pub(super) Option<Vec2>);
#[derive(Component)]
pub(super) struct PreviewWire(pub(super) MaterialFunctionId);
#[derive(Component)]
pub(super) struct Wire {
    pub(super) owner: MaterialFunctionId,
    pub(super) source: MaterialExpressionId,
    pub(super) target: Target,
}

#[derive(Component)]
pub(super) struct FunctionGraphContextMenu;

#[derive(Component, Clone, Copy)]
pub(super) enum FunctionGraphContextAction {
    Clipboard(crate::material_graph::clipboard::Shortcut),
    Arrange(crate::material_graph::arrange::ArrangeScope),
    Open(MaterialExpressionId),
    Duplicate(MaterialExpressionId),
    Delete(MaterialExpressionId),
    Disconnect(Target),
}
#[derive(Component, Clone, Copy)]
pub(super) struct BodyAction {
    pub(super) owner: MaterialFunctionId,
    pub(super) scope: Option<crate::docking::EditorViewId>,
    pub(super) kind: BodyActionKind,
}
#[derive(Clone, Copy)]
pub(super) enum BodyActionKind {
    Back,
    Locate,
    Arrange(crate::material_graph::arrange::ArrangeScope),
    Create(aestra_compiler::MaterialGraphCreateKind),
    Boolean(MaterialExpressionId, bool),
    Input(MaterialFunctionInputId),
    Remove(MaterialExpressionId),
}
#[derive(Component)]
pub(super) struct Constant {
    pub(super) owner: MaterialFunctionId,
    pub(super) expression: MaterialExpressionId,
    pub(super) component: usize,
}
