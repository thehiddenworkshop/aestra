//! Actual native rasterization, not a duplicate CPU implementation of the sampling math.
use aestra_compiler::{EffectCompiler, MaterialCompiler};
use aestra_core::{EffectAsset, Emitter, material::MaterialProgram};
use aestra_gpu::{
    GpuEffectArtifact, GpuParticle, GpuRenderGlobals, GpuRenderParams, GpuRenderer,
    material::{
        MATERIAL_FRAGMENT_ENTRY_POINT, MaterialBackendCapabilities, MaterialShaderCompiler,
    },
    shader::{GpuShaderKind, compile},
};
use bevy::math::{Mat4, Vec3, Vec4};
use std::{borrow::Cow, sync::Arc, time::Duration};
use wgpu::util::DeviceExt;

fn storage<T: encase::ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = encase::StorageBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

fn map(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: wgpu::CommandEncoder,
    buffer: &wgpu::Buffer,
) -> Vec<u8> {
    let submission = queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: Some(submission),
            timeout: Some(Duration::from_secs(60)),
        })
        .unwrap();
    receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    let bytes = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

#[test]
fn subpixel_sprites_survive_pixel_phase_without_amplifying_quad_energy() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let adapter = match pollster::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) if adapter.limits().max_storage_buffers_per_shader_stage >= 6 => adapter,
        _ => {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "native sprite-capable adapter required"
            );
            eprintln!("Skipping sprite sampling conformance: no native adapter");
            return;
        }
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    assert_projection_math(&device, &queue);
    let legacy = compile(GpuShaderKind::SpriteRender).unwrap();
    // Constant alpha ensures attenuation is not dependent on the authored ParticleOpacity input.
    let semantic = MaterialShaderCompiler
        .compile(
            &MaterialCompiler
                .compile(&MaterialProgram::additive_sprite("Sampling contract"))
                .unwrap(),
            &MaterialBackendCapabilities::portable_minimum(),
        )
        .unwrap();
    let mut effect = EffectAsset::new("Sampling", 1.0);
    effect.emitters.push(Emitter::basic_sprite("Stars", 1.0));
    let artifact = GpuEffectArtifact::from_instance(&aestra_runtime::EffectInstance::new(
        Arc::new(EffectCompiler::default().compile(&effect).unwrap()),
    ))
    .unwrap();
    let mut renderer = artifact.renderers[0];
    renderer.blend_mode = aestra_gpu::GpuBlend::Additive as u32;
    renderer.softness = 0.2;
    renderer.tint = Vec4::ONE;
    for (shader, fragment) in [
        (&legacy.wgsl, "fragment_additive"),
        (&semantic.shader.wgsl, MATERIAL_FRAGMENT_ENTRY_POINT),
    ] {
        let off = raster(&device, &queue, shader, fragment, renderer, 0.25, 0.0);
        assert!(
            cell_energy(&off).contains(&0.0),
            "untreated quarter-pixel quads should demonstrate dropout"
        );
        for floor in [2.0, 4.0] {
            let treated = raster(&device, &queue, shader, fragment, renderer, 0.25, floor);
            for energy in cell_energy(&treated) {
                assert!(
                    energy > 0.0 && energy <= 0.25 * 0.25 + 1e-5,
                    "{fragment}, floor {floor}: invalid cell energy {energy}"
                );
            }
            let zero = raster(&device, &queue, shader, fragment, renderer, 0.0, floor);
            assert!(
                cell_energy(&zero).iter().all(|energy| *energy == 0.0),
                "zero-size sprites must remain invisible"
            );
        }
        let large_off = raster(&device, &queue, shader, fragment, renderer, 4.0, 0.0);
        let large_on = raster(&device, &queue, shader, fragment, renderer, 4.0, 2.0);
        assert_eq!(large_off, large_on, "resolved quads must be unchanged");
        assert_temporal_sampling(&device, &queue, shader, fragment, renderer);
        assert_viewport_clipping(&device, &queue, shader, fragment, renderer);
    }
}

fn assert_temporal_sampling(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
) {
    // Cross the pixel lattice continuously in two axes, at two quad orientations.
    // Read linear alpha before exposure/bloom. This is a bounded procedural-mask probe,
    // not a guarantee of shimmer-free arbitrary semantic materials or trail sampling.
    for rotation in [0.0, 0.65] {
        let mut untreated_cv = 0.0;
        let mut two_pixel_cv = 0.0;
        for floor in [0.0, 2.0, 4.0] {
            let mut trajectories = vec![Vec::new(); 16];
            for tick in 0..32 {
                let phase = [tick as f32 / 32.0, tick as f32 * 0.618034 / 32.0];
                let bytes = raster_case(
                    device,
                    queue,
                    source,
                    fragment,
                    renderer,
                    RasterCase {
                        pixels: 0.25,
                        floor,
                        phase,
                        rotation,
                        translation_pixels: [0.0; 2],
                    },
                );
                for (trajectory, energy) in trajectories.iter_mut().zip(cell_energy(&bytes)) {
                    trajectory.push(energy);
                }
            }
            let samples = trajectories.iter().flatten().copied().collect::<Vec<_>>();
            let mean = samples.iter().sum::<f32>() / samples.len() as f32;
            let cv = (samples
                .iter()
                .map(|energy| (energy - mean).powi(2))
                .sum::<f32>()
                / samples.len() as f32)
                .sqrt()
                / mean;
            let dropouts = samples.iter().filter(|energy| **energy == 0.0).count();
            eprintln!(
                "{fragment}: rotation={rotation}, floor={floor}, samples={}, dropouts={dropouts}, mean_alpha={mean:.6}, relative_stddev={cv:.4}",
                samples.len()
            );
            let upper_bound = if floor == 0.0 { 1.0 } else { 0.0625 };
            assert!(
                samples.iter().all(|value| value.is_finite()
                    && *value >= 0.0
                    && *value <= upper_bound + 1e-5)
            );
            if floor == 0.0 {
                assert!(
                    dropouts > 0 && mean > 0.0,
                    "probe must demonstrate untreated modulation, not all-invisible geometry"
                );
                untreated_cv = cv;
            } else {
                assert_eq!(
                    dropouts, 0,
                    "{fragment}: moving sampled quads must not drop out"
                );
                assert!(
                    cv < untreated_cv * 0.5,
                    "{fragment}: temporal modulation {cv} did not improve over {untreated_cv}"
                );
                // Integral of the smoothstep-feathered circular mask: R²π(1-f+0.3f²).
                // Phase-averaged sum, not an analytic integral for each rasterized frame.
                let expected = 0.0625 * std::f32::consts::PI * 0.25 * (1.0 - 0.2 + 0.3 * 0.2 * 0.2);
                assert!(
                    (mean - expected).abs() < expected * 0.05,
                    "phase-averaged coverage drifted: {mean} != {expected}"
                );
                if floor == 2.0 {
                    two_pixel_cv = cv;
                } else {
                    assert!(
                        cv < two_pixel_cv * 0.5,
                        "larger floor must improve this bounded procedural probe"
                    );
                }
            }
        }
    }
}

fn assert_viewport_clipping(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
) {
    // One column/row's centers are a quarter pixel beyond each clip edge. The original
    // quads are entirely outside, but expanded footprints correctly reach the viewport.
    // This is why world-space AABB culling cannot discard treated draws unchanged.
    for translation_pixels in [[28.25, 0.0], [-29.0, 0.0], [0.0, 28.25], [0.0, -29.0]] {
        for floor in [0.0, 2.0, 4.0] {
            let bytes = raster_case(
                device,
                queue,
                source,
                fragment,
                renderer,
                RasterCase {
                    pixels: 0.25,
                    floor,
                    phase: [0.0; 2],
                    rotation: 0.0,
                    translation_pixels,
                },
            );
            let energy = cell_energy(&bytes).iter().sum::<f32>();
            assert!(energy.is_finite());
            if floor == 0.0 {
                assert_eq!(
                    energy, 0.0,
                    "untreated {fragment} edge {translation_pixels:?}"
                );
            } else {
                assert!(
                    energy > 0.0 && energy <= 4.0 * 0.0625,
                    "expanded {fragment} edge {translation_pixels:?}, floor {floor}: {energy}"
                );
            }
        }
    }
    // The maximum policy must not pull a distant offscreen draw back into view.
    for floor in [0.0, 2.0, 4.0, 8.0] {
        let bytes = raster_case(
            device,
            queue,
            source,
            fragment,
            renderer,
            RasterCase {
                pixels: 0.25,
                floor,
                phase: [0.0; 2],
                rotation: 0.65,
                translation_pixels: [64.0, 0.0],
            },
        );
        assert!(
            bytes.iter().all(|byte| *byte == 0),
            "fully clipped {fragment}, floor {floor}"
        );
    }
}

fn cell_energy(bytes: &[u8]) -> Vec<f32> {
    (0..16)
        .map(|cell| {
            let mut energy = 0.0;
            for y in (cell / 4 * 8)..(cell / 4 * 8 + 8) {
                for x in (cell % 4 * 8)..(cell % 4 * 8 + 8) {
                    let offset = (y * 32 + x) * 16 + 12;
                    energy += f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
                }
            }
            energy
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn raster(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    renderer: GpuRenderer,
    pixels: f32,
    floor: f32,
) -> Vec<u8> {
    raster_case(
        device,
        queue,
        source,
        fragment,
        renderer,
        RasterCase {
            pixels,
            floor,
            phase: [0.0; 2],
            rotation: 0.0,
            translation_pixels: [0.0; 2],
        },
    )
}

struct RasterCase {
    pixels: f32,
    floor: f32,
    phase: [f32; 2],
    rotation: f32,
    translation_pixels: [f32; 2],
}

fn raster_case(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    source: &str,
    fragment: &str,
    mut renderer: GpuRenderer,
    case: RasterCase,
) -> Vec<u8> {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sprite sampling raster"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
    });
    let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let entries = (0..8)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: if binding == 5 || binding == 6 {
                wgpu::ShaderStages::FRAGMENT
            } else {
                wgpu::ShaderStages::VERTEX_FRAGMENT
            },
            ty: match binding {
                5 => wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                6 => wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                _ => wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            },
            count: None,
        })
        .collect::<Vec<_>>();
    let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&view_layout), Some(&draw_layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba32Float,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    });
    let init = |bytes: &[u8], usage| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
    };
    // Bevy ViewUniform prefix through main_pass_viewport, with seven identity matrices.
    let mut view = vec![0_u8; 512];
    for matrix in 0..7 {
        for diagonal in 0..4 {
            let offset = matrix * 64 + diagonal * 20;
            view[offset..offset + 4].copy_from_slice(&1_f32.to_le_bytes());
        }
    }
    for offset in [472, 476, 488, 492] {
        view[offset..offset + 4].copy_from_slice(&32_f32.to_le_bytes());
    }
    let view = init(&view, wgpu::BufferUsages::UNIFORM);
    let view_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &view_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        }],
    });
    renderer.attribute_flags.y = case.floor.to_bits();
    let particles = (0..16)
        .map(|i| {
            let x = 4.0
                + (i % 4) as f32 * 8.0
                + ((i % 4) as f32 * 0.25 + case.phase[0]).fract()
                + case.translation_pixels[0];
            let y = 4.0
                + (i / 4) as f32 * 8.0
                + ((i / 4) as f32 * 0.25 + case.phase[1]).fract()
                + case.translation_pixels[1];
            GpuParticle {
                color: Vec4::ONE,
                position: Vec3::new(x / 16.0 - 1.0, 1.0 - y / 16.0, 0.5),
                size: case.pixels / 16.0,
                rotation: case.rotation,
                packed_emitter_alive: 1,
                particle_index: i,
                ..Default::default()
            }
        })
        .collect::<Vec<_>>();
    let data = [
        storage(&vec![renderer]),
        storage(&particles),
        storage(&(0..16_u32).collect::<Vec<_>>()),
        storage(&GpuRenderGlobals {
            world_from_effect: Mat4::IDENTITY,
            ..Default::default()
        }),
        storage(&GpuRenderParams::default()),
        storage(&vec![0_u32; 128]),
    ];
    let buffers = data
        .iter()
        .map(|bytes| init(bytes, wgpu::BufferUsages::STORAGE))
        .collect::<Vec<_>>();
    let white = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let white_view = white.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let mut draw_entries = (0..5)
        .map(|binding| wgpu::BindGroupEntry {
            binding,
            resource: buffers[binding as usize].as_entire_binding(),
        })
        .collect::<Vec<_>>();
    draw_entries.extend([
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(&white_view),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 7,
            resource: buffers[5].as_entire_binding(),
        },
    ]);
    let draw_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &draw_layout,
        entries: &draw_entries,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&Default::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32 * 32 * 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &view_group, &[]);
        pass.set_bind_group(1, &draw_group, &[]);
        pass.draw(0..4, 0..16);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: Some(32),
            },
        },
        wgpu::Extent3d {
            width: 32,
            height: 32,
            depth_or_array_layers: 1,
        },
    );
    map(device, queue, encoder, &readback)
}

fn assert_projection_math(device: &wgpu::Device, queue: &wgpu::Queue) {
    let source = format!(
        "{}\n{}",
        include_str!("../../../crates/aestra-gpu/src/shaders/aestra_sprite_sampling.wesl"),
        r#"
        @group(0) @binding(0) var<storage, read_write> results: array<vec4<f32>>;
        @compute @workgroup_size(1) fn check() {
            let widths = array<f32, 8>(0.25, 0.5, 1.0, 2.0, 4.0, 0.0, 0.25, 0.00000001);
            for (var i = 0u; i < 8u; i++) {
                let floor = select(2.0, 0.0, i == 6u);
                results[i] = vec4<f32>(aestra_sprite_sampling(widths[i], floor), 0.0, 0.0);
            }
            let identity = mat4x4<f32>(vec4<f32>(1,0,0,0),vec4<f32>(0,1,0,0),vec4<f32>(0,0,1,0),vec4<f32>(0,0,0,1));
            let x = vec3<f32>(0.02,0,0); let y = vec3<f32>(0,0.01,0);
            results[8].x = aestra_sprite_projected_pixels(identity, vec4<f32>(0,0,0,1), x, y, vec2<f32>(100,200));
            results[9].x = aestra_sprite_projected_pixels(identity, vec4<f32>(0,0,0,1), x, y, vec2<f32>(50,100));
            let perspective = mat4x4<f32>(vec4<f32>(2,0,0,0),vec4<f32>(0,2,0,0),vec4<f32>(0,0,0,-1),vec4<f32>(0,0,0.1,0));
            let px = vec3<f32>(0.1,0,0); let py = vec3<f32>(0,0.1,0);
            results[10].x = aestra_sprite_projected_pixels(perspective, vec4<f32>(0,0,-10,1), px, py, vec2<f32>(100));
            results[11].x = aestra_sprite_projected_pixels(perspective, vec4<f32>(0,0,-20,1), px, py, vec2<f32>(100));
            results[12].x = aestra_sprite_projected_pixels(perspective, vec4<f32>(0,0,10,1), px, py, vec2<f32>(100));
        }
    "#
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
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
        size: 13 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 13 * 16,
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
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 13 * 16);
    let bytes = map(device, queue, encoder, &readback);
    let results = bytes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|value| {
            let words: &[[u8; 4]; 4] = value.as_chunks::<4>().0.try_into().unwrap();
            words.map(f32::from_le_bytes)
        })
        .collect::<Vec<_>>();
    for (actual, expected) in results.iter().zip([
        [8.0, 0.015625],
        [4.0, 0.0625],
        [2.0, 0.25],
        [1.0, 1.0],
        [1.0, 1.0],
        [1.0, 1.0],
        [1.0, 1.0],
        [1.0, 1.0],
    ]) {
        assert_eq!(actual[..2], expected);
        assert_eq!(
            actual[0] * actual[0] * actual[1],
            1.0,
            "continuous area times coverage must be invariant"
        );
    }
    for (actual, expected) in results[8..].iter().zip([1.0, 0.5, 1.0, 0.5, 0.0]) {
        assert!((actual[0] - expected).abs() < 1e-6);
    }
}
