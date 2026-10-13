//! Actual material palette and context-menu state, independent of panel rendering.
use aestra_authoring::MaterialConnectionTarget;
use aestra_core::{MaterialExpressionId, MaterialProgramId};
use bevy::prelude::*;

#[derive(Debug, Clone)]
pub(super) struct MaterialGraphPaletteOpen {
    pub(super) program: MaterialProgramId,
    pub(super) editing_target: crate::material_document::MaterialEditingTarget,
    /// The selection scope (editor view / effect panel) the palette was opened in, so a created
    /// node selects in that view only.
    pub(super) scope: MaterialSelectionScope,
    pub(super) menu_position: Vec2,
    pub(super) graph_position: Vec2,
    pub(super) graph_key: String,
    pub(super) connection: Option<MaterialGraphPaletteConnection>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum MaterialGraphPaletteConnection {
    FromOutput(MaterialExpressionId),
    FromInput(MaterialConnectionTarget),
}

#[derive(Debug, Clone)]
pub(super) struct MaterialGraphNodeMenuOpen {
    pub(super) program: MaterialProgramId,
    /// The selection scope the node menu was opened in, so its edits act on that view's selection.
    pub(super) scope: MaterialSelectionScope,
    pub(super) menu_position: Vec2,
}

#[derive(Debug, Clone)]
pub(super) struct MaterialGraphConnectionMenuOpen {
    pub(super) program: MaterialProgramId,
    pub(super) scope: MaterialSelectionScope,
    pub(super) menu_position: Vec2,
    pub(super) connections: Vec<MaterialGraphConnection>,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct MaterialGraphPaletteState {
    pub(super) open: Option<MaterialGraphPaletteOpen>,
    pub(super) node_menu: Option<MaterialGraphNodeMenuOpen>,
    pub(super) connection_menu: Option<MaterialGraphConnectionMenuOpen>,
    pub(super) query: String,
}

impl MaterialGraphPaletteState {
    pub(crate) fn is_open(&self) -> bool {
        self.open.is_some() || self.node_menu.is_some() || self.connection_menu.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MaterialGraphConnection {
    pub(super) program: MaterialProgramId,
    pub(super) source: MaterialExpressionId,
    pub(super) target: MaterialConnectionTarget,
}

/// The scope a graph selection belongs to: an editor view, or the effect tool panel (`None`). Two
/// views of one program select independently while sharing the document, so selection is keyed by
/// scope rather than being a singleton.
pub(crate) type MaterialSelectionScope = Option<crate::docking::EditorViewId>;

#[cfg(test)]
impl MaterialGraphPaletteState {
    pub(crate) fn blocking_states_for_test() -> Vec<Self> {
        let program = MaterialProgramId::new();
        let expression = MaterialExpressionId::new();
        vec![
            Self {
                open: Some(MaterialGraphPaletteOpen {
                    program,
                    editing_target: crate::material_document::MaterialEditingTarget::Program {
                        root: "project".into(),
                        id: program,
                    },
                    scope: None,
                    menu_position: Vec2::ONE,
                    graph_position: Vec2::ZERO,
                    graph_key: "material:test".into(),
                    connection: Some(MaterialGraphPaletteConnection::FromOutput(expression)),
                }),
                ..default()
            },
            Self {
                node_menu: Some(MaterialGraphNodeMenuOpen {
                    program,
                    scope: Some(crate::docking::EditorViewId(1)),
                    menu_position: Vec2::ONE,
                }),
                ..default()
            },
            Self {
                connection_menu: Some(MaterialGraphConnectionMenuOpen {
                    program,
                    scope: None,
                    menu_position: Vec2::ONE,
                    connections: vec![],
                }),
                ..default()
            },
        ]
    }
}
