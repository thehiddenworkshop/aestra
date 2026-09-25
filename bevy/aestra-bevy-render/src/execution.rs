//! Native GPU execution of program-backed Execution IR stages (extensible-stages M13).
//!
//! [`StageExecutor`] runs one compiled plugin stage's [`ExecutionBlock`] on a `wgpu` device, tick by
//! tick. It is what the M7 conformance harness proved, promoted into the render backend so plugin
//! stages (the fluid first) have one real executor:
//!
//! - every declared resource becomes one storage buffer, bound at `@group(0) @binding(i)` for its
//!   declaration index `i` (the IR binding convention). Allocation is bounded by the declared sizes;
//! - [`AESTRA_RESOURCE_STAGE_CONSTANTS`] is uploaded once from the block's constants,
//!   [`AESTRA_RESOURCE_FRAME`] before every tick, and [`AESTRA_RESOURCE_HOST_BINDINGS`] whenever the
//!   host supplies a fresh [`GpuHostBindings`];
//! - **transient resources are zeroed at the start of every tick**, so no state survives in scratch
//!   and a restored checkpoint replays exactly;
//! - every compute op is its own compute pass, so each op's writes are visible to the next (a real
//!   ordering barrier); `Repeat` loops its body; `Copy` copies buffers;
//! - before anything is allocated the block is checked against its programs'
//!   WGSL ([`check_program_block`]), so the declared accesses the IR's hazard reasoning trusts are
//!   exactly what the shaders use.
//!
//! The executor is engine-neutral `wgpu`: tests drive it on a standalone device, and the Bevy render
//! world can drive it on its own device.

use aestra_compiler::ComputeProgramRegistry;
use aestra_core::{ComputeProgramId, ResourceTypeId};
use aestra_gpu::{GpuHostBindings, check_program_block};
use aestra_runtime::{
    AESTRA_RESOURCE_FRAME, AESTRA_RESOURCE_HOST_BINDINGS, AESTRA_RESOURCE_STAGE_CONSTANTS,
    ExecutionBlock, ExecutionOp, FrameConstants, ResourceLifetime,
};
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;
use std::time::Duration;

const READBACK_TIMEOUT: Duration = Duration::from_secs(120);

/// The persistent state of a stage at a tick boundary: the bytes of every persistent resource the
/// stage owns (host-written built-ins excluded). Restoring it and replaying the same frames reproduces
/// the uninterrupted run.
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

/// One compiled plugin stage, allocated on a device and ready to run ticks.
pub struct StageExecutor {
    block: ExecutionBlock,
    buffers: Vec<wgpu::Buffer>,
    pipelines: Vec<wgpu::ComputePipeline>,
    /// Per compute op, in depth-first op order (repeat bodies once): its pipeline and bind group.
    dispatches: Vec<(usize, wgpu::BindGroup)>,
}

impl StageExecutor {
    /// Checks `block` against `programs`, allocates its resources, uploads its constants and builds a
    /// pipeline per distinct entry point. `host_binding_bytes` sizes the host-bindings resource when
    /// the block declares it (see [`GpuHostBindings::byte_len`]; the size is fixed per compiled
    /// effect).
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

    /// The block this executor runs.
    pub fn block(&self) -> &ExecutionBlock {
        &self.block
    }

    fn binding(&self, id: &str) -> Option<usize> {
        self.block
            .resources
            .iter()
            .position(|resource| resource.id.as_str() == id)
    }

    /// Runs one tick: uploads `frame` (and `host_bindings`, when given and declared), zeroes the
    /// transient resources, encodes every op in order and submits.
    pub fn run_tick(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: FrameConstants,
        host_bindings: Option<&GpuHostBindings>,
    ) -> Result<(), String> {
        if let Some(binding) = self.binding(AESTRA_RESOURCE_FRAME) {
            queue.write_buffer(
                &self.buffers[binding],
                0,
                &words_to_bytes(&frame.to_words()),
            );
        }
        if let (Some(binding), Some(host_bindings)) =
            (self.binding(AESTRA_RESOURCE_HOST_BINDINGS), host_bindings)
        {
            let buffer = &self.buffers[binding];
            if host_bindings.byte_len() > buffer.size() {
                return Err(format!(
                    "host bindings are {} bytes; the stage allocated {}",
                    host_bindings.byte_len(),
                    buffer.size()
                ));
            }
            queue.write_buffer(buffer, 0, &host_bindings.to_bytes());
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("aestra stage tick"),
        });
        for (resource, buffer) in self.block.resources.iter().zip(&self.buffers) {
            if resource.lifetime == ResourceLifetime::Transient {
                encoder.clear_buffer(buffer, 0, None);
            }
        }
        let mut cursor = 0;
        self.encode_ops(&self.block.ops, &mut encoder, &mut cursor)?;
        queue.submit([encoder.finish()]);
        Ok(())
    }

    fn encode_ops(
        &self,
        ops: &[ExecutionOp],
        encoder: &mut wgpu::CommandEncoder,
        cursor: &mut usize,
    ) -> Result<(), String> {
        for op in ops {
            match op {
                ExecutionOp::Compute(compute) => {
                    let (pipeline, bind_group) = &self.dispatches[*cursor];
                    *cursor += 1;
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: Some(&compute.name),
                        timestamp_writes: None,
                    });
                    pass.set_pipeline(&self.pipelines[*pipeline]);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.dispatch_workgroups(
                        compute.dispatch.x,
                        compute.dispatch.y,
                        compute.dispatch.z,
                    );
                }
                ExecutionOp::Barrier => {} // every compute op is already its own pass
                ExecutionOp::Copy(copy) => {
                    let from = self.buffer(copy.from.as_str())?;
                    let to = self.buffer(copy.to.as_str())?;
                    encoder.copy_buffer_to_buffer(from, 0, to, 0, from.size().min(to.size()));
                }
                ExecutionOp::Repeat { policy, body } => {
                    let start = *cursor;
                    for _ in 0..policy.count() {
                        *cursor = start;
                        self.encode_ops(body, encoder, cursor)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn buffer(&self, id: &str) -> Result<&wgpu::Buffer, String> {
        self.binding(id)
            .map(|binding| &self.buffers[binding])
            .ok_or_else(|| format!("undeclared resource '{id}'"))
    }

    /// Reads a resource back (blocking). For tests, checkpoints and debug views — not per frame.
    pub fn read_resource(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: &str,
    ) -> Result<Vec<u8>, String> {
        let source = self.buffer(id)?;
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

    /// Snapshots every persistent resource the stage owns (host-written built-ins excluded).
    pub fn checkpoint(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<StageCheckpoint, String> {
        let mut resources = BTreeMap::new();
        for resource in self.stage_owned_persistent() {
            resources.insert(
                resource.clone(),
                self.read_resource(device, queue, resource.as_str())?,
            );
        }
        Ok(StageCheckpoint { resources })
    }

    /// Restores a checkpoint taken from this stage.
    pub fn restore(&self, queue: &wgpu::Queue, checkpoint: &StageCheckpoint) -> Result<(), String> {
        for resource in self.stage_owned_persistent() {
            let bytes = checkpoint
                .resources
                .get(resource)
                .ok_or_else(|| format!("checkpoint lacks '{}'", resource.as_str()))?;
            queue.write_buffer(self.buffer(resource.as_str())?, 0, bytes);
        }
        Ok(())
    }

    fn stage_owned_persistent(&self) -> impl Iterator<Item = &ResourceTypeId> {
        self.block
            .resources
            .iter()
            .filter(|resource| {
                resource.lifetime == ResourceLifetime::Persistent
                    && !matches!(
                        resource.id.as_str(),
                        AESTRA_RESOURCE_FRAME
                            | AESTRA_RESOURCE_STAGE_CONSTANTS
                            | AESTRA_RESOURCE_HOST_BINDINGS
                    )
            })
            .map(|resource| &resource.id)
    }
}

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}
