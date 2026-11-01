//! Native launch-layout parity and compute-only cost for the production sampler.
use aestra_core::{
    EmitterShape, VelocityDistribution,
    particle_sampling::{ParticleSampling, SamplingMode},
};
use aestra_runtime::particle_sampling::{SamplingCohort, direction, position};
use std::{borrow::Cow, sync::mpsc, time::Duration};
use wgpu::*;
#[test]
#[ignore = "native particle layout parity/cost oracle; run alone on a GPU"]
fn native_particle_layouts_match_cpu_and_report_launch_cost() {
    const COUNT: u32 = 4096;
    const REPEATS: u32 = 8;
    let instance = Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
        power_preference: PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .unwrap();
    let timestamps = adapter.features().contains(Features::TIMESTAMP_QUERY);
    let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
        required_features: if timestamps {
            Features::TIMESTAMP_QUERY
        } else {
            Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let source = format!(
        "{}\n@group(0) @binding(0) var<storage,read> params:array<u32>;\n@group(0) @binding(1) var<storage,read_write> out:array<vec4<f32>>;\n@compute @workgroup_size(64) fn verify(@builtin(global_invocation_id) gid:vec3<u32>){{let i=gid.x;let ordinal=0xffffe000u+i;let config=vec4<u32>(params[204],params[205],params[206],params[207]);let cohort=aestra_sampling_sequence(ordinal,config.z);out[i*2u]=vec4<f32>(aestra_sampled_direction(config,params[208],42u,cohort,1u+i%6u,vec3<f32>(1.0,2.0,3.0),60.0),1.0);out[i*2u+1u]=vec4<f32>(aestra_sampled_position(config,params[208],42u,cohort,i%8u,2.0,3.0,4.0),1.0);}}",
        aestra_gpu::STATEFUL_SPAWN_RNG_WGSL
    );
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("production particle layouts"),
        source: ShaderSource::Wgsl(Cow::Owned(source)),
    });
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("verify"),
        compilation_options: Default::default(),
        cache: None,
    });
    let buffer = |size, usage| {
        device.create_buffer(&BufferDescriptor {
            label: None,
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let params = buffer(
        aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS as u64 * 4,
        BufferUsages::STORAGE | BufferUsages::COPY_DST,
    );
    let output = buffer(
        u64::from(COUNT) * 32,
        BufferUsages::STORAGE | BufferUsages::COPY_SRC,
    );
    let readback = buffer(
        u64::from(COUNT) * 32,
        BufferUsages::MAP_READ | BufferUsages::COPY_DST,
    );
    let query = timestamps.then(|| {
        device.create_query_set(&QuerySetDescriptor {
            label: None,
            ty: QueryType::Timestamp,
            count: REPEATS * 2,
        })
    });
    let resolve = timestamps.then(|| {
        buffer(
            u64::from(REPEATS) * 16,
            BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
        )
    });
    let times = timestamps.then(|| {
        buffer(
            u64::from(REPEATS) * 16,
            BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        )
    });
    let group = device.create_bind_group(&BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut cases = Vec::new();
    for mode in [SamplingMode::Uniform, SamplingMode::Stratified] {
        for count in [1, 2, 3, 7, 64, 800, 65537] {
            for jitter in [0.0, 1.0] {
                let settings = ParticleSampling {
                    positions: mode,
                    directions: mode,
                    sequence_count: count,
                    jitter,
                    seed: u32::MAX,
                };
                let mut words = vec![0u32; aestra_gpu::STATEFUL_SIMULATION_PARAM_WORDS];
                aestra_gpu::particle_sampling::pack(Some(settings), &mut words);
                queue.write_buffer(
                    &params,
                    0,
                    &words
                        .into_iter()
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                );
                // Warm the exact pipeline and bindings before sampling GPU timestamps.
                let mut warm = device.create_command_encoder(&Default::default());
                {
                    let mut p = warm.begin_compute_pass(&Default::default());
                    p.set_pipeline(&pipeline);
                    p.set_bind_group(0, &group, &[]);
                    p.dispatch_workgroups(COUNT / 64, 1, 1);
                }
                queue.submit([warm.finish()]);
                device
                    .poll(PollType::Wait {
                        submission_index: None,
                        timeout: Some(Duration::from_secs(30)),
                    })
                    .unwrap();
                let mut encoder = device.create_command_encoder(&Default::default());
                for repeat in 0..REPEATS {
                    let mut p = encoder.begin_compute_pass(&ComputePassDescriptor {
                        label: None,
                        timestamp_writes: query.as_ref().map(|query_set| {
                            ComputePassTimestampWrites {
                                query_set,
                                beginning_of_pass_write_index: Some(repeat * 2),
                                end_of_pass_write_index: Some(repeat * 2 + 1),
                            }
                        }),
                    });
                    p.set_pipeline(&pipeline);
                    p.set_bind_group(0, &group, &[]);
                    p.dispatch_workgroups(COUNT / 64, 1, 1);
                }
                encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, u64::from(COUNT) * 32);
                if let (Some(q), Some(r), Some(t)) = (&query, &resolve, &times) {
                    encoder.resolve_query_set(q, 0..REPEATS * 2, r, 0);
                    encoder.copy_buffer_to_buffer(r, 0, t, 0, u64::from(REPEATS) * 16);
                }
                let submission = queue.submit([encoder.finish()]);
                let (tx, rx) = mpsc::channel();
                readback
                    .slice(..)
                    .map_async(MapMode::Read, move |r| tx.send(r).unwrap());
                let (ttx, trx) = mpsc::channel();
                if let Some(t) = &times {
                    t.slice(..)
                        .map_async(MapMode::Read, move |r| ttx.send(r).unwrap());
                }
                device
                    .poll(PollType::Wait {
                        submission_index: Some(submission),
                        timeout: Some(Duration::from_secs(30)),
                    })
                    .unwrap();
                rx.recv().unwrap().unwrap();
                let mapped = readback.slice(..).get_mapped_range().unwrap();
                let mut max_error = 0.0f32;
                for (i, bytes) in mapped.as_chunks::<32>().0.iter().enumerate() {
                    let cohort = SamplingCohort::sequence(0xffffe000u32 + i as u32, count);
                    let d = direction(
                        settings,
                        42,
                        cohort,
                        VelocityDistribution::ALL[1 + i % 6],
                        [1.0, 2.0, 3.0],
                        60.0,
                    );
                    let shape = match i % 8 {
                        0 => EmitterShape::Point,
                        1 => EmitterShape::Circle { radius: 2.0 },
                        2 => EmitterShape::Ring { radius: 2.0 },
                        3 => EmitterShape::Sphere { radius: 2.0 },
                        4 => EmitterShape::Hemisphere { radius: 2.0 },
                        5 => EmitterShape::Box {
                            half_extents: [2.0, 3.0, 4.0],
                        },
                        6 => EmitterShape::Cylinder {
                            radius: 2.0,
                            depth: 3.0,
                        },
                        _ => EmitterShape::Cone {
                            radius: 2.0,
                            depth: 3.0,
                        },
                    };
                    let p = position(settings, 42, cohort, shape);
                    for (offset, values) in [(0, d), (16, p)] {
                        for (axis, expected) in values.into_iter().enumerate() {
                            let at = offset + axis * 4;
                            let actual = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                            let error = (actual - expected).abs();
                            max_error = max_error.max(error);
                            assert!(
                                actual.is_finite() && error < 0.0001,
                                "{mode:?} count={count} jitter={jitter} sample={i} axis={axis}: {actual} != {expected}"
                            );
                        }
                    }
                }
                drop(mapped);
                readback.unmap();
                let gpu_ms = times.as_ref().map(|t| {
                    trx.recv().unwrap().unwrap();
                    let bytes = t.slice(..).get_mapped_range().unwrap();
                    let mut values = bytes
                        .as_chunks::<16>()
                        .0
                        .iter()
                        .map(|b| {
                            let a = u64::from_le_bytes(b[..8].try_into().unwrap());
                            let z = u64::from_le_bytes(b[8..].try_into().unwrap());
                            (z - a) as f64 * f64::from(queue.get_timestamp_period()) / 1e6
                        })
                        .collect::<Vec<_>>();
                    values.sort_by(f64::total_cmp);
                    let median = values[values.len() / 2];
                    drop(bytes);
                    t.unmap();
                    median
                });
                cases.push(serde_json::json!({"mode":format!("{mode:?}"),"count":count,"jitter":jitter,"launches":COUNT,"max_abs_error":max_error,"median_gpu_dispatch_ms":gpu_ms,"gpu_ns_per_launch_pair":gpu_ms.map(|ms|ms*1e6/f64::from(COUNT))}));
            }
        }
    }
    let report = serde_json::json!({"adapter":format!("{:?}",adapter.get_info()),"repeats_after_warmup":REPEATS,"tolerance":0.0001,"cases":cases,"scope":"Production position and direction functions, six velocity modes and all eight shapes. Compute-only timestamps exclude admission/integration/uploads/rendering. Not a full-frame benchmark."});
    if let Some(path) = std::env::var_os("AESTRA_SAMPLING_REPORT") {
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
}
