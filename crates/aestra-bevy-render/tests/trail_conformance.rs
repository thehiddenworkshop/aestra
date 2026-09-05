//! Persistent history exercised on a native adapter using the production buffer ABI.
use aestra_gpu::{
    GpuEmitter, GpuGlobals, GpuParticle,
    shader::{SIMULATION_WESL, compile_wesl},
};
use bevy::math::{Mat4, UVec2, Vec3, Vec4};
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use wgpu::util::DeviceExt;

fn encode<T: ShaderType + WriteInto>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    StorageBuffer::new(&mut bytes).write(value).unwrap();
    bytes
}

fn word(bytes: &[u8], record: usize, offset: usize) -> u32 {
    let at = record * 64 + offset;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

#[test]
fn trails_preserve_identity_world_history_and_retired_tails_and_reset_on_discontinuities() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        assert!(
            std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
            "native GPU required"
        );
        eprintln!("Skipping trail conformance: no adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let shader = compile_wesl(
        "package::trail_test",
        SIMULATION_WESL,
        &["link_ribbons", "update_trails"],
    )
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.wgsl.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &(0..7)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: matches!(binding, 0 | 6),
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>(),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipelines = ["link_ribbons", "update_trails"].map(|entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    });
    let data = [
        encode(&vec![GpuEmitter {
            max_particles: 2,
            _turbulence_padding: 1,
            trail_offset: 2,
            trail_points: 4,
            trail_interval: 0.125,
            trail_lifetime: 1.0,
            ..Default::default()
        }]),
        encode(&vec![GpuParticle::default(); 11]),
        encode(&vec![0u32, 1]),
        encode(&vec![0u32; 2]),
        encode(&vec![0u32; 2]),
        encode(&vec![6u32, 2, 0, 0]),
        encode(&GpuGlobals::default()),
    ];
    let buffers = data
        .iter()
        .map(|bytes| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
        })
        .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &buffers
            .iter()
            .enumerate()
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    });
    let step = |time: f32, ids: [u32; 2], count: u32, translation: f32, epoch: u32, seed: u32| {
        let parents = ids
            .map(|particle_index| GpuParticle {
                particle_index,
                position: Vec3::new(particle_index as f32 + time, 0.0, 0.0),
                size: 2.0,
                color: Vec4::ONE,
                alive: 1,
                ..Default::default()
            })
            .to_vec();
        // Upload only the simulation prefix. The history tail is never reinitialized.
        queue.write_buffer(&buffers[1], 0, &encode(&parents));
        queue.write_buffer(&buffers[2], 0, &encode(&vec![0u32, 1]));
        queue.write_buffer(&buffers[5], 0, &encode(&vec![6u32, count, 0, 0]));
        queue.write_buffer(
            &buffers[6],
            0,
            &encode(&GpuGlobals {
                time,
                total_slots: 2,
                emitter_count: 1,
                seed,
                duration: 0.25,
                continuous: 1,
                _padding: UVec2::new(epoch, 0),
                world_from_effect: Mat4::from_translation(Vec3::new(translation, 0.0, 0.0)),
            }),
        );
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buffers[1].size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_bind_group(0, &group, &[]);
            for pipeline in &pipelines {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&buffers[1], 0, &readback, 0, readback.size());
        let submission = queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let bytes = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        bytes
    };
    let initial = step(0.0, [0, 1], 2, 100.0, 0, 0);
    assert_eq!(word(&initial, 3, 56), 1);
    assert_eq!(f32::from_bits(word(&initial, 4, 16)), 100.0);
    step(0.125, [0, 1], 2, 200.0, 0, 0);
    let moved = step(0.25, [1, 0], 2, 300.0, 0, 0);
    assert_eq!(
        word(&moved, 3, 48),
        0,
        "owner survives physical slot permutation"
    );
    assert_eq!(word(&moved, 3, 56), 3);
    assert_eq!(f32::from_bits(word(&moved, 3, 16)), 300.25);
    assert_eq!(
        f32::from_bits(word(&moved, 4, 16)),
        100.0,
        "past world position does not follow current transform"
    );
    let paused = step(0.25, [1, 0], 2, 300.0, 0, 0);
    assert_eq!(&moved[128..], &paused[128..], "pause freezes history");
    let looped = step(0.375, [2, 1], 2, 400.0, 0, 0);
    assert_eq!(word(&looped, 7, 48), 1);
    assert_eq!(
        word(&looped, 7, 56),
        3,
        "survivor keeps ring through loop boundary"
    );
    assert_eq!(word(&looped, 3, 48), 2);
    assert_eq!(
        word(&looped, 3, 56),
        1,
        "new parent cannot inherit evicted trail"
    );
    let retired = step(0.5, [0, 0], 0, 0.0, 0, 0);
    assert_eq!(word(&retired, 7, 44), 1, "tail remains after parent dies");
    let seek = step(0.625, [2, 1], 2, 0.0, 1, 0);
    assert_eq!(word(&seek, 3, 56), 1, "forward seek epoch resets history");
    let seed = step(0.75, [2, 1], 2, 0.0, 1, 1);
    assert_eq!(word(&seed, 3, 56), 1, "seed change resets history");
    let backward = step(0.5, [2, 1], 2, 0.0, 1, 1);
    assert_eq!(word(&backward, 3, 56), 1);
    let expired = step(1.5, [0, 0], 0, 0.0, 1, 1);
    assert_eq!(word(&expired, 3, 44), 0);
    assert_eq!(word(&expired, 7, 44), 0);
}
