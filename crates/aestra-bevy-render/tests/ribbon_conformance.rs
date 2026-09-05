//! Actual particle ABI and ribbon linking shader, independent of atomic compaction order.
use aestra_gpu::{
    GpuEmitter, GpuGlobals, GpuParticle,
    shader::{SIMULATION_WESL, compile_wesl},
};
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use wgpu::util::DeviceExt;

fn encode<T: ShaderType + WriteInto>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    StorageBuffer::new(&mut bytes).write(value).unwrap();
    bytes
}

#[test]
fn ribbon_geometry_has_front_facing_winding_shared_joins_and_hidden_degenerate_segments() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
        eprintln!("Skipping ribbon geometry conformance: no adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    // Invoke the actual shared vertex code with private fixture data. Buffer ABI and
    // linking are checked separately below; this probe isolates camera/geometry math.
    let mut source = aestra_gpu::shader::SPRITE_VERTEX_WESL.to_owned();
    for (declaration, replacement) in [
        (
            "@group(0) @binding(0) var<uniform> view: View;",
            "var<private> view: View;",
        ),
        (
            "@group(1) @binding(0) var<storage, read> renderers: array<Renderer>;",
            "var<private> renderers: array<Renderer, 1>;",
        ),
        (
            "@group(1) @binding(1) var<storage, read> particles: array<Particle>;",
            "var<private> particles: array<Particle, 3>;",
        ),
        (
            "@group(1) @binding(2) var<storage, read> alive_indices: array<u32>;",
            "var<private> alive_indices: array<u32, 3>;",
        ),
        (
            "@group(1) @binding(3) var<storage, read> globals: RenderGlobals;",
            "var<private> globals: RenderGlobals;",
        ),
        (
            "@group(1) @binding(4) var<storage, read> params: RenderParams;",
            "var<private> params: RenderParams;",
        ),
    ] {
        assert!(source.contains(declaration));
        source = source.replace(declaration, replacement);
    }
    source.push_str(r#"
@group(0) @binding(0) var<storage, read_write> probe: array<vec4<f32>>;
@compute @workgroup_size(1)
fn probe_ribbon(@builtin(global_invocation_id) id: vec3<u32>) {
    let scenario = id.x / 18u;
    let identity = mat4x4<f32>(vec4<f32>(1,0,0,0), vec4<f32>(0,1,0,0), vec4<f32>(0,0,1,0), vec4<f32>(0,0,0,1));
    view.clip_from_world = identity;
    view.world_from_view = identity;
    if scenario == 1u {
        view.world_from_view = mat4x4<f32>(vec4<f32>(0,0,-1,0), vec4<f32>(0,1,0,0), vec4<f32>(1,0,0,0), vec4<f32>(0,0,0,1));
    }
    globals.world_from_effect = identity;
    renderers[0].renderer_kind = 3u;
    renderers[0].attribute_flags.y = bitcast<u32>(1.0);
    for (var i = 0u; i < 3u; i++) {
        alive_indices[i] = i;
        particles[i].position = vec3<f32>(0.0, select(f32(i), 0.0, scenario == 2u), 0.0);
        particles[i].size = 1.0;
        particles[i]._padding_0 = select(i + 1u, 0xffffffffu, i == 2u);
        particles[i]._padding_1 = select(i - 1u, 0xffffffffu, i == 0u);
        particles[i]._padding_2 = bitcast<u32>(f32(i) * 0.5);
    }
    let value = aestra_sprite_vertex(id.x % 6u, (id.x % 18u) / 6u);
    probe[id.x * 2u] = value.clip_position;
    probe[id.x * 2u + 1u] = vec4<f32>(value.uv, f32(value.visible), 0.0);
}
"#);
    let shader = compile_wesl("package::ribbon_geometry_test", &source, &["probe_ribbon"]).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("probe_ribbon"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 54 * 32,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: output.as_entire_binding(),
        }],
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: output.size(),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(54, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
    let submission = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
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
    let bytes = readback.slice(..).get_mapped_range();
    let value = |vertex: usize, component: usize| {
        let offset = vertex * 32 + component * 4;
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let position = |v| bevy::math::Vec3::new(value(v, 0), value(v, 1), value(v, 2));
    for (scenario, camera) in [bevy::math::Vec3::Z, bevy::math::Vec3::X]
        .into_iter()
        .enumerate()
    {
        let base = scenario * 18;
        let normal =
            (position(base + 1) - position(base)).cross(position(base + 2) - position(base));
        assert!(normal.dot(camera) > 0.0, "front-facing winding");
        assert!(((position(base + 1) - position(base)).length() - 1.0).abs() < 1e-6);
        assert_eq!(position(base + 2), position(base + 7), "right join");
        assert_eq!(position(base + 5), position(base + 6), "left join");
        assert_eq!(value(base, 4), 0.0);
        assert_eq!(value(base + 2, 4), 0.5);
        assert_eq!(value(base + 8, 4), 1.0);
        for v in base..base + 12 {
            assert_eq!(value(v, 6), 1.0);
        }
        for v in base + 12..base + 18 {
            assert_eq!(value(v, 6), 0.0);
        }
    }
    for v in 36..54 {
        assert!(position(v).is_finite());
        assert_eq!(
            value(v, 6),
            0.0,
            "coincident and terminal segments are hidden"
        );
    }
}

#[test]
fn ribbon_linking_is_deterministic_for_sparse_empty_singleton_and_loop_identities() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        assert!(
            std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
            "native GPU required"
        );
        eprintln!("Skipping ribbon conformance: no adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let shader = compile_wesl("package::ribbon_test", SIMULATION_WESL, &["link_ribbons"]).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.wgsl.into()),
    });
    let entries = (0..7)
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
        .collect::<Vec<_>>();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("link_ribbons"),
        compilation_options: Default::default(),
        cache: None,
    });
    let emitters = vec![
        GpuEmitter {
            max_particles: 8,
            _turbulence_padding: 1,
            ..Default::default()
        },
        GpuEmitter {
            slot_offset: 8,
            max_particles: 2,
            ..Default::default()
        },
    ];
    // Physical slots deliberately differ from spawn order, including a previous loop's survivors.
    let identities = [129, 1, 128, 2, 256, 0, 257, 3, 9, 8];
    let particles = identities
        .iter()
        .map(|&particle_index| GpuParticle {
            particle_index,
            _padding_0: 17,
            _padding_1: 19,
            _padding_2: 23,
            ..Default::default()
        })
        .collect::<Vec<_>>();
    for selected in [vec![], vec![3], vec![7, 2], vec![7, 2, 0, 4, 1, 5]] {
        for reverse in [false, true] {
            let mut alive = vec![0; 10];
            let mut order = selected.clone();
            if reverse {
                order.reverse();
            }
            alive[..order.len()].copy_from_slice(&order);
            alive[8..].copy_from_slice(&[8, 9]);
            let data = [
                encode(&emitters),
                encode(&particles),
                encode(&alive),
                encode(&vec![0u32; 10]),
                encode(&vec![0u32; 2]),
                encode(&vec![6u32, order.len() as u32, 0, 0, 6, 2, 0, 0]),
                encode(&GpuGlobals {
                    emitter_count: 2,
                    ..Default::default()
                }),
            ];
            let buffers = data
                .iter()
                .map(|bytes| {
                    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytes,
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    })
                })
                .collect::<Vec<_>>();
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &buffers
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
            });
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: 680,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&buffers[1], 0, &readback, 0, 640);
            encoder.copy_buffer_to_buffer(&buffers[2], 0, &readback, 640, 40);
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
            let bytes = readback.slice(..).get_mapped_range();
            let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            let mut expected = selected.clone();
            expected.sort_by_key(|&slot| identities[slot as usize]);
            for (i, &slot) in expected.iter().enumerate() {
                assert_eq!(word(640 + i * 4), slot, "compaction {order:?}");
                let base = slot as usize * 64;
                assert_eq!(
                    word(base + 52),
                    expected.get(i + 1).copied().unwrap_or(u32::MAX)
                );
                assert_eq!(
                    word(base + 56),
                    i.checked_sub(1).map_or(u32::MAX, |j| expected[j])
                );
                assert!(
                    (f32::from_bits(word(base + 60))
                        - i as f32 / (expected.len().max(2) - 1) as f32)
                        .abs()
                        < 1e-6
                );
            }
            // Another emitter and dead slots must never be linked or reordered.
            for slot in (0..10u32).filter(|slot| !selected.contains(slot)) {
                assert_eq!(word(slot as usize * 64 + 52), 17);
            }
            assert_eq!(word(640 + 8 * 4), 8);
            assert_eq!(word(640 + 9 * 4), 9);
        }
    }
}
