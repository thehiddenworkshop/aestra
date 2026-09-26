//! Fluid F5 (G7) on the real GPU: a convergent repeat stops on the device once its residual is small
//! enough, with no readback, and the shared reduction module sums in a fixed order.
//!
//! Runs only where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_bevy_render::execution::StageExecutor;
use aestra_compiler::{ComputeProgram, ExtensionRegistry};
use aestra_core::{ComputeProgramId, ResourceTypeId};
use aestra_runtime::{
    ComputeOp, ExecutionBlock, ExecutionOp, FrameConstants, RepeatPolicy, ResourceAccess,
    ResourceDescriptor, ResourceLifetime, StagedDispatch,
};

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const PROGRAM: &str = "org.example.test::program/convergence";
const VALUE: &str = "org.example.test::resource/value";
const RESIDUAL: &str = "org.example.test::resource/residual";
const VALUES: &str = "org.example.test::resource/values";
const PARTIALS: &str = "org.example.test::resource/partials";

/// `value` holds [the halved value, how many times `halve` ran]; `residual` the value after each run.
const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> value: array<f32>;
@group(0) @binding(1) var<storage, read_write> residual: array<f32>;
@group(0) @binding(2) var<storage, read_write> values: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> partials: array<vec4<f32>>;

@compute @workgroup_size(1)
fn start() {
    value[0] = 1.0;
    value[1] = 0.0;
}

@compute @workgroup_size(1)
fn halve() {
    value[0] = value[0] * 0.5;
    value[1] = value[1] + 1.0;
    residual[0] = value[0];
}

@compute @workgroup_size(4, 4, 4)
fn reduce_groups(
    @builtin(local_invocation_index) index: u32,
    @builtin(workgroup_id) group: vec3<u32>,
) {
    let total = aestra_workgroup_sum(index, values[group.x * 64u + index]);
    if (index == 0u) {
        partials[group.x] = total;
    }
}

@compute @workgroup_size(64)
fn reduce_partials(@builtin(local_invocation_index) index: u32) {
    var own = vec4<f32>(0.0);
    for (var i = index; i < arrayLength(&partials) - 1u; i = i + 64u) {
        own = own + partials[i];
    }
    let total = aestra_workgroup_sum(index, own);
    if (index == 0u) {
        partials[arrayLength(&partials) - 1u] = total;
    }
}
"#;

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
    .ok()
    .filter(|adapter| {
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
        eprintln!("skipping convergent-repeat conformance: no compute adapter");
        return None;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra convergent repeat"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("the adapter provides a device");
    Some(Gpu { device, queue })
}

fn registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::builtin();
    registry
        .register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM),
            wgsl: format!("{WGSL}\n{}", aestra_gpu::reduce::REDUCE_WGSL),
            entry_points: ["start", "halve", "reduce_groups", "reduce_partials"]
                .map(String::from)
                .to_vec(),
        })
        .unwrap();
    registry
}

fn pass(entry: &str, accesses: Vec<ResourceAccess>, x: u32) -> ExecutionOp {
    ExecutionOp::Compute(ComputeOp {
        name: entry.into(),
        program: Some(ComputeProgramId::new(PROGRAM)),
        entry_point: entry.into(),
        accesses,
        dispatch: StagedDispatch { x, y: 1, z: 1 },
    })
}

fn resource(id: &str, bytes: u64, lifetime: ResourceLifetime) -> ResourceDescriptor {
    ResourceDescriptor {
        id: ResourceTypeId::new(id),
        bytes,
        lifetime,
    }
}

/// Starts at 1 and halves until the value is at most `tolerance`, at most `max` times.
fn halving(tolerance: f32, max: u32) -> ExecutionBlock {
    ExecutionBlock {
        resources: vec![
            resource(VALUE, 8, ResourceLifetime::Persistent),
            resource(RESIDUAL, 4, ResourceLifetime::Transient),
        ],
        ops: vec![
            pass("start", vec![ResourceAccess::read_write(VALUE)], 1),
            ExecutionOp::Repeat {
                policy: RepeatPolicy::UntilConverged {
                    residual: ResourceTypeId::new(RESIDUAL),
                    tolerance,
                    max,
                },
                body: vec![pass(
                    "halve",
                    vec![
                        ResourceAccess::read_write(VALUE),
                        ResourceAccess::write(RESIDUAL),
                    ],
                    1,
                )],
            },
        ],
        constants: Vec::new(),
        fields: Vec::new(),
    }
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| f32::from_le_bytes(*word))
        .collect()
}

#[test]
fn a_convergent_repeat_stops_on_the_device_once_converged() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let run = |block: &ExecutionBlock| {
        let stage = StageExecutor::new(&gpu.device, &gpu.queue, block, &registry.programs, 4)
            .expect("the block runs");
        stage
            .run_tick(
                &gpu.device,
                &gpu.queue,
                FrameConstants::fixed_step(0, 1.0 / 60.0, 0),
                None,
            )
            .unwrap();
        let value = floats(&stage.read_resource(&gpu.device, &gpu.queue, VALUE).unwrap());
        let iterations = stage
            .convergent_iterations(&gpu.device, &gpu.queue)
            .unwrap();
        (value, iterations)
    };

    // 1 → ½ → ¼ → ⅛ → 1/16 ≤ 0.1: four halvings, and the other six are empty dispatches.
    let (value, iterations) = run(&halving(0.1, 10));
    assert_eq!(value, [0.0625, 4.0]);
    assert_eq!(iterations, [4]);

    // Never small enough: capped at `max`.
    let (value, iterations) = run(&halving(0.0, 10));
    assert_eq!(value, [0.5f32.powi(10), 10.0]);
    assert_eq!(iterations, [10]);

    // Converged by the first test: one iteration runs.
    let (value, iterations) = run(&halving(0.5, 10));
    assert_eq!(value, [0.5, 1.0]);
    assert_eq!(iterations, [1]);
}

/// The same fixed tree on the CPU: a workgroup halves its active range, adding the upper half on.
fn tree_sum(values: &[[f32; 4]]) -> [f32; 4] {
    let mut scratch = values.to_vec();
    let mut stride = scratch.len() / 2;
    while stride > 0 {
        for index in 0..stride {
            let upper = scratch[index + stride];
            for (lane, value) in scratch[index].iter_mut().zip(upper) {
                *lane += value;
            }
        }
        stride /= 2;
    }
    scratch[0]
}

#[test]
fn the_shared_reduction_sums_in_a_fixed_order() {
    let Some(gpu) = gpu() else { return };
    let registry = registry();
    let groups = 300u32;
    let values: Vec<[f32; 4]> = (0..groups * 64)
        .map(|i| {
            let x = (i as f32 * 0.618_034).fract();
            [x, x * x, 1.0 / (1.0 + i as f32), (i % 7) as f32 - 3.0]
        })
        .collect();
    let block = ExecutionBlock {
        resources: vec![
            resource(VALUE, 8, ResourceLifetime::Persistent),
            resource(RESIDUAL, 4, ResourceLifetime::Persistent),
            resource(
                VALUES,
                u64::from(groups) * 64 * 16,
                ResourceLifetime::Persistent,
            ),
            resource(
                PARTIALS,
                (u64::from(groups) + 1) * 16,
                ResourceLifetime::Persistent,
            ),
        ],
        ops: vec![
            pass(
                "reduce_groups",
                vec![
                    ResourceAccess::read_write(VALUES),
                    ResourceAccess::read_write(PARTIALS),
                ],
                groups,
            ),
            pass(
                "reduce_partials",
                vec![ResourceAccess::read_write(PARTIALS)],
                1,
            ),
        ],
        constants: Vec::new(),
        fields: Vec::new(),
    };
    let stage = StageExecutor::new(&gpu.device, &gpu.queue, &block, &registry.programs, 4).unwrap();
    let bytes: Vec<u8> = values
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    gpu.queue
        .write_buffer(stage.buffer(VALUES).unwrap(), 0, &bytes);
    stage
        .run_tick(
            &gpu.device,
            &gpu.queue,
            FrameConstants::fixed_step(0, 1.0 / 60.0, 0),
            None,
        )
        .unwrap();
    let partials = floats(
        &stage
            .read_resource(&gpu.device, &gpu.queue, PARTIALS)
            .unwrap(),
    );
    let total = &partials[partials.len() - 4..];

    // Per workgroup, the tree; over the partials, each invocation's strided run, then the tree.
    let group_sums: Vec<[f32; 4]> = values.chunks(64).map(tree_sum).collect();
    let mut runs = vec![[0.0f32; 4]; 64];
    for (index, sum) in group_sums.iter().enumerate() {
        for (lane, value) in runs[index % 64].iter_mut().zip(sum) {
            *lane += value;
        }
    }
    let expected = tree_sum(&runs);
    assert_eq!(
        total.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        expected.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        "bit for bit the fixed tree: {total:?} vs {expected:?}"
    );
}
