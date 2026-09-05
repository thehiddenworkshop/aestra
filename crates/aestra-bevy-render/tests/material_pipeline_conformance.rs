//! Native stage-link validation for minimized semantic and legacy sprite interfaces.
use aestra_compiler::MaterialCompiler;
use aestra_core::material::{MaterialExpressionKind, MaterialInput, MaterialProgram};
use aestra_gpu::{
    material::{
        MATERIAL_FRAGMENT_ENTRY_POINT, MaterialBackendCapabilities, MaterialShaderCompiler,
    },
    shader::{GpuShaderKind, compile},
};
use std::borrow::Cow;

#[test]
fn minimized_material_and_legacy_stages_link_on_the_native_backend() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    let adapter = match adapter {
        Ok(adapter) if adapter.limits().max_storage_buffers_per_shader_stage >= 5 => adapter,
        _ => {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "a native sprite-capable adapter is required"
            );
            eprintln!("Skipping material pipeline conformance: no sprite-capable native adapter");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra material stage-link conformance"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();

    assert_tangent_transform(&device, &queue);
    let legacy = compile(GpuShaderKind::SpriteRender).unwrap();
    let wireframe = aestra_gpu::shader::compile_wesl(
        "package::aestra_mesh_wireframe",
        &aestra_gpu::shader::mesh_wireframe_wesl(),
        &["vertex_mesh_wireframe", "fragment_mesh_wireframe"],
    )
    .unwrap();
    for samples in [1, 4] {
        assert_pipeline(&device, &wireframe.wgsl, "fragment_mesh_wireframe", samples);
    }
    for fragment in [
        "fragment_alpha",
        "fragment_additive",
        "fragment_multiply",
        "fragment_wireframe",
    ] {
        assert_pipeline(&device, &legacy.wgsl, fragment, 1);
    }
    let mut mesh_program = MaterialProgram::from_ron(include_str!(
        "../../../assets/materials/mesh_material_lab.aestra.material.ron"
    ))
    .unwrap();
    // Both display modes use the exact same deformed vertex entry implementation.
    let breathing = MaterialShaderCompiler
        .compile(
            &MaterialCompiler.compile(&mesh_program).unwrap(),
            &MaterialBackendCapabilities::portable_minimum(),
        )
        .unwrap();
    for samples in [1, 4] {
        assert_pipeline(
            &device,
            &breathing.shader.wgsl,
            MATERIAL_FRAGMENT_ENTRY_POINT,
            samples,
        );
        assert_pipeline(
            &device,
            &breathing.shader.wgsl,
            "fragment_mesh_wireframe",
            samples,
        );
    }
    // Exercise geometry position, normal, UV and view direction interfaces in native pipelines.
    for input in [
        MaterialInput::Normal,
        MaterialInput::Tangent,
        MaterialInput::Bitangent,
        MaterialInput::LocalPosition,
        MaterialInput::WorldPosition,
        MaterialInput::ViewDirection,
    ] {
        mesh_program
            .expressions
            .iter_mut()
            .find(|expression| expression.id == mesh_program.outputs.color)
            .unwrap()
            .kind = MaterialExpressionKind::Input(input);
        let ir = MaterialCompiler.compile(&mesh_program).unwrap();
        let compiled = MaterialShaderCompiler
            .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
            .unwrap();
        assert_pipeline(
            &device,
            &compiled.shader.wgsl,
            MATERIAL_FRAGMENT_ENTRY_POINT,
            1,
        );
        if input == MaterialInput::Bitangent {
            // The basis input must also survive when only the vertex-offset path reads it.
            let mut vertex_program = mesh_program.clone();
            vertex_program.outputs.vertex_offset = Some(vertex_program.outputs.color);
            let color = aestra_core::MaterialExpressionId::new();
            vertex_program
                .expressions
                .push(aestra_core::material::MaterialExpression {
                    id: color,
                    kind: MaterialExpressionKind::Constant(
                        aestra_core::material::MaterialValue::Vec3([1.0; 3]),
                    ),
                });
            vertex_program.outputs.color = color;
            let compiled = MaterialShaderCompiler
                .compile(
                    &MaterialCompiler.compile(&vertex_program).unwrap(),
                    &MaterialBackendCapabilities::portable_minimum(),
                )
                .unwrap();
            for samples in [1, 4] {
                assert_pipeline(
                    &device,
                    &compiled.shader.wgsl,
                    MATERIAL_FRAGMENT_ENTRY_POINT,
                    samples,
                );
                assert_pipeline(
                    &device,
                    &compiled.shader.wgsl,
                    "fragment_mesh_wireframe",
                    samples,
                );
            }
        }
    }
    for input in [
        None,
        Some(MaterialInput::ParticleOpacity),
        Some(MaterialInput::ParticleNormalizedAge),
        Some(MaterialInput::EffectTime),
        Some(MaterialInput::SceneDepth),
        Some(MaterialInput::PixelDepth),
    ] {
        let mut program = MaterialProgram::additive_sprite("Varying contract");
        if let Some(input) = input {
            program
                .expressions
                .iter_mut()
                .find(|expression| expression.id == program.outputs.alpha)
                .unwrap()
                .kind = MaterialExpressionKind::Input(input);
            program
                .expressions
                .iter_mut()
                .find(|expression| expression.id == program.outputs.color)
                .unwrap()
                .kind = MaterialExpressionKind::Input(MaterialInput::ParticleColor);
        }
        let ir = MaterialCompiler.compile(&program).unwrap();
        let compiled = MaterialShaderCompiler
            .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
            .unwrap();
        assert_pipeline(
            &device,
            &compiled.shader.wgsl,
            MATERIAL_FRAGMENT_ENTRY_POINT,
            1,
        );
        assert_pipeline(
            &device,
            &compiled.multisampled_shader.wgsl,
            MATERIAL_FRAGMENT_ENTRY_POINT,
            4,
        );
    }
}

fn assert_pipeline(device: &wgpu::Device, wgsl: &str, fragment: &str, samples: u32) {
    let wireframe = fragment == "fragment_mesh_wireframe";
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Aestra sprite stage-link contract"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
    });
    let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x2, 4 => Float32x4];
    let mesh_layout = [wgpu::VertexBufferLayout {
        array_stride: 56,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &mesh_attributes,
    }];
    let line_attributes = mesh_attributes;
    let line_layout = [wgpu::VertexBufferLayout {
        array_stride: 56,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &line_attributes,
    }];
    let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some(if wireframe {
                "vertex_mesh_wireframe"
            } else {
                "vertex"
            }),
            compilation_options: Default::default(),
            buffers: if wireframe {
                &line_layout
            } else if wgsl.contains("struct MeshVertexInput") {
                &mesh_layout
            } else {
                &[]
            },
        },
        primitive: wgpu::PrimitiveState {
            topology: if wireframe {
                wgpu::PrimitiveTopology::LineList
            } else {
                wgpu::PrimitiveTopology::TriangleList
            },
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let error = pollster::block_on(scope.pop());
    assert!(error.is_none(), "{fragment}, samples={samples}: {error:?}");
}

fn assert_tangent_transform(device: &wgpu::Device, queue: &wgpu::Queue) {
    // Exercise the actual shared shader math: nonuniform/mirrored scale plus a 90-degree
    // rotation. A translation column must not affect a direction.
    let source = format!(
        "{}\n{}",
        include_str!("../../aestra-gpu/src/shaders/aestra_mesh_tangent.wesl"),
        r#"
        @group(0) @binding(0) var<storage, read_write> results: array<vec4<f32>>;
        @compute @workgroup_size(1)
        fn check() {
            let transform = mat4x4<f32>(vec4<f32>(0.0, -2.0, 0.0, 0.0),
                vec4<f32>(-3.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 0.5, 0.0),
                vec4<f32>(20.0, 30.0, 40.0, 1.0));
            results[0] = vec4<f32>(aestra_mesh_tangent(normalize(vec3<f32>(1.0, 1.0, 0.0)),
                vec3<f32>(0.0, 0.0, 1.0), transform), 0.0);
            // Gram-Schmidt removes a normal component in imperfect imported tangent data.
            results[1] = vec4<f32>(aestra_mesh_tangent(vec3<f32>(1.0, 0.0, 0.25),
                vec3<f32>(0.0, 0.0, 1.0), transform), 0.0);
            let normal = vec3<f32>(0.0, 0.0, 1.0);
            results[2] = vec4<f32>(aestra_mesh_bitangent(normal, results[0].xyz, 1.0, transform), 0.0);
            results[3] = vec4<f32>(aestra_mesh_bitangent(normal, results[0].xyz, -1.0, transform), 0.0);
            var positive = transform;
            positive[0].y = 2.0;
            let tangent = aestra_mesh_tangent(normalize(vec3<f32>(1.0, 1.0, 0.0)), normal, positive);
            results[4] = vec4<f32>(aestra_mesh_bitangent(normal, tangent, 1.0, positive), 0.0);
            results[5] = vec4<f32>(aestra_mesh_bitangent(normal, tangent, -1.0, positive), 0.0);
            let identity = mat4x4<f32>(vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 1.0, 0.0, 0.0),
                vec4<f32>(0.0, 0.0, 1.0, 0.0), vec4<f32>(0.0, 0.0, 0.0, 1.0));
            results[6] = vec4<f32>(aestra_mesh_bitangent(normal, vec3<f32>(1.0, 0.0, 0.0), 1.0, identity), 0.0);
            results[7] = vec4<f32>(aestra_mesh_bitangent(normal, vec3<f32>(1.0, 0.0, 0.0), -1.0, identity), 0.0);
        }
    "#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mesh tangent transform"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("check"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 128,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 128,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
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
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 128);
    let submission = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
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
    let mapped = readback.slice(..).get_mapped_range();
    let values = mapped
        .chunks_exact(4)
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    let length = 13.0_f32.sqrt();
    assert_eq!(values.len(), 32);
    for (actual, expected) in values.iter().zip([
        -3.0 / length,
        -2.0 / length,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        0.0,
        -2.0 / length,
        3.0 / length,
        0.0,
        0.0,
        2.0 / length,
        -3.0 / length,
        0.0,
        0.0,
        -2.0 / length,
        -3.0 / length,
        0.0,
        0.0,
        2.0 / length,
        3.0 / length,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        -1.0,
        0.0,
        0.0,
    ]) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "tangent transform {actual} != {expected}"
        );
    }
}
