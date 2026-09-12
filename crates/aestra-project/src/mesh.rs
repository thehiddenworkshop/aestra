//! Metadata only: buffer/image decoding remains the runtime loader's responsibility.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMeshPrimitive {
    pub mesh: usize,
    pub primitive: usize,
    pub vertices: usize,
}

impl ProjectMeshPrimitive {
    /// Bevy's Mesh loader label, not a scene or GltfMesh handle.
    pub fn loader_label(&self) -> String {
        format!("Mesh{}/Primitive{}", self.mesh, self.primitive)
    }
}

/// Enumerate triangle primitives without loading external buffers or images.
/// Call off the UI thread; the caller bounds the input size before reading it.
pub fn inspect_mesh_primitives(bytes: &[u8]) -> Result<Vec<ProjectMeshPrimitive>, String> {
    let gltf =
        gltf::Gltf::from_slice(bytes).map_err(|error| format!("Invalid glTF/GLB: {error}"))?;
    let mut result = Vec::new();
    for mesh in gltf.meshes() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let Some(positions) = primitive.get(&gltf::Semantic::Positions) else {
                continue;
            };
            if positions.count() < 3 {
                continue;
            }
            if result.len() == 256 {
                return Err(
                    "This mesh has more than 256 triangle primitives; split it before assigning"
                        .into(),
                );
            }
            result.push(ProjectMeshPrimitive {
                mesh: mesh.index(),
                primitive: primitive.index(),
                vertices: positions.count(),
            });
        }
    }
    if result.is_empty() {
        return Err("This file has no triangle mesh primitives with positions".into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gltf_primitive_labels_match_runtime_mesh_paths() {
        let result =
            inspect_mesh_primitives(include_bytes!("../../../assets/test/meshes/lab_cube.gltf"))
                .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].loader_label(), "Mesh0/Primitive0");
        assert!(result[0].vertices >= 3);
    }

    #[test]
    fn invalid_or_empty_mesh_documents_are_rejected() {
        for bytes in [b"not gltf".as_slice(), br#"{"asset":{"version":"2.0"}}"#] {
            assert!(inspect_mesh_primitives(bytes).is_err());
        }
    }

    #[test]
    fn multiple_meshes_and_glb_keep_original_primitive_indices() {
        let mut document: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../assets/test/meshes/lab_cube.gltf"))
                .unwrap();
        let triangle = document["meshes"][0]["primitives"][0].clone();
        let mut lines = triangle.clone();
        lines["mode"] = 1.into();
        document["meshes"][0]["primitives"] = serde_json::json!([lines, triangle]);
        let mesh = document["meshes"][0].clone();
        document["meshes"].as_array_mut().unwrap().push(mesh);
        let mut json = serde_json::to_vec(&document).unwrap();
        let expected = inspect_mesh_primitives(&json).unwrap();
        assert_eq!(
            expected
                .iter()
                .map(ProjectMeshPrimitive::loader_label)
                .collect::<Vec<_>>(),
            ["Mesh0/Primitive1", "Mesh1/Primitive1"]
        );
        // A valid GLB can keep external/data-URI buffers and contain only a JSON chunk.
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut glb = b"glTF".to_vec();
        glb.extend_from_slice(&2u32.to_le_bytes());
        glb.extend_from_slice(&(20u32 + json.len() as u32).to_le_bytes());
        glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
        glb.extend_from_slice(b"JSON");
        glb.extend_from_slice(&json);
        assert_eq!(inspect_mesh_primitives(&glb).unwrap(), expected);
    }
}
