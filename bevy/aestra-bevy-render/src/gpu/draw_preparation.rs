//! Actual mesh allocation and effect/material/prepass bind-group preparation.
use super::{
    draw_commands::*,
    draw_instance::{GpuDrawInstance, GpuRenderMode},
    mesh_inputs::MeshInputs,
    pipeline::{GpuSpritePipeline, MaterialSceneUniforms},
};
use crate::material_layout::{bevy_sampler_descriptor, material_bind_group_layout};
use bevy::{
    core_pipeline::prepass::ViewPrepassTextures,
    prelude::*,
    render::{
        mesh::{RenderMesh, RenderMeshBufferInfo, allocator::MeshAllocator},
        render_asset::RenderAssets,
        render_resource::{
            BindGroupEntries, BindGroupEntry, BindingResource, BufferInitDescriptor, BufferUsages,
            IndexFormat, PipelineCache, PrimitiveTopology, Sampler, encase::UniformBuffer,
        },
        renderer::{RenderDevice, RenderQueue},
        storage::GpuShaderBuffer,
        texture::GpuImage,
        view::ExtractedView,
    },
};

pub(super) fn prepare_mesh_draws(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    meshes: Res<RenderAssets<RenderMesh>>,
    allocator: Res<MeshAllocator>,
    draws: Query<(Entity, &GpuDrawInstance, Option<&PreparedMeshDraw>)>,
) {
    for (entity, draw, previous) in &draws {
        let Some(handle) = &draw.mesh else {
            if previous.is_some() {
                commands.entity(entity).remove::<PreparedMeshDraw>();
            }
            continue;
        };
        if draw.render_mode == GpuRenderMode::Wireframe {
            let Some(geometry) = &draw.wireframe_geometry else {
                commands.entity(entity).remove::<PreparedMeshDraw>();
                continue;
            };
            if previous
                .and_then(|previous| previous.wireframe.as_ref())
                .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, geometry))
            {
                continue;
            }
            let positions = geometry
                .vertices
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            let indices = geometry
                .indices
                .iter()
                .flat_map(|i| i.to_le_bytes())
                .collect::<Vec<_>>();
            let words = [geometry.indices.len() as u32, 0, 0, 0, 0];
            let indirect = device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra mesh wireframe indirect"),
                contents: &words
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>(),
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            });
            commands.entity(entity).insert(PreparedMeshDraw {
                indirect,
                vertex: device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some("aestra mesh wireframe positions"),
                    contents: &positions,
                    usage: BufferUsages::VERTEX,
                }),
                index: Some((
                    device.create_buffer_with_data(&BufferInitDescriptor {
                        label: Some("aestra mesh wireframe edges"),
                        contents: &indices,
                        usage: BufferUsages::INDEX,
                    }),
                    IndexFormat::Uint32,
                )),
                layout: None,
                wireframe: Some(geometry.clone()),
            });
            continue;
        }
        let Some(mesh) = meshes.get(handle) else {
            commands.entity(entity).remove::<PreparedMeshDraw>();
            continue;
        };
        let inputs = draw
            .semantic_material
            .as_ref()
            .map_or_else(MeshInputs::default, |material| {
                MeshInputs::for_program(&material.program)
            });
        let validation = if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            Err("TriangleList topology is required".to_owned())
        } else {
            inputs.validate(&mesh.layout.0)
        };
        if let Err(error) = validation {
            bevy::log::warn_once!("Aestra mesh {:?}: {error}", handle.id());
            commands.entity(entity).remove::<PreparedMeshDraw>();
            continue;
        }
        let Some(vertices) = allocator.mesh_vertex_slice(&handle.id()) else {
            commands.entity(entity).remove::<PreparedMeshDraw>();
            continue;
        };
        let (words, index) = match mesh.buffer_info {
            RenderMeshBufferInfo::Indexed {
                count,
                index_format,
            } => {
                let Some(indices) = allocator.mesh_index_slice(&handle.id()) else {
                    commands.entity(entity).remove::<PreparedMeshDraw>();
                    continue;
                };
                (
                    [count, 0, indices.range.start, vertices.range.start, 0],
                    Some((indices.buffer.clone(), index_format)),
                )
            }
            RenderMeshBufferInfo::NonIndexed => (
                [vertices.range.len() as u32, 0, vertices.range.start, 0, 0],
                None,
            ),
        };
        let bytes = words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let indirect = if let Some(previous) = previous {
            queue.write_buffer(&previous.indirect, 0, &bytes);
            previous.indirect.clone()
        } else {
            device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra mesh indirect"),
                contents: &bytes,
                usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
            })
        };
        commands.entity(entity).insert(PreparedMeshDraw {
            indirect,
            vertex: vertices.buffer.clone(),
            index,
            layout: Some(mesh.layout.clone()),
            wireframe: None,
        });
    }
}

pub(super) fn prepare_scene_depth_bind_groups(
    mut commands: Commands,
    pipeline: Res<GpuSpritePipeline>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    views: Query<(Entity, &ExtractedView, &ViewPrepassTextures, &Msaa)>,
) {
    for (entity, view, prepass, msaa) in &views {
        let Some(depth_view) = super::scene_depth::depth_view(prepass) else {
            commands.entity(entity).remove::<GpuSceneDepthBindGroup>();
            continue;
        };
        let mut encoded = UniformBuffer::new(Vec::new());
        encoded
            .write(&MaterialSceneUniforms {
                view_from_clip: view.clip_from_view.inverse(),
                viewport: view.viewport.as_vec4(),
            })
            .expect("material scene uniforms have a fixed valid layout");
        let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra material scene uniforms"),
            contents: &encoded.into_inner(),
            usage: BufferUsages::UNIFORM,
        });
        let descriptor = if msaa.samples() > 1 {
            &pipeline.multisampled_scene_depth_layout
        } else {
            &pipeline.scene_depth_layout
        };
        let bind_group = render_device.create_bind_group(
            Some("aestra material scene depth"),
            &pipeline_cache.get_bind_group_layout(descriptor),
            &BindGroupEntries::sequential((buffer.as_entire_buffer_binding(), depth_view)),
        );
        commands
            .entity(entity)
            .insert(GpuSceneDepthBindGroup(bind_group));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_render_bind_groups(
    mut commands: Commands,
    pipeline: Res<GpuSpritePipeline>,
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    images: Res<RenderAssets<GpuImage>>,
    effects: Query<(Entity, &GpuDrawInstance)>,
    compaction: Res<super::draw_resources::TrailCompaction>,
    alpha_sort: Res<super::draw_resources::AlphaSort>,
    views: Query<Entity, With<Camera3d>>,
) {
    for (entity, effect) in &effects {
        let Some(renderers) = buffers.get(&effect.renderers) else {
            continue;
        };
        let Some(particles) = buffers.get(&effect.particles) else {
            continue;
        };
        let Some(alive) = buffers.get(&effect.alive) else {
            continue;
        };
        let Some(globals) = buffers.get(&effect.render_globals) else {
            continue;
        };
        let Some(params) = buffers.get(&effect.render_params) else {
            continue;
        };
        let Some(image) = images
            .get(&effect.texture)
            .or_else(|| images.get(&effect.fallback_texture))
        else {
            continue;
        };
        let Some(aux) = buffers.get(&effect.aux) else {
            continue;
        };
        let bind_group = render_device.create_bind_group(
            Some("aestra gpu sprite"),
            &pipeline_cache.get_bind_group_layout(&pipeline.effect_layout),
            &BindGroupEntries::sequential((
                renderers.buffer.as_entire_buffer_binding(),
                particles.buffer.as_entire_buffer_binding(),
                alive.buffer.as_entire_buffer_binding(),
                globals.buffer.as_entire_buffer_binding(),
                params.buffer.as_entire_buffer_binding(),
                &image.texture_view,
                &image.sampler,
                aux.buffer.as_entire_buffer_binding(),
            )),
        );
        let compact_group = compaction.entries.get(&entity).map(|entry| {
            render_device.create_bind_group(
                Some("aestra compact trail vertex"),
                &pipeline_cache.get_bind_group_layout(&pipeline.effect_layout),
                &BindGroupEntries::sequential((
                    renderers.buffer.as_entire_buffer_binding(),
                    particles.buffer.as_entire_buffer_binding(),
                    entry.output.as_entire_buffer_binding(),
                    globals.buffer.as_entire_buffer_binding(),
                    entry.render_params.as_entire_buffer_binding(),
                    &image.texture_view,
                    &image.sampler,
                    aux.buffer.as_entire_buffer_binding(),
                )),
            )
        });
        let sorted_groups = views
            .iter()
            .filter_map(|view| {
                let (indices, params) = alpha_sort.buffers(view, entity)?;
                Some((
                    view,
                    render_device.create_bind_group(
                        Some("aestra sorted alpha sprite vertex"),
                        &pipeline_cache.get_bind_group_layout(&pipeline.effect_layout),
                        &BindGroupEntries::sequential((
                            renderers.buffer.as_entire_buffer_binding(),
                            particles.buffer.as_entire_buffer_binding(),
                            indices.as_entire_buffer_binding(),
                            globals.buffer.as_entire_buffer_binding(),
                            params.as_entire_buffer_binding(),
                            &image.texture_view,
                            &image.sampler,
                            aux.buffer.as_entire_buffer_binding(),
                        )),
                    ),
                ))
            })
            .collect();
        commands.entity(entity).insert(GpuRenderBindGroup(
            bind_group,
            compact_group,
            sorted_groups,
        ));
        let Some(material) = &effect.semantic_material else {
            commands.entity(entity).remove::<GpuMaterialBindGroup>();
            continue;
        };
        let descriptor = material_bind_group_layout(&material.program.resource_layout);
        let uniform_buffer = (!material.uniforms.is_empty()).then(|| {
            render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("aestra semantic material uniforms"),
                contents: &material.uniforms,
                usage: BufferUsages::UNIFORM,
            })
        });
        let resolved_images = material
            .textures
            .iter()
            .map(|texture| {
                images
                    .get(texture)
                    .or_else(|| images.get(&material.fallback_texture))
            })
            .collect::<Option<Vec<_>>>();
        let Some(resolved_images) = resolved_images else {
            continue;
        };
        let samplers = material
            .program
            .resource_layout
            .samplers
            .iter()
            .map(|slot| render_device.create_sampler(&bevy_sampler_descriptor(slot.descriptor)))
            .collect::<Vec<Sampler>>();
        let mut entries = Vec::with_capacity(descriptor.entries.len());
        if let (Some(binding), Some(buffer)) = (
            material.program.resource_layout.uniforms.binding,
            uniform_buffer.as_ref(),
        ) {
            entries.push(BindGroupEntry {
                binding,
                resource: BindingResource::Buffer(buffer.as_entire_buffer_binding()),
            });
        }
        for (slot, image) in material
            .program
            .resource_layout
            .textures
            .iter()
            .zip(&resolved_images)
        {
            entries.push(BindGroupEntry {
                binding: slot.binding,
                resource: BindingResource::TextureView(&image.texture_view),
            });
        }
        for (slot, sampler) in material
            .program
            .resource_layout
            .samplers
            .iter()
            .zip(&samplers)
        {
            entries.push(BindGroupEntry {
                binding: slot.binding,
                resource: BindingResource::Sampler(sampler),
            });
        }
        entries.sort_by_key(|entry| entry.binding);
        let bind_group = render_device.create_bind_group(
            Some("aestra semantic material"),
            &pipeline_cache.get_bind_group_layout(&descriptor),
            &entries,
        );
        commands
            .entity(entity)
            .insert(GpuMaterialBindGroup(bind_group));
    }
}
