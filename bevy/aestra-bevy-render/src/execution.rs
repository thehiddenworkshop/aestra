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
//! - a convergent repeat (fluid F5) is expanded to its cap with its body dispatched **indirectly**; a
//!   one-invocation test after each iteration empties every remaining shape once the residual has
//!   converged, so the loop stops on the device without a readback;
//! - an op with `indirect` counts (fluid F7) is sized on the device: its workgroup counts are read
//!   from a resource earlier ops wrote (a sparse grid's active bricks), again without a readback;
//! - before anything is allocated the block is checked against its programs' WGSL
//!   ([`check_program_block`]), so the declared accesses the IR's hazard reasoning trusts are exactly
//!   what the shaders use.
//!
//! [`StageTimeline`] adds time: it advances a stage to the fixed tick a presentation time asks for,
//! within a per-frame catch-up budget, capturing GPU-resident checkpoints at a cadence and restoring
//! the nearest one on a backward seek — so scrubbing reproduces the uninterrupted run exactly. Both are
//! engine-neutral `wgpu`: tests drive them on a standalone device, the Bevy render world on its own.

use aestra_compiler::{ComputeProgram, ComputeProgramRegistry};
use aestra_core::{ComputeProgramId, ResourceTypeId};
use aestra_gpu::{
    GpuHostBindings, GpuWorldSdf, ProgramInterfaces, check_program_block_cached, source_hash,
};
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_HOST_BINDINGS, AESTRA_RESOURCE_STAGE_CONSTANTS,
    AESTRA_RESOURCE_WORLD_SDF, ExecutionBlock, ExecutionOp, FieldLayout, FrameConstants,
    RepeatPolicy, ResourceLifetime, StagedDispatch,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;
use std::time::Duration;
use wgpu::util::DeviceExt;

const READBACK_TIMEOUT: Duration = Duration::from_secs(120);

/// A grid field's buffers as a reader binds them (see [`StageExecutor::field_buffers`]).
#[derive(Clone, Copy)]
pub struct FieldBuffers<'a> {
    pub field: &'a wgpu::Buffer,
    /// A bricked field's brick table (fluid F7); `None` when every cell is stored.
    pub table: Option<&'a wgpu::Buffer>,
}

impl<'a> FieldBuffers<'a> {
    /// The table, or — for a field that stores every cell — the field itself, bound where a reader's
    /// shader declares the table but never reads it.
    fn table_or_field(&self) -> &'a wgpu::Buffer {
        self.table.unwrap_or(self.field)
    }
}

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

/// Where an indirect dispatch reads its workgroup counts.
#[derive(Debug, Clone, Copy)]
enum Indirect {
    /// A convergent repeat's control buffer: the repeat, the byte offset.
    Control(usize, u64),
    /// A declared resource the block's ops wrote them to (fluid F7): its binding, the byte offset.
    Resource(usize, u64),
}

/// One step of a tick, with repeats expanded.
#[derive(Debug, Clone, Copy)]
enum Step {
    /// A dispatch: its prepared pipeline/bind group and its shape, unless its shape is read on the
    /// device. Inside a convergent repeat every body shape is read from the repeat's control buffer.
    Compute {
        dispatch: usize,
        shape: StagedDispatch,
        indirect: Option<Indirect>,
    },
    /// Before a convergent repeat runs: an indirect body op's device-written counts copied into its
    /// slot of the repeat's control buffer (resource binding and byte offset, repeat, slot offset).
    Counts {
        from: usize,
        offset: u64,
        repeat: usize,
        to: u64,
    },
    /// A convergent repeat's device-side test: after an iteration, or before the first one (which
    /// counts no iteration).
    Check {
        repeat: usize,
        before: bool,
    },
    Copy {
        from: usize,
        to: usize,
    },
}

impl Step {
    /// Encoded inside a compute pass.
    fn in_pass(&self) -> bool {
        matches!(self, Self::Compute { .. } | Self::Check { .. })
    }
}

/// The device-side test of a [`RepeatPolicy::UntilConverged`] (fluid F5). Its control buffer holds
/// `[iterations run, live, then (x, y, z) per body dispatch]`; the body dispatches indirectly from it,
/// and after each iteration [`CONVERGENCE_WGSL`] zeroes every shape once the residual is at or below
/// the tolerance — the remaining iterations then dispatch nothing. Reset from `initial` every tick.
struct ConvergentRepeat {
    control: wgpu::Buffer,
    initial: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

/// The convergence test: runs after each iteration of a convergent repeat, one invocation.
const CONVERGENCE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> residual: array<u32>;
@group(0) @binding(1) var<storage, read_write> control: array<u32>;
@group(0) @binding(2) var<storage, read> params: array<u32>;

// `<=` is false for a residual that is not a number: it never converges.
fn converge() {
    if (bitcast<f32>(residual[0]) <= bitcast<f32>(params[0])) {
        control[1] = 0u;
        for (var i = 0u; i < params[1] * 3u; i = i + 1u) {
            control[2u + i] = 0u;
        }
    }
}

@compute @workgroup_size(1)
fn check() {
    if (control[1] == 0u) {
        return;
    }
    control[0] = control[0] + 1u;
    converge();
}

@compute @workgroup_size(1)
fn check_first() {
    converge();
}
"#;

/// Shader modules, pipelines and checked program interfaces kept across executors on one device.
/// Rebuilding a stage (an edit that changes its resources or ops) names the same programs again;
/// with a cache it reuses their compiled pipelines instead of recompiling every entry point. Entries
/// are keyed by program and a hash of its source, so a changed program is compiled again.
#[derive(Default)]
pub struct ProgramCache {
    interfaces: ProgramInterfaces,
    modules: HashMap<ComputeProgramId, (u64, wgpu::ShaderModule)>,
    pipelines: HashMap<(ComputeProgramId, String), (u64, wgpu::ComputePipeline)>,
    check: Option<[wgpu::ComputePipeline; 2]>,
}

impl ProgramCache {
    fn pipeline(
        &mut self,
        device: &wgpu::Device,
        program: &ComputeProgram,
        entry_point: &str,
    ) -> wgpu::ComputePipeline {
        let hash = source_hash(&program.wgsl);
        let key = (program.id.clone(), entry_point.to_owned());
        if let Some((cached, pipeline)) = self.pipelines.get(&key)
            && *cached == hash
        {
            return pipeline.clone();
        }
        let fresh = self
            .modules
            .get(&program.id)
            .is_some_and(|(cached, _)| *cached == hash);
        if !fresh {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(program.id.as_str()),
                source: wgpu::ShaderSource::Wgsl(program.wgsl.as_str().into()),
            });
            self.modules.insert(program.id.clone(), (hash, module));
        }
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry_point),
            layout: None,
            module: &self.modules[&program.id].1,
            entry_point: Some(entry_point),
            compilation_options: Default::default(),
            cache: None,
        });
        self.pipelines.insert(key, (hash, pipeline.clone()));
        pipeline
    }
}

/// One compiled plugin stage, allocated on a device and ready to run ticks.
pub struct StageExecutor {
    block: ExecutionBlock,
    buffers: Vec<wgpu::Buffer>,
    pipelines: Vec<wgpu::ComputePipeline>,
    /// Per compute op, in depth-first op order (repeat bodies once): its pipeline and bind group.
    dispatches: Vec<(usize, wgpu::BindGroup)>,
    /// Per compute op, likewise: its name and the bindings its bind group holds, so the groups can be
    /// built again when a host-sized resource (the world SDF) is reallocated.
    dispatch_bindings: Vec<(String, Vec<u32>)>,
    /// The revision of the world SDF uploaded, if any (fluid F11).
    world_revision: Option<u64>,
    /// A tick's steps, repeats expanded.
    steps: Vec<Step>,
    /// Convergent repeats, in depth-first op order, and the test pipelines they share: after an
    /// iteration, and before the first.
    convergent: Vec<ConvergentRepeat>,
    check: Option<[wgpu::ComputePipeline; 2]>,
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
        Self::with_cache(
            device,
            queue,
            block,
            programs,
            host_binding_bytes,
            &mut ProgramCache::default(),
        )
    }

    /// [`StageExecutor::new`], reusing the programs `cache` already compiled on `device`.
    pub fn with_cache(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        block: &ExecutionBlock,
        programs: &ComputeProgramRegistry,
        host_binding_bytes: u64,
        cache: &mut ProgramCache,
    ) -> Result<Self, String> {
        block.validate().map_err(|error| error.to_string())?;
        check_program_block_cached(block, programs, &mut cache.interfaces)?;

        let mut counts = Vec::new();
        indirect_sources(&block.ops, &mut counts);
        let mut buffers = Vec::with_capacity(block.resources.len());
        for resource in &block.resources {
            let indirect = if counts.contains(&&resource.id) {
                wgpu::BufferUsages::INDIRECT
            } else {
                wgpu::BufferUsages::empty()
            };
            let bytes = match resource.id.as_str() {
                AESTRA_RESOURCE_HOST_BINDINGS => host_binding_bytes,
                // Absent until the host supplies a world (fluid F11): a zeroed header says so.
                AESTRA_RESOURCE_WORLD_SDF => GpuWorldSdf::absent().byte_len(),
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
                    | wgpu::BufferUsages::COPY_DST
                    | indirect,
                mapped_at_creation: false,
            }));
        }
        let mut stage = Self {
            block: block.clone(),
            buffers,
            pipelines: Vec::new(),
            dispatches: Vec::new(),
            dispatch_bindings: Vec::new(),
            world_revision: None,
            steps: Vec::new(),
            convergent: Vec::new(),
            check: None,
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

        let mut pipeline_index: HashMap<(ComputeProgramId, String), usize> = HashMap::new();
        let mut dispatches = Vec::new();
        stage.prepare_ops(
            device,
            &block.ops,
            programs,
            cache,
            &mut pipeline_index,
            &mut dispatches,
        )?;
        stage.dispatches = dispatches;
        stage.prepare_convergence(device, &block.ops, cache)?;
        let mut steps = Vec::new();
        let (mut cursor, mut repeat) = (0, 0);
        stage.expand_steps(&block.ops, &mut cursor, &mut repeat, None, &mut steps)?;
        stage.steps = steps;
        Ok(stage)
    }

    /// Allocates each convergent repeat's control buffer and binds its test, in depth-first order.
    fn prepare_convergence(
        &mut self,
        device: &wgpu::Device,
        ops: &[ExecutionOp],
        cache: &mut ProgramCache,
    ) -> Result<(), String> {
        for op in ops {
            let ExecutionOp::Repeat { policy, body } = op else {
                continue;
            };
            let RepeatPolicy::UntilConverged {
                residual,
                tolerance,
                ..
            } = policy
            else {
                self.prepare_convergence(device, body, cache)?;
                continue;
            };
            let shapes: Vec<StagedDispatch> = body
                .iter()
                .filter_map(|op| match op {
                    ExecutionOp::Compute(compute) => Some(compute.dispatch),
                    _ => None,
                })
                .collect();
            let mut words = vec![0, 1];
            for shape in &shapes {
                words.extend([shape.x, shape.y, shape.z]);
            }
            let bytes = words_to_bytes(&words);
            let initial = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aestra convergence reset"),
                contents: &bytes,
                usage: wgpu::BufferUsages::COPY_SRC,
            });
            let control = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aestra convergence control"),
                contents: &bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDIRECT
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            });
            let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("aestra convergence tolerance"),
                contents: &words_to_bytes(&[tolerance.to_bits(), shapes.len() as u32]),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let residual = self.require_binding(residual.as_str())?;
            let residual = &self.buffers[residual];
            let check = cache.check.get_or_insert_with(|| {
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("aestra convergence"),
                    source: wgpu::ShaderSource::Wgsl(CONVERGENCE_WGSL.into()),
                });
                // One explicit layout: the two entries use the same bindings.
                let entry = |binding, read_only| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                };
                let bindings = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some("aestra convergence"),
                    entries: &[entry(0, true), entry(1, false), entry(2, true)],
                });
                let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("aestra convergence"),
                    bind_group_layouts: &[Some(&bindings)],
                    immediate_size: 0,
                });
                ["check", "check_first"].map(|name| {
                    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some("aestra convergence"),
                        layout: Some(&layout),
                        module: &module,
                        entry_point: Some(name),
                        compilation_options: Default::default(),
                        cache: None,
                    })
                })
            });
            let check = self.check.get_or_insert_with(|| check.clone());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aestra convergence"),
                layout: &check[0].get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: residual.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: control.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
            self.convergent.push(ConvergentRepeat {
                control,
                initial,
                bind_group,
            });
        }
        Ok(())
    }

    fn prepare_ops(
        &mut self,
        device: &wgpu::Device,
        ops: &[ExecutionOp],
        programs: &ComputeProgramRegistry,
        cache: &mut ProgramCache,
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
                            let program = programs
                                .get(&program_id)
                                .expect("checked by check_program_block");
                            let pipeline = cache.pipeline(device, program, &compute.entry_point);
                            self.pipelines.push(pipeline);
                            pipeline_index.insert(key, self.pipelines.len() - 1);
                            self.pipelines.len() - 1
                        }
                    };
                    let bindings: Vec<u32> = compute
                        .accesses
                        .iter()
                        .map(|access| {
                            self.block
                                .binding_of(&access.resource)
                                .expect("validated block")
                        })
                        .collect();
                    let bind_group = self.bind_group(device, index, &compute.name, &bindings);
                    dispatches.push((index, bind_group));
                    self.dispatch_bindings
                        .push((compute.name.clone(), bindings));
                }
                ExecutionOp::Repeat { body, .. } => {
                    self.prepare_ops(device, body, programs, cache, pipeline_index, dispatches)?;
                }
                ExecutionOp::Barrier | ExecutionOp::Copy(_) => {}
            }
        }
        Ok(())
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        pipeline: usize,
        name: &str,
        bindings: &[u32],
    ) -> wgpu::BindGroup {
        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|&binding| wgpu::BindGroupEntry {
                binding,
                resource: self.buffers[binding as usize].as_entire_binding(),
            })
            .collect();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &self.pipelines[pipeline].get_bind_group_layout(0),
            entries: &entries,
        })
    }

    /// Hands the stage the host's world SDF (fluid F11): uploaded in `encoder` when its revision is
    /// new, into a buffer reallocated (and the bind groups built again) when its size changed. Ticks
    /// encoded after it see it. A stage that does not collide with the world ignores it. True when
    /// something was uploaded.
    pub fn provide_world_sdf(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        world: &GpuWorldSdf,
    ) -> bool {
        let Some(binding) = self.binding(AESTRA_RESOURCE_WORLD_SDF) else {
            return false;
        };
        if self.world_revision == Some(world.revision) {
            return false;
        }
        if self.buffers[binding].size() != world.byte_len() {
            self.buffers[binding] = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(AESTRA_RESOURCE_WORLD_SDF),
                size: world.byte_len(),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let dispatches = (0..self.dispatches.len())
                .map(|op| {
                    let (name, bindings) = &self.dispatch_bindings[op];
                    let pipeline = self.dispatches[op].0;
                    (pipeline, self.bind_group(device, pipeline, name, bindings))
                })
                .collect();
            self.dispatches = dispatches;
        }
        self.stage_upload(device, encoder, binding, &world.to_bytes());
        self.world_revision = Some(world.revision);
        true
    }

    /// Expands `ops` into steps. `cursor` walks the prepared dispatches, `repeat` the convergent
    /// repeats; `convergent` is the repeat whose body `ops` is, if any.
    fn expand_steps(
        &self,
        ops: &[ExecutionOp],
        cursor: &mut usize,
        repeat: &mut usize,
        convergent: Option<usize>,
        steps: &mut Vec<Step>,
    ) -> Result<(), String> {
        let mut body_dispatch = 0u64;
        for op in ops {
            match op {
                ExecutionOp::Compute(compute) => {
                    let indirect = match (convergent, &compute.indirect) {
                        // After the two leading control words, three per dispatch.
                        (Some(repeat), _) => {
                            Some(Indirect::Control(repeat, (2 + 3 * body_dispatch) * 4))
                        }
                        (None, Some(counts)) => Some(Indirect::Resource(
                            self.require_binding(counts.resource.as_str())?,
                            u64::from(counts.word) * 4,
                        )),
                        (None, None) => None,
                    };
                    steps.push(Step::Compute {
                        dispatch: *cursor,
                        shape: compute.dispatch,
                        indirect,
                    });
                    *cursor += 1;
                    body_dispatch += 1;
                }
                ExecutionOp::Barrier => {} // pass order already orders dependent dispatches
                ExecutionOp::Copy(copy) => steps.push(Step::Copy {
                    from: self.require_binding(copy.from.as_str())?,
                    to: self.require_binding(copy.to.as_str())?,
                }),
                ExecutionOp::Repeat { policy, body } => {
                    let start = *cursor;
                    let own = matches!(policy, RepeatPolicy::UntilConverged { .. }).then(|| {
                        *repeat += 1;
                        *repeat - 1
                    });
                    let nested_start = *repeat;
                    if let Some(own) = own {
                        // Device-written counts replace their ops' full shapes before the repeat
                        // starts (its test then empties them once converged).
                        let body_ops = body.iter().filter_map(|op| match op {
                            ExecutionOp::Compute(compute) => Some(compute),
                            _ => None,
                        });
                        for (slot, compute) in body_ops.enumerate() {
                            if let Some(counts) = &compute.indirect {
                                steps.push(Step::Counts {
                                    from: self.require_binding(counts.resource.as_str())?,
                                    offset: u64::from(counts.word) * 4,
                                    repeat: own,
                                    to: (2 + 3 * slot as u64) * 4,
                                });
                            }
                        }
                    }
                    if let (
                        Some(own),
                        RepeatPolicy::UntilConverged {
                            test_first: true, ..
                        },
                    ) = (own, policy)
                    {
                        steps.push(Step::Check {
                            repeat: own,
                            before: true,
                        });
                    }
                    for _ in 0..policy.count() {
                        *cursor = start;
                        *repeat = nested_start;
                        self.expand_steps(body, cursor, repeat, own, steps)?;
                        if let Some(own) = own {
                            steps.push(Step::Check {
                                repeat: own,
                                before: false,
                            });
                        }
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

    /// The buffers a reader of the grid field `layout` binds: the field's, and a bricked field's
    /// brick table's (fluid F7).
    pub fn field_buffers(&self, layout: &FieldLayout) -> Option<FieldBuffers<'_>> {
        let field = self.buffer(layout.resource.as_str())?;
        let table = match &layout.bricks {
            Some(bricks) => Some(self.buffer(bricks.table.as_str())?),
            None => None,
        };
        Some(FieldBuffers { field, table })
    }

    /// Compute passes one tick encodes (runs of consecutive dispatches).
    pub fn passes_per_tick(&self) -> usize {
        self.steps
            .iter()
            .enumerate()
            .filter(|(index, step)| {
                step.in_pass() && (*index == 0 || !self.steps[index - 1].in_pass())
            })
            .count()
    }

    /// How many iterations each convergent repeat ran in the last tick encoded, in depth-first op
    /// order (blocking readback: for tests, benchmarks and tools).
    pub fn convergent_iterations(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Vec<u32>, String> {
        self.convergent
            .iter()
            .map(|repeat| {
                let bytes = read_buffer(device, queue, &repeat.control)?;
                Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            })
            .collect()
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
        // Every convergent repeat starts the tick live, with its full dispatch shapes.
        for repeat in &self.convergent {
            encoder.copy_buffer_to_buffer(
                &repeat.initial,
                0,
                &repeat.control,
                0,
                repeat.initial.size(),
            );
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
                Step::Counts {
                    from,
                    offset,
                    repeat,
                    to,
                } => {
                    let control = &self.convergent[repeat].control;
                    encoder.copy_buffer_to_buffer(&self.buffers[from], offset, control, to, 12);
                    index += 1;
                }
                Step::Compute { .. } | Step::Check { .. } => {
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
                    while let Some(step) = self.steps.get(index).filter(|step| step.in_pass()) {
                        match *step {
                            Step::Compute {
                                dispatch,
                                shape,
                                indirect,
                            } => {
                                let (pipeline, bind_group) = &self.dispatches[dispatch];
                                pass.set_pipeline(&self.pipelines[*pipeline]);
                                pass.set_bind_group(0, bind_group, &[]);
                                match indirect {
                                    Some(Indirect::Control(repeat, offset)) => pass
                                        .dispatch_workgroups_indirect(
                                            &self.convergent[repeat].control,
                                            offset,
                                        ),
                                    Some(Indirect::Resource(binding, offset)) => pass
                                        .dispatch_workgroups_indirect(
                                            &self.buffers[binding],
                                            offset,
                                        ),
                                    None => pass.dispatch_workgroups(shape.x, shape.y, shape.z),
                                }
                            }
                            Step::Check { repeat, before } => {
                                let check = self.check.as_ref().expect("prepared with its repeats");
                                pass.set_pipeline(&check[usize::from(before)]);
                                pass.set_bind_group(0, &self.convergent[repeat].bind_group, &[]);
                                pass.dispatch_workgroups(1, 1, 1);
                            }
                            Step::Copy { .. } | Step::Counts { .. } => {
                                unreachable!("filtered to in-pass steps")
                            }
                        }
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

    /// Replaces the stage constants in place (fluid F3: live domain edits). The layout must keep its
    /// length; the constants are not stage state, so nothing else changes.
    pub fn set_constants(&mut self, queue: &wgpu::Queue, constants: &[u32]) -> Result<(), String> {
        if constants.len() != self.block.constants.len() {
            return Err(format!(
                "stage constants changed length ({} → {} words)",
                self.block.constants.len(),
                constants.len()
            ));
        }
        if let Some(binding) = self
            .binding(AESTRA_RESOURCE_STAGE_CONSTANTS)
            .filter(|_| !constants.is_empty())
        {
            queue.write_buffer(&self.buffers[binding], 0, &words_to_bytes(constants));
        }
        self.block.constants = constants.to_vec();
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
        read_buffer(device, queue, &self.buffers[self.require_binding(id)?])
    }

    /// Copies the stage's output resources out for the host and zeroes them, in `encoder` (fluid F11):
    /// ticks encoded after it start a new frame of outputs. Once the encoder's work is submitted and
    /// done, `done` receives the words of each output resource (from the device's poll; nothing when
    /// the mapping fails). False for a stage without outputs.
    pub fn encode_output_readback(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        done: impl FnOnce(BTreeMap<ResourceTypeId, Vec<u32>>) + Send + 'static,
    ) -> bool {
        let mut bindings: Vec<usize> = Vec::new();
        for output in &self.block.outputs {
            if let Some(binding) = self.binding(output.resource.as_str())
                && !bindings.contains(&binding)
            {
                bindings.push(binding);
            }
        }
        if bindings.is_empty() {
            return false;
        }
        let mut layout = Vec::new();
        let mut offset = 0;
        for &binding in &bindings {
            let size = self.buffers[binding].size();
            layout.push((self.block.resources[binding].id.clone(), offset, size));
            offset += size;
        }
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aestra stage outputs"),
            size: offset,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        for (&binding, (_, offset, size)) in bindings.iter().zip(&layout) {
            encoder.copy_buffer_to_buffer(&self.buffers[binding], 0, &staging, *offset, *size);
            encoder.clear_buffer(&self.buffers[binding], 0, None);
        }
        let mapped = staging.clone();
        encoder.map_buffer_on_submit(&staging, wgpu::MapMode::Read, .., move |result| {
            if result.is_err() {
                return;
            }
            let words = split_outputs(&layout, &mapped.slice(..).get_mapped_range());
            mapped.unmap();
            done(words);
        });
        true
    }

    /// The stage's outputs now, as their resources' words, zeroing them (blocking: tests and tools).
    pub fn read_outputs(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<BTreeMap<ResourceTypeId, Vec<u32>>, String> {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let (sender, receiver) = mpsc::channel();
        if !self.encode_output_readback(device, &mut encoder, move |words| {
            let _ = sender.send(words);
        }) {
            return Ok(BTreeMap::new());
        }
        queue.submit([encoder.finish()]);
        device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(READBACK_TIMEOUT),
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(READBACK_TIMEOUT)
            .map_err(|error| error.to_string())
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
                            | AESTRA_RESOURCE_WORLD_SDF
                    )
                    // Outputs are the host's (fluid F11).
                    && !self.block.is_output(&resource.id)
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
    /// The host's world SDF (fluid F11), uploaded to stages that collide with it when it changes.
    pub world_sdf: Option<&'a GpuWorldSdf>,
}

impl Default for StageInputs<'_> {
    /// No host bindings; the effect at the world origin.
    fn default() -> Self {
        Self {
            host_bindings: None,
            world_to_effect: aestra_runtime::IDENTITY_AFFINE,
            world_sdf: None,
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
    /// The live state was simulated under more than one set of constants (a live edit), so a replay
    /// would not reproduce it: no checkpoint is captured from it until the next reset.
    mixed_history: bool,
}

impl StageTimeline {
    pub fn new(executor: StageExecutor, policy: TimelinePolicy, seed: u32) -> Self {
        Self {
            executor,
            policy,
            seed,
            last_tick: 0,
            checkpoints: Vec::new(),
            mixed_history: false,
        }
    }

    /// Applies new constants to the running stage without restarting it (fluid F3): the edit acts
    /// from the next tick on. Checkpoints recorded under the old constants are dropped, and none is
    /// captured until a reset replays under the new ones. False when nothing changed.
    pub fn set_constants(
        &mut self,
        queue: &wgpu::Queue,
        constants: &[u32],
    ) -> Result<bool, String> {
        if self.executor.block.constants == constants {
            return Ok(false);
        }
        self.executor.set_constants(queue, constants)?;
        self.checkpoints.clear();
        self.mixed_history = self.last_tick > 0;
        Ok(true)
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

    /// Returns the stage to exactly `tick` (fluid F2b's joint seek): tick 0 resets it; any other tick
    /// restores the checkpoint captured there. False (state unchanged) when there is none.
    pub fn restore_to(&mut self, encoder: &mut wgpu::CommandEncoder, tick: u32) -> bool {
        if tick == 0 {
            self.executor.reset(encoder);
            self.last_tick = 0;
            self.mixed_history = false;
            return true;
        }
        let Some((_, checkpoint)) = self.checkpoints.iter().find(|(at, _)| *at == tick) else {
            return false;
        };
        self.executor.restore_from(encoder, checkpoint);
        self.last_tick = tick;
        true
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
        if let Some(world) = inputs.world_sdf {
            self.executor.provide_world_sdf(device, encoder, world);
        }
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
                    self.mixed_history = false;
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
        if self.mixed_history
            || self.policy.max_checkpoints == 0
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

/// Follow Field for stateful particles (fluid F2b): pulls each live slot's velocity toward a domain's
/// vector field (see [`aestra_gpu::field_follow_wgsl`]). Engine-neutral, like [`StageExecutor`].
pub struct FieldFollowPipeline {
    pipeline: wgpu::ComputePipeline,
}

impl FieldFollowPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aestra follow field"),
            source: wgpu::ShaderSource::Wgsl(aestra_gpu::field_follow_wgsl().into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("aestra follow field"),
            layout: None,
            module: &module,
            entry_point: Some("follow_field"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { pipeline }
    }

    /// Encodes one pull of `capacity` persistent-state slots toward `follow`'s field.
    #[allow(clippy::too_many_arguments)]
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        state: &wgpu::Buffer,
        capacity: u32,
        field: FieldBuffers<'_>,
        follow: &aestra_runtime::CompiledFieldFollow,
        dt: f32,
    ) {
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra follow field params"),
            contents: &words_to_bytes(&aestra_gpu::field_follow_params(capacity, follow, dt)),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra follow field"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: field.field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: field.table_or_field().as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra follow field"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(capacity.div_ceil(64).max(1), 1, 1);
    }
}

/// An emitter's persistent buffers, as Spawn From Domain binds them.
pub struct SpawnState<'a> {
    pub state: &'a wgpu::Buffer,
    pub free_list: &'a wgpu::Buffer,
    pub free_count: &'a wgpu::Buffer,
    pub spawn_counter: &'a wgpu::Buffer,
    /// The emitter's stateful params for the tick.
    pub params: &'a wgpu::Buffer,
}

/// Spawn From Domain for stateful particles (fluid F10, G8): turns a domain's emission list into
/// particles on the device — a one-thread plan, then an indirect dispatch over the planned records
/// (see [`aestra_gpu::DOMAIN_SPAWN_PLAN_WGSL`] and [`aestra_gpu::domain_spawn_wgsl`]). Engine-neutral,
/// like [`StageExecutor`].
pub struct DomainSpawnPipeline {
    plan: wgpu::ComputePipeline,
    spawn: wgpu::ComputePipeline,
}

pub struct SpawnAcceptanceCounter<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub word: u32,
}

impl DomainSpawnPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let pipeline = |label: &str, source: String, entry: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            plan: pipeline(
                "aestra domain spawn plan",
                aestra_gpu::DOMAIN_SPAWN_PLAN_WGSL.into(),
                "domain_spawn_plan",
            ),
            spawn: pipeline(
                "aestra domain spawn",
                aestra_gpu::domain_spawn_wgsl(),
                "domain_spawn",
            ),
        }
    }

    /// Encodes one tick's spawn of `emission`'s records into `target`, as `spawn` says.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: SpawnState<'_>,
        emission: &wgpu::Buffer,
        spawn: &aestra_runtime::CompiledDomainSpawn,
    ) {
        let unused = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra unused spawn acceptance counter"),
            contents: &0u32.to_le_bytes(),
            usage: wgpu::BufferUsages::STORAGE,
        });
        self.encode_with_acceptance(
            device,
            encoder,
            target,
            emission,
            spawn,
            SpawnAcceptanceCounter {
                buffer: &unused,
                word: u32::MAX,
            },
        );
    }

    pub fn encode_with_acceptance(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: SpawnState<'_>,
        emission: &wgpu::Buffer,
        spawn: &aestra_runtime::CompiledDomainSpawn,
        acceptance: SpawnAcceptanceCounter<'_>,
    ) {
        let mut words = aestra_gpu::domain_spawn_params(spawn);
        words[2] = acceptance.word;
        let spawn_params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra domain spawn params"),
            contents: &words_to_bytes(&words),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let plan = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aestra domain spawn plan"),
            size: aestra_gpu::DOMAIN_SPAWN_PLAN_WORDS as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::INDIRECT,
            mapped_at_creation: false,
        });
        let group = |pipeline: &wgpu::ComputePipeline, buffers: &[&wgpu::Buffer]| {
            let entries: Vec<wgpu::BindGroupEntry> = buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("aestra domain spawn"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            })
        };
        let plan_group = group(
            &self.plan,
            &[
                target.free_count,
                target.spawn_counter,
                emission,
                &plan,
                &spawn_params,
                acceptance.buffer,
            ],
        );
        let spawn_group = group(
            &self.spawn,
            &[
                target.state,
                target.free_list,
                target.params,
                emission,
                &plan,
                &spawn_params,
            ],
        );
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("aestra domain spawn plan"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.plan);
            pass.set_bind_group(0, &plan_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra domain spawn"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.spawn);
        pass.set_bind_group(0, &spawn_group, &[]);
        pass.dispatch_workgroups_indirect(&plan, 0);
    }
}

/// Particle event links on the device (host bindings HB9b): gathers one link's events of a tick into
/// an emission list in source-ordinal order ([`aestra_gpu::PARTICLE_EVENT_GATHER_WGSL`]), which
/// [`DomainSpawnPipeline`] then turns into the target's particles ([`Self::spawn`]). Engine-neutral,
/// like [`StageExecutor`].
pub struct EventGatherPipeline {
    order: wgpu::ComputePipeline,
    expand: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Particle output routes' per-tick aggregation (event system E3), beside the links' gather.
    outputs: wgpu::ComputePipeline,
}

/// Where a particle output route's tick record goes (event system E3): the tick reached, into its
/// slot of the route's ring, which starts at word `ring` of `counters`.
pub struct ParticleOutputSlot<'a> {
    pub counters: &'a wgpu::Buffer,
    pub ring: u32,
    pub tick: u32,
}

/// Persistent counter words for a link's captured child demand and list overflow.
pub struct EventLinkCounters<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub requested_word: u32,
    pub dropped_word: u32,
}

/// An event link's emission buffer and the number of records allocated in it.
pub struct EventEmissionList<'a> {
    pub buffer: &'a wgpu::Buffer,
    pub capacity: u32,
}

impl EventGatherPipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        const _: () = assert!(aestra_runtime::PARTICLE_EVENT_CAPACITY == 1024);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aestra event gather"),
            source: wgpu::ShaderSource::Wgsl(aestra_gpu::PARTICLE_EVENT_GATHER_WGSL.into()),
        });
        let entries: [_; 5] = std::array::from_fn(|binding| wgpu::BindGroupLayoutEntry {
            binding: binding as u32,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: matches!(binding, 0 | 2),
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("aestra event gather layout"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("aestra event gather pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let order = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("aestra event order"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("order_events"),
            // The kernel writes every workgroup word it reads. wgpu's default zero-fill of its
            // 12 KiB made the software D3D12 adapter (WARP) spend a minute compiling the shader on
            // its first dispatch, and is wasted work on every other backend. Bevy's own pipeline
            // descriptors leave it off by default.
            compilation_options: wgpu::PipelineCompilationOptions {
                zero_initialize_workgroup_memory: false,
                ..Default::default()
            },
            cache: None,
        });
        let expand = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("aestra event expansion"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("expand_events"),
            compilation_options: Default::default(),
            cache: None,
        });
        let outputs_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("aestra particle outputs"),
            source: wgpu::ShaderSource::Wgsl(aestra_gpu::PARTICLE_OUTPUT_WGSL.into()),
        });
        let outputs = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("aestra particle outputs"),
            layout: None,
            module: &outputs_module,
            entry_point: Some("aggregate_particle_outputs"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self {
            order,
            expand,
            layout,
            outputs,
        }
    }

    /// Encodes one tick's aggregation of a particle output route (event system E3): the source's
    /// `events` of the route's trigger, counted and the lowest-ordinal ones written into `slot`.
    /// See [`aestra_gpu::PARTICLE_OUTPUT_WGSL`].
    pub fn encode_output(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        events: &wgpu::Buffer,
        slot: ParticleOutputSlot<'_>,
        route: &aestra_runtime::CompiledParticleOutput,
    ) {
        let ParticleOutputSlot {
            counters,
            ring,
            tick,
        } = slot;
        let slot = ring
            + (tick % aestra_gpu::PARTICLE_OUTPUT_RING_TICKS)
                * aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS;
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra particle output route"),
            contents: &words_to_bytes(&[
                aestra_runtime::event_trigger_bit(route.trigger),
                route.aggregation.limit(),
                slot,
                tick + 1,
                aestra_runtime::PARTICLE_EVENT_CAPACITY,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra particle outputs"),
            layout: &self.outputs.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: events.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: counters.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra particle outputs"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.outputs);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }

    /// Bytes of a source emitter's bounded event buffer.
    pub fn buffer_bytes() -> u64 {
        aestra_gpu::particle_event_words(aestra_runtime::PARTICLE_EVENT_CAPACITY) as u64 * 4
    }

    /// A link can produce `count` children for every event its source can capture in one tick.
    /// The authoring limit of 800 children bounds this to 819,200 records (25 MiB) per link.
    pub fn list_capacity(source_capacity: u32, count: u32) -> u32 {
        source_capacity
            .min(aestra_runtime::PARTICLE_EVENT_CAPACITY)
            .saturating_mul(count)
            .clamp(
                1,
                aestra_runtime::PARTICLE_EVENT_CAPACITY * aestra_core::MAX_EVENT_LINK_COUNT,
            )
    }

    pub fn list_bytes(capacity: u32) -> u64 {
        aestra_gpu::particle_event_words(capacity) as u64 * 4
    }

    /// The Spawn From Domain a link's list is spawned with.
    pub fn spawn(
        link: &aestra_runtime::CompiledEventLink,
        list_capacity: u32,
    ) -> aestra_runtime::CompiledDomainSpawn {
        aestra_runtime::CompiledDomainSpawn {
            stage: 0,
            emission: aestra_runtime::EmissionLayout {
                resource: aestra_core::ResourceTypeId::new("aestra.resource.particle_events"),
                capacity: list_capacity,
            },
            inherit: link.inherit,
        }
    }

    /// Encodes one tick's gather of `link`'s events from the source's `events` buffer into `list`
    /// (cleared first).
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        events: &wgpu::Buffer,
        list: EventEmissionList<'_>,
        link: &aestra_runtime::CompiledEventLink,
    ) {
        // Standalone callers need no telemetry; production passes the effect's persistent counter.
        let unused = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra unused event overflow counter"),
            contents: &0u32.to_le_bytes(),
            usage: wgpu::BufferUsages::STORAGE,
        });
        self.encode_with_overflow(
            device,
            encoder,
            events,
            list,
            link,
            EventLinkCounters {
                buffer: &unused,
                requested_word: u32::MAX,
                dropped_word: u32::MAX,
            },
        );
    }

    pub fn encode_with_overflow(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        events: &wgpu::Buffer,
        list: EventEmissionList<'_>,
        link: &aestra_runtime::CompiledEventLink,
        counters: EventLinkCounters<'_>,
    ) {
        encoder.clear_buffer(list.buffer, 0, Some(4));
        let capacity = aestra_runtime::PARTICLE_EVENT_CAPACITY;
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra event gather params"),
            contents: &words_to_bytes(&[
                aestra_runtime::event_trigger_bit(link.trigger),
                link.count,
                list.capacity,
                capacity,
                counters.requested_word,
                counters.dropped_word,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let order = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aestra event order and dispatch plan"),
            size: (6 + aestra_runtime::PARTICLE_EVENT_CAPACITY as u64) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let indirect = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aestra event expansion indirect dispatch"),
            size: 12,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::INDIRECT,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra event gather"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: events.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: list.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: counters.buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: order.as_entire_binding(),
                },
            ],
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("aestra event order"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.order);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&order, 0, &indirect, 0, 12);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra event expansion"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.expand);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups_indirect(&indirect, 0);
    }
}

/// An input route's burst on the device (event system E3), the host counterpart of the link gather:
/// its records ([`aestra_runtime::InputSpawnBurst::records`]) as a Spawn From Domain emission list —
/// the `[count, 0, 0, 0]` header, then 8-word records `[position xyz, 0, velocity xyz, 0]`, velocity
/// zero — and the spawn that turns them into the target's particles with its launch velocity.
pub fn input_burst_list(
    burst: &aestra_runtime::InputSpawnBurst,
) -> (Vec<u32>, aestra_runtime::CompiledDomainSpawn) {
    let records: Vec<[f32; 3]> = burst.records().collect();
    let count = records.len() as u32;
    let mut words = vec![count, 0, 0, 0];
    for position in records {
        words.extend(position.map(f32::to_bits));
        words.extend([0; 5]);
    }
    let spawn = aestra_runtime::CompiledDomainSpawn {
        stage: 0,
        emission: aestra_runtime::EmissionLayout {
            resource: aestra_core::ResourceTypeId::new("aestra.resource.input_burst"),
            capacity: count,
        },
        inherit: 0.0,
    };
    (words, spawn)
}

/// Copies a grid field into an `rgba16float` 3-D storage texture of the grid's size (fluid F3), so
/// volume presentations sample it with hardware trilinear filtering (see
/// [`aestra_gpu::volume::FIELD_TO_VOLUME_WGSL`]); a bricked field (fluid F7) into a brick atlas and
/// its table texture ([`aestra_gpu::volume::bricks_to_volume_wgsl`]). Engine-neutral, like
/// [`StageExecutor`].
pub struct FieldVolumePipeline {
    pipeline: wgpu::ComputePipeline,
    bricks: wgpu::ComputePipeline,
    table: wgpu::ComputePipeline,
}

impl FieldVolumePipeline {
    pub fn new(device: &wgpu::Device) -> Self {
        let compute = |source: String, entry: &str| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("aestra field volume"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("aestra field volume"),
                layout: None,
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let bricks = aestra_gpu::volume::bricks_to_volume_wgsl();
        Self {
            pipeline: compute(
                aestra_gpu::volume::FIELD_TO_VOLUME_WGSL.into(),
                "copy_field",
            ),
            bricks: compute(bricks.clone(), "copy_bricks"),
            table: compute(bricks, "copy_table"),
        }
    }

    /// Encodes the copy of `field`, laid out as `layout`, into the 3-D texture `volume` — for a
    /// bricked field, a brick atlas, and its brick table into `table`.
    pub fn encode(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        field: FieldBuffers<'_>,
        volume: &wgpu::TextureView,
        layout: &aestra_runtime::FieldLayout,
        table: Option<&wgpu::TextureView>,
    ) {
        if let (Some(bricks), Some(table_buffer), Some(table)) =
            (&layout.bricks, field.table, table)
        {
            self.encode_bricks(
                device,
                encoder,
                (field.field, table_buffer),
                (volume, table),
                layout,
                bricks,
            );
            return;
        }
        let field = field.field;
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra field volume params"),
            contents: &words_to_bytes(&[
                layout.dims[0],
                layout.dims[1],
                layout.dims[2],
                layout.components,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra field volume"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: field.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(volume),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra field volume"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(
            layout.dims[0].div_ceil(4),
            layout.dims[1].div_ceil(4),
            layout.dims[2].div_ceil(4),
        );
    }

    fn encode_bricks(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        (field, table_buffer): (&wgpu::Buffer, &wgpu::Buffer),
        (volume, table): (&wgpu::TextureView, &wgpu::TextureView),
        layout: &aestra_runtime::FieldLayout,
        bricks: &aestra_runtime::BrickLayout,
    ) {
        let per_axis = aestra_gpu::volume::brick_atlas_bricks(bricks.slots);
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("aestra brick volume params"),
            contents: &words_to_bytes(&[
                layout.dims[0],
                layout.dims[1],
                layout.dims[2],
                layout.components,
                bricks.edge,
                per_axis,
                bricks.table_word,
                bricks.slot_bricks_word,
                bricks.slots,
            ]),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        let atlas = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra brick atlas"),
            layout: &self.bricks.get_bind_group_layout(0),
            entries: &[
                entry(0, field.as_entire_binding()),
                entry(1, params.as_entire_binding()),
                entry(2, wgpu::BindingResource::TextureView(volume)),
                entry(3, table_buffer.as_entire_binding()),
            ],
        });
        let table_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("aestra brick table"),
            layout: &self.table.get_bind_group_layout(0),
            entries: &[
                entry(1, params.as_entire_binding()),
                entry(3, table_buffer.as_entire_binding()),
                entry(4, wgpu::BindingResource::TextureView(table)),
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("aestra brick volume"),
            timestamp_writes: None,
        });
        let texels = per_axis * (bricks.edge + 2);
        pass.set_pipeline(&self.bricks);
        pass.set_bind_group(0, &atlas, &[]);
        pass.dispatch_workgroups(texels.div_ceil(4), texels.div_ceil(4), texels.div_ceil(4));
        let grid = bricks.grid(layout.dims);
        pass.set_pipeline(&self.table);
        pass.set_bind_group(0, &table_group, &[]);
        pass.dispatch_workgroups(
            grid[0].div_ceil(4),
            grid[1].div_ceil(4),
            grid[2].div_ceil(4),
        );
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

/// The resources indirect compute ops read their counts from.
fn indirect_sources<'a>(ops: &'a [ExecutionOp], sources: &mut Vec<&'a ResourceTypeId>) {
    for op in ops {
        match op {
            ExecutionOp::Compute(compute) => {
                if let Some(counts) = &compute.indirect {
                    sources.push(&counts.resource);
                }
            }
            ExecutionOp::Repeat { body, .. } => indirect_sources(body, sources),
            ExecutionOp::Barrier | ExecutionOp::Copy(_) => {}
        }
    }
}

/// Reads a buffer back (blocking).
fn read_buffer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &wgpu::Buffer,
) -> Result<Vec<u8>, String> {
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

fn split_outputs(
    layout: &[(ResourceTypeId, u64, u64)],
    bytes: &[u8],
) -> BTreeMap<ResourceTypeId, Vec<u32>> {
    layout
        .iter()
        .map(|(id, offset, size)| {
            let range = &bytes[*offset as usize..(*offset + *size) as usize];
            let words = range
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| u32::from_le_bytes(*chunk))
                .collect();
            (id.clone(), words)
        })
        .collect()
}

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}
