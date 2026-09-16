//! Stateful GPU backend conformance (hybrid roadmap M6 + M7, first increments).
//!
//! Proves the core stateful claims on real GPU compute:
//! - **M6** — persistent per-particle state that advances *incrementally* across fixed ticks and
//!   matches the CPU reference's semi-implicit Euler integration (the same integration
//!   `aestra_runtime::StatefulSimulation` performs). The GPU keeps its state in a storage buffer
//!   across dispatches — no readback during simulation, only for the final conformance check.
//! - **M7** — a backward seek done as a GPU-resident checkpoint restore plus forward replay
//!   (snapshot and restore entirely GPU→GPU) reaches the same state as an uninterrupted forward run.
//! - **M6 spawn** — the deterministic spawn RNG (u64 splitmix emulated in WGSL, from
//!   `aestra_gpu::STATEFUL_SPAWN_RNG_WGSL`) matches the CPU reference, and the full GPU spawn+integrate
//!   loop reproduces `StatefulSimulation` across a window with no death.
//! - **M6 death/reuse allocator** — the GPU atomic free list
//!   (`aestra_gpu::STATEFUL_FREE_LIST_WGSL`) hands out distinct slots under parallel allocation, the
//!   core property that makes dead-slot recycling correct.
//!
//! Still deferred: assembling integrate + spawn + death + the free list into one loop with
//! per-particle identity (matched by spawn ordinal), and presentation extraction (state → the
//! 48-byte `GpuParticle`).
//!
//! Like the other GPU conformance tests, this **skips when no compute adapter is present**, so it
//! does not run on GPU-less CI; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require a GPU.

use aestra_gpu::{STATEFUL_FREE_LIST_WGSL, STATEFUL_SPAWN_RNG_WGSL};
use aestra_runtime::StatefulSimulation;
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use std::{borrow::Cow, sync::mpsc, time::Duration};
use wgpu::util::DeviceExt;

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const GPU_SUBMISSION_TIMEOUT: Duration = Duration::from_secs(120);
const GPU_MAP_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);
const WORKGROUP: u32 = 64;
/// Floats per particle: position xyz, velocity xyz, age, lifetime.
const STRIDE: usize = 8;
/// The canonical fixed simulation timestep, matching `StatefulSimulation::TICK_DT`.
const TICK_DT: f32 = 1.0 / 60.0;

const INTEGRATE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<f32, 4>; // gravity.xyz, dt

@compute @workgroup_size(64)
fn integrate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let count = arrayLength(&state) / 8u;
    let i = gid.x;
    if (i >= count) { return; }
    let base = i * 8u;
    let dt = params[3];
    // Semi-implicit (symplectic) Euler: velocity first, then position — identical to the CPU
    // reference's advance_tick.
    state[base + 3u] = state[base + 3u] + params[0] * dt;
    state[base + 4u] = state[base + 4u] + params[1] * dt;
    state[base + 5u] = state[base + 5u] + params[2] * dt;
    state[base + 0u] = state[base + 0u] + state[base + 3u] * dt;
    state[base + 1u] = state[base + 1u] + state[base + 4u] * dt;
    state[base + 2u] = state[base + 2u] + state[base + 5u] * dt;
    state[base + 6u] = state[base + 6u] + dt;
}
"#;

/// The CPU reference integration — exactly the persistent-state update `StatefulSimulation` applies
/// each tick (M5), in place.
fn cpu_integrate(state: &mut [f32], gravity: [f32; 3], ticks: u32) {
    let count = state.len() / STRIDE;
    for _ in 0..ticks {
        for particle in 0..count {
            let base = particle * STRIDE;
            for axis in 0..3 {
                state[base + 3 + axis] += gravity[axis] * TICK_DT;
                state[base + axis] += state[base + 3 + axis] * TICK_DT;
            }
            state[base + 6] += TICK_DT;
        }
    }
}

/// A deterministic fixed particle set: origin position, varied non-zero velocity, long lifetime.
fn seed_particles(count: usize) -> Vec<f32> {
    let mut state = Vec::with_capacity(count * STRIDE);
    for particle in 0..count {
        let velocity = [
            (particle % 7) as f32 - 3.0,
            5.0 + (particle % 3) as f32,
            (particle % 5) as f32 - 2.0,
        ];
        state.extend_from_slice(&[
            0.0,
            0.0,
            0.0, // position
            velocity[0],
            velocity[1],
            velocity[2], // velocity
            0.0,         // age
            1_000.0,     // lifetime (no death in the test window)
        ]);
    }
    state
}

const SPAWN_WGSL_ENTRY: &str = r#"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> seed: vec2<u32>;

@compute @workgroup_size(64)
fn spawn(@builtin(global_invocation_id) gid: vec3<u32>) {
    let count = arrayLength(&out) / 3u;
    let i = gid.x;
    if (i >= count) { return; }
    let dir = spawn_launch_direction(seed, vec2<u32>(i, 0u));
    out[i * 3u + 0u] = dir.x;
    out[i * 3u + 1u] = dir.y;
    out[i * 3u + 2u] = dir.z;
}
"#;

/// The combined per-tick stateful advance: integrate the already-spawned slots by one dt, then spawn
/// this tick's new slots into the persistent buffer using the shared RNG — exactly the order
/// `StatefulSimulation::advance_tick` uses. Params are a flat `array<u32>` to avoid struct alignment.
const ADVANCE_WGSL_ENTRY: &str = r#"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<u32>;

@compute @workgroup_size(64)
fn advance(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    let capacity = params[3];
    if (slot >= capacity) { return; }
    let seed = vec2<u32>(params[0], params[1]);
    let spawned_before = params[4];
    let to_spawn_end = params[5];
    let speed = bitcast<f32>(params[6]);
    let lifetime = bitcast<f32>(params[7]);
    let dt = bitcast<f32>(params[8]);
    let gravity = vec3<f32>(bitcast<f32>(params[9]), bitcast<f32>(params[10]), bitcast<f32>(params[11]));
    let base = slot * 8u;
    if (slot < spawned_before) {
        let vx = state[base + 3u] + gravity.x * dt;
        let vy = state[base + 4u] + gravity.y * dt;
        let vz = state[base + 5u] + gravity.z * dt;
        state[base + 3u] = vx;
        state[base + 4u] = vy;
        state[base + 5u] = vz;
        state[base + 0u] = state[base + 0u] + vx * dt;
        state[base + 1u] = state[base + 1u] + vy * dt;
        state[base + 2u] = state[base + 2u] + vz * dt;
        state[base + 6u] = state[base + 6u] + dt;
    } else if (slot < to_spawn_end) {
        let dir = spawn_launch_direction(seed, vec2<u32>(slot, 0u));
        state[base + 0u] = 0.0;
        state[base + 1u] = 0.0;
        state[base + 2u] = 0.0;
        state[base + 3u] = dir.x * speed;
        state[base + 4u] = dir.y * speed;
        state[base + 5u] = dir.z * speed;
        state[base + 6u] = 0.0;
        state[base + 7u] = lifetime;
    }
}
"#;

/// Pops `arrayLength(&output)` slots from the free list in parallel, one per thread, into `output` —
/// exercising the production `aestra_free_pop`. The dispatch pops no more than the free count.
const FREE_LIST_ENTRY: &str = r#"
@group(0) @binding(0) var<storage, read_write> free_list: array<u32>;
@group(0) @binding(1) var<storage, read_write> free_count: atomic<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn allocate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&output)) { return; }
    output[i] = aestra_free_pop();
}
"#;

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    spawn_pipeline: wgpu::ComputePipeline,
    advance_pipeline: wgpu::ComputePipeline,
    free_list_layout: wgpu::BindGroupLayout,
    free_list_pipeline: wgpu::ComputePipeline,
}

impl Harness {
    fn new() -> Result<Option<Self>, String> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_descriptor);
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
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
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Aestra stateful conformance device"),
            required_limits: limits,
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
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aestra stateful bindings"),
            entries: &[storage(0, false), storage(1, true)],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Aestra stateful pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra stateful integrate"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(INTEGRATE_WGSL)),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("integrate"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("integrate"),
            compilation_options: Default::default(),
            cache: None,
        });
        // The spawn kernel shares the same (rw storage, ro storage) bind layout as integrate. Its
        // WGSL is the production spawn RNG from aestra-gpu, so this conformance-checks that exact code.
        let spawn_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra stateful spawn"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!(
                "{STATEFUL_SPAWN_RNG_WGSL}{SPAWN_WGSL_ENTRY}"
            ))),
        });
        let spawn_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("spawn"),
            layout: Some(&pipeline_layout),
            module: &spawn_shader,
            entry_point: Some("spawn"),
            compilation_options: Default::default(),
            cache: None,
        });
        let advance_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra stateful advance"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!(
                "{STATEFUL_SPAWN_RNG_WGSL}{ADVANCE_WGSL_ENTRY}"
            ))),
        });
        let advance_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("advance"),
            layout: Some(&pipeline_layout),
            module: &advance_shader,
            entry_point: Some("advance"),
            compilation_options: Default::default(),
            cache: None,
        });
        // A 3-binding layout for the free-list allocator (the atomic counter is a storage buffer).
        let free_list_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aestra free-list bindings"),
            entries: &[storage(0, false), storage(1, false), storage(2, false)],
        });
        let free_list_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Aestra free-list pipeline layout"),
                bind_group_layouts: &[Some(&free_list_layout)],
                immediate_size: 0,
            });
        let free_list_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra free-list allocate"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!(
                "{STATEFUL_FREE_LIST_WGSL}{FREE_LIST_ENTRY}"
            ))),
        });
        let free_list_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("allocate"),
            layout: Some(&free_list_pipeline_layout),
            module: &free_list_shader,
            entry_point: Some("allocate"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Some(Self {
            device,
            queue,
            bind_group_layout,
            pipeline,
            spawn_pipeline,
            advance_pipeline,
            free_list_layout,
            free_list_pipeline,
        }))
    }

    /// Submits the encoder, waits, and reads the mapped staging buffer back as `u32`s.
    fn read_back_u32(
        &self,
        encoder: wgpu::CommandEncoder,
        staging: &wgpu::Buffer,
    ) -> Result<Vec<u32>, String> {
        let submission = self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(GPU_SUBMISSION_TIMEOUT),
            })
            .map_err(|error| format!("GPU submission did not complete: {error}"))?;
        receiver
            .recv_timeout(GPU_MAP_CALLBACK_TIMEOUT)
            .map_err(|error| format!("GPU readback callback not delivered: {error}"))?
            .map_err(|error| error.to_string())?;
        let bytes = slice.get_mapped_range().to_vec();
        staging.unmap();
        StorageBuffer::new(&bytes)
            .create()
            .map_err(|error| error.to_string())
    }

    /// Pops `poppers` slots in parallel from a fresh free list of `capacity` slots, returning the
    /// popped indices. `poppers <= capacity`, so the allocator never underflows.
    fn allocate_slots(&self, capacity: u32, poppers: u32) -> Result<Vec<u32>, String> {
        let free_list = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("free list"),
                contents: &encode(&(0..capacity).collect::<Vec<u32>>())?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let free_count = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("free count"),
                contents: &encode(&capacity)?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let out_bytes = encode(&vec![0u32; poppers as usize])?;
        let output = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("alloc output"),
                contents: &out_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Aestra free-list bind group"),
            layout: &self.free_list_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: free_list.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: free_count.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: output.as_entire_binding(),
                },
            ],
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("alloc readback"),
            size: out_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("alloc commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("allocate"),
                ..Default::default()
            });
            pass.set_pipeline(&self.free_list_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(poppers.div_ceil(WORKGROUP), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, out_bytes.len() as u64);
        self.read_back_u32(encoder, &staging)
    }

    /// Uploads `initial`, dispatches the integrate kernel `ticks` times on the *persistent* state
    /// buffer (one pass; WebGPU orders dispatches within a pass), then reads the final state back.
    fn integrate(
        &self,
        initial: &[f32],
        gravity: [f32; 3],
        ticks: u32,
    ) -> Result<Vec<f32>, String> {
        let count = (initial.len() / STRIDE) as u32;
        let state_bytes = encode(&initial.to_vec())?;
        let state = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("state"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: &encode(&[gravity[0], gravity[1], gravity[2], TICK_DT])?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Aestra stateful bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: state_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Aestra stateful commands"),
            });
        self.dispatch_ticks(&mut encoder, &bind_group, count, ticks);
        encoder.copy_buffer_to_buffer(&state, 0, &staging, 0, state_bytes.len() as u64);
        self.read_back(encoder, &staging)
    }

    /// Runs `ticks` integrate dispatches over `count` particles in one pass. WebGPU orders dispatches
    /// within a pass, so the persistent state accumulates across ticks.
    fn dispatch_ticks(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        count: u32,
        ticks: u32,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("Aestra stateful integration"),
            ..Default::default()
        });
        pass.set_bind_group(0, bind_group, &[]);
        pass.set_pipeline(&self.pipeline);
        for _ in 0..ticks {
            pass.dispatch_workgroups(count.div_ceil(WORKGROUP), 1, 1);
        }
    }

    /// Submits the encoder, waits, and reads the mapped staging buffer back as floats.
    fn read_back(
        &self,
        encoder: wgpu::CommandEncoder,
        staging: &wgpu::Buffer,
    ) -> Result<Vec<f32>, String> {
        let submission = self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(GPU_SUBMISSION_TIMEOUT),
            })
            .map_err(|error| format!("GPU submission did not complete: {error}"))?;
        receiver
            .recv_timeout(GPU_MAP_CALLBACK_TIMEOUT)
            .map_err(|error| format!("GPU readback callback not delivered: {error}"))?
            .map_err(|error| error.to_string())?;
        let bytes = slice.get_mapped_range().to_vec();
        staging.unmap();
        StorageBuffer::new(&bytes)
            .create()
            .map_err(|error| error.to_string())
    }

    /// Runs the GPU spawn RNG for ordinals `0..count`, returning `count * 3` floats (the launch
    /// direction per ordinal). Exercises the production `aestra_gpu::STATEFUL_SPAWN_RNG_WGSL`.
    fn spawn_directions(&self, seed: u64, count: u32) -> Result<Vec<f32>, String> {
        let out_bytes = encode(&vec![0.0_f32; count as usize * 3])?;
        let out = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("spawn out"),
                contents: &out_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let seed_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("spawn seed"),
                contents: &encode(&[seed as u32, (seed >> 32) as u32])?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Aestra spawn bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: out.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: seed_buffer.as_entire_binding(),
                },
            ],
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("spawn readback"),
            size: out_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Aestra spawn commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Aestra spawn"),
                ..Default::default()
            });
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_pipeline(&self.spawn_pipeline);
            pass.dispatch_workgroups(count.div_ceil(WORKGROUP), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&out, 0, &staging, 0, out_bytes.len() as u64);
        self.read_back(encoder, &staging)
    }

    /// Runs the full stateful spawn+integrate loop for `ticks` fixed ticks over a persistent state
    /// buffer, mirroring `StatefulSimulation::advance_tick` (integrate the already-spawned slots,
    /// then spawn this tick's slots). Returns the final packed state (`capacity * 8` floats). One
    /// dispatch per tick within a single pass, so state persists across ticks.
    #[allow(clippy::too_many_arguments)]
    fn advance_stateful(
        &self,
        gravity: [f32; 3],
        spawn_per_tick: u32,
        speed: f32,
        lifetime: f32,
        capacity: u32,
        seed: u64,
        ticks: u32,
    ) -> Result<Vec<f32>, String> {
        let state_bytes = encode(&vec![0.0_f32; capacity as usize * 8])?;
        let state = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stateful state"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("stateful readback"),
            size: state_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        // One params buffer + bind group per tick (spawned_before / to_spawn_end change each tick).
        let mut bind_groups = Vec::with_capacity(ticks as usize);
        let mut param_buffers = Vec::with_capacity(ticks as usize);
        for tick in 1..=ticks {
            let spawned_before = (tick - 1).saturating_mul(spawn_per_tick).min(capacity);
            let to_spawn_end = tick.saturating_mul(spawn_per_tick).min(capacity);
            let params: Vec<u32> = vec![
                seed as u32,
                (seed >> 32) as u32,
                spawn_per_tick,
                capacity,
                spawned_before,
                to_spawn_end,
                speed.to_bits(),
                lifetime.to_bits(),
                TICK_DT.to_bits(),
                gravity[0].to_bits(),
                gravity[1].to_bits(),
                gravity[2].to_bits(),
            ];
            let params_buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("stateful params"),
                    contents: &encode(&params)?,
                    usage: wgpu::BufferUsages::STORAGE,
                });
            bind_groups.push(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("stateful advance bind group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: state.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: params_buffer.as_entire_binding(),
                    },
                ],
            }));
            param_buffers.push(params_buffer);
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("stateful advance commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("stateful advance"),
                ..Default::default()
            });
            pass.set_pipeline(&self.advance_pipeline);
            for bind_group in &bind_groups {
                pass.set_bind_group(0, bind_group, &[]);
                pass.dispatch_workgroups(capacity.div_ceil(WORKGROUP), 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&state, 0, &staging, 0, state_bytes.len() as u64);
        let result = self.read_back(encoder, &staging);
        drop(param_buffers);
        result
    }

    /// Integrates to `checkpoint_at`, snapshots the state buffer GPU→GPU, overshoots forward to
    /// `overshoot_to`, then restores the checkpoint (a backward seek) and replays forward to
    /// `seek_target` — all in GPU commands, no readback until the final state (§4.4/§19). Proves a
    /// backward seek via a GPU-resident checkpoint reaches the uninterrupted forward state.
    fn checkpoint_restore_replay(
        &self,
        initial: &[f32],
        gravity: [f32; 3],
        checkpoint_at: u32,
        overshoot_to: u32,
        seek_target: u32,
    ) -> Result<Vec<f32>, String> {
        assert!(checkpoint_at <= seek_target && seek_target <= overshoot_to);
        let count = (initial.len() / STRIDE) as u32;
        let state_bytes = encode(&initial.to_vec())?;
        let byte_len = state_bytes.len() as u64;
        let state = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("state"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            });
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: &encode(&[gravity[0], gravity[1], gravity[2], TICK_DT])?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let checkpoint = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("checkpoint"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: byte_len,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Aestra stateful bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Aestra stateful checkpoint commands"),
            });
        self.dispatch_ticks(&mut encoder, &bind_group, count, checkpoint_at);
        encoder.copy_buffer_to_buffer(&state, 0, &checkpoint, 0, byte_len);
        self.dispatch_ticks(
            &mut encoder,
            &bind_group,
            count,
            overshoot_to - checkpoint_at,
        );
        encoder.copy_buffer_to_buffer(&checkpoint, 0, &state, 0, byte_len);
        self.dispatch_ticks(
            &mut encoder,
            &bind_group,
            count,
            seek_target - checkpoint_at,
        );
        encoder.copy_buffer_to_buffer(&state, 0, &staging, 0, byte_len);
        self.read_back(encoder, &staging)
    }
}

fn encode<T>(value: &T) -> Result<Vec<u8>, String>
where
    T: ShaderType + WriteInto,
{
    let mut bytes = Vec::new();
    StorageBuffer::new(&mut bytes)
        .write(value)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn require_harness() -> Option<Harness> {
    let require_gpu = std::env::var_os(REQUIRED_GPU_ENV).is_some();
    match Harness::new() {
        Ok(Some(harness)) => Some(harness),
        Ok(None) if !require_gpu => {
            eprintln!(
                "skipping stateful GPU conformance: no compatible compute adapter; set \
                 {REQUIRED_GPU_ENV}=1 to require one"
            );
            None
        }
        Ok(None) => panic!("{REQUIRED_GPU_ENV}=1 but no compatible compute adapter was found"),
        Err(error) => panic!("failed to create stateful GPU harness: {error}"),
    }
}

fn assert_positions_match(cpu: &[f32], gpu: &[f32]) {
    assert_eq!(cpu.len(), gpu.len());
    for particle in 0..cpu.len() / STRIDE {
        let base = particle * STRIDE;
        for axis in 0..3 {
            let (expected, actual) = (cpu[base + axis], gpu[base + axis]);
            let tolerance = 1e-3 + 1e-4 * expected.abs().max(actual.abs());
            assert!(
                (expected - actual).abs() <= tolerance,
                "particle {particle} axis {axis} diverged: CPU={expected:.6} GPU={actual:.6}"
            );
        }
    }
}

#[test]
fn gpu_persistent_state_integration_matches_the_cpu_reference() {
    let Some(harness) = require_harness() else {
        return;
    };
    let gravity = [0.0, -9.81, 0.0];
    let initial = seed_particles(300);

    for ticks in [1_u32, 5, 60, 240] {
        let gpu = harness.integrate(&initial, gravity, ticks).unwrap();
        let mut cpu = initial.clone();
        cpu_integrate(&mut cpu, gravity, ticks);
        assert_positions_match(&cpu, &gpu);
    }
}

#[test]
fn gpu_state_persists_and_advances_incrementally_between_ticks() {
    let Some(harness) = require_harness() else {
        return;
    };
    let gravity = [0.0, -9.81, 0.0];
    let initial = seed_particles(64);

    let after_1 = harness.integrate(&initial, gravity, 1).unwrap();
    let after_10 = harness.integrate(&initial, gravity, 10).unwrap();

    // State advanced away from the initial upload...
    assert_ne!(after_1, initial, "one tick advances the persistent state");
    // ...and ten incremental ticks differ from one — the buffer persisted and kept integrating
    // across dispatches rather than resetting each dispatch.
    assert_ne!(
        after_1, after_10,
        "state persists and accumulates across ticks"
    );
    // Age is exactly the number of applied ticks * dt (integer tick accounting on the GPU).
    for particle in 0..after_10.len() / STRIDE {
        let age = after_10[particle * STRIDE + 6];
        assert!(
            (age - 10.0 * TICK_DT).abs() < 1e-4,
            "age accrues one dt per tick"
        );
    }
}

#[test]
fn gpu_checkpoint_restore_and_replay_reaches_the_uninterrupted_state() {
    // Hybrid roadmap M7: a backward seek is a GPU-resident checkpoint restore plus forward replay,
    // never a reverse integration (§16). Prove the restored+replayed state equals the uninterrupted
    // forward run to the same tick — with the snapshot and restore done entirely GPU→GPU (§4.4/§19).
    let Some(harness) = require_harness() else {
        return;
    };
    let gravity = [0.0, -9.81, 0.0];
    let initial = seed_particles(128);

    let uninterrupted = harness.integrate(&initial, gravity, 90).unwrap();
    // Checkpoint at tick 30, run the playhead forward to 150, then seek back: restore the checkpoint
    // (<= target) and replay forward to tick 90.
    let restored = harness
        .checkpoint_restore_replay(&initial, gravity, 30, 150, 90)
        .unwrap();

    assert_eq!(uninterrupted.len(), restored.len());
    // The full persistent state matches — position, velocity, and age — not just positions.
    for (index, (expected, actual)) in uninterrupted.iter().zip(&restored).enumerate() {
        let tolerance = 1e-3 + 1e-4 * expected.abs().max(actual.abs());
        assert!(
            (expected - actual).abs() <= tolerance,
            "state float {index} diverged after checkpoint/replay: uninterrupted={expected:.6} \
             restored={actual:.6}"
        );
    }
    // Sanity: the seek actually moved backward from the overshoot (state at 90 != state at 150).
    let overshot = harness.integrate(&initial, gravity, 150).unwrap();
    assert_ne!(
        restored, overshot,
        "seeking back to 90 must not leave the state at the overshoot tick 150"
    );
}

#[test]
fn gpu_spawn_rng_matches_the_cpu_reference() {
    // Hybrid roadmap M6, the harder half: GPU spawn needs the same deterministic splitmix64 RNG as
    // the CPU reference, but WGSL has no native u64 — it is emulated with u32 pairs
    // (aestra_gpu::STATEFUL_SPAWN_RNG_WGSL). Prove the emulation is correct: for many seeds and
    // ordinals, the GPU launch direction matches StatefulSimulation::launch_direction. Any error in
    // the u64 math produces a wildly different hash, so a tight tolerance is a strong check.
    let Some(harness) = require_harness() else {
        return;
    };
    let count = 1024_u32;
    for &seed in &[
        0_u64,
        1,
        42,
        0x1234_5678_9abc_def0,
        0xDEAD_BEEF_CAFE_F00D,
        u64::MAX,
    ] {
        let gpu = harness.spawn_directions(seed, count).unwrap();
        for ordinal in 0..count as u64 {
            let cpu = StatefulSimulation::launch_direction(seed, ordinal);
            for axis in 0..3 {
                let actual = gpu[ordinal as usize * 3 + axis];
                assert!(
                    (actual - cpu[axis]).abs() <= 1e-6,
                    "seed {seed:#018x} ordinal {ordinal} axis {axis}: CPU={:.7} GPU={:.7}",
                    cpu[axis],
                    actual
                );
            }
        }
    }
}

#[test]
fn gpu_spawn_and_integrate_matches_the_cpu_reference() {
    // Hybrid roadmap M6: the GPU spawn+integrate loop must reproduce the M5 CPU reference. Run the
    // full per-tick loop on the GPU (spawn new slots with the shared RNG, integrate the rest) and
    // compare the presented positions to StatefulSimulation. A long lifetime keeps every particle
    // alive across the window, so no death/free-list is exercised yet (that is the next step).
    use aestra_runtime::StatefulConfig;
    let Some(harness) = require_harness() else {
        return;
    };
    let config = StatefulConfig {
        gravity: [0.0, -9.81, 0.0],
        spawn_per_tick: 4,
        initial_speed: 12.0,
        lifetime: 1000.0,
        capacity: 512,
    };
    let ticks = 100_u32;
    let seed = 0x1234_5678_9abc_def0_u64;

    let gpu = harness
        .advance_stateful(
            config.gravity,
            config.spawn_per_tick,
            config.initial_speed,
            config.lifetime,
            config.capacity,
            seed,
            ticks,
        )
        .unwrap();

    let mut simulation = StatefulSimulation::new(config, seed);
    simulation.advance_to_tick(ticks as u64);
    let mut cpu = Vec::new();
    simulation.present(&mut cpu);

    assert_eq!(cpu.len(), simulation.live_count());
    assert!(
        !cpu.is_empty(),
        "particles are alive at the end of the window"
    );
    // With no death, CPU particle `i` (spawn order) is GPU slot `i`.
    for (index, sample) in cpu.iter().enumerate() {
        let base = index * 8;
        for axis in 0..3 {
            let expected = sample.position[axis];
            let actual = gpu[base + axis];
            let tolerance = 1e-3 + 1e-4 * expected.abs().max(actual.abs());
            assert!(
                (expected - actual).abs() <= tolerance,
                "particle {index} axis {axis}: CPU={expected:.5} GPU={actual:.5}"
            );
        }
    }
}

#[test]
fn gpu_free_list_allocator_yields_distinct_slots() {
    // Hybrid roadmap M6 death/reuse: dead slots are recycled through a GPU atomic free list. The
    // hard property is that parallel allocation never hands the same slot to two spawns. Prove it —
    // pop many slots in parallel and check they are all distinct, valid indices. Which slot a spawn
    // gets does not matter (particles are matched by spawn ordinal), only distinctness.
    let Some(harness) = require_harness() else {
        return;
    };
    let capacity = 4096_u32;
    for &poppers in &[1_u32, 64, 1000, capacity] {
        let mut slots = harness.allocate_slots(capacity, poppers).unwrap();
        assert_eq!(slots.len(), poppers as usize);
        assert!(
            slots.iter().all(|&slot| slot < capacity),
            "every popped slot is a valid index"
        );
        slots.sort_unstable();
        slots.dedup();
        assert_eq!(
            slots.len(),
            poppers as usize,
            "parallel allocation of {poppers} slots yields distinct slots (no double-allocation)"
        );
    }
}
