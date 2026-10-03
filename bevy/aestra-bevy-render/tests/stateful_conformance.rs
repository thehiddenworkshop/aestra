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
    STATEFUL_COLLISION_WGSL, STATEFUL_FREE_LIST_WGSL, STATEFUL_NO_PHYSICS_WGSL,
    STATEFUL_NO_WORLD_WGSL, STATEFUL_PLACEMENT_WGSL, STATEFUL_PRESENT_WGSL,
    STATEFUL_SPAWN_RNG_WGSL, stateful_simulation_wgsl,
};
use aestra_runtime::{
    Collider, ColliderShape, MAX_COLLIDERS, SpawnPlacement, SpawnShape, StatefulConfig,
    StatefulSimulation,
};
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use std::{borrow::Cow, sync::mpsc, time::Duration};
use wgpu::util::DeviceExt;

/// Encodes a spawn shape as `(kind, radius, half_extents)` for the params buffer (0 = point,
/// 1 = sphere, 2 = box), matching the GPU `spawn_launch_position`.
fn shape_params(shape: SpawnShape) -> (u32, f32, [f32; 3]) {
    match shape {
        SpawnShape::Point => (0, 0.0, [0.0; 3]),
        SpawnShape::Sphere { radius } => (1, radius, [0.0; 3]),
        SpawnShape::Box { half_extents } => (2, 0.0, half_extents),
    }
}

/// Builds the production 25-word stateful `params` buffer from a config (the layout the death loop and
/// production kernels share). Floats are stored as bits.
/// The appearance `present` draws a particle with, when a test checks only its motion: white,
/// opaque, one unit across, all life (a gradient without keys is white).
fn plain_appearance(words: &mut [u32]) {
    let mut keys = [bevy::math::Vec2::ZERO; aestra_gpu::MAX_CURVE_KEYS];
    keys[0] = bevy::math::Vec2::new(0.0, 1.0);
    let one = aestra_gpu::GpuCurve {
        keys,
        count: 1,
        ..Default::default()
    };
    aestra_gpu::pack_stateful_appearance(
        &one,
        &one,
        &aestra_gpu::GpuGradient::default(),
        1.0,
        words,
    );
}

fn stateful_params(
    config: &StatefulConfig,
    seed: u64,
    emitter_index: u32,
    slot_offset: u32,
) -> Vec<u32> {
    let (shape_kind, shape_radius, half) = shape_params(config.shape);
    let mut words = vec![0_u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
    words[aestra_gpu::STATEFUL_VELOCITY_MODE_INDEX] = config.velocity_distribution as u32;
    words[..26].copy_from_slice(&[
        config.capacity,
        config.spawn_per_tick,
        seed as u32,
        (seed >> 32) as u32,
        config.speed.0.to_bits(),
        config.speed.1.to_bits(),
        config.lifetime.0.to_bits(),
        config.lifetime.1.to_bits(),
        TICK_DT.to_bits(),
        config.gravity[0].to_bits(),
        config.gravity[1].to_bits(),
        config.gravity[2].to_bits(),
        config.direction[0].to_bits(),
        config.direction[1].to_bits(),
        config.direction[2].to_bits(),
        config.spread.to_bits(),
        config.drag.to_bits(),
        emitter_index,
        slot_offset,
        config.turbulence.to_bits(),
        shape_kind,
        shape_radius.to_bits(),
        half[0].to_bits(),
        half[1].to_bits(),
        half[2].to_bits(),
        0.0_f32.to_bits(), // subtick (unused by death/spawn)
    ]);
    // Collider block (M10): count at 26, then up to MAX_COLLIDERS 10-word records from 27.
    let count = (config.collider_count as usize).min(MAX_COLLIDERS);
    aestra_gpu::pack_stateful_colliders(&config.colliders[..count], &mut words);
    aestra_gpu::pack_spawn_placement(&config.placement, &mut words);
    plain_appearance(&mut words);
    words
}

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

/// The combined per-tick stateful advance (no death): integrate the already-spawned slots by one dt,
/// then spawn this tick's new slots (spawn order = slot index), exactly as
/// `StatefulSimulation::advance_tick` does, with the full richer dynamics (speed & lifetime ranges,
/// spread cone, drag, spawn shape, value-noise turbulence). Stride-8 state (no ordinal, since
/// slot == ordinal here). Params (24 words): [seed_lo, seed_hi, capacity, spawned_before, to_spawn_end,
/// speed_min, speed_max, life_min, life_max, dt, gx, gy, gz, dir_x, dir_y, dir_z, spread, drag,
/// turbulence, shape_kind, shape_radius, half_x, half_y, half_z].
const ADVANCE_WGSL_ENTRY: &str = r#"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<u32>;

@compute @workgroup_size(64)
fn advance(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    let capacity = params[2];
    if (slot >= capacity) { return; }
    let seed = vec2<u32>(params[0], params[1]);
    let spawned_before = params[3];
    let to_spawn_end = params[4];
    let dt = bitcast<f32>(params[9]);
    let drag = bitcast<f32>(params[17]);
    let base = slot * 8u;
    if (slot < spawned_before) {
        // slot == the particle's spawn ordinal in this no-death by-slot loop.
        let age = state[base + 6u];
        let turbulence = spawn_turbulence(seed, slot, age, bitcast<f32>(params[18]));
        let ax = bitcast<f32>(params[10]) + turbulence.x;
        let ay = bitcast<f32>(params[11]) + turbulence.y;
        let az = bitcast<f32>(params[12]) + turbulence.z;
        let vgx = state[base + 3u] + ax * dt;
        let vgy = state[base + 4u] + ay * dt;
        let vgz = state[base + 5u] + az * dt;
        let vx = vgx - drag * vgx * dt;
        let vy = vgy - drag * vgy * dt;
        let vz = vgz - drag * vgz * dt;
        state[base + 3u] = vx;
        state[base + 4u] = vy;
        state[base + 5u] = vz;
        state[base + 0u] = state[base + 0u] + vx * dt;
        state[base + 1u] = state[base + 1u] + vy * dt;
        state[base + 2u] = state[base + 2u] + vz * dt;
        state[base + 6u] = age + dt;
    } else if (slot < to_spawn_end) {
        let direction = vec3<f32>(bitcast<f32>(params[13]), bitcast<f32>(params[14]), bitcast<f32>(params[15]));
        let velocity = spawn_launch_velocity(
            seed, slot, bitcast<f32>(params[5]), bitcast<f32>(params[6]), direction, bitcast<f32>(params[16]));
        let lifetime = bitcast<f32>(params[7])
            + (bitcast<f32>(params[8]) - bitcast<f32>(params[7])) * aestra_spawn_uniform(seed, vec2<u32>(slot, 0u), 1u);
        let half_extents = vec3<f32>(bitcast<f32>(params[21]), bitcast<f32>(params[22]), bitcast<f32>(params[23]));
        let position = spawn_launch_position(seed, slot, params[19], bitcast<f32>(params[20]), half_extents);
        state[base + 0u] = position.x;
        state[base + 1u] = position.y;
        state[base + 2u] = position.z;
        state[base + 3u] = velocity.x;
        state[base + 4u] = velocity.y;
        state[base + 5u] = velocity.z;
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
/// `params` uses the production 19-word layout (`aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS`).
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
        let dt = bitcast<f32>(params[8]);
        let drag = bitcast<f32>(params[16]);
        let seed = vec2<u32>(params[2], params[3]);
        let ordinal = bitcast<u32>(state[base + 8u]);
        let turbulence = spawn_turbulence(seed, ordinal, age, bitcast<f32>(params[19]));
        let ax = bitcast<f32>(params[9]) + turbulence.x;
        let ay = bitcast<f32>(params[10]) + turbulence.y;
        let az = bitcast<f32>(params[11]) + turbulence.z;
        let vgx = state[base + 3u] + ax * dt;
        let vgy = state[base + 4u] + ay * dt;
        let vgz = state[base + 5u] + az * dt;
        let vx = vgx - drag * vgx * dt;
        let vy = vgy - drag * vgy * dt;
        let vz = vgz - drag * vgz * dt;
        let px = state[base + 0u] + vx * dt;
        let py = state[base + 1u] + vy * dt;
        let pz = state[base + 2u] + vz * dt;
        // Collision against the authored colliders, via the shared production resolver (M10).
        let collision = aestra_resolve_colliders(vec3<f32>(px, py, pz), vec3<f32>(vx, vy, vz));
        state[base + 0u] = collision.position.x;
        state[base + 1u] = collision.position.y;
        state[base + 2u] = collision.position.z;
        state[base + 3u] = collision.velocity.x;
        state[base + 4u] = collision.velocity.y;
        state[base + 5u] = collision.velocity.z;
        var new_age = age + dt;
        if (collision.killed) { new_age = lifetime; }
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
    let direction = vec3<f32>(bitcast<f32>(params[12]), bitcast<f32>(params[13]), bitcast<f32>(params[14]));
    let velocity = spawn_distributed_velocity(
        seed, ordinal, bitcast<f32>(params[4]), bitcast<f32>(params[5]), direction,
        bitcast<f32>(params[15]), params[188]);
    let lifetime = bitcast<f32>(params[6])
        + (bitcast<f32>(params[7]) - bitcast<f32>(params[6])) * aestra_spawn_uniform(seed, vec2<u32>(ordinal, 0u), 1u);
    let half_extents = vec3<f32>(bitcast<f32>(params[22]), bitcast<f32>(params[23]), bitcast<f32>(params[24]));
    var position = spawn_launch_position(seed, ordinal, params[20], bitcast<f32>(params[21]), half_extents);
    var launch = velocity;
    if (params[AESTRA_PLACEMENT_INDEX] != 0u) {
        position = aestra_place_point(position);
        launch = aestra_place_vector(velocity);
    }
    let base = slot * 9u;
    state[base + 0u] = position.x;
    state[base + 1u] = position.y;
    state[base + 2u] = position.z;
    state[base + 3u] = launch.x;
    state[base + 4u] = launch.y;
    state[base + 5u] = launch.z;
    state[base + 6u] = 0.0;
    state[base + 7u] = lifetime;
    state[base + 8u] = bitcast<f32>(ordinal);
}
"#;

/// Presentation extraction: map each stride-9 persistent state slot to a 12-word GpuParticle record,
/// exercising the production `aestra_gpu::STATEFUL_PRESENT_WGSL`. Four bindings: state (read), the
/// presentation output (read-write), the sub-tick interpolation time (read), and the params whose
/// appearance block `present` draws with (read; a plain one here).
const PRESENT_ENTRY: &str = r#"
@group(0) @binding(0) var<storage, read> state: array<f32>;
@group(0) @binding(1) var<storage, read_write> present_out: array<f32>;
@group(0) @binding(2) var<storage, read> subtick: f32;
@group(0) @binding(3) var<storage, read> params: array<u32>;

@compute @workgroup_size(64)
fn present(@builtin(global_invocation_id) gid: vec3<u32>) {
    let slot = gid.x;
    if (slot >= arrayLength(&state) / AESTRA_STATE_STRIDE) { return; }
    aestra_present_stateful(slot, slot, 0u, subtick);
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
    /// A world SDF saying no world is supplied (host bindings HB10), for bindings that need one.
    no_world: wgpu::Buffer,
    /// A physics scene of no proxies (host bindings HB10).
    no_physics: wgpu::Buffer,
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
                "{STATEFUL_SPAWN_RNG_WGSL}{STATEFUL_FREE_LIST_WGSL}{STATEFUL_NO_WORLD_WGSL}\
                 {STATEFUL_NO_PHYSICS_WGSL}{STATEFUL_COLLISION_WGSL}{STATEFUL_PLACEMENT_WGSL}\
                 {DEATH_LOOP_WGSL}"
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
            entries: &[
                storage(0, true),
                storage(1, false),
                storage(2, true),
                storage(3, true),
            ],
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
        // The production unified module (10 bindings): here we drive its `present` entry, which both
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
                storage(9, false), // particle events (host bindings HB9b)
                storage(10, true), // the host's world SDF (host bindings HB10)
                storage(11, true), // the host's physics scene (host bindings HB10)
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
        let no_world = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("no world"),
            contents: &[0u8; 32],
            usage: wgpu::BufferUsages::STORAGE,
        });
        let no_physics = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("no physics"),
            contents: &[0u8; 16],
            usage: wgpu::BufferUsages::STORAGE,
        });
        Ok(Some(Self {
            no_world,
            no_physics,
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
    fn advance_stateful(
        &self,
        config: &StatefulConfig,
        seed: u64,
        ticks: u32,
    ) -> Result<Vec<f32>, String> {
        let capacity = config.capacity;
        let spawn_per_tick = config.spawn_per_tick;
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
            // ADVANCE layout (24 words): seed, capacity, spawned_before, to_spawn_end, speed range,
            // lifetime range, dt, gravity, direction, spread, drag, turbulence, shape.
            let (shape_kind, shape_radius, half) = shape_params(config.shape);
            let params: Vec<u32> = vec![
                seed as u32,
                (seed >> 32) as u32,
                capacity,
                spawned_before,
                to_spawn_end,
                config.speed.0.to_bits(),
                config.speed.1.to_bits(),
                config.lifetime.0.to_bits(),
                config.lifetime.1.to_bits(),
                TICK_DT.to_bits(),
                config.gravity[0].to_bits(),
                config.gravity[1].to_bits(),
                config.gravity[2].to_bits(),
                config.direction[0].to_bits(),
                config.direction[1].to_bits(),
                config.direction[2].to_bits(),
                config.spread.to_bits(),
                config.drag.to_bits(),
                config.turbulence.to_bits(),
                shape_kind,
                shape_radius.to_bits(),
                half[0].to_bits(),
                half[1].to_bits(),
                half[2].to_bits(),
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
    fn advance_stateful_with_death(
        &self,
        config: &StatefulConfig,
        seed: u64,
        ticks: u32,
    ) -> Result<Vec<(u64, [f32; 3])>, String> {
        let capacity = config.capacity;
        let spawn_per_tick = config.spawn_per_tick;
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
        // Params are constant across every tick (the production 19-word layout).
        let params = stateful_params(config, seed, 0, 0);
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

    /// The M7 seek proof for the full death loop: advance to `checkpoint_at` and snapshot ALL FOUR
    /// persistent buffers (state, free list, free count, spawn counter) GPU→GPU, overshoot forward to
    /// `overshoot_to`, then restore the snapshot and replay forward to `seek_target` — all in GPU
    /// commands. Returns the live `(ordinal, position)` set. Snapshotting all four buffers (not just
    /// state) is what makes the death loop's spawn ordinals and free-list reuse reproduce exactly.
    fn death_loop_seek_via_checkpoint(
        &self,
        config: &StatefulConfig,
        seed: u64,
        checkpoint_at: u32,
        overshoot_to: u32,
        seek_target: u32,
    ) -> Result<Vec<(u64, [f32; 3])>, String> {
        assert!(checkpoint_at <= seek_target && seek_target <= overshoot_to);
        let capacity = config.capacity;
        let copyable = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let make = |label, contents: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage: copyable,
                })
        };
        let state_bytes = encode(&vec![0.0_f32; capacity as usize * DEATH_STRIDE])?;
        let state = make("seek state", &state_bytes);
        let free_list = make(
            "seek free list",
            &encode(&(0..capacity).collect::<Vec<u32>>())?,
        );
        let free_count = make("seek free count", &encode(&capacity)?);
        let spawn_counter = make("seek spawn counter", &encode(&0_u32)?);
        // Checkpoint copies of the four buffers.
        let cp_state = make("cp state", &state_bytes);
        let cp_free_list = make("cp free list", &encode(&vec![0_u32; capacity as usize])?);
        let cp_free_count = make("cp free count", &encode(&0_u32)?);
        let cp_spawn_counter = make("cp spawn counter", &encode(&0_u32)?);
        let params_buffer = make(
            "seek params",
            &encode(&stateful_params(config, seed, 0, 0))?,
        );
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("seek bind group"),
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
        let spawn_per_tick = config.spawn_per_tick;
        let advance = |encoder: &mut wgpu::CommandEncoder, ticks: u32| {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("seek advance"),
                ..Default::default()
            });
            pass.set_bind_group(0, &bind_group, &[]);
            for _ in 0..ticks {
                pass.set_pipeline(&self.death_integrate_pipeline);
                pass.dispatch_workgroups(capacity.div_ceil(WORKGROUP), 1, 1);
                pass.set_pipeline(&self.death_spawn_pipeline);
                pass.dispatch_workgroups(spawn_per_tick.div_ceil(WORKGROUP), 1, 1);
            }
        };
        let snapshot =
            |encoder: &mut wgpu::CommandEncoder, from: &[&wgpu::Buffer], to: &[&wgpu::Buffer]| {
                for (source, dest) in from.iter().zip(to) {
                    encoder.copy_buffer_to_buffer(source, 0, dest, 0, source.size());
                }
            };
        let live = [&state, &free_list, &free_count, &spawn_counter];
        let saved = [&cp_state, &cp_free_list, &cp_free_count, &cp_spawn_counter];
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("seek readback"),
            size: state_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("seek commands"),
            });
        advance(&mut encoder, checkpoint_at);
        snapshot(&mut encoder, &live, &saved); // capture
        advance(&mut encoder, overshoot_to - checkpoint_at);
        snapshot(&mut encoder, &saved, &live); // restore
        advance(&mut encoder, seek_target - checkpoint_at);
        encoder.copy_buffer_to_buffer(&state, 0, &staging, 0, state_bytes.len() as u64);
        let raw = self.read_back_u32(encoder, &staging)?;
        let mut alive = Vec::new();
        for slot in 0..capacity as usize {
            let base = slot * DEATH_STRIDE;
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

    /// Runs presentation extraction over an uploaded stride-9 state buffer with a `subtick`
    /// interpolation time, returning the raw `capacity * 12` presentation words (the `GpuParticle`
    /// ABI). Floats come back as bits so `packed_emitter_alive` and `particle_index` are recovered
    /// exactly.
    fn extract_presentation(&self, state_words: &[f32], subtick: f32) -> Result<Vec<u32>, String> {
        let mut params = vec![0_u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
        plain_appearance(&mut params);
        self.extract_presentation_with(state_words, subtick, &params)
    }

    /// [`Self::extract_presentation`] drawing with the appearance block of `params`.
    fn extract_presentation_with(
        &self,
        state_words: &[f32],
        subtick: f32,
        params: &[u32],
    ) -> Result<Vec<u32>, String> {
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
        let subtick_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("present subtick"),
                contents: &encode(&subtick)?,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let params_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("present params"),
                contents: &encode(&params)?,
                usage: wgpu::BufferUsages::STORAGE,
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
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: subtick_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params_buffer.as_entire_binding(),
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
        // The 25-word layout; present reads only params[0] (capacity), [17] (emitter_index), and
        // [18] (slot_offset).
        let mut params = vec![0_u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
        plain_appearance(&mut params);
        params[0] = capacity;
        params[17] = emitter_index;
        params[18] = slot_offset;
        let params_buffer = buffer("present-compact params", &encode(&params)?, false);
        // Present writes at the global slot (slot_offset + local), so size the shared buffer to cover
        // this emitter's region.
        let present_out = buffer(
            "present-compact out",
            &encode(&vec![
                0.0_f32;
                (slot_offset + capacity) as usize * PRESENT_STRIDE
            ])?,
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
        let events = buffer("present-compact events", &encode(&vec![0_u32; 4])?, false);
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
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: events.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: self.no_world.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: self.no_physics.as_entire_binding(),
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

    // subtick 0: the raw tick state, no interpolation.
    let gpu = harness.extract_presentation(&state, 0.0).unwrap();
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
fn gpu_present_draws_the_emitters_appearance() {
    // Stateful particles (those that collide or follow a field) are drawn with the emitter's look —
    // its size and opacity over life, colour gradient and scale, sampled as the analytic path does —
    // not as white unit dots.
    let Some(harness) = require_harness() else {
        return;
    };
    let linear = |pairs: [(f32, f32); 2]| {
        let mut keys = [bevy::math::Vec2::ZERO; aestra_gpu::MAX_CURVE_KEYS];
        for (index, (time, value)) in pairs.into_iter().enumerate() {
            keys[index] = bevy::math::Vec2::new(time, value);
        }
        aestra_gpu::GpuCurve {
            keys,
            count: 2,
            _padding: bevy::math::Vec3::new(1.0, 0.0, 0.0), // linear
        }
    };
    let mut color_keys = [aestra_gpu::GpuGradientKey::default(); aestra_gpu::MAX_CURVE_KEYS];
    color_keys[0].color = bevy::math::Vec4::new(1.0, 0.0, 0.0, 1.0);
    color_keys[1].time = 1.0;
    color_keys[1].color = bevy::math::Vec4::new(0.0, 0.0, 1.0, 0.8);
    let color = aestra_gpu::GpuGradient {
        keys: color_keys,
        count: 2,
        ..Default::default()
    };
    let mut params = vec![0_u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
    aestra_gpu::pack_stateful_appearance(
        &linear([(0.0, 2.0), (1.0, 6.0)]),
        &linear([(0.0, 1.0), (1.0, 0.0)]),
        &color,
        2.0,
        &mut params,
    );
    // Half-way through life: size 4 × scale 2, opacity ½, colour half red, half blue.
    let slot = state_slot([0.0; 3], [0.0; 3], 1.0, 2.0, 0);
    let record = harness
        .extract_presentation_with(&slot, 0.0, &params)
        .unwrap();
    let words: Vec<f32> = record[..8]
        .iter()
        .map(|word| f32::from_bits(*word))
        .collect();
    let expected = [0.5, 0.0, 0.5, 0.9 * 0.5];
    for (channel, (got, want)) in words[..4].iter().zip(expected).enumerate() {
        assert!(
            (got - want).abs() < 1e-6,
            "colour {channel}: {got} vs {want}"
        );
    }
    assert!((words[7] - 8.0).abs() < 1e-5, "size: {}", words[7]);
}

#[test]
fn gpu_present_interpolates_position_and_age_by_the_subtick() {
    // Hybrid roadmap M8 presentation interpolation: at a non-zero sub-tick, present extrapolates the
    // position by velocity * subtick and the age by subtick, so stateful particles move smoothly
    // between the 60 Hz ticks (and stay coherent with the continuous time analytic emitters use).
    let Some(harness) = require_harness() else {
        return;
    };
    let slots = [
        state_slot([0.0, 0.0, 0.0], [10.0, -4.0, 2.0], 0.5, 2.0, 0),
        state_slot([1.0, 2.0, 3.0], [0.0, 8.0, -1.0], 1.0, 2.0, 1),
    ];
    let state: Vec<f32> = slots.iter().flatten().copied().collect();
    let subtick = 0.5_f32 / 60.0; // half a tick

    let gpu = harness.extract_presentation(&state, subtick).unwrap();

    for (index, slot) in slots.iter().enumerate() {
        let base = index * PRESENT_STRIDE;
        let position = [slot[0], slot[1], slot[2]];
        let velocity = [slot[3], slot[4], slot[5]];
        for axis in 0..3 {
            let expected = position[axis] + velocity[axis] * subtick;
            let actual = f32::from_bits(gpu[base + 4 + axis]);
            assert!(
                (expected - actual).abs() <= 1e-5,
                "slot {index} axis {axis}: extrapolated {actual} != {expected}"
            );
        }
        // normalized_age is (age + subtick) / lifetime, clamped.
        let expected_age = ((slot[6] + subtick) / slot[7]).clamp(0.0, 1.0);
        let actual_age = f32::from_bits(gpu[base + 9]);
        assert!(
            (expected_age - actual_age).abs() <= 1e-5,
            "slot {index} normalized age {actual_age} != {expected_age}"
        );
    }
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
    let slot_offset = 3_u32;
    // alive_indices stores the GLOBAL particle index (slot_offset + local slot), which the renderer
    // uses to fetch present_out[global].
    let expected_global: Vec<u32> = (0..slots.len() as u32)
        .filter(|&slot| {
            let s = slot as usize;
            slots[s][7] > 0.0 && slots[s][6] < slots[s][7]
        })
        .map(|local| slot_offset + local)
        .collect();
    let state: Vec<f32> = slots.iter().flatten().copied().collect();

    // slot_offset = 3 exercises the emitter's non-zero region of alive_indices and present_out.
    let (alive_indices, indirect, counters) =
        harness.present_compact(&state, 0, slot_offset).unwrap();

    let count = expected_global.len() as u32;
    assert_eq!(indirect[1], count, "indirect instance count == live count");
    assert_eq!(counters[0], count, "live counter == live count");
    // The compacted region [slot_offset, slot_offset + count) holds exactly the alive global indices
    // (order is arbitrary under parallel compaction, so compare as sets).
    let mut compacted =
        alive_indices[slot_offset as usize..slot_offset as usize + count as usize].to_vec();
    compacted.sort_unstable();
    assert_eq!(
        compacted, expected_global,
        "the compacted alive_indices region lists exactly the live global slots"
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
    // Hybrid roadmap M6: the GPU spawn+integrate loop must reproduce the M5 CPU reference, including
    // the richer dynamics (per-particle speed & lifetime ranges, a spread cone, and drag). Run the full
    // per-tick loop on the GPU and compare the presented positions to StatefulSimulation. Long lifetimes
    // keep every particle alive across the window, so no death/free-list is exercised here.
    let Some(harness) = require_harness() else {
        return;
    };
    let config = StatefulConfig {
        gravity: [0.0, -9.81, 0.0],
        spawn_per_tick: 4,
        speed: (8.0, 16.0),
        lifetime: (1000.0, 1000.0),
        direction: [0.2, 1.0, -0.1],
        spread: 0.6,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.5,
        shape: SpawnShape::Box {
            half_extents: [4.0, 1.0, 6.0],
        },
        turbulence: 5.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        homing: None,
    };
    let ticks = 100_u32;
    let seed = 0x1234_5678_9abc_def0_u64;

    let gpu = harness.advance_stateful(&config, seed, ticks).unwrap();

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
fn gpu_death_loop_checkpoint_seek_reaches_the_uninterrupted_state() {
    // Hybrid roadmap M7: a backward seek of a full stateful effect restores the nearest GPU-resident
    // checkpoint (all four persistent buffers) and replays forward — never a reverse integration.
    // Prove it for the death loop: checkpoint at tick 40, overshoot to 90, restore + replay to 70, and
    // match the uninterrupted forward run to 70 by spawn ordinal. Snapshotting the free list and spawn
    // counter (not just state) is what makes the ordinals and slot reuse line up.
    let Some(harness) = require_harness() else {
        return;
    };
    let config = StatefulConfig {
        gravity: [0.0, -9.81, 0.0],
        spawn_per_tick: 4,
        speed: (9.0, 15.0),
        lifetime: (0.45, 0.6),
        direction: [0.0, 1.0, 0.0],
        spread: 0.5,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.8,
        shape: SpawnShape::Sphere { radius: 2.0 },
        turbulence: 7.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        homing: None,
    };
    let seed = 0xC0FF_EE00_1234_5678_u64;

    let seeked = harness
        .death_loop_seek_via_checkpoint(&config, seed, 40, 90, 70)
        .unwrap();
    let uninterrupted = harness
        .advance_stateful_with_death(&config, seed, 70)
        .unwrap();

    assert!(!uninterrupted.is_empty(), "particles are alive at tick 70");
    assert_eq!(
        seeked.len(),
        uninterrupted.len(),
        "restore+replay and the uninterrupted run agree on the live count"
    );
    let by_id: std::collections::HashMap<u64, [f32; 3]> = uninterrupted.into_iter().collect();
    for (id, seeked_pos) in seeked {
        let expected = by_id
            .get(&id)
            .unwrap_or_else(|| panic!("ordinal {id} present after seek but not uninterrupted"));
        for axis in 0..3 {
            let (a, b) = (seeked_pos[axis], expected[axis]);
            let tolerance = 1e-3 + 1e-4 * a.abs().max(b.abs());
            assert!(
                (a - b).abs() <= tolerance,
                "ordinal {id} axis {axis}: seek={a:.5} uninterrupted={b:.5}"
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
    let Some(harness) = require_harness() else {
        return;
    };
    // Short lifetimes (~0.5s) force death well inside the 90-tick window; capacity is ample (steady
    // state << 512), so no capacity pressure — every spawn succeeds and the ordinals line up exactly,
    // isolating the death/reuse path. Richer dynamics (speed & lifetime ranges, a spread cone, drag)
    // are all exercised, and all match the CPU reference bit-for-bit.
    let config = StatefulConfig {
        gravity: [0.0, -9.81, 0.0],
        spawn_per_tick: 4,
        speed: (9.0, 15.0),
        lifetime: (0.45, 0.6),
        direction: [0.0, 1.0, 0.0],
        spread: 0.5,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.8,
        shape: SpawnShape::Sphere { radius: 2.5 },
        turbulence: 7.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        homing: None,
    };
    let ticks = 90_u32;
    let seed = 0xDEAD_BEEF_CAFE_F00D_u64;

    let gpu = harness
        .advance_stateful_with_death(&config, seed, ticks)
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

#[test]
fn explicit_velocity_distributions_match_gpu_across_death_reuse_and_replay() {
    let Some(harness) = require_harness() else {
        return;
    };
    for mode in aestra_core::VelocityDistribution::ALL {
        for direction in [
            [0.0, 1.0, 0.0],
            [1.0, -2.0, 0.25],
            [0.0; 3],
            [2e-7, 0.0, 0.0],
            [f32::MAX, 0.0, 0.0],
        ] {
            if mode.is_legacy() && direction[0] == f32::MAX {
                continue;
            }
            let config = StatefulConfig {
                gravity: [0.0, -9.81, 0.0],
                spawn_per_tick: 4,
                speed: (18.0, 22.0),
                lifetime: (0.45, 0.6),
                direction,
                velocity_distribution: mode,
                spread: if mode.is_legacy() { 0.5 } else { 60.0 },
                drag: 0.8,
                shape: SpawnShape::Point,
                turbulence: 0.0,
                placement: SpawnPlacement::IDENTITY,
                colliders: [Collider::NONE; MAX_COLLIDERS],
                collider_count: 0,
                capacity: 512,
                homing: None,
            };
            let seed = 0xDEAD_BEEF_CAFE_F00D;
            let gpu = harness
                .advance_stateful_with_death(&config, seed, 90)
                .unwrap();
            let mut cpu = StatefulSimulation::new(config, seed);
            cpu.advance_to_tick(30);
            let snapshot = cpu.clone();
            cpu.advance_to_tick(90);
            let samples = cpu.alive_particles();
            cpu = snapshot;
            cpu.advance_to_tick(90);
            assert_eq!(samples, cpu.alive_particles(), "{mode:?} replay");
            assert_eq!(gpu.len(), samples.len());
            let by_id: std::collections::HashMap<_, _> = gpu.into_iter().collect();
            for (id, expected) in samples {
                let actual = by_id.get(&id).expect("stable spawn identity");
                for axis in 0..3 {
                    assert!(
                        (expected[axis] - actual[axis]).abs() <= 1e-3 + 1e-4 * expected[axis].abs(),
                        "{mode:?}, direction {direction:?}, ordinal {id}, axis {axis}: CPU {expected:?}, GPU {actual:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn gpu_spawn_placement_matches_the_cpu_reference() {
    // A moved, rotated and scaled emitter: the GPU spawn kernel places each spawn exactly as
    // `SpawnPlacement` does on the CPU, through a window with death and reuse.
    let Some(harness) = require_harness() else {
        return;
    };
    let (sin, cos) = (0.6_f32.sin(), 0.6_f32.cos());
    let axis = [0.48_f32, 0.6, 0.64];
    let config = StatefulConfig {
        gravity: [0.0, -9.81, 0.0],
        spawn_per_tick: 4,
        speed: (9.0, 15.0),
        lifetime: (0.45, 0.6),
        direction: [0.0, 1.0, 0.0],
        spread: 0.5,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.8,
        shape: SpawnShape::Box {
            half_extents: [3.0, 1.0, 2.0],
        },
        turbulence: 7.0,
        placement: SpawnPlacement {
            translation: [30.0, 7.5, -12.0],
            rotation: [axis[0] * sin, axis[1] * sin, axis[2] * sin, cos],
            scale: [2.0, 0.5, 1.5],
        },
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        homing: None,
    };
    let seed = 0x0DD5_EED5_1234_5678_u64;
    let ticks = 90_u32;

    let gpu = harness
        .advance_stateful_with_death(&config, seed, ticks)
        .unwrap();
    let mut simulation = StatefulSimulation::new(config, seed);
    simulation.advance_to_tick(ticks as u64);
    let cpu = simulation.alive_particles();

    assert!(!cpu.is_empty());
    assert_eq!(gpu.len(), cpu.len());
    let count = cpu.len() as f32;
    let centroid_x = cpu.iter().map(|(_, p)| p[0]).sum::<f32>() / count;
    let centroid_z = cpu.iter().map(|(_, p)| p[2]).sum::<f32>() / count;
    assert!(
        (centroid_x - 30.0).abs() < 10.0 && (centroid_z + 12.0).abs() < 10.0,
        "the particles live around the moved emitter ({centroid_x}, {centroid_z})"
    );
    let gpu_by_id: std::collections::HashMap<u64, [f32; 3]> = gpu.into_iter().collect();
    for (id, cpu_pos) in cpu {
        let gpu_pos = gpu_by_id[&id];
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

/// A config exercising all three collider shapes (M10): a ground plane, a sphere obstacle, and a box
/// obstacle in the particles' fall path, plus gravity + drag + turbulence so the persistent dynamics
/// are non-trivial between contacts.
fn collision_config(colliders: [Collider; MAX_COLLIDERS], collider_count: u32) -> StatefulConfig {
    StatefulConfig {
        gravity: [0.0, -18.0, 0.0],
        spawn_per_tick: 4,
        speed: (6.0, 10.0),
        lifetime: (1.2, 1.6),
        direction: [0.0, 1.0, 0.0],
        spread: 0.7,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.3,
        shape: SpawnShape::Sphere { radius: 1.0 },
        turbulence: 3.0,
        placement: SpawnPlacement::IDENTITY,
        colliders,
        collider_count,
        capacity: 512,
        homing: None,
    }
}

#[test]
fn gpu_collision_matches_the_cpu_reference() {
    // Hybrid roadmap M10: the stateful collision response (plane, sphere, and box colliders; bounce
    // with restitution + friction) must reproduce the CPU reference bit-for-bit through the shared
    // production resolver. Match every live GPU particle to StatefulSimulation by spawn ordinal, as the
    // death-loop test does — collision is deterministic per fixed tick, so the two agree.
    let Some(harness) = require_harness() else {
        return;
    };
    let mut colliders = [Collider::NONE; MAX_COLLIDERS];
    // The ground plane sits below the spawn sphere (radius 1 at the origin) so every particle — even
    // one spawned on the final tick, before its first collision resolve — starts above it.
    colliders[0] = Collider {
        shape: ColliderShape::Plane {
            normal: [0.0, 1.0, 0.0],
            distance: -2.0,
        },
        restitution: 0.6,
        friction: 0.3,
        kill: false,
    };
    colliders[1] = Collider {
        shape: ColliderShape::Sphere {
            center: [1.5, 3.0, 0.0],
            radius: 1.4,
        },
        restitution: 0.8,
        friction: 0.1,
        kill: false,
    };
    colliders[2] = Collider {
        shape: ColliderShape::Aabb {
            min: [-2.5, 1.0, -1.5],
            max: [-0.8, 2.2, 1.5],
        },
        restitution: 0.4,
        friction: 0.5,
        kill: false,
    };
    let config = collision_config(colliders, 3);
    let ticks = 120_u32;
    let seed = 0x00C0_11DE_0000_0001_u64;

    let gpu = harness
        .advance_stateful_with_death(&config, seed, ticks)
        .unwrap();

    let mut simulation = StatefulSimulation::new(config, seed);
    simulation.advance_to_tick(ticks as u64);
    let cpu = simulation.alive_particles();

    assert!(
        !cpu.is_empty(),
        "particles are alive at the end of the window"
    );
    assert_eq!(
        gpu.len(),
        cpu.len(),
        "GPU and CPU agree on the live particle count ({} vs {})",
        gpu.len(),
        cpu.len()
    );

    let gpu_by_id: std::collections::HashMap<u64, [f32; 3]> = gpu.into_iter().collect();
    for (id, cpu_pos) in &cpu {
        let gpu_pos = gpu_by_id.get(id).unwrap_or_else(|| {
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

    // Collision is actually doing something: the ground plane keeps every live particle on/above it,
    // whereas the same effect without colliders lets particles fall well below it.
    for (id, position) in &cpu {
        assert!(
            position[1] >= -2.05,
            "ordinal {id} stays on/above the ground plane at y = -2: y = {}",
            position[1]
        );
    }
    let mut without =
        StatefulSimulation::new(collision_config([Collider::NONE; MAX_COLLIDERS], 0), seed);
    without.advance_to_tick(ticks as u64);
    let lowest = without
        .alive_particles()
        .iter()
        .map(|(_, position)| position[1])
        .fold(f32::INFINITY, f32::min);
    assert!(
        lowest < -2.5,
        "without colliders particles fall below the plane (lowest y = {lowest}), so the plane matters"
    );
}

#[test]
fn gpu_collision_checkpoint_seek_reaches_the_uninterrupted_state() {
    // Hybrid roadmap M10 acceptance: a backward seek reproduces the uninterrupted forward simulation
    // *with collision active*. Because collision reads only the persistent position/velocity that the
    // checkpoint store snapshots, restoring the nearest checkpoint and replaying forward — bounces and
    // all — lands on the same state as running straight through. Checkpoint at 40, overshoot to 90,
    // restore + replay to 70, and match the uninterrupted run to 70 by spawn ordinal.
    let Some(harness) = require_harness() else {
        return;
    };
    let mut colliders = [Collider::NONE; MAX_COLLIDERS];
    colliders[0] = Collider {
        shape: ColliderShape::Plane {
            normal: [0.0, 1.0, 0.0],
            distance: -2.0,
        },
        restitution: 0.6,
        friction: 0.3,
        kill: false,
    };
    colliders[1] = Collider {
        shape: ColliderShape::Sphere {
            center: [1.5, 3.0, 0.0],
            radius: 1.4,
        },
        restitution: 0.8,
        friction: 0.1,
        kill: false,
    };
    let config = collision_config(colliders, 2);
    let seed = 0x00C0_11DE_5EEC_0001_u64;

    let seeked = harness
        .death_loop_seek_via_checkpoint(&config, seed, 40, 90, 70)
        .unwrap();
    let uninterrupted = harness
        .advance_stateful_with_death(&config, seed, 70)
        .unwrap();

    assert!(!uninterrupted.is_empty(), "particles are alive at tick 70");
    assert_eq!(
        seeked.len(),
        uninterrupted.len(),
        "restore+replay and the uninterrupted run agree on the live count with collision active"
    );
    let by_id: std::collections::HashMap<u64, [f32; 3]> = uninterrupted.into_iter().collect();
    for (id, seeked_pos) in seeked {
        let expected = by_id
            .get(&id)
            .unwrap_or_else(|| panic!("ordinal {id} present after seek but not uninterrupted"));
        for axis in 0..3 {
            let (a, b) = (seeked_pos[axis], expected[axis]);
            let tolerance = 1e-3 + 1e-4 * a.abs().max(b.abs());
            assert!(
                (a - b).abs() <= tolerance,
                "ordinal {id} axis {axis}: seek={a:.5} uninterrupted={b:.5}"
            );
        }
    }
}

// ---- Homing (host bindings HB7) ----

/// Runs the *production* unified module's `death_integrate` + `spawn` for one tick per entry of
/// `targets`, packing each tick's homing block from the host's input as the Bevy backend does (a
/// `HomingTracker` resolving losses), and returns the live `(ordinal, position)` set.
fn advance_production_homing(
    harness: &Harness,
    config: &StatefulConfig,
    seed: u64,
    targets: &[Option<aestra_runtime::HomingTarget>],
) -> Result<Vec<(u64, [f32; 3])>, String> {
    let homing = config.homing.expect("a homing config");
    let mut tracker = aestra_runtime::HomingTracker::default();
    let ticks: Vec<Vec<u32>> = targets
        .iter()
        .map(|target| {
            let resolved = tracker.resolve(homing.lost, *target);
            let mut words = stateful_params(config, seed, 0, 0);
            aestra_gpu::pack_stateful_homing(Some(&homing), resolved.as_ref(), &mut words);
            words
        })
        .collect();
    advance_production_ticks(harness, config, &ticks)
}

/// Runs the *production* unified module's `death_integrate` + `spawn` once per entry of `ticks` —
/// each that tick's packed params — and returns the live `(ordinal, position)` set.
fn advance_production_ticks(
    harness: &Harness,
    config: &StatefulConfig,
    ticks: &[Vec<u32>],
) -> Result<Vec<(u64, [f32; 3])>, String> {
    advance_production_counted(harness, config, ticks).map(|(live, _)| live)
}

/// The live `(ordinal, position)` set, and the four `counters` words the kernels counted into.
type CountedRun = (Vec<(u64, [f32; 3])>, Vec<u32>);

/// [`advance_production_ticks`], also returning the `counters` words.
fn advance_production_counted(
    harness: &Harness,
    config: &StatefulConfig,
    ticks: &[Vec<u32>],
) -> Result<CountedRun, String> {
    advance_production_in_world(harness, config, ticks, &harness.no_world)
}

/// [`advance_production_counted`] in the host world `world` (a packed world SDF, host bindings
/// HB10).
fn advance_production_in_world(
    harness: &Harness,
    config: &StatefulConfig,
    ticks: &[Vec<u32>],
    world: &wgpu::Buffer,
) -> Result<CountedRun, String> {
    advance_production_in_scene(harness, config, ticks, world, &harness.no_physics)
}

/// [`advance_production_in_world`] among the host's physics proxies `physics` too (a packed physics
/// scene, host bindings HB10).
fn advance_production_in_scene(
    harness: &Harness,
    config: &StatefulConfig,
    ticks: &[Vec<u32>],
    world: &wgpu::Buffer,
    physics: &wgpu::Buffer,
) -> Result<CountedRun, String> {
    let device = &harness.device;
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("homing"),
        bind_group_layouts: &[Some(&harness.unified_layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("homing"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(stateful_simulation_wgsl())),
    });
    let pipeline = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let (death, spawn) = (pipeline("death_integrate"), pipeline("spawn"));
    let capacity = config.capacity;
    let buffer = |label: &str, bytes: Vec<u8>, usage: wgpu::BufferUsages| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE | usage,
        })
    };
    let state_bytes = encode(&vec![0.0_f32; capacity as usize * 9])?;
    let state = buffer("state", state_bytes.clone(), wgpu::BufferUsages::COPY_SRC);
    let free_list = buffer(
        "free list",
        encode(&(0..capacity).collect::<Vec<u32>>())?,
        wgpu::BufferUsages::empty(),
    );
    let free_count = buffer(
        "free count",
        encode(&capacity)?,
        wgpu::BufferUsages::empty(),
    );
    let spawn_counter = buffer(
        "spawn counter",
        encode(&0_u32)?,
        wgpu::BufferUsages::empty(),
    );
    let scratch =
        |words: usize| buffer("scratch", vec![0u8; words * 4], wgpu::BufferUsages::empty());
    let (present, alive, indirect) = (
        scratch(capacity as usize * 12),
        scratch(capacity as usize),
        scratch(8),
    );
    let counters = buffer("counters", vec![0u8; 16], wgpu::BufferUsages::COPY_SRC);
    let events = scratch(4);
    let mut encoder = device.create_command_encoder(&Default::default());
    for words in ticks {
        let params = buffer("params", encode(words)?, wgpu::BufferUsages::empty());
        let entries: Vec<wgpu::BindGroupEntry> = [
            &state,
            &free_list,
            &free_count,
            &spawn_counter,
            &params,
            &present,
            &alive,
            &indirect,
            &counters,
            &events,
            world,
            physics,
        ]
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("homing tick"),
            layout: &harness.unified_layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_bind_group(0, &group, &[]);
        pass.set_pipeline(&death);
        pass.dispatch_workgroups(capacity.div_ceil(WORKGROUP), 1, 1);
        pass.set_pipeline(&spawn);
        pass.dispatch_workgroups(config.spawn_per_tick.div_ceil(WORKGROUP).max(1), 1, 1);
    }
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("homing readback"),
        size: state_bytes.len() as u64 + 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&state, 0, &staging, 0, state_bytes.len() as u64);
    encoder.copy_buffer_to_buffer(&counters, 0, &staging, state_bytes.len() as u64, 16);
    let raw = harness.read_back_u32(encoder, &staging)?;
    let counted = raw[capacity as usize * 9..].to_vec();
    let mut live = Vec::new();
    for slot in 0..capacity as usize {
        let base = slot * 9;
        let (age, lifetime) = (f32::from_bits(raw[base + 6]), f32::from_bits(raw[base + 7]));
        if lifetime > 0.0 && age < lifetime {
            live.push((
                raw[base + 8] as u64,
                [
                    f32::from_bits(raw[base]),
                    f32::from_bits(raw[base + 1]),
                    f32::from_bits(raw[base + 2]),
                ],
            ));
        }
    }
    Ok((live, counted))
}

fn homing_config(lost: aestra_runtime::HomingLostPolicy, arrival_radius: f32) -> StatefulConfig {
    StatefulConfig {
        gravity: [0.0, 0.0, 0.0],
        spawn_per_tick: 2,
        speed: (4.0, 6.0),
        lifetime: (3.0, 3.5),
        direction: [0.0, 1.0, 0.0],
        spread: 0.8,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.0,
        shape: SpawnShape::Sphere { radius: 0.5 },
        turbulence: 0.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        homing: Some(aestra_runtime::HomingConfig {
            speed: 20.0,
            acceleration: 40.0,
            turn_rate: 6.0,
            arrival_radius,
            lost,
        }),
    }
}

/// A target circling at radius 6 around (0, 4, 0) — moving each tick — and lost for ticks 60..120.
fn circling_target(ticks: u32) -> Vec<Option<aestra_runtime::HomingTarget>> {
    (0..ticks)
        .map(|tick| {
            if (60..120).contains(&tick) {
                return None;
            }
            // Trig on the host only: the kernels receive the resolved point.
            let angle = tick as f32 * 0.03;
            Some(aestra_runtime::HomingTarget {
                position: [6.0 * angle.cos(), 4.0, 6.0 * angle.sin()],
                velocity: [-0.18 * angle.sin() * 60.0, 0.0, 0.18 * angle.cos() * 60.0],
            })
        })
        .collect()
}

fn cpu_homing(
    config: StatefulConfig,
    seed: u64,
    targets: &[Option<aestra_runtime::HomingTarget>],
) -> Vec<(u64, [f32; 3])> {
    let mut simulation = StatefulSimulation::new(config, seed);
    for target in targets {
        simulation.set_homing_target(*target);
        simulation.advance_tick();
    }
    simulation.alive_particles()
}

fn assert_same_particles(cpu: &[(u64, [f32; 3])], gpu: &[(u64, [f32; 3])]) {
    assert_eq!(
        gpu.len(),
        cpu.len(),
        "live counts: GPU {} vs CPU {}",
        gpu.len(),
        cpu.len()
    );
    let by_id: std::collections::HashMap<u64, [f32; 3]> = gpu.iter().copied().collect();
    for (id, expected) in cpu {
        let actual = by_id
            .get(id)
            .unwrap_or_else(|| panic!("ordinal {id} is missing from the GPU"));
        for axis in 0..3 {
            let tolerance = 1e-3 + 1e-4 * expected[axis].abs().max(actual[axis].abs());
            assert!(
                (expected[axis] - actual[axis]).abs() <= tolerance,
                "ordinal {id} axis {axis}: CPU {} GPU {}",
                expected[axis],
                actual[axis]
            );
        }
    }
}

#[test]
fn gpu_homing_steers_like_the_cpu_reference_toward_a_moving_target() {
    use aestra_runtime::HomingLostPolicy;
    let Some(harness) = require_harness() else {
        return;
    };
    let seed = 0x00AB_0017_0000_0001_u64;
    let targets = circling_target(180);
    // A target that moves every tick, lost for a second, then back: GPU and CPU agree by ordinal.
    let config = homing_config(HomingLostPolicy::KeepLastPosition, 0.3);
    let gpu = advance_production_homing(&harness, &config, seed, &targets).unwrap();
    let cpu = cpu_homing(config, seed, &targets);
    assert!(!cpu.is_empty());
    assert_same_particles(&cpu, &gpu);
    // Homing does something: the particles a second old or more — time to catch up — gather near
    // the target, unlike undirected ones.
    let last = targets.last().unwrap().unwrap().position;
    let spread = |particles: &[(u64, [f32; 3])]| {
        let settled: Vec<_> = particles.iter().filter(|(id, _)| *id < 2 * 120).collect();
        settled
            .iter()
            .map(|(_, p)| {
                ((p[0] - last[0]).powi(2) + (p[1] - last[1]).powi(2) + (p[2] - last[2]).powi(2))
                    .sqrt()
            })
            .sum::<f32>()
            / settled.len().max(1) as f32
    };
    let mut free = homing_config(HomingLostPolicy::KeepLastPosition, 0.3);
    free.homing = None;
    let unsteered = cpu_homing(free, seed, &targets);
    eprintln!(
        "mean distance to the target: {} homing, {} without",
        spread(&cpu),
        spread(&unsteered)
    );
    assert!(spread(&cpu) < 0.5 * spread(&unsteered));

    // Killed while lost: the same on both sides, and nothing older than the loss survives it.
    let config = homing_config(HomingLostPolicy::Kill, 0.3);
    let lost_until = &targets[..120];
    let gpu = advance_production_homing(&harness, &config, seed, lost_until).unwrap();
    let cpu = cpu_homing(config, seed, lost_until);
    assert_same_particles(&cpu, &gpu);
    assert!(cpu.iter().all(|(id, _)| *id >= 118), "{cpu:?}");

    // Arrival retires them: a still target, a generous radius.
    let still = vec![
        Some(aestra_runtime::HomingTarget {
            position: [0.0, 3.0, 0.0],
            velocity: [0.0; 3],
        });
        120
    ];
    let config = homing_config(HomingLostPolicy::KeepDirection, 1.5);
    let gpu = advance_production_homing(&harness, &config, seed, &still).unwrap();
    let cpu = cpu_homing(config, seed, &still);
    assert_same_particles(&cpu, &gpu);
    assert!(
        cpu.len() < 240 / 2,
        "most arrived and retired: {} of 240 spawned are left",
        cpu.len()
    );
}

// ---- Attachment (host bindings HB7b) ----

#[test]
fn gpu_spawns_follow_a_moving_attachment_like_the_cpu_reference_and_leave_a_wake() {
    // An emitter attached to a blade sweeping along +x and turning: each tick's spawns land where the
    // blade is that tick (the host resolves the placement per frame), the ones in flight keep their
    // motion — the same by ordinal on both sides, and spread out behind the blade.
    let Some(harness) = require_harness() else {
        return;
    };
    let base = StatefulConfig {
        gravity: [0.0, 0.0, 0.0],
        spawn_per_tick: 3,
        speed: (0.5, 1.0),
        lifetime: (0.8, 1.0),
        direction: [0.0, 1.0, 0.0],
        spread: 0.4,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.5,
        shape: SpawnShape::Sphere { radius: 0.1 },
        turbulence: 0.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        homing: None,
    };
    let seed = 0x00AB_007B_0000_0001_u64;
    // The host's trig: the blade tip moves 0.5 per tick along +x and turns about +y.
    let placements: Vec<SpawnPlacement> = (0..90)
        .map(|tick| {
            let half = tick as f32 * 0.01;
            SpawnPlacement {
                translation: [-20.0 + 0.5 * tick as f32, 2.0, 0.0],
                rotation: [0.0, half.sin(), 0.0, half.cos()],
                scale: [1.0; 3],
            }
        })
        .collect();
    let ticks: Vec<Vec<u32>> = placements
        .iter()
        .map(|placement| {
            let config = StatefulConfig {
                placement: *placement,
                ..base
            };
            stateful_params(&config, seed, 0, 0)
        })
        .collect();
    let gpu = advance_production_ticks(&harness, &base, &ticks).unwrap();
    let mut simulation = StatefulSimulation::new(base, seed);
    for placement in &placements {
        simulation.set_placement(*placement);
        simulation.advance_tick();
    }
    let cpu = simulation.alive_particles();
    assert!(!cpu.is_empty());
    assert_same_particles(&cpu, &gpu);
    // A wake: the live particles stretch back along the blade's path (~0.9 s at 30 units/s), where a
    // still emitter's would stay within a couple of units.
    let (min_x, max_x) = cpu.iter().fold((f32::MAX, f32::MIN), |(lo, hi), (_, p)| {
        (lo.min(p[0]), hi.max(p[0]))
    });
    assert!(max_x - min_x > 20.0, "the wake spans {min_x}..{max_x}");
    let tip = placements.last().unwrap().translation[0];
    assert!(
        (max_x - tip).abs() < 2.0,
        "the newest spawns are at the tip ({max_x} vs {tip})"
    );
}

#[test]
fn gpu_homing_counts_arrivals_like_the_cpu_reference_only_when_asked() {
    // The `impact` event (host bindings HB9): each particle reaching the target adds one to the
    // counters word the params name — as many as the CPU reference's `arrivals()` — and a tick whose
    // params name none (a replay) counts nothing.
    use aestra_runtime::HomingLostPolicy;
    let Some(harness) = require_harness() else {
        return;
    };
    let seed = 0x00AB_0019_0000_0001_u64;
    let still = aestra_runtime::HomingTarget {
        position: [0.0, 3.0, 0.0],
        velocity: [0.0; 3],
    };
    let config = homing_config(HomingLostPolicy::KeepDirection, 1.5);
    let homing = config.homing.unwrap();
    let pack = |count: bool| {
        let mut words = stateful_params(&config, seed, 0, 0);
        aestra_gpu::pack_stateful_homing_counted(
            Some(&homing),
            Some(&still),
            count.then_some(3),
            &mut words,
        );
        words
    };
    let ticks: Vec<Vec<u32>> = (0..120).map(|_| pack(true)).collect();
    let (gpu, counters) = advance_production_counted(&harness, &config, &ticks).unwrap();
    let mut simulation = StatefulSimulation::new(config, seed);
    simulation.set_homing_target(Some(still));
    simulation.advance_to_tick(120);
    assert_same_particles(&simulation.alive_particles(), &gpu);
    assert!(simulation.arrivals() > 100, "{}", simulation.arrivals());
    assert_eq!(u64::from(counters[3]), simulation.arrivals());
    let silent: Vec<Vec<u32>> = (0..120).map(|_| pack(false)).collect();
    let (_, counters) = advance_production_counted(&harness, &config, &silent).unwrap();
    assert_eq!(counters[3], 0, "a replay raises nothing");
}

// ---- Particle event links (host bindings HB9b) ----

#[test]
fn event_link_list_capacity_follows_captured_sources_and_has_a_finite_ceiling() {
    use aestra_bevy_render::execution::EventGatherPipeline;
    assert_eq!(EventGatherPipeline::list_capacity(64, 64), 4096);
    assert_eq!(EventGatherPipeline::list_capacity(16, 64), 1024);
    assert_eq!(EventGatherPipeline::list_capacity(4096, 800), 819_200);
    assert_eq!(EventGatherPipeline::list_bytes(819_200), 26_214_416);
}

#[test]
fn gpu_event_gather_sizes_the_link_list_and_counts_only_actual_overflow() {
    use aestra_bevy_render::execution::{
        EventEmissionList, EventGatherPipeline, EventLinkCounters,
    };
    let Some(harness) = require_harness() else {
        return;
    };
    let device = &harness.device;
    let gather = EventGatherPipeline::new(device);
    let mut link = aestra_runtime::CompiledEventLink {
        source: 0,
        trigger: aestra_core::EventTrigger::OnDeath,
        target: 1,
        count: 64,
        inherit: 0.0,
    };
    for (sources, fanout, list_capacity, expected_kept, expected_dropped) in [
        (1, 800, 800, 800, 0),
        (16, 64, 1024, 1024, 0),
        (64, 64, 4096, 4096, 0),
        (64, 64, 1024, 1024, 3072),
        (1024, 64, 65_536, 65_536, 0),
    ] {
        link.count = fanout;
        let mut words = vec![0_u32; aestra_gpu::particle_event_words(1024)];
        words[0] = sources;
        for ordinal in 0..sources {
            let offset = 4 + ordinal as usize * 8;
            let reverse_ordinal = sources - 1 - ordinal;
            words[offset] = aestra_runtime::event_trigger_bit(link.trigger);
            words[offset + 1] = reverse_ordinal;
            words[offset + 2] = (reverse_ordinal as f32).to_bits();
        }
        let events = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("event overflow probe source"),
            contents: &encode(&words).unwrap(),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let list = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("event overflow probe list"),
            contents: &vec![0_u8; EventGatherPipeline::list_bytes(list_capacity) as usize],
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        });
        let counters = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("event overflow probe counters"),
            contents: &[0_u8; 12],
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("event overflow probe readback"),
            size: 24,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        gather.encode_with_overflow(
            device,
            &mut encoder,
            &events,
            EventEmissionList {
                buffer: &list,
                capacity: list_capacity,
            },
            &link,
            EventLinkCounters {
                buffer: &counters,
                requested_word: 1,
                dropped_word: 2,
            },
        );
        encoder.copy_buffer_to_buffer(&counters, 0, &staging, 0, 12);
        encoder.copy_buffer_to_buffer(&list, 0, &staging, 12, 4);
        encoder.copy_buffer_to_buffer(&list, 16, &staging, 16, 4);
        encoder.copy_buffer_to_buffer(
            &list,
            16 + u64::from(expected_kept - 1) * 32,
            &staging,
            20,
            4,
        );
        let result = harness.read_back_u32(encoder, &staging).unwrap();
        assert_eq!(
            result,
            vec![
                0,
                sources * fanout,
                expected_dropped,
                expected_kept,
                0.0_f32.to_bits(),
                (((expected_kept - 1) / fanout) as f32).to_bits(),
            ]
        );
    }
}

/// One emitter's live `(ordinal, position)` set.
type LiveParticles = Vec<(u64, [f32; 3])>;

/// Runs several emitters in lockstep through the *production* kernels, as the Bevy backend does for an
/// effect with event links: each tick every emitter's `death_integrate` + `spawn` (its event buffer
/// cleared first, its params asking for the triggers its links read), then each link's gather and
/// event spawn into its target, then the tick's input bursts and particle output aggregations (event
/// system E3), each output route's record read back every tick. Returns each emitter's live
/// `(ordinal, position)` set, and the outputs raised in tick order.
fn advance_production_linked(
    harness: &Harness,
    configs: &[StatefulConfig],
    links: &[aestra_runtime::CompiledEventLink],
    bursts: &[aestra_runtime::InputSpawnBurst],
    outputs: &[aestra_runtime::CompiledParticleOutput],
    seed: u64,
    ticks: u32,
) -> Result<(Vec<LiveParticles>, Vec<aestra_runtime::EffectOutputEvent>), String> {
    use aestra_bevy_render::execution::{
        DomainSpawnPipeline, EventEmissionList, EventGatherPipeline, ParticleOutputSlot, SpawnState,
    };
    let device = &harness.device;
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("linked"),
        bind_group_layouts: &[Some(&harness.unified_layout)],
        immediate_size: 0,
    });
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("linked"),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(stateful_simulation_wgsl())),
    });
    let pipeline = |entry: &str| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let (death, spawn) = (pipeline("death_integrate"), pipeline("spawn"));
    let gather = EventGatherPipeline::new(device);
    let spawner = DomainSpawnPipeline::new(device);
    let buffer = |label: &str, bytes: Vec<u8>, usage: wgpu::BufferUsages| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: &bytes,
            usage: wgpu::BufferUsages::STORAGE | usage,
        })
    };
    let masks: Vec<u32> = (0..configs.len())
        .map(|index| {
            links
                .iter()
                .filter(|link| link.source == index)
                .map(|link| link.trigger)
                .chain(
                    outputs
                        .iter()
                        .filter(|route| route.source == index)
                        .map(|route| route.trigger),
                )
                .fold(0, |mask, trigger| {
                    mask | aestra_runtime::event_trigger_bit(trigger)
                })
        })
        .collect();
    struct Emitter {
        state: wgpu::Buffer,
        free_list: wgpu::Buffer,
        free_count: wgpu::Buffer,
        spawn_counter: wgpu::Buffer,
        scratch: [wgpu::Buffer; 4],
        events: wgpu::Buffer,
    }
    let event_bytes = EventGatherPipeline::buffer_bytes() as usize;
    let emitters: Vec<Emitter> = configs
        .iter()
        .map(|config| -> Result<Emitter, String> {
            let capacity = config.capacity;
            let scratch =
                |words: usize| buffer("scratch", vec![0u8; words * 4], wgpu::BufferUsages::empty());
            Ok(Emitter {
                state: buffer(
                    "state",
                    encode(&vec![0.0_f32; capacity as usize * 9])?,
                    wgpu::BufferUsages::COPY_SRC,
                ),
                free_list: buffer(
                    "free list",
                    encode(&(0..capacity).collect::<Vec<u32>>())?,
                    wgpu::BufferUsages::empty(),
                ),
                free_count: buffer(
                    "free count",
                    encode(&capacity)?,
                    wgpu::BufferUsages::empty(),
                ),
                spawn_counter: buffer(
                    "spawn counter",
                    encode(&0_u32)?,
                    wgpu::BufferUsages::empty(),
                ),
                scratch: [
                    scratch(capacity as usize * 12),
                    scratch(capacity as usize),
                    scratch(8),
                    scratch(4),
                ],
                events: buffer(
                    "events",
                    vec![0u8; event_bytes],
                    wgpu::BufferUsages::COPY_DST,
                ),
            })
        })
        .collect::<Result<_, _>>()?;
    let lists: Vec<wgpu::Buffer> = links
        .iter()
        .map(|link| {
            let list_capacity =
                EventGatherPipeline::list_capacity(configs[link.source].capacity, link.count);
            buffer(
                "event list",
                vec![0u8; EventGatherPipeline::list_bytes(list_capacity) as usize],
                wgpu::BufferUsages::COPY_DST,
            )
        })
        .collect();
    let emitter_seed = |index: usize| seed ^ (index as u64).wrapping_mul(0x9E37_79B9);
    let ring_bytes = u64::from(aestra_gpu::PARTICLE_OUTPUT_RING_WORDS) * 4;
    let rings = buffer(
        "output rings",
        vec![0u8; (ring_bytes as usize) * outputs.len().max(1)],
        wgpu::BufferUsages::COPY_SRC,
    );
    let rings_staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("output rings readback"),
        size: ring_bytes * outputs.len().max(1) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut raised = Vec::new();
    let mut encoder = device.create_command_encoder(&Default::default());
    for tick in 0..ticks {
        let mut params = Vec::new();
        for (index, (config, emitter)) in configs.iter().zip(&emitters).enumerate() {
            let mut words = stateful_params(config, emitter_seed(index), index as u32, 0);
            aestra_gpu::pack_stateful_events(
                masks[index],
                aestra_runtime::PARTICLE_EVENT_CAPACITY,
                &mut words,
            );
            let tick_params = buffer("params", encode(&words)?, wgpu::BufferUsages::empty());
            encoder.clear_buffer(&emitter.events, 0, Some(4));
            let entries: Vec<wgpu::BindGroupEntry> = [
                &emitter.state,
                &emitter.free_list,
                &emitter.free_count,
                &emitter.spawn_counter,
                &tick_params,
                &emitter.scratch[0],
                &emitter.scratch[1],
                &emitter.scratch[2],
                &emitter.scratch[3],
                &emitter.events,
                &harness.no_world,
                &harness.no_physics,
            ]
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("linked tick"),
                layout: &harness.unified_layout,
                entries: &entries,
            });
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_bind_group(0, &group, &[]);
                pass.set_pipeline(&death);
                pass.dispatch_workgroups(config.capacity.div_ceil(WORKGROUP), 1, 1);
                pass.set_pipeline(&spawn);
                pass.dispatch_workgroups(config.spawn_per_tick.div_ceil(WORKGROUP).max(1), 1, 1);
            }
            params.push(tick_params);
        }
        for (link, list) in links.iter().zip(&lists) {
            let list_capacity =
                EventGatherPipeline::list_capacity(configs[link.source].capacity, link.count);
            gather.encode(
                device,
                &mut encoder,
                &emitters[link.source].events,
                EventEmissionList {
                    buffer: list,
                    capacity: list_capacity,
                },
                link,
            );
            let target = &emitters[link.target];
            spawner.encode(
                device,
                &mut encoder,
                SpawnState {
                    state: &target.state,
                    free_list: &target.free_list,
                    free_count: &target.free_count,
                    spawn_counter: &target.spawn_counter,
                    params: &params[link.target],
                },
                list,
                &EventGatherPipeline::spawn(link, list_capacity),
            );
        }
        // Then the tick's input route bursts (event system E3), as production uploads them.
        for burst in bursts.iter().filter(|burst| burst.tick == u64::from(tick)) {
            let (words, spawn) = aestra_bevy_render::execution::input_burst_list(burst);
            let list = buffer("input burst", encode(&words)?, wgpu::BufferUsages::empty());
            let target = &emitters[burst.target];
            spawner.encode(
                device,
                &mut encoder,
                SpawnState {
                    state: &target.state,
                    free_list: &target.free_list,
                    free_count: &target.free_count,
                    spawn_counter: &target.spawn_counter,
                    params: &params[burst.target],
                },
                &list,
                &spawn,
            );
        }
        // Particle output routes, reading the tick's events, into each route's ring.
        for (index, route) in outputs.iter().enumerate() {
            gather.encode_output(
                device,
                &mut encoder,
                &emitters[route.source].events,
                ParticleOutputSlot {
                    counters: &rings,
                    ring: index as u32 * aestra_gpu::PARTICLE_OUTPUT_RING_WORDS,
                    tick: tick + 1,
                },
                route,
            );
        }
        // Submit tick by tick, so the per-tick buffers and bind groups are released as we go.
        if outputs.is_empty() {
            harness.queue.submit(Some(encoder.finish()));
        } else {
            encoder.copy_buffer_to_buffer(&rings, 0, &rings_staging, 0, rings_staging.size());
            let words = harness.read_back_u32(encoder, &rings_staging)?;
            for (index, route) in outputs.iter().enumerate() {
                let slot = index * aestra_gpu::PARTICLE_OUTPUT_RING_WORDS as usize
                    + ((tick + 1) % aestra_gpu::PARTICLE_OUTPUT_RING_TICKS) as usize
                        * aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize;
                let record = &words[slot..slot + aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize];
                let read = aestra_gpu::read_particle_output_slot(record)
                    .ok_or("every tick writes its record")?;
                raised.extend(route.raise(read.count, &read.first, read.tick));
            }
        }
        encoder = device.create_command_encoder(&Default::default());
    }
    let sizes: Vec<u64> = configs
        .iter()
        .map(|config| config.capacity as u64 * 36)
        .collect();
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("linked readback"),
        size: sizes.iter().sum(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut offset = 0;
    for (emitter, size) in emitters.iter().zip(&sizes) {
        encoder.copy_buffer_to_buffer(&emitter.state, 0, &staging, offset, *size);
        offset += size;
    }
    let raw = harness.read_back_u32(encoder, &staging)?;
    let mut live = Vec::new();
    let mut at = 0;
    for config in configs {
        let mut particles = Vec::new();
        for slot in 0..config.capacity as usize {
            let base = at + slot * 9;
            let (age, lifetime) = (f32::from_bits(raw[base + 6]), f32::from_bits(raw[base + 7]));
            if lifetime > 0.0 && age < lifetime {
                particles.push((
                    raw[base + 8] as u64,
                    [
                        f32::from_bits(raw[base]),
                        f32::from_bits(raw[base + 1]),
                        f32::from_bits(raw[base + 2]),
                    ],
                ));
            }
        }
        at += config.capacity as usize * 9;
        live.push(particles);
    }
    Ok((live, raised))
}

#[test]
fn gpu_two_generation_death_chain_preserves_parent_positions_and_velocity() {
    use aestra_core::{EventTrigger, VelocityDistribution};
    use aestra_runtime::{CompiledEventLink, InputSpawnBurst, ParticleEvent};
    let Some(harness) = require_harness() else {
        return;
    };
    // One externally launched rocket, then 64 parents, then 8 sparks per
    // parent. Only ordinary death events spawn the two subsequent generations.
    let rocket = StatefulConfig {
        gravity: [0.0, -9.0, 0.0],
        spawn_per_tick: 0,
        speed: (24.0, 24.0),
        lifetime: (0.5, 0.5),
        direction: [0.0, 1.0, 0.0],
        spread: 0.0,
        velocity_distribution: VelocityDistribution::LegacyCone,
        drag: 0.0,
        shape: SpawnShape::Point,
        turbulence: 0.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 1,
        homing: None,
    };
    let configs = [
        rocket,
        StatefulConfig {
            speed: (18.0, 22.0),
            velocity_distribution: VelocityDistribution::Sphere,
            drag: 0.45,
            capacity: 64,
            ..rocket
        },
        StatefulConfig {
            speed: (3.0, 6.0),
            lifetime: (1.0, 1.0),
            velocity_distribution: VelocityDistribution::Sphere,
            drag: 0.6,
            capacity: 512,
            ..rocket
        },
    ];
    let links = [
        CompiledEventLink {
            source: 0,
            trigger: EventTrigger::OnDeath,
            target: 1,
            count: 64,
            inherit: 0.02,
        },
        CompiledEventLink {
            source: 1,
            trigger: EventTrigger::OnDeath,
            target: 2,
            count: 8,
            inherit: 0.25,
        },
    ];
    let bursts = [InputSpawnBurst {
        tick: 0,
        route: 0,
        target: 0,
        count: 1,
        events: vec![ParticleEvent {
            ordinal: 0,
            position: [12.0, 4.0, -8.0],
            velocity: [0.0; 3],
        }],
    }];
    let seed = 0xf1e0_0000_0000_0001;
    let ticks = 65;
    let mut sims: Vec<_> = configs
        .iter()
        .enumerate()
        .map(|(i, config)| {
            StatefulSimulation::new(*config, seed ^ (i as u64).wrapping_mul(0x9E37_79B9))
        })
        .collect();
    let mut deaths = [0; 2];
    let mut parent_positions = Vec::new();
    for tick in 0..ticks {
        for sim in &mut sims {
            sim.advance_tick();
        }
        for (i, link) in links.iter().enumerate() {
            let events = sims[link.source].events(link.trigger).to_vec();
            deaths[i] += events.len();
            let before = sims[link.target].live_count();
            sims[link.target].spawn_from_events(&events, link.count, link.inherit);
            assert_eq!(
                sims[link.target].live_count() - before,
                events.len() * link.count as usize
            );
            if i == 1 && !events.is_empty() {
                parent_positions.extend(events.iter().map(|event| event.position));
                // At birth there is no integration yet; every child must be at
                // one real parent's final position, not the authored origin.
                for (_, position) in sims[2].alive_particles() {
                    assert!(events.iter().any(|event| event.position == position));
                }
            }
        }
        for burst in bursts.iter().filter(|burst| burst.tick == u64::from(tick)) {
            sims[burst.target].spawn_from_events(&burst.events, burst.count, 0.0);
        }
    }
    assert_eq!(deaths, [1, 64]);
    assert_eq!(parent_positions.len(), 64);
    assert!(parent_positions.iter().any(|p| (p[0] - 12.0).abs() > 1.0));
    assert!(parent_positions.iter().all(|p| p[1] > 4.0));
    let gpu = advance_production_linked(&harness, &configs, &links, &bursts, &[], seed, ticks)
        .unwrap()
        .0;
    assert_eq!(gpu.iter().map(Vec::len).collect::<Vec<_>>(), [0, 0, 512]);
    for (sim, particles) in sims.iter().zip(&gpu) {
        assert_same_particles(&sim.alive_particles(), particles);
    }
    // Death events are produced before routing: link declaration order must
    // not delay a second-generation birth by a tick.
    let reversed = [links[1], links[0]];
    let reordered =
        advance_production_linked(&harness, &configs, &reversed, &bursts, &[], seed, ticks)
            .unwrap()
            .0;
    assert_same_particles(&gpu[2], &reordered[2]);
    let mut no_inheritance = links;
    no_inheritance[1].inherit = 0.0;
    let independent = advance_production_linked(
        &harness,
        &configs,
        &no_inheritance,
        &bursts,
        &[],
        seed,
        ticks,
    )
    .unwrap()
    .0;
    assert!(
        gpu[2].iter().any(|(ordinal, position)| {
            let other = independent[2]
                .iter()
                .find(|(id, _)| id == ordinal)
                .unwrap()
                .1;
            position
                .iter()
                .zip(other)
                .any(|(a, b)| (a - b).abs() > 0.01)
        }),
        "inherited parent velocity must change subsequent spark motion"
    );
    // No checkpoints/restarts/seeks anywhere in this helper's forward loop.
    let finished = advance_production_linked(&harness, &configs, &links, &bursts, &[], seed, 180)
        .unwrap()
        .0;
    assert!(finished.iter().all(Vec::is_empty));
}

#[test]
fn gpu_event_links_spawn_sub_emitters_like_the_cpu_reference() {
    // A fountain bouncing on the ground feeds three sub-emitters: a splash per bounce (OnCollision,
    // two particles, inheriting some velocity), a puff per death (OnDeath, three, capped by a small
    // capacity), and a spark per spawn (OnSpawn, inheriting all of it). Every particle of every
    // emitter matches the CPU reference by ordinal, whatever order the GPU raised the events in.
    use aestra_core::EventTrigger;
    use aestra_runtime::CompiledEventLink;
    let Some(harness) = require_harness() else {
        return;
    };
    let ground = Collider {
        shape: ColliderShape::Plane {
            normal: [0.0, 1.0, 0.0],
            distance: 0.0,
        },
        restitution: 0.5,
        friction: 0.2,
        kill: false,
    };
    let mut colliders = [Collider::NONE; MAX_COLLIDERS];
    colliders[0] = ground;
    let fountain = StatefulConfig {
        gravity: [0.0, -18.0, 0.0],
        spawn_per_tick: 4,
        speed: (6.0, 10.0),
        lifetime: (1.0, 1.3),
        direction: [0.0, 1.0, 0.0],
        spread: 0.4,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.1,
        shape: SpawnShape::Sphere { radius: 0.5 },
        turbulence: 2.0,
        placement: SpawnPlacement::IDENTITY,
        colliders,
        collider_count: 1,
        capacity: 512,
        homing: None,
    };
    let sub = |capacity: u32| StatefulConfig {
        gravity: [0.0, -4.0, 0.0],
        spawn_per_tick: 0,
        speed: (1.0, 2.0),
        lifetime: (0.3, 0.5),
        spread: 1.0,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        turbulence: 0.0,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity,
        ..fountain
    };
    let configs = [
        fountain,
        StatefulConfig {
            velocity_distribution: aestra_core::VelocityDistribution::Ring,
            ..sub(2048)
        },
        StatefulConfig {
            velocity_distribution: aestra_core::VelocityDistribution::Sphere,
            ..sub(64)
        },
        StatefulConfig {
            velocity_distribution: aestra_core::VelocityDistribution::Cone,
            spread: 60.0,
            ..sub(1024)
        },
    ];
    let link = |trigger, target, count, inherit| CompiledEventLink {
        source: 0,
        trigger,
        target,
        count,
        inherit,
    };
    let links = [
        link(EventTrigger::OnCollision, 1, 2, 0.3),
        link(EventTrigger::OnDeath, 2, 3, 0.0),
        link(EventTrigger::OnSpawn, 3, 1, 1.0),
    ];
    let seed = 0x00AB_009B_0000_0001_u64;
    let ticks = 150;
    let gpu = advance_production_linked(&harness, &configs, &links, &[], &[], seed, ticks)
        .unwrap()
        .0;

    let mut sims: Vec<StatefulSimulation> = configs
        .iter()
        .enumerate()
        .map(|(index, config)| {
            StatefulSimulation::new(*config, seed ^ (index as u64).wrapping_mul(0x9E37_79B9))
        })
        .collect();
    let mut raised = [0usize; 3];
    for _ in 0..ticks {
        for sim in &mut sims {
            sim.advance_tick();
        }
        for (index, link) in links.iter().enumerate() {
            let events = sims[link.source].events(link.trigger).to_vec();
            raised[index] += events.len();
            sims[link.target].spawn_from_events(&events, link.count, link.inherit);
        }
    }
    eprintln!("events raised per link: {raised:?}");
    assert!(raised.iter().all(|count| *count > 50), "{raised:?}");
    for (index, sim) in sims.iter().enumerate() {
        let cpu = sim.alive_particles();
        assert!(!cpu.is_empty(), "emitter {index} has particles");
        assert_same_particles(&cpu, &gpu[index]);
    }
    let replay = advance_production_linked(&harness, &configs, &links, &[], &[], seed, ticks)
        .unwrap()
        .0;
    for (index, (first, second)) in gpu.iter().zip(&replay).enumerate() {
        let canonical = |particles: &LiveParticles| {
            let mut by_ordinal: Vec<_> = particles
                .iter()
                .map(|(ordinal, position)| (*ordinal, position.map(f32::to_bits)))
                .collect();
            by_ordinal.sort_unstable_by_key(|(ordinal, _)| *ordinal);
            by_ordinal
        };
        assert_eq!(
            canonical(first),
            canonical(second),
            "emitter {index} diverged across fresh GPU replays"
        );
    }
    assert_eq!(
        sims[2].alive_particles().len(),
        64,
        "the puffs fill their small capacity and the rest are dropped alike"
    );
}

#[test]
fn gpu_input_bursts_spawn_like_the_cpu_reference() {
    // Event system E3 (Scenario C): a host detonates a mine — once, then twice in one tick, beyond
    // the shrapnel's free room — while the fountain's deaths keep feeding a link into the same
    // emitter. Every burst spawns after its tick's links, at the payload positions, exactly as in
    // the CPU reference, by ordinal.
    use aestra_runtime::{CompiledEventLink, InputSpawnBurst, ParticleEvent};
    let Some(harness) = require_harness() else {
        return;
    };
    let fountain = StatefulConfig {
        gravity: [0.0, -18.0, 0.0],
        spawn_per_tick: 3,
        speed: (6.0, 10.0),
        lifetime: (0.8, 1.0),
        direction: [0.0, 1.0, 0.0],
        spread: 0.4,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.1,
        shape: SpawnShape::Sphere { radius: 0.5 },
        turbulence: 1.0,
        placement: SpawnPlacement::IDENTITY,
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 256,
        homing: None,
    };
    let shrapnel = StatefulConfig {
        gravity: [0.0, -9.0, 0.0],
        spawn_per_tick: 0,
        speed: (4.0, 8.0),
        lifetime: (0.3, 0.5),
        spread: 1.0,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        turbulence: 0.0,
        capacity: 96,
        ..fountain
    };
    let configs = [fountain, shrapnel];
    let links = [CompiledEventLink {
        source: 0,
        trigger: aestra_core::EventTrigger::OnDeath,
        target: 1,
        count: 1,
        inherit: 0.5,
    }];
    let burst = |tick, positions: &[[f32; 3]]| InputSpawnBurst {
        tick,
        route: 0,
        target: 1,
        count: 40,
        events: positions
            .iter()
            .enumerate()
            .map(|(ordinal, position)| ParticleEvent {
                ordinal: ordinal as u64,
                position: *position,
                velocity: [0.0; 3],
            })
            .collect(),
    };
    let bursts = [
        burst(20, &[[2.0, 6.0, 0.0]]),
        burst(40, &[[-3.0, 4.0, 1.0], [0.0, 8.0, -2.0]]),
    ];
    let seed = 0x00E3_0000_0000_0003_u64;
    let ticks = 70;
    let gpu = advance_production_linked(&harness, &configs, &links, &bursts, &[], seed, ticks)
        .unwrap()
        .0;

    let mut sims: Vec<StatefulSimulation> = configs
        .iter()
        .enumerate()
        .map(|(index, config)| {
            StatefulSimulation::new(*config, seed ^ (index as u64).wrapping_mul(0x9E37_79B9))
        })
        .collect();
    let mut capped = false;
    for tick in 0..u64::from(ticks) {
        for sim in &mut sims {
            sim.advance_tick();
        }
        for link in &links {
            let events = sims[link.source].events(link.trigger).to_vec();
            sims[link.target].spawn_from_events(&events, link.count, link.inherit);
        }
        for burst in bursts.iter().filter(|burst| burst.tick == tick) {
            let before = sims[burst.target].live_count();
            sims[burst.target].spawn_from_events(&burst.events, burst.count, 0.0);
            capped |= sims[burst.target].live_count() - before < burst.records().count();
        }
    }
    assert!(capped, "the double detonation exceeds the shrapnel's room");
    for (index, sim) in sims.iter().enumerate() {
        let cpu = sim.alive_particles();
        assert!(!cpu.is_empty(), "emitter {index} has particles");
        assert_same_particles(&cpu, &gpu[index]);
    }
}

#[test]
fn gpu_particle_outputs_raise_the_cpu_reference_stream() {
    // Event system E3 (Rocket.OnDeath → Exploded, first per tick): rockets die and bounce while a
    // link bursts sparks from each death. Each tick's deaths raise one `Exploded` at the first dead
    // rocket, and each bounce up to three `Bounced`, lowest ordinals first; the kernels raise
    // exactly the CPU reference's stream, tick by tick.
    use aestra_core::{EventAggregation, EventTrigger};
    use aestra_runtime::{CompiledEventLink, CompiledParticleOutput, first_particle_events};
    let Some(harness) = require_harness() else {
        return;
    };
    let mut colliders = [Collider::NONE; MAX_COLLIDERS];
    colliders[0] = Collider {
        shape: ColliderShape::Plane {
            normal: [0.0, 1.0, 0.0],
            distance: 0.0,
        },
        restitution: 0.4,
        friction: 0.2,
        kill: false,
    };
    let rockets = StatefulConfig {
        gravity: [0.0, -20.0, 0.0],
        spawn_per_tick: 2,
        speed: (8.0, 14.0),
        lifetime: (0.6, 1.1),
        direction: [0.0, 1.0, 0.0],
        spread: 0.5,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.05,
        shape: SpawnShape::Sphere { radius: 0.5 },
        turbulence: 1.0,
        placement: SpawnPlacement::IDENTITY,
        colliders,
        collider_count: 1,
        capacity: 256,
        homing: None,
    };
    let sparks = StatefulConfig {
        spawn_per_tick: 0,
        lifetime: (0.2, 0.4),
        colliders: [Collider::NONE; MAX_COLLIDERS],
        collider_count: 0,
        capacity: 512,
        ..rockets
    };
    let configs = [rockets, sparks];
    let links = [CompiledEventLink {
        source: 0,
        trigger: EventTrigger::OnDeath,
        target: 1,
        count: 6,
        inherit: 0.3,
    }];
    let outputs = [
        CompiledParticleOutput {
            output: "Exploded".into(),
            source: 0,
            trigger: EventTrigger::OnDeath,
            aggregation: EventAggregation::FirstPerTick,
        },
        CompiledParticleOutput {
            output: "Bounced".into(),
            source: 0,
            trigger: EventTrigger::OnCollision,
            aggregation: EventAggregation::EachEvent { limit: 3 },
        },
    ];
    let seed = 0x00E3_B000_0000_0001_u64;
    let ticks = 110;
    let (gpu_particles, gpu) =
        advance_production_linked(&harness, &configs, &links, &[], &outputs, seed, ticks).unwrap();

    let mut sims: Vec<StatefulSimulation> = configs
        .iter()
        .enumerate()
        .map(|(index, config)| {
            StatefulSimulation::new(*config, seed ^ (index as u64).wrapping_mul(0x9E37_79B9))
        })
        .collect();
    let mut cpu = Vec::new();
    for _ in 0..ticks {
        for sim in &mut sims {
            sim.advance_tick();
        }
        for link in &links {
            let events = sims[link.source].events(link.trigger).to_vec();
            sims[link.target].spawn_from_events(&events, link.count, link.inherit);
        }
        for route in &outputs {
            let source = &sims[route.source];
            let events = source.events(route.trigger);
            let first = first_particle_events(events, route.aggregation.limit());
            cpu.extend(route.raise(events.len() as u32, &first, source.tick()));
        }
    }
    let kinds = |stream: &[aestra_runtime::EffectOutputEvent], kind: &str| {
        stream.iter().filter(|event| event.kind == kind).count()
    };
    assert!(kinds(&cpu, "Exploded") > 10, "{}", kinds(&cpu, "Exploded"));
    assert!(kinds(&cpu, "Bounced") > 10, "{}", kinds(&cpu, "Bounced"));
    assert!(
        cpu.iter()
            .any(|event| event.kind == "Bounced" && event.magnitude > 3.0),
        "some tick bounces more rockets than the route raises"
    );
    // The same outputs, in the same order; positions agree to float rounding, as particles do.
    let exact = |stream: &[aestra_runtime::EffectOutputEvent]| {
        stream
            .iter()
            .map(|event| {
                (
                    event.kind.clone(),
                    event.tick,
                    event.magnitude,
                    event.origin,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(exact(&gpu), exact(&cpu));
    for (gpu, cpu) in gpu.iter().zip(&cpu) {
        assert!(
            gpu.value
                .iter()
                .zip(&cpu.value)
                .all(|(a, b)| (a - b).abs() <= 1e-4 * (1.0 + b.abs())),
            "{gpu:?} vs {cpu:?}"
        );
    }
    for (index, sim) in sims.iter().enumerate() {
        assert_same_particles(&sim.alive_particles(), &gpu_particles[index]);
    }
}

// ---- World collision (host bindings HB10) ----

/// A world of a ground at y = 0 with a spherical bump at the origin, as a signed distance volume.
fn ground_with_bump() -> aestra_runtime::SdfVolume {
    let (dims, origin, voxel) = ([24u32, 12, 24], [-24.0f32, -6.0, -24.0], 2.0f32);
    let mut distances = Vec::new();
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let p = [
                    origin[0] + (x as f32 + 0.5) * voxel,
                    origin[1] + (y as f32 + 0.5) * voxel,
                    origin[2] + (z as f32 + 0.5) * voxel,
                ];
                let bump = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt() - 5.0;
                distances.push(p[1].min(bump));
            }
        }
    }
    aestra_runtime::SdfVolume {
        dims,
        origin,
        voxel_size: voxel,
        distances,
    }
}

#[test]
fn gpu_world_colliders_match_the_cpu_reference_in_a_placed_effect() {
    // Sparks rain onto the host's world — a ground with a bump — from an effect placed in it
    // (translated, tilted, scaled): the kernel's world collider bounces them exactly as the CPU
    // reference does, by ordinal, and keeps them above the ground; without a world they fall through.
    let Some(harness) = require_harness() else {
        return;
    };
    let volume = std::sync::Arc::new(ground_with_bump());
    let (sin, cos) = (0.3f32.sin(), 0.3f32.cos());
    let scale = 1.5f32;
    // world = translate(2, 14, 1) · rotate_x(0.3) · scale(1.5).
    let world_from_effect = [
        [scale, 0.0, 0.0, 2.0],
        [0.0, scale * cos, -scale * sin, 14.0],
        [0.0, scale * sin, scale * cos, 1.0],
    ];
    let mut colliders = [Collider::NONE; MAX_COLLIDERS];
    colliders[0] = Collider {
        shape: ColliderShape::World { radius: 0.2 },
        restitution: 0.4,
        friction: 0.3,
        kill: false,
    };
    let config = StatefulConfig {
        gravity: [0.0, -12.0, 0.0],
        spawn_per_tick: 3,
        speed: (2.0, 6.0),
        lifetime: (1.6, 2.0),
        direction: [0.0, -1.0, 0.0],
        spread: 0.8,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.1,
        shape: SpawnShape::Sphere { radius: 2.0 },
        turbulence: 0.0,
        placement: SpawnPlacement::IDENTITY,
        colliders,
        collider_count: 1,
        capacity: 1024,
        homing: None,
    };
    let seed = 0x00AB_0010_0000_0001_u64;
    let ticks: Vec<Vec<u32>> = (0..110)
        .map(|_| {
            let mut words = stateful_params(&config, seed, 0, 0);
            aestra_gpu::pack_stateful_world(&world_from_effect, &mut words);
            words
        })
        .collect();
    let world = harness
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("world"),
            contents: &aestra_gpu::GpuWorldSdf::new(&volume, 1).to_bytes(),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let (gpu, _) = advance_production_in_world(&harness, &config, &ticks, &world).unwrap();
    let mut simulation = StatefulSimulation::new(config, seed);
    simulation.set_world(Some(aestra_runtime::ParticleWorld {
        volume: volume.clone(),
        world_from_effect,
    }));
    simulation.advance_to_tick(110);
    let cpu = simulation.alive_particles();
    assert!(!cpu.is_empty());
    assert_same_particles(&cpu, &gpu);

    // They rest on the world: every particle's world position is above the ground and the bump.
    let in_world = |p: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|i| {
            let r = world_from_effect[i];
            r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3]
        })
    };
    let lowest = cpu
        .iter()
        .map(|(_, p)| volume.sample(in_world(*p)))
        .fold(f32::MAX, f32::min);
    assert!(lowest > -0.05, "a particle sank {lowest} into the world");
    // Without a world they fall through the ground.
    let (fallen, _) =
        advance_production_in_world(&harness, &config, &ticks, &harness.no_world).unwrap();
    assert!(
        fallen.iter().any(|(_, p)| in_world(*p)[1] < -1.0),
        "without a world nothing stops them"
    );
}

#[test]
fn gpu_physics_colliders_match_the_cpu_reference_against_every_proxy_kind() {
    // The host's physics scene as proxies — a ground half-space, a ball, a slanted capsule and a
    // rotated crate — with sparks raining on them from a placed effect: the kernel's physics
    // collider resolves every contact as the CPU reference does, by ordinal.
    use aestra_runtime::{PhysicsProxy, PhysicsScene};
    let Some(harness) = require_harness() else {
        return;
    };
    let (sin, cos) = (0.35f32.sin(), 0.35f32.cos());
    let scene = std::sync::Arc::new(PhysicsScene {
        proxies: vec![
            PhysicsProxy::HalfSpace {
                normal: [0.0, 1.0, 0.0],
                distance: 0.0,
            },
            PhysicsProxy::Sphere {
                center: [-4.0, 1.5, 0.0],
                radius: 2.0,
            },
            PhysicsProxy::Capsule {
                a: [3.0, 1.0, -3.0],
                b: [5.0, 4.0, 2.0],
                radius: 1.0,
            },
            PhysicsProxy::Box {
                center: [0.5, 2.0, 3.5],
                rotation: [0.0, (0.3f32).sin(), 0.0, (0.3f32).cos()],
                half_extents: [2.0, 1.0, 1.5],
            },
        ],
    });
    let world_from_effect = [
        [1.2, 0.0, 0.0, 0.5],
        [0.0, 1.2 * cos, -1.2 * sin, 12.0],
        [0.0, 1.2 * sin, 1.2 * cos, 0.0],
    ];
    let mut colliders = [Collider::NONE; MAX_COLLIDERS];
    colliders[0] = Collider {
        shape: ColliderShape::Physics { radius: 0.15 },
        restitution: 0.35,
        friction: 0.25,
        kill: false,
    };
    let config = StatefulConfig {
        gravity: [0.0, -14.0, 0.0],
        spawn_per_tick: 4,
        speed: (1.0, 4.0),
        lifetime: (1.4, 1.8),
        direction: [0.0, -1.0, 0.0],
        spread: 1.0,
        velocity_distribution: aestra_core::VelocityDistribution::LegacyCone,
        drag: 0.1,
        shape: SpawnShape::Box {
            half_extents: [6.0, 0.5, 6.0],
        },
        turbulence: 0.0,
        placement: SpawnPlacement::IDENTITY,
        colliders,
        collider_count: 1,
        capacity: 1024,
        homing: None,
    };
    let seed = 0x00AB_0010_0000_0002_u64;
    let ticks: Vec<Vec<u32>> = (0..100)
        .map(|_| {
            let mut words = stateful_params(&config, seed, 0, 0);
            aestra_gpu::pack_stateful_world(&world_from_effect, &mut words);
            words
        })
        .collect();
    let physics = harness
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("physics"),
            contents: &aestra_gpu::pack_physics_scene(&scene)
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<u8>>(),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let (gpu, _) =
        advance_production_in_scene(&harness, &config, &ticks, &harness.no_world, &physics)
            .unwrap();
    let mut simulation = StatefulSimulation::new(config, seed);
    simulation.set_physics(Some(aestra_runtime::ParticlePhysics {
        scene: scene.clone(),
        world_from_effect,
    }));
    simulation.advance_to_tick(100);
    let cpu = simulation.alive_particles();
    assert!(cpu.len() > 100);
    assert_same_particles(&cpu, &gpu);
    // Nothing sits inside a body.
    let in_world = |p: [f32; 3]| -> [f32; 3] {
        std::array::from_fn(|i| {
            let r = world_from_effect[i];
            r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3]
        })
    };
    let deepest = cpu
        .iter()
        .map(|(_, p)| scene.distance_normal(in_world(*p)).0)
        .fold(f32::MAX, f32::min);
    assert!(deepest > -0.05, "a particle sank {deepest} into a body");
}

// ---- A host's stop_emitting / kill (event system E2b) ----

/// The production module stops spawning from a host's `stop_emitting` and retires every particle
/// silently from its `kill`, tick for tick like the CPU reference.
#[test]
fn gpu_emission_cutoffs_match_the_cpu_reference() {
    let Some(harness) = require_harness() else {
        return;
    };
    let config = collision_config([Collider::NONE; MAX_COLLIDERS], 0);
    let seed = 0x00E2_B000_0000_0001_u64;
    let cutoffs = aestra_runtime::EmissionCutoffs {
        stop_tick: Some(30),
        kill_tick: Some(70),
    };
    // As the Bevy backend packs each tick: no spawns from the stop, the kill flag from the kill.
    let params = |tick: u64| {
        let mut words = stateful_params(&config, seed, 0, 0);
        if cutoffs.stop_tick.is_some_and(|stop| tick >= stop) {
            words[1] = 0;
        }
        aestra_gpu::pack_stateful_kill(
            cutoffs.kill_tick.is_some_and(|kill| tick >= kill),
            &mut words,
        );
        words
    };
    let cpu = |ticks: u64| {
        let mut simulation = StatefulSimulation::new(config, seed);
        simulation.set_cutoffs(cutoffs);
        simulation.advance_to_tick(ticks);
        simulation.alive_particles()
    };
    for ticks in [29_u64, 50, 70, 71, 90] {
        let words: Vec<Vec<u32>> = (0..ticks).map(params).collect();
        let gpu = advance_production_ticks(&harness, &config, &words).unwrap();
        assert_same_particles(&cpu(ticks), &gpu);
    }
    // Nothing born from the stop; everything gone once the kill tick has run.
    let stopped = cpu(50);
    assert!(!stopped.is_empty());
    assert!(stopped.iter().all(|(id, _)| *id < 30 * 4), "{stopped:?}");
    assert!(!cpu(70).is_empty() && cpu(71).is_empty());
}
