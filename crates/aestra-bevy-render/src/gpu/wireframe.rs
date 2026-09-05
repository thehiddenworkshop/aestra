//! CPU topology conversion only; particle animation and instance counts stay on the GPU.
use super::{GpuDrawInstance, GpuRenderMode};
use bevy::{
    mesh::{PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Debug)]
pub(super) struct WireframeGeometry {
    pub vertices: Vec<[f32; 14]>,
    pub inputs: super::mesh_inputs::MeshInputs,
    pub deformation_ready: bool,
    pub indices: Vec<u32>,
}

type GeometryCache = HashMap<AssetId<Mesh>, Option<Arc<WireframeGeometry>>>;

pub(super) fn prepare_wireframe_geometry(
    meshes: Res<Assets<Mesh>>,
    mut cache: Local<GeometryCache>,
    mut draws: Query<&mut GpuDrawInstance>,
) {
    if meshes.is_changed() {
        cache.clear();
    }
    for mut draw in &mut draws {
        let geometry = draw
            .mesh
            .as_ref()
            .filter(|_| draw.render_mode == GpuRenderMode::Wireframe)
            .and_then(|handle| {
                cache
                    .entry(handle.id())
                    .or_insert_with(|| meshes.get(handle).and_then(mesh_edges).map(Arc::new))
                    .clone()
            });
        let deformed = draw
            .semantic_material
            .as_ref()
            .is_some_and(|material| material.program.has_vertex_offset);
        let required = draw
            .semantic_material
            .as_ref()
            .filter(|material| material.program.has_vertex_offset)
            .map(|material| super::mesh_inputs::MeshInputs::for_program(&material.program))
            .unwrap_or_default();
        draw.wireframe_geometry = geometry.filter(|geometry| {
            let valid = (!deformed || geometry.deformation_ready) && (!required.uv1 || geometry.inputs.uv1) && (!required.tangent || geometry.inputs.tangent);
            if !valid { bevy::log::warn_once!("Aestra deformed mesh wireframe requires valid Normal/UV0 and material-required attributes (UV1: {}, Tangent: {})", required.uv1, required.tangent); }
            valid
        });
    }
}

fn mesh_edges(mesh: &Mesh) -> Option<WireframeGeometry> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let VertexAttributeValues::Float32x3(positions) = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?
    else {
        return None;
    };
    if positions.is_empty() || positions.iter().flatten().any(|v| !v.is_finite()) {
        return None;
    }
    let triangles = match mesh.indices() {
        Some(indices) => indices
            .iter()
            .map(u32::try_from)
            .collect::<Result<Vec<_>, _>>()
            .ok()?,
        None => (0..u32::try_from(positions.len()).ok()?).collect(),
    };
    if triangles.is_empty()
        || !triangles.len().is_multiple_of(3)
        || triangles.iter().any(|&i| i as usize >= positions.len())
    {
        return None;
    }
    let mut seen = HashSet::new();
    let mut indices = Vec::new();
    for triangle in triangles.as_chunks::<3>().0 {
        for (a, b) in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            if a != b && seen.insert((a.min(b), a.max(b))) {
                indices.extend([a, b]);
            }
        }
    }
    if indices.is_empty() || u32::try_from(indices.len()).is_err() {
        return None;
    }
    let normals = mesh
        .attribute(Mesh::ATTRIBUTE_NORMAL)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x3(values) => Some(values),
            _ => None,
        });
    let uvs = mesh
        .attribute(Mesh::ATTRIBUTE_UV_0)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x2(values) => Some(values),
            _ => None,
        });
    let deformation_ready = normals.is_some_and(|values| values.len() == positions.len())
        && uvs.is_some_and(|values| values.len() == positions.len());
    let uv1s = mesh
        .attribute(Mesh::ATTRIBUTE_UV_1)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x2(values) if values.len() == positions.len() => {
                Some(values)
            }
            _ => None,
        });
    let tangents = mesh
        .attribute(Mesh::ATTRIBUTE_TANGENT)
        .and_then(|values| match values {
            VertexAttributeValues::Float32x4(values) if values.len() == positions.len() => {
                Some(values)
            }
            _ => None,
        });
    let inputs = super::mesh_inputs::MeshInputs {
        uv1: uv1s.is_some(),
        tangent: tangents.is_some(),
    };
    let vertices = positions
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let n = normals
                .and_then(|values| values.get(index))
                .copied()
                .unwrap_or([0.0, 0.0, 1.0]);
            let uv = uvs
                .and_then(|values| values.get(index))
                .copied()
                .unwrap_or([0.0; 2]);
            let uv1 = uv1s.map_or([0.0; 2], |values| values[index]);
            let tangent = tangents.map_or([1.0, 0.0, 0.0, 1.0], |values| values[index]);
            [
                p[0], p[1], p[2], n[0], n[1], n[2], uv[0], uv[1], uv1[0], uv1[1], tangent[0],
                tangent[1], tangent[2], tangent[3],
            ]
        })
        .collect::<Vec<_>>();
    if vertices.iter().flatten().any(|value| !value.is_finite()) {
        return None;
    }
    Some(WireframeGeometry {
        vertices,
        indices,
        inputs,
        deformation_ready,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{asset::RenderAssetUsages, mesh::Indices};

    fn triangle() -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(
            Mesh::ATTRIBUTE_POSITION,
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0; 3]],
        )
    }

    #[test]
    fn indexed_edges_preserve_triangle_diagonal_without_duplicate_shared_edges() {
        for indices in [
            Indices::U16(vec![0, 1, 2, 2, 1, 3]),
            Indices::U32(vec![0, 1, 2, 2, 1, 3]),
        ] {
            let geometry = mesh_edges(&triangle().with_inserted_indices(indices)).unwrap();
            assert_eq!(geometry.indices, [0, 1, 1, 2, 2, 0, 1, 3, 3, 2]);
            assert_eq!(geometry.vertices.len(), 4);
        }
    }

    #[test]
    fn nonindexed_triangle_generates_three_edges() {
        let mesh = triangle()
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3], [1.0; 3], [2.0; 3]]);
        assert_eq!(mesh_edges(&mesh).unwrap().indices, [0, 1, 1, 2, 2, 0]);
    }

    #[test]
    fn wireframe_preserves_secondary_uv_and_tangent_handedness() {
        let mesh = triangle()
            .with_inserted_indices(Indices::U16(vec![0, 1, 2]))
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_1, vec![[0.125, 0.625]; 4])
            .with_inserted_attribute(Mesh::ATTRIBUTE_TANGENT, vec![[0.0, 1.0, 0.0, -1.0]; 4]);
        let geometry = mesh_edges(&mesh).unwrap();
        assert!(geometry.inputs.uv1 && geometry.inputs.tangent);
        assert_eq!(
            &geometry.vertices[0][8..],
            &[0.125, 0.625, 0.0, 1.0, 0.0, -1.0]
        );
        let legacy =
            mesh_edges(&triangle().with_inserted_indices(Indices::U16(vec![0, 1, 2]))).unwrap();
        assert!(!legacy.inputs.uv1 && !legacy.inputs.tangent);
    }

    #[test]
    fn malformed_geometry_is_rejected() {
        assert!(mesh_edges(&triangle()).is_none());
        assert!(
            mesh_edges(&triangle().with_inserted_indices(Indices::U32(vec![0, 1, 99]))).is_none()
        );
        assert!(
            mesh_edges(&triangle().with_inserted_indices(Indices::U32(vec![0, 0, 0]))).is_none()
        );
        assert!(
            mesh_edges(
                &triangle()
                    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[f32::NAN; 3]; 3])
            )
            .is_none()
        );
    }

    #[test]
    fn mode_switches_and_asset_reload_refresh_shared_wireframe_geometry() {
        let mut app = App::new();
        app.init_resource::<Assets<Mesh>>()
            .add_systems(Update, prepare_wireframe_geometry);
        let handle = app
            .world_mut()
            .resource_mut::<Assets<Mesh>>()
            .reserve_handle();
        let draw = GpuDrawInstance {
            mesh: Some(handle.clone()),
            wireframe_geometry: None,
            renderers: default(),
            particles: default(),
            alive: default(),
            indirect: default(),
            render_globals: default(),
            render_params: default(),
            texture: default(),
            fallback_texture: default(),
            renderer_order: 0,
            emitter_index: 0,
            indirect_offset: 0,
            blend: super::super::GpuBlend::Alpha,
            material: aestra_core::MaterialId::new(),
            semantic_material: None,
            render_mode: GpuRenderMode::Wireframe,
            mesh_center: Vec3::ZERO,
        };
        let first = app.world_mut().spawn(draw.clone()).id();
        let second = app.world_mut().spawn(draw).id();
        app.update();
        assert!(
            app.world()
                .get::<GpuDrawInstance>(first)
                .unwrap()
                .wireframe_geometry
                .is_none()
        );
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .insert(
                handle.id(),
                triangle().with_inserted_indices(Indices::U16(vec![0, 1, 2])),
            )
            .unwrap();
        app.update();
        let original = app
            .world()
            .get::<GpuDrawInstance>(first)
            .unwrap()
            .wireframe_geometry
            .clone()
            .unwrap();
        assert!(Arc::ptr_eq(
            &original,
            app.world()
                .get::<GpuDrawInstance>(second)
                .unwrap()
                .wireframe_geometry
                .as_ref()
                .unwrap()
        ));
        app.update();
        assert!(Arc::ptr_eq(
            &original,
            app.world()
                .get::<GpuDrawInstance>(first)
                .unwrap()
                .wireframe_geometry
                .as_ref()
                .unwrap()
        ));
        app.world_mut()
            .get_mut::<GpuDrawInstance>(first)
            .unwrap()
            .render_mode = GpuRenderMode::Rendered;
        app.update();
        assert!(
            app.world()
                .get::<GpuDrawInstance>(first)
                .unwrap()
                .wireframe_geometry
                .is_none()
        );
        app.world_mut()
            .get_mut::<GpuDrawInstance>(first)
            .unwrap()
            .render_mode = GpuRenderMode::Wireframe;
        app.update();
        assert!(Arc::ptr_eq(
            &original,
            app.world()
                .get::<GpuDrawInstance>(first)
                .unwrap()
                .wireframe_geometry
                .as_ref()
                .unwrap()
        ));
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .get_mut(&handle)
            .unwrap()
            .insert_indices(Indices::U32(vec![0, 1, 2, 2, 1, 3]));
        app.update();
        let changed = app
            .world()
            .get::<GpuDrawInstance>(first)
            .unwrap()
            .wireframe_geometry
            .clone()
            .unwrap();
        assert!(!Arc::ptr_eq(&original, &changed));
        assert_eq!(changed.indices.len(), 10);
        app.world_mut()
            .resource_mut::<Assets<Mesh>>()
            .remove(handle.id());
        app.update();
        assert!(
            app.world()
                .get::<GpuDrawInstance>(first)
                .unwrap()
                .wireframe_geometry
                .is_none()
        );
    }
}
