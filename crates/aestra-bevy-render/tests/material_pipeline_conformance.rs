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
    let (device, _) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Aestra material stage-link conformance"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();

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
    let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    let mesh_layout = [wgpu::VertexBufferLayout {
        array_stride: 32,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &mesh_attributes,
    }];
    let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    let line_layout = [wgpu::VertexBufferLayout {
        array_stride: 32,
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
