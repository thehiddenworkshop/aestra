// Native compute contract; readback exists only in this ignored test, never playback.
use wgpu::util::DeviceExt;

#[test]
#[ignore = "native GPU alpha-sort permutation contract; run alone"]
fn native_alpha_sort_handles_large_sparse_pools_and_opposite_views() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..default()
    }))
    .expect("native adapter required");
    assert_ne!(adapter.get_info().device_type, wgpu::DeviceType::Cpu);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..default()
    }))
    .unwrap();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &(0..8)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 4 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding < 6,
                        }
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
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(include_str!("alpha_sort.wgsl").into()),
    });
    let pipelines = ["classify", "merge", "finish"].map(|name| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(name),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(name),
            compilation_options: default(),
            cache: None,
        })
    });
    let encoded = |value: &Vec<GpuParticle>| {
        let mut b = StorageBuffer::new(Vec::new());
        b.write(value).unwrap();
        b.into_inner()
    };
    let upload = |bytes: &[u8]| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage: wgpu::BufferUsages::STORAGE,
        })
    };
    let mut cases = 0;
    for capacity in [1u32, 255, 256, 257, 4097, 8193, 65537] {
        for live in [0, capacity / 2, capacity] {
            for reverse in [false, true] {
                let (count, widths) = stages(capacity).unwrap();
                let offset = 17u32;
                let particles = (0..offset + capacity)
                    .map(|slot| GpuParticle {
                        position: Vec3::new(0.0, 0.0, ((slot * 17) % 23) as f32 - 12.0),
                        particle_index: slot - slot.min(offset),
                        ..default()
                    })
                    .collect::<Vec<_>>();
                let particles_buffer = upload(&encoded(&particles));
                let mut alive = vec![0u32; (offset + capacity) as usize];
                for i in 0..live {
                    alive[(offset + i) as usize] = offset + live - i - 1;
                }
                let words = |values: &[u32]| {
                    values
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>()
                };
                let alive_buffer = upload(&words(&alive));
                let mut commands = vec![0u32; 12];
                commands[9] = live;
                let indirect = upload(&words(&commands));
                let mut globals = StorageBuffer::new(Vec::new());
                globals
                    .write(&GpuRenderGlobals {
                        world_from_effect: Mat4::from_translation(Vec3::new(1.0, 0.0, 3.0)),
                        ..default()
                    })
                    .unwrap();
                let globals = upload(globals.as_ref());
                let allocate = |bytes, usage| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: None,
                        size: bytes,
                        usage,
                        mapped_at_creation: false,
                    })
                };
                let runs = [
                    allocate(u64::from(count) * 16, wgpu::BufferUsages::STORAGE),
                    allocate(u64::from(count) * 16, wgpu::BufferUsages::STORAGE),
                ];
                let indices = allocate(
                    u64::from(count) * 4,
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                );
                let readback = allocate(
                    u64::from(count) * 4,
                    wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                );
                let view = if reverse {
                    Mat4::from_rotation_y(std::f32::consts::PI)
                } else {
                    Mat4::IDENTITY
                };
                let mut encoder = device.create_command_encoder(&default());
                for (stage, width) in widths.iter().enumerate() {
                    let mut data = UniformBuffer::new(Vec::new());
                    data.write(&Params {
                        view_from_world: view,
                        range: UVec4::new(offset, capacity, 2, *width),
                    })
                    .unwrap();
                    let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: data.as_ref(),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let buffers = [
                        &particles_buffer,
                        &alive_buffer,
                        &indirect,
                        &globals,
                        &params,
                        &runs[stage % 2],
                        &runs[(stage % 2) ^ 1],
                        &indices,
                    ];
                    let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                    let last = stage + 1 == widths.len();
                    let mut pass = encoder.begin_compute_pass(&default());
                    pass.set_pipeline(
                        &pipelines[if stage == 0 {
                            0
                        } else if last {
                            2
                        } else {
                            1
                        }],
                    );
                    pass.set_bind_group(0, &bindings, &[]);
                    let groups = count.div_ceil(if stage == 0 { 256 } else { 64 });
                    pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
                }
                encoder.copy_buffer_to_buffer(&indices, 0, &readback, 0, u64::from(count) * 4);
                queue.submit([encoder.finish()]);
                let (send, receive) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        send.send(result).unwrap();
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: None,
                        timeout: Some(std::time::Duration::from_secs(60)),
                    })
                    .unwrap();
                receive.recv().unwrap().unwrap();
                let bytes = readback.slice(..).get_mapped_range();
                let actual = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .take(live as usize)
                    .map(|p| u32::from_le_bytes(*p))
                    .collect::<Vec<_>>();
                let mut expected = (offset..offset + live).collect::<Vec<_>>();
                expected.sort_by(|a, b| {
                    let z = |slot: u32| {
                        (view
                            * (particles[slot as usize].position + Vec3::new(1.0, 0.0, 3.0))
                                .extend(1.0))
                        .z
                    };
                    z(*a)
                        .total_cmp(&z(*b))
                        .then(
                            particles[*a as usize]
                                .particle_index
                                .cmp(&particles[*b as usize].particle_index),
                        )
                        .then(a.cmp(b))
                });
                assert_eq!(
                    actual, expected,
                    "capacity={capacity} live={live} reverse={reverse}"
                );
                drop(bytes);
                readback.unmap();
                cases += 1;
            }
        }
    }
    println!(
        "alpha_sort_native cases={cases} max_capacity=65537 camera_orders=2 zero_partial_full=true test_only_readback=true adapter={}",
        adapter.get_info().name
    );
}
