//! Fluid F7a (G7) on the real GPU: a grid-wide prefix scan compacts a sparse set into a list in index
//! order, and a pass sized on the device from that list's length runs exactly that many workgroups.
//!
//! Runs only where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_bevy_render::execution::StageExecutor;
use aestra_compiler::{ComputeProgram, ExtensionRegistry};
use aestra_core::{ComputeProgramId, ResourceTypeId};
use aestra_runtime::{
    ComputeOp, ExecutionBlock, ExecutionOp, FrameConstants, IndirectDispatch, ResourceAccess,
    ResourceDescriptor, ResourceLifetime, StagedDispatch,
};

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const PROGRAM: &str = "org.example.test::program/scan";
const FLAGS: &str = "org.example.test::resource/flags";
const PREFIX: &str = "org.example.test::resource/prefix";
const BLOCKS: &str = "org.example.test::resource/blocks";
const COUNTS: &str = "org.example.test::resource/counts";
const LIST: &str = "org.example.test::resource/list";
const MARKS: &str = "org.example.test::resource/marks";
const VISITS: &str = "org.example.test::resource/visits";

/// Blocks of 64 flags.
const BLOCK_COUNT: u32 = 1000;
const LEN: u32 = BLOCK_COUNT * 64;
const EMPTY: u32 = 0xffff_ffff;

/// `counts` holds the workgroups to run (x, y, z) then the set's size; `list` the set's indices in
/// order, then `EMPTY`; `mark` numbers each listed index by its place, from 1, and counts its
/// workgroups.
const WGSL: &str = r#"
@group(0) @binding(0) var<storage, read_write> flags: array<u32>;
@group(0) @binding(1) var<storage, read_write> prefix: array<u32>;
@group(0) @binding(2) var<storage, read_write> blocks: array<u32>;
@group(0) @binding(3) var<storage, read_write> counts: array<u32>;
@group(0) @binding(4) var<storage, read_write> list: array<u32>;
@group(0) @binding(5) var<storage, read_write> marks: array<u32>;
@group(0) @binding(6) var<storage, read_write> visits: array<atomic<u32>>;

@compute @workgroup_size(64)
fn scan_blocks(@builtin(local_invocation_index) local: u32, @builtin(workgroup_id) group: vec3<u32>) {
    let i = group.x * 64u + local;
    let scanned = aestra_workgroup_scan(local, flags[i]);
    prefix[i] = scanned.x;
    if (local == 0u) {
        blocks[group.x] = scanned.y;
    }
}

@compute @workgroup_size(64)
fn scan_totals(@builtin(local_invocation_index) local: u32) {
    let n = arrayLength(&blocks);
    var carry = 0u;
    for (var base = 0u; base < n; base = base + 64u) {
        let i = base + local;
        var total = 0u;
        if (i < n) {
            total = blocks[i];
        }
        let scanned = aestra_workgroup_scan(local, total);
        if (i < n) {
            blocks[i] = carry + scanned.x;
        }
        carry = carry + scanned.y;
    }
    if (local == 0u) {
        counts[0] = (carry + 63u) / 64u;
        counts[1] = 1u;
        counts[2] = 1u;
        counts[3] = carry;
    }
}

@compute @workgroup_size(64)
fn clear(@builtin(global_invocation_id) id: vec3<u32>) {
    list[id.x] = 0xffffffffu;
}

@compute @workgroup_size(64)
fn scatter(@builtin(global_invocation_id) id: vec3<u32>, @builtin(workgroup_id) group: vec3<u32>) {
    let i = id.x;
    if (flags[i] != 0u) {
        list[blocks[group.x] + prefix[i]] = i;
    }
}

@compute @workgroup_size(64)
fn mark(@builtin(global_invocation_id) id: vec3<u32>, @builtin(local_invocation_index) local: u32) {
    if (local == 0u) {
        atomicAdd(&visits[0], 1u);
    }
    let index = list[id.x];
    if (index != 0xffffffffu) {
        marks[index] = id.x + 1u;
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
        eprintln!("skipping indirect-scan conformance: no compute adapter");
        return None;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra indirect scan"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("the adapter provides a device");
    Some(Gpu { device, queue })
}

fn pass(entry: &str, accesses: Vec<ResourceAccess>, x: u32) -> ExecutionOp {
    ExecutionOp::Compute(ComputeOp {
        name: entry.into(),
        program: Some(ComputeProgramId::new(PROGRAM)),
        entry_point: entry.into(),
        accesses,
        dispatch: StagedDispatch { x, y: 1, z: 1 },
        indirect: None,
    })
}

fn resource(id: &str, words: u32, lifetime: ResourceLifetime) -> ResourceDescriptor {
    ResourceDescriptor {
        id: ResourceTypeId::new(id),
        bytes: u64::from(words) * 4,
        lifetime,
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect()
}

#[test]
fn a_scan_compacts_a_set_and_sizes_the_pass_over_it_on_the_device() {
    let Some(gpu) = gpu() else { return };
    let mut registry = ExtensionRegistry::builtin();
    registry
        .register_program(ComputeProgram {
            id: ComputeProgramId::new(PROGRAM),
            wgsl: format!("{WGSL}\n{}", aestra_gpu::scan::SCAN_WGSL),
            entry_points: ["scan_blocks", "scan_totals", "clear", "scatter", "mark"]
                .map(String::from)
                .to_vec(),
        })
        .unwrap();
    use ResourceLifetime::{Persistent, Transient};
    let mut mark = pass(
        "mark",
        vec![
            ResourceAccess::read(LIST),
            ResourceAccess::read_write(MARKS),
            ResourceAccess::read_write(VISITS),
        ],
        BLOCK_COUNT,
    );
    let ExecutionOp::Compute(compute) = &mut mark else {
        unreachable!()
    };
    compute.indirect = Some(IndirectDispatch {
        resource: ResourceTypeId::new(COUNTS),
        word: 0,
    });
    let block = ExecutionBlock {
        resources: vec![
            resource(FLAGS, LEN, Persistent),
            resource(PREFIX, LEN, Transient),
            resource(BLOCKS, BLOCK_COUNT, Transient),
            resource(COUNTS, 4, Transient),
            resource(LIST, LEN, Transient),
            resource(MARKS, LEN, Transient),
            resource(VISITS, 1, Transient),
        ],
        ops: vec![
            pass(
                "scan_blocks",
                vec![
                    ResourceAccess::read_write(FLAGS),
                    ResourceAccess::read_write(PREFIX),
                    ResourceAccess::read_write(BLOCKS),
                ],
                BLOCK_COUNT,
            ),
            pass(
                "scan_totals",
                vec![
                    ResourceAccess::read_write(BLOCKS),
                    ResourceAccess::read_write(COUNTS),
                ],
                1,
            ),
            pass("clear", vec![ResourceAccess::read_write(LIST)], BLOCK_COUNT),
            pass(
                "scatter",
                vec![
                    ResourceAccess::read_write(FLAGS),
                    ResourceAccess::read_write(PREFIX),
                    ResourceAccess::read_write(BLOCKS),
                    ResourceAccess::read_write(LIST),
                ],
                BLOCK_COUNT,
            ),
            mark,
        ],
        constants: Vec::new(),
        fields: Vec::new(),
    };
    let stage = StageExecutor::new(&gpu.device, &gpu.queue, &block, &registry.programs, 4)
        .expect("the block runs");

    // Sparse, clustered and empty stretches: a hash thinned to about 3 in 10, with holes.
    let flags: Vec<u32> = (0..LEN)
        .map(|i| {
            let h = i.wrapping_mul(0x9e37_79b9).rotate_left(13) ^ (i >> 7);
            u32::from(h % 10 < 3 && (i / 4096) % 5 != 2)
        })
        .collect();
    let bytes: Vec<u8> = flags.iter().flat_map(|flag| flag.to_le_bytes()).collect();
    gpu.queue
        .write_buffer(stage.buffer(FLAGS).unwrap(), 0, &bytes);
    stage
        .run_tick(
            &gpu.device,
            &gpu.queue,
            FrameConstants::fixed_step(0, 1.0 / 60.0, 0),
            None,
        )
        .unwrap();
    let read = |id| words(&stage.read_resource(&gpu.device, &gpu.queue, id).unwrap());

    let listed: Vec<u32> = (0..LEN).filter(|&i| flags[i as usize] != 0).collect();
    let total = listed.len() as u32;
    assert_eq!(read(COUNTS), [total.div_ceil(64), 1, 1, total]);
    let list = read(LIST);
    assert_eq!(&list[..listed.len()], listed, "the set, in index order");
    assert!(list[listed.len()..].iter().all(|&entry| entry == EMPTY));
    let marks = read(MARKS);
    for (place, &index) in listed.iter().enumerate() {
        assert_eq!(marks[index as usize], place as u32 + 1);
    }
    assert_eq!(
        marks.iter().filter(|&&mark| mark != 0).count(),
        listed.len()
    );
    assert_eq!(
        read(VISITS),
        [total.div_ceil(64)],
        "the device-sized pass ran only the workgroups the set needs, not its bound"
    );
}
