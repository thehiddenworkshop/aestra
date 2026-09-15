//! Stateful GPU backend conformance (hybrid roadmap M6 + M7, first increments).
//!
//! Proves the core stateful claims on real GPU compute:
//! - **M6** — persistent per-particle state that advances *incrementally* across fixed ticks and
//!   matches the CPU reference's semi-implicit Euler integration (the same integration
//!   `aestra_runtime::StatefulSimulation` performs). The GPU keeps its state in a storage buffer
//!   across dispatches — no readback during simulation, only for the final conformance check.
//! - **M7** — a backward seek done as a GPU-resident checkpoint restore plus forward replay
//!   (snapshot and restore entirely GPU→GPU) reaches the same state as an uninterrupted forward run.
//!
//! Spawn, death, and the free list are the harder half of M6 (they need a u64 splitmix on GPU and
//! slot compaction) and are a deliberate follow-up; this increment fixes the particle set and proves
//! the integration + persistence.
//!
//! Like the other GPU conformance tests, this **skips when no compute adapter is present**, so it
//! does not run on GPU-less CI; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require a GPU.

use aestra_gpu::STATEFUL_SPAWN_RNG_WGSL;
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

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    spawn_pipeline: wgpu::ComputePipeline,
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
        Ok(Some(Self {
            device,
            queue,
            bind_group_layout,
            pipeline,
            spawn_pipeline,
        }))
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
