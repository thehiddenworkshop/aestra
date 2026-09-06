//! Preparation-only experiment; deliberately blocking, never part of runtime profiling.
use super::{CaseReport, Report, Timing, encode, group};
use crate::Config;
use aestra_gpu::{GpuParticle, GpuRenderGlobals, GpuTrailCullParams, shader};
use glam::{Mat4, UVec4, Vec3, Vec4};
use wgpu::util::DeviceExt;

pub(super) fn run(config: &Config) -> Report {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
    let instance = wgpu::Instance::new(descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("native GPU required");
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TIMESTAMP_QUERY,
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("timestamp queries required");
    println!("adapter={:?}", adapter.get_info());
    let mut report = Report::new("trail-preparation", config, adapter.get_info());
    let period = f64::from(queue.get_timestamp_period());
    let compact_source = shader::compile_wesl(
        "package::compact",
        &shader::trail_compact_wesl(),
        &["classify_trail", "prefix_trail", "scatter_trail"],
    )
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(compact_source.wgsl.into()),
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
    let passes = ["classify_trail", "prefix_trail", "scatter_trail"].map(|entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    });
    let cull_source =
        shader::compile_wesl("package::cull", &shader::trail_cull_wesl(), &["cull_trail"]).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(cull_source.wgsl.into()),
    });
    let cull = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("cull_trail"),
        compilation_options: Default::default(),
        cache: None,
    });
    let make_buffer = |bytes: Vec<u8>, uniform| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &bytes,
            usage: (if uniform {
                wgpu::BufferUsages::UNIFORM
            } else {
                wgpu::BufferUsages::STORAGE
            }) | wgpu::BufferUsages::COPY_SRC,
        })
    };
    let mut asset = aestra_core::EffectAsset::new("Benchmark", 3.0);
    asset
        .emitters
        .push(aestra_core::Emitter::basic_sprite("Trail", 3.0));
    let compiled = aestra_compiler::EffectCompiler::default()
        .compile(&asset)
        .unwrap();
    let effect = aestra_runtime::EffectInstance::new(std::sync::Arc::new(compiled));
    let mut renderer = aestra_gpu::GpuEffectArtifact::from_instance(&effect)
        .unwrap()
        .renderers[0];
    const OWNERS: u32 = 1024;
    const POINTS: u32 = 64;
    const CANDIDATES: u32 = OWNERS * (POINTS - 1);
    renderer.renderer_kind = 4;
    renderer.frame_count = POINTS;
    renderer.frame_rate = 1.0;
    renderer.playback_mode = OWNERS;
    renderer.flipbook_flags = 0;
    renderer.attribute_flags.z = 0;
    renderer.attribute_flags.y = 1.0f32.to_bits();
    let renderers = make_buffer(encode(&vec![renderer]), false);
    let globals = make_buffer(
        encode(&GpuRenderGlobals {
            time: 1.0,
            seed: config.seed as u32,
            ..Default::default()
        }),
        false,
    );
    let params = make_buffer(encode(&UVec4::new(0, CANDIDATES, OWNERS, POINTS - 1)), true);
    let cull_params = make_buffer(
        encode(&GpuTrailCullParams {
            clip_from_world: Mat4::IDENTITY,
            renderer_index: 0,
            instance_count: CANDIDATES,
            epoch: 1,
            _padding: 0,
        }),
        true,
    );
    let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 4,
    });
    let resolve = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 32,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 48,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    println!(
        "case,views,candidates,submitted,compaction_median_ns,compaction_p95_ns,culling_median_ns,culling_p95_ns"
    );
    for (name, active) in [("sparse", 16), ("dense", OWNERS)] {
        let mut records = vec![GpuParticle::default(); (1 + OWNERS * POINTS) as usize];
        records[0] = GpuParticle {
            packed_emitter_alive: 1,
            rotation: 1.0,
            particle_index: config.seed as u32,
            size: 1.0,
            position: Vec3::splat(-0.5),
            color: Vec4::splat(0.5),
            ..Default::default()
        };
        let mut words = vec![0u32; records.len() * 3];
        words[0] = 1;
        for owner in 0..active as usize {
            let base = 1 + owner * POINTS as usize;
            records[base] = GpuParticle {
                packed_emitter_alive: 1,
                rotation: 1.0,
                position: Vec3::X,
                ..Default::default()
            };
            words[base * 3 + 1] = POINTS - 1;
            for point in 1..POINTS as usize {
                records[base + point] = GpuParticle {
                    position: Vec3::X * ((point - 1) as f32 / POINTS as f32),
                    rotation: 0.9,
                    ..Default::default()
                };
            }
        }
        let particles = make_buffer(encode(&records), false);
        let aux = make_buffer(encode(&words), false);
        let scratch = make_buffer(encode(&vec![0u32; (CANDIDATES + OWNERS) as usize]), false);
        let output = make_buffer(encode(&vec![0u32; (CANDIDATES + 4) as usize]), false);
        let compact_group = group(
            &device,
            &layout,
            &[
                &particles, &renderers, &globals, &aux, &params, &scratch, &output,
            ],
        );
        let draws: Vec<_> = (0..4)
            .map(|_| make_buffer(encode(&vec![0u32; 4]), false))
            .collect();
        let cull_groups: Vec<_> = draws
            .iter()
            .map(|draw| {
                group(
                    &device,
                    &cull.get_bind_group_layout(0),
                    &[
                        &particles,
                        &renderers,
                        &cull_params,
                        draw,
                        &globals,
                        &aux,
                        &output,
                    ],
                )
            })
            .collect();
        for views in [1, 4] {
            let mut compact_times = Vec::new();
            let mut cull_times = Vec::new();
            for frame in 0..config.warmup + config.frames {
                let mut encoder = device.create_command_encoder(&Default::default());
                for (stage, pipeline) in passes.iter().enumerate() {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: None,
                        timestamp_writes: (stage != 1).then_some(
                            wgpu::ComputePassTimestampWrites {
                                query_set: &queries,
                                beginning_of_pass_write_index: (stage == 0).then_some(0),
                                end_of_pass_write_index: (stage == 2).then_some(1),
                            },
                        ),
                    });
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &compact_group, &[]);
                    pass.dispatch_workgroups(
                        match stage {
                            0 => OWNERS.div_ceil(64),
                            1 => 1,
                            _ => CANDIDATES.div_ceil(64),
                        },
                        1,
                        1,
                    );
                }
                {
                    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        label: None,
                        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                            query_set: &queries,
                            beginning_of_pass_write_index: Some(2),
                            end_of_pass_write_index: Some(3),
                        }),
                    });
                    pass.set_pipeline(&cull);
                    for group in cull_groups.iter().take(views) {
                        pass.set_bind_group(0, group, &[]);
                        pass.dispatch_workgroups(1, 1, 1);
                    }
                }
                encoder.copy_buffer_to_buffer(&output, 0, &readback, 32, 16);
                let submission = queue.submit(super::timestamps::finish(
                    &device,
                    encoder,
                    &queries,
                    0..4,
                    &resolve,
                    &readback,
                ));
                let (tx, rx) = std::sync::mpsc::channel();
                readback
                    .slice(..)
                    .map_async(wgpu::MapMode::Read, move |result| {
                        let _ = tx.send(result);
                    });
                device
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(std::time::Duration::from_secs(60)),
                    })
                    .unwrap();
                rx.recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
                let bytes = readback.slice(..).get_mapped_range();
                let ticks: Vec<_> = bytes[..32]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|b| u64::from_le_bytes(*b))
                    .collect();
                assert_eq!(
                    u32::from_le_bytes(bytes[36..40].try_into().unwrap()),
                    active * (POINTS - 1)
                );
                assert!(
                    super::timestamps::valid_sequence(&ticks),
                    "invalid GPU timestamps: {ticks:?}"
                );
                if frame >= config.warmup {
                    compact_times.push(
                        (ticks[1].checked_sub(ticks[0]).unwrap() as f64 * period).round() as u64,
                    );
                    cull_times.push(
                        (ticks[3].checked_sub(ticks[2]).unwrap() as f64 * period).round() as u64,
                    );
                }
                drop(bytes);
                readback.unmap();
            }
            let compaction = Timing::new(compact_times);
            let culling = Timing::new(cull_times);
            println!(
                "{name},{views},{CANDIDATES},{},{},{},{},{}",
                active * (POINTS - 1),
                compaction.median_ns,
                compaction.p95_ns,
                culling.median_ns,
                culling.p95_ns
            );
            report.cases.push(CaseReport {
                case: name.into(),
                views,
                path: "compact".into(),
                candidates_per_view: CANDIDATES,
                submitted_per_view: active * (POINTS - 1),
                compaction,
                culling,
                total: None,
                drawing: None,
                image_equivalence: None,
            });
        }
    }
    report
}
