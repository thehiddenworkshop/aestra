//! Actual render-pass submissions, including per-view repetition and culling.
//! Only indirect command headers and their simulation context are copied.
use super::*;
use bevy::render::{render_resource::Buffer, renderer::WgpuWrapper, sync_world::MainEntity};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

const MAX_OWNERS: usize = 256;
const MAX_DRAWS: usize = 2048;
const MAX_IN_FLIGHT: usize = 3;
const BUFFER_SIZE: u64 = (MAX_OWNERS * 16 + MAX_DRAWS * 8) as u64;

#[derive(Clone, Debug, Default)]
pub(super) struct Sample {
    pub owner: Option<Entity>,
    pub token: u32,
    pub time: f32,
    pub instances: u32,
    pub vertices: u64,
    pub primitives: u64,
    pub draws: u32,
}

#[derive(Clone, Copy)]
pub(super) enum Topology {
    Strip,
    Triangles,
    Lines,
}

impl Sample {
    fn add(&mut self, vertices: u32, instances: u32, topology: Topology) {
        self.draws = self.draws.saturating_add(1);
        self.instances = self.instances.saturating_add(instances);
        self.vertices = self
            .vertices
            .saturating_add(u64::from(vertices) * u64::from(instances));
        let primitives = match topology {
            Topology::Strip => vertices.saturating_sub(2),
            Topology::Triangles => vertices / 3,
            Topology::Lines => vertices / 2,
        };
        self.primitives = self
            .primitives
            .saturating_add(u64::from(primitives) * u64::from(instances));
    }
}

struct Draw {
    owner: Entity,
    command: Option<(Buffer, u64)>,
    direct: [u32; 2],
    topology: Topology,
}

#[derive(Default)]
struct Frame {
    draws: Vec<Draw>,
    overflow: bool,
}

#[derive(Resource, Default)]
pub(super) struct Submissions(Mutex<Frame>);

impl Submissions {
    pub(super) fn record(
        &self,
        owner: Entity,
        command: Option<(&Buffer, u64)>,
        direct: [u32; 2],
        topology: Topology,
    ) {
        let mut frame = self.0.lock().unwrap();
        if frame.draws.len() == MAX_DRAWS {
            frame.overflow = true;
            return;
        }
        frame.draws.push(Draw {
            owner,
            command: command.map(|(b, o)| (b.clone(), o)),
            direct,
            topology,
        });
    }
}

#[derive(Default)]
struct Mailbox {
    sequence: u64,
    pending: Option<Vec<Sample>>,
}

#[derive(Resource, Clone, Default)]
pub(super) struct GeometryMailbox(Arc<Mutex<Mailbox>>);

impl GeometryMailbox {
    fn publish(&self, sequence: u64, samples: Vec<Sample>) {
        let mut mailbox = self.0.lock().unwrap();
        if sequence > mailbox.sequence {
            mailbox.sequence = sequence;
            mailbox.pending = Some(samples);
        }
    }
}

pub(super) fn receive(
    mailbox: Res<GeometryMailbox>,
    mut players: Query<(&PresentedEffect, &mut GpuParticleStatistics)>,
) {
    let Some(samples) = mailbox.0.lock().unwrap().pending.take() else {
        return;
    };
    for (_, mut stats) in &mut players {
        stats.geometry = None;
    }
    for sample in samples {
        let Some(owner) = sample.owner else {
            continue;
        };
        let Ok((player, mut stats)) = players.get_mut(owner) else {
            continue;
        };
        if stats.context_token(&player.instance) == Some(sample.token)
            && sample.time.is_finite()
            && sample.time >= 0.0
            && sample.time <= player.instance.time()
        {
            stats.geometry = Some(sample);
        }
    }
}

#[derive(Clone)]
struct Slot {
    buffer: WgpuWrapper<wgpu::Buffer>,
    busy: Arc<AtomicBool>,
}
#[derive(Default)]
pub(super) struct Readbacks {
    slots: Vec<Slot>,
    sequence: u64,
}

pub(super) fn begin(submissions: Res<Submissions>) {
    *submissions.0.lock().unwrap() = Frame::default();
}

// Runs after all view render passes but before the render graph submits its encoder.
pub(super) fn finish(
    mut context: RenderContext,
    device: Res<RenderDevice>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    owners: Query<(&MainEntity, &GpuEffectBuffers)>,
    submissions: Res<Submissions>,
    mailbox: Res<GeometryMailbox>,
    mut readbacks: Local<Readbacks>,
) {
    let frame = std::mem::take(&mut *submissions.0.lock().unwrap());
    readbacks.sequence += 1;
    let sequence = readbacks.sequence;
    if frame.overflow {
        mailbox.publish(sequence, Vec::new());
        return;
    }
    let index = readbacks
        .slots
        .iter()
        .position(|s| !s.busy.load(Ordering::Acquire));
    let index = index.or_else(|| {
        if readbacks.slots.len() == MAX_IN_FLIGHT {
            return None;
        }
        readbacks.slots.push(Slot {
            buffer: WgpuWrapper::new(device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra submitted geometry readback"),
                size: BUFFER_SIZE,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })),
            busy: Arc::new(AtomicBool::new(false)),
        });
        Some(readbacks.slots.len() - 1)
    });
    let Some(index) = index else {
        return;
    };
    let slot = readbacks.slots[index].clone();
    let encoder = context.command_encoder();
    let mut samples = Vec::new();
    for (owner, effect) in owners.iter().take(MAX_OWNERS) {
        let Some(indirect) = buffers.get(&effect.indirect) else {
            continue;
        };
        let offset = indirect.buffer.size() - 16;
        encoder.copy_buffer_to_buffer(
            &indirect.buffer,
            offset,
            &slot.buffer,
            (samples.len() * 16) as u64,
            16,
        );
        samples.push(Sample {
            owner: Some(owner.id()),
            token: effect.statistics_token,
            ..default()
        });
    }
    let draw_offset = (samples.len() * 16) as u64;
    for (i, draw) in frame.draws.iter().enumerate() {
        if let Some((buffer, offset)) = &draw.command {
            encoder.copy_buffer_to_buffer(
                buffer,
                *offset,
                &slot.buffer,
                draw_offset + (i * 8) as u64,
                8,
            );
        }
    }
    let size = draw_offset + (frame.draws.len() * 8) as u64;
    if size == 0 {
        mailbox.publish(sequence, Vec::new());
        return;
    }
    slot.busy.store(true, Ordering::Release);
    let readback = slot.buffer.clone();
    let mailbox = mailbox.clone();
    encoder.map_buffer_on_submit(&slot.buffer, wgpu::MapMode::Read, 0..size, move |result| {
        let samples = if result.is_ok() {
            let bytes = readback.slice(0..size).get_mapped_range();
            let word =
                |offset: usize| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            for (i, sample) in samples.iter_mut().enumerate() {
                let offset = i * 16;
                sample.time = f32::from_bits(word(offset + 12));
                if word(offset) != aestra_gpu::PARTICLE_STATISTICS_MAGIC
                    || word(offset + 4) != sample.token
                {
                    sample.owner = None;
                }
            }
            for (i, draw) in frame.draws.iter().enumerate() {
                let Some(sample) = samples.iter_mut().find(|s| s.owner == Some(draw.owner)) else {
                    continue;
                };
                let [vertices, instances] = if draw.command.is_some() {
                    let offset = draw_offset as usize + i * 8;
                    [word(offset), word(offset + 4)]
                } else {
                    draw.direct
                };
                sample.add(vertices, instances, draw.topology);
            }
            drop(bytes);
            readback.unmap();
            samples
        } else {
            Vec::new()
        };
        mailbox.publish(sequence, samples);
        slot.busy.store(false, Ordering::Release);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_runtime::{
        EffectInstance, EffectProfile, ProfileValue, ProjectInstanceProfile, ProjectProfile,
    };

    #[test]
    fn gpu_indirect_readback_is_bounded_recycles_and_preserves_command_formats() {
        use bevy::render::renderer::PendingCommandBuffers;
        let gpu = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(gpu.request_adapter(&Default::default())) else {
            eprintln!("Skipping geometry readback test: no GPU adapter");
            return;
        };
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let device = RenderDevice::new(WgpuWrapper::new(device));
        let words: [u32; 13] = [
            4,
            7,
            0,
            0,
            36,
            2,
            0,
            0,
            0,
            aestra_gpu::PARTICLE_STATISTICS_MAGIC,
            42,
            0,
            1.5_f32.to_bits(),
        ];
        let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("geometry regression commands"),
            contents: &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            usage: BufferUsages::COPY_SRC,
        });
        let handle = Handle::<ShaderBuffer>::default();
        let mut assets = RenderAssets::<GpuShaderBuffer>::default();
        assets.insert(
            handle.id(),
            GpuShaderBuffer {
                buffer: buffer.clone(),
                buffer_descriptor: wgpu::BufferDescriptor {
                    label: None,
                    size: 52,
                    usage: BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                },
                had_data: true,
            },
        );
        let mailbox = GeometryMailbox::default();
        let mut app = App::new();
        app.insert_resource(device.clone())
            .insert_resource(assets)
            .insert_resource(mailbox.clone())
            .init_resource::<Submissions>()
            .init_resource::<PendingCommandBuffers>()
            .add_systems(Update, finish);
        let owner = app.world_mut().spawn_empty().id();
        app.world_mut().entity_mut(owner).insert((
            MainEntity::from(owner),
            GpuEffectBuffers {
                emitters: default(),
                renderers: default(),
                particles: default(),
                alive: default(),
                dead: default(),
                counters: default(),
                indirect: handle,
                globals: default(),
                aux: default(),
                render_globals: default(),
                workgroups: 1,
                has_ribbons: false,
                has_trails: false,
                ribbon_workgroups: 1,
                total_slots: 8,
                simulation_time: 1.5,
                history_epoch: 0,
                statistics_token: 42,
                checkpoint_context: default(),
                trail_roots: vec![],
            },
        ));
        for _ in 0..MAX_IN_FLIGHT + 1 {
            let submissions = app.world().resource::<Submissions>();
            for _ in 0..2 {
                // Same sprite renderer submitted in two views.
                submissions.record(owner, Some((&buffer, 0)), [0; 2], Topology::Strip);
            }
            submissions.record(owner, Some((&buffer, 16)), [0; 2], Topology::Triangles);
            submissions.record(owner, None, [4, 6], Topology::Strip);
            app.update();
        }
        let commands = app
            .world_mut()
            .resource_mut::<PendingCommandBuffers>()
            .take();
        assert_eq!(
            commands.len(),
            MAX_IN_FLIGHT,
            "backpressure must skip a fourth batch"
        );
        let submit = |commands| {
            let submission = queue.submit(commands);
            // Only the test blocks; production telemetry never polls or waits.
            device
                .poll(wgpu::PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(std::time::Duration::from_secs(60)),
                })
                .unwrap();
        };
        submit(commands);
        let samples = mailbox.0.lock().unwrap().pending.take().unwrap();
        assert_eq!(samples.len(), 1);
        let s = &samples[0];
        assert_eq!((s.owner, s.token, s.time), (Some(owner), 42, 1.5));
        assert_eq!(
            (s.instances, s.vertices, s.primitives, s.draws),
            (22, 152, 64, 4)
        );
        // No draw calls in this view/frame: recycle a slot and report an observed zero.
        app.update();
        let commands = app
            .world_mut()
            .resource_mut::<PendingCommandBuffers>()
            .take();
        assert_eq!(commands.len(), 1);
        submit(commands);
        let samples = mailbox.0.lock().unwrap().pending.take().unwrap();
        assert_eq!(samples[0].owner, Some(owner));
        assert_eq!(
            (samples[0].instances, samples[0].vertices, samples[0].draws),
            (0, 0, 0)
        );
        // A rebuilt context cannot relabel an old GPU trailer as current.
        app.world_mut()
            .get_mut::<GpuEffectBuffers>(owner)
            .unwrap()
            .statistics_token = 43;
        app.update();
        submit(
            app.world_mut()
                .resource_mut::<PendingCommandBuffers>()
                .take(),
        );
        assert_eq!(
            mailbox.0.lock().unwrap().pending.take().unwrap()[0].owner,
            None
        );
    }

    #[test]
    fn counts_actual_commands_not_alive_particles_and_preserves_view_multiplicity() {
        let mut sample = Sample::default();
        // Two views each submit a sprite and ribbon renderer of the same emitter.
        for _ in 0..2 {
            sample.add(4, 7, Topology::Strip);
            sample.add(4, 7, Topology::Strip);
        }
        assert_eq!(
            (
                sample.instances,
                sample.vertices,
                sample.primitives,
                sample.draws
            ),
            (28, 112, 56, 4)
        );
        // Culled draws never call add; a zero-instance indirect call still is a submission.
        sample.add(4, 0, Topology::Strip);
        assert_eq!(
            (
                sample.instances,
                sample.vertices,
                sample.primitives,
                sample.draws
            ),
            (28, 112, 56, 5)
        );
        sample.add(36, 2, Topology::Triangles); // Indexed or non-indexed mesh.
        sample.add(24, 3, Topology::Lines); // Mesh wireframe.
        assert_eq!(
            (sample.instances, sample.vertices, sample.primitives),
            (33, 256, 116)
        );
        sample.add(1, 8, Topology::Strip); // No complete primitive.
        assert_eq!(sample.primitives, 116);
        sample.vertices = u64::MAX - 1;
        sample.instances = u32::MAX;
        sample.add(u32::MAX, u32::MAX, Topology::Triangles);
        assert_eq!(sample.vertices, u64::MAX);
        assert_eq!(sample.instances, u32::MAX);
    }

    #[test]
    fn bounded_frame_and_mailbox_never_publish_partial_or_old_totals() {
        let submissions = Submissions::default();
        for _ in 0..MAX_DRAWS + 2 {
            submissions.record(Entity::PLACEHOLDER, None, [4, 1], Topology::Strip);
        }
        let frame = submissions.0.lock().unwrap();
        assert!(frame.overflow);
        assert_eq!(frame.draws.len(), MAX_DRAWS);
        let mailbox = GeometryMailbox::default();
        mailbox.publish(2, vec![Sample::default()]);
        assert_eq!(mailbox.0.lock().unwrap().pending.take().unwrap().len(), 1);
        mailbox.publish(1, vec![Sample::default()]);
        assert!(mailbox.0.lock().unwrap().pending.is_none());
        mailbox.publish(3, Vec::new());
        assert!(mailbox.0.lock().unwrap().pending.take().unwrap().is_empty());
    }

    #[test]
    fn nested_owners_stale_contexts_and_hidden_zero_are_distinct_from_unavailable() {
        let mut asset = aestra_core::EffectAsset::new("Geometry", 3.0);
        asset
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Particles", 3.0));
        let effect = Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&asset)
                .unwrap(),
        );
        let mut app = App::new();
        let mailbox = GeometryMailbox::default();
        app.insert_resource(mailbox.clone())
            .add_systems(Update, receive);
        let mut entries = Vec::new();
        let mut samples = Vec::new();
        for count in [3, 5, 0] {
            let mut instance = EffectInstance::new(effect.clone());
            instance.set_playback_time(2.0);
            let stats = GpuParticleStatistics::new(&instance);
            let token = stats.context_token(&instance).unwrap();
            let mut presented = PresentedEffect::new(effect.clone());
            presented.instance = instance;
            let owner = app.world_mut().spawn((presented, stats)).id();
            let mut sample = Sample {
                owner: Some(owner),
                token,
                time: 1.5,
                ..default()
            };
            if count != 0 {
                sample.add(4, count, Topology::Strip);
            }
            samples.push(sample);
            entries.push(owner);
        }
        mailbox.publish(1, samples.clone());
        app.update();
        let profile = |app: &App, owner| {
            let p = app.world().get::<PresentedEffect>(owner).unwrap();
            let stats = app.world().get::<GpuParticleStatistics>(owner).unwrap();
            let mut result = EffectProfile::from_compiled(&effect);
            stats.record_geometry_profile(&p.instance, &mut result);
            result
        };
        assert_eq!(
            profile(&app, entries[2]).submitted_instances,
            ProfileValue::Measured(0)
        );
        let mut project = ProjectProfile::default();
        project.update(
            entries
                .iter()
                .take(2)
                .map(|&owner| ProjectInstanceProfile {
                    path: vec![aestra_core::EffectClipId::new()],
                    effect: effect.source,
                    name: "Repeated nested source".into(),
                    profile: profile(&app, owner),
                })
                .collect(),
        );
        assert_eq!(project.total.submitted_instances, ProfileValue::Measured(8));
        assert_eq!(project.total.submitted_vertices, ProfileValue::Measured(32));
        assert_eq!(
            project.total.submitted_primitives,
            ProfileValue::Measured(16)
        );
        app.world_mut()
            .get_mut::<PresentedEffect>(entries[0])
            .unwrap()
            .instance
            .seek(2.0);
        assert_eq!(
            profile(&app, entries[0]).submitted_instances,
            ProfileValue::Unavailable
        );
        mailbox.publish(2, samples);
        app.update();
        assert_eq!(
            profile(&app, entries[0]).submitted_instances,
            ProfileValue::Unavailable
        );
        assert_eq!(
            profile(&app, entries[1]).submitted_instances,
            ProfileValue::Measured(5)
        );
        app.world_mut().entity_mut(entries[0]).despawn();
        mailbox.publish(3, Vec::new()); // Failed readback invalidates the active set.
        app.update();
        assert_eq!(
            profile(&app, entries[1]).submitted_vertices,
            ProfileValue::Unavailable
        );
    }
}
