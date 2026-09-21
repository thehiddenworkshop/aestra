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
//! - **M6 assembled death loop** — integrate + death + spawn + free-list reuse in one loop, with
//!   per-particle identity (the spawn ordinal) written into state so live GPU slots can be matched
//!   to the CPU reference by ordinal even though the parallel slot assignment differs. Over a window
//!   with real death and slot reuse, every live GPU particle matches `StatefulSimulation`.
//!
//! Still deferred: presentation extraction (state → the 48-byte `GpuParticle`).
//!
//! Like the other GPU conformance tests, this **skips when no compute adapter is present**, so it
//! does not run on GPU-less CI; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require a GPU.

use aestra_gpu::{
    STATEFUL_FREE_LIST_WGSL, STATEFUL_PRESENT_WGSL, STATEFUL_SPAWN_RNG_WGSL,
    stateful_simulation_wgsl,
};
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

/// The full stateful loop with death and slot reuse. Two per-tick phases sharing five bindings:
/// `death_integrate` integrates each live slot by one dt and frees it (pushes to the free list) if it
/// died; `spawn` claims a free slot and a fresh ordinal for each of this tick's new particles. The
/// state stride is 9 floats: position, velocity, age, lifetime, and the spawn ordinal (identity).
/// `params` = [capacity, spawn_per_tick, seed_lo, seed_hi, speed, lifetime, dt, gx, gy, gz].
const DEATH_LOOP_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read_write> free_list: array<u32>;
@group(0) @binding(2) var<storage, read_write> free_count: atomic<u32>;
@group(0) @binding(3) var<storage, read_write> spawn_counter: atomic<u32>;
@group(0) @binding(4) var<storage, read> params: array<u32>;

@compute @workgroup_size(64)
fn death_integrate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= params[0]) { return; }
    let base = slot * 9u;
    let lifetime = state[base + 7u];
    let age = state[base + 6u];
    if (lifetime > 0.0 && age < lifetime) {
        let dt = bitcast<f32>(params[6]);
        let gx = bitcast<f32>(params[7]);
        let gy = bitcast<f32>(params[8]);
        let gz = bitcast<f32>(params[9]);
        let vx = state[base + 3u] + gx * dt;
        let vy = state[base + 4u] + gy * dt;
        let vz = state[base + 5u] + gz * dt;
        state[base + 3u] = vx;
        state[base + 4u] = vy;
        state[base + 5u] = vz;
        state[base + 0u] = state[base + 0u] + vx * dt;
        state[base + 1u] = state[base + 1u] + vy * dt;
        state[base + 2u] = state[base + 2u] + vz * dt;
        let new_age = age + dt;
        state[base + 6u] = new_age;
        if (new_age >= lifetime) {
            state[base + 7u] = 0.0; // mark the slot free
            aestra_free_push(slot);
        }
    }
}

@compute @workgroup_size(64)
fn spawn(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params[1]) { return; }
    // Claim a free slot; if the free list is empty this tick, this spawn does not happen (matching
    // the CPU reference's room bound).
    let top = atomicSub(&free_count, 1u);
    if (top == 0u || top > params[0]) {
        atomicAdd(&free_count, 1u);
        return;
    }
    let slot = free_list[top - 1u];
    let ordinal = atomicAdd(&spawn_counter, 1u);
    let seed = vec2<u32>(params[2], params[3]);
    let speed = bitcast<f32>(params[4]);
    let lifetime = bitcast<f32>(params[5]);
    let dir = spawn_launch_direction(seed, vec2<u32>(ordinal, 0u));
    let base = slot * 9u;
    state[base + 0u] = 0.0;
    state[base + 1u] = 0.0;
    state[base + 2u] = 0.0;
    state[base + 3u] = dir.x * speed;
    state[base + 4u] = dir.y * speed;
    state[base + 5u] = dir.z * speed;
    state[base + 6u] = 0.0;
    state[base + 7u] = lifetime;
    state[base + 8u] = bitcast<f32>(ordinal);
}
"#;

/// Presentation extraction: map each stride-9 persistent state slot to a 12-word GpuParticle record,
/// exercising the production `aestra_gpu::STATEFUL_PRESENT_WGSL`. Two bindings: state (read) and the
/// presentation output (read-write).
const PRESENT_ENTRY: &str = r#"
@group(0) @binding(0) var<storage, read> state: array<f32>;
@group(0) @binding(1) var<storage, read_write> present_out: array<f32>;

@compute @workgroup_size(64)
fn present(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= arrayLength(&state) / AESTRA_STATE_STRIDE) { return; }
    aestra_present_stateful(slot, 0u);
}
"#;

/// The compaction outputs `present` produces: `(alive_indices, indirect, counters)`.
type CompactionOutputs = (Vec<u32>, Vec<u32>, Vec<u32>);

/// Words per presentation record — the 48-byte `GpuParticle` ABI.
const PRESENT_STRIDE: usize = 12;
/// Persistent state slot stride for the death loop and presentation extraction (adds the ordinal).
const DEATH_STRIDE: usize = 9;

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    spawn_pipeline: wgpu::ComputePipeline,
    advance_pipeline: wgpu::ComputePipeline,
    free_list_layout: wgpu::BindGroupLayout,
    free_list_pipeline: wgpu::ComputePipeline,
    death_layout: wgpu::BindGroupLayout,
    death_integrate_pipeline: wgpu::ComputePipeline,
    death_spawn_pipeline: wgpu::ComputePipeline,
    present_layout: wgpu::BindGroupLayout,
    present_pipeline: wgpu::ComputePipeline,
    unified_layout: wgpu::BindGroupLayout,
    unified_present_pipeline: wgpu::ComputePipeline,
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
        // The full death loop shares five bindings across its two phases: state (rw), free list (rw),
        // free count (atomic rw), spawn counter (atomic rw), params (ro). Its WGSL is assembled from
        // the two production primitives (the spawn RNG and the free-list allocator) plus the two
        // kernels, so this conformance-checks that exact reusable code.
        let death_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aestra death-loop bindings"),
            entries: &[
                storage(0, false),
                storage(1, false),
                storage(2, false),
                storage(3, false),
                storage(4, true),
            ],
        });
        let death_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Aestra death-loop pipeline layout"),
                bind_group_layouts: &[Some(&death_layout)],
                immediate_size: 0,
            });
        let death_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra death loop"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!(
                "{STATEFUL_SPAWN_RNG_WGSL}{STATEFUL_FREE_LIST_WGSL}{DEATH_LOOP_WGSL}"
            ))),
        });
        let death_integrate_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("death_integrate"),
                layout: Some(&death_pipeline_layout),
                module: &death_shader,
                entry_point: Some("death_integrate"),
                compilation_options: Default::default(),
                cache: None,
            });
        let death_spawn_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("death_spawn"),
                layout: Some(&death_pipeline_layout),
                module: &death_shader,
                entry_point: Some("spawn"),
                compilation_options: Default::default(),
                cache: None,
            });
        // Presentation extraction: state (read) -> GpuParticle records (read-write). Its WGSL is the
        // production STATEFUL_PRESENT_WGSL plus the entry point.
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aestra present bindings"),
            entries: &[storage(0, true), storage(1, false)],
        });
        let present_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Aestra present pipeline layout"),
                bind_group_layouts: &[Some(&present_layout)],
                immediate_size: 0,
            });
        let present_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra present"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(format!(
                "{STATEFUL_PRESENT_WGSL}{PRESENT_ENTRY}"
            ))),
        });
        let present_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("present"),
            layout: Some(&present_pipeline_layout),
            module: &present_shader,
            entry_point: Some("present"),
            compilation_options: Default::default(),
            cache: None,
        });
        // The production unified module (9 bindings): here we drive its `present` entry, which both
        // extracts presentation and compacts the live slots into alive_indices/indirect/counters.
        let unified_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aestra unified stateful bindings"),
            entries: &[
                storage(0, false), // state
                storage(1, false), // free_list
                storage(2, false), // free_count
                storage(3, false), // spawn_counter
                storage(4, true),  // params
                storage(5, false), // present_out
                storage(6, false), // alive_indices
                storage(7, false), // indirect
                storage(8, false), // counters
            ],
        });
        let unified_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Aestra unified stateful pipeline layout"),
                bind_group_layouts: &[Some(&unified_layout)],
                immediate_size: 0,
            });
        let unified_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra unified stateful"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(stateful_simulation_wgsl())),
        });
        let unified_present_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("unified present"),
                layout: Some(&unified_pipeline_layout),
                module: &unified_shader,
                entry_point: Some("present"),
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
            death_layout,
            death_integrate_pipeline,
            death_spawn_pipeline,
            present_layout,
            present_pipeline,
            unified_layout,
            unified_present_pipeline,
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

    /// Runs the full death loop — integrate + death + spawn + free-list reuse with per-particle
    /// identity — for `ticks` fixed ticks, then reads back every live slot as `(spawn ordinal,
    /// position)`. Each tick runs two dispatches in one pass: `death_integrate` over all `capacity`
    /// slots (advance the live ones, free the ones that died this tick) then `spawn` over
    /// `spawn_per_tick` threads (each claims a freed slot and a fresh ordinal). State stride is 9
    /// floats (position, velocity, age, lifetime, ordinal-as-bits). Slots are matched to the CPU
    /// reference by ordinal, so the arbitrary parallel slot assignment need not agree.
    #[allow(clippy::too_many_arguments)]
    fn advance_stateful_with_death(
        &self,
        gravity: [f32; 3],
        spawn_per_tick: u32,
        speed: f32,
        lifetime: f32,
        capacity: u32,
        seed: u64,
        ticks: u32,
    ) -> Result<Vec<(u64, [f32; 3])>, String> {
        let state_bytes = encode(&vec![0.0_f32; capacity as usize * 9])?;
        let state = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("death state"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        // Every slot starts free.
        let free_list = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("death free list"),
                contents: &encode(&(0..capacity).collect::<Vec<u32>>())?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let free_count = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("death free count"),
                contents: &encode(&capacity)?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let spawn_counter = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("death spawn counter"),
                contents: &encode(&0_u32)?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        // Params are constant across every tick.
        let params: Vec<u32> = vec![
            capacity,
            spawn_per_tick,
            seed as u32,
            (seed >> 32) as u32,
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
                label: Some("death params"),
                contents: &encode(&params)?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("death bind group"),
            layout: &self.death_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: free_list.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: free_count.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: spawn_counter.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: params_buffer.as_entire_binding(),
                },
            ],
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("death readback"),
            size: state_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("death commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("death loop"),
                ..Default::default()
            });
            pass.set_bind_group(0, &bind_group, &[]);
            for _ in 0..ticks {
                pass.set_pipeline(&self.death_integrate_pipeline);
                pass.dispatch_workgroups(capacity.div_ceil(WORKGROUP), 1, 1);
                pass.set_pipeline(&self.death_spawn_pipeline);
                pass.dispatch_workgroups(spawn_per_tick.div_ceil(WORKGROUP), 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&state, 0, &staging, 0, state_bytes.len() as u64);
        // Read the raw words so the ordinal (stored as bits) comes back exactly.
        let raw = self.read_back_u32(encoder, &staging)?;
        let mut alive = Vec::new();
        for slot in 0..capacity as usize {
            let base = slot * 9;
            let lifetime = f32::from_bits(raw[base + 7]);
            let age = f32::from_bits(raw[base + 6]);
            if lifetime > 0.0 && age < lifetime {
                let position = [
                    f32::from_bits(raw[base]),
                    f32::from_bits(raw[base + 1]),
                    f32::from_bits(raw[base + 2]),
                ];
                alive.push((raw[base + 8] as u64, position));
            }
        }
        Ok(alive)
    }

    /// Runs presentation extraction over an uploaded stride-9 state buffer, returning the raw
    /// `capacity * 12` presentation words (the `GpuParticle` ABI). Floats come back as bits so
    /// `packed_emitter_alive` and `particle_index` are recovered exactly.
    fn extract_presentation(&self, state_words: &[f32]) -> Result<Vec<u32>, String> {
        let count = state_words.len() / DEATH_STRIDE;
        let state_bytes = encode(&state_words.to_vec())?;
        let state = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("present state"),
                contents: &state_bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let out_bytes = encode(&vec![0.0_f32; count * PRESENT_STRIDE])?;
        let present_out = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("present out"),
                contents: &out_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present bind group"),
            layout: &self.present_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: present_out.as_entire_binding(),
                },
            ],
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("present readback"),
            size: out_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("present"),
                ..Default::default()
            });
            pass.set_pipeline(&self.present_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups((count as u32).div_ceil(WORKGROUP), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&present_out, 0, &staging, 0, out_bytes.len() as u64);
        self.read_back_u32(encoder, &staging)
    }

    /// Runs the production unified module's `present` entry over an uploaded stride-9 state buffer and
    /// reads back the compaction outputs it produces alongside presentation: `(alive_indices, indirect,
    /// counters)`. `alive_indices` has `slot_offset + capacity` words, `indirect` has `(emitter+1)*4`
    /// words (the per-emitter draw commands), and `counters` has two. Proves the stateful path compacts
    /// its live slots into the same buffers the analytic path feeds the renderer.
    fn present_compact(
        &self,
        state_words: &[f32],
        emitter_index: u32,
        slot_offset: u32,
    ) -> Result<CompactionOutputs, String> {
        let capacity = (state_words.len() / DEATH_STRIDE) as u32;
        let buffer = |label, contents: &[u8], copy_src| {
            let mut usage = wgpu::BufferUsages::STORAGE;
            if copy_src {
                usage |= wgpu::BufferUsages::COPY_SRC;
            }
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage,
                })
        };
        let state = buffer(
            "present-compact state",
            &encode(&state_words.to_vec())?,
            false,
        );
        let free_list = buffer("dummy free list", &encode(&[0_u32])?, false);
        let free_count = buffer("dummy free count", &encode(&0_u32)?, false);
        let spawn_counter = buffer("dummy spawn counter", &encode(&0_u32)?, false);
        let params: Vec<u32> = vec![
            capacity,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            emitter_index,
            slot_offset,
        ];
        let params_buffer = buffer("present-compact params", &encode(&params)?, false);
        let present_out = buffer(
            "present-compact out",
            &encode(&vec![0.0_f32; capacity as usize * PRESENT_STRIDE])?,
            false,
        );
        let alive_len = (slot_offset + capacity) as usize;
        let alive_indices = buffer(
            "present-compact alive",
            &encode(&vec![0_u32; alive_len])?,
            true,
        );
        let indirect_len = (emitter_index as usize + 1) * 4;
        let indirect = buffer(
            "present-compact indirect",
            &encode(&vec![0_u32; indirect_len])?,
            true,
        );
        let counters = buffer("present-compact counters", &encode(&vec![0_u32; 2])?, true);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present-compact bind group"),
            layout: &self.unified_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: free_list.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: free_count.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: spawn_counter.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: present_out.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: alive_indices.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: indirect.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: counters.as_entire_binding(),
                },
            ],
        });
        // One staging buffer holds indirect ++ counters ++ alive_indices, read back together.
        let indirect_bytes = (indirect_len * 4) as u64;
        let counters_bytes = 2 * 4;
        let alive_bytes = (alive_len * 4) as u64;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("present-compact readback"),
            size: indirect_bytes + counters_bytes + alive_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("present-compact commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("present-compact"),
                ..Default::default()
            });
            pass.set_pipeline(&self.unified_present_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(capacity.div_ceil(WORKGROUP), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&indirect, 0, &staging, 0, indirect_bytes);
        encoder.copy_buffer_to_buffer(&counters, 0, &staging, indirect_bytes, counters_bytes);
        encoder.copy_buffer_to_buffer(
            &alive_indices,
            0,
            &staging,
            indirect_bytes + counters_bytes,
            alive_bytes,
        );
        let all = self.read_back_u32(encoder, &staging)?;
        let (indirect_out, rest) = all.split_at(indirect_len);
        let (counters_out, alive_out) = rest.split_at(2);
        Ok((
            alive_out.to_vec(),
            indirect_out.to_vec(),
            counters_out.to_vec(),
        ))
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

/// One stride-9 persistent state slot: position, velocity, age, lifetime, and the spawn ordinal
/// stored as bits (subnormal for small ordinals, which is what production stores).
fn state_slot(
    position: [f32; 3],
    velocity: [f32; 3],
    age: f32,
    lifetime: f32,
    ordinal: u32,
) -> [f32; DEATH_STRIDE] {
    [
        position[0],
        position[1],
        position[2],
        velocity[0],
        velocity[1],
        velocity[2],
        age,
        lifetime,
        f32::from_bits(ordinal),
    ]
}

/// The reference presentation mapping, as raw `GpuParticle` words — identical to what
/// `aestra_gpu::STATEFUL_PRESENT_WGSL` (and `StatefulSimulation::present`) must produce for one slot:
/// white color, state position, unit size, zero rotation, clamped normalized age, packed
/// emitter/alive, and the spawn ordinal in `particle_index`.
fn cpu_present_record(slot: &[f32]) -> [u32; PRESENT_STRIDE] {
    let age = slot[6];
    let lifetime = slot[7];
    let alive = lifetime > 0.0 && age < lifetime;
    let normalized_age = if lifetime > 0.0 {
        (age / lifetime).clamp(0.0, 1.0)
    } else {
        0.0
    };
    [
        1.0_f32.to_bits(),
        1.0_f32.to_bits(),
        1.0_f32.to_bits(),
        1.0_f32.to_bits(),
        slot[0].to_bits(),
        slot[1].to_bits(),
        slot[2].to_bits(),
        1.0_f32.to_bits(),
        0.0_f32.to_bits(),
        normalized_age.to_bits(),
        alive as u32, // packed_emitter_alive; emitter 0 << 16 | alive
        slot[8].to_bits(),
    ]
}

#[test]
fn gpu_presentation_extraction_matches_the_reference_abi() {
    // Hybrid roadmap M6 presentation extraction: the stateful path must emit the same 48-byte
    // GpuParticle records the analytic path does, so both feed one alive/compaction/render pipeline.
    // Prove aestra_gpu::STATEFUL_PRESENT_WGSL maps persistent state to that ABI exactly, for a mix of
    // alive particles (varied age), a particle exactly at its lifetime (dead), one past it (dead), and
    // a free slot (lifetime 0) — matching the reference rule word for word, including the packed
    // alive flag and the spawn ordinal carried in particle_index.
    let Some(harness) = require_harness() else {
        return;
    };
    let slots = [
        state_slot([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], 0.0, 2.0, 0), // fresh, age 0
        state_slot([1.5, -4.0, 2.0], [0.0, -9.0, 0.0], 1.0, 2.0, 1), // mid-life
        state_slot([10.0, 20.0, -5.0], [0.0, 0.0, 0.0], 1.999, 2.0, 2), // near death
        state_slot([3.0, 3.0, 3.0], [0.0, 0.0, 0.0], 2.0, 2.0, 3), // age == lifetime: dead
        state_slot([7.0, 7.0, 7.0], [0.0, 0.0, 0.0], 5.0, 2.0, 4), // past lifetime: dead
        state_slot([9.0, 9.0, 9.0], [0.0, 0.0, 0.0], 0.0, 0.0, 5), // free slot (lifetime 0)
    ];
    let state: Vec<f32> = slots.iter().flatten().copied().collect();

    let gpu = harness.extract_presentation(&state).unwrap();
    assert_eq!(gpu.len(), slots.len() * PRESENT_STRIDE);

    for (slot_index, slot) in slots.iter().enumerate() {
        let expected = cpu_present_record(slot);
        let base = slot_index * PRESENT_STRIDE;
        for (word, &expected_word) in expected.iter().enumerate() {
            assert_eq!(
                gpu[base + word],
                expected_word,
                "slot {slot_index} word {word}: GPU={:#010x} reference={expected_word:#010x} \
                 (float GPU={} reference={})",
                gpu[base + word],
                f32::from_bits(gpu[base + word]),
                f32::from_bits(expected_word)
            );
        }
    }

    // The alive flags land where expected: slots 0/1/2 alive, 3/4/5 dead.
    let alive_flag = |slot_index: usize| gpu[slot_index * PRESENT_STRIDE + 10] & 0xffff;
    assert_eq!(
        (0..slots.len()).map(alive_flag).collect::<Vec<_>>(),
        vec![1, 1, 1, 0, 0, 0],
        "the alive bit reflects lifetime/age exactly"
    );
}

#[test]
fn gpu_present_compacts_live_slots_into_the_render_buffers() {
    // Hybrid roadmap M6 (render wiring, Step 2): the unified module's `present` entry must compact
    // the live slots into alive_indices and bump the indirect instance count + live counter, exactly
    // as the analytic `simulate` does, so the stateful output draws through the identical render path.
    // Prove it: over a mix of alive/dead/free slots, the compaction outputs match the alive set.
    let Some(harness) = require_harness() else {
        return;
    };
    let slots = [
        state_slot([0.0, 0.0, 0.0], [1.0, 2.0, 3.0], 0.0, 2.0, 0), // alive
        state_slot([1.0, 1.0, 1.0], [0.0, 0.0, 0.0], 3.0, 2.0, 1), // dead (age > lifetime)
        state_slot([2.0, 2.0, 2.0], [0.0, 0.0, 0.0], 1.0, 2.0, 2), // alive
        state_slot([3.0, 3.0, 3.0], [0.0, 0.0, 0.0], 0.0, 0.0, 3), // free (lifetime 0)
        state_slot([4.0, 4.0, 4.0], [0.0, 0.0, 0.0], 1.9, 2.0, 4), // alive
    ];
    let expected_alive: Vec<u32> = (0..slots.len() as u32)
        .filter(|&slot| {
            let s = slot as usize;
            slots[s][7] > 0.0 && slots[s][6] < slots[s][7]
        })
        .collect();
    let state: Vec<f32> = slots.iter().flatten().copied().collect();

    // slot_offset = 3 exercises the emitter's non-zero region of alive_indices.
    let (alive_indices, indirect, counters) = harness.present_compact(&state, 0, 3).unwrap();

    let count = expected_alive.len() as u32;
    assert_eq!(indirect[1], count, "indirect instance count == live count");
    assert_eq!(counters[0], count, "live counter == live count");
    // The compacted region [slot_offset, slot_offset + count) holds exactly the alive slots (order is
    // arbitrary under parallel compaction, so compare as sets).
    let mut compacted = alive_indices[3..3 + count as usize].to_vec();
    compacted.sort_unstable();
    assert_eq!(
        compacted, expected_alive,
        "the compacted alive_indices region lists exactly the live slots"
    );
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

#[test]
fn gpu_death_loop_with_reuse_matches_the_cpu_reference() {
    // Hybrid roadmap M6, the last algorithmically-interesting piece: the full stateful loop with
    // particle death and slot reuse must reproduce the M5 CPU reference. Run a window long enough
    // that early particles die and their slots are recycled by later spawns, then match every live
    // GPU particle to StatefulSimulation *by spawn ordinal* — the GPU's parallel slot assignment is
    // arbitrary and need not agree with the CPU's, only the per-identity state must.
    use aestra_runtime::StatefulConfig;
    let Some(harness) = require_harness() else {
        return;
    };
    // Short lifetime (0.5s = 30 ticks) forces death well inside the 90-tick window; capacity is
    // ample (steady-state live count ~= 30 * 4 = 120 << 512), so no capacity pressure — every spawn
    // succeeds and the ordinals line up exactly, isolating the death/reuse path.
    let config = StatefulConfig {
        gravity: [0.0, -9.81, 0.0],
        spawn_per_tick: 4,
        initial_speed: 12.0,
        lifetime: 0.5,
        capacity: 512,
    };
    let ticks = 90_u32;
    let seed = 0xDEAD_BEEF_CAFE_F00D_u64;

    let gpu = harness
        .advance_stateful_with_death(
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
    let cpu = simulation.alive_particles();

    // The window must actually exercise death: fewer particles are alive than were ever spawned.
    assert!(
        !cpu.is_empty(),
        "particles are alive at the end of the window"
    );
    assert!(
        cpu.len() < (ticks * config.spawn_per_tick) as usize,
        "the window retires particles (death is exercised, not just spawn+integrate)"
    );
    assert_eq!(
        gpu.len(),
        cpu.len(),
        "GPU and CPU agree on the live particle count ({} vs {})",
        gpu.len(),
        cpu.len()
    );

    let gpu_by_id: std::collections::HashMap<u64, [f32; 3]> = gpu.into_iter().collect();
    for (id, cpu_pos) in cpu {
        let gpu_pos = gpu_by_id.get(&id).unwrap_or_else(|| {
            panic!("CPU particle with ordinal {id} is missing from the GPU live set")
        });
        for axis in 0..3 {
            let (expected, actual) = (cpu_pos[axis], gpu_pos[axis]);
            let tolerance = 1e-3 + 1e-4 * expected.abs().max(actual.abs());
            assert!(
                (expected - actual).abs() <= tolerance,
                "ordinal {id} axis {axis}: CPU={expected:.5} GPU={actual:.5}"
            );
        }
    }
}
