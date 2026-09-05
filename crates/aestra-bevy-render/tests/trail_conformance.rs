//! Persistent history exercised on a native adapter using the production buffer ABI.
use aestra_gpu::{
    GpuEmitter, GpuGlobals, GpuParticle,
    shader::{SIMULATION_WESL, compile_wesl},
};
use bevy::math::{Mat4, UVec2, Vec3, Vec4};
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use wgpu::util::DeviceExt;

#[path = "../src/gpu/trail_replay.rs"]
mod trail_replay;

fn encode<T: ShaderType + WriteInto>(value: &T) -> Vec<u8> {
    let mut bytes = Vec::new();
    StorageBuffer::new(&mut bytes).write(value).unwrap();
    bytes
}

fn word(bytes: &[u8], record: usize, offset: usize) -> u32 {
    let at = record * 64 + offset;
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}

// The record is now a packed 48-byte struct (emitter_index and alive share one word)
// with ring state living in the separate aux buffer. Reconstruct the historical
// 64-byte layout (emitter_index@40, alive@44, particle_index@48, ring state@52/56/60)
// from the packed records + aux, so the offset-based `word` assertions keep working.
// `counters` (if any) is appended unchanged so stats_record reads still land.
fn expand_legacy(packed: &[u8], aux: &[u8], counters: &[u8], records: usize) -> Vec<u8> {
    let mut out = vec![0u8; records * 64 + counters.len()];
    for r in 0..records {
        let src = &packed[r * 48..r * 48 + 48];
        // color/position/size/rotation/normalized_age share the first 40 bytes.
        out[r * 64..r * 64 + 40].copy_from_slice(&src[0..40]);
        let packed_word = u32::from_le_bytes(src[40..44].try_into().unwrap());
        let emitter = packed_word >> 16;
        let alive = packed_word & 0xffff;
        out[r * 64 + 40..r * 64 + 44].copy_from_slice(&emitter.to_le_bytes());
        out[r * 64 + 44..r * 64 + 48].copy_from_slice(&alive.to_le_bytes());
        out[r * 64 + 48..r * 64 + 52].copy_from_slice(&src[44..48]); // particle_index
        for k in 0..3 {
            let a = r * 12 + k * 4;
            out[r * 64 + 52 + k * 4..r * 64 + 56 + k * 4].copy_from_slice(&aux[a..a + 4]);
        }
    }
    out[records * 64..].copy_from_slice(counters);
    out
}

// Exercise the real simulation as well as history: a direct jump has live
// particles but only coincident head/anchor pairs, which cannot draw a trail.
fn check_seek_replay(device: &wgpu::Device, queue: &wgpu::Queue) {
    let effect = aestra_core::EffectAsset::from_ron(include_str!(
        "../../../assets/effects/trail_lab.aestra.ron"
    ))
    .unwrap();
    let program = aestra_core::material::MaterialProgram::from_ron(include_str!(
        "../../../assets/materials/trail_lab.aestra.material.ron"
    ))
    .unwrap();
    let effect = aestra_compiler::EffectCompiler::default()
        .compile_with_material_programs(
            &effect,
            &std::collections::BTreeMap::from([(program.id, program)]),
        )
        .unwrap();
    let instance = aestra_runtime::EffectInstance::new(std::sync::Arc::new(effect));
    let artifact = aestra_gpu::GpuEffectArtifact::from_instance(&instance).unwrap();
    let e = artifact.emitters[0];
    let globals = GpuGlobals {
        time: 86.0 / 60.0,
        total_slots: artifact.total_slots,
        emitter_count: 1,
        seed: aestra_gpu::fold_seed(instance.seed()),
        duration: 3.0,
        continuous: 1,
        world_from_effect: Mat4::IDENTITY,
        ..Default::default()
    };
    let data = [
        encode(&artifact.emitters),
        encode(&artifact.particles),
        encode(&vec![0u32; artifact.total_slots as usize]),
        encode(&vec![0u32; artifact.total_slots as usize]),
        encode(&vec![0u32; 8]),
        encode(&aestra_gpu::indirect_draw_commands(&artifact.emitters)),
        encode(&globals),
        encode(&vec![0u32; artifact.particles.len() * 3]),
    ];
    let buffers = data
        .iter()
        .map(|bytes| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
        })
        .collect::<Vec<_>>();
    let entries = (0..8)
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
    let entries = ["reset", "simulate", "link_ribbons", "update_trails"];
    let shader = compile_wesl("package::trail_seek", SIMULATION_WESL, &entries).unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.wgsl.into()),
    });
    let pipelines = entries.map(|entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    });
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
    let run = |times: &[f32], epoch| {
        queue.write_buffer(
            &buffers[6],
            0,
            &encode(&GpuGlobals {
                _padding: UVec2::new(epoch, 0),
                ..globals
            }),
        );
        let times_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: &times
                .iter()
                .flat_map(|t| t.to_le_bytes())
                .collect::<Vec<_>>(),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buffers[1].size() + buffers[7].size(),
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        for (i, _) in times.iter().enumerate() {
            encoder.copy_buffer_to_buffer(&times_buffer, i as u64 * 4, &buffers[6], 0, 4);
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_bind_group(0, &group, &[]);
            for pipeline in &pipelines {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&buffers[1], 0, &readback, 0, buffers[1].size());
        encoder.copy_buffer_to_buffer(
            &buffers[7],
            0,
            &readback,
            buffers[1].size(),
            buffers[7].size(),
        );
        let submission = queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = sender.send(r);
        });
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(120)),
            })
            .unwrap();
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let bytes = readback.slice(..).get_mapped_range().to_vec();
        readback.unmap();
        let p = buffers[1].size() as usize;
        expand_legacy(&bytes[0..p], &bytes[p..], &[], p / 48)
    };
    let heads =
        || (0..e.trail_capacity).map(|i| (e.trail_offset + 1 + i * e.trail_points) as usize);
    let direct = run(&[globals.time], 0);
    assert!(
        heads()
            .filter(|&h| word(&direct, h, 44) != 0)
            .all(|h| word(&direct, h, 56) == 1)
    );
    let mut planner = trail_replay::TrailReplay::default();
    let replayed = run(&planner.observations(1, globals.time), 1);
    assert!(
        heads().any(|h| word(&replayed, h, 56) > 20),
        "seek must create drawable history, not only live heads"
    );
    assert_eq!(
        replayed,
        run(&planner.observations(1, globals.time), 1),
        "paused history must be stable"
    );
    let backward = run(&planner.observations(2, 0.75), 2);
    assert!(heads().any(|h| word(&backward, h, 56) > 5));
    let forward = run(&planner.observations(3, globals.time), 3);
    // Header epoch differs; owner/sample data must reproduce the first seek.
    let history_start = (e.trail_offset as usize + 1) * 64;
    assert_eq!(&replayed[history_start..], &forward[history_start..]);
    let beyond_loop = run(&planner.observations(4, 3.5), 4);
    assert!(heads().any(|h| word(&beyond_loop, h, 56) > 20));
}

#[test]
fn trails_preserve_identity_world_history_and_retired_tails_and_reset_on_discontinuities() {
    check_pool(2, false);
}

#[test]
fn separate_trail_budget_retains_burst_tails_until_expiry_or_oldest_retired_eviction() {
    check_pool(4, false);
}

#[test]
fn distance_sampling_handles_stationary_speed_changes_overflow_loops_and_resets() {
    check_pool(4, true);
}

fn check_pool(max_trails: u32, distance: bool) {
    let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    descriptor.backends = wgpu::Backends::PRIMARY;
    let instance = wgpu::Instance::new(descriptor);
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        assert!(
            std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
            "native GPU required"
        );
        eprintln!("Skipping trail conformance: no adapter");
        return;
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    if distance {
        check_seek_replay(&device, &queue);
    }
    let shader = compile_wesl(
        "package::trail_test",
        SIMULATION_WESL,
        &["link_ribbons", "update_trails"],
    )
    .unwrap();
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(shader.wgsl.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &(0..8)
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
            .collect::<Vec<_>>(),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipelines = ["link_ribbons", "update_trails"].map(|entry| {
        device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    });
    let data = [
        encode(&vec![GpuEmitter {
            max_particles: 2,
            _turbulence_padding: 1,
            trail_offset: 2,
            trail_points: 4,
            trail_capacity: max_trails,
            trail_sampling: u32::from(distance),
            trail_distance: 1.0,
            trail_interval: 0.125,
            trail_lifetime: 1.0,
            ..Default::default()
        }]),
        encode(&vec![GpuParticle::default(); 3 + max_trails as usize * 4]),
        encode(&vec![0u32, 1]),
        encode(&vec![0u32; 2]),
        encode(&vec![0u32; 8]),
        encode(&vec![6u32, 2, 0, 0]),
        encode(&GpuGlobals::default()),
        encode(&vec![0u32; (3 + max_trails as usize * 4) * 3]),
    ];
    let buffers = data
        .iter()
        .map(|bytes| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
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
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>(),
    });
    let step_at =
        |time: f32, ids: [u32; 2], count: u32, translation: Vec3, epoch: u32, seed: u32| {
            let parents = ids
                .map(|particle_index| GpuParticle {
                    particle_index,
                    position: Vec3::new(
                        particle_index as f32 + if distance { 0.0 } else { time },
                        0.0,
                        0.0,
                    ),
                    size: 2.0,
                    color: Vec4::ONE,
                    // packed emitter_index (0) << 16 | alive (1).
                    packed_emitter_alive: 1,
                    ..Default::default()
                })
                .to_vec();
            // Upload only the simulation prefix. The history tail is never reinitialized.
            queue.write_buffer(&buffers[1], 0, &encode(&parents));
            queue.write_buffer(&buffers[2], 0, &encode(&vec![0u32, 1]));
            queue.write_buffer(&buffers[5], 0, &encode(&vec![6u32, count, 0, 0]));
            queue.write_buffer(
                &buffers[6],
                0,
                &encode(&GpuGlobals {
                    time,
                    total_slots: 2,
                    emitter_count: 1,
                    seed,
                    duration: 0.25,
                    continuous: 1,
                    _padding: UVec2::new(epoch, 0),
                    world_from_effect: Mat4::from_translation(translation),
                }),
            );
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: buffers[1].size() + buffers[4].size() + buffers[7].size(),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_bind_group(0, &group, &[]);
                for pipeline in &pipelines {
                    pass.set_pipeline(pipeline);
                    pass.dispatch_workgroups(1, 1, 1);
                }
            }
            encoder.copy_buffer_to_buffer(&buffers[1], 0, &readback, 0, buffers[1].size());
            encoder.copy_buffer_to_buffer(
                &buffers[4],
                0,
                &readback,
                buffers[1].size(),
                buffers[4].size(),
            );
            encoder.copy_buffer_to_buffer(
                &buffers[7],
                0,
                &readback,
                buffers[1].size() + buffers[4].size(),
                buffers[7].size(),
            );
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
            let bytes = readback.slice(..).get_mapped_range().to_vec();
            readback.unmap();
            let p = buffers[1].size() as usize;
            let c = buffers[4].size() as usize;
            expand_legacy(&bytes[0..p], &bytes[p + c..], &bytes[p..p + c], p / 48)
        };
    let step = |time, ids, count, translation, epoch, seed| {
        step_at(
            time,
            ids,
            count,
            Vec3::new(translation, 0.0, 0.0),
            epoch,
            seed,
        )
    };
    if distance {
        let initial = step(0.0, [0, 1], 2, 0.0, 0, 0);
        let stats_record = 3 + max_trails as usize * 4;
        let stationary = step(0.25, [0, 1], 2, 0.0, 0, 0);
        assert_eq!(
            word(&stationary, stats_record, 28),
            0,
            "no truncation during warmup"
        );
        assert_eq!(
            word(&stationary, 3, 56),
            1,
            "stationary parents do not spend history points"
        );
        assert_eq!(&initial[4 * 64..7 * 64], &stationary[4 * 64..7 * 64]);
        let slow = step(0.5, [0, 1], 2, 0.4, 0, 0);
        assert_eq!(word(&slow, 3, 56), 1);
        let crossing = step(0.75, [1, 0], 2, 1.2, 0, 0);
        assert_eq!(
            word(&crossing, 3, 48),
            0,
            "stable owner survives slot permutation"
        );
        assert_eq!(word(&crossing, 3, 56), 2);
        assert!((f32::from_bits(word(&crossing, 5, 16)) - 1.0).abs() < 0.0001);
        assert!(
            (f32::from_bits(word(&crossing, 5, 32)) - 0.6875).abs() < 0.0001,
            "sample timestamps interpolate, lifetime remains time-based"
        );
        let fast = step(1.0, [1, 0], 2, 10.2, 0, 0);
        assert_eq!(
            word(&fast, stats_record, 28),
            2,
            "fast movement discards unexpired points"
        );
        assert!(
            (f32::from_bits(word(&fast, 4, 56)) - 10.2).abs() < 0.0001,
            "head path length survives ring wrap"
        );
        for slot in 4..7 {
            assert!(
                (f32::from_bits(word(&fast, slot, 52)) - f32::from_bits(word(&fast, slot, 16)))
                    .abs()
                    < 0.0001,
                "samples retain their birth-anchored distance, not distance from the moving tail"
            );
        }
        assert_eq!(
            word(&fast, 3, 56),
            3,
            "many crossed distances stay ring-bounded"
        );
        let mut xs = (4..7)
            .map(|i| f32::from_bits(word(&fast, i, 16)))
            .collect::<Vec<_>>();
        xs.sort_by(f32::total_cmp);
        for (x, expected) in xs.into_iter().zip([8.0, 9.0, 10.0]) {
            assert!((x - expected).abs() < 0.0001);
        }
        let paused = step(1.0, [1, 0], 2, 10.2, 0, 0);
        assert_eq!(&fast[128..], &paused[128..]);
        let looped = step(1.25, [2, 1], 2, 10.4, 0, 0);
        assert_eq!(
            word(&looped, 3, 48),
            0,
            "retired owner retained through loop births"
        );
        assert_eq!(word(&looped, 7, 48), 1);
        assert_eq!(word(&looped, 11, 48), 2);
        assert_eq!(
            word(&looped, 12, 56),
            0,
            "new loop owner starts its own tile phase"
        );
        assert_eq!(
            word(&looped, 4, 56),
            word(&fast, 4, 56),
            "retired phase stays fixed"
        );
        assert_eq!(
            word(&looped, 11, 56),
            1,
            "new owner starts with zero distance remainder"
        );
        let reset = step(1.5, [2, 1], 2, 40.0, 1, 0);
        assert_eq!(
            word(&reset, stats_record, 28),
            0,
            "seek clears point warnings"
        );
        assert_eq!(
            word(&reset, 3, 56),
            1,
            "seek clears history instead of drawing a discontinuity"
        );
        assert_eq!(word(&reset, 3, 60), 0, "seek clears distance phase");
        assert_eq!(
            word(&reset, 4, 56),
            0,
            "seek resets cumulative UV distance too"
        );
        let resumed = step(1.75, [1, 2], 2, 40.6, 1, 0);
        assert_eq!(word(&resumed, 3, 56), 1);
        let expired = step(2.75, [0, 0], 0, 0.0, 1, 0);
        assert_eq!(
            word(&expired, 3, 44),
            0,
            "distance mode still expires by lifetime"
        );
        assert_eq!(word(&expired, 7, 44), 0);
        step(0.0, [0, 1], 2, 0.0, 2, 0);
        step(0.1, [0, 1], 2, 0.4, 2, 0);
        let stopped = step(1.0, [0, 1], 2, 0.4, 2, 0);
        assert_eq!(word(&stopped, stats_record, 28), 0);
        assert_eq!(
            word(&stopped, 3, 56),
            0,
            "stationary live heads cannot retain expired anchors"
        );
        assert_eq!(word(&stopped, 3, 44), 1, "living owner is still reserved");
        let corner = step_at(1.1, [0, 1], 2, Vec3::new(0.4, 0.8, 0.0), 2, 0);
        assert!((f32::from_bits(word(&corner, 4, 56)) - 1.2).abs() < 0.0001);
        assert!(
            (f32::from_bits(word(&corner, 5, 52)) - 1.0).abs() < 0.0001,
            "UV distance follows the corner, not the chord"
        );
        assert_eq!(word(&corner, 3, 56), 1);
        assert!((f32::from_bits(word(&corner, 5, 16)) - 0.4).abs() < 0.0001);
        assert!(
            (f32::from_bits(word(&corner, 5, 20)) - 0.6).abs() < 0.0001,
            "distance remainder follows the observed polyline through corners"
        );
        step(3.0, [0, 1], 2, 0.0, 3, 0);
        let truncated = step(3.2, [0, 1], 2, 10.0, 3, 0);
        assert_eq!(word(&truncated, stats_record, 28), 2);
        let held = step(3.7, [0, 1], 2, 10.0, 3, 0);
        assert_eq!(
            word(&held, stats_record, 28),
            2,
            "warning survives while missing points are unexpired"
        );
        let aged = step(4.2, [0, 1], 2, 10.0, 3, 0);
        assert_eq!(word(&aged, stats_record, 8), 2, "owners remain alive");
        assert_eq!(
            word(&aged, stats_record, 28),
            0,
            "warning clears without a seek or owner death"
        );
        return;
    }
    let initial = step(0.0, [0, 1], 2, 100.0, 0, 0);
    let stats_record = 3 + max_trails as usize * 4;
    assert_eq!(word(&initial, stats_record, 8), 2, "two occupied owners");
    assert_eq!(word(&initial, stats_record, 12), 0, "no retired owners");
    assert_eq!(word(&initial, 3, 56), 1);
    assert_eq!(f32::from_bits(word(&initial, 4, 16)), 100.0);
    step(0.125, [0, 1], 2, 200.0, 0, 0);
    let moved = step(0.25, [1, 0], 2, 300.0, 0, 0);
    assert_eq!(
        word(&moved, 3, 48),
        0,
        "owner survives physical slot permutation"
    );
    assert_eq!(word(&moved, 3, 56), 3);
    assert!(
        (f32::from_bits(word(&moved, 4, 56)) - 200.25).abs() < 0.0001,
        "time sampling records world-space path length too"
    );
    assert_eq!(f32::from_bits(word(&moved, 3, 16)), 300.25);
    assert_eq!(
        f32::from_bits(word(&moved, 4, 16)),
        100.0,
        "past world position does not follow current transform"
    );
    let paused = step(0.25, [1, 0], 2, 300.0, 0, 0);
    assert_eq!(
        word(&paused, stats_record, 28),
        0,
        "a full ring is not itself truncation"
    );
    assert_eq!(&moved[128..], &paused[128..], "pause freezes history");
    let looped = step(0.375, [2, 1], 2, 400.0, 0, 0);
    assert_eq!(
        word(&looped, stats_record, 28),
        1,
        "surviving time-sampled owner loses an unexpired point"
    );
    assert_eq!(word(&looped, 7, 48), 1);
    assert_eq!(
        word(&looped, 7, 56),
        3,
        "survivor keeps ring through loop boundary"
    );
    let new_owner = if max_trails == 2 { 3 } else { 11 };
    assert_eq!(word(&looped, new_owner, 48), 2);
    assert_eq!(
        word(&looped, new_owner, 56),
        1,
        "new parent cannot inherit evicted trail"
    );
    if max_trails == 4 {
        assert_eq!(
            word(&looped, 3, 48),
            0,
            "retired parent retains its own slot"
        );
        assert_eq!(
            word(&looped, 3, 56),
            3,
            "retired samples survive new births"
        );
        assert_eq!(word(&looped, 2, 56), 0, "no eviction with spare capacity");
        let full = step(0.4375, [3, 1], 2, 400.0, 0, 0);
        assert_eq!(
            word(&full, 2, 60),
            4,
            "peak occupancy includes retired owners"
        );
        assert_eq!(word(&full, 2, 56), 0);
        let overflow = step(0.45, [4, 1], 2, 400.0, 0, 0);
        assert_eq!(word(&overflow, 2, 56), 1);
        assert_eq!(
            word(&overflow, 3, 48),
            4,
            "oldest retired owner is evicted first"
        );
        assert_eq!(word(&overflow, 7, 48), 1, "living owner cannot be evicted");
        assert_eq!(
            word(&overflow, 11, 48),
            2,
            "newer retired owner is retained"
        );
        assert_eq!(word(&overflow, stats_record, 8), 4);
        assert_eq!(word(&overflow, stats_record, 12), 2);
        assert_eq!(word(&overflow, stats_record, 16), 1);
    }
    let retired = step(0.5, [0, 0], 0, 0.0, 0, 0);
    assert_eq!(word(&retired, 7, 44), 1, "tail remains after parent dies");
    let seek = step(0.625, [2, 1], 2, 0.0, 1, 0);
    assert_eq!(word(&seek, 3, 56), 1, "forward seek epoch resets history");
    assert_eq!(word(&seek, 2, 56), 0, "seek resets eviction counters");
    let seed = step(0.75, [2, 1], 2, 0.0, 1, 1);
    assert_eq!(word(&seed, 3, 56), 1, "seed change resets history");
    let backward = step(0.5, [2, 1], 2, 0.0, 1, 1);
    assert_eq!(word(&backward, 3, 56), 1);
    let expired = step(1.5, [0, 0], 0, 0.0, 1, 1);
    assert_eq!(word(&expired, 3, 44), 0);
    assert_eq!(word(&expired, 7, 44), 0);
    assert_eq!(word(&expired, stats_record, 8), 0);
    assert_eq!(word(&expired, stats_record, 12), 0);
    assert_eq!(
        word(&expired, stats_record, 28),
        0,
        "warning expires with the missing history"
    );
    assert_eq!(
        word(&expired, stats_record, 16),
        0,
        "expiry is not eviction"
    );
}
