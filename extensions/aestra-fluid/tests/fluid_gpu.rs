//! Extensible-stages M13 on the real GPU: the fluid plugin's lowered solver runs on the render backend's
//! Execution IR executor. There is no CPU fluid to compare with (plan §44.6); correctness is
//! GPU-vs-GPU reproducibility plus physical sanity:
//!
//! - rerunning the same asset + seed + frames reproduces the same bits;
//! - restoring a checkpoint (only the persistent grids) and replaying reaches the uninterrupted state;
//! - the pressure projection leaves the velocity far closer to divergence-free than it found it;
//! - a density source bound to a host object follows that object.
//!
//! Runs only where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_bevy_render::execution::StageExecutor;
use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_POSITION, BindingUpdateMode, EffectAsset, EffectBinding, HostFieldRef,
    ModuleParameters, PropertySource, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_DENSITY_SOURCE, MODULE_GRID, RESOURCE_DENSITY, RESOURCE_DIVERGENCE,
    RESOURCE_VELOCITY, smoke_effect,
};
use aestra_gpu::GpuHostBindings;
use aestra_runtime::{EffectInstance, FrameConstants, SpatialBindingSnapshot};
use std::sync::Arc;

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const DT: f32 = 1.0 / 60.0;
const SEED: u32 = 7;
/// A 16³ grid of 0.2 m cells: the default 3.2 m box, small enough to read back every tick.
const RESOLUTION: u32 = 16;
const CELL_SIZE: f32 = 0.2;
const CENTER: [f32; 3] = [0.0, 1.6, 0.0];

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

fn gpu() -> Option<Gpu> {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .ok();
    let adapter = adapter.filter(|adapter| {
        adapter
            .get_downlevel_capabilities()
            .flags
            .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
    });
    let Some(adapter) = adapter else {
        assert!(
            std::env::var_os(REQUIRED_GPU_ENV).is_none(),
            "{REQUIRED_GPU_ENV} is set but no compute adapter is available"
        );
        eprintln!("skipping fluid GPU conformance: no compatible compute adapter");
        return None;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra fluid conformance"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("the adapter provides a device");
    Some(Gpu { device, queue })
}

fn registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::builtin();
    registry.install(&FluidExtension).unwrap();
    registry
}

fn set_input(effect: &mut EffectAsset, type_id: &str, name: &str, value: Value) {
    let module = effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .unwrap();
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

/// The smoke effect on the test grid; `bound` binds the source position to a host object.
fn effect(registry: &ExtensionRegistry, bound: bool, iterations: u32) -> EffectAsset {
    let mut effect = smoke_effect(registry);
    set_input(
        &mut effect,
        MODULE_GRID,
        "resolution",
        Value::U32(RESOLUTION),
    );
    set_input(
        &mut effect,
        MODULE_GRID,
        "cell_size",
        Value::Scalar(CELL_SIZE),
    );
    set_input(&mut effect, MODULE_GRID, "center", Value::Vec3(CENTER));
    set_input(
        &mut effect,
        MODULE_GRID,
        "pressure_iterations",
        Value::U32(iterations),
    );
    if bound {
        let emitter = EffectBinding::spatial("Emitter", BindingUpdateMode::Live);
        let emitter_id = emitter.id;
        effect.bindings = vec![emitter];
        let source = effect.emitters[0]
            .modules
            .iter_mut()
            .find(|module| module.module_type.0 == MODULE_DENSITY_SOURCE)
            .unwrap();
        source
            .property_sources
            .insert("position".into(), PropertySource::HostBinding);
        source.host_bindings.insert(
            "position".into(),
            HostFieldRef::new(emitter_id, AESTRA_FIELD_POSITION),
        );
    }
    effect
}

/// A compiled fluid effect, its host-side instance, and its stage running on the GPU.
struct Fluid {
    instance: EffectInstance,
    stage: StageExecutor,
}

impl Fluid {
    fn new(gpu: &Gpu, registry: &ExtensionRegistry, effect: &EffectAsset) -> Self {
        let compiled = EffectCompiler::with_extensions(registry.clone())
            .compile(effect)
            .expect("the fluid effect compiles");
        let block = compiled.emitters[0].extension_stages[0].block.clone();
        let instance = EffectInstance::new(Arc::new(compiled));
        let host_bytes = GpuHostBindings::from_instance(&instance).byte_len();
        let stage = StageExecutor::new(
            &gpu.device,
            &gpu.queue,
            &block,
            &registry.programs,
            host_bytes,
        )
        .expect("the solver block runs on this backend");
        Self { instance, stage }
    }

    fn run(&self, gpu: &Gpu, ticks: std::ops::Range<u32>) {
        for tick in ticks {
            self.stage
                .run_tick(
                    &gpu.device,
                    &gpu.queue,
                    FrameConstants::fixed_step(tick, DT, SEED),
                    Some(&GpuHostBindings::from_instance(&self.instance)),
                )
                .unwrap();
        }
    }

    fn floats(&self, gpu: &Gpu, resource: &str) -> Vec<f32> {
        self.stage
            .read_resource(&gpu.device, &gpu.queue, resource)
            .unwrap()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| f32::from_le_bytes(*bytes))
            .collect()
    }

    /// The persistent state, bit for bit.
    fn state(&self, gpu: &Gpu) -> (Vec<u8>, Vec<u8>) {
        let read = |id| {
            self.stage
                .read_resource(&gpu.device, &gpu.queue, id)
                .unwrap()
        };
        (read(RESOURCE_VELOCITY), read(RESOURCE_DENSITY))
    }
}

fn cell_center(index: usize) -> [f32; 3] {
    let n = RESOLUTION as usize;
    let cell = [index % n, (index / n) % n, index / (n * n)];
    let half = RESOLUTION as f32 * CELL_SIZE * 0.5;
    std::array::from_fn(|axis| CENTER[axis] - half + (cell[axis] as f32 + 0.5) * CELL_SIZE)
}

/// Total density and its density-weighted centroid.
fn mass_and_centroid(density: &[f32]) -> (f32, [f32; 3]) {
    let mass: f32 = density.iter().sum();
    let mut centroid = [0.0; 3];
    for (index, d) in density.iter().enumerate() {
        let position = cell_center(index);
        for axis in 0..3 {
            centroid[axis] += d * position[axis] / mass;
        }
    }
    (mass, centroid)
}

#[test]
fn rerunning_the_same_asset_seed_and_frames_reproduces_the_same_bits() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let effect = effect(&registry, false, 24);
    let first = Fluid::new(&gpu, &registry, &effect);
    let second = Fluid::new(&gpu, &registry, &effect);
    let mut rising = Vec::new();
    for window in [0..20, 20..40, 40..60] {
        first.run(&gpu, window.clone());
        second.run(&gpu, window);
        assert_eq!(
            first.state(&gpu),
            second.state(&gpu),
            "GPU-vs-GPU determinism"
        );
        let density = first.floats(&gpu, RESOURCE_DENSITY);
        assert!(
            density
                .iter()
                .chain(&first.floats(&gpu, RESOURCE_VELOCITY))
                .all(|v| v.is_finite())
        );
        let (mass, centroid) = mass_and_centroid(&density);
        assert!(mass > 0.0, "the source injected density");
        rising.push(centroid[1]);
    }
    // The source sits low in the box and buoyancy lifts the smoke.
    assert!(
        rising.windows(2).all(|pair| pair[1] > pair[0]),
        "the smoke rises: {rising:?}"
    );
}

#[test]
fn restoring_a_checkpoint_and_replaying_reaches_the_uninterrupted_state() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &effect(&registry, false, 24));
    fluid.run(&gpu, 0..20);
    let checkpoint = fluid.stage.checkpoint(&gpu.device, &gpu.queue).unwrap();
    let cells = (RESOLUTION as usize).pow(3);
    assert_eq!(
        checkpoint.bytes(),
        cells * (16 + 4),
        "only the persistent velocity and density grids — no scratch, no host inputs"
    );
    fluid.run(&gpu, 20..45);
    let uninterrupted = fluid.state(&gpu);

    fluid.stage.restore(&gpu.queue, &checkpoint).unwrap();
    fluid.run(&gpu, 20..45);
    assert_eq!(fluid.state(&gpu), uninterrupted, "restore + replay");
}

/// RMS of the central-difference divergence over cells at least two away from every wall.
fn interior_rms(values: impl Fn(usize, usize, usize) -> f32) -> f32 {
    let n = RESOLUTION as usize;
    let (mut sum, mut count) = (0.0, 0.0);
    for z in 2..n - 2 {
        for y in 2..n - 2 {
            for x in 2..n - 2 {
                sum += values(x, y, z).powi(2);
                count += 1.0;
            }
        }
    }
    (sum / count).sqrt()
}

#[test]
fn the_pressure_projection_removes_most_of_the_divergence() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &effect(&registry, false, 80));
    fluid.run(&gpu, 0..30);
    let n = RESOLUTION as usize;
    let index = |x: usize, y: usize, z: usize| (z * n + y) * n + x;
    // The divergence the tick's projection started from…
    let before = fluid.floats(&gpu, RESOURCE_DIVERGENCE);
    let before = interior_rms(|x, y, z| before[index(x, y, z)]);
    // …and what is left in the projected velocity.
    let velocity = fluid.floats(&gpu, RESOURCE_VELOCITY);
    let component = |x, y, z, axis| velocity[index(x, y, z) * 4 + axis];
    let after = interior_rms(|x, y, z| {
        (component(x + 1, y, z, 0) - component(x - 1, y, z, 0) + component(x, y + 1, z, 1)
            - component(x, y - 1, z, 1)
            + component(x, y, z + 1, 2)
            - component(x, y, z - 1, 2))
            * (0.5 / CELL_SIZE)
    });
    eprintln!("divergence rms: before {before}, after {after}");
    assert!(before > 0.0);
    // The solve uses the operator the projection applies, so it removes nearly all of it (~40× on
    // the reference GPU), not just the part a mismatched Laplacian could reach.
    assert!(
        after < before * 0.1,
        "projection removes the divergence: {before} -> {after}"
    );
}

#[test]
fn a_host_bound_source_follows_its_target() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let centroid_x = |target: Option<[f32; 3]>| {
        let mut fluid = Fluid::new(&gpu, &registry, &effect(&registry, true, 24));
        if let Some(position) = target {
            fluid
                .instance
                .set_spatial_binding("Emitter", SpatialBindingSnapshot::at(position))
                .unwrap();
        }
        fluid.run(&gpu, 0..30);
        mass_and_centroid(&fluid.floats(&gpu, RESOURCE_DENSITY)).1[0]
    };
    let left = centroid_x(Some([-1.0, 0.5, 0.0]));
    let right = centroid_x(Some([1.0, 0.5, 0.0]));
    // Unbound: the authored fallback position, on the axis.
    let unbound = centroid_x(None);
    eprintln!("centroid x: left {left}, right {right}, unbound {unbound}");
    assert!(left < -0.5 && right > 0.5, "left {left}, right {right}");
    assert!(unbound.abs() < 0.1, "unbound {unbound}");
}
