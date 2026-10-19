//! Original material connection gesture and ghost identity.
use crate::material_graph::sockets::MaterialGraphSocketKind;
use aestra_core::MaterialProgramId;
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Copy)]
pub(super) struct MaterialGraphGhostWire {
    pub(super) program: MaterialProgramId,
}

#[derive(Resource, Debug, Default)]
pub(super) enum MaterialGraphGesture {
    #[default]
    Idle,
    Connecting {
        program: MaterialProgramId,
        /// The viewport the drag started in, so the ghost wire and snapping project through that
        /// view — correct even when two views of one program are open at once.
        viewport: Entity,
        origin: MaterialGraphSocketKind,
        cursor: Vec2,
        snap: Option<MaterialGraphSocketKind>,
    },
}
