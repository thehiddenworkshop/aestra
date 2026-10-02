//! Actual indirect visibility decisions, without any CPU bounds readback.
use aestra_gpu::{GpuParticle, GpuRenderGlobals, GpuRenderer, GpuTrailCullParams, shader};
use bevy::math::{Mat4, Vec3, Vec4};
use encase::ShaderType;
use wgpu::util::DeviceExt;

fn encode<T: ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut buffer = encase::StorageBuffer::new(Vec::new());
    buffer.write(value).unwrap();
    buffer.into_inner()
}

#[test]
fn gpu_trail_culling_is_conservative_current_and_specific_to_each_camera() {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
        eprintln!("Skipping trail culling: no adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let code = shader::compile_wesl(
        "package::trail_cull",
        &shader::trail_cull_wesl(),
        &["cull_trail"],
    )
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(code.wgsl.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("cull_trail"),
        compilation_options: Default::default(),
        cache: None,
    });
    let mut effect = aestra_core::EffectAsset::new("Trail culling", 2.0);
    effect
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Trail", 2.0));
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&effect)
        .unwrap();
    let instance = aestra_runtime::EffectInstance::new(std::sync::Arc::new(compiled));
    let mut renderer = aestra_gpu::GpuEffectArtifact::from_instance(&instance)
        .unwrap()
        .renderers[0];
    renderer.renderer_kind = 4;
    renderer.frames[63].w = 0.0;
    renderer.attribute_flags.z = 0;
    renderer.attribute_flags.y = 1.0f32.to_bits();
    let globals = GpuRenderGlobals {
        time: 1.0,
        seed: 7,
        ..Default::default()
    };
    let params = GpuTrailCullParams {
        clip_from_world: Mat4::IDENTITY,
        renderer_index: 0,
        instance_count: 79,
        epoch: 9,
        pixel_radius_per_clip_w: 0.0,
    };
    let header = GpuParticle {
        packed_emitter_alive: 1,
        particle_index: 7,
        rotation: 1.0,
        position: Vec3::new(-0.25, -0.25, 0.5),
        color: Vec4::new(0.25, 0.25, 0.5, 1.0),
        size: 1.0,
        ..Default::default()
    };
    let buffers = [
        encode(&vec![header]),
        encode(&vec![renderer]),
        encode(&params),
        encode(&vec![0u32; 4]),
        encode(&globals),
        encode(&vec![9u32, 0, 0]),
        encode(&vec![4u32, 79, 0, 0]),
    ]
    .into_iter()
    .enumerate()
    .map(|(binding, data)| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &data,
            usage: (if binding == 2 {
                wgpu::BufferUsages::UNIFORM
            } else {
                wgpu::BufferUsages::STORAGE
            }) | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
        })
    })
    .collect::<Vec<_>>();
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    });
    let run = |header: GpuParticle, params: GpuTrailCullParams, renderer: GpuRenderer| {
        queue.write_buffer(&buffers[0], 0, &encode(&vec![header]));
        queue.write_buffer(&buffers[1], 0, &encode(&vec![renderer]));
        queue.write_buffer(&buffers[2], 0, &encode(&params));
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
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
        encoder.copy_buffer_to_buffer(&buffers[3], 0, &readback, 0, 16);
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
        let words: Vec<_> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect();
        assert_eq!([words[0], words[2], words[3]], [4, 0, 0]);
        words[1]
    };
    assert_eq!(run(header, params, renderer), 79);
    let moved = |position: Vec3| GpuParticle {
        position,
        color: position.extend(1.0),
        ..header
    };
    for position in [
        Vec3::new(3.0, 0.0, 0.5),
        Vec3::new(-3.0, 0.0, 0.5),
        Vec3::new(0.0, 3.0, 0.5),
        Vec3::new(0.0, -3.0, 0.5),
        Vec3::new(0.0, 0.0, 3.0),
        Vec3::new(0.0, 0.0, -3.0),
    ] {
        assert_eq!(
            run(moved(position), params, renderer),
            0,
            "outside any frustum plane"
        );
    }
    let outside = moved(Vec3::new(3.0, 0.0, 0.5));
    assert_eq!(
        run(
            outside,
            GpuTrailCullParams {
                clip_from_world: Mat4::from_translation(Vec3::new(-3.0, 0.0, 0.0)),
                ..params
            },
            renderer
        ),
        79,
        "another camera sees the same world history"
    );
    assert_eq!(run(outside, params, renderer), 0);
    let mut sampled = renderer;
    sampled.blend_mode = aestra_gpu::GpuBlend::Additive as u32;
    sampled.frames[63].w = 2.0;
    assert_eq!(
        run(outside, params, sampled),
        79,
        "unsupported views still bypass spatial culling"
    );
    let sampled_params = GpuTrailCullParams {
        pixel_radius_per_clip_w: 1.0001 / 540.0,
        ..params
    };
    assert_eq!(
        run(outside, sampled_params, sampled),
        0,
        "supported view rejects distant sampled history"
    );
    let mut thin = sampled;
    thin.attribute_flags.y = 0.0001_f32.to_bits();
    for floor in [2.0, 4.0, 8.0] {
        thin.frames[63].w = floor;
        for edge in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y] {
            let near = moved(edge * 1.0005 + Vec3::Z * 0.5);
            let far = moved(edge * 1.2 + Vec3::Z * 0.5);
            for flags in [0, 2] {
                thin.flipbook_flags = flags;
                assert_eq!(run(near, params, thin), 79);
                assert_eq!(
                    run(near, sampled_params, thin),
                    79,
                    "expanded body/cap at every edge stays visible"
                );
                assert_eq!(
                    run(far, sampled_params, thin),
                    0,
                    "wholly outside expanded radius"
                );
            }
        }
    }
    thin.frames[63].w = 4.0;
    let near = moved(Vec3::new(1.02, 0.0, 0.5));
    assert_eq!(run(near, sampled_params, thin), 0);
    assert_eq!(
        run(
            near,
            GpuTrailCullParams {
                pixel_radius_per_clip_w: 1.0001 / 54.0,
                ..params
            },
            thin
        ),
        79,
        "lower physical render resolution widens the same floor"
    );
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(
            run(
                outside,
                GpuTrailCullParams {
                    pixel_radius_per_clip_w: scale,
                    ..params
                },
                sampled
            ),
            79,
            "unsupported or invalid pixel scale fails open"
        );
    }
    assert_eq!(
        run(
            GpuParticle {
                size: 2.0,
                ..outside
            },
            params,
            sampled
        ),
        0,
        "empty histories remain rejected"
    );
    sampled.blend_mode = aestra_gpu::GpuBlend::Alpha as u32;
    assert_eq!(
        run(outside, params, sampled),
        0,
        "excluded blend modes keep authored-width culling"
    );
    queue.write_buffer(&buffers[6], 0, &encode(&vec![4u32, 17, 0, 0]));
    assert_eq!(
        run(header, params, renderer),
        17,
        "preserve compacted count"
    );
    assert_eq!(run(outside, params, renderer), 0, "cull compacted geometry");
    queue.write_buffer(&buffers[6], 0, &encode(&vec![4u32, 0, 0, 0]));
    assert_eq!(run(header, params, renderer), 0, "empty compacted history");
    queue.write_buffer(&buffers[6], 0, &encode(&vec![4u32, 79, 0, 0]));
    assert_eq!(
        run(header, params, renderer),
        79,
        "returning onscreen overwrites the zero draw"
    );
    let mut widened = renderer;
    widened.attribute_flags.y = 8.0f32.to_bits();
    assert_eq!(
        run(outside, params, widened),
        79,
        "a live width edit expands old history bounds"
    );
    for flags in [0, 1, 2, 3] {
        let mut r = renderer;
        r.flipbook_flags = flags;
        assert_eq!(
            run(moved(Vec3::new(1.49, 0.0, 0.5)), params, r),
            79,
            "width/caps enter the viewport"
        );
        assert_eq!(run(moved(Vec3::new(1.51, 0.0, 0.5)), params, r), 0);
    }
    let historical = GpuParticle {
        position: Vec3::new(0.0, 0.0, 0.5),
        color: Vec4::new(100.0, 0.0, 0.5, 1.0),
        ..header
    };
    assert_eq!(
        run(historical, params, renderer),
        79,
        "retired tail can be onscreen while emitter is far away"
    );
    assert_eq!(
        run(
            GpuParticle {
                size: 2.0,
                ..header
            },
            params,
            renderer
        ),
        0,
        "expired history is empty"
    );
    for unknown in [
        GpuParticle {
            size: 0.0,
            ..outside
        },
        GpuParticle {
            packed_emitter_alive: 0,
            ..outside
        },
        GpuParticle {
            rotation: 0.5,
            ..outside
        },
        GpuParticle {
            particle_index: 8,
            ..outside
        },
        GpuParticle {
            position: Vec3::splat(f32::NAN),
            ..outside
        },
        GpuParticle {
            color: Vec4::splat(f32::INFINITY),
            ..outside
        },
    ] {
        assert_eq!(
            run(unknown, params, renderer),
            79,
            "unknown/stale bounds cannot cull"
        );
    }
    assert_eq!(
        run(
            outside,
            GpuTrailCullParams {
                epoch: 10,
                ..params
            },
            renderer
        ),
        79,
        "seek invalidates stale bounds"
    );
    let mut invalid_width = renderer;
    invalid_width.attribute_flags.y = f32::INFINITY.to_bits();
    assert_eq!(run(outside, params, invalid_width), 79);
    let invalid_camera = Mat4::from_cols_array(&[f32::NAN; 16]);
    assert_eq!(
        run(
            outside,
            GpuTrailCullParams {
                clip_from_world: invalid_camera,
                ..params
            },
            renderer
        ),
        79
    );
    sampled.blend_mode = aestra_gpu::GpuBlend::Additive as u32;
    let perspective = GpuTrailCullParams {
        clip_from_world: Mat4::perspective_infinite_reverse_rh(1.0, 1.0, 0.1),
        ..params
    };
    assert_eq!(
        run(moved(Vec3::new(0.0, 0.0, -3.0)), perspective, renderer),
        79
    );
    assert_eq!(
        run(moved(Vec3::new(30.0, 0.0, -3.0)), perspective, renderer),
        0
    );
    let perspective = GpuTrailCullParams {
        pixel_radius_per_clip_w: 1.0001 / (perspective.clip_from_world.y_axis.y * 540.0),
        ..perspective
    };
    assert_eq!(
        run(moved(Vec3::new(30.0, 0.0, -3.0)), perspective, sampled),
        0
    );
    let crosses_eye = GpuParticle {
        position: Vec3::new(30.0, 0.0, -3.0),
        color: Vec4::new(31.0, 0.0, 3.0, 1.0),
        ..header
    };
    assert_eq!(
        run(crosses_eye, perspective, sampled),
        79,
        "eye-plane crossing fails open"
    );
    assert_eq!(
        run(
            outside,
            GpuTrailCullParams {
                epoch: 10,
                ..sampled_params
            },
            sampled
        ),
        79,
        "sampled views cannot trust stale epoch bounds either"
    );
    for projection in [
        Mat4::perspective_infinite_reverse_rh(1.0, 960.0 / 540.0, 0.1),
        Mat4::orthographic_rh(-16.0, 16.0, -9.0, 9.0, 0.1, 1000.0),
    ] {
        for viewport in [
            bevy::math::Vec2::new(960.0, 540.0),
            bevy::math::Vec2::new(96.0, 54.0),
        ] {
            let pose = Mat4::from_scale_rotation_translation(
                Vec3::ONE,
                bevy::math::Quat::from_euler(bevy::math::EulerRot::XYZ, 0.4, 1.3, 0.2),
                Vec3::new(12.0, 3.0, -4.0),
            );
            let view_params = GpuTrailCullParams {
                clip_from_world: projection * pose.inverse(),
                pixel_radius_per_clip_w: 1.0001
                    / (bevy::math::Vec2::new(projection.x_axis.x.abs(), projection.y_axis.y.abs())
                        * viewport)
                        .min_element(),
                ..params
            };
            let depth = -10.0;
            let clip = projection * Vec3::new(0.0, 0.0, depth).extend(1.0);
            let position = |ndc: bevy::math::Vec2| {
                pose.transform_point3(Vec3::new(
                    (ndc.x * clip.w - clip.x) / projection.x_axis.x,
                    (ndc.y * clip.w - clip.y) / projection.y_axis.y,
                    depth,
                ))
            };
            for floor in [2.0, 4.0, 8.0] {
                thin.frames[63].w = floor;
                for edge in [
                    bevy::math::Vec2::X,
                    -bevy::math::Vec2::X,
                    bevy::math::Vec2::Y,
                    -bevy::math::Vec2::Y,
                ] {
                    assert_eq!(
                        run(
                            moved(position(
                                edge * (bevy::math::Vec2::ONE
                                    + bevy::math::Vec2::splat(0.5) / viewport)
                            )),
                            view_params,
                            thin
                        ),
                        79,
                        "rotated camera retains widened edge"
                    );
                    assert_eq!(
                        run(moved(position(edge * 1.2)), view_params, thin),
                        0,
                        "rotated camera rejects offscreen history"
                    );
                }
            }
        }
    }
}
