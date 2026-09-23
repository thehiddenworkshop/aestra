//! On-GPU conformance for the generic staged simulation plan (hybrid roadmap M13), validated with the
//! 2D-diffusion workload.
//!
//! A small plan-driven executor allocates the plan's ping-pong grid resource and runs its `diffuse`
//! pass for the plan's iteration count — each iteration a *separate* compute pass (so the
//! read-after-write between iterations is a correctly-ordered barrier), swapping the front/back buffers
//! between iterations, timing each pass with GPU timestamp queries. The tests prove the M13 acceptance
//! criteria: multiple ordered passes execute deterministically and match the CPU reference
//! (`aestra_runtime::diffuse_2d`); the ping-pong state survives a checkpoint and replays to the same
//! result as an uninterrupted run; and pass-level timestamps are available.
//!
//! Like the stateful conformance suite this only runs where a compute adapter exists; set
//! `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_gpu::{STAGED_DIFFUSION_PARAM_WORDS, STAGED_DIFFUSION_WGSL};
use aestra_runtime::{
    StagedDispatch, StagedPass, StagedPlan, StagedResource, StagedResourceLifetime, diffuse_2d,
};
use std::{borrow::Cow, sync::mpsc, time::Duration};

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const GPU_SUBMISSION_TIMEOUT: Duration = Duration::from_secs(120);
const GPU_MAP_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The canonical 2D-diffusion validation plan (mirrors the CPU-side plan in `aestra_runtime::staged`):
/// one persistent ping-pong grid, one `diffuse` pass iterated `steps` times over `width`×`height` with
/// 8×8 workgroups.
fn diffusion_plan(width: u32, height: u32, steps: u32) -> StagedPlan {
    StagedPlan {
        resources: vec![StagedResource {
            name: "grid".to_string(),
            bytes: u64::from(width) * u64::from(height) * 4,
            ping_pong: true,
            lifetime: StagedResourceLifetime::Persistent,
        }],
        passes: vec![StagedPass {
            name: "diffuse".to_string(),
            entry_point: "diffuse".to_string(),
            reads: vec!["grid".to_string()],
            writes: vec!["grid".to_string()],
            dispatch: StagedDispatch {
                x: width.div_ceil(8),
                y: height.div_ceil(8),
                z: 1,
            },
            iterations: steps,
        }],
    }
}

struct StagedHarness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    timestamps_supported: bool,
}

impl StagedHarness {
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
        let required_features = if timestamps_supported {
            wgpu::Features::TIMESTAMP_QUERY
        } else {
            wgpu::Features::empty()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Aestra staged conformance device"),
            required_features,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|error| error.to_string())?;

        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        // The staged executor's binding convention for this pass: read resources, then write
        // resources, then params. Diffusion: grid_in (read), grid_out (read-write), params (read).
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("staged diffusion bindings"),
            entries: &[storage(0, true), storage(1, false), storage(2, true)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("staged diffusion pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("staged diffusion"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(STAGED_DIFFUSION_WGSL)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("diffuse"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("diffuse"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Some(Self {
            device,
            queue,
            layout,
            pipeline,
            timestamps_supported,
        }))
    }

    /// Executes the plan's `diffuse` pass from `initial`, plan-driven: the iteration count, dispatch
    /// shape, and grid byte size all come from `plan`. Returns the final grid (front buffer) and the
    /// number of pass-level timestamp intervals recorded (when the feature is present).
    fn run_plan(
        &self,
        plan: &StagedPlan,
        initial: &[f32],
        width: u32,
        height: u32,
        rate: f32,
    ) -> Result<(Vec<f32>, Option<u32>), String> {
        let pass = &plan.passes[0];
        let iterations = pass.iterations;
        let grid = plan.resource("grid").expect("plan has a grid resource");
        let grid_bytes = grid.bytes;
        assert_eq!(
            grid_bytes as usize,
            initial.len() * 4,
            "grid sizing matches"
        );

        let make = |label: &str, contents: &[u8], extra: wgpu::BufferUsages| {
            use wgpu::util::DeviceExt;
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage: wgpu::BufferUsages::STORAGE | extra,
                })
        };
        // Ping-pong pair for the "grid" resource; front starts with the initial field.
        let mut front = make(
            "grid front",
            &f32_bytes(initial),
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let mut back = make(
            "grid back",
            &vec![0u8; grid_bytes as usize],
            wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let params = make(
            "diffusion params",
            &u32_bytes(&[width, height, rate.to_bits()]),
            wgpu::BufferUsages::COPY_DST,
        );
        assert_eq!(STAGED_DIFFUSION_PARAM_WORDS, 3);

        // Pass-level timestamps: two per iteration (begin, end).
        let query_set = self.timestamps_supported.then(|| {
            self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("staged pass timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: iterations * 2,
            })
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("staged diffusion encoder"),
            });
        for iteration in 0..iterations {
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("staged diffusion bind group"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: front.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: back.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                ],
            });
            let timestamp_writes = query_set
                .as_ref()
                .map(|set| wgpu::ComputePassTimestampWrites {
                    query_set: set,
                    beginning_of_pass_write_index: Some(iteration * 2),
                    end_of_pass_write_index: Some(iteration * 2 + 1),
                });
            {
                // Each iteration is its own compute pass, so the write of `back` this iteration is
                // ordered before the read of it (as next iteration's `front`) by a barrier.
                let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("staged diffuse pass"),
                    timestamp_writes,
                });
                compute.set_pipeline(&self.pipeline);
                compute.set_bind_group(0, &bind_group, &[]);
                compute.dispatch_workgroups(pass.dispatch.x, pass.dispatch.y, pass.dispatch.z);
            }
            // Ping-pong swap: the freshly written `back` becomes next iteration's `front`.
            std::mem::swap(&mut front, &mut back);
        }

        // Read back the front buffer (the current state after the last swap).
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staged readback"),
            size: grid_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&front, 0, &readback, 0, grid_bytes);

        // Resolve timestamps into a mappable buffer.
        let timestamp_readback = query_set.as_ref().map(|set| {
            let resolved = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staged timestamp resolve"),
                size: u64::from(iterations * 2) * 8,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            encoder.resolve_query_set(set, 0..iterations * 2, &resolved, 0);
            let mappable = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("staged timestamp readback"),
                size: u64::from(iterations * 2) * 8,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(
                &resolved,
                0,
                &mappable,
                0,
                u64::from(iterations * 2) * 8,
            );
            mappable
        });

        let grid = self.read_back_f32(encoder, &readback)?;
        let intervals = match timestamp_readback {
            Some(buffer) => Some(self.count_timestamp_intervals(&buffer)?),
            None => None,
        };
        Ok((grid, intervals))
    }

    fn map_and_read(&self, buffer: &wgpu::Buffer) -> Result<Vec<u8>, String> {
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
            .map_err(|error| format!("GPU submission did not complete: {error}"))?;
        receiver
            .recv_timeout(GPU_MAP_CALLBACK_TIMEOUT)
            .map_err(|error| format!("GPU map callback not delivered: {error}"))?
            .map_err(|error| error.to_string())?;
        let bytes = slice.get_mapped_range().to_vec();
        buffer.unmap();
        Ok(bytes)
    }

    fn read_back_f32(
        &self,
        encoder: wgpu::CommandEncoder,
        readback: &wgpu::Buffer,
    ) -> Result<Vec<f32>, String> {
        self.queue.submit([encoder.finish()]);
        let bytes = self.map_and_read(readback)?;
        let (words, _) = bytes.as_chunks::<4>();
        Ok(words.iter().map(|word| f32::from_le_bytes(*word)).collect())
    }

    /// Reads resolved timestamps (u64 pairs) and returns the number of intervals with end >= begin.
    fn count_timestamp_intervals(&self, buffer: &wgpu::Buffer) -> Result<u32, String> {
        let bytes = self.map_and_read(buffer)?;
        let (words, _) = bytes.as_chunks::<8>();
        let stamps: Vec<u64> = words.iter().map(|word| u64::from_le_bytes(*word)).collect();
        let (pairs, _) = stamps.as_chunks::<2>();
        let intervals = pairs.iter().filter(|pair| pair[1] >= pair[0]).count() as u32;
        Ok(intervals)
    }
}

fn require_harness() -> Option<StagedHarness> {
    let require_gpu = std::env::var_os(REQUIRED_GPU_ENV).is_some();
    match StagedHarness::new() {
        Ok(Some(harness)) => Some(harness),
        Ok(None) if !require_gpu => {
            eprintln!(
                "skipping staged GPU conformance: no compatible compute adapter; set \
                 {REQUIRED_GPU_ENV}=1 to require one"
            );
            None
        }
        Ok(None) => panic!("{REQUIRED_GPU_ENV}=1 but no compatible compute adapter was found"),
        Err(error) => panic!("failed to create staged GPU harness: {error}"),
    }
}

fn seed_field(width: usize, height: usize) -> Vec<f32> {
    // A deterministic non-uniform field: a hot spike plus a structured background.
    let mut field = vec![0.0_f32; width * height];
    for (index, cell) in field.iter_mut().enumerate() {
        *cell = (index % 11) as f32 * 0.5;
    }
    field[height / 2 * width + width / 2] = 120.0;
    field
}

fn assert_fields_match(cpu: &[f32], gpu: &[f32]) {
    assert_eq!(cpu.len(), gpu.len(), "grid sizes agree");
    for (index, (expected, actual)) in cpu.iter().zip(gpu.iter()).enumerate() {
        let tolerance = 1e-2 + 1e-3 * expected.abs().max(actual.abs());
        assert!(
            (expected - actual).abs() <= tolerance,
            "cell {index}: CPU={expected:.5} GPU={actual:.5}"
        );
    }
}

#[test]
fn staged_diffusion_ordered_passes_match_the_cpu_reference() {
    // Acceptance: multiple ordered passes (one diffuse pass iterated N times, each its own compute pass
    // with a barrier between) execute deterministically and reproduce the CPU reference bit-for-bit
    // (within tolerance; diffusion is damping so any residual rounding decays).
    let Some(harness) = require_harness() else {
        return;
    };
    let (width, height) = (64_u32, 64_u32);
    let steps = 40_u32;
    let rate = 0.2_f32;
    let plan = diffusion_plan(width, height, steps);
    plan.validate().expect("plan is valid");

    let initial = seed_field(width as usize, height as usize);
    let (gpu, intervals) = harness
        .run_plan(&plan, &initial, width, height, rate)
        .unwrap();

    let cpu = diffuse_2d(&initial, width as usize, height as usize, rate, steps);
    assert_fields_match(&cpu, &gpu);

    // Deterministic: a second identical run yields the same field.
    let (gpu_again, _) = harness
        .run_plan(&plan, &initial, width, height, rate)
        .unwrap();
    assert_eq!(gpu, gpu_again, "the staged run is deterministic");

    // Pass-level timestamps are available: one interval per dispatch (total_dispatches).
    if let Some(intervals) = intervals {
        assert_eq!(
            intervals,
            plan.total_dispatches(),
            "one ordered timestamp interval per pass dispatch"
        );
    }
}

#[test]
fn staged_diffusion_checkpoint_and_replay_reproduces_the_uninterrupted_run() {
    // Acceptance: the ping-pong state survives a checkpoint and can restore/replay. The front buffer is
    // the checkpoint of the ping-pong grid; snapshotting it at K steps and replaying the remaining
    // (N-K) reaches the same field as running straight through N.
    let Some(harness) = require_harness() else {
        return;
    };
    let (width, height) = (48_u32, 48_u32);
    let rate = 0.24_f32;
    let (total, checkpoint_at) = (50_u32, 30_u32);
    let initial = seed_field(width as usize, height as usize);

    let (uninterrupted, _) = harness
        .run_plan(
            &diffusion_plan(width, height, total),
            &initial,
            width,
            height,
            rate,
        )
        .unwrap();

    // Snapshot the ping-pong grid at the checkpoint tick, then replay the remainder from it.
    let (checkpoint, _) = harness
        .run_plan(
            &diffusion_plan(width, height, checkpoint_at),
            &initial,
            width,
            height,
            rate,
        )
        .unwrap();
    let (replayed, _) = harness
        .run_plan(
            &diffusion_plan(width, height, total - checkpoint_at),
            &checkpoint,
            width,
            height,
            rate,
        )
        .unwrap();

    assert_eq!(
        uninterrupted,
        replayed,
        "checkpoint at {checkpoint_at} + replay {} == uninterrupted {total}",
        total - checkpoint_at
    );
}
