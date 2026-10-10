//! Actual custom RenderCommand tuples and GPU indirect submissions.
use super::draw_instance::GpuDrawInstance;
use bevy::{
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::*},
    },
    mesh::MeshVertexBufferLayoutRef,
    pbr::SetMeshViewBindGroup,
    prelude::*,
    render::{
        render_asset::RenderAssets,
        render_phase::{
            PhaseItem, RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
        },
        render_resource::{BindGroup, Buffer, IndexFormat},
        storage::GpuShaderBuffer,
    },
    sprite_render::SetMesh2dViewBindGroup,
};

/// Renderer-local geometry command; only its instance count comes from simulation.
#[derive(Component)]
pub(super) struct PreparedMeshDraw {
    pub(super) indirect: Buffer,
    pub(super) vertex: Buffer,
    pub(super) index: Option<(Buffer, IndexFormat)>,
    pub(super) layout: Option<MeshVertexBufferLayoutRef>,
    pub(super) wireframe: Option<std::sync::Arc<super::draw_instance::WireframeGeometry>>,
}

#[derive(Component)]
pub(super) struct GpuRenderBindGroup(
    pub(super) BindGroup,
    pub(super) Option<BindGroup>,
    pub(super) std::collections::BTreeMap<Entity, BindGroup>,
);

#[derive(Component)]
pub(super) struct GpuMaterialBindGroup(pub(super) BindGroup);

#[derive(Component)]
pub(super) struct GpuSceneDepthBindGroup(pub(super) BindGroup);

pub(super) type DrawGpuSprites = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    DrawGpuSpritesIndirect,
);

pub(super) type DrawSemanticGpuSprites = (
    SetItemPipeline,
    SetMesh2dViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    SetGpuMaterialBindGroup<2>,
    DrawGpuSpritesIndirect,
);

pub(super) type DrawGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    DrawGpuSpritesIndirect,
);

pub(super) type DrawSemanticGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    SetGpuMaterialBindGroup<2>,
    DrawGpuSpritesIndirect,
);

pub(super) type DrawSemanticDepthGpuSprites3d = (
    SetItemPipeline,
    SetMeshViewBindGroup<0>,
    SetGpuRenderBindGroup<1>,
    SetGpuMaterialBindGroup<2>,
    SetGpuSceneDepthBindGroup<3>,
    DrawGpuSpritesIndirect,
);

pub(super) struct SetGpuRenderBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetGpuRenderBindGroup<I> {
    type Param = (
        SRes<super::draw_resources::TrailCompaction>,
        SRes<super::draw_resources::AlphaSort>,
    );
    type ViewQuery = Entity;
    type ItemQuery = Read<GpuRenderBindGroup>;

    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        bind_group: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        resources: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = bind_group else {
            return RenderCommandResult::Skip;
        };
        let (compaction, alpha_sort) = resources;
        let alpha_sort = alpha_sort.into_inner();
        if alpha_sort.prepared(view, item.entity()) {
            if !alpha_sort.dispatched {
                return RenderCommandResult::Skip;
            }
            let Some(group) = bind_group.2.get(&view) else {
                return RenderCommandResult::Skip;
            };
            pass.set_bind_group(I, group, &[]);
            return RenderCommandResult::Success;
        }
        let group = if compaction.into_inner().dispatched {
            bind_group.1.as_ref().unwrap_or(&bind_group.0)
        } else {
            &bind_group.0
        };
        pass.set_bind_group(I, group, &[]);
        RenderCommandResult::Success
    }
}

pub(super) struct SetGpuMaterialBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetGpuMaterialBindGroup<I> {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = Read<GpuMaterialBindGroup>;

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        bind_group: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, &bind_group.0, &[]);
        RenderCommandResult::Success
    }
}

pub(super) struct SetGpuSceneDepthBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetGpuSceneDepthBindGroup<I> {
    type Param = ();
    type ViewQuery = Read<GpuSceneDepthBindGroup>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        bind_group: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, &bind_group.0, &[]);
        RenderCommandResult::Success
    }
}

pub(super) struct DrawGpuSpritesIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawGpuSpritesIndirect {
    type Param = (
        SRes<RenderAssets<GpuShaderBuffer>>,
        SRes<super::draw_resources::TrailCulling>,
        SRes<super::draw_resources::Submissions>,
        SRes<super::draw_resources::TrailCompaction>,
    );
    type ViewQuery = Entity;
    type ItemQuery = (Read<GpuDrawInstance>, Option<Read<PreparedMeshDraw>>);

    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        effect: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        buffers: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some((effect, mesh)) = effect else {
            return RenderCommandResult::Skip;
        };
        let (buffers, culling, submissions, compaction) = buffers;
        let submissions = submissions.into_inner();
        use super::draw_resources::Topology;
        if let Some(mesh) = mesh {
            pass.set_vertex_buffer(0, mesh.vertex.slice(..));
            if let Some((index, format)) = &mesh.index {
                pass.set_index_buffer(index.slice(..), *format);
                pass.draw_indexed_indirect(&mesh.indirect, 0);
            } else {
                pass.draw_indirect(&mesh.indirect, 0);
            }
            submissions.record(
                effect.owner,
                Some((&mesh.indirect, 0)),
                [0; 2],
                if mesh.wireframe.is_some() {
                    Topology::Lines
                } else {
                    Topology::Triangles
                },
            );
            return RenderCommandResult::Success;
        }
        if effect.mesh.is_some() {
            return RenderCommandResult::Skip;
        }
        let Some(indirect) = buffers.into_inner().get(&effect.indirect) else {
            return RenderCommandResult::Skip;
        };
        if let Some(count) = effect.trail_instances {
            if let Some(indirect) = culling.into_inner().indirect(view, item.entity()) {
                pass.draw_indirect(indirect, 0);
                submissions.record(effect.owner, Some((indirect, 0)), [0; 2], Topology::Strip);
            } else if let Some(indirect) = compaction.into_inner().output(item.entity()) {
                pass.draw_indirect(indirect, 0);
                submissions.record(effect.owner, Some((indirect, 0)), [0; 2], Topology::Strip);
            } else {
                pass.draw(0..4, 0..count);
                submissions.record(effect.owner, None, [4, count], Topology::Strip);
            }
        } else {
            pass.draw_indirect(&indirect.buffer, effect.indirect_offset);
            submissions.record(
                effect.owner,
                Some((&indirect.buffer, effect.indirect_offset)),
                [0; 2],
                Topology::Strip,
            );
        }
        RenderCommandResult::Success
    }
}
