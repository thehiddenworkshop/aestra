//! Shared particle world upload and production lockstep domain/event coupling.
use super::{
    coupled_simulation::Coupling, stage_inputs::ExtractedStages, stage_runtimes::StageRuntimes,
    world_sdf::AestraWorldSdf,
};
use crate::execution::{DomainSpawnPipeline, FieldFollowPipeline};
use aestra_gpu::GpuWorldSdf;
use bevy::{
    prelude::*,
    render::{render_resource::Buffer, renderer::RenderDevice},
};

/// The host's world SDF on the device for stateful particles' `World` colliders (host bindings
/// HB10): uploaded again only on a new revision; a header saying "no world" until the host sets one.
#[derive(Resource)]
pub(crate) struct ParticleWorldBuffer {
    revision: Option<u64>,
    pub(crate) buffer: Buffer,
}

pub(super) fn prepare_particle_world(
    mut commands: Commands,
    device: Res<RenderDevice>,
    world: Option<Res<AestraWorldSdf>>,
    current: Option<Res<ParticleWorldBuffer>>,
) {
    let packed = world.as_deref().and_then(AestraWorldSdf::packed);
    let revision = packed.map(|packed| packed.revision);
    if current
        .as_ref()
        .is_some_and(|current| current.revision == revision)
    {
        return;
    }
    let absent = GpuWorldSdf::absent();
    let bytes = packed.unwrap_or(&absent).to_bytes();
    let buffer =
        device.create_buffer_with_data(&bevy::render::render_resource::BufferInitDescriptor {
            label: Some("aestra particle world"),
            contents: &bytes,
            usage: bevy::render::render_resource::BufferUsages::STORAGE,
        });
    commands.insert_resource(ParticleWorldBuffer { revision, buffer });
}

/// The Follow Field (fluid F2b), Spawn From Domain (fluid F10) and event gather (host bindings HB9b)
/// pipelines the stateful path uses for emitters advancing in lockstep.
#[derive(Resource)]
pub(crate) struct FieldFollow(
    FieldFollowPipeline,
    DomainSpawnPipeline,
    crate::execution::EventGatherPipeline,
);

pub(super) fn init_field_follow(mut commands: Commands, device: Res<RenderDevice>) {
    let device = device.wgpu_device();
    commands.insert_resource(FieldFollow(
        FieldFollowPipeline::new(device),
        DomainSpawnPipeline::new(device),
        crate::execution::EventGatherPipeline::new(device),
    ));
}

/// The lockstep coupling for an effect with particle event links but no coupled domains (host
/// bindings HB9b): its emitters advance tick by tick together, with no domain.
pub(super) fn link_coupling(follower: Option<&FieldFollow>) -> Option<Coupling<'_>> {
    let follower = follower?;
    Some(Coupling {
        domains: &mut [],
        inputs: crate::execution::StageInputs::default(),
        follower: &follower.0,
        spawner: &follower.1,
        gatherer: &follower.2,
    })
}

/// The coupling for an effect whose stateful emitters follow its domains (fluid F2b), when it has one
/// and its domains are prepared.
pub(super) fn coupling<'a>(
    runtimes: &'a mut StageRuntimes,
    entity: Entity,
    extracted: Option<&'a ExtractedStages>,
    follower: Option<&'a FieldFollow>,
) -> Option<Coupling<'a>> {
    let extracted = extracted.filter(|extracted| extracted.coupled)?;
    let runtime = runtimes.0.get_mut(&entity)?;
    let follower = follower?;
    Some(Coupling {
        domains: &mut runtime.timelines,
        inputs: extracted.inputs(),
        follower: &follower.0,
        spawner: &follower.1,
        gatherer: &follower.2,
    })
}
