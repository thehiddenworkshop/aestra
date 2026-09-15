//! Invoke the production trail vertex path to verify Stretch/Tile UVs.
use aestra_gpu::shader::compile_wesl;

#[test]
fn trail_uvs_stretch_or_tile_with_continuous_joins_and_stable_phase() {
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
            "var<private> particles: array<Particle, 5>;",
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
        (
            "@group(1) @binding(7) var<storage, read> aux: array<u32>;",
            "var<private> aux: array<u32, 15>;",
        ),
    ] {
        assert!(source.contains(declaration));
        source = source.replace(declaration, replacement);
    }
    source.push_str(r#"
@group(0) @binding(0) var<storage, read_write> probe: array<vec4<f32>>;
@compute @workgroup_size(1)
fn probe_trail(@builtin(global_invocation_id) id: vec3<u32>) {
    let rounded = id.x >= 32u;
    let scenario = select(id.x / 8u, 4u + (id.x - 32u) / 76u, rounded);
    let identity = mat4x4<f32>(vec4<f32>(1,0,0,0), vec4<f32>(0,1,0,0), vec4<f32>(0,0,1,0), vec4<f32>(0,0,0,1));
    view.clip_from_world = identity;
    view.world_from_view = identity;
    globals.time = 0.5;
    if scenario == 5u { globals.time = 0.75; } // retired head keeps aging
    if scenario == 8u { globals.time = 1.5; } // all history expired
    renderers[0].renderer_kind = 4u;
    renderers[0].frame_count = 4u;
    renderers[0].frame_rate = 1.0;
    renderers[0].attribute_flags.y = bitcast<u32>(1.0);
    renderers[0].flipbook_flags = select(1u, 0u, scenario == 0u);
    if rounded { renderers[0].flipbook_flags |= 2u; }
    renderers[0].frames[0].x = select(2.0, 4.0, scenario == 3u);
    particles[1].packed_emitter_alive = 1u;
    particles[1].position = vec3<f32>(4.0, 0.0, 0.0);
    particles[1].rotation = 0.5;
    particles[1].size = 1.0;
    particles[2].position = vec3<f32>(0.0);
    particles[2].rotation = 0.0;
    particles[2].size = 1.0;
    particles[3].position = vec3<f32>(2.0, 0.0, 0.0);
    particles[3].rotation = 0.25;
    particles[3].size = 1.0;
    if scenario == 6u { particles[3].position = particles[1].position; }
    if scenario == 7u {
        particles[2].position = particles[1].position;
        particles[3].position = particles[1].position;
    }
    aux[3] = 2u; // next ring index
    aux[4] = select(2u, 1u, scenario == 2u); // expire the oldest anchor
    aux[6] = bitcast<u32>(10.0);
    aux[7] = bitcast<u32>(14.0); // cumulative head distance
    aux[9] = bitcast<u32>(12.0);
    let primitive = select((id.x % 8u) / 4u, ((id.x - 32u) % 76u) / 4u, rounded);
    let value = aestra_trail_vertex(id.x % 4u, primitive);
    probe[id.x * 2u] = vec4<f32>(value.uv, f32(value.visible), value.quad_position.y);
    probe[id.x * 2u + 1u] = value.clip_position;
}
"#);
    let shader = compile_wesl("package::trail_uv_test", &source, &["probe_trail"]).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("probe_trail"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (32 + 5 * 76) * 32,
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
        pass.dispatch_workgroups(32 + 5 * 76, 1, 1);
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
    for (scenario, expected) in [
        [0.0, 0.5, 1.0],
        [5.0, 6.0, 7.0],
        [6.0, 7.0, 0.0],
        [2.5, 3.0, 3.5],
    ]
    .into_iter()
    .enumerate()
    {
        let base = scenario * 8;
        assert_eq!(value(base, 0), expected[0]);
        assert_eq!(value(base + 2, 0), expected[1]);
        assert_eq!(value(base, 1), 0.0);
        assert_eq!(value(base + 1, 1), 1.0);
        if scenario != 2 {
            assert_eq!(value(base + 6, 0), expected[2]);
            assert_eq!(value(base + 2, 0), value(base + 4, 0), "shared join U");
            assert_eq!(value(base + 3, 1), value(base + 5, 1), "shared join V");
            assert_eq!(value(base, 3), 0.5, "age fade is independent of UV mode");
        } else {
            assert_eq!(value(base + 4, 2), 0.0, "expired segment hidden");
        }
        assert_eq!(value(base, 2), 1.0);
    }
    for scenario in 4..9 {
        let base = 32 + (scenario - 4) * 76;
        for primitive in 3..19 {
            let head = primitive >= 11;
            let center_x = if head { 4.0 } else { 0.0 };
            let fade = match (scenario, head) {
                (5, true) => 0.75,
                (5, false) => 0.25,
                (_, true) => 1.0,
                (_, false) => 0.5,
            };
            for vertex in 0..4 {
                let index = base + primitive * 4 + vertex;
                if scenario >= 7 {
                    assert_eq!(
                        value(index, 2),
                        0.0,
                        "coincident/expired trails have no caps"
                    );
                    continue;
                }
                assert_eq!(
                    value(index, 2),
                    1.0,
                    "caps remain visible at coincident head samples"
                );
                assert_eq!(
                    value(index, 0),
                    if head { 7.0 } else { 5.0 },
                    "cap UV meets the body endpoint"
                );
                assert_eq!(
                    value(index, 3),
                    fade,
                    "retired caps age with their endpoint"
                );
                let dx = value(index, 4) - center_x;
                let dy = value(index, 5);
                let radius = if vertex == 0 { 0.0 } else { fade * 0.5 };
                assert!((dx.hypot(dy) - radius).abs() < 0.0001, "cap is circular");
                assert!(
                    if head { dx >= -0.0001 } else { dx <= 0.0001 },
                    "cap extends outward only"
                );
                if vertex == 3 {
                    assert_eq!(value(index, 4), value(index - 1, 4));
                    assert_eq!(
                        value(index, 5),
                        value(index - 1, 5),
                        "second strip triangle degenerates"
                    );
                }
            }
        }
        if scenario == 4 {
            // Tail and head fan endpoints meet the body exactly across its width.
            for (cap, body) in [
                (3 * 4 + 1, 0),
                (10 * 4 + 2, 1),
                (11 * 4 + 1, 7),
                (18 * 4 + 2, 6),
            ] {
                for component in 4..7 {
                    assert!(
                        (value(base + cap, component) - value(base + body, component)).abs()
                            < 0.0001
                    );
                }
            }
        }
    }
}
