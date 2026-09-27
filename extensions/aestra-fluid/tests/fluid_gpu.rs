//! Extensible-stages M13 on the real GPU: the fluid plugin's lowered solver runs on the render backend's
//! Execution IR executor. There is no CPU fluid to compare with (plan §44.6); correctness is
//! GPU-vs-GPU reproducibility plus physical sanity:
//!
//! - rerunning the same asset + seed + frames reproduces the same bits;
//! - restoring a checkpoint (only the persistent grids) and replaying reaches the uninterrupted state;
//! - the pressure projection leaves the velocity far closer to divergence-free than it found it;
//! - a density source bound to a host object follows that object.
//!
//! Simulations run serially on hardware compute adapters, not hosted software adapters. Shader
//! build checks still run on software adapters. Set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require
//! hardware; the native GPU workflow runs the complete simulation suite with that requirement.

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
use std::sync::{Arc, Mutex, MutexGuard};

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const DT: f32 = 1.0 / 60.0;
const SEED: u32 = 7;
/// A 16³ grid of 0.2 m cells: the default 3.2 m box, small enough to read back every tick.
const RESOLUTION: u32 = 16;
const CELL_SIZE: f32 = 0.2;
const CENTER: [f32; 3] = [0.0, 1.6, 0.0];

// Separate devices still share one adapter. Concurrent long fluid runs overwhelm software
// adapters and can time out readbacks or crash the native driver, hiding the original failure.
static GPU_TEST_LOCK: Mutex<()> = Mutex::new(());

fn exclusive_gpu() -> MutexGuard<'static, ()> {
    GPU_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn supports_simulation(device_type: wgpu::DeviceType, flags: wgpu::DownlevelFlags) -> bool {
    device_type != wgpu::DeviceType::Cpu && flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    // Released after the device and queue, so the next test cannot overlap their teardown.
    _exclusive: MutexGuard<'static, ()>,
}

fn gpu() -> Option<Gpu> {
    let exclusive = exclusive_gpu();
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
        supports_simulation(
            adapter.get_info().device_type,
            adapter.get_downlevel_capabilities().flags,
        )
    });
    let Some(adapter) = adapter else {
        assert!(
            std::env::var_os(REQUIRED_GPU_ENV).is_none(),
            "{REQUIRED_GPU_ENV} is set but no hardware compute adapter is available"
        );
        eprintln!("skipping fluid GPU simulation: no compatible hardware compute adapter");
        return None;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra fluid conformance"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("the adapter provides a device");
    Some(Gpu {
        device,
        queue,
        _exclusive: exclusive,
    })
}

#[test]
fn simulations_require_hardware_compute_but_not_a_specific_gpu_type() {
    let compute = wgpu::DownlevelFlags::COMPUTE_SHADERS;
    assert!(!supports_simulation(wgpu::DeviceType::Cpu, compute));
    for device_type in [
        wgpu::DeviceType::DiscreteGpu,
        wgpu::DeviceType::IntegratedGpu,
        wgpu::DeviceType::VirtualGpu,
        wgpu::DeviceType::Other,
    ] {
        assert!(supports_simulation(device_type, compute));
        assert!(!supports_simulation(
            device_type,
            wgpu::DownlevelFlags::empty()
        ));
    }
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

/// The effect with its pressure solved by Jacobi sweeps rather than MGPCG.
fn jacobi(mut effect: EffectAsset) -> EffectAsset {
    set_input(
        &mut effect,
        MODULE_GRID,
        "multigrid_pressure",
        Value::Bool(false),
    );
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
    let _exclusive = exclusive_gpu();
    let registry = registry();
    // With a collider, fire and flow maps, so every entry point is built.
    let mut everything = aestra_fluid::fire_effect(&registry);
    with_module(
        &registry,
        &mut everything,
        aestra_fluid::MODULE_CAPSULE_COLLIDER,
    );
    // And on a sparse grid (fluid F7), which runs without flow maps.
    let mut sparse_everything = sparse(everything.clone(), 64, 1e-3);
    set_input(
        &mut sparse_everything,
        MODULE_GRID,
        "resolution",
        Value::U32(64),
    );
    set_input(&mut everything, MODULE_GRID, "flow_map", Value::Bool(true));
    let blocks: Vec<_> = [everything, sparse_everything]
        .iter()
        .map(|effect| {
            EffectCompiler::with_extensions(registry.clone())
                .compile(effect)
                .unwrap()
                .extension_stages[0]
                .block
                .clone()
        })
        .collect();
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
        for block in &blocks {
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let executor = StageExecutor::new(&device, &queue, block, &registry.programs, 4);
            let error = pollster::block_on(scope.pop());
            assert!(
                executor.is_ok() && error.is_none(),
                "{backends:?}: {:?} {error:?}",
                executor.err()
            );
        }
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
        bricks: None,
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
        fluid.stage.field_buffers(&layout).unwrap(),
        &texture.create_view(&Default::default()),
        &layout,
        None,
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
        cells * (16 + 4 + 4) + 3 * 16,
        "only the persistent velocity, density and pressure grids (the pressure solve's warm \
         start, fluid F5), and the flow maps' state at its few bytes when they are off — no \
         scratch, no host inputs"
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
    // Every copy ends a pass: sources .. advect + correct velocity | divergence, the pressure solve
    // (MGPCG in a convergent repeat, or Jacobi's ping-pong: no copies either way, fluid F5), project,
    // advect + correct density.
    assert_eq!(fluid.stage.passes_per_tick(), 2);
    let jacobi = Fluid::new(&gpu, &registry, &jacobi(effect(&registry, false, 24)));
    assert_eq!(
        (
            jacobi.stage.block().compute_pass_count(),
            jacobi.stage.passes_per_tick()
        ),
        (35, 2)
    );
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

/// Particles up the plume's axis — slot 1 dead — each velocity pulled all the way to `fluid`'s
/// velocity field where it is: the state after one Follow Field pass.
fn follow_plume(gpu: &Gpu, fluid: &Fluid) -> Vec<f32> {
    use wgpu::util::DeviceExt;
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
    let positions: Vec<[f32; 3]> = (0..8).map(|i| [0.0, 0.6 + 0.2 * i as f32, 0.0]).collect();
    let state = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("state"),
            contents: &particle_slots(&positions, 1)
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<u8>>(),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        });
    let pipeline = FieldFollowPipeline::new(&gpu.device);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let field = fluid.stage.field_buffers(&follow.field).unwrap();
    pipeline.encode(&gpu.device, &mut encoder, &state, 8, field, &follow, DT);
    gpu.queue.submit([encoder.finish()]);
    read_floats(gpu, &state)
}

#[test]
fn particles_following_the_field_take_the_plumes_velocity() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &effect(&registry, false, 24));
    fluid.run(&gpu, 0..40);
    let followed = follow_plume(&gpu, &fluid);
    assert_eq!(
        follow_plume(&gpu, &fluid),
        followed,
        "a rerun gives the same bits"
    );
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

fn with_module(
    registry: &ExtensionRegistry,
    effect: &mut EffectAsset,
    type_id: &str,
) -> aestra_core::ModuleId {
    let mut module = registry
        .modules
        .instantiate(&aestra_core::ModuleTypeId::new(type_id))
        .unwrap();
    module.stage = aestra_core::StageKind::Simulation(aestra_fluid::SMOKE_STAGE.into());
    let id = module.id;
    effect.simulation_stages[0].modules.push(module);
    id
}

fn set_module_input(effect: &mut EffectAsset, id: aestra_core::ModuleId, name: &str, value: Value) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.id == id)
        .unwrap();
    let ModuleParameters::Custom(values) = &mut module.parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    values.insert(name.into(), value);
}

/// Mean x-velocity over the cells in `x_range` (cell indices) around the grid's middle in y and z.
fn mean_x_velocity(velocity: &[f32], x_range: std::ops::Range<usize>, y: usize) -> f32 {
    let n = RESOLUTION as usize;
    let mut sum = 0.0;
    let mut count = 0.0;
    for z in n / 2 - 1..=n / 2 {
        for yy in y - 1..=y {
            for x in x_range.clone() {
                sum += velocity[((z * n + yy) * n + x) * 4];
                count += 1.0;
            }
        }
    }
    sum / count
}

/// Fluid F4: a sphere a host object moves through still fluid drags a wake behind it.
#[test]
fn a_moving_sphere_carves_a_wake() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let speed = 2.0;
    let wake = |with_sphere: bool| {
        let mut still = effect(&registry, false, 40);
        // No source: the fluid is at rest until the sphere moves through it.
        set_input(
            &mut still,
            MODULE_DENSITY_SOURCE,
            "position",
            Value::Vec3([0.0, 100.0, 0.0]),
        );
        let mut paddle = EffectBinding::spatial("Paddle", BindingUpdateMode::Live);
        paddle
            .optional_fields
            .insert(aestra_core::BindingFieldId::new(
                aestra_core::AESTRA_FIELD_LINEAR_VELOCITY,
            ));
        let paddle_id = paddle.id;
        still.bindings = vec![paddle];
        if with_sphere {
            let sphere = with_module(&registry, &mut still, aestra_fluid::MODULE_SPHERE_COLLIDER);
            set_module_input(&mut still, sphere, "radius", Value::Scalar(0.4));
            let module = still.simulation_stages[0]
                .modules
                .iter_mut()
                .find(|module| module.id == sphere)
                .unwrap();
            for (input, field) in [
                ("position", AESTRA_FIELD_POSITION),
                ("velocity", aestra_core::AESTRA_FIELD_LINEAR_VELOCITY),
            ] {
                module
                    .property_sources
                    .insert(input.into(), PropertySource::HostBinding);
                module
                    .host_bindings
                    .insert(input.into(), HostFieldRef::new(paddle_id, field));
            }
        }
        let mut fluid = Fluid::new(&gpu, &registry, &still);
        // The sphere crosses the grid's middle in +x: from x = -1.0 to x = 0.0 (30 ticks).
        for tick in 0..30u32 {
            let x = -1.0 + speed * tick as f32 * DT;
            let mut snapshot = SpatialBindingSnapshot::at([x, 1.6, 0.0]);
            snapshot.linear_velocity = Some([speed, 0.0, 0.0]);
            fluid
                .instance
                .set_spatial_binding("Paddle", snapshot)
                .unwrap();
            fluid.run(&gpu, tick..tick + 1);
        }
        fluid.floats(&gpu, RESOURCE_VELOCITY)
    };
    // The sphere ends at x = 0 (cell 8 of 16): the wake is the cells behind it, 2.5–4.5 cells back.
    let behind = |velocity: &[f32]| mean_x_velocity(velocity, 4..6, 8);
    let with_sphere = behind(&wake(true));
    let without = behind(&wake(false));
    eprintln!("wake x-velocity: with sphere {with_sphere}, without {without}");
    assert!(without.abs() < 1e-4, "still fluid stays still");
    assert!(
        with_sphere > 0.1 * speed,
        "the fluid behind the sphere follows it ({with_sphere})"
    );
}

/// Fluid F4: nothing flows through a still solid, and it holds no smoke.
#[test]
fn a_still_collider_blocks_the_plume() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let mut blocked = effect(&registry, false, 40);
    let block = with_module(&registry, &mut blocked, aestra_fluid::MODULE_BOX_COLLIDER);
    // A slab above the source, in the plume's path.
    set_module_input(
        &mut blocked,
        block,
        "position",
        Value::Vec3([0.0, 1.4, 0.0]),
    );
    set_module_input(
        &mut blocked,
        block,
        "half_extents",
        Value::Vec3([0.8, 0.2, 0.8]),
    );
    let fluid = Fluid::new(&gpu, &registry, &blocked);
    fluid.run(&gpu, 0..40);
    let velocity = fluid.floats(&gpu, RESOURCE_VELOCITY);
    let density = fluid.floats(&gpu, RESOURCE_DENSITY);
    let n = RESOLUTION as usize;
    let index = |x: usize, y: usize, z: usize| (z * n + y) * n + x;
    // Cells whose centre is inside the slab: |x|, |z| < 0.8 and |y - 1.4| < 0.2.
    let inside = |x: usize, y: usize, z: usize| {
        let centre = cell_center(index(x, y, z));
        centre[0].abs() < 0.8 && (centre[1] - 1.4).abs() < 0.2 && centre[2].abs() < 0.8
    };
    let (mut solid_cells, mut through) = (0, 0.0f32);
    for z in 0..n {
        for y in 1..n {
            for x in 0..n {
                if inside(x, y, z) {
                    solid_cells += 1;
                    assert_eq!(density[index(x, y, z)], 0.0, "no smoke inside the solid");
                    // The y face between the solid and the fluid below carries nothing.
                    if !inside(x, y - 1, z) {
                        through = through.max(velocity[index(x, y, z) * 4 + 1].abs());
                    }
                }
            }
        }
    }
    assert!(solid_cells > 0);
    assert_eq!(through, 0.0, "no flow into the still solid");
    // The plume is pushed aside: under the slab it spreads sideways.
    let (mass, _) = mass_and_centroid(&density);
    assert!(mass > 0.0);
}

/// Fluid F4: a sticky (no-slip) collider drags the fluid sliding past it; a slippery one does not.
#[test]
fn a_sticky_surface_drags_the_fluid_and_a_slippery_one_does_not() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let drag = |sticky: bool| {
        let mut belt = effect(&registry, false, 40);
        set_input(
            &mut belt,
            MODULE_DENSITY_SOURCE,
            "position",
            Value::Vec3([0.0, 100.0, 0.0]),
        );
        // A slab across the whole grid whose surface moves in +x, like a conveyor belt: it stays
        // where it is and has no end faces pushing fluid, only its top surface moving along.
        let slab = with_module(&registry, &mut belt, aestra_fluid::MODULE_BOX_COLLIDER);
        set_module_input(&mut belt, slab, "position", Value::Vec3([0.0, 1.0, 0.0]));
        set_module_input(
            &mut belt,
            slab,
            "half_extents",
            Value::Vec3([2.0, 0.3, 2.0]),
        );
        set_module_input(&mut belt, slab, "velocity", Value::Vec3([2.0, 0.0, 0.0]));
        set_module_input(&mut belt, slab, "no_slip", Value::Bool(sticky));
        let fluid = Fluid::new(&gpu, &registry, &belt);
        fluid.run(&gpu, 0..10);
        // Solid rows are y = 4, 5 (centres 0.9, 1.1); the first fluid row on top is y = 6.
        let velocity = fluid.floats(&gpu, RESOURCE_VELOCITY);
        let n = RESOLUTION as usize;
        let row: Vec<f32> = (4..12)
            .flat_map(|x| (6..10).map(move |z| (x, z)))
            .map(|(x, z)| velocity[((z * n + 6) * n + x) * 4])
            .collect();
        row.iter().sum::<f32>() / row.len() as f32
    };
    let (sticky, slippery) = (drag(true), drag(false));
    eprintln!("fluid over the belt: sticky {sticky}, slippery {slippery}");
    assert!(sticky > 1.0, "the sticky belt carries the fluid ({sticky})");
    assert!(
        slippery.abs() < sticky * 0.25,
        "the slippery belt barely moves it ({slippery} vs {sticky})"
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
                bricks: None,
            },
            strength: 1.0e6,
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        FieldFollowPipeline::new(&gpu.device).encode(
            &gpu.device,
            &mut encoder,
            &state,
            points.len() as u32,
            aestra_bevy_render::execution::FieldBuffers {
                field: &field,
                table: None,
            },
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

/// Turbulence: masked, it leaves still air perfectly still; unmasked, it stirs it. And a steady plume,
/// which rises straight up the grid's middle on its own, is pushed off that line by it.
#[test]
fn turbulence_stirs_only_where_there_is_smoke_and_breaks_the_plume_s_symmetry() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let turbulent = |masked: bool, source: bool| {
        let mut effect = effect(&registry, false, 20);
        if !source {
            set_input(
                &mut effect,
                MODULE_DENSITY_SOURCE,
                "density_rate",
                Value::Scalar(0.0),
            );
            set_input(
                &mut effect,
                MODULE_DENSITY_SOURCE,
                "velocity",
                Value::Vec3([0.0; 3]),
            );
        }
        let id = with_module(&registry, &mut effect, aestra_fluid::MODULE_TURBULENCE);
        set_module_input(&mut effect, id, "strength", Value::Scalar(6.0));
        set_module_input(&mut effect, id, "scale", Value::Scalar(0.8));
        set_module_input(&mut effect, id, "masked", Value::Bool(masked));
        effect
    };
    let peak = |effect: &EffectAsset| {
        let fluid = Fluid::new(&gpu, &registry, effect);
        fluid.run(&gpu, 0..30);
        fluid
            .floats(&gpu, RESOURCE_VELOCITY)
            .iter()
            .fold(0.0f32, |peak, v| peak.max(v.abs()))
    };
    assert_eq!(
        peak(&turbulent(true, false)),
        0.0,
        "masked: no smoke, no push"
    );
    assert!(
        peak(&turbulent(false, false)) > 0.05,
        "unmasked: the air is stirred"
    );

    let drift = |effect: &EffectAsset| {
        let fluid = Fluid::new(&gpu, &registry, effect);
        fluid.run(&gpu, 0..90);
        let (_, centroid) = mass_and_centroid(&fluid.floats(&gpu, RESOURCE_DENSITY));
        ((centroid[0] - CENTER[0]).powi(2) + (centroid[2] - CENTER[2]).powi(2)).sqrt()
    };
    let calm = drift(&effect(&registry, false, 20));
    let stirred = drift(&turbulent(true, true));
    eprintln!("plume off-axis: calm {calm}, stirred {stirred}");
    assert!(
        stirred > 10.0 * calm.max(1e-4),
        "calm {calm}, stirred {stirred}"
    );
}

/// MGPCG (fluid F5) solves the pressure to its tolerance in few iterations — in an open box, a box
/// closed on every side (whose right-hand side loses its mean) and around a collider (whose solids the
/// coarse levels inherit) — and leaves far less divergence than Jacobi's fixed sweeps on a 48³ grid.
#[test]
fn mgpcg_converges_to_its_tolerance_and_leaves_far_less_divergence_than_jacobi() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let n = 48usize;
    let cell = 3.2 / n as f32;
    let fine = |mut effect: EffectAsset| {
        set_input(&mut effect, MODULE_GRID, "resolution", Value::U32(n as u32));
        set_input(&mut effect, MODULE_GRID, "cell_size", Value::Scalar(cell));
        set_input(
            &mut effect,
            MODULE_GRID,
            "pressure_tolerance",
            Value::Scalar(1e-4),
        );
        effect
    };
    let index = |x: usize, y: usize, z: usize| (z * n + y) * n + x;
    let rms = |values: &dyn Fn(usize, usize, usize) -> f32| {
        let (mut sum, mut count) = (0.0f64, 0.0f64);
        for z in 2..n - 2 {
            for y in 2..n - 2 {
                for x in 2..n - 2 {
                    sum += f64::from(values(x, y, z)).powi(2);
                    count += 1.0;
                }
            }
        }
        (sum / count).sqrt()
    };
    // Divergence before and after the last tick's projection; the solve's iterations and residual.
    let solve = |effect: &EffectAsset| {
        let fluid = Fluid::new(&gpu, &registry, effect);
        fluid.run(&gpu, 0..30);
        let before = fluid.floats(&gpu, RESOURCE_DIVERGENCE);
        let before = rms(&|x, y, z| before[index(x, y, z)]);
        let velocity = fluid.floats(&gpu, RESOURCE_VELOCITY);
        let face = |x, y, z, axis| velocity[index(x, y, z) * 4 + axis];
        let after = rms(&|x, y, z| {
            (face(x + 1, y, z, 0) - face(x, y, z, 0) + face(x, y + 1, z, 1) - face(x, y, z, 1)
                + face(x, y, z + 1, 2)
                - face(x, y, z, 2))
                / cell
        });
        let iterations = fluid
            .stage
            .convergent_iterations(&gpu.device, &gpu.queue)
            .unwrap();
        let residual = fluid.floats(&gpu, aestra_fluid::RESOURCE_PCG_REDUCTION)[0];
        (before, after, iterations, residual)
    };

    let (before, after, iterations, residual) = solve(&fine(effect(&registry, false, 50)));
    let (jacobi_before, jacobi_after, _, _) = solve(&fine(jacobi(effect(&registry, false, 24))));
    eprintln!(
        "48³ open top: MGPCG {iterations:?} iterations, residual {residual}, divergence \
         {before} -> {after}; Jacobi ×24 {jacobi_before} -> {jacobi_after}"
    );
    assert_eq!(iterations.len(), 1);
    assert!(iterations[0] < 50, "converged before the cap");
    assert!(residual <= 1e-4, "to the tolerance: {residual}");
    assert!(after < before * 1e-3, "{before} -> {after}");
    assert!(
        after < jacobi_after * 0.05,
        "{after} vs Jacobi {jacobi_after}"
    );

    let mut closed = fine(effect(&registry, false, 50));
    set_input(&mut closed, MODULE_GRID, "open_top", Value::Bool(false));
    let (before, after, iterations, residual) = solve(&closed);
    eprintln!("closed box: {iterations:?} iterations, residual {residual}, {before} -> {after}");
    assert!(iterations[0] < 50 && residual <= 1e-4);
    assert!(after < before * 1e-3, "{before} -> {after}");

    let mut obstructed = fine(effect(&registry, false, 50));
    let sphere = with_module(
        &registry,
        &mut obstructed,
        aestra_fluid::MODULE_SPHERE_COLLIDER,
    );
    set_module_input(
        &mut obstructed,
        sphere,
        "position",
        Value::Vec3([0.0, 1.6, 0.0]),
    );
    set_module_input(&mut obstructed, sphere, "radius", Value::Scalar(0.5));
    let (_, _, iterations, residual) = solve(&obstructed);
    eprintln!("around a sphere: {iterations:?} iterations, residual {residual}");
    assert!(iterations[0] < 50 && residual <= 1e-4);
}
/// A slab across the whole grid seals the region under it: its pressure is known only up to a
/// constant. Held still, the region takes no net inflow and the solve converges as anywhere else; a slab
/// moving up would draw fluid out of it — impossible for an incompressible fluid — and the solve must
/// stay bounded rather than diverge (fluid F5).
#[test]
fn a_sealed_region_stays_solvable() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let sealed = |velocity: [f32; 3]| {
        let mut effect = effect(&registry, false, 40);
        let slab = with_module(&registry, &mut effect, aestra_fluid::MODULE_BOX_COLLIDER);
        set_module_input(&mut effect, slab, "position", Value::Vec3([0.0, 1.0, 0.0]));
        set_module_input(
            &mut effect,
            slab,
            "half_extents",
            Value::Vec3([2.0, 0.3, 2.0]),
        );
        set_module_input(&mut effect, slab, "velocity", Value::Vec3(velocity));
        let fluid = Fluid::new(&gpu, &registry, &effect);
        let mut iterations = Vec::new();
        for tick in 0..30 {
            fluid.run(&gpu, tick..tick + 1);
            iterations.extend(
                fluid
                    .stage
                    .convergent_iterations(&gpu.device, &gpu.queue)
                    .unwrap(),
            );
        }
        let peak = fluid
            .floats(&gpu, RESOURCE_VELOCITY)
            .iter()
            .fold(0.0f32, |peak, v| peak.max(v.abs()));
        (iterations, peak)
    };
    let (iterations, peak) = sealed([2.0, 0.0, 0.0]);
    eprintln!("sealed, sliding: iterations {iterations:?}, peak speed {peak}");
    assert!(iterations[1..].iter().all(|&count| count < 40), "converges");
    assert!(peak < 10.0, "no blow-up ({peak})");
    let (iterations, peak) = sealed([0.0, 1.0, 0.0]);
    eprintln!("sealed, drawn from: iterations {iterations:?}, peak speed {peak}");
    assert!(peak.is_finite() && peak < 50.0, "bounded ({peak})");
}
/// The pressure solve's cost per tick, Jacobi against MGPCG (fluid F5): many ticks of a plume, timed on
/// the device. Not a pass/fail test — run with `--ignored --nocapture` on the reference GPU.
#[test]
#[ignore]
fn bench_pressure_solvers() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    for n in [48u32, 96, 128] {
        for (label, multigrid, iterations) in [
            ("Jacobi ×24", false, 24u32),
            ("MGPCG cap 24", true, 24),
            ("MGPCG cap 12", true, 12),
        ] {
            let mut plume = effect(&registry, false, iterations);
            set_input(&mut plume, MODULE_GRID, "resolution", Value::U32(n));
            set_input(
                &mut plume,
                MODULE_GRID,
                "cell_size",
                Value::Scalar(3.2 / n as f32),
            );
            set_input(
                &mut plume,
                MODULE_GRID,
                "multigrid_pressure",
                Value::Bool(multigrid),
            );
            let fluid = Fluid::new(&gpu, &registry, &plume);
            fluid.run(&gpu, 0..60);
            let wait = || {
                gpu.device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(120)),
                    })
                    .unwrap();
            };
            wait();
            let ticks = 120;
            let start = std::time::Instant::now();
            fluid.run(&gpu, 60..60 + ticks);
            wait();
            let per_tick = start.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks);
            let iterations = if multigrid {
                fluid
                    .stage
                    .convergent_iterations(&gpu.device, &gpu.queue)
                    .unwrap()[0]
            } else {
                iterations
            };
            eprintln!("BENCH {n}³ {label}: {per_tick:.3} ms/tick ({iterations} iterations)");
        }
    }
}

/// A 48³ vortex ring launched by a short burst and then left alone (no forces, no dissipation), with
/// its kinetic energy after each of `samples` spans of `span` free ticks.
fn free_ring_energy(
    gpu: &Gpu,
    registry: &ExtensionRegistry,
    flow_map: bool,
    span: u32,
    samples: u32,
) -> Vec<f32> {
    let n = 48u32;
    let mut ring = effect(registry, false, 40);
    ring.simulation_stages[0].modules.retain(|module| {
        module.module_type.0 != aestra_fluid::MODULE_VORTICITY
            && module.module_type.0 != aestra_fluid::MODULE_BUOYANCY
    });
    for (name, value) in [
        ("resolution", Value::U32(n)),
        ("cell_size", Value::Scalar(3.2 / n as f32)),
        ("velocity_dissipation", Value::Scalar(0.0)),
        ("flow_map", Value::Bool(flow_map)),
    ] {
        set_input(&mut ring, MODULE_GRID, name, value);
    }
    for (name, value) in [
        ("position", Value::Vec3([0.0, 0.5, 0.0])),
        ("radius", Value::Scalar(0.35)),
        ("velocity", Value::Vec3([0.0, 6.0, 0.0])),
        ("density_rate", Value::Scalar(30.0)),
    ] {
        set_input(&mut ring, MODULE_DENSITY_SOURCE, name, value);
    }
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
    (0..samples)
        .map(|sample| {
            fluid.run(gpu, 16 + sample * span..16 + (sample + 1) * span);
            fluid
                .floats(gpu, RESOURCE_VELOCITY)
                .iter()
                .map(|v| v * v)
                .sum::<f32>()
                * 0.5
        })
        .collect()
}

/// Fluid F6's benchmark: a free vortex ring keeps far more of its energy with leapfrog flow maps than
/// with MacCormack advection at the same resolution — and, left alone for 400 ticks after it has
/// left the open top, the flow map never gains energy it was not given.
#[test]
fn flow_maps_keep_a_vortex_ring_s_energy() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let advected = free_ring_energy(&gpu, &registry, false, 96, 1)[0];
    let mapped = free_ring_energy(&gpu, &registry, true, 48, 8);
    eprintln!(
        "ring energy after 96 free ticks: MacCormack {advected}, flow map {}",
        mapped[1]
    );
    eprintln!("flow map energy every 48 ticks: {mapped:?}");
    assert!(
        mapped[1] > advected * 1.25,
        "flow maps keep a quarter more ({advected} vs {})",
        mapped[1]
    );
    assert!(
        mapped
            .windows(2)
            .skip(1)
            .all(|pair| pair[1] <= pair[0] * 1.02),
        "no energy from nowhere: {mapped:?}"
    );
}

/// Flow-map state is persistent and checkpointed with the rest: restoring a checkpoint taken in the
/// middle of a cycle and replaying reaches the uninterrupted state bit for bit (fluid F6).
#[test]
fn flow_maps_replay_a_mid_cycle_checkpoint_bit_for_bit() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let mut mapped = effect(&registry, false, 24);
    set_input(&mut mapped, MODULE_GRID, "flow_map", Value::Bool(true));
    set_input(&mut mapped, MODULE_GRID, "flow_map_cycle", Value::U32(8));
    let fluid = Fluid::new(&gpu, &registry, &mapped);
    // Tick 20 is step 4 of its cycle.
    fluid.run(&gpu, 0..20);
    let checkpoint = fluid.stage.checkpoint(&gpu.device, &gpu.queue).unwrap();
    let cells = (RESOLUTION as usize).pow(3);
    assert_eq!(
        checkpoint.bytes(),
        cells * (16 + 4 + 4 + 12 * 8 + 16 + 72),
        "velocity, density, pressure, then the flow maps: 8 stored steps, the cycle's initial \
         velocity and the forward maps"
    );
    fluid.run(&gpu, 20..45);
    let uninterrupted = (
        fluid.state(&gpu),
        fluid.floats(&gpu, aestra_fluid::RESOURCE_LFM_FORWARD),
    );
    fluid.stage.restore(&gpu.queue, &checkpoint).unwrap();
    fluid.run(&gpu, 20..45);
    assert_eq!(
        (
            fluid.state(&gpu),
            fluid.floats(&gpu, aestra_fluid::RESOURCE_LFM_FORWARD)
        ),
        uninterrupted
    );
}

/// A fire — strong buoyancy, open sides, a source pulling the flow — keeps bounded on flow maps: where
/// the maps distort too much over a cycle, the safeguard starts the next cycle from the midpoint
/// velocity instead of feeding the error (fluid F6).
#[test]
fn a_fire_on_flow_maps_stays_bounded() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let peak = |flow_map: bool| {
        let mut fire = aestra_fluid::fire_effect(&registry);
        set_input(&mut fire, MODULE_GRID, "open_sides", Value::Bool(true));
        set_input(&mut fire, MODULE_GRID, "flow_map", Value::Bool(flow_map));
        set_input(&mut fire, MODULE_GRID, "flow_map_cycle", Value::U32(16));
        let fluid = Fluid::new(&gpu, &registry, &fire);
        let mut peak = 0.0f32;
        for span in 0..10 {
            fluid.run(&gpu, span * 30..(span + 1) * 30);
            let velocity = fluid.floats(&gpu, RESOURCE_VELOCITY);
            assert!(
                velocity.iter().all(|v| v.is_finite()),
                "finite at tick {}",
                (span + 1) * 30
            );
            peak = peak.max(velocity.iter().fold(0.0f32, |p, v| p.max(v.abs())));
        }
        peak
    };
    let (advected, mapped) = (peak(false), peak(true));
    eprintln!("fire peak speed over 300 ticks: MacCormack {advected}, flow map {mapped}");
    assert!(mapped < advected * 3.0, "bounded: {mapped} vs {advected}");
}
/// Flow maps' cost per tick against MacCormack (fluid F6): a buoyant plume, and the same with fire,
/// timed on the device over whole cycles. Not a pass/fail test — run with `--ignored --nocapture` on
/// the reference GPU.
#[test]
#[ignore]
fn bench_flow_maps() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    for n in [48u32, 64, 96] {
        for burning in [false, true] {
            for (label, flow_map, cycle) in [
                ("MacCormack", false, 8u32),
                ("flow map ×4", true, 4),
                ("flow map ×8", true, 8),
            ] {
                let mut plume = if burning {
                    aestra_fluid::fire_effect(&registry)
                } else {
                    effect(&registry, false, 12)
                };
                set_input(&mut plume, MODULE_GRID, "resolution", Value::U32(n));
                set_input(
                    &mut plume,
                    MODULE_GRID,
                    "cell_size",
                    Value::Scalar(3.2 / n as f32),
                );
                set_input(&mut plume, MODULE_GRID, "center", Value::Vec3(CENTER));
                set_input(&mut plume, MODULE_GRID, "flow_map", Value::Bool(flow_map));
                set_input(&mut plume, MODULE_GRID, "flow_map_cycle", Value::U32(cycle));
                let fluid = Fluid::new(&gpu, &registry, &plume);
                let wait = || {
                    gpu.device
                        .poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: Some(std::time::Duration::from_secs(120)),
                        })
                        .unwrap();
                };
                fluid.run(&gpu, 0..64);
                wait();
                let ticks = 128;
                let start = std::time::Instant::now();
                fluid.run(&gpu, 64..64 + ticks);
                wait();
                let per_tick = start.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks);
                let state_mb = fluid.stage.persistent_bytes() as f64 / (1024.0 * 1024.0);
                let kind = if burning { "fire" } else { "smoke" };
                eprintln!(
                    "BENCH {n}³ {kind} {label}: {per_tick:.3} ms/tick, state {state_mb:.1} MiB"
                );
            }
        }
    }
}

// ---- Sparse bricks (fluid F7) ----

/// `effect` on a sparse grid of `budget` bricks, each kept while a cell holds more than `threshold`
/// (negative: every brick is kept, so the grid is stored whole).
fn sparse(mut effect: EffectAsset, budget: u32, threshold: f32) -> EffectAsset {
    set_input(&mut effect, MODULE_GRID, "sparse", Value::Bool(true));
    set_input(&mut effect, MODULE_GRID, "brick_budget", Value::U32(budget));
    set_input(
        &mut effect,
        MODULE_GRID,
        "brick_threshold",
        Value::Scalar(threshold),
    );
    effect
}

/// A sparse grid's bricks as read back: the active slots in list order, each slot's packed brick
/// (+1; 0 when free) and each brick's slot (0 when inactive).
struct Bricks {
    list: Vec<u32>,
    coords: Vec<u32>,
    table: Vec<u32>,
}

impl Fluid {
    fn words(&self, gpu: &Gpu, resource: &str) -> Vec<u32> {
        self.floats(gpu, resource)
            .into_iter()
            .map(f32::to_bits)
            .collect()
    }

    fn bricks(&self, gpu: &Gpu, budget: u32, resolution: u32) -> Bricks {
        let words = self.words(gpu, aestra_fluid::RESOURCE_BRICKS);
        let slots = budget as usize + 1;
        let count = words[0] as usize;
        let table = ((resolution / 8) as usize).pow(3);
        Bricks {
            list: words[16..16 + count].to_vec(),
            coords: words[16 + slots..16 + 2 * slots].to_vec(),
            table: words[16 + 2 * slots..16 + 2 * slots + table].to_vec(),
        }
    }

    /// A sparse field laid out like the dense grid (inactive bricks zero), `components` floats a cell.
    fn unbricked(
        &self,
        gpu: &Gpu,
        resource: &str,
        components: usize,
        budget: u32,
        resolution: u32,
    ) -> Vec<f32> {
        let pool = self.floats(gpu, resource);
        let bricks = self.bricks(gpu, budget, resolution);
        let (n, g) = (resolution as usize, resolution as usize / 8);
        let mut grid = vec![0.0; n * n * n * components];
        for z in 0..n {
            for y in 0..n {
                for x in 0..n {
                    let slot = bricks.table[((z / 8) * g + y / 8) * g + x / 8] as usize;
                    let from = (slot * 512 + ((z % 8) * 8 + y % 8) * 8 + x % 8) * components;
                    let to = ((z * n + y) * n + x) * components;
                    grid[to..to + components].copy_from_slice(&pool[from..from + components]);
                }
            }
        }
        grid
    }
}

fn bits(values: &[f32]) -> Vec<u32> {
    values.iter().map(|value| value.to_bits()).collect()
}

#[test]
fn a_sparse_grid_stored_whole_computes_what_the_dense_grid_does() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let all = (RESOLUTION / 8).pow(3);
    // Jacobi sweeps are per-cell arithmetic only: stored whole, the bricks agree bit for bit.
    let dense = Fluid::new(&gpu, &registry, &jacobi(effect(&registry, false, 24)));
    let whole = Fluid::new(
        &gpu,
        &registry,
        &sparse(jacobi(effect(&registry, false, 24)), all, -1.0),
    );
    dense.run(&gpu, 0..40);
    whole.run(&gpu, 0..40);
    assert_eq!(
        whole.bricks(&gpu, all, RESOLUTION).list.len(),
        all as usize,
        "every brick stored"
    );
    for (resource, components) in [
        (RESOURCE_VELOCITY, 4),
        (RESOURCE_DENSITY, 1),
        (aestra_fluid::RESOURCE_PRESSURE, 1),
    ] {
        assert_eq!(
            bits(&whole.unbricked(&gpu, resource, components, all, RESOLUTION)),
            bits(&dense.floats(&gpu, resource)),
            "{resource}"
        );
    }

    // MGPCG: the bricks' V-cycle goes one level coarser and sums in another order, so the two agree
    // to the solve's tolerance, not bit for bit.
    let dense = Fluid::new(&gpu, &registry, &effect(&registry, false, 24));
    let whole = Fluid::new(
        &gpu,
        &registry,
        &sparse(effect(&registry, false, 24), all, -1.0),
    );
    dense.run(&gpu, 0..40);
    whole.run(&gpu, 0..40);
    let expected = dense.floats(&gpu, RESOURCE_DENSITY);
    let got = whole.unbricked(&gpu, RESOURCE_DENSITY, 1, all, RESOLUTION);
    let peak = expected.iter().fold(0.0f32, |p, d| p.max(d.abs()));
    let worst = expected
        .iter()
        .zip(&got)
        .fold(0.0f32, |w, (a, b)| w.max((a - b).abs()));
    eprintln!("sparse vs dense MGPCG density: worst {worst} of peak {peak}");
    assert!(worst < 1e-2 * peak, "{worst} vs peak {peak}");
}

/// The smoke plume on a sparse 64³ grid of 0.05-unit cells: the test box, 8³ bricks of it.
const SPARSE_RESOLUTION: u32 = 64;

fn sparse_plume(registry: &ExtensionRegistry, budget: u32) -> EffectAsset {
    let mut plume = sparse(effect(registry, false, 24), budget, 1e-3);
    set_input(
        &mut plume,
        MODULE_GRID,
        "resolution",
        Value::U32(SPARSE_RESOLUTION),
    );
    set_input(&mut plume, MODULE_GRID, "cell_size", Value::Scalar(0.05));
    plume
}

#[test]
fn a_sparse_plume_simulates_only_the_bricks_around_its_smoke() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let budget = 512;
    let fluid = Fluid::new(&gpu, &registry, &sparse_plume(&registry, budget));
    let n = SPARSE_RESOLUTION as usize;
    let height = |density: &[f32]| {
        let mass: f32 = density.iter().sum();
        let lift: f32 = density
            .iter()
            .enumerate()
            .map(|(index, d)| d * (index / n % n) as f32)
            .sum();
        (mass, lift / mass)
    };
    fluid.run(&gpu, 0..20);
    let (_, early) = height(&fluid.unbricked(&gpu, RESOURCE_DENSITY, 1, budget, SPARSE_RESOLUTION));
    fluid.run(&gpu, 20..60);
    let bricks = fluid.bricks(&gpu, budget, SPARSE_RESOLUTION);
    let total = (SPARSE_RESOLUTION / 8).pow(3) as usize;
    eprintln!(
        "sparse plume: {} of {total} bricks active",
        bricks.list.len()
    );
    assert!(!bricks.list.is_empty() && bricks.list.len() < total / 2);
    // The list is the active slots in slot order; each names its brick, which names it back.
    assert!(bricks.list.windows(2).all(|pair| pair[0] < pair[1]));
    let g = n / 8;
    for &slot in &bricks.list {
        let packed = bricks.coords[slot as usize] - 1;
        let brick = [packed & 1023, (packed >> 10) & 1023, packed >> 20].map(|c| c as usize);
        assert_eq!(bricks.table[(brick[2] * g + brick[1]) * g + brick[0]], slot);
    }
    assert_eq!(
        bricks.table.iter().filter(|&&slot| slot != 0).count(),
        bricks.list.len()
    );
    let (mass, late) =
        height(&fluid.unbricked(&gpu, RESOURCE_DENSITY, 1, budget, SPARSE_RESOLUTION));
    assert!(mass > 0.0 && late > early + 1.0, "rises: {early} → {late}");

    // The same run reproduces the same bits, allocation included.
    let again = Fluid::new(&gpu, &registry, &sparse_plume(&registry, budget));
    again.run(&gpu, 0..60);
    for resource in [
        RESOURCE_VELOCITY,
        RESOURCE_DENSITY,
        aestra_fluid::RESOURCE_BRICKS,
    ] {
        assert_eq!(
            again.words(&gpu, resource),
            fluid.words(&gpu, resource),
            "{resource}"
        );
    }
}

#[test]
fn a_sparse_checkpoint_replays_bit_for_bit() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &sparse_plume(&registry, 256));
    fluid.run(&gpu, 0..30);
    let checkpoint = fluid.stage.checkpoint(&gpu.device, &gpu.queue).unwrap();
    fluid.run(&gpu, 30..60);
    let uninterrupted = (
        fluid.state(&gpu),
        fluid.words(&gpu, aestra_fluid::RESOURCE_BRICKS),
    );
    fluid.stage.restore(&gpu.queue, &checkpoint).unwrap();
    fluid.run(&gpu, 30..60);
    assert_eq!(
        (
            fluid.state(&gpu),
            fluid.words(&gpu, aestra_fluid::RESOURCE_BRICKS)
        ),
        uninterrupted
    );
}

#[test]
fn a_sparse_grid_stays_within_its_brick_budget() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let fluid = Fluid::new(&gpu, &registry, &sparse_plume(&registry, 6));
    fluid.run(&gpu, 0..30);
    let bricks = fluid.bricks(&gpu, 6, SPARSE_RESOLUTION);
    assert_eq!(bricks.list.len(), 6, "full, and no further");
    let density = fluid.unbricked(&gpu, RESOURCE_DENSITY, 1, 6, SPARSE_RESOLUTION);
    assert!(density.iter().all(|d| d.is_finite()) && density.iter().sum::<f32>() > 0.0);
}

/// The F7 acceptance: a plume in a sparse 512³ grid against the same plume in a dense 128³ one, cells
/// of the same size, timed on the device while the plume grows (the sparse budget, 4096 bricks, holds
/// as many cells as the dense grid). Not a pass/fail test — run with `--ignored --nocapture` on the
/// reference GPU.
#[test]
#[ignore]
fn bench_sparse_grid() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let h = 0.1f32;
    for (label, resolution, sparse_budget) in [
        ("dense 128³", 128u32, None),
        ("sparse 512³", 512, Some(4096u32)),
    ] {
        let mut plume = smoke_effect(&registry);
        // The floor at y = 0; the source low, in the middle.
        let half = resolution as f32 * h * 0.5;
        for (name, value) in [
            ("resolution", Value::U32(resolution)),
            ("cell_size", Value::Scalar(h)),
            ("center", Value::Vec3([0.0, half, 0.0])),
        ] {
            set_input(&mut plume, MODULE_GRID, name, value);
        }
        for (name, value) in [
            ("position", Value::Vec3([0.0, 1.5, 0.0])),
            ("radius", Value::Scalar(1.0)),
            ("velocity", Value::Vec3([0.0, 3.0, 0.0])),
        ] {
            set_input(&mut plume, MODULE_DENSITY_SOURCE, name, value);
        }
        if let Some(budget) = sparse_budget {
            plume = sparse(plume, budget, 1e-3);
        }
        let fluid = Fluid::new(&gpu, &registry, &plume);
        let wait = || {
            gpu.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(120)),
                })
                .unwrap();
        };
        let mut tick = 0;
        for phase in 0..4 {
            fluid.run(&gpu, tick..tick + 90);
            tick += 90;
            wait();
            let ticks = 60;
            let start = std::time::Instant::now();
            fluid.run(&gpu, tick..tick + ticks);
            wait();
            tick += ticks;
            let per_tick = start.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks);
            let bricks = sparse_budget.map_or(String::new(), |budget| {
                let active = fluid.bricks(&gpu, budget, resolution).list.len();
                format!(", {active} bricks")
            });
            let iterations = fluid
                .stage
                .convergent_iterations(&gpu.device, &gpu.queue)
                .unwrap();
            eprintln!(
                "BENCH {label} after {tick} ticks (phase {phase}): {per_tick:.2} ms/tick{bricks}, \
                 pressure iterations {iterations:?}"
            );
        }
    }
}

#[test]
fn particles_follow_a_sparse_field_as_they_follow_the_dense_one() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let all = (RESOLUTION / 8).pow(3);
    let dense = Fluid::new(&gpu, &registry, &jacobi(effect(&registry, false, 24)));
    let whole = Fluid::new(
        &gpu,
        &registry,
        &sparse(jacobi(effect(&registry, false, 24)), all, -1.0),
    );
    dense.run(&gpu, 0..40);
    whole.run(&gpu, 0..40);
    assert!(
        whole
            .stage
            .block()
            .field(&aestra_core::ResourceTypeId::new(RESOURCE_VELOCITY))
            .unwrap()
            .bricks
            .is_some()
    );
    // The same field, read through the brick table: the same bits.
    assert_eq!(
        bits(&follow_plume(&gpu, &whole)),
        bits(&follow_plume(&gpu, &dense))
    );
}

/// Samples field slot 0 of the volume interface at a lattice of points off the cell centres, through
/// a compute shader composed like the march: `params` the interface's uniform words, `field` bound to
/// every field slot, `table` the brick table (a one-texel placeholder for a dense field).
fn probe_volume(
    gpu: &Gpu,
    params: &[u32],
    field: &wgpu::TextureView,
    table: &wgpu::TextureView,
) -> Vec<f32> {
    use wgpu::util::DeviceExt;
    const LATTICE: u32 = 11;
    let source = format!(
        "{}\n@group(0) @binding(7) var<storage, read_write> probes: array<vec4<f32>>;\n\
         @compute @workgroup_size(64)\nfn probe(@builtin(global_invocation_id) id: vec3<u32>) {{\n    \
         let i = id.x;\n    if (i >= arrayLength(&probes)) {{ return; }}\n    \
         let n = {LATTICE}u;\n    \
         let at = vec3<f32>(f32(i % n), f32((i / n) % n), f32(i / (n * n))) + vec3<f32>(0.37, 0.61, 0.23);\n    \
         probes[i] = aestra_volume_field(0u, at / f32(n));\n}}",
        aestra_gpu::volume::volume_interface_wgsl("0"),
    );
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("volume probe"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("volume probe"),
            layout: None,
            module: &module,
            entry_point: Some("probe"),
            compilation_options: Default::default(),
            cache: None,
        });
    let uniform = gpu
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("volume params"),
            contents: &params
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<u8>>(),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let count = LATTICE.pow(3) as u64;
    let probes = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probes"),
        size: count * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
    let view = || wgpu::BindingResource::TextureView(field);
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("volume probe"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            entry(0, uniform.as_entire_binding()),
            entry(1, view()),
            entry(2, wgpu::BindingResource::Sampler(&sampler)),
            entry(3, view()),
            entry(4, view()),
            entry(5, view()),
            entry(6, wgpu::BindingResource::TextureView(table)),
            entry(7, probes.as_entire_binding()),
        ],
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((count as u32).div_ceil(64), 1, 1);
    }
    gpu.queue.submit([encoder.finish()]);
    read_floats(gpu, &probes)
        .chunks(4)
        .map(|value| value[0])
        .collect()
}

/// A 3-D texture a field or a table is copied into and the probe samples.
fn volume_texture(gpu: &Gpu, edge: u32, format: wgpu::TextureFormat) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("volume"),
            size: wgpu::Extent3d {
                width: edge,
                height: edge,
                depth_or_array_layers: edge,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

#[test]
fn a_sparse_field_is_drawn_from_its_brick_atlas_as_the_dense_field_is() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let all = (RESOLUTION / 8).pow(3);
    let dense = Fluid::new(&gpu, &registry, &jacobi(effect(&registry, false, 24)));
    let whole = Fluid::new(
        &gpu,
        &registry,
        &sparse(jacobi(effect(&registry, false, 24)), all, -1.0),
    );
    dense.run(&gpu, 0..40);
    whole.run(&gpu, 0..40);
    let density = aestra_core::ResourceTypeId::new(RESOURCE_DENSITY);
    let dense_layout = dense.stage.block().field(&density).unwrap().clone();
    let sparse_layout = whole.stage.block().field(&density).unwrap().clone();
    let bricks = sparse_layout.bricks.clone().expect("bricked");
    let copies = FieldVolumePipeline::new(&gpu.device);

    let dense_texture = volume_texture(&gpu, RESOLUTION, wgpu::TextureFormat::Rgba16Float);
    let placeholder = volume_texture(&gpu, 1, wgpu::TextureFormat::R32Uint);
    let atlas_texels = aestra_gpu::volume::brick_atlas_texels(bricks.slots, bricks.edge);
    let atlas = volume_texture(&gpu, atlas_texels, wgpu::TextureFormat::Rgba16Float);
    let table = volume_texture(&gpu, RESOLUTION / 8, wgpu::TextureFormat::R32Uint);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    copies.encode(
        &gpu.device,
        &mut encoder,
        dense.stage.field_buffers(&dense_layout).unwrap(),
        &dense_texture,
        &dense_layout,
        None,
    );
    copies.encode(
        &gpu.device,
        &mut encoder,
        whole.stage.field_buffers(&sparse_layout).unwrap(),
        &atlas,
        &sparse_layout,
        Some(&table),
    );
    gpu.queue.submit([encoder.finish()]);

    // The interface's uniform: size and cell, dims and field count, bricks, 64 constant words.
    let params = |bricks: [u32; 2]| {
        let extent = (RESOLUTION as f32 * CELL_SIZE).to_bits();
        let mut words = vec![extent, extent, extent, CELL_SIZE.to_bits()];
        words.extend([
            RESOLUTION, RESOLUTION, RESOLUTION, 1, bricks[0], bricks[1], 0, 0,
        ]);
        words.extend([0; aestra_runtime::MAX_VOLUME_CONSTANTS]);
        words
    };
    let expected = probe_volume(&gpu, &params([0, 0]), &dense_texture, &placeholder);
    let per_axis = aestra_gpu::volume::brick_atlas_bricks(bricks.slots);
    let got = probe_volume(&gpu, &params([bricks.edge, per_axis]), &atlas, &table);
    let peak = expected.iter().fold(0.0f32, |p, v| p.max(v.abs()));
    assert!(peak > 0.05, "the probes cross the plume (peak {peak})");
    for (index, (a, b)) in expected.iter().zip(&got).enumerate() {
        assert!(
            (a - b).abs() <= 1e-2 * peak,
            "probe {index}: dense {a}, bricks {b}"
        );
    }
}
