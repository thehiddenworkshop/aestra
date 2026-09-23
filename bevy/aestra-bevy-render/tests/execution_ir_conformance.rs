//! On-GPU execution of the generic Execution IR (extensible-stages redesign M7).
//!
//! A small backend consumes an `aestra_runtime::ExecutionBlock` (the M6 IR): it allocates the declared
//! resources as GPU buffers, builds a compute pipeline per distinct op entry point, and walks the ops
//! in order — each `Compute` is a dispatch, each iteration is its own compute pass (so the ordering is
//! a real barrier), `Repeat` loops its body, `Copy` copies buffers — timing every pass with GPU
//! timestamp queries. This proves a synthetic multi-pass stage runs on the native backend with
//! **deterministic ordering** (an order- and repeat-dependent result), **GPU validation** (the block
//! validates and the kernels compile), **capture** (one timestamp interval per dispatch), and
//! **bounded resource allocation** — no real fluid solver required.
//!
//! Runs only where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_runtime::{
    ComputeOp, ExecutionBlock, ExecutionOp, RepeatPolicy, ResourceAccess, ResourceDescriptor,
    ResourceLifetime, StagedDispatch,
};
use std::{borrow::Cow, collections::HashMap, sync::mpsc, time::Duration};

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const GPU_SUBMISSION_TIMEOUT: Duration = Duration::from_secs(120);
const GPU_MAP_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Three tiny order-dependent kernels over one `value` buffer: set to 1, double, add 100. Composed by
/// the block, the final value is order- and repeat-sensitive, so a wrong ordering or repeat count would
/// change the result.
const OPS_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> value: array<f32>;

@compute @workgroup_size(1) fn set_one(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x == 0u) { value[0] = 1.0; }
}
@compute @workgroup_size(1) fn double_it(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x == 0u) { value[0] = value[0] * 2.0; }
}
@compute @workgroup_size(1) fn add_hundred(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x == 0u) { value[0] = value[0] + 100.0; }
}
"#;

const VALUE_RESOURCE: &str = "test.resource/value";

struct IrHarness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipelines: HashMap<String, wgpu::ComputePipeline>,
    timestamps_supported: bool,
}

impl IrHarness {
    fn new() -> Result<Option<Self>, String> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_descriptor);
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })) {
                Ok(adapter) => adapter,
                Err(_) => return Ok(None),
            };
        if !adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        {
            return Ok(None);
        }
        let timestamps_supported = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Aestra execution-IR device"),
            required_features: if timestamps_supported {
                wgpu::Features::TIMESTAMP_QUERY
            } else {
                wgpu::Features::empty()
            },
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|error| error.to_string())?;

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ir value binding"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ir pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ir ops"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(OPS_WGSL)),
        });
        let mut pipelines = HashMap::new();
        for entry in ["set_one", "double_it", "add_hundred"] {
            pipelines.insert(
                entry.to_string(),
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                }),
            );
        }
        Ok(Some(Self {
            device,
            queue,
            layout,
            pipelines,
            timestamps_supported,
        }))
    }

    /// Runs a validated block on the GPU: allocates a buffer per declared resource, walks the ops in
    /// order (repeats expanded, each dispatch its own compute pass), and reads back the `value`
    /// resource. Returns the value and, when supported, the number of timestamp intervals captured.
    fn run(&self, block: &ExecutionBlock) -> Result<(f32, Option<u32>), String> {
        use wgpu::util::DeviceExt;
        let mut buffers: HashMap<String, wgpu::Buffer> = HashMap::new();
        for resource in &block.resources {
            let bytes = resource.bytes.max(4);
            buffers.insert(
                resource.id.as_str().to_string(),
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(resource.id.as_str()),
                        contents: &vec![0u8; bytes as usize],
                        usage: wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_SRC
                            | wgpu::BufferUsages::COPY_DST,
                    }),
            );
        }

        let dispatches = block.compute_pass_count();
        let query_set = self.timestamps_supported.then(|| {
            self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("ir timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: (dispatches * 2).max(2),
            })
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ir") });
        let mut pass_index = 0u32;
        self.run_ops(
            &block.ops,
            &buffers,
            &query_set,
            &mut encoder,
            &mut pass_index,
        )?;

        // Read back the value resource.
        let value_buffer = buffers
            .get(VALUE_RESOURCE)
            .ok_or("block does not declare the value resource")?;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ir readback"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(value_buffer, 0, &readback, 0, 4);

        let timestamp_readback = query_set.as_ref().map(|set| {
            let size = u64::from((dispatches * 2).max(2)) * 8;
            let resolved = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ir ts resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            encoder.resolve_query_set(set, 0..dispatches * 2, &resolved, 0);
            let mappable = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ir ts readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&resolved, 0, &mappable, 0, size);
            mappable
        });

        self.queue.submit([encoder.finish()]);
        let bytes = self.map_and_read(&readback);
        let value = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let intervals = timestamp_readback.map(|buffer| {
            let bytes = self.map_and_read(&buffer);
            let (stamps, _) = bytes.as_chunks::<8>();
            let (pairs, _) = stamps.as_chunks::<2>();
            pairs
                .iter()
                .filter(|pair| u64::from_le_bytes(pair[1]) >= u64::from_le_bytes(pair[0]))
                .count() as u32
        });
        Ok((value, intervals))
    }

    fn run_ops(
        &self,
        ops: &[ExecutionOp],
        buffers: &HashMap<String, wgpu::Buffer>,
        query_set: &Option<wgpu::QuerySet>,
        encoder: &mut wgpu::CommandEncoder,
        pass_index: &mut u32,
    ) -> Result<(), String> {
        for op in ops {
            match op {
                ExecutionOp::Compute(compute) => {
                    self.run_compute(compute, buffers, query_set, encoder, pass_index)?;
                }
                ExecutionOp::Barrier => {} // separate compute passes already order-barrier the buffer
                ExecutionOp::Copy(copy) => {
                    let from = buffers.get(copy.from.as_str()).ok_or("copy from missing")?;
                    let to = buffers.get(copy.to.as_str()).ok_or("copy to missing")?;
                    let size = from.size().min(to.size());
                    encoder.copy_buffer_to_buffer(from, 0, to, 0, size);
                }
                ExecutionOp::Repeat { policy, body } => {
                    for _ in 0..policy.count() {
                        self.run_ops(body, buffers, query_set, encoder, pass_index)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn run_compute(
        &self,
        compute: &ComputeOp,
        buffers: &HashMap<String, wgpu::Buffer>,
        query_set: &Option<wgpu::QuerySet>,
        encoder: &mut wgpu::CommandEncoder,
        pass_index: &mut u32,
    ) -> Result<(), String> {
        let pipeline = self
            .pipelines
            .get(&compute.entry_point)
            .ok_or_else(|| format!("no pipeline for entry '{}'", compute.entry_point))?;
        // Bind the op's accessed resources in order (binding 0..N).
        let mut entries = Vec::new();
        for (binding, access) in compute.accesses.iter().enumerate() {
            let buffer = buffers
                .get(access.resource.as_str())
                .ok_or("compute accesses an undeclared resource")?;
            entries.push(wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            });
        }
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&compute.name),
            layout: &self.layout,
            entries: &entries,
        });
        let timestamp_writes = query_set
            .as_ref()
            .map(|set| wgpu::ComputePassTimestampWrites {
                query_set: set,
                beginning_of_pass_write_index: Some(*pass_index * 2),
                end_of_pass_write_index: Some(*pass_index * 2 + 1),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some(&compute.name),
                timestamp_writes,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(compute.dispatch.x, compute.dispatch.y, compute.dispatch.z);
        }
        *pass_index += 1;
        Ok(())
    }

    fn map_and_read(&self, buffer: &wgpu::Buffer) -> Vec<u8> {
        let slice = buffer.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(GPU_SUBMISSION_TIMEOUT),
            })
            .expect("GPU submission completes");
        receiver
            .recv_timeout(GPU_MAP_CALLBACK_TIMEOUT)
            .expect("map callback delivered")
            .expect("map succeeds");
        let bytes = slice.get_mapped_range().to_vec();
        buffer.unmap();
        bytes
    }
}

fn require_harness() -> Option<IrHarness> {
    let require_gpu = std::env::var_os(REQUIRED_GPU_ENV).is_some();
    match IrHarness::new() {
        Ok(Some(harness)) => Some(harness),
        Ok(None) if !require_gpu => {
            eprintln!(
                "skipping execution-IR GPU conformance: no compatible compute adapter; set \
                 {REQUIRED_GPU_ENV}=1 to require one"
            );
            None
        }
        Ok(None) => panic!("{REQUIRED_GPU_ENV}=1 but no compatible compute adapter was found"),
        Err(error) => panic!("failed to create execution-IR harness: {error}"),
    }
}

fn one_dispatch(entry: &str) -> ExecutionOp {
    ExecutionOp::Compute(ComputeOp {
        name: entry.to_string(),
        entry_point: entry.to_string(),
        accesses: vec![ResourceAccess::read_write(VALUE_RESOURCE)],
        dispatch: StagedDispatch { x: 1, y: 1, z: 1 },
    })
}

/// The synthetic multi-pass stage: set 1, barrier, double x4, add 100.
fn synthetic_block() -> ExecutionBlock {
    ExecutionBlock {
        resources: vec![ResourceDescriptor {
            id: aestra_core::ResourceTypeId::new(VALUE_RESOURCE),
            bytes: 4,
            lifetime: ResourceLifetime::Persistent,
        }],
        ops: vec![
            one_dispatch("set_one"),
            ExecutionOp::Barrier,
            ExecutionOp::Repeat {
                policy: RepeatPolicy::FixedCount(4),
                body: vec![one_dispatch("double_it")],
            },
            one_dispatch("add_hundred"),
        ],
    }
}

#[test]
fn a_synthetic_multi_pass_stage_executes_on_gpu_in_deterministic_order() {
    let Some(harness) = require_harness() else {
        return;
    };
    let block = synthetic_block();
    // GPU validation: the block is well-formed before the backend runs it.
    block.validate().expect("the block validates");
    assert_eq!(
        block.compute_pass_count(),
        6,
        "set + 4x double + add = 6 dispatches"
    );

    let (value, intervals) = harness.run(&block).unwrap();
    // Order- and repeat-dependent: 1 -> (x2)^4 = 16 -> +100 = 116. A wrong order or repeat count would
    // not produce 116.
    assert_eq!(value, 116.0, "the ordered, repeated multi-pass result");

    // Deterministic: a second run yields the same value.
    let (again, _) = harness.run(&block).unwrap();
    assert_eq!(value, again, "GPU execution of the block is deterministic");

    // Capture: one timestamp interval per dispatch (when supported).
    if let Some(intervals) = intervals {
        assert_eq!(
            intervals,
            block.compute_pass_count(),
            "one timestamp per dispatch"
        );
    }
}
