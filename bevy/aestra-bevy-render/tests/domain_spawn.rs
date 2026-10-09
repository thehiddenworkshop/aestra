//! Fluid F10 (G8) on the real GPU: Spawn From Domain turns a domain's emission list into particles
//! with no readback — record `i` becomes ordinal `first + i`, at the record's position, in a slot
//! popped from the top of the free list — bounded by the list's capacity and the free slots, and a
//! rerun reproduces every bit.
//!
//! Runs only where a compute adapter exists; set `AESTRA_REQUIRE_GPU_CONFORMANCE=1` to require one.

use aestra_bevy_render::execution::{DomainSpawnPipeline, SpawnAcceptanceCounter, SpawnState};
use aestra_core::ResourceTypeId;
use aestra_runtime::{CompiledDomainSpawn, EmissionLayout};
use wgpu::util::DeviceExt;

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
const CAPACITY: u32 = 128;
const STRIDE: usize = aestra_gpu::STATEFUL_STATE_STRIDE as usize;

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
        eprintln!("skipping domain-spawn conformance: no compute adapter");
        return None;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra domain spawn"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("the adapter provides a device");
    Some(Gpu { device, queue })
}

fn storage(gpu: &Gpu, words: &[u32]) -> wgpu::Buffer {
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &words
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        })
}

fn read(gpu: &Gpu, buffer: &wgpu::Buffer) -> Vec<u32> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    gpu.queue.submit([encoder.finish()]);
    staging.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("the staging buffer maps");
    });
    gpu.device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .unwrap();
    let words = staging
        .slice(..)
        .get_mapped_range()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|chunk| u32::from_le_bytes(*chunk))
        .collect();
    staging.unmap();
    words
}

/// An emitter's persistent buffers after `spawned` ordinals, with `free` free slots.
struct Emitter {
    state: wgpu::Buffer,
    free_list: wgpu::Buffer,
    free_count: wgpu::Buffer,
    spawn_counter: wgpu::Buffer,
    params: wgpu::Buffer,
}

fn emitter(gpu: &Gpu, free: u32, spawned: u32) -> Emitter {
    emitter_capacity(gpu, CAPACITY, free, spawned)
}

fn emitter_capacity(gpu: &Gpu, capacity: u32, free: u32, spawned: u32) -> Emitter {
    let mut params = vec![0u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
    params[0] = capacity;
    params[2] = 7; // seed
    params[4] = 2.0f32.to_bits(); // speed range
    params[5] = 2.0f32.to_bits();
    params[6] = 1.0f32.to_bits(); // lifetime range
    params[7] = 3.0f32.to_bits();
    params[13] = 1.0f32.to_bits(); // direction +y, no spread
    Emitter {
        state: storage(gpu, &vec![0; capacity as usize * STRIDE]),
        free_list: storage(gpu, &(0..capacity).rev().collect::<Vec<_>>()),
        free_count: storage(gpu, &[free]),
        spawn_counter: storage(gpu, &[spawned]),
        params: storage(gpu, &params),
    }
}

#[path = "domain_spawn/birth_outputs.rs"]
mod birth_outputs;

/// A list of `count` records: record `i` at `(i, 2i, -i)` moving `(0, 0, 10)`.
fn emission(gpu: &Gpu, count: u32, capacity: u32) -> wgpu::Buffer {
    let mut words = vec![0u32; 4 + 8 * capacity as usize];
    words[0] = count;
    for i in 0..count.min(capacity) as usize {
        let record = &mut words[4 + 8 * i..4 + 8 * i + 8];
        record[0] = (i as f32).to_bits();
        record[1] = (2.0 * i as f32).to_bits();
        record[2] = (-(i as f32)).to_bits();
        record[6] = 10.0f32.to_bits();
    }
    storage(gpu, &words)
}

fn spawn(
    gpu: &Gpu,
    pipeline: &DomainSpawnPipeline,
    target: &Emitter,
    list: &wgpu::Buffer,
    capacity: u32,
) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    pipeline.encode(
        &gpu.device,
        &mut encoder,
        SpawnState {
            state: &target.state,
            free_list: &target.free_list,
            free_count: &target.free_count,
            spawn_counter: &target.spawn_counter,
            params: &target.params,
            output_births: None,
        },
        list,
        &CompiledDomainSpawn {
            stage: 0,
            emission: EmissionLayout {
                resource: ResourceTypeId::new("org.example.test::resource/emission"),
                capacity,
            },
            inherit: 0.5,
        },
    );
    gpu.queue.submit([encoder.finish()]);
}

#[test]
fn records_become_particles_in_order_bounded_and_reproducibly() {
    let Some(gpu) = gpu() else { return };
    let pipeline = DomainSpawnPipeline::new(&gpu.device);

    // 40 records, room for all: 40 particles, ordinals 5..45, record i in slot free_list[top - 1 - i].
    let target = emitter(&gpu, CAPACITY, 5);
    let list = emission(&gpu, 40, 64);
    spawn(&gpu, &pipeline, &target, &list, 64);
    assert_eq!(read(&gpu, &target.free_count), [CAPACITY - 40]);
    assert_eq!(read(&gpu, &target.spawn_counter), [45]);
    let state = read(&gpu, &target.state);
    let free_list: Vec<u32> = (0..CAPACITY).rev().collect();
    for i in 0..40usize {
        let slot = free_list[CAPACITY as usize - 1 - i] as usize;
        let particle: Vec<f32> = state[slot * STRIDE..slot * STRIDE + STRIDE]
            .iter()
            .map(|word| f32::from_bits(*word))
            .collect();
        assert_eq!(
            particle[..3],
            [i as f32, 2.0 * i as f32, -(i as f32)],
            "{i}"
        );
        // Half the record's velocity, plus the emitter's launch: 2 along +y.
        assert_eq!(particle[3..6], [0.0, 2.0, 5.0], "{i}");
        assert_eq!(particle[6], 0.0);
        assert!((1.0..=3.0).contains(&particle[7]), "{}", particle[7]);
        assert_eq!(state[slot * STRIDE + 8], 5 + i as u32);
    }
    let again = emitter(&gpu, CAPACITY, 5);
    spawn(&gpu, &pipeline, &again, &list, 64);
    assert_eq!(read(&gpu, &again.state), state);

    // More records than the list holds: its capacity bounds them.
    let target = emitter(&gpu, CAPACITY, 0);
    spawn(&gpu, &pipeline, &target, &emission(&gpu, 1000, 16), 16);
    assert_eq!(read(&gpu, &target.spawn_counter), [16]);

    // More records than free slots: the free slots bound them.
    let target = emitter(&gpu, 10, 0);
    spawn(&gpu, &pipeline, &target, &list, 64);
    assert_eq!(read(&gpu, &target.free_count), [0]);
    assert_eq!(read(&gpu, &target.spawn_counter), [10]);

    // An empty list spawns nothing.
    let target = emitter(&gpu, CAPACITY, 3);
    spawn(&gpu, &pipeline, &target, &emission(&gpu, 0, 8), 8);
    assert_eq!(read(&gpu, &target.free_count), [CAPACITY]);
    assert_eq!(read(&gpu, &target.spawn_counter), [3]);
    assert!(read(&gpu, &target.state).iter().all(|word| *word == 0));
}

#[test]
fn destination_acceptance_counter_reports_only_allocated_slots() {
    let Some(gpu) = gpu() else { return };
    let pipeline = DomainSpawnPipeline::new(&gpu.device);
    let target = emitter(&gpu, 10, 0);
    let list = emission(&gpu, 40, 64);
    let counters = storage(&gpu, &[0, 0, 0]);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    pipeline.encode_with_acceptance(
        &gpu.device,
        &mut encoder,
        SpawnState {
            state: &target.state,
            free_list: &target.free_list,
            free_count: &target.free_count,
            spawn_counter: &target.spawn_counter,
            params: &target.params,
            output_births: None,
        },
        &list,
        &CompiledDomainSpawn {
            stage: 0,
            emission: EmissionLayout {
                resource: ResourceTypeId::new("org.example.test::resource/emission"),
                capacity: 64,
            },
            inherit: 0.0,
        },
        SpawnAcceptanceCounter {
            buffer: &counters,
            word: 1,
        },
    );
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(read(&gpu, &counters), [0, 10, 0]);
    assert_eq!(read(&gpu, &target.spawn_counter), [10]);
}
