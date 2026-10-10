//! Actual draw extraction payload, shared by the shipping and migration adapters.
use super::mesh_inputs::MeshInputs;
use aestra_core::MaterialId;
use aestra_gpu::{GpuBlend, material::CompiledMaterialProgram};
use bevy::{
    camera::{
        primitives::Aabb,
        visibility::{self, VisibilityClass},
    },
    prelude::*,
    render::storage::ShaderBuffer,
};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(super) enum GpuRenderMode {
    #[default]
    Rendered,
    Wireframe,
}

#[derive(Debug)]
pub(super) struct WireframeGeometry {
    pub vertices: Vec<[f32; 14]>,
    pub inputs: MeshInputs,
    pub deformation_ready: bool,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct SpriteCullBounds {
    pub half_extents: Vec3,
    pub maximum_size: f32,
    pub world_from_effect: Mat4,
    pub minimum_pixels: f32,
}

#[derive(Component, Clone)]
#[require(Transform, Visibility, VisibilityClass)]
#[component(on_add = visibility::add_visibility_class::<GpuDrawInstance>)]
pub(super) struct GpuDrawInstance {
    pub(super) sort_range: UVec2,
    pub(super) renderer_kind: u32,
    pub(super) owner: Entity,
    pub(super) mesh: Option<Handle<Mesh>>,
    pub(super) wireframe_geometry: Option<Arc<WireframeGeometry>>,
    pub(super) renderers: Handle<ShaderBuffer>,
    pub(super) particles: Handle<ShaderBuffer>,
    pub(super) alive: Handle<ShaderBuffer>,
    pub(super) aux: Handle<ShaderBuffer>,
    pub(super) indirect: Handle<ShaderBuffer>,
    pub(super) render_globals: Handle<ShaderBuffer>,
    pub(super) render_params: Handle<ShaderBuffer>,
    pub(super) texture: Handle<Image>,
    pub(super) fallback_texture: Handle<Image>,
    pub(super) renderer_order: u32,
    pub(super) emitter_index: u32,
    pub(super) indirect_offset: u64,
    pub(super) trail_instances: Option<u32>,
    pub(super) trail_owners: u32,
    pub(super) blend: GpuBlend,
    pub(super) material: MaterialId,
    pub(super) semantic_material: Option<GpuSemanticMaterialBinding>,
    pub(super) render_mode: GpuRenderMode,
    pub(super) mesh_center: Vec3,
    pub(super) sampled_sprite_cull: Option<SpriteCullBounds>,
}

#[derive(Clone)]
pub(super) struct GpuSemanticMaterialBinding {
    pub(super) program: Arc<CompiledMaterialProgram>,
    pub(super) render_state: aestra_core::material::MaterialRenderState,
    pub(super) shader: Handle<Shader>,
    pub(super) multisampled_shader: Handle<Shader>,
    pub(super) uniforms: Arc<[u8]>,
    pub(super) textures: Vec<Handle<Image>>,
    pub(super) fallback_texture: Handle<Image>,
}

impl GpuDrawInstance {
    /// Explicit None is intentional: a visibility-filtered extraction would
    /// skip hidden entities and could retain the previous render-world draw.
    pub(super) fn extracted(
        &self,
        visibility: &ViewVisibility,
        transform: &GlobalTransform,
        bounds: &Aabb,
    ) -> Option<Self> {
        visibility.get().then(|| {
            let mut extracted = self.clone();
            extracted.mesh_center = gpu_draw_mesh_center(transform, bounds);
            if let Some(culling) = &mut extracted.sampled_sprite_cull {
                culling.world_from_effect = Mat4::from(transform.affine());
            }
            extracted
        })
    }
}

pub(super) fn gpu_draw_mesh_center(transform: &GlobalTransform, bounds: &Aabb) -> Vec3 {
    transform.transform_point(Vec3::from(bounds.center))
}
