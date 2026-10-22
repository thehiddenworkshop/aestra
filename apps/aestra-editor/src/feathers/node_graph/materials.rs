//! Production graph UI materials, shared by both Bevy qualification graphs.
use super::drag_assist::model::GRID_SPACING;
use bevy::{
    prelude::*, reflect::TypePath, render::render_resource::AsBindGroup, shader::ShaderRef,
    ui_render::prelude::UiMaterial,
};

/// Anti-aliased cubic wire drawn across a graph canvas.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(crate) struct GraphWireMaterial {
    #[uniform(0)]
    pub(crate) start: Vec2,
    #[uniform(0)]
    pub(crate) control_start: Vec2,
    #[uniform(0)]
    pub(crate) control_end: Vec2,
    #[uniform(0)]
    pub(crate) end: Vec2,
    #[uniform(0)]
    pub(crate) color: Vec4,
    #[uniform(0)]
    pub(crate) width: f32,
    #[uniform(0)]
    pub(crate) inverse_scale: f32,
}

impl Default for GraphWireMaterial {
    fn default() -> Self {
        Self {
            start: Vec2::ZERO,
            control_start: Vec2::ZERO,
            control_end: Vec2::ZERO,
            end: Vec2::ZERO,
            color: Vec4::new(0.61, 0.47, 1.0, 0.78),
            width: 2.0,
            inverse_scale: 1.0,
        }
    }
}

impl UiMaterial for GraphWireMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://aestra_editor/feathers/shaders/node_graph_wire.wesl".into()
    }
}

/// Viewport-space grid whose graph coordinates follow pan and zoom without finite edges.
#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub(super) struct GraphGridMaterial {
    #[uniform(0)]
    pub(super) pan: Vec2,
    #[uniform(0)]
    pub(super) zoom: f32,
    #[uniform(0)]
    pub(super) spacing: f32,
}

impl Default for GraphGridMaterial {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            spacing: GRID_SPACING,
        }
    }
}

impl UiMaterial for GraphGridMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://aestra_editor/feathers/shaders/node_graph_grid.wesl".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_materials_keep_real_asset_handles_and_independent_camera_uniforms() {
        let mut grids = Assets::<GraphGridMaterial>::default();
        let a = grids.add(GraphGridMaterial::default());
        let b = grids.add(GraphGridMaterial::default());
        {
            let mut grid = grids.get_mut(&a).unwrap();
            grid.pan = Vec2::new(30.0, -20.0);
            grid.zoom = 0.08;
            assert_eq!(grid.spacing, 32.0);
        }
        assert_eq!(grids.get(&b).unwrap().pan, Vec2::ZERO);
        assert_eq!(grids.get(&b).unwrap().zoom, 1.0);
        let mut wires = Assets::<GraphWireMaterial>::default();
        let handle = wires.add(GraphWireMaterial::default());
        let mut wire = wires.get_mut(&handle).unwrap();
        wire.start = Vec2::new(4.0, 8.0);
        wire.end = Vec2::new(300.0, 80.0);
        wire.inverse_scale = 2.0;
        assert_eq!(wire.width, 2.0);
        assert_eq!(wire.color, Vec4::new(0.61, 0.47, 1.0, 0.78));
        assert_eq!(wire.control_start, Vec2::ZERO);
        assert_eq!(wire.control_end, Vec2::ZERO);
    }

    #[test]
    fn material_shader_paths_retain_the_shipping_embedded_namespace() {
        for (shader, expected) in [
            (
                GraphWireMaterial::fragment_shader(),
                "embedded://aestra_editor/feathers/shaders/node_graph_wire.wesl",
            ),
            (
                GraphGridMaterial::fragment_shader(),
                "embedded://aestra_editor/feathers/shaders/node_graph_grid.wesl",
            ),
        ] {
            let ShaderRef::Path(path) = shader else {
                panic!("expected an embedded shader path")
            };
            assert_eq!(path.to_string(), expected);
        }
    }
}
