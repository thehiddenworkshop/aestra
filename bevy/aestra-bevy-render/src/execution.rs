//! Native GPU execution of program-backed Execution IR stages (extensible-stages M13, fluid F1).
//!
//! [`StageExecutor`] runs one compiled plugin stage's [`ExecutionBlock`] on a `wgpu` device:
//!
//! - every declared resource becomes one storage buffer, bound at `@group(0) @binding(i)` for its
//!   declaration index `i` (the IR binding convention). Allocation is bounded by the declared sizes;
//! - [`AESTRA_RESOURCE_STAGE_CONSTANTS`] is uploaded once from the block's constants;
//!   [`AESTRA_RESOURCE_FRAME`] and [`AESTRA_RESOURCE_HOST_BINDINGS`] are uploaded **inside the command
//!   encoder** before each tick, so several ticks encoded into one submission each see their own
//!   frame (a `queue.write_buffer` would expose only the last);
//! - **transient resources are zeroed at the start of every tick**, so no state survives in scratch
//!   and a restored checkpoint replays exactly;
//! - consecutive compute ops share one compute pass. WebGPU orders dependent dispatches inside a pass
//!   (each dispatch is its own usage scope), so this is as correct as one pass per op and far
//!   cheaper; a `Copy` ends the pass;
//! - before anything is allocated the block is checked against its programs' WGSL
//!   ([`check_program_block`]), so the declared accesses the IR's hazard reasoning trusts are exactly
//!   what the shaders use.
//!
//! [`StageTimeline`] adds time: it advances a stage to the fixed tick a presentation time asks for,
//! within a per-frame catch-up budget, capturing GPU-resident checkpoints at a cadence and restoring
//! the nearest one on a backward seek — so scrubbing reproduces the uninterrupted run exactly. Both are
//! engine-neutral `wgpu`: tests drive them on a standalone device, the Bevy render world on its own.

use aestra_compiler::ComputeProgramRegistry;
use aestra_core::{ComputeProgramId, ResourceTypeId};
use aestra_gpu::{GpuHostBindings, check_program_block};
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_HOST_BINDINGS, AESTRA_RESOURCE_STAGE_CONSTANTS,
    ExecutionBlock, ExecutionOp, FrameConstants, ResourceLifetime, StagedDispatch,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;
use std::time::Duration;
use wgpu::util::DeviceExt;

const READBACK_TIMEOUT: Duration = Duration::from_secs(120);

/// The persistent state of a stage at a tick boundary, read back to the CPU: the bytes of every
/// persistent resource the stage owns (host-written built-ins excluded).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StageCheckpoint {
    pub resources: BTreeMap<ResourceTypeId, Vec<u8>>,
}

impl StageCheckpoint {
    /// Total checkpointed bytes.
    pub fn bytes(&self) -> usize {
        self.resources.values().map(Vec::len).sum()
    }
}

/// A GPU-resident copy of a stage's persistent state — what [`StageTimeline`] keeps for seeking.
pub struct GpuStageCheckpoint {
    buffers: Vec<(usize, wgpu::Buffer)>,
    bytes: u64,
}

impl GpuStageCheckpoint {
    /// Device memory the checkpoint holds.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

/// Timestamp writes for the first and/or last compute pass a call encodes.
#[derive(Clone, Copy)]
pub struct PassTimestamps<'a> {
    pub query_set: &'a wgpu::QuerySet,
    pub begin: Option<u32>,
    pub end: Option<u32>,
}

/// One step of a tick, with repeats expanded.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// A dispatch: its prepared pipeline/bind group and its shape.
    Compute {
        dispatch: usize,
        shape: StagedDispatch,
    },
    Copy {
        from: usize,
        to: usize,
    },
}

/// One compiled plugin stage, allocated on a device and ready to run ticks.
pub struct StageExecutor {
    block: ExecutionBlock,
    buffers: Vec<wgpu::Buffer>,
    pipelines: Vec<wgpu::ComputePipeline>,
    /// Per compute op, in depth-first op order (repeat bodies once): its pipeline and bind group.
    dispatches: Vec<(usize, wgpu::BindGroup)>,
    /// A tick's steps, repeats expanded.
    steps: Vec<Step>,
}

impl StageExecutor {
    /// Checks `block` against `programs`, allocates its resources, uploads its constants and builds a
    /// pipeline per distinct entry point. `host_binding_bytes` sizes the host-bindings resource when
    /// the block declares it (see [`GpuHostBindings::byte_len`]; the size is fixed per compiled
    /// effect). Persistent resources start zeroed — a stage's tick-0 state.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        block: &ExecutionBlock,
        programs: &ComputeProgramRegistry,
        host_binding_bytes: u64,
    ) -> Result<Self, String> {
        block.validate().map_err(|error| error.to_string())?;
        check_program_block(block, programs)?;

        let mut buffers = Vec::with_capacity(block.resources.len());
        for resource in &block.resources {
            let bytes = match resource.id.as_str() {
                AESTRA_RESOURCE_HOST_BINDINGS => host_binding_bytes,
                _ => resource.bytes,
            };
            if bytes == 0 {
                return Err(format!(
                    "resource '{}' has no size; this executor owns only sized stage resources",
                    resource.id.as_str()
                ));
            }
            buffers.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(resource.id.as_str()),
                size: bytes.next_multiple_of(4),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let mut stage = Self {
            block: block.clone(),
            buffers,
            pipelines: Vec::new(),
            dispatches: Vec::new(),
            steps: Vec::new(),
        };
        if let Some(binding) = stage
            .binding(AESTRA_RESOURCE_STAGE_CONSTANTS)
            .filter(|_| !block.constants.is_empty())
        {
            queue.write_buffer(
                &stage.buffers[binding],
                0,
                &words_to_bytes(&block.constants),
            );
        }

        let mut modules: HashMap<ComputeProgramId, wgpu::ShaderModule> = HashMap::new();
        let mut pipeline_index: HashMap<(ComputeProgramId, String), usize> = HashMap::new();
        let mut dispatches = Vec::new();
        stage.prepare_ops(
            device,
            &block.ops,
            programs,
            &mut modules,
            &mut pipeline_index,
            &mut dispatches,
        )?;
        stage.dispatches = dispatches;
        let mut steps = Vec::new();
        let mut cursor = 0;
        stage.expand_steps(&block.ops, &mut cursor, &mut steps)?;
        stage.steps = steps;
        Ok(stage)
    }

    fn prepare_ops(
        &mut self,
        device: &wgpu::Device,
        ops: &[ExecutionOp],
        programs: &ComputeProgramRegistry,
        modules: &mut HashMap<ComputeProgramId, wgpu::ShaderModule>,
        pipeline_index: &mut HashMap<(ComputeProgramId, String), usize>,
        dispatches: &mut Vec<(usize, wgpu::BindGroup)>,
    ) -> Result<(), String> {
        for op in ops {
            match op {
                ExecutionOp::Compute(compute) => {
                    // `check_program_block` guaranteed the program exists and declares the entry.
                    let program_id = compute
                        .program
                        .clone()
                        .ok_or("compute op without program")?;
                    let key = (program_id.clone(), compute.entry_point.clone());
                    let index = match pipeline_index.get(&key) {
                        Some(index) => *index,
                        None => {
                            let module = modules.entry(program_id.clone()).or_insert_with(|| {
                                let program = programs
                                    .get(&program_id)
                                    .expect("checked by check_program_block");
                                device.create_shader_module(wgpu::ShaderModuleDescriptor {
                                    label: Some(program_id.as_str()),
                                    source: wgpu::ShaderSource::Wgsl(program.wgsl.as_str().into()),
                                })
                            });
                            let pipeline =
                                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                                    label: Some(&compute.entry_point),
                                    layout: None,
                                    module,
                                    entry_point: Some(&compute.entry_point),
                                    compilation_options: Default::default(),
                                    cache: None,
                                });
                            self.pipelines.push(pipeline);
                            pipeline_index.insert(key, self.pipelines.len() - 1);
                            self.pipelines.len() - 1
                        }
                    };
                    let entries: Vec<wgpu::BindGroupEntry> = compute
                        .accesses
                        .iter()
                        .map(|access| {
                            let binding = self
                                .block
                                .binding_of(&access.resource)
                                .expect("validated block");
                            wgpu::BindGroupEntry {
                                binding,
                                resource: self.buffers[binding as usize].as_entire_binding(),
                            }
                        })
                        .collect();
                    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&compute.name),
                        layout: &self.pipelines[index].get_bind_group_layout(0),
                        entries: &entries,
                    });
                    dispatches.push((index, bind_group));
                }
                ExecutionOp::Repeat { body, .. } => {
                    self.prepare_ops(device, body, programs, modules, pipeline_index, dispatches)?;
                }
                ExecutionOp::Barrier | ExecutionOp::Copy(_) => {}
            }
        }
        Ok(())
    }

    fn expand_steps(
        &self,
        ops: &[ExecutionOp],
        cursor: &mut usize,
        steps: &mut Vec<Step>,
    ) -> Result<(), String> {
        for op in ops {
            match op {
                ExecutionOp::Compute(compute) => {
                    steps.push(Step::Compute {
                        dispatch: *cursor,
                        shape: compute.dispatch,
                    });
                    *cursor += 1;
                }
                ExecutionOp::Barrier => {} // pass order already orders dependent dispatches
                ExecutionOp::Copy(copy) => steps.push(Step::Copy {
                    from: self.require_binding(copy.from.as_str())?,
                    to: self.require_binding(copy.to.as_str())?,
                }),
                ExecutionOp::Repeat { policy, body } => {
                    let start = *cursor;
                    for _ in 0..policy.count() {
                        *cursor = start;
                        self.expand_steps(body, cursor, steps)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// The block this executor runs.
    pub fn block(&self) -> &ExecutionBlock {
        &self.block
    }

    /// The device buffer holding a declared resource — for renderers and debug views that read a
    /// stage's fields in place.
    pub fn buffer(&self, id: &str) -> Option<&wgpu::Buffer> {
        self.binding(id).map(|binding| &self.buffers[binding])
    }

    /// Compute passes one tick encodes (runs of consecutive dispatches).
    pub fn passes_per_tick(&self) -> usize {
        self.steps
            .iter()
            .enumerate()
            .filter(|(index, step)| {
                matches!(step, Step::Compute { .. })
                    && (*index == 0 || !matches!(self.steps[index - 1], Step::Compute { .. }))
            })
            .count()
    }

    fn binding(&self, id: &str) -> Option<usize> {
        self.block
            .resources
            .iter()
            .position(|resource| resource.id.as_str() == id)
    }

    fn require_binding(&self, id: &str) -> Result<usize, String> {
        self.binding(id)
            .ok_or_else(|| format!("undeclared resource '{id}'"))
    }

    /// Encodes one tick into `encoder`: stages `frame` (and `host_bindings`, when given and declared)
    /// into their resources, zeroes the transient resources, then encodes every step in order.
    /// `timestamps` are written at the start of the tick's first pass and the end of its last.
    pub fn encode_tick(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        frame: FrameConstants,
        host_bindings: Option<&GpuHostBindings>,
        timestamps: Option<PassTimestamps<'_>>,
    ) -> Result<(), String> {
        if let Some(binding) = self.binding(AESTRA_RESOURCE_FRAME) {
            self.stage_upload(device, encoder, binding, &words_to_bytes(&frame.to_words()));
        }
        if let (Some(binding), Some(host_bindings)) =
            (self.binding(AESTRA_RESOURCE_HOST_BINDINGS), host_bindings)
        {
            if host_bindings.byte_len() > self.buffers[binding].size() {
                return Err(format!(
                    "host bindings are {} bytes; the stage allocated {}",
                    host_bindings.byte_len(),
                    self.buffers[binding].size()
                ));
            }
            self.stage_upload(device, encoder, binding, &host_bindings.to_bytes());
        }
        for (resource, buffer) in self.block.resources.iter().zip(&self.buffers) {
            if resource.lifetime == ResourceLifetime::Transient {
                encoder.clear_buffer(buffer, 0, None);
            }
        }
        let pass_count = self.passes_per_tick();
        let mut pass_index = 0;
        let mut index = 0;
        while index < self.steps.len() {
            match self.steps[index] {
                Step::Copy { from, to } => {
                    let (from, to) = (&self.buffers[from], &self.buffers[to]);
                    encoder.copy_buffer_to_buffer(from, 0, to, 0, from.size().min(to.size()));
                    index += 1;
                }
                Step::Compute { .. } => {
                    let timestamp_writes = timestamps.and_then(|stamps| {
                        let begin = stamps.begin.filter(|_| pass_index == 0);
                        let end = stamps.end.filter(|_| pass_index + 1 == pass_count);
                        (begin.is_some() || end.is_some()).then_some(
                            wgpu::ComputePassTimestampWrites {
                                query_set: stamps.query_set,
                                beginning_of_pass_write_index: begin,
                                end_of_pass_write_index: end,
                            },
                        )
                    });
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some("aestra stage"),
                        timestamp_writes,
                    });
                    while let Some(Step::Compute { dispatch, shape }) = self.steps.get(index) {
                        let (pipeline, bind_group) = &self.dispatches[*dispatch];
                        pass.set_pipeline(&self.pipelines[*pipeline]);
                        pass.set_bind_group(0, bind_group, &[]);
                        pass.dispatch_workgroups(shape.x, shape.y, shape.z);
                        index += 1;
                    }
                    pass_index += 1;
                }
            }
        }
        Ok(())
    }

    fn stage_upload(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        binding: usize,
        bytes: &[u8],
    ) {
        let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra stage upload"),
            contents: bytes,
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&staging, 0, &self.buffers[binding], 0, bytes.len() as u64);
    }

    /// Encodes and submits one tick.
    pub fn run_tick(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: FrameConstants,
        host_bindings: Option<&GpuHostBindings>,
    ) -> Result<(), String> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("aestra stage tick"),
        });
        self.encode_tick(device, &mut encoder, frame, host_bindings, None)?;
        queue.submit([encoder.finish()]);
        Ok(())
    }

    /// Returns the stage to its tick-0 state: every stage-owned persistent resource zeroed.
    pub fn reset(&self, encoder: &mut wgpu::CommandEncoder) {
        for binding in self.stage_owned_persistent() {
            encoder.clear_buffer(&self.buffers[binding], 0, None);
        }
    }

    /// Bytes of stage-owned persistent state — the size of one checkpoint.
    pub fn persistent_bytes(&self) -> u64 {
        self.stage_owned_persistent()
            .map(|binding| self.buffers[binding].size())
            .sum()
    }

    /// Copies the persistent state into a new GPU-resident checkpoint (GPU→GPU, in `encoder`).
    pub fn capture(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> GpuStageCheckpoint {
        let mut buffers = Vec::new();
        let mut bytes = 0;
        for binding in self.stage_owned_persistent() {
            let source = &self.buffers[binding];
            let copy = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aestra stage checkpoint"),
                size: source.size(),
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(source, 0, &copy, 0, source.size());
            bytes += source.size();
            buffers.push((binding, copy));
        }
        GpuStageCheckpoint { buffers, bytes }
    }

    /// Restores a GPU-resident checkpoint taken from this stage (GPU→GPU, in `encoder`).
    pub fn restore_from(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        checkpoint: &GpuStageCheckpoint,
    ) {
        for (binding, copy) in &checkpoint.buffers {
            encoder.copy_buffer_to_buffer(copy, 0, &self.buffers[*binding], 0, copy.size());
        }
    }

    /// Reads a resource back (blocking). For tests, CPU checkpoints and tools — never per frame.
    pub fn read_resource(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: &str,
    ) -> Result<Vec<u8>, String> {
        let source = &self.buffers[self.require_binding(id)?];
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aestra stage readback"),
            size: source.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(source, 0, &readback, 0, source.size());
        queue.submit([encoder.finish()]);
        let slice = readback.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(READBACK_TIMEOUT)
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let bytes = slice.get_mapped_range().to_vec();
        readback.unmap();
        Ok(bytes)
    }

    /// Reads every persistent resource the stage owns back to the CPU (blocking).
    pub fn checkpoint(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<StageCheckpoint, String> {
        let mut resources = BTreeMap::new();
        for binding in self.stage_owned_persistent() {
            let id = &self.block.resources[binding].id;
            resources.insert(id.clone(), self.read_resource(device, queue, id.as_str())?);
        }
        Ok(StageCheckpoint { resources })
    }

    /// Restores a CPU checkpoint taken from this stage.
    pub fn restore(&self, queue: &wgpu::Queue, checkpoint: &StageCheckpoint) -> Result<(), String> {
        for binding in self.stage_owned_persistent() {
            let id = &self.block.resources[binding].id;
            let bytes = checkpoint
                .resources
                .get(id)
                .ok_or_else(|| format!("checkpoint lacks '{}'", id.as_str()))?;
            queue.write_buffer(&self.buffers[binding], 0, bytes);
        }
        Ok(())
    }

    fn stage_owned_persistent(&self) -> impl Iterator<Item = usize> + '_ {
        self.block
            .resources
            .iter()
            .enumerate()
            .filter(|(_, resource)| {
                resource.lifetime == ResourceLifetime::Persistent
                    && !matches!(
                        resource.id.as_str(),
                        AESTRA_RESOURCE_FRAME
                            | AESTRA_RESOURCE_STAGE_CONSTANTS
                            | AESTRA_RESOURCE_HOST_BINDINGS
                    )
            })
            .map(|(binding, _)| binding)
    }
}

/// How a [`StageTimeline`] maps time to ticks and bounds its checkpoints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelinePolicy {
    /// The fixed simulation step.
    pub tick_dt: f32,
    /// Ticks between checkpoints.
    pub cadence: u32,
    /// Checkpoints kept; beyond it the store is coarsened (every other dropped).
    pub max_checkpoints: usize,
    /// Checkpoint memory kept; beyond it the store is coarsened the same way.
    pub max_bytes: u64,
}

impl Default for TimelinePolicy {
    /// 60 Hz, a checkpoint every 20 ticks (1/3 s), at most 64 checkpoints and 256 MiB — the same
    /// cadence and entry budget the stateful particle path uses.
    fn default() -> Self {
        Self {
            tick_dt: 1.0 / 60.0,
            cadence: 20,
            max_checkpoints: 64,
            max_bytes: 256 * 1024 * 1024,
        }
    }
}

/// The host inputs a timeline hands every tick it simulates (fluid F2): the effect's host binding
/// snapshots and its placement (`world_to_effect`, see [`FrameConstants`]). Catch-up and replay ticks
/// reuse the current inputs — recorded host history is host-bindings HB8.
#[derive(Clone, Copy)]
pub struct StageInputs<'a> {
    pub host_bindings: Option<&'a GpuHostBindings>,
    pub world_to_effect: [[f32; 4]; 3],
}

impl Default for StageInputs<'_> {
    /// No host bindings; the effect at the world origin.
    fn default() -> Self {
        Self {
            host_bindings: None,
            world_to_effect: aestra_runtime::IDENTITY_AFFINE,
        }
    }
}

/// What one [`StageTimeline::advance_to`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdvanceReport {
    /// Ticks simulated.
    pub ticks: u32,
    /// The checkpoint tick a backward seek restored, or `Some(0)` when it reset to the start.
    pub restored_from: Option<u32>,
}

/// A stage executor placed on a fixed-tick timeline, with GPU-resident checkpoints for seeking.
pub struct StageTimeline {
    executor: StageExecutor,
    policy: TimelinePolicy,
    seed: u32,
    last_tick: u32,
    checkpoints: Vec<(u32, GpuStageCheckpoint)>,
}

impl StageTimeline {
    pub fn new(executor: StageExecutor, policy: TimelinePolicy, seed: u32) -> Self {
        Self {
            executor,
            policy,
            seed,
            last_tick: 0,
            checkpoints: Vec::new(),
        }
    }

    pub fn executor(&self) -> &StageExecutor {
        &self.executor
    }

    pub fn policy(&self) -> TimelinePolicy {
        self.policy
    }

    /// The tick the persistent state currently represents.
    pub fn last_tick(&self) -> u32 {
        self.last_tick
    }

    /// The fixed tick a presentation time falls in (a small epsilon keeps `tick * dt` on its tick).
    pub fn tick_for_time(&self, time: f32) -> u32 {
        (time.max(0.0) / self.policy.tick_dt + 1e-3).floor() as u32
    }

    /// The ticks of the checkpoints held, ascending.
    pub fn checkpoint_ticks(&self) -> Vec<u32> {
        self.checkpoints.iter().map(|(tick, _)| *tick).collect()
    }

    /// Device memory the checkpoints hold.
    pub fn checkpoint_bytes(&self) -> u64 {
        self.checkpoints
            .iter()
            .map(|(_, checkpoint)| checkpoint.bytes())
            .sum()
    }

    /// Drops every checkpoint — when what they were recorded against changed (a rebound host object)
    /// while the live state stays valid.
    pub fn invalidate_checkpoints(&mut self) {
        self.checkpoints.clear();
    }

    /// Brings the state to `target` tick. A backward seek restores the nearest checkpoint at or before
    /// the target (or resets to tick 0) and replays forward — never integrates in reverse. At most
    /// `budget` ticks are simulated; the rest continue on later calls. Checkpoints are captured at the
    /// policy cadence. `timestamps` bracket all the ticks simulated.
    pub fn advance_to(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: u32,
        budget: u32,
        inputs: StageInputs<'_>,
        timestamps: Option<PassTimestamps<'_>>,
    ) -> Result<AdvanceReport, String> {
        let mut report = AdvanceReport::default();
        if target < self.last_tick {
            let nearest = self
                .checkpoints
                .partition_point(|(tick, _)| *tick <= target)
                .checked_sub(1);
            match nearest {
                Some(index) => {
                    let (tick, checkpoint) = &self.checkpoints[index];
                    self.executor.restore_from(encoder, checkpoint);
                    self.last_tick = *tick;
                    report.restored_from = Some(*tick);
                }
                None => {
                    self.executor.reset(encoder);
                    self.last_tick = 0;
                    report.restored_from = Some(0);
                }
            }
        }
        let ticks = (target - self.last_tick).min(budget);
        for index in 0..ticks {
            let stamps = timestamps.map(|stamps| PassTimestamps {
                query_set: stamps.query_set,
                begin: stamps.begin.filter(|_| index == 0),
                end: stamps.end.filter(|_| index + 1 == ticks),
            });
            let frame = FrameConstants::fixed_step(self.last_tick, self.policy.tick_dt, self.seed)
                .with_world_to_effect(inputs.world_to_effect);
            self.executor
                .encode_tick(device, encoder, frame, inputs.host_bindings, stamps)?;
            self.last_tick += 1;
            if self.last_tick.is_multiple_of(self.policy.cadence) {
                self.capture(device, encoder);
            }
        }
        report.ticks = ticks;
        Ok(report)
    }

    fn capture(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        let tick = self.last_tick;
        if self.policy.max_checkpoints == 0
            || self.executor.persistent_bytes() > self.policy.max_bytes
            || self
                .checkpoints
                .iter()
                .any(|(existing, _)| *existing == tick)
        {
            return;
        }
        let position = self
            .checkpoints
            .partition_point(|(existing, _)| *existing < tick);
        self.checkpoints
            .insert(position, (tick, self.executor.capture(device, encoder)));
        while self.checkpoints.len() > self.policy.max_checkpoints
            || self.checkpoint_bytes() > self.policy.max_bytes
        {
            retain_every_other(&mut self.checkpoints);
        }
    }
}

/// Halves a store by keeping entries 0, 2, 4… — doubling the effective cadence while keeping
/// full-timeline coverage.
fn retain_every_other<T>(items: &mut Vec<T>) {
    let mut keep = false;
    items.retain(|_| {
        keep = !keep;
        keep
    });
}

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}
