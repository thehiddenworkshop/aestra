//! Production function signature control identities; native handlers own semantic edits.
use aestra_core::{
    MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    material::MaterialValueType,
};
use bevy::prelude::*;

#[derive(Clone, Copy)]
pub(super) enum Port {
    Input(MaterialFunctionInputId),
    Output(MaterialFunctionOutputId),
}
#[derive(Component, Clone, Copy)]
pub(super) struct Field {
    pub(super) owner: MaterialFunctionId,
    pub(super) kind: FieldKind,
}
#[derive(Clone, Copy)]
pub(super) enum FieldKind {
    Name,
    PortName(Port),
    Default(MaterialFunctionInputId),
}
#[derive(Component, Clone, Copy)]
pub(super) struct Action {
    pub(super) owner: MaterialFunctionId,
    pub(super) kind: ActionKind,
}
#[derive(Clone, Copy)]
pub(super) enum ActionKind {
    AddInput,
    AddOutput,
    Remove(Port),
    Type(Port, MaterialValueType),
}
