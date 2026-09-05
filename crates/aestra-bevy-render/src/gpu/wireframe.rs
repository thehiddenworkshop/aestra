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
    pub positions: Vec<[f32; 3]>,
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
        draw.wireframe_geometry = geometry;
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
    for triangle in triangles.chunks_exact(3) {
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
    Some(WireframeGeometry {
        positions: positions.clone(),
        indices,
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
            assert_eq!(geometry.positions.len(), 4);
        }
    }

    #[test]
    fn nonindexed_triangle_generates_three_edges() {
        let mesh = triangle()
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0; 3], [1.0; 3], [2.0; 3]]);
        assert_eq!(mesh_edges(&mesh).unwrap().indices, [0, 1, 1, 2, 2, 0]);
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
