//! Original material node bootstrap geometry used by composition and semantic placement.
use crate::feathers::node_graph::{NODE_PREVIEW_SIZE, PORT_ROW_HEIGHT};
use aestra_compiler::{MaterialGraphNode, MaterialGraphNodeKind};
use aestra_core::material::MaterialValueType;
pub(super) const MATERIAL_PREVIEW_LAYOUT_HEIGHT: f32 = NODE_PREVIEW_SIZE + 8.0;

pub(super) fn material_graph_node_row_count(node: &MaterialGraphNode) -> usize {
    if node.kind != MaterialGraphNodeKind::Constant {
        return node.inputs.len();
    }
    match node.value_type {
        Some(MaterialValueType::Vec2) => 2,
        Some(MaterialValueType::Vec3) => 3,
        Some(MaterialValueType::Vec4 | MaterialValueType::Color) => 4,
        Some(MaterialValueType::Float | MaterialValueType::Bool)
        | Some(MaterialValueType::Texture2D(_))
        | None => 1,
    }
}

pub(super) fn node_height(input_count: usize, has_state: bool, has_preview: bool) -> f32 {
    40.0 + input_count.max(1) as f32 * PORT_ROW_HEIGHT
        + if has_state { 18.0 } else { 0.0 }
        + if has_preview {
            MATERIAL_PREVIEW_LAYOUT_HEIGHT
        } else {
            0.0
        }
}
