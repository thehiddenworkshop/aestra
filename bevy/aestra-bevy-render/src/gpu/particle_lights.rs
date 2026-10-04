//! Opt-in forward-presentation particle light selection. No CPU particle readback,
//! replay dependency, host event queue, light entity pool or synchronous GPU wait.
use super::*;
use aestra_core::{EffectClipId, EffectId, EmitterId, EmitterRegionId, SceneOutputId};
use aestra_gpu::particle_lights::*;
use bevy::render::{
    extract_resource::{ExtractResource, ExtractResourcePlugin},
    render_resource::{
        BufferDescriptor, ShaderType, binding_types::uniform_buffer, encase::StorageBuffer,
    },
    renderer::RenderQueue,
};

/// Global selected-light cap across every GPU presentation in this render app,
/// independent from each output's authored quality cap. Zero disables all light
/// jobs and releases their scratch. A separate host memory budget rejects an
/// over-budget frame explicitly; no hard-coded emitter/output count limit.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, ExtractResource)]
pub struct AestraParticleLightSettings {
    pub max_lights: u32,
    pub max_scratch_bytes: u64,
}

/// Render-graph scheduling point after selected GPU records are written and
/// before camera rendering. GPU consumers must run after this set, retaining
/// this frame's matching manifest; never use a buffer from an earlier frame.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ParticleLightSelectionSet;
impl Default for AestraParticleLightSettings {
    fn default() -> Self {
        Self {
            max_lights: 0,
            max_scratch_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Canonical occurrence identity. Tokens are frame-local manifest indices, not
/// persistent light identities. Consumers must retain the matching frame manifest
/// with any async selected-set copy and validate owner/root epochs before use.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParticleLightSource {
    pub root: Entity,
    pub root_epoch: u32,
    pub clip_path: Vec<EffectClipId>,
    pub owner: Entity,
    pub owner_epoch: u32,
    pub revision: u64,
    pub effect: EffectId,
    pub seed: u64,
    pub emitter: EmitterId,
    pub region: EmitterRegionId,
    pub output: SceneOutputId,
    /// Keep the originating compiled artifact alive through async consumption.
    /// An in-place replacement with the same authored IDs/seed/epoch is not the
    /// same presentation. Pointer reuse cannot occur while this handle is held.
    pub artifact: ParticleLightArtifact,
}

#[derive(Clone)]
pub struct ParticleLightArtifact(pub Arc<aestra_runtime::CompiledEffect>);
impl ParticleLightArtifact {
    pub fn matches(&self, effect: &Arc<aestra_runtime::CompiledEffect>) -> bool {
        Arc::ptr_eq(&self.0, effect)
    }
    fn address(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}
impl std::fmt::Debug for ParticleLightArtifact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ParticleLightArtifact")
            .field(&self.address())
            .finish()
    }
}
impl PartialEq for ParticleLightArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.matches(&other.0)
    }
}
impl Eq for ParticleLightArtifact {}
impl PartialOrd for ParticleLightArtifact {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for ParticleLightArtifact {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.address().cmp(&other.address())
    }
}

/// Render-world selected buffers only, suitable for F7E's bounded async copy.
/// `records` starts with a sorted valid prefix followed by zero-lumen padding;
/// counters determine the actual prefix length. Never read back source particles.
#[derive(Clone)]
pub struct SelectedGpuParticleLights {
    pub records: Buffer,
    pub counters: Buffer,
    pub manifest: Vec<ParticleLightSource>,
    pub selected_capacity: u32,
    /// Ping-pong selection storage only, excluding plans/keys/counters/uniforms.
    pub scratch_bytes: u64,
    /// All reserved selection buffers covered by the host memory budget.
    pub reserved_bytes: u64,
    pub sequence: u64,
    pub rejected_outputs: u32,
}

#[derive(Resource, Default)]
pub struct GpuSelectedParticleLights {
    frame: Option<SelectedGpuParticleLights>,
    pub rejection: Option<String>,
    entries: BTreeMap<ParticleLightSource, Entry>,
    global: Option<GlobalEntry>,
    sequence: u64,
    rejected_outputs: u32,
    reserved_bytes: u64,
}
impl GpuSelectedParticleLights {
    /// None during disable, empty/unprepared scenes, pipeline loading or failure.
    pub fn frame(&self) -> Option<&SelectedGpuParticleLights> {
        self.frame.as_ref()
    }
}

#[derive(Component, Clone)]
pub(super) struct Pools(pub Vec<(u32, u32)>);
#[derive(Clone)]
struct Input {
    source: ParticleLightSource,
    emitter_index: u32,
    offset: u32,
    count: u32,
    plan: aestra_runtime::ParticlePointLightPlan,
    parameters: Arc<[aestra_runtime::RuntimeValue]>,
}
#[derive(Component, Clone, ExtractComponent, Default)]
struct Inputs(Vec<Input>);
#[derive(Resource)]
struct Pipelines {
    local_layout: BindGroupLayoutDescriptor,
    global_layout: BindGroupLayoutDescriptor,
    local: [CachedComputePipelineId; 3],
    global: [CachedComputePipelineId; 3],
}

struct Entry {
    work: ParticleLightWorkPlan,
    scratch: [Buffer; 2],
    counters: Buffer,
    plan: Buffer,
    keys: Buffer,
    plan_bytes: Vec<u8>,
    key_bytes: Vec<u8>,
    particles: Buffer,
    globals: Buffer,
    groups: Vec<BindGroup>,
    uniforms: Vec<Buffer>,
}
impl Entry {
    fn selected(&self) -> &Buffer {
        &self.scratch[self.work.merges.len() % 2]
    }
}
struct GlobalEntry {
    work: GlobalLightWorkPlan,
    scratch: [Buffer; 2],
    input_counts: Buffer,
    counters: Buffer,
    groups: Vec<BindGroup>,
}

#[derive(Resource, Default)]
pub(super) struct PresentedOwners {
    pub enabled: bool,
    pub owners: std::collections::BTreeSet<Entity>,
}
impl GlobalEntry {
    fn selected(&self) -> &Buffer {
        &self.scratch[self.work.merges.len() % 2]
    }
}

pub(super) fn install(app: &mut App) {
    let registry = app.world().resource::<EmbeddedAssetRegistry>();
    for (name, source) in [
        ("particle_lights", PARTICLE_LIGHT_WESL),
        ("global_lights", GLOBAL_PARTICLE_LIGHT_WESL),
    ] {
        registry.insert_asset(
            PathBuf::from(format!("{name}.wesl")),
            Path::new(&format!("aestra_bevy_render/shaders/{name}.wesl")),
            source.as_bytes(),
        );
    }
    app.init_resource::<AestraParticleLightSettings>()
        .add_plugins((
            ExtractResourcePlugin::<AestraParticleLightSettings>::default(),
            ExtractComponentPlugin::<Inputs>::default(),
        ))
        .add_systems(PostUpdate, collect.after(super::sync_gpu_render_transforms));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .init_resource::<GpuSelectedParticleLights>()
            .init_resource::<PresentedOwners>()
            .add_systems(Render, init.in_set(RenderSystems::PrepareResources))
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareBindGroups))
            .add_systems(
                RenderGraph,
                select
                    .in_set(ParticleLightSelectionSet)
                    .after(super::run_simulation)
                    .before(RenderGraphSystems::Render),
            );
    }
}

fn collect(
    mut commands: Commands,
    settings: Res<AestraParticleLightSettings>,
    players: Query<(
        Entity,
        &PresentedEffect,
        &Pools,
        &EffectRuntimeStatus,
        Option<&EffectOutputContext>,
    )>,
    previous: Query<Entity, With<Inputs>>,
) {
    let mut retained = std::collections::BTreeSet::new();
    if settings.max_lights != 0 {
        for (owner, player, pools, runtime, context) in &players {
            if runtime.active != ActiveBackend::Gpu {
                continue;
            }
            let parameters: Arc<[_]> = player.instance.parameter_values().into();
            let mut inputs = Vec::new();
            for (index, emitter) in player.effect().emitters.iter().enumerate() {
                if !emitter.enabled {
                    continue;
                }
                let Some(&(offset, count)) = pools.0.get(index) else {
                    continue;
                };
                for output in &emitter.scene_outputs {
                    let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(plan) =
                        &output.kind;
                    if plan.max_lights == 0 || count == 0 {
                        continue;
                    }
                    inputs.push(Input {
                        source: ParticleLightSource {
                            root: context.map_or(owner, |c| c.root),
                            root_epoch: context
                                .map_or(player.instance.history_epoch(), |c| c.playback_epoch),
                            clip_path: context.map_or_else(Vec::new, |c| c.clip_path.clone()),
                            owner,
                            owner_epoch: player.instance.history_epoch(),
                            revision: player.instance.history_revision(),
                            effect: player.effect().source,
                            seed: player.instance.seed(),
                            emitter: emitter.source,
                            region: emitter.region,
                            output: output.source,
                            artifact: ParticleLightArtifact(player.effect().clone()),
                        },
                        emitter_index: index as u32,
                        offset,
                        count,
                        plan: plan.clone(),
                        parameters: parameters.clone(),
                    });
                }
            }
            if !inputs.is_empty() {
                commands.entity(owner).insert(Inputs(inputs));
                retained.insert(owner);
            }
        }
    }
    for owner in &previous {
        if !retained.contains(&owner) {
            commands.entity(owner).remove::<Inputs>();
        }
    }
}

fn init(
    mut commands: Commands,
    settings: Res<AestraParticleLightSettings>,
    existing: Option<Res<Pipelines>>,
    assets: Res<AssetServer>,
    cache: Res<PipelineCache>,
) {
    if settings.max_lights == 0 || existing.is_some() {
        return;
    }
    let local_layout = BindGroupLayoutDescriptor::new(
        "aestra particle light evaluation",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuParticle>>(false),
                storage_buffer_read_only::<GpuParticleLightPlan>(false),
                storage_buffer_read_only::<Vec<GpuLightKey>>(false),
                uniform_buffer::<GpuLightDispatch>(false),
                storage_buffer_read_only::<Vec<GpuParticleLight>>(false),
                storage_buffer::<Vec<GpuParticleLight>>(false),
                storage_buffer::<GpuLightCounters>(false),
                storage_buffer_read_only::<GpuRenderGlobals>(false),
            ),
        ),
    );
    let global_layout = BindGroupLayoutDescriptor::new(
        "aestra global light admission",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuParticleLight>>(false),
                storage_buffer::<Vec<GpuParticleLight>>(false),
                uniform_buffer::<GpuLightDispatch>(false),
                storage_buffer_read_only::<Vec<UVec4>>(false),
                storage_buffer::<GpuLightCounters>(false),
            ),
        ),
    );
    let build = |entries: &[&str], layout: &BindGroupLayoutDescriptor, name: &str| {
        entries
            .iter()
            .map(|entry| {
                cache.queue_compute_pipeline(ComputePipelineDescriptor {
                    label: Some(format!("aestra light {entry}").into()),
                    layout: vec![layout.clone()],
                    shader: assets
                        .load(format!("embedded://aestra_bevy_render/shaders/{name}.wesl")),
                    entry_point: Some((*entry).to_owned().into()),
                    ..default()
                })
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    };
    let local = build(
        PARTICLE_LIGHT_ENTRY_POINTS,
        &local_layout,
        "particle_lights",
    );
    let global = build(GLOBAL_LIGHT_ENTRY_POINTS, &global_layout, "global_lights");
    commands.insert_resource(Pipelines {
        local_layout,
        global_layout,
        local,
        global,
    });
}

fn encoded<T: ShaderType + bevy::render::render_resource::encase::internal::WriteInto>(
    value: &T,
) -> Vec<u8> {
    let mut bytes = StorageBuffer::new(Vec::new());
    bytes.write(value).unwrap();
    bytes.into_inner()
}
fn data(device: &RenderDevice, bytes: &[u8], usage: BufferUsages) -> Buffer {
    device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("aestra particle light data"),
        contents: bytes,
        usage,
    })
}
fn scratch(device: &RenderDevice, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("aestra particle light reusable scratch"),
        size,
        mapped_at_creation: false,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
    })
}

fn local_groups(device: &RenderDevice, layout: &BindGroupLayout, entry: &Entry) -> Vec<BindGroup> {
    let last = entry.uniforms.len() - 1;
    entry
        .uniforms
        .iter()
        .enumerate()
        .map(|(index, uniform)| {
            let (read, write) = if index == 0 {
                (1, 0)
            } else if index == last {
                (entry.work.merges.len() % 2, 1 - entry.work.merges.len() % 2)
            } else {
                ((index - 1) % 2, index % 2)
            };
            device.create_bind_group(
                Some("aestra output light job"),
                layout,
                &BindGroupEntries::sequential((
                    entry.particles.as_entire_buffer_binding(),
                    entry.plan.as_entire_buffer_binding(),
                    entry.keys.as_entire_buffer_binding(),
                    uniform.as_entire_buffer_binding(),
                    entry.scratch[read].as_entire_buffer_binding(),
                    entry.scratch[write].as_entire_buffer_binding(),
                    entry.counters.as_entire_buffer_binding(),
                    entry.globals.as_entire_buffer_binding(),
                )),
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn prepare(
    inputs: Query<(&Inputs, &GpuEffectBuffers)>,
    buffers: Res<RenderAssets<GpuShaderBuffer>>,
    settings: Res<AestraParticleLightSettings>,
    pipeline: Option<Res<Pipelines>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut state: ResMut<GpuSelectedParticleLights>,
    mut readiness: ResMut<PresentedOwners>,
) {
    readiness.enabled = settings.max_lights != 0;
    state.frame = None;
    state.rejection = None;
    state.rejected_outputs = 0;
    state.reserved_bytes = 0;
    if settings.max_lights == 0 {
        state.entries.clear();
        state.global = None;
        return;
    }
    let Some(pipeline) = pipeline else {
        return;
    };
    let limits = device.limits();
    let storage_limit = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    let mut jobs = BTreeMap::new();
    let mut bytes = 0u64;
    let mut requested_bound = 0u32;
    // Plan the entire frame before any GPU allocation: failure cannot bypass global
    // admission or silently clamp a source's quality cap to a device hard limit.
    let planning = (|| -> Result<_, ParticleLightError> {
        for (inputs, buffers_source) in &inputs {
            let (Some(particles), Some(globals)) = (
                buffers.get(&buffers_source.particles),
                buffers.get(&buffers_source.render_globals),
            ) else {
                state.rejected_outputs += inputs.0.len() as u32;
                continue;
            };
            for input in &inputs.0 {
                let token = u32::try_from(jobs.len()).map_err(|_| ParticleLightError::Capacity)?;
                let Some(work) = ParticleLightWorkPlan::for_output(
                    &input.plan,
                    input.offset,
                    input.count,
                    settings.max_lights,
                    storage_limit,
                    limits.max_compute_workgroups_per_dimension,
                )?
                else {
                    continue;
                };
                let (mut plan, keys) = match lower_plan(
                    &input.plan,
                    &input.parameters,
                    input.emitter_index,
                    token,
                    Mat4::IDENTITY,
                    storage_limit,
                ) {
                    Ok(value) => value,
                    Err(ParticleLightError::InvalidPlan) => {
                        state.rejected_outputs += 1;
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                plan.source.z = 1; // exactly the same transform as live sprite presentation
                let plan_bytes = encoded(&plan);
                let key_bytes = encoded(&keys);
                requested_bound = requested_bound
                    .checked_add(input.count)
                    .ok_or(ParticleLightError::Capacity)?;
                bytes = bytes
                    .checked_add(
                        work.scratch_bytes_per_buffer * 2
                            + 16
                            + plan_bytes.len() as u64
                            + key_bytes.len() as u64
                            + (work.merges.len() as u64 + 2) * 32,
                    )
                    .ok_or(ParticleLightError::Capacity)?;
                jobs.insert(
                    input.source.clone(),
                    (
                        work,
                        plan,
                        key_bytes,
                        particles.buffer.clone(),
                        globals.buffer.clone(),
                    ),
                );
            }
        }
        if jobs.is_empty() {
            return Ok(None);
        }
        let runs = u32::try_from(jobs.len()).map_err(|_| ParticleLightError::Capacity)?;
        let width = jobs.values().map(|j| j.0.selected_capacity).max().unwrap();
        let global = GlobalLightWorkPlan::new(
            width,
            runs,
            settings.max_lights,
            storage_limit,
            limits.max_compute_workgroups_per_dimension,
        )?
        .ok_or(ParticleLightError::InvalidPlan)?;
        bytes = bytes
            .checked_add(
                global.scratch_bytes_per_buffer * 2
                    + u64::from(runs) * 16
                    + 16
                    + (global.merges.len() as u64 + 2) * 32,
            )
            .ok_or(ParticleLightError::Capacity)?;
        if bytes > settings.max_scratch_bytes {
            return Err(ParticleLightError::Capacity);
        }
        Ok(Some(global))
    })();
    let global_work = match planning {
        Ok(Some(work)) => work,
        Ok(None) => {
            state.entries.clear();
            state.global = None;
            return;
        }
        Err(error) => {
            state.entries.clear();
            state.global = None;
            state.rejection = Some(format!("particle-light selection rejected: {error}"));
            return;
        }
    };
    state.reserved_bytes = bytes;
    state.entries.retain(|source, _| jobs.contains_key(source));
    let local_layout = cache.get_bind_group_layout(&pipeline.local_layout);
    for (token, (source, (work, mut plan, key_bytes, particles, globals))) in
        jobs.into_iter().enumerate()
    {
        // BTreeMap source order, not query/insertion order, defines tie keys.
        plan.gradient.z = token as u32;
        let plan_bytes = encoded(&plan);
        let entry = state.entries.entry(source).or_insert_with(|| {
            new_entry(
                &device,
                work.clone(),
                plan_bytes.clone(),
                key_bytes.clone(),
                particles.clone(),
                globals.clone(),
            )
        });
        if entry.work.evaluate != work.evaluate
            || entry.work.scratch_records != work.scratch_records
            || entry.key_bytes.len() != key_bytes.len()
        {
            *entry = new_entry(
                &device,
                work,
                plan_bytes.clone(),
                key_bytes.clone(),
                particles.clone(),
                globals.clone(),
            );
        }
        let rebind = entry.groups.is_empty()
            || entry.particles.id() != particles.id()
            || entry.globals.id() != globals.id();
        entry.particles = particles;
        entry.globals = globals;
        if entry.plan_bytes != plan_bytes {
            queue.write_buffer(&entry.plan, 0, &plan_bytes);
            entry.plan_bytes = plan_bytes;
        }
        if entry.key_bytes != key_bytes {
            queue.write_buffer(&entry.keys, 0, &key_bytes);
            entry.key_bytes = key_bytes;
        }
        if rebind {
            entry.groups = local_groups(&device, &local_layout, entry);
        }
    }
    let same = state.global.as_ref().is_some_and(|g| {
        g.work.input_width == global_work.input_width
            && g.work.input_runs == global_work.input_runs
            && g.work.selected_capacity == global_work.selected_capacity
    });
    if !same {
        state.global = Some(new_global(
            &device,
            &cache.get_bind_group_layout(&pipeline.global_layout),
            global_work,
        ));
    }
}

fn new_entry(
    device: &RenderDevice,
    work: ParticleLightWorkPlan,
    plan_bytes: Vec<u8>,
    key_bytes: Vec<u8>,
    particles: Buffer,
    globals: Buffer,
) -> Entry {
    let uniforms = std::iter::once(work.evaluate)
        .chain(work.merges.iter().copied())
        .chain(std::iter::once(work.finish))
        .map(|d| data(device, &encoded(&d), BufferUsages::UNIFORM))
        .collect();
    Entry {
        scratch: [
            scratch(device, work.scratch_bytes_per_buffer),
            scratch(device, work.scratch_bytes_per_buffer),
        ],
        counters: scratch(device, 16),
        plan: data(
            device,
            &plan_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        ),
        keys: data(
            device,
            &key_bytes,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        ),
        plan_bytes,
        key_bytes,
        work,
        particles,
        globals,
        groups: Vec::new(),
        uniforms,
    }
}

fn new_global(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    work: GlobalLightWorkPlan,
) -> GlobalEntry {
    let scratch_buffers = [
        scratch(device, work.scratch_bytes_per_buffer),
        scratch(device, work.scratch_bytes_per_buffer),
    ];
    let input_counts = scratch(device, u64::from(work.input_runs) * 16);
    let counters = scratch(device, 16);
    let schedule = work
        .merges
        .iter()
        .copied()
        .chain([work.finish, work.finish]);
    let count = work.merges.len();
    let groups = schedule
        .enumerate()
        .map(|(index, dispatch)| {
            let read = if index < count { index % 2 } else { count % 2 };
            let uniform = data(device, &encoded(&dispatch), BufferUsages::UNIFORM);
            device.create_bind_group(
                Some("aestra global light job"),
                layout,
                &BindGroupEntries::sequential((
                    scratch_buffers[read].as_entire_buffer_binding(),
                    scratch_buffers[1 - read].as_entire_buffer_binding(),
                    uniform.as_entire_buffer_binding(),
                    input_counts.as_entire_buffer_binding(),
                    counters.as_entire_buffer_binding(),
                )),
            )
        })
        .collect();
    GlobalEntry {
        work,
        scratch: scratch_buffers,
        input_counts,
        counters,
        groups,
    }
}

fn select(
    mut context: RenderContext,
    pipeline: Option<Res<Pipelines>>,
    cache: Res<PipelineCache>,
    mut state: ResMut<GpuSelectedParticleLights>,
    readiness: Res<PresentedOwners>,
    entities: Query<(&bevy::render::sync_world::MainEntity, Entity), With<GpuEffectBuffers>>,
) {
    let Some(pipeline) = pipeline else {
        return;
    };
    let Some(local): Option<Vec<_>> = pipeline
        .local
        .iter()
        .map(|id| cache.get_compute_pipeline(*id))
        .collect()
    else {
        return;
    };
    let Some(global): Option<Vec<_>> = pipeline
        .global
        .iter()
        .map(|id| cache.get_compute_pipeline(*id))
        .collect()
    else {
        return;
    };
    let Some(global_entry) = &state.global else {
        return;
    };
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let span = diagnostics.time_span(context.command_encoder(), "aestra::gpu::particle_lights");
    let encoder = context.command_encoder();
    encoder.clear_buffer(&global_entry.scratch[0], 0, None);
    encoder.clear_buffer(&global_entry.counters, 0, None);
    encoder.clear_buffer(&global_entry.input_counts, 0, None);
    let presented: std::collections::BTreeSet<_> = entities
        .iter()
        .filter(|(_, entity)| readiness.owners.contains(entity))
        .map(|(main, _)| main.id())
        .collect();
    for (index, (source, entry)) in state.entries.iter().enumerate() {
        if !presented.contains(&source.owner) {
            continue;
        }
        encoder.clear_buffer(&entry.counters, 0, None);
        for (pass_index, group) in entry.groups.iter().enumerate() {
            let first = pass_index == 0;
            let last = pass_index == entry.groups.len() - 1;
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("aestra output light selection"),
                timestamp_writes: None,
            });
            pass.set_pipeline(
                local[if first {
                    0
                } else if last {
                    2
                } else {
                    1
                }],
            );
            pass.set_bind_group(0, group, &[]);
            let count = if first {
                entry.work.evaluate.input.w
            } else if last {
                1
            } else {
                let d = entry.work.merges[pass_index - 1];
                (d.input.z * d.input.w).div_ceil(64)
            };
            pass.dispatch_workgroups(count, 1, 1);
        }
        encoder.copy_buffer_to_buffer(
            entry.selected(),
            0,
            &global_entry.scratch[0],
            index as u64 * u64::from(global_entry.work.input_width) * LIGHT_RECORD_BYTES,
            u64::from(entry.work.selected_capacity) * LIGHT_RECORD_BYTES,
        );
        encoder.copy_buffer_to_buffer(
            &entry.counters,
            0,
            &global_entry.input_counts,
            index as u64 * 16,
            16,
        );
    }
    for (index, group) in global_entry.groups.iter().enumerate() {
        let merge_count = global_entry.work.merges.len();
        let (pipeline_index, workgroups) = if index < merge_count {
            let d = global_entry.work.merges[index];
            (0, (d.input.z * d.input.w).div_ceil(64))
        } else if index == merge_count {
            (1, global_entry.work.input_runs.div_ceil(64))
        } else {
            (2, 1)
        };
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra global light admission"),
            timestamp_writes: None,
        });
        pass.set_pipeline(global[pipeline_index]);
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(workgroups, 1, 1);
    }
    span.end(encoder);
    let records = global_entry.selected().clone();
    let counters = global_entry.counters.clone();
    let selected_capacity = global_entry.work.selected_capacity;
    let scratch_bytes = global_entry.work.scratch_bytes_per_buffer * 2
        + state
            .entries
            .values()
            .map(|e| e.work.scratch_bytes_per_buffer * 2)
            .sum::<u64>();
    state.sequence = state.sequence.wrapping_add(1);
    state.frame = Some(SelectedGpuParticleLights {
        records,
        counters,
        manifest: state.entries.keys().cloned().collect(),
        selected_capacity,
        scratch_bytes,
        reserved_bytes: state.reserved_bytes,
        sequence: state.sequence,
        rejected_outputs: state.rejected_outputs,
    });
}
