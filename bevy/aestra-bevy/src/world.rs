//! The host's world for simulations to collide with (fluid F11, host bindings HB10), from Bevy.
//!
//! Level geometry reaches Aestra as a signed distance volume ([`SdfVolume`]) in world space, set on
//! the [`crate::AestraWorldSdf`] resource. Bake it offline and load the `.aestra-sdf` file, or bake it
//! here from the scene's meshes: [`sdf_from_meshes`] takes each mesh's triangles through its
//! transform. A static world is uploaded once; set the resource again when the world changes.

use aestra_runtime::SdfVolume;
use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// Bakes the triangle meshes `meshes`, each placed by its transform, into one world SDF of voxels of
/// `voxel_size`, reaching `padding` past the meshes' bounds. Meshes that are not indexed or not
/// triangle lists are read as such too (every three positions a triangle). Brute force — every voxel
/// against every triangle — so keep level geometry coarse or bake offline.
pub fn sdf_from_meshes(
    meshes: &[(&Mesh, GlobalTransform)],
    voxel_size: f32,
    padding: f32,
) -> Result<SdfVolume, String> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut triangles: Vec<[u32; 3]> = Vec::new();
    for (mesh, transform) in meshes {
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            return Err("an SDF bakes triangle-list meshes only".into());
        }
        let Some(VertexAttributeValues::Float32x3(local)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            return Err("a mesh without float positions cannot be baked".into());
        };
        let base = positions.len() as u32;
        let affine = transform.affine();
        positions.extend(
            local
                .iter()
                .map(|p| affine.transform_point3(Vec3::from_array(*p)).to_array()),
        );
        let indices: Vec<u32> = match mesh.indices() {
            Some(Indices::U16(indices)) => indices.iter().map(|&i| u32::from(i)).collect(),
            Some(Indices::U32(indices)) => indices.clone(),
            None => (0..local.len() as u32).collect(),
        };
        triangles.extend(
            indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(|t| [base + t[0], base + t[1], base + t[2]]),
        );
    }
    SdfVolume::bake_triangle_mesh(&positions, &triangles, voxel_size, padding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_placed_cuboid_bakes_into_a_world_sdf_where_it_stands() {
        let cuboid = Mesh::from(Cuboid::new(4.0, 2.0, 4.0));
        let placed = GlobalTransform::from_translation(Vec3::new(10.0, 1.0, 0.0));
        let sdf = sdf_from_meshes(&[(&cuboid, placed)], 0.5, 1.0).unwrap();
        assert!(
            sdf.sample([10.0, 1.0, 0.0]) < -0.5,
            "inside, where it was placed"
        );
        assert!(
            (sdf.sample([10.0, 3.0, 0.0]) - 1.0).abs() < 0.1,
            "a unit above its top"
        );
        assert!(sdf.sample([0.0, 1.0, 0.0]) > 5.0, "the origin is well away");
    }
}
