//! On-GPU proof that the generic staged infrastructure (M13) runs a real multi-stage, multi-resource
//! grid simulation (hybrid roadmap M14).
//!
//! Per the M14 reconciliation, Aestra ships **no first-party fluid solver and no CPU-reference fluid** —
//! the fluid solver itself is an external GPU-only plugin. This milestone is therefore the
//! *staged-infrastructure proof*: a multi-pass "smoke" workload (inject → advect → diffuse×K over a
//! velocity + density grid, plus a presentation pass that lets the fluid drive particles) drives a
//! generalized multi-pass/multi-resource staged executor, and correctness is **GPU-vs-GPU determinism**
//! (same asset + inputs → identical frames) rather than a CPU match. The workload WGSL lives here in the
//! test, not in the shipping crates.
//!
//! What this proves beyond M13 (which ran one pass iterated): heterogeneous ordered passes with
//! cross-resource reads/writes, multiple persistent resources plus a transient one, injection of
//! deterministic input, the grid driving stateful particles, deterministic fixed-step evolution,
//! multi-resource checkpoint/restore/replay, and bounded (persistent-only) checkpoint memory. Runs only
//! where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_gpu::STAGED_DIFFUSION_WGSL;
use aestra_runtime::{
    StagedDispatch, StagedPass, StagedPlan, StagedResource, StagedResourceLifetime,
};
use std::{borrow::Cow, collections::HashMap, sync::mpsc, time::Duration};
use wgpu::util::DeviceExt;

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const GPU_SUBMISSION_TIMEOUT: Duration = Duration::from_secs(120);
const GPU_MAP_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);

// Grid + injection source injected each tick, plus advection/diffusion coefficients. All test-only.
const WIDTH: u32 = 64;
const HEIGHT: u32 = 64;
const DIFFUSE_ITERS: u32 = 8;
const DIFFUSE_RATE: f32 = 0.15;
const ADVECT_DT: f32 = 1.0;
const PARTICLE_COUNT: u32 = 32;

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn u32_bytes(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Inject pass: add a radial (linear-falloff) source to the density grid. reads=[density],
/// writes=[density]; params = [width, height, cx, cy, radius, strength].
const INJECT_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> density_in: array<f32>;
@group(0) @binding(1) var<storage, read_write> density_out: array<f32>;
@group(0) @binding(2) var<storage, read> params: array<u32>;
@compute @workgroup_size(8, 8, 1)
fn inject(@builtin(global_invocation_id) gid: vec3<u32>) {
    let width = params[0]; let height = params[1];
    let x = gid.x; let y = gid.y;
    if (x >= width || y >= height) { return; }
    let cx = bitcast<f32>(params[2]); let cy = bitcast<f32>(params[3]);
    let radius = bitcast<f32>(params[4]); let strength = bitcast<f32>(params[5]);
    let dx = f32(x) - cx; let dy = f32(y) - cy;
    let d2 = dx * dx + dy * dy;
    let r2 = radius * radius;
    var add = 0.0;
    if (d2 < r2) { add = strength * (1.0 - d2 / r2); }
    let i = y * width + x;
    density_out[i] = density_in[i] + add;
}
"#;

/// Advect pass: semi-Lagrangian backtrace along the velocity field, bilinear-sampling the density.
/// reads=[velocity, density], writes=[density]; params = [width, height, dt].
const ADVECT_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> velocity: array<f32>;   // 2 per cell (vx, vy)
@group(0) @binding(1) var<storage, read> density_in: array<f32>;
@group(0) @binding(2) var<storage, read_write> density_out: array<f32>;
@group(0) @binding(3) var<storage, read> params: array<u32>;

fn wrapi(v: i32, n: i32) -> i32 { return ((v % n) + n) % n; }

fn sample_density(px: f32, py: f32, w: u32, h: u32) -> f32 {
    let x0 = floor(px); let y0 = floor(py);
    let fx = px - x0; let fy = py - y0;
    let ix0 = u32(wrapi(i32(x0), i32(w))); let iy0 = u32(wrapi(i32(y0), i32(h)));
    let ix1 = u32(wrapi(i32(x0) + 1, i32(w))); let iy1 = u32(wrapi(i32(y0) + 1, i32(h)));
    let c00 = density_in[iy0 * w + ix0];
    let c10 = density_in[iy0 * w + ix1];
    let c01 = density_in[iy1 * w + ix0];
    let c11 = density_in[iy1 * w + ix1];
    let top = c00 + (c10 - c00) * fx;
    let bot = c01 + (c11 - c01) * fx;
    return top + (bot - top) * fy;
}

@compute @workgroup_size(8, 8, 1)
fn advect(@builtin(global_invocation_id) gid: vec3<u32>) {
    let w = params[0]; let h = params[1]; let dt = bitcast<f32>(params[2]);
    let x = gid.x; let y = gid.y;
    if (x >= w || y >= h) { return; }
    let vx = velocity[2u * (y * w + x) + 0u];
    let vy = velocity[2u * (y * w + x) + 1u];
    let px = f32(x) - vx * dt;
    let py = f32(y) - vy * dt;
    density_out[y * w + x] = sample_density(px, py, w, h);
}
"#;

/// Presentation pass — the fluid drives particles: each particle bilinear-samples the density grid into
/// a brightness. reads=[density, particles], writes=[brightness]; params = [width, height, count].
const SAMPLE_WGSL: &str = r#"
@group(0) @binding(0) var<storage, read> density: array<f32>;
@group(0) @binding(1) var<storage, read> particles: array<f32>;   // 2 per particle (x, y)
@group(0) @binding(2) var<storage, read_write> brightness: array<f32>;
@group(0) @binding(3) var<storage, read> params: array<u32>;

fn wrapi(v: i32, n: i32) -> i32 { return ((v % n) + n) % n; }

fn sample_density(px: f32, py: f32, w: u32, h: u32) -> f32 {
    let x0 = floor(px); let y0 = floor(py);
    let fx = px - x0; let fy = py - y0;
    let ix0 = u32(wrapi(i32(x0), i32(w))); let iy0 = u32(wrapi(i32(y0), i32(h)));
    let ix1 = u32(wrapi(i32(x0) + 1, i32(w))); let iy1 = u32(wrapi(i32(y0) + 1, i32(h)));
    let c00 = density[iy0 * w + ix0];
    let c10 = density[iy0 * w + ix1];
    let c01 = density[iy1 * w + ix0];
    let c11 = density[iy1 * w + ix1];
    let top = c00 + (c10 - c00) * fx;
    let bot = c01 + (c11 - c01) * fx;
    return top + (bot - top) * fy;
}

@compute @workgroup_size(64, 1, 1)
fn sample(@builtin(global_invocation_id) gid: vec3<u32>) {
    let count = params[2];
    let i = gid.x;
    if (i >= count) { return; }
    let w = params[0]; let h = params[1];
    let px = particles[2u * i + 0u];
    let py = particles[2u * i + 1u];
    brightness[i] = sample_density(px, py, w, h);
}
"#;

/// One resource of the staged executor: 1 buffer, or 2 for a ping-pong resource with a front index.
struct Res {
    buffers: Vec<wgpu::Buffer>,
    ping_pong: bool,
    front: usize,
    persistent: bool,
    bytes: u64,
}

impl Res {
    fn read_buffer(&self) -> wgpu::Buffer {
        self.buffers[self.front].clone()
    }
    fn write_buffer(&self) -> wgpu::Buffer {
        if self.ping_pong {
            self.buffers[1 - self.front].clone()
        } else {
            self.buffers[self.front].clone()
        }
    }
    fn swap(&mut self) {
        if self.ping_pong {
            self.front = 1 - self.front;
        }
    }
}

/// A generalized multi-pass / multi-resource staged executor (hybrid roadmap M14). Builds a bind group
/// per pass by the convention `[reads (read-only) …, writes (read-write) …, params (read-only)]`, runs
/// each pass (its own compute pass — an ordered barrier) for its iteration count, and swaps written
/// ping-pong resources. Persistent resources are checkpointable; transient ones are not.
struct FluidHarness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    resources: HashMap<String, Res>,
    pipelines: HashMap<String, wgpu::ComputePipeline>,
    plan: StagedPlan,
}

impl FluidHarness {
    fn new() -> Result<Option<Self>, String> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_descriptor);
        let adapter = match pollster::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            },
        )) {
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
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Aestra staged fluid device"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|error| error.to_string())?;

        let plan = smoke_plan();
        plan.validate().expect("smoke plan is valid");

        // Build a pipeline per pass entry point from its own WGSL module (each pass has its own binding
        // layout, so each needs its own module).
        let mut pipelines = HashMap::new();
        let sample = sample_pass();
        for (entry, wgsl) in [
            ("inject", INJECT_WGSL),
            ("advect", ADVECT_WGSL),
            ("diffuse", STAGED_DIFFUSION_WGSL),
            ("sample", SAMPLE_WGSL),
        ] {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(entry),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
            });
            let pass = plan
                .passes
                .iter()
                .chain(std::iter::once(&sample))
                .find(|p| p.entry_point == entry)
                .expect("pass exists for entry");
            let layout = device.create_bind_group_layout(&pass_layout_descriptor(pass));
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            });
            pipelines.insert(entry.to_string(), pipeline);
        }

        let mut harness = Self {
            device,
            queue,
            resources: HashMap::new(),
            pipelines,
            plan,
        };
        harness.allocate_resources();
        Ok(Some(harness))
    }

    fn cells(&self) -> usize {
        (WIDTH * HEIGHT) as usize
    }

    /// Allocates every resource the plan declares (plus the presentation-only ones), seeding velocity,
    /// particles, and the initial density from deterministic CPU data.
    fn allocate_resources(&mut self) {
        let cells = self.cells();
        // A static rotational velocity field (linear in position — deterministic; trig unnecessary).
        let (cx, cy) = (WIDTH as f32 / 2.0, HEIGHT as f32 / 2.0);
        let mut velocity = vec![0.0_f32; cells * 2];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let i = (y * WIDTH + x) as usize;
                velocity[2 * i] = (y as f32 - cy) * 0.05;
                velocity[2 * i + 1] = -(x as f32 - cx) * 0.05;
            }
        }
        // Particles scattered deterministically across the grid.
        let mut particles = vec![0.0_f32; PARTICLE_COUNT as usize * 2];
        for p in 0..PARTICLE_COUNT as usize {
            particles[2 * p] = ((p * 37) % WIDTH as usize) as f32;
            particles[2 * p + 1] = ((p * 19) % HEIGHT as usize) as f32;
        }

        self.insert_resource("velocity", &f32_bytes(&velocity), false, true);
        self.insert_resource("density", &f32_bytes(&vec![0.0_f32; cells]), true, true);
        self.insert_resource("particles", &f32_bytes(&particles), false, true);
        self.insert_resource(
            "brightness",
            &f32_bytes(&vec![0.0_f32; PARTICLE_COUNT as usize]),
            false,
            false, // transient — never checkpointed
        );
    }

    fn insert_resource(&mut self, name: &str, contents: &[u8], ping_pong: bool, persistent: bool) {
        let make = |label: &str, data: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: data,
                    usage: wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::COPY_DST,
                })
        };
        let mut buffers = vec![make(name, contents)];
        if ping_pong {
            buffers.push(make(name, &vec![0u8; contents.len()]));
        }
        self.resources.insert(
            name.to_string(),
            Res {
                buffers,
                ping_pong,
                front: 0,
                persistent,
                bytes: contents.len() as u64,
            },
        );
    }

    /// Per-pass params, by entry point.
    fn pass_params(&self, entry: &str) -> Vec<u8> {
        match entry {
            "inject" => u32_bytes(&[
                WIDTH,
                HEIGHT,
                (WIDTH as f32 * 0.35).to_bits(),
                (HEIGHT as f32 * 0.5).to_bits(),
                8.0_f32.to_bits(),
                5.0_f32.to_bits(),
            ]),
            "advect" => u32_bytes(&[WIDTH, HEIGHT, ADVECT_DT.to_bits()]),
            "diffuse" => u32_bytes(&[WIDTH, HEIGHT, DIFFUSE_RATE.to_bits()]),
            "sample" => u32_bytes(&[WIDTH, HEIGHT, PARTICLE_COUNT]),
            other => panic!("unknown pass '{other}'"),
        }
    }

    /// Runs one pass (its reads/writes bound by convention) for `iterations`, swapping written ping-pong
    /// resources between iterations. Each iteration is its own compute pass (ordered barrier).
    fn run_pass(&mut self, encoder: &mut wgpu::CommandEncoder, pass: &StagedPass) {
        let pipeline = self.pipelines[&pass.entry_point].clone();
        let layout = self
            .device
            .create_bind_group_layout(&pass_layout_descriptor(pass));
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("pass params"),
                contents: &self.pass_params(&pass.entry_point),
                usage: wgpu::BufferUsages::STORAGE,
            });
        for _ in 0..pass.iterations {
            // Snapshot the buffers for this iteration's bindings, then swap indices afterwards.
            let read_buffers: Vec<wgpu::Buffer> = pass
                .reads
                .iter()
                .map(|name| self.resources[name].read_buffer())
                .collect();
            let write_buffers: Vec<wgpu::Buffer> = pass
                .writes
                .iter()
                .map(|name| self.resources[name].write_buffer())
                .collect();
            let mut entries = Vec::new();
            let mut binding = 0u32;
            for buffer in read_buffers.iter().chain(write_buffers.iter()) {
                entries.push(wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                });
                binding += 1;
            }
            entries.push(wgpu::BindGroupEntry {
                binding,
                resource: params.as_entire_binding(),
            });
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("staged pass bind group"),
                layout: &layout,
                entries: &entries,
            });
            {
                let mut compute = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some(&pass.name),
                    timestamp_writes: None,
                });
                compute.set_pipeline(&pipeline);
                compute.set_bind_group(0, &bind_group, &[]);
                compute.dispatch_workgroups(pass.dispatch.x, pass.dispatch.y, pass.dispatch.z);
            }
            for name in &pass.writes {
                self.resources.get_mut(name).unwrap().swap();
            }
        }
    }

    /// Advances the simulation by `ticks` fixed steps — each step runs the plan's passes in order.
    fn advance(&mut self, ticks: u32) {
        for _ in 0..ticks {
            let passes = self.plan.passes.clone();
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("staged fluid tick"),
                });
            for pass in &passes {
                self.run_pass(&mut encoder, pass);
            }
            self.queue.submit([encoder.finish()]);
        }
    }

    /// Reads back a resource's current front buffer as `f32`s.
    fn read_field(&self, name: &str) -> Vec<f32> {
        let res = &self.resources[name];
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("field readback"),
            size: res.bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(&res.read_buffer(), 0, &readback, 0, res.bytes);
        self.queue.submit([encoder.finish()]);
        let bytes = self.map_and_read(&readback);
        let (words, _) = bytes.as_chunks::<4>();
        words.iter().map(|word| f32::from_le_bytes(*word)).collect()
    }

    /// Runs the presentation `sample` pass once and reads back the per-particle brightness — the fluid
    /// driving stateful particles.
    fn sample_particles(&mut self) -> Vec<f32> {
        let pass = sample_pass();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.run_pass(&mut encoder, &pass);
        self.queue.submit([encoder.finish()]);
        self.read_field("brightness")
    }

    /// Snapshots every persistent resource's current front buffer — a checkpoint. The bytes are the sum
    /// of the persistent resources only (transient scratch excluded), independent of ticks/iterations.
    fn checkpoint(&self) -> Vec<(String, Vec<f32>)> {
        let mut names: Vec<&String> = self.resources.keys().collect();
        names.sort();
        names
            .into_iter()
            .filter(|name| self.resources[*name].persistent)
            .map(|name| (name.clone(), self.read_field(name)))
            .collect()
    }

    fn checkpoint_bytes(&self, checkpoint: &[(String, Vec<f32>)]) -> u64 {
        checkpoint
            .iter()
            .map(|(_, data)| (data.len() * 4) as u64)
            .sum()
    }

    /// Restores a checkpoint into the persistent resources' current front buffers.
    fn restore(&mut self, checkpoint: &[(String, Vec<f32>)]) {
        for (name, data) in checkpoint {
            let buffer = self.resources[name].read_buffer();
            self.queue.write_buffer(&buffer, 0, &f32_bytes(data));
        }
        self.queue.submit(std::iter::empty());
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

/// A bind group layout matching a pass's convention: read resources (read-only), write resources
/// (read-write), then params (read-only).
fn pass_layout_descriptor(pass: &StagedPass) -> wgpu::BindGroupLayoutDescriptor<'static> {
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
    let mut entries = Vec::new();
    let mut binding = 0u32;
    for _ in &pass.reads {
        entries.push(storage(binding, true));
        binding += 1;
    }
    for _ in &pass.writes {
        entries.push(storage(binding, false));
        binding += 1;
    }
    entries.push(storage(binding, true)); // params
    wgpu::BindGroupLayoutDescriptor {
        label: None,
        // Leak the entries to obtain a 'static slice (test-only; a handful of small allocations).
        entries: Box::leak(entries.into_boxed_slice()),
    }
}

/// The per-tick smoke plan: inject, advect, then diffuse×K, over a velocity + density grid.
fn smoke_plan() -> StagedPlan {
    let grid_bytes = u64::from(WIDTH) * u64::from(HEIGHT) * 4;
    let dispatch = StagedDispatch {
        x: WIDTH.div_ceil(8),
        y: HEIGHT.div_ceil(8),
        z: 1,
    };
    StagedPlan {
        resources: vec![
            StagedResource {
                name: "velocity".to_string(),
                bytes: grid_bytes * 2,
                ping_pong: false,
                lifetime: StagedResourceLifetime::Persistent,
            },
            StagedResource {
                name: "density".to_string(),
                bytes: grid_bytes,
                ping_pong: true,
                lifetime: StagedResourceLifetime::Persistent,
            },
        ],
        passes: vec![
            StagedPass {
                name: "inject".to_string(),
                entry_point: "inject".to_string(),
                reads: vec!["density".to_string()],
                writes: vec!["density".to_string()],
                dispatch,
                iterations: 1,
            },
            StagedPass {
                name: "advect".to_string(),
                entry_point: "advect".to_string(),
                reads: vec!["velocity".to_string(), "density".to_string()],
                writes: vec!["density".to_string()],
                dispatch,
                iterations: 1,
            },
            StagedPass {
                name: "diffuse".to_string(),
                entry_point: "diffuse".to_string(),
                reads: vec!["density".to_string()],
                writes: vec!["density".to_string()],
                dispatch,
                iterations: DIFFUSE_ITERS,
            },
        ],
    }
}

/// The presentation pass — separate from the per-tick plan; the fluid drives particles.
fn sample_pass() -> StagedPass {
    StagedPass {
        name: "sample".to_string(),
        entry_point: "sample".to_string(),
        reads: vec!["density".to_string(), "particles".to_string()],
        writes: vec!["brightness".to_string()],
        dispatch: StagedDispatch {
            x: PARTICLE_COUNT.div_ceil(64),
            y: 1,
            z: 1,
        },
        iterations: 1,
    }
}

fn require_harness() -> Option<FluidHarness> {
    let require_gpu = std::env::var_os(REQUIRED_GPU_ENV).is_some();
    match FluidHarness::new() {
        Ok(Some(harness)) => Some(harness),
        Ok(None) if !require_gpu => {
            eprintln!(
                "skipping staged fluid GPU conformance: no compatible compute adapter; set \
                 {REQUIRED_GPU_ENV}=1 to require one"
            );
            None
        }
        Ok(None) => panic!("{REQUIRED_GPU_ENV}=1 but no compatible compute adapter was found"),
        Err(error) => panic!("failed to create staged fluid harness: {error}"),
    }
}

#[test]
fn staged_multipass_smoke_is_deterministic_and_injection_adds_mass() {
    // Acceptance (reframed M14): a deterministic fixed-step multi-stage grid sim on the generic staged
    // infra. Two independent runs of the same plan produce identical frames (GPU-vs-GPU determinism),
    // and injection deterministically adds mass to the field.
    let Some(mut harness) = require_harness() else {
        return;
    };
    harness.advance(24);
    let field = harness.read_field("density");

    let total: f32 = field.iter().sum();
    assert!(
        total > 0.0,
        "injection has added density to the field (total {total})"
    );
    assert!(
        field.iter().all(|v| v.is_finite()),
        "the field stays finite (stable diffusion + advection)"
    );

    let mut rerun = require_harness().expect("adapter still present");
    rerun.advance(24);
    assert_eq!(
        field,
        rerun.read_field("density"),
        "the multi-stage staged sim is deterministic across runs"
    );
}

#[test]
fn staged_multipass_checkpoint_replay_matches_and_checkpoint_memory_is_bounded() {
    // Acceptance: multi-resource checkpoint/restore/replay reaches the uninterrupted state, and the
    // checkpoint memory is bounded — the persistent resources only (velocity + density), independent of
    // the tick count and the diffuse iteration count; the transient brightness is excluded.
    let Some(mut uninterrupted) = require_harness() else {
        return;
    };
    uninterrupted.advance(30);
    let target = uninterrupted.read_field("density");

    let mut run = require_harness().expect("adapter still present");
    run.advance(18);
    let checkpoint = run.checkpoint();
    // Bounded: exactly the persistent resources — velocity (2× grid), density (1× grid), and the
    // particle positions — never the transient brightness, and independent of ticks/iterations.
    let grid_bytes = u64::from(WIDTH) * u64::from(HEIGHT) * 4;
    let expected_bytes = grid_bytes * 3 + u64::from(PARTICLE_COUNT) * 2 * 4;
    assert_eq!(
        run.checkpoint_bytes(&checkpoint),
        expected_bytes,
        "checkpoint holds only the persistent resources (velocity + density + particles), bounded"
    );
    assert!(
        checkpoint.iter().all(|(name, _)| name != "brightness"),
        "the transient presentation buffer is not checkpointed"
    );

    // Overshoot, then restore the checkpoint and replay the remainder — must match the uninterrupted run.
    run.advance(40);
    run.restore(&checkpoint);
    run.advance(30 - 18);
    assert_eq!(
        target,
        run.read_field("density"),
        "restore at tick 18 + replay 12 == uninterrupted 30"
    );
}

#[test]
fn the_fluid_drives_particles_deterministically() {
    // Acceptance: the fluid can drive stateful particles. The presentation pass samples the density grid
    // at each particle position; a particle sitting where the sim has deposited density reads brighter
    // than one in an untouched region, and the sampling is deterministic.
    let Some(mut harness) = require_harness() else {
        return;
    };
    harness.advance(24);
    let brightness = harness.sample_particles();
    assert_eq!(brightness.len(), PARTICLE_COUNT as usize);
    assert!(
        brightness.iter().all(|v| v.is_finite()),
        "particle samples are finite"
    );
    assert!(
        brightness.iter().cloned().fold(0.0_f32, f32::max) > 0.0,
        "at least one particle sits in density the fluid deposited (the grid drives particles)"
    );

    let again = harness.sample_particles();
    assert_eq!(brightness, again, "grid-driven particle sampling is deterministic");
}
