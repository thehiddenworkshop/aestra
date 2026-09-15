//! Real GPU stable compaction of sparse/retired histories, independent of cameras.
use aestra_gpu::{GpuParticle, GpuRenderGlobals, shader};
use bevy::math::{UVec4, Vec3};
use encase::ShaderType;
use wgpu::util::DeviceExt;

fn encode<T: ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = encase::StorageBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

#[test]
fn compacted_trails_preserve_order_caps_expired_anchors_and_rebuild_after_seek() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let gpu = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(gpu.request_adapter(&Default::default())) else {
        assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
        eprintln!("Skipping trail compaction: no adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let code = shader::compile_wesl(
        "package::trail_compact",
        &shader::trail_compact_wesl(),
        &["classify_trail", "prefix_trail", "scatter_trail"],
    )
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(code.wgsl.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &(0..7)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 4 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding < 5,
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
    let pipelines = ["classify_trail", "prefix_trail", "scatter_trail"].map(|entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    });
    let mut asset = aestra_core::EffectAsset::new("Compaction", 3.0);
    asset
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Trail", 3.0));
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&asset)
        .unwrap();
    let instance = aestra_runtime::EffectInstance::new(std::sync::Arc::new(compiled));
    let mut renderer = aestra_gpu::GpuEffectArtifact::from_instance(&instance)
        .unwrap()
        .renderers[0];
    renderer.renderer_kind = 4;
    renderer.attribute_flags.z = 0;
    renderer.frame_count = 5;
    renderer.frame_rate = 1.0;
    // Cross workgroup boundaries, with most owners empty; capped stride is 20.
    let owners = 70u32;
    renderer.playback_mode = owners;
    let mut particles = vec![GpuParticle::default(); 1 + owners as usize * 5];
    let mut aux = vec![0u32; particles.len() * 3];
    for owner in [0, 32, 69] {
        let base = 1 + owner * 5;
        // Retired owners also retain an alive history header, even without a live parent.
        particles[base] = GpuParticle {
            packed_emitter_alive: 1,
            position: Vec3::X * 2.0,
            rotation: 1.2,
            ..Default::default()
        };
        aux[base * 3] = 2;
        aux[base * 3 + 1] = 2;
        particles[base + 1] = GpuParticle {
            position: Vec3::ZERO,
            rotation: 0.0,
            ..Default::default()
        };
        particles[base + 2] = GpuParticle {
            position: Vec3::X,
            rotation: 0.9,
            ..Default::default()
        };
    }
    let run = |particles: &[GpuParticle], aux: &[u32], flags: u32, time: f32, repeats: usize| {
        let mut renderer = renderer;
        renderer.flipbook_flags = flags;
        let stride = if flags & 2 != 0 { 20 } else { 4 };
        let count = owners * stride;
        let buffers = [
            encode(&particles.to_vec()),
            encode(&vec![renderer]),
            encode(&GpuRenderGlobals {
                time,
                ..Default::default()
            }),
            encode(&aux.to_vec()),
            encode(&UVec4::new(0, count, owners, stride)),
            encode(&vec![u32::MAX; (count + owners) as usize]),
            encode(&vec![u32::MAX; (count + 4) as usize]),
        ]
        .into_iter()
        .enumerate()
        .map(|(binding, bytes)| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes,
                usage: (if binding == 4 {
                    wgpu::BufferUsages::UNIFORM
                } else {
                    wgpu::BufferUsages::STORAGE
                }) | wgpu::BufferUsages::COPY_SRC
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
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(count + 4) * 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        // Repeated frames must overwrite counters and ranks, not accumulate.
        for _ in 0..repeats {
            for (stage, pipeline) in pipelines.iter().enumerate() {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(
                    match stage {
                        0 => owners.div_ceil(64),
                        1 => 1,
                        _ => count.div_ceil(64),
                    },
                    1,
                    1,
                );
            }
        }
        encoder.copy_buffer_to_buffer(&buffers[6], 0, &readback, 0, readback.size());
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
        let data = readback.slice(..).get_mapped_range();
        let words: Vec<_> = data
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| u32::from_le_bytes(*v))
            .collect();
        assert_eq!([words[0], words[2], words[3]], [4, 0, 0]);
        assert!(words[1] <= count);
        let indices = words[4..4 + words[1] as usize].to_vec();
        assert!(
            indices.windows(2).all(|pair| pair[0] < pair[1]),
            "alpha order must be stable"
        );
        indices
    };
    for flags in [0, 1, 2, 3] {
        // Stretch/tiled UVs and flat/round caps.
        let stride = if flags & 2 != 0 { 20 } else { 4 };
        let expected: Vec<_> = [0, 32, 69]
            .into_iter()
            .flat_map(|owner| {
                [0, 1]
                    .into_iter()
                    .chain(if flags & 2 != 0 { 4..20 } else { 0..0 })
                    .map(move |p| owner * stride + p)
            })
            .collect();
        assert_eq!(run(&particles, &aux, flags, 1.5, 1), expected);
        assert_eq!(run(&particles, &aux, flags, 1.5, 4), expected);
    }
    // Coincident head: reject only the zero-length body, keep round cap using the preceding anchor.
    for owner in [0, 32, 69] {
        particles[1 + owner * 5].position = Vec3::X;
    }
    let capped = run(&particles, &aux, 2, 1.5, 2);
    for owner in [0, 32, 69] {
        assert!(!capped.contains(&(owner * 20 + 1)));
        assert!(capped.contains(&(owner * 20 + 12)));
    }
    // Expiry must reject otherwise non-degenerate body segments and caps.
    assert!(run(&particles, &aux, 2, 4.0, 1).is_empty());
    // All anchors coincident: no body or cap can generate geometry.
    for particle in &mut particles {
        particle.position = Vec3::ZERO;
    }
    assert!(run(&particles, &aux, 2, 1.5, 1).is_empty());
    // Expired histories and cleared seek/restart state also produce a zero indirect count.
    assert!(run(&particles, &aux, 2, 4.0, 1).is_empty());
    aux.fill(0);
    assert!(run(&particles, &aux, 2, 0.0, 2).is_empty());
}
