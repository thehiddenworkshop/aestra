//! Optional geometry requirements derived from the live semantic material.
use aestra_core::material::MaterialInput;
use aestra_gpu::material::CompiledMaterialProgram;
use bevy::mesh::{Mesh, MeshVertexBufferLayout, VertexAttributeDescriptor};
use bevy::render::render_resource::VertexFormat;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct MeshInputs {
    pub uv1: bool,
    pub tangent: bool,
}

impl MeshInputs {
    pub fn for_program(program: &CompiledMaterialProgram) -> Self {
        let inputs = &program.reflection.required_vertex_inputs;
        Self {
            uv1: inputs.contains(&MaterialInput::Uv1),
            tangent: inputs.contains(&MaterialInput::Tangent),
        }
    }

    pub fn validate(self, layout: &MeshVertexBufferLayout) -> Result<(), String> {
        let attributes = self.attributes();
        let layout = layout
            .get_layout(&attributes)
            .map_err(|error| error.to_string())?;
        for attribute in &layout.attributes {
            let expected = match attribute.shader_location {
                2 | 3 => VertexFormat::Float32x2,
                4 => VertexFormat::Float32x4,
                _ => VertexFormat::Float32x3,
            };
            if attribute.format != expected {
                return Err(format!(
                    "{} requires {expected:?}, got {:?}",
                    ["Position", "Normal", "UV0", "UV1", "Tangent"]
                        [attribute.shader_location as usize],
                    attribute.format
                ));
            }
        }
        Ok(())
    }

    pub fn attributes(self) -> Vec<VertexAttributeDescriptor> {
        let mut attributes = vec![
            Mesh::ATTRIBUTE_POSITION.at_shader_location(0),
            Mesh::ATTRIBUTE_NORMAL.at_shader_location(1),
            Mesh::ATTRIBUTE_UV_0.at_shader_location(2),
        ];
        if self.uv1 {
            attributes.push(Mesh::ATTRIBUTE_UV_1.at_shader_location(3));
        }
        if self.tangent {
            attributes.push(Mesh::ATTRIBUTE_TANGENT.at_shader_location(4));
        }
        attributes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        mesh::{MeshVertexBufferLayouts, PrimitiveTopology},
    };

    #[test]
    fn optional_attributes_are_required_only_when_used_and_formats_are_checked() {
        let mut layouts = MeshVertexBufferLayouts::default();
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 3])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0; 2]; 3]);
        let layout = mesh.get_mesh_vertex_buffer_layout(&mut layouts);
        assert!(MeshInputs::default().validate(&layout.0).is_ok());
        assert!(
            MeshInputs {
                uv1: true,
                tangent: false
            }
            .validate(&layout.0)
            .unwrap_err()
            .contains("Uv_1")
        );
        assert!(
            MeshInputs {
                uv1: false,
                tangent: true
            }
            .validate(&layout.0)
            .unwrap_err()
            .contains("Tangent")
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.0; 2]; 3]);
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[1.0, 0.0, 0.0, -1.0]; 3]);
        let layout = mesh.get_mesh_vertex_buffer_layout(&mut layouts);
        assert!(
            MeshInputs {
                uv1: true,
                tangent: true
            }
            .validate(&layout.0)
            .is_ok()
        );
        mesh.insert_attribute(
            bevy::mesh::MeshVertexAttribute::new("Wrong UV1", 3, VertexFormat::Float32x3),
            vec![[0.0; 3]; 3],
        );
        let layout = mesh.get_mesh_vertex_buffer_layout(&mut layouts);
        assert!(
            MeshInputs {
                uv1: true,
                tangent: false
            }
            .validate(&layout.0)
            .unwrap_err()
            .contains("UV1 requires Float32x2")
        );
    }
}
