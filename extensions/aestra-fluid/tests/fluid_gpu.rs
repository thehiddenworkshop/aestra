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

use aestra_bevy_render::execution::{
    FieldFollowPipeline, FieldVolumePipeline, StageExecutor, StageInputs, StageTimeline,
    TimelinePolicy,
};
use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    AESTRA_FIELD_POSITION, BindingUpdateMode, EffectAsset, EffectBinding, HostFieldRef,
    ModuleParameters, PropertySource, Value,
};
use aestra_fluid::{
    FluidExtension, MODULE_BUOYANCY, MODULE_DENSITY_SOURCE, MODULE_GRID, RESOURCE_DENSITY,
    RESOURCE_DIVERGENCE, RESOURCE_VELOCITY, smoke_effect,
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
    let module = effect.simulation_stages[0]
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
    // A small-scale scene: the source sits low in the 3.2-unit box.
    for (name, value) in [
        ("position", Value::Vec3([0.0, 0.5, 0.0])),
        ("radius", Value::Scalar(0.4)),
        ("velocity", Value::Vec3([0.0, 1.5, 0.0])),
    ] {
        set_input(&mut effect, MODULE_DENSITY_SOURCE, name, value);
    }
    set_input(&mut effect, MODULE_BUOYANCY, "strength", Value::Scalar(1.5));
    if bound {
        let emitter = EffectBinding::spatial("Emitter", BindingUpdateMode::Live);
        let emitter_id = emitter.id;
        effect.bindings = vec![emitter];
        let source = effect.simulation_stages[0]
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
        let block = compiled.extension_stages[0].block.clone();
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
        self.run_placed(gpu, ticks, aestra_runtime::IDENTITY_AFFINE);
    }

    /// Runs with the effect placed in the world by `world_to_effect`.
    fn run_placed(&self, gpu: &Gpu, ticks: std::ops::Range<u32>, world_to_effect: [[f32; 4]; 3]) {
        for tick in ticks {
            self.stage
                .run_tick(
                    &gpu.device,
                    &gpu.queue,
                    FrameConstants::fixed_step(tick, DT, SEED)
                        .with_world_to_effect(world_to_effect),
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

/// Every pipeline of the solver builds on each native backend present — D3D12 through FXC included,
/// which rejects what Vulkan accepts (a dynamically indexed vector write forces loops to unroll).
#[test]
fn the_solver_builds_on_every_available_backend() {
    let registry = registry();
    let effect = EffectCompiler::with_extensions(registry.clone())
        .compile(&smoke_effect(&registry))
        .unwrap();
    let block = &effect.extension_stages[0].block;
    for backends in [
        wgpu::Backends::VULKAN,
        wgpu::Backends::DX12,
        wgpu::Backends::METAL,
    ] {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = backends;
        descriptor.backend_options.dx12.shader_compiler = wgpu::Dx12Compiler::Fxc;
        let instance = wgpu::Instance::new(descriptor);
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            continue;
        };
        let Ok((device, queue)) = pollster::block_on(adapter.request_device(&Default::default()))
        else {
            continue;
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let executor = StageExecutor::new(&device, &queue, block, &registry.programs, 4);
        let error = pollster::block_on(scope.pop());
        assert!(
            executor.is_ok() && error.is_none(),
            "{backends:?}: {:?} {error:?}",
            executor.err()
        );
        // The volume look's march function, with the interface it is composed with (fluid F3).
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        volume_pipeline(&device);
        let error = pollster::block_on(scope.pop());
        assert!(error.is_none(), "{backends:?} volume: {error:?}");
    }
}

/// A render pipeline around the volume look's march function: the interface, the plugin's WGSL and
/// a full-screen triangle whose fragment marches a fixed ray.
fn volume_pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let source = format!(
        "{}\n{}\n\
         @vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {{\n    \
         let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));\n    \
         return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);\n}}\n\
         @fragment fn fragment(@builtin(position) pixel: vec4<f32>) -> @location(0) vec4<f32> {{\n    \
         let u = pixel.x / 16.0;\n    \
         return {}(AestraVolumeRay(vec3<f32>(u, 0.5, 0.0), vec3<f32>(0.0, 0.0, 1.0 / 3.2), 0.0, 3.2, \
         vec3<f32>(3.2), pixel.xy));\n}}",
        aestra_gpu::volume::volume_interface_wgsl("0"),
        aestra_fluid::VOLUME_WGSL,
        aestra_fluid::VOLUME_ENTRY,
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("fluid volume test"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("fluid volume test"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::TextureFormat::Rgba16Float.into())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

/// An IEEE half as `f32` (normal, subnormal and zero; the fields hold no infinities).
fn half_to_f32(bits: u16) -> f32 {
    let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = i32::from((bits >> 10) & 0x1f);
    let mantissa = f32::from(bits & 0x3ff);
    sign * if exponent == 0 {
        mantissa * 2f32.powi(-24)
    } else {
        (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15)
    }
}

/// The test grid set on fire: the source emits fuel and heat but no smoke of its own, and hot gas
/// rises at the test scale.
fn fire(registry: &ExtensionRegistry) -> EffectAsset {
    let mut fire = effect(registry, false, 24);
    let combustion = aestra_fluid::fire_effect(registry).simulation_stages[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_fluid::MODULE_COMBUSTION)
        .unwrap()
        .clone();
    fire.simulation_stages[0].modules.push(combustion);
    for (name, value) in [
        ("density_rate", 0.0),
        ("temperature_rate", 8.0),
        ("fuel_rate", 4.0),
    ] {
        set_input(&mut fire, MODULE_DENSITY_SOURCE, name, Value::Scalar(value));
    }
    set_input(
        &mut fire,
        aestra_fluid::MODULE_COMBUSTION,
        "thermal_lift",
        Value::Scalar(1.0),
    );
    fire
}

#[test]
fn fire_burns_fuel_into_heat_and_smoke_and_the_hot_gas_rises() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let effect = fire(&registry);
    let burning = Fluid::new(&gpu, &registry, &effect);
    burning.run(&gpu, 0..60);
    let temperature = burning.floats(&gpu, aestra_fluid::RESOURCE_TEMPERATURE);
    let fuel = burning.floats(&gpu, aestra_fluid::RESOURCE_FUEL);
    let density = burning.floats(&gpu, RESOURCE_DENSITY);
    let hottest = temperature.iter().copied().fold(0.0, f32::max);
    assert!(hottest > 0.5, "the source heats past ignition ({hottest})");
    let (smoke, _) = mass_and_centroid(&density);
    assert!(
        smoke > 0.01,
        "with no smoke of its own, the source's smoke comes from burning ({smoke})"
    );
    // Without burning, the same fuel would pile up.
    let mut unlit = effect.clone();
    set_input(
        &mut unlit,
        aestra_fluid::MODULE_COMBUSTION,
        "ignition_temperature",
        Value::Scalar(1.0e6),
    );
    let unlit = Fluid::new(&gpu, &registry, &unlit);
    unlit.run(&gpu, 0..60);
    let fuel_total: f32 = fuel.iter().sum();
    let unburned: f32 = unlit.floats(&gpu, aestra_fluid::RESOURCE_FUEL).iter().sum();
    assert!(
        fuel_total < unburned * 0.8,
        "burning consumes fuel ({fuel_total} of {unburned})"
    );
    assert!(
        mass_and_centroid(&unlit.floats(&gpu, RESOURCE_DENSITY)).0 < 1e-6,
        "unlit fuel makes no smoke"
    );

    // The heat rises.
    let (_, early) = mass_and_centroid(&temperature);
    burning.run(&gpu, 60..90);
    let (_, late) = mass_and_centroid(&burning.floats(&gpu, aestra_fluid::RESOURCE_TEMPERATURE));
    assert!(late[1] > early[1], "hot gas rises ({early:?} → {late:?})");

    // Temperature and fuel are state: a rerun reproduces them bit for bit.
    let again = Fluid::new(&gpu, &registry, &effect);
    again.run(&gpu, 0..90);
    for resource in [
        aestra_fluid::RESOURCE_TEMPERATURE,
        aestra_fluid::RESOURCE_FUEL,
    ] {
        assert_eq!(
            again
                .stage
                .read_resource(&gpu.device, &gpu.queue, resource)
                .unwrap(),
            burning
                .stage
                .read_resource(&gpu.device, &gpu.queue, resource)
                .unwrap()
        );
    }
}

#[test]
fn the_density_reaches_its_volume_texture_cell_for_cell() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &effect(&registry, false, 24));
    fluid.run(&gpu, 0..30);
    let density = fluid.floats(&gpu, RESOURCE_DENSITY);
    let layout = aestra_runtime::FieldLayout {
        resource: aestra_core::ResourceTypeId::new(RESOURCE_DENSITY),
        dims: [RESOLUTION; 3],
        components: 1,
        origin: [0.0; 3],
        cell_size: CELL_SIZE,
        staggered: false,
    };
    let size = wgpu::Extent3d {
        width: RESOLUTION,
        height: RESOLUTION,
        depth_or_array_layers: RESOLUTION,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("volume"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    // Rows of 16 texels × 8 bytes, padded to the 256-byte copy alignment.
    let row = 256u32;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("volume readback"),
        size: u64::from(row * RESOLUTION * RESOLUTION),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    FieldVolumePipeline::new(&gpu.device).encode(
        &gpu.device,
        &mut encoder,
        fluid.stage.buffer(RESOURCE_DENSITY).unwrap(),
        &texture.create_view(&Default::default()),
        &layout,
    );
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(RESOLUTION),
            },
        },
        size,
    );
    gpu.queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let bytes = readback.slice(..).get_mapped_range().to_vec();
    let n = RESOLUTION as usize;
    let mut compared = 0;
    for (index, expected) in density.iter().enumerate() {
        let (x, y, z) = (index % n, (index / n) % n, index / (n * n));
        let at = (z * n + y) * row as usize + x * 8;
        let red = half_to_f32(u16::from_le_bytes([bytes[at], bytes[at + 1]]));
        assert!(
            (red - expected).abs() <= 1e-3 + expected.abs() * 1e-3,
            "cell {index}: field {expected}, texture {red}"
        );
        compared += usize::from(*expected > 0.01);
    }
    assert!(
        compared > 20,
        "the plume is in the texture ({compared} cells)"
    );
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
    // …and what is left in the projected velocity: on the MAC grid, each cell's net flow out through
    // its six faces (component `axis` of a cell is its minimum face on that axis).
    let velocity = fluid.floats(&gpu, RESOURCE_VELOCITY);
    let face = |x, y, z, axis| velocity[index(x, y, z) * 4 + axis];
    let after = interior_rms(|x, y, z| {
        (face(x + 1, y, z, 0) - face(x, y, z, 0) + face(x, y + 1, z, 1) - face(x, y, z, 1)
            + face(x, y, z + 1, 2)
            - face(x, y, z, 2))
            / CELL_SIZE
    });
    eprintln!("divergence rms: before {before}, after {after}");
    assert!(before > 0.0);
    // The compact Laplacian is exactly the operator the MAC projection applies.
    assert!(
        after < before * 0.1,
        "projection removes the divergence: {before} -> {after}"
    );

    // No checkerboard: the pressure does not correlate with the odd/even pattern (-1)^(x+y+z) a
    // collocated grid's pressure modes would follow.
    let pressure = fluid.floats(&gpu, aestra_fluid::RESOURCE_PRESSURE);
    let (mut alternating, mut magnitude) = (0.0f64, 0.0f64);
    for z in 1..n - 1 {
        for y in 1..n - 1 {
            for x in 1..n - 1 {
                let p = f64::from(pressure[index(x, y, z)]);
                let sign = if (x + y + z) % 2 == 0 { 1.0 } else { -1.0 };
                alternating += sign * p;
                magnitude += p.abs();
            }
        }
    }
    let checkerboard = alternating.abs() / magnitude;
    eprintln!("pressure checkerboard ratio {checkerboard}");
    assert!(magnitude > 0.0);
    assert!(
        checkerboard < 0.02,
        "no odd/even pressure mode ({checkerboard})"
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

/// A timeline over the smoke stage, as the render world drives it.
fn timeline(gpu: &Gpu, registry: &ExtensionRegistry, policy: TimelinePolicy) -> StageTimeline {
    let fluid = Fluid::new(gpu, registry, &effect(registry, false, 24));
    StageTimeline::new(fluid.stage, policy, SEED)
}

/// One frame: advance toward `target` within `budget`, submitted like a render-graph frame.
fn advance(gpu: &Gpu, timeline: &mut StageTimeline, target: u32, budget: u32) -> u32 {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let report = timeline
        .advance_to(
            &gpu.device,
            &mut encoder,
            target,
            budget,
            StageInputs::default(),
            None,
        )
        .unwrap();
    gpu.queue.submit([encoder.finish()]);
    report.ticks
}

fn timeline_state(gpu: &Gpu, timeline: &StageTimeline) -> (Vec<u8>, Vec<u8>) {
    let read = |id| {
        timeline
            .executor()
            .read_resource(&gpu.device, &gpu.queue, id)
            .unwrap()
    };
    (read(RESOURCE_VELOCITY), read(RESOURCE_DENSITY))
}

#[test]
fn consecutive_dispatches_share_passes_without_changing_the_result() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &effect(&registry, false, 24));
    let passes = fluid.stage.passes_per_tick();
    let dispatches = fluid.stage.block().compute_pass_count() as usize;
    // Every copy ends a pass: sources .. advect + correct velocity | divergence + relax | 23 relax |
    // project + advect + correct density.
    assert_eq!((dispatches, passes), (35, 26));
    // Many ticks encoded into ONE submission each see their own frame: the result equals ticking
    // with one submission per tick.
    let mut timeline = timeline(&gpu, &registry, TimelinePolicy::default());
    assert_eq!(advance(&gpu, &mut timeline, 40, 1000), 40);
    fluid.run(&gpu, 0..40);
    assert_eq!(timeline_state(&gpu, &timeline), fluid.state(&gpu));
}

#[test]
fn scrubbing_reproduces_the_uninterrupted_run_bit_for_bit() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let uninterrupted = |target: u32| {
        let mut timeline = timeline(&gpu, &registry, TimelinePolicy::default());
        advance(&gpu, &mut timeline, target, u32::MAX);
        timeline_state(&gpu, &timeline)
    };
    let (at_35, at_90) = (uninterrupted(35), uninterrupted(90));

    let mut scrubbed = timeline(&gpu, &registry, TimelinePolicy::default());
    // Play forward in small per-frame budgets, as a preview catch-up does.
    while advance(&gpu, &mut scrubbed, 90, 24) > 0 {}
    assert_eq!(scrubbed.last_tick(), 90);
    assert_eq!(scrubbed.checkpoint_ticks(), [20, 40, 60, 80]);
    // Scrub back: restores the checkpoint at 20 and replays 15 ticks.
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let report = scrubbed
        .advance_to(
            &gpu.device,
            &mut encoder,
            35,
            u32::MAX,
            StageInputs::default(),
            None,
        )
        .unwrap();
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(report.restored_from, Some(20));
    assert_eq!(report.ticks, 15);
    assert_eq!(
        timeline_state(&gpu, &scrubbed),
        at_35,
        "scrubbed back to 35"
    );
    // Before the first checkpoint: reset to tick 0 and replay.
    advance(&gpu, &mut scrubbed, 7, u32::MAX);
    advance(&gpu, &mut scrubbed, 90, u32::MAX);
    assert_eq!(
        timeline_state(&gpu, &scrubbed),
        at_90,
        "scrubbed forward again"
    );
}

/// A dragged source (fluid F3): new constants apply to the running stage without a restart; the
/// history mixing both is never checkpointed, and a backward seek replays under the new constants.
#[test]
fn a_live_constant_edit_keeps_the_run_and_a_seek_replays_under_the_new_values() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let mut moved = effect(&registry, false, 24);
    set_input(
        &mut moved,
        MODULE_DENSITY_SOURCE,
        "position",
        Value::Vec3([0.6, 0.5, 0.0]),
    );
    let moved = Fluid::new(&gpu, &registry, &moved);
    let moved_constants = moved.stage.block().constants.clone();
    let mut placed = StageTimeline::new(moved.stage, TimelinePolicy::default(), SEED);
    advance(&gpu, &mut placed, 90, u32::MAX);
    let placed = timeline_state(&gpu, &placed);

    let mut live = timeline(&gpu, &registry, TimelinePolicy::default());
    advance(&gpu, &mut live, 40, u32::MAX);
    let before = timeline_state(&gpu, &live);
    assert!(live.set_constants(&gpu.queue, &moved_constants).unwrap());
    assert!(!live.set_constants(&gpu.queue, &moved_constants).unwrap());
    assert!(live.checkpoint_ticks().is_empty());
    // One more tick, from the live state: no restart.
    assert_eq!(advance(&gpu, &mut live, 41, u32::MAX), 1);
    assert_eq!(live.last_tick(), 41);
    advance(&gpu, &mut live, 90, u32::MAX);
    assert!(
        live.checkpoint_ticks().is_empty(),
        "a history mixing two sources is never checkpointed"
    );
    assert_ne!(timeline_state(&gpu, &live), before);
    // Seeking back replays from tick 0 under the new source: the run placed there from the start.
    advance(&gpu, &mut live, 10, u32::MAX);
    advance(&gpu, &mut live, 90, u32::MAX);
    assert_eq!(timeline_state(&gpu, &live), placed);
    assert_eq!(
        live.checkpoint_ticks(),
        [20, 40, 60, 80],
        "checkpoints resume"
    );
}

#[test]
fn checkpoint_memory_stays_within_the_policy() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let per_checkpoint = (RESOLUTION as u64).pow(3) * (16 + 4);
    let policy = TimelinePolicy {
        cadence: 5,
        max_checkpoints: 64,
        max_bytes: per_checkpoint * 5 / 2,
        ..TimelinePolicy::default()
    };
    let mut timeline = timeline(&gpu, &registry, policy);
    advance(&gpu, &mut timeline, 120, u32::MAX);
    assert!(timeline.checkpoint_bytes() <= policy.max_bytes);
    assert!(
        !timeline.checkpoint_ticks().is_empty(),
        "coarsened, not emptied"
    );
    // Seeking still reproduces the uninterrupted run from the coarser store.
    let mut reference = self::timeline(&gpu, &registry, TimelinePolicy::default());
    advance(&gpu, &mut reference, 77, u32::MAX);
    advance(&gpu, &mut timeline, 77, u32::MAX);
    assert_eq!(
        timeline_state(&gpu, &timeline),
        timeline_state(&gpu, &reference)
    );
}

#[test]
fn the_domain_lives_in_effect_space_and_host_inputs_are_converted_into_it() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let run = |host: [f32; 3], world_to_effect: [[f32; 4]; 3]| {
        let mut fluid = Fluid::new(&gpu, &registry, &effect(&registry, true, 24));
        fluid
            .instance
            .set_spatial_binding("Emitter", SpatialBindingSnapshot::at(host))
            .unwrap();
        fluid.run_placed(&gpu, 0..30, world_to_effect);
        fluid.state(&gpu)
    };
    // The effect at the origin with its host object at (-1, 0.5, 0)…
    let at_origin = run([-1.0, 0.5, 0.0], aestra_runtime::IDENTITY_AFFINE);
    // …equals the effect placed at x = 5 with the object moved the same way: the grid moved with the
    // effect and the world-space host position was brought into effect space.
    let moved = [
        [1.0, 0.0, 0.0, -5.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    assert_eq!(run([4.0, 0.5, 0.0], moved), at_origin, "bit for bit");
    assert_ne!(
        run([-1.0, 0.5, 0.0], moved),
        at_origin,
        "an object that did not move is elsewhere in effect space"
    );
}

/// Persistent-state slots (9 floats: position, velocity, age, lifetime, ordinal) — the stateful ABI.
fn particle_slots(positions: &[[f32; 3]], dead: usize) -> Vec<f32> {
    let mut state = Vec::new();
    for (index, position) in positions.iter().enumerate() {
        let lifetime = if index == dead { 0.0 } else { 10.0 };
        state.extend([
            position[0],
            position[1],
            position[2],
            0.0,
            0.0,
            0.0,
            0.0,
            lifetime,
            0.0,
        ]);
    }
    state
}

#[test]
fn particles_following_the_field_take_the_plumes_velocity() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &effect(&registry, false, 24));
    fluid.run(&gpu, 0..40);
    let layout = fluid
        .stage
        .block()
        .field(&aestra_core::ResourceTypeId::new(RESOURCE_VELOCITY))
        .unwrap()
        .clone();
    let follow = aestra_runtime::CompiledFieldFollow {
        stage: 0,
        field: layout,
        strength: 1.0e6, // pull clamps to 1: velocity becomes the sampled field value
    };
    // Particles up the plume's axis; slot 1 is dead and must be left alone.
    let positions: Vec<[f32; 3]> = (0..8).map(|i| [0.0, 0.6 + 0.2 * i as f32, 0.0]).collect();
    let initial = particle_slots(&positions, 1);
    let run = || {
        use wgpu::util::DeviceExt;
        let state = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("state"),
                contents: &initial
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<u8>>(),
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            });
        let pipeline = FieldFollowPipeline::new(&gpu.device);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let field = fluid.stage.buffer(RESOURCE_VELOCITY).unwrap();
        pipeline.encode(&gpu.device, &mut encoder, &state, 8, field, &follow, DT);
        gpu.queue.submit([encoder.finish()]);
        read_floats(&gpu, &state)
    };
    let followed = run();
    assert_eq!(run(), followed, "a rerun gives the same bits");
    let velocity = |slot: usize| {
        [
            followed[slot * 9 + 3],
            followed[slot * 9 + 4],
            followed[slot * 9 + 5],
        ]
    };
    assert_eq!(velocity(1), [0.0; 3], "a dead slot is not touched");
    // After 40 ticks the plume's head is low: every live particle is carried up, fastest near the
    // source, the pull fading with height.
    let live: Vec<usize> = (0..8).filter(|&slot| slot != 1).collect();
    assert!(
        live.iter().all(|&slot| velocity(slot)[1] > 0.0),
        "particles in the plume move up with it: {followed:?}"
    );
    assert!(velocity(0)[1] > 0.5, "near the source the plume is fast");
    assert!(
        live.windows(2)
            .all(|pair| velocity(pair[0])[1] > velocity(pair[1])[1]),
        "the pull fades with height"
    );
}

/// How high a puff launched by a short burst (a vortex ring) has risen after `ticks`, with or without
/// MacCormack advection — no buoyancy or confinement, so only advection carries it.
fn vortex_ring_height(gpu: &Gpu, registry: &ExtensionRegistry, sharp: bool, ticks: u32) -> f32 {
    let mut ring = effect(registry, false, 40);
    ring.simulation_stages[0].modules.retain(|module| {
        module.module_type.0 != aestra_fluid::MODULE_VORTICITY
            && module.module_type.0 != aestra_fluid::MODULE_BUOYANCY
    });
    set_input(
        &mut ring,
        MODULE_GRID,
        "sharp_advection",
        Value::Bool(sharp),
    );
    for (name, value) in [
        ("position", Value::Vec3([0.0, 0.5, 0.0])),
        ("radius", Value::Scalar(0.35)),
        ("velocity", Value::Vec3([0.0, 6.0, 0.0])),
        ("density_rate", Value::Scalar(30.0)),
    ] {
        set_input(&mut ring, MODULE_DENSITY_SOURCE, name, value);
    }
    // The same stage with its source moved far outside the grid: nothing more is injected.
    let mut quiet = ring.clone();
    set_input(
        &mut quiet,
        MODULE_DENSITY_SOURCE,
        "position",
        Value::Vec3([0.0, 100.0, 0.0]),
    );
    let quiet = Fluid::new(gpu, registry, &quiet)
        .stage
        .block()
        .constants
        .clone();
    let mut fluid = Fluid::new(gpu, registry, &ring);
    fluid.run(gpu, 0..16);
    fluid.stage.set_constants(&gpu.queue, &quiet).unwrap();
    fluid.run(gpu, 8..ticks);
    mass_and_centroid(&fluid.floats(gpu, RESOURCE_DENSITY)).1[1]
}

/// Fluid F4's benchmark: a vortex ring travels measurably farther with MacCormack advection than with
/// plain semi-Lagrangian at the same resolution — less numerical dissipation keeps its momentum.
#[test]
fn a_vortex_ring_travels_farther_with_maccormack_advection() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let plain = vortex_ring_height(&gpu, &registry, false, 60);
    let sharp = vortex_ring_height(&gpu, &registry, true, 60);
    eprintln!("vortex ring centroid height: semi-Lagrangian {plain}, MacCormack {sharp}");
    let launched_from = CENTER[1] - RESOLUTION as f32 * CELL_SIZE * 0.5 + 0.5;
    assert!(
        sharp - launched_from > (plain - launched_from) * 1.1,
        "MacCormack carries the ring at least 10% farther ({plain} vs {sharp})"
    );
}

/// A staggered (MAC) field is sampled at its faces (fluid F4): a linear field — each face holding its
/// own position — is reproduced exactly at any interior point, and read as cell-centred it is off by
/// half a cell.
#[test]
fn a_staggered_field_is_sampled_at_its_faces() {
    use wgpu::util::DeviceExt;
    let Some(gpu) = gpu() else { return };
    const N: usize = 8;
    const H: f32 = 0.5;
    let origin = [1.0f32, -2.0, 3.0];
    // Component `axis` of cell `i` sits on the cell's minimum face: at `i · H` along that axis.
    let mut faces = Vec::with_capacity(N * N * N * 4);
    for z in 0..N {
        for y in 0..N {
            for x in 0..N {
                faces.extend([x as f32 * H, y as f32 * H, z as f32 * H, 0.0]);
            }
        }
    }
    let field = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("staggered field"),
            contents: &faces
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<u8>>(),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let points = [[1.3, 1.1, 0.8], [2.0, 2.5, 1.7], [0.9, 0.75, 3.1]];
    let sample = |staggered: bool| {
        let positions: Vec<[f32; 3]> = points
            .iter()
            .map(|local| std::array::from_fn(|axis| origin[axis] + local[axis]))
            .collect();
        let state = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("state"),
                contents: &particle_slots(&positions, usize::MAX)
                    .iter()
                    .flat_map(|value| value.to_le_bytes())
                    .collect::<Vec<u8>>(),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
        let follow = aestra_runtime::CompiledFieldFollow {
            stage: 0,
            field: aestra_runtime::FieldLayout {
                resource: aestra_core::ResourceTypeId::new("test::resource/faces"),
                dims: [N as u32; 3],
                components: 4,
                origin,
                cell_size: H,
                staggered,
            },
            strength: 1.0e6,
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        FieldFollowPipeline::new(&gpu.device).encode(
            &gpu.device,
            &mut encoder,
            &state,
            points.len() as u32,
            &field,
            &follow,
            DT,
        );
        gpu.queue.submit([encoder.finish()]);
        read_floats(&gpu, &state)
    };
    let staggered = sample(true);
    let centred = sample(false);
    for (slot, local) in points.iter().enumerate() {
        for axis in 0..3 {
            let value = staggered[slot * 9 + 3 + axis];
            assert!(
                (value - local[axis]).abs() < 1e-5,
                "staggered: slot {slot} axis {axis}: {value} vs {}",
                local[axis]
            );
            let off = centred[slot * 9 + 3 + axis];
            assert!(
                (off - (local[axis] - 0.5 * H)).abs() < 1e-5,
                "cell-centred reading is half a cell off: {off}"
            );
        }
    }
}

fn read_floats(gpu: &Gpu, buffer: &wgpu::Buffer) -> Vec<f32> {
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &readback, 0, buffer.size());
    gpu.queue.submit([encoder.finish()]);
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(std::time::Duration::from_secs(60)),
        })
        .unwrap();
    let values = slice
        .get_mapped_range()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|bytes| f32::from_le_bytes(*bytes))
        .collect();
    readback.unmap();
    values
}
