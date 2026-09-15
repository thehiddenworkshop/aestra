//! Numerical GPU/reference conformance for normal maps and the imported mesh tangent basis.
use aestra_compiler::evaluate_normal_map;
use std::fmt::Write;

#[test]
fn normal_maps_match_reference_across_mirrors_handedness_strength_and_conventions() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        assert!(
            std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
            "native GPU is required"
        );
        eprintln!("Skipping normal-map numeric conformance: no native adapter");
        return;
    };
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let mut source = format!(
        "{}\n{}\n@group(0) @binding(0) var<storage, read_write> results: array<vec4<f32>>;\n@compute @workgroup_size(1) fn check() {{\n",
        include_str!("../../aestra-gpu/src/shaders/aestra_normal_map.wesl"),
        include_str!("../../aestra-gpu/src/shaders/aestra_mesh_tangent.wesl")
    );
    let mut expected = Vec::new();
    for sx in [-2.0_f32, 2.0] {
        for handedness in [-1.0_f32, 1.0] {
            for flip_y in [false, true] {
                for strength in [0.0_f32, 0.5, 2.0] {
                    let index = expected.len();
                    // Nonuniform scale, quarter-turn rotation, and translation. The sign of
                    // sx controls the transform mirror independently of UV handedness.
                    writeln!(source, "{{ let matrix = mat4x4<f32>(vec4<f32>(0.0, {sx:?}, 0.0, 0.0), vec4<f32>(-3.0, 0.0, 0.0, 0.0), vec4<f32>(0.0, 0.0, 0.5, 0.0), vec4<f32>(20.0, 30.0, 40.0, 1.0));
                        let n = vec3<f32>(0.0, 0.0, 1.0);
                        let t = aestra_mesh_tangent(normalize(vec3<f32>(1.0, 1.0, 0.0)), n, matrix);
                        let b = aestra_mesh_bitangent(n, t, {handedness:?}, matrix);
                        results[{index}] = vec4<f32>(aestra_normal_map(vec3<f32>(0.8, 0.65, 0.9), {strength:?}, {flip_y}, n, t, b), 1.0); }}").unwrap();
                    let length = 13.0_f32.sqrt();
                    let t = [-3.0 / length, sx / length, 0.0];
                    let sign = sx.signum() * handedness;
                    let b = [-sx / length * sign, -3.0 / length * sign, 0.0];
                    expected.push(evaluate_normal_map(
                        [0.8, 0.65, 0.9],
                        strength,
                        flip_y,
                        [0.0, 0.0, 1.0],
                        t,
                        b,
                    ));
                }
            }
        }
    }
    source.push_str("}\n");
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Normal map conformance"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("check"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bytes = expected.len() as u64 * 16;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
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
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, bytes);
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
    for (index, (bytes, expected)) in mapped.as_chunks::<16>().0.iter().zip(expected).enumerate() {
        let actual: [f32; 3] = std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        for (a, e) in actual.into_iter().zip(expected) {
            assert!(
                (a - e).abs() < 1e-5,
                "case {index}: {actual:?} != {expected:?}"
            );
        }
    }
}
