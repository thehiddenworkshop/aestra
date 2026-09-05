//! Persistent history exercised on a native adapter using the production buffer ABI.
use aestra_gpu::{
    GpuEmitter, GpuGlobals, GpuParticle,
    shader::{SIMULATION_WESL, compile_wesl},
};
use bevy::math::{Mat4, UVec2, Vec3, Vec4};
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use wgpu::util::DeviceExt;

#[allow(dead_code)] // The native harness uses the same ordered transform packing as rendering.
#[path = "../src/host_transform.rs"]
mod host_transform;
#[allow(dead_code)] // Also contains render-world bookkeeping, not used by this native harness.
#[path = "../src/gpu/trail_checkpoints.rs"]
mod trail_checkpoints;
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
fn assert_world_bounds(bytes: &[u8], root: usize, points: usize, owners: usize) {
    let float = |slot, offset| f32::from_bits(word(bytes, slot, offset));
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut size = 0.0f32;
    let mut found = false;
    let mut valid = true;
    for owner in 0..owners {
        let base = root + 1 + owner * points;
        if word(bytes, base, 44) == 0 {
            continue;
        }
        let count = word(bytes, base, 56) as usize;
        if count == 0 {
            continue;
        }
        for index in 0..=count {
            let slot = if index == count {
                base
            } else {
                base + 1
                    + (word(bytes, base, 52) as usize + points - 1 - count + index) % (points - 1)
            };
            let position = Vec3::new(float(slot, 16), float(slot, 20), float(slot, 24));
            let width = float(slot, 28).abs();
            valid &= position.is_finite() && width.is_finite();
            minimum = minimum.min(position);
            maximum = maximum.max(position);
            size = size.max(width);
            found = true;
        }
    }
    assert_eq!(
        float(root, 28),
        if !valid {
            0.0
        } else if found {
            1.0
        } else {
            2.0
        }
    );
    if valid && found {
        assert_eq!(
            Vec3::new(float(root, 16), float(root, 20), float(root, 24)),
            minimum
        );
        assert_eq!(
            Vec3::new(float(root, 0), float(root, 4), float(root, 8)),
            maximum
        );
        assert_eq!(float(root, 12), size);
    }
}

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
fn check_seek_replay(device: &wgpu::Device, queue: &wgpu::Queue, sampling: u32, moving: bool) {
    let effect = aestra_core::EffectAsset::from_ron(if moving {
        include_str!("../../../assets/effects/moving_trail_lab.aestra.ron")
    } else {
        include_str!("../../../assets/effects/trail_lab.aestra.ron")
    })
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
    let mut artifact = aestra_gpu::GpuEffectArtifact::from_instance(&instance).unwrap();
    artifact.emitters[0].trail_sampling = sampling;
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
    let state_buffers: Vec<bevy::render::render_resource::Buffer> =
        [1, 2, 3, 4, 5, 7].map(|i| buffers[i].clone().into()).into();
    let state = state_buffers.iter().collect::<Vec<_>>();
    let render_device = bevy::render::renderer::RenderDevice::from(device.clone());
    let globals_buffer = bevy::render::render_resource::Buffer::from(buffers[6].clone());
    let run = |times: &[f32],
               epoch,
               mut cache: Option<&mut trail_checkpoints::TrailCheckpoints>| {
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
            contents: &host_transform::observation_bytes(
                times,
                Mat4::from_translation(Vec3::new(4.0, -2.0, 7.0)),
                instance.host_transform_track().map(|t| t.as_ref()),
            ),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: buffers[1].size() + buffers[7].size(),
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        let restored = cache
            .as_ref()
            .and_then(|c| c.restore(&mut encoder, &state, *times.last().unwrap()));
        if restored.is_some() {
            trail_checkpoints::rebase_epoch(
                &mut encoder,
                &globals_buffer,
                state[5],
                state[3],
                &[(0, e.trail_offset)],
            );
        }
        for (i, &time) in times.iter().enumerate() {
            if restored.is_some_and(|saved| time <= saved && time != *times.last().unwrap()) {
                continue;
            }
            encoder.copy_buffer_to_buffer(&times_buffer, i as u64 * 68, &buffers[6], 0, 4);
            encoder.copy_buffer_to_buffer(&times_buffer, i as u64 * 68 + 4, &buffers[6], 32, 64);
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_bind_group(0, &group, &[]);
            for pipeline in &pipelines {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(1, 1, 1);
            }
            drop(pass);
            if time >= 1.0
                && time.fract() == 0.0
                && let Some(cache) = cache.as_mut()
            {
                cache.capture(
                    &render_device,
                    &mut encoder,
                    &state,
                    time,
                    trail_checkpoints::MEMORY_LIMIT.saturating_sub(cache.bytes()),
                );
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
        let result = expand_legacy(&bytes[0..p], &bytes[p..], &[], p / 48);
        assert_world_bounds(&result, artifact.total_slots as usize, 64, 64);
        result
    };
    let heads =
        || (0..e.trail_capacity).map(|i| (e.trail_offset + 1 + i * e.trail_points) as usize);
    let direct = run(&[globals.time], 0, None);
    assert!(
        heads()
            .filter(|&h| word(&direct, h, 44) != 0)
            .all(|h| word(&direct, h, 56) == 1)
    );
    let mut planner = trail_replay::TrailReplay::default();
    let replayed = run(&planner.observations(1, globals.time), 1, None);
    assert!(
        heads().any(|h| word(&replayed, h, 56) > if sampling == 2 { 1 } else { 20 }),
        "seek must create drawable history, not only live heads"
    );
    assert_eq!(
        replayed,
        run(&planner.observations(1, globals.time), 1, None),
        "paused history must be stable"
    );
    let backward = run(&planner.observations(2, 0.75), 2, None);
    assert!(heads().any(|h| word(&backward, h, 56) > 5));
    let forward = run(&planner.observations(3, globals.time), 3, None);
    // Header epoch differs; owner/sample data must reproduce the first seek.
    let history_start = (e.trail_offset as usize + 1) * 64;
    assert_eq!(&replayed[history_start..], &forward[history_start..]);
    if moving && sampling == 0 {
        // Separate live render submissions must match one batched seek exactly.
        let mut live = trail_replay::TrailReplay::default();
        let mut observed = Vec::new();
        for frame in 0..=86 {
            let target = frame as f32 / 60.0;
            live.prepare_tracked(target);
            observed = run(&live.observations(30, target), 30, None);
        }
        assert_eq!(
            &replayed[history_start..],
            &observed[history_start..],
            "historical poses differ between playback and seeking"
        );
    }
    let beyond_loop = run(&planner.observations(4, 3.5), 4, None);
    assert!(heads().any(|h| word(&beyond_loop, h, 56) > if sampling == 2 { 1 } else { 20 }));

    let mut cache = trail_checkpoints::TrailCheckpoints::default();
    run(&planner.observations(5, 3.5), 5, Some(&mut cache));
    let snapshot_size: u64 = state.iter().map(|b| b.size()).sum();
    assert_eq!(cache.bytes(), snapshot_size * 3);
    for (i, target) in [1.0, 1.25, 2.005, 3.5, 0.75, 2.5].into_iter().enumerate() {
        let epoch = 6 + i as u32 * 2;
        let reference = run(&planner.observations(epoch, target), epoch, None);
        let restored = run(
            &planner.observations(epoch + 1, target),
            epoch + 1,
            Some(&mut cache),
        );
        assert_eq!(
            &reference[history_start..],
            &restored[history_start..],
            "checkpoint history/UV phase differs at {target}"
        );
        assert_eq!(word(&restored, e.trail_offset as usize, 52), epoch + 1);
    }
    // A zero available budget still permits reuse, never growth; an empty cache
    // falls back without allocating. Replacing a source buffer discards all state.
    let mut encoder = device.create_command_encoder(&Default::default());
    cache.capture(&render_device, &mut encoder, &state, 4.0, 0);
    assert_eq!(cache.bytes(), snapshot_size * 3);
    for time in [5.0, 6.0, 7.0] {
        cache.capture(
            &render_device,
            &mut encoder,
            &state,
            time,
            trail_checkpoints::MEMORY_LIMIT.saturating_sub(cache.bytes()),
        );
    }
    assert_eq!(cache.bytes(), snapshot_size * 4);
    assert_eq!(cache.restore(&mut encoder, &state, 2.0), None);
    assert_eq!(cache.restore(&mut encoder, &state, 6.5), Some(6.0));
    let mut empty = trail_checkpoints::TrailCheckpoints::default();
    empty.capture(&render_device, &mut encoder, &state, 1.0, 0);
    assert_eq!(empty.bytes(), 0);
    assert_eq!(empty.restore(&mut encoder, &state, 2.0), None);
    let mut history = trail_checkpoints::TrailHistory::default();
    history.sync_buffers(&state);
    history.checkpoints = cache;
    history.sync_buffers(&state);
    assert_ne!(history.checkpoints.bytes(), 0);
    let replacement = render_device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: state[0].size(),
        mapped_at_creation: false,
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
    });
    let mut replaced = state.clone();
    replaced[0] = &replacement;
    history.sync_buffers(&replaced);
    assert_eq!(history.checkpoints.bytes(), 0);
    queue.submit([encoder.finish()]);
}

#[test]
fn trails_preserve_identity_world_history_and_retired_tails_and_reset_on_discontinuities() {
    check_pool(2, 0);
}

#[test]
fn separate_trail_budget_retains_burst_tails_until_expiry_or_oldest_retired_eviction() {
    check_pool(4, 0);
}

#[test]
fn distance_sampling_handles_stationary_speed_changes_overflow_loops_and_resets() {
    check_pool(4, 1);
}

#[test]
fn adaptive_sampling_bounds_observed_curve_error_and_preserves_corners_with_fewer_points() {
    check_pool(4, 2);
}

fn check_adaptive(
    step: &impl Fn(f32, [u32; 2], u32, Vec3, u32, u32) -> Vec<u8>,
    points: u32,
    owners: u32,
) {
    let float = |bytes: &[u8], slot, offset| f32::from_bits(word(bytes, slot, offset));
    let position = |bytes: &[u8], slot| {
        Vec3::new(
            float(bytes, slot, 16),
            float(bytes, slot, 20),
            float(bytes, slot, 24),
        )
    };
    let stats = (3 + owners * points) as usize;
    let capacity = points as usize - 1;
    let slots = |bytes: &[u8]| {
        let count = word(bytes, 3, 56) as usize;
        let next = word(bytes, 3, 52) as usize;
        (0..count)
            .map(|i| 4 + (next + capacity - count + i) % capacity)
            .collect::<Vec<_>>()
    };
    step(0.0, [0, 1], 1, Vec3::ZERO, 0, 0);
    let stationary = step(0.2, [0, 1], 1, Vec3::ZERO, 0, 0);
    assert_eq!(word(&stationary, 3, 56), 1);
    for i in 1..=20 {
        let straight = step(
            0.2 + i as f32 / 120.0,
            [0, 1],
            1,
            Vec3::X * (i as f32 * 0.025),
            0,
            0,
        );
        assert_eq!(
            word(&straight, 3, 56),
            1,
            "straight observations should collapse into one span"
        );
    }
    step(0.0, [0, 1], 1, Vec3::ZERO, 1, 0);
    step(0.1, [0, 1], 1, Vec3::new(0.4, 0.0, 0.0), 1, 0);
    let corner = step(0.2, [0, 1], 1, Vec3::new(0.4, 0.4, 0.0), 1, 0);
    assert_eq!(word(&corner, 3, 56), 2);
    assert_eq!(position(&corner, 5), Vec3::new(0.4, 0.0, 0.0));
    assert!(
        (float(&corner, 5, 52) - 0.4).abs() < 1e-5,
        "corner UV phase follows observed arc length"
    );
    assert!((float(&corner, 4, 56) - 0.8).abs() < 1e-5);
    step(0.0, [0, 1], 1, Vec3::ZERO, 4, 0);
    step(0.1, [0, 1], 1, Vec3::X * 0.4, 4, 0);
    let reversal = step(0.2, [0, 1], 1, Vec3::ZERO, 4, 0);
    assert_eq!(
        word(&reversal, 3, 56),
        2,
        "coincident endpoints must not erase a reversal"
    );
    assert_eq!(position(&reversal, 5), Vec3::X * 0.4);

    // Check every omitted observation, not just the last bend, against each
    // retained chord of a gradual 3D curve. All observations are still unexpired.
    let observations = (0..=60)
        .map(|i| {
            let theta = i as f32 * std::f32::consts::FRAC_PI_2 / 60.0;
            (
                i as f32 / 120.0,
                Vec3::new(theta.cos() * 2.0, theta.sin() * 2.0, theta * 0.3),
            )
        })
        .collect::<Vec<_>>();
    let mut curve = Vec::new();
    for &(time, p) in &observations {
        curve = step(time, [0, 1], 1, p, 2, 0);
    }
    let mut anchors = slots(&curve);
    assert!(anchors.len() > 2 && anchors.len() < observations.len() / 2);
    anchors.push(3); // Live head ends the last simplified span.
    for pair in anchors.windows(2) {
        let a = position(&curve, pair[0]);
        let b = position(&curve, pair[1]);
        assert!(a.distance(b) <= 1.0001, "maximum spacing exceeded");
        for &(time, p) in &observations {
            if time < float(&curve, pair[0], 32) || time > float(&curve, pair[1], 32) {
                continue;
            }
            let along = ((p - a).dot(b - a) / (b - a).length_squared().max(1e-10)).clamp(0.0, 1.0);
            assert!(
                p.distance(a.lerp(b, along)) <= 0.0201,
                "curve tolerance exceeded at {time}"
            );
        }
    }
    for pair in slots(&curve).windows(2) {
        assert!(float(&curve, pair[0], 52) <= float(&curve, pair[1], 52));
    }
    let paused = step(0.5, [0, 1], 1, observations[60].1, 2, 0);
    assert_eq!(&curve[3 * 64..], &paused[3 * 64..]);
    let retired = step(0.6, [0, 1], 0, Vec3::ZERO, 2, 0);
    assert_eq!(word(&retired, stats, 12), 1);
    assert_eq!(
        &curve[4 * 64..(3 + points as usize) * 64],
        &retired[4 * 64..(3 + points as usize) * 64]
    );
    let expired = step(1.6, [0, 1], 0, Vec3::ZERO, 2, 0);
    assert_eq!(word(&expired, stats, 8), 0);

    step(0.0, [0, 1], 1, Vec3::ZERO, 3, 0);
    let fast = step(0.1, [0, 1], 1, Vec3::X * 1000.25, 3, 0);
    assert_eq!(word(&fast, 3, 56), points - 1);
    assert_eq!(
        word(&fast, stats, 28),
        1,
        "budget loss must remain visible in telemetry"
    );
    for pair in slots(&fast).windows(2) {
        assert!((position(&fast, pair[0]).distance(position(&fast, pair[1])) - 1.0).abs() < 1e-4);
    }
    let stopped = step(1.095, [0, 1], 1, Vec3::X * 1000.25, 3, 0);
    assert!(word(&stopped, 3, 56) < points - 1);
    let stopped = step(1.2, [0, 1], 1, Vec3::X * 1000.25, 3, 0);
    assert_eq!(word(&stopped, 3, 56), 0);
    let moving_again = step(1.3, [0, 1], 1, Vec3::X * 1000.5, 3, 0);
    assert_eq!(
        word(&moving_again, 3, 56),
        1,
        "expired anchors must not bridge a stopped path"
    );
}

fn check_pool(max_trails: u32, sampling: u32) {
    let distance = sampling != 0;
    let points = if sampling == 2 { 64 } else { 4 };
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
    if sampling == 1 {
        for moving in [false, true] {
            for sampling in 0..3 {
                check_seek_replay(&device, &queue, sampling, moving);
            }
        }
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
            trail_points: points,
            trail_capacity: max_trails,
            trail_sampling: sampling,
            trail_tolerance: 0.02,
            trail_distance: 1.0,
            trail_interval: 0.125,
            trail_lifetime: 1.0,
            ..Default::default()
        }]),
        encode(&vec![
            GpuParticle::default();
            3 + (max_trails * points) as usize
        ]),
        encode(&vec![0u32, 1]),
        encode(&vec![0u32; 2]),
        encode(&vec![0u32; 8]),
        encode(&vec![6u32, 2, 0, 0]),
        encode(&GpuGlobals::default()),
        encode(&vec![0u32; (3 + (max_trails * points) as usize) * 3]),
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
            let result = expand_legacy(&bytes[0..p], &bytes[p + c..], &bytes[p..p + c], p / 48);
            assert_world_bounds(&result, 2, points as usize, max_trails as usize);
            result
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
    if sampling == 2 {
        check_adaptive(&step_at, points, max_trails);
        return;
    }
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
