//! GPU analytic-vs-stateful simulation benchmark (hybrid roadmap M9).
//!
//! Runs the same semantic workload — a single continuous emitter — two ways on real GPU compute, and
//! measures the per-frame simulation cost of each across a matrix of (capacity × occupancy):
//!
//! * **analytic** — the production `simulate` kernel, which recomputes every slot from scratch each
//!   frame (full-capacity dispatch), the way the stateless backend does.
//! * **stateful** — the M6 death loop (`death_integrate` + `spawn`) advanced one fixed tick plus the
//!   `present` extraction, over persistent state.
//!
//! It reports each path's median/p95 GPU time, the persistent memory each keeps, and the dispatch
//! count, then a crossover comparison per cell. This is evidence for a policy decision, not a policy:
//! the runtime does not switch backends based on it (strategy §16, roadmap M9).

use crate::Config;
use aestra_compiler::EffectCompiler;
use aestra_core::{
    EffectAsset, Emitter, EmitterShape, MODULE_EMISSION, MODULE_INITIALIZE, MODULE_MOTION,
    MODULE_SHAPE, ModuleParameters, ScalarRange,
};
use aestra_gpu::shader::{SIMULATION_WESL, compile_wesl};
use aestra_gpu::{
    GpuEffectArtifact, GpuGlobals, GpuParticle, STATEFUL_SIMULATION_PARAM_WORDS,
    indirect_draw_commands_with_statistics,
};
use aestra_runtime::EffectInstance;
use encase::ShaderType;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use wgpu::util::DeviceExt;

const TICK_DT: f32 = 1.0 / 60.0;
const WORKGROUP: u32 = 64;
const STATE_STRIDE: u32 = 9;
/// Capacities and occupancies swept. Kept modest so a full run finishes in seconds.
const CAPACITIES: [u32; 3] = [4_096, 32_768, 131_072];
const OCCUPANCIES: [f64; 3] = [0.05, 0.25, 1.0];

pub(crate) fn run(config: &Config) -> Result<(), String> {
    let mut harness = Harness::new()?;
    let mut report = Report::new(config, harness.adapter_info.clone());
    for &capacity in &CAPACITIES {
        for &occupancy in &OCCUPANCIES {
            report
                .cells
                .push(harness.measure_cell(config, capacity, occupancy)?);
        }
    }
    let path = config.out.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("benchmarks/gpu-baselines")
            .join(format!("sim-{}", report.captured_at_unix_ns))
            .join("analytic-vs-stateful.json")
    });
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    print_summary(&report);
    println!("wrote GPU baseline {}", path.display());
    Ok(())
}

fn print_summary(report: &Report) {
    println!(
        "\naestra-bench gpu-sim  ({} on {})",
        report.adapter["name"], report.adapter["backend"]
    );
    println!(
        "{:>9} {:>6}  {:>13} {:>13}  {:>11}  {:>10}",
        "capacity", "occ%", "analytic med", "stateful med", "stateful/an", "verdict"
    );
    for cell in &report.cells {
        let ratio = if cell.analytic.median_ns > 0 {
            cell.stateful.median_ns as f64 / cell.analytic.median_ns as f64
        } else {
            f64::NAN
        };
        let verdict = if ratio < 1.0 { "stateful" } else { "analytic" };
        println!(
            "{:>9} {:>6.1}  {:>10} ns {:>10} ns  {:>10.2}x  {:>10}",
            cell.capacity,
            cell.occupancy_percent,
            cell.analytic.median_ns,
            cell.stateful.median_ns,
            ratio,
            verdict,
        );
    }
}

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
    timestamp_period: f32,
    // Analytic pipelines share the 8-binding simulation layout.
    analytic_layout: wgpu::BindGroupLayout,
    analytic_reset: wgpu::ComputePipeline,
    analytic_simulate: wgpu::ComputePipeline,
    // Stateful pipelines share the 9-binding layout.
    stateful_layout: wgpu::BindGroupLayout,
    death_integrate: wgpu::ComputePipeline,
    stateful_spawn: wgpu::ComputePipeline,
    stateful_present: wgpu::ComputePipeline,
}

impl Harness {
    fn new() -> Result<Self, String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|_| "no compatible GPU adapter".to_owned())?;
        let adapter_info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| format!("timestamp queries required: {e}"))?;
        let timestamp_period = queue.get_timestamp_period();

        // Analytic simulate module (WESL → WGSL) and its 8-binding layout.
        let analytic_wgsl = compile_wesl(
            "package::aestra_simulation",
            SIMULATION_WESL,
            &["reset", "simulate"],
        )
        .map_err(|e| format!("analytic shader: {e}"))?
        .wgsl;
        let analytic_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("analytic simulate"),
            source: wgpu::ShaderSource::Wgsl(analytic_wgsl.into()),
        });
        let analytic_layout = storage_layout(
            &device,
            "analytic",
            &[true, false, false, false, false, false, true, false],
        );
        let analytic_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("analytic layout"),
                bind_group_layouts: &[Some(&analytic_layout)],
                immediate_size: 0,
            });
        let analytic_pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&analytic_pipeline_layout),
                module: &analytic_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let analytic_reset = analytic_pipeline("reset");
        let analytic_simulate = analytic_pipeline("simulate");

        // Stateful module and its 9-binding layout.
        let stateful_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stateful"),
            source: wgpu::ShaderSource::Wgsl(aestra_gpu::stateful_simulation_wgsl().into()),
        });
        let stateful_layout = storage_layout(
            &device,
            "stateful",
            &[false, false, false, false, true, false, false, false, false],
        );
        let stateful_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("stateful layout"),
                bind_group_layouts: &[Some(&stateful_layout)],
                immediate_size: 0,
            });
        let stateful_pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&stateful_pipeline_layout),
                module: &stateful_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let death_integrate = stateful_pipeline("death_integrate");
        let stateful_spawn = stateful_pipeline("spawn");
        let stateful_present = stateful_pipeline("present");

        Ok(Self {
            device,
            queue,
            adapter_info,
            timestamp_period,
            analytic_layout,
            analytic_reset,
            analytic_simulate,
            stateful_layout,
            death_integrate,
            stateful_spawn,
            stateful_present,
        })
    }

    fn measure_cell(
        &mut self,
        config: &Config,
        capacity: u32,
        occupancy: f64,
    ) -> Result<Cell, String> {
        let spawn_per_tick = ((occupancy * f64::from(capacity)) / 60.0).round().max(1.0) as u32;
        let analytic = self.measure_analytic(config, capacity, occupancy)?;
        let stateful = self.measure_stateful(config, capacity, spawn_per_tick)?;
        Ok(Cell {
            capacity,
            occupancy_percent: occupancy * 100.0,
            spawn_per_tick,
            analytic,
            stateful,
            // Analytic keeps no persistent simulation state; stateful keeps the 9-float state slot plus
            // a free-list index per slot.
            analytic_persistent_bytes_per_slot: 0,
            stateful_persistent_bytes_per_slot: (STATE_STRIDE + 1) * 4,
            analytic_dispatches_per_frame: 2, // reset + simulate
            stateful_dispatches_per_frame: 3, // death_integrate + spawn + present
        })
    }

    fn measure_analytic(
        &self,
        config: &Config,
        capacity: u32,
        occupancy: f64,
    ) -> Result<Timing, String> {
        let spawn_rate = (occupancy * f64::from(capacity)) as f32; // per second, lifetime ~1s
        let artifact = analytic_artifact(capacity, spawn_rate);
        let emitters = &artifact.emitters;
        let make = |label, contents: &[u8]| self.buffer(label, contents);
        let emitters_buf = make("emitters", &encode(emitters));
        let particles = make(
            "particles",
            &encode(&vec![GpuParticle::default(); capacity as usize]),
        );
        let alive = make("alive", &encode(&vec![0u32; capacity as usize]));
        let dead = make("dead", &encode(&vec![0u32; capacity as usize]));
        let counters = make("counters", &encode(&vec![0u32; 2]));
        let indirect = make(
            "indirect",
            &encode(&indirect_draw_commands_with_statistics(emitters)),
        );
        let globals = make(
            "globals",
            &encode(&GpuGlobals {
                time: 2.0,
                total_slots: capacity,
                seed: config.seed as u32,
                emitter_count: 1,
                duration: 10.0,
                continuous: 1,
                _padding: glam::UVec2::ZERO,
                world_from_effect: glam::Mat4::IDENTITY,
            }),
        );
        let aux = make("aux", &encode(&vec![0u32; 1]));
        let bind_group = self.bind_group(
            &self.analytic_layout,
            &[
                &emitters_buf,
                &particles,
                &alive,
                &dead,
                &counters,
                &indirect,
                &globals,
                &aux,
            ],
        );
        let workgroups = capacity.div_ceil(WORKGROUP);
        self.time_frames(config, |pass| {
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_pipeline(&self.analytic_reset);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(&self.analytic_simulate);
            pass.dispatch_workgroups(workgroups, 1, 1);
        })
    }

    fn measure_stateful(
        &self,
        config: &Config,
        capacity: u32,
        spawn_per_tick: u32,
    ) -> Result<Timing, String> {
        let copyable = wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST;
        let init = |label, contents: &[u8]| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents,
                    usage: copyable,
                })
        };
        let state = init(
            "state",
            &encode(&vec![0.0f32; (capacity * STATE_STRIDE) as usize]),
        );
        let free_list = init("free list", &encode(&(0..capacity).collect::<Vec<u32>>()));
        let free_count = init("free count", &encode(&capacity));
        let spawn_counter = init("spawn counter", &encode(&0u32));
        let particles = init(
            "particles",
            &encode(&vec![GpuParticle::default(); capacity as usize]),
        );
        let alive = init("alive", &encode(&vec![0u32; capacity as usize]));
        let indirect = init("indirect", &encode(&vec![0u32; 8]));
        let counters = init("counters", &encode(&vec![0u32; 2]));
        let advance_params = self.buffer(
            "advance params",
            &stateful_params(capacity, spawn_per_tick, config.seed, 0.0),
        );
        let present_params = self.buffer(
            "present params",
            &stateful_params(capacity, 0, config.seed, 0.0),
        );
        let bind = |params: &wgpu::Buffer| {
            self.bind_group(
                &self.stateful_layout,
                &[
                    &state,
                    &free_list,
                    &free_count,
                    &spawn_counter,
                    params,
                    &particles,
                    &alive,
                    &indirect,
                    &counters,
                ],
            )
        };
        let advance_group = bind(&advance_params);
        let present_group = bind(&present_params);
        let workgroups = capacity.div_ceil(WORKGROUP);
        self.time_frames(config, |pass| {
            // One fixed tick (death + spawn) then presentation — the steady-state per-frame cost.
            pass.set_bind_group(0, &advance_group, &[]);
            pass.set_pipeline(&self.death_integrate);
            pass.dispatch_workgroups(workgroups, 1, 1);
            pass.set_pipeline(&self.stateful_spawn);
            pass.dispatch_workgroups(workgroups, 1, 1);
            pass.set_bind_group(0, &present_group, &[]);
            pass.set_pipeline(&self.stateful_present);
            pass.dispatch_workgroups(workgroups, 1, 1);
        })
    }

    /// Records `warmup + frames` timed compute passes (each running `body`), returns the last `frames`
    /// timings. State-carrying buffers advance across passes, so the stateful path reaches steady state.
    fn time_frames(
        &self,
        config: &Config,
        body: impl Fn(&mut wgpu::ComputePass),
    ) -> Result<Timing, String> {
        let total = (config.warmup + config.frames) as u32;
        let queries = self.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu-sim timestamps"),
            ty: wgpu::QueryType::Timestamp,
            count: total * 2,
        });
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resolve"),
            size: u64::from(total) * 16,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(total) * 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for frame in 0..total {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu-sim frame"),
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: &queries,
                    beginning_of_pass_write_index: Some(frame * 2),
                    end_of_pass_write_index: Some(frame * 2 + 1),
                }),
            });
            body(&mut pass);
        }
        encoder.resolve_query_set(&queries, 0..total * 2, &resolve, 0);
        let work = encoder.finish();
        let mut copy = self.device.create_command_encoder(&Default::default());
        copy.copy_buffer_to_buffer(&resolve, 0, &readback, 0, u64::from(total) * 16);
        let submission = self.queue.submit([work, copy.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bytes = readback.slice(..).get_mapped_range();
        let ticks: Vec<u64> = bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        let samples: Vec<u64> = (config.warmup..config.warmup + config.frames)
            .map(|frame| {
                let (begin, end) = (ticks[frame * 2], ticks[frame * 2 + 1]);
                ((end.saturating_sub(begin)) as f32 * self.timestamp_period) as u64
            })
            .collect();
        drop(bytes);
        readback.unmap();
        Ok(Timing::new(samples))
    }

    fn buffer(&self, label: &str, contents: &[u8]) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
    }

    fn bind_group(
        &self,
        layout: &wgpu::BindGroupLayout,
        buffers: &[&wgpu::Buffer],
    ) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout,
            entries: &buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        })
    }
}

fn storage_layout(device: &wgpu::Device, label: &str, read_only: &[bool]) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &read_only
            .iter()
            .enumerate()
            .map(|(binding, &read_only)| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>(),
    })
}

/// A single-emitter analytic effect at the requested capacity and spawn rate (lifetime ~1 s), so a
/// fraction `spawn_rate/capacity` of slots is alive at steady state.
fn analytic_artifact(capacity: u32, spawn_rate: f32) -> GpuEffectArtifact {
    let mut effect = EffectAsset::new("bench", 10.0);
    let mut emitter = Emitter::basic_sprite("bench", 10.0);
    emitter.max_particles = capacity;
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate: rate,
                burst_count,
            } if module.module_type.0 == MODULE_EMISSION => {
                *rate = spawn_rate;
                *burst_count = 0;
            }
            ModuleParameters::Initialize {
                lifetime, speed, ..
            } if module.module_type.0 == MODULE_INITIALIZE => {
                *lifetime = ScalarRange::new(1.0, 1.0);
                *speed = ScalarRange::new(20.0, 40.0);
            }
            ModuleParameters::Motion { turbulence, .. }
                if module.module_type.0 == MODULE_MOTION =>
            {
                *turbulence = 3.0;
            }
            ModuleParameters::Shape { shape } if module.module_type.0 == MODULE_SHAPE => {
                *shape = EmitterShape::Sphere { radius: 4.0 };
            }
            _ => {}
        }
    }
    effect.emitters.push(emitter);
    let compiled = Arc::new(EffectCompiler::default().compile(&effect).expect("compile"));
    GpuEffectArtifact::from_instance(&EffectInstance::new(compiled)).expect("artifact")
}

/// The 26-word stateful params buffer (constant gravity/spread/etc.; only what the death loop reads
/// matters for cost). Matches `aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS`.
fn stateful_params(capacity: u32, spawn_per_tick: u32, seed: u64, subtick: f32) -> Vec<u8> {
    let mut words = [0u32; STATEFUL_SIMULATION_PARAM_WORDS];
    words[0] = capacity;
    words[1] = spawn_per_tick;
    words[2] = seed as u32;
    words[3] = (seed >> 32) as u32;
    words[4] = 20.0f32.to_bits(); // speed min
    words[5] = 40.0f32.to_bits(); // speed max
    words[6] = 1.0f32.to_bits(); // lifetime min (~60 ticks)
    words[7] = 1.0f32.to_bits(); // lifetime max
    words[8] = TICK_DT.to_bits();
    words[10] = (-9.81f32).to_bits(); // gravity.y
    words[13] = 1.0f32.to_bits(); // direction.y
    words[15] = 0.5f32.to_bits(); // spread
    words[16] = 0.5f32.to_bits(); // drag
    words[19] = 3.0f32.to_bits(); // turbulence
    words[20] = 1; // shape kind = sphere
    words[21] = 4.0f32.to_bits(); // shape radius
    words[25] = subtick.to_bits();
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn encode<T: ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = encase::StorageBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

#[derive(Serialize)]
struct Report {
    schema_version: u32,
    experiment: &'static str,
    commit: String,
    captured_at_unix_ns: u128,
    os: &'static str,
    arch: &'static str,
    adapter: std::collections::BTreeMap<&'static str, String>,
    frames: usize,
    warmup: usize,
    seed: u64,
    cells: Vec<Cell>,
}

impl Report {
    fn new(config: &Config, adapter: wgpu::AdapterInfo) -> Self {
        Self {
            schema_version: 1,
            experiment: "analytic-vs-stateful",
            commit: config.commit.clone(),
            captured_at_unix_ns: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            os: std::env::consts::OS,
            arch: std::env::consts::ARCH,
            adapter: std::collections::BTreeMap::from([
                ("name", adapter.name),
                ("backend", format!("{:?}", adapter.backend)),
                ("device_type", format!("{:?}", adapter.device_type)),
            ]),
            frames: config.frames,
            warmup: config.warmup,
            seed: config.seed,
            cells: Vec::new(),
        }
    }
}

#[derive(Serialize)]
struct Cell {
    capacity: u32,
    occupancy_percent: f64,
    spawn_per_tick: u32,
    analytic: Timing,
    stateful: Timing,
    analytic_persistent_bytes_per_slot: u32,
    stateful_persistent_bytes_per_slot: u32,
    analytic_dispatches_per_frame: u32,
    stateful_dispatches_per_frame: u32,
}

#[derive(Serialize)]
struct Timing {
    median_ns: u64,
    p95_ns: u64,
    samples_ns: Vec<u64>,
}

impl Timing {
    fn new(samples_ns: Vec<u64>) -> Self {
        assert!(!samples_ns.is_empty());
        let mut sorted = samples_ns.clone();
        sorted.sort_unstable();
        Self {
            median_ns: sorted[sorted.len() / 2],
            p95_ns: sorted[((sorted.len() as f64 * 0.95) as usize).min(sorted.len() - 1)],
            samples_ns,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_reports_median_and_p95_and_preserves_order() {
        let single = Timing::new(vec![7]);
        assert_eq!((single.median_ns, single.p95_ns), (7, 7));
        let stats = Timing::new((0..100).rev().collect());
        assert_eq!(stats.median_ns, 50);
        assert_eq!(stats.p95_ns, 95);
        assert_eq!(stats.samples_ns[0], 99, "acquisition order preserved");
    }

    #[test]
    fn stateful_params_have_the_production_word_count() {
        let bytes = stateful_params(4096, 8, 0x1234, 0.0);
        assert_eq!(bytes.len(), STATEFUL_SIMULATION_PARAM_WORDS * 4);
        // capacity and spawn_per_tick land in words 0 and 1.
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 4096);
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 8);
    }
}
