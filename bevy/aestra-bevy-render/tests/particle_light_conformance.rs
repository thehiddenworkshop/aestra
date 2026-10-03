//! F7D1 native GPU/CPU agreement and bounded-selection measurements.
//! Full particle uploads/readback here are fixture validation, NOT a playback adapter.
use aestra_core::*;
use aestra_gpu::{GpuParticle, particle_lights::*, shader};
use aestra_runtime::{
    CompiledGradient, ParticleLightColorPlan, ParticleSample, RuntimeValue, SceneOutputPlanKind,
};
use bevy::math::{Mat4, Vec3, Vec4};
use encase::ShaderType;
use wgpu::util::DeviceExt;

fn encode<T: ShaderType + encase::internal::WriteInto>(value: &T) -> Vec<u8> {
    let mut bytes = encase::StorageBuffer::new(Vec::new());
    bytes.write(value).unwrap();
    bytes.into_inner()
}

struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    pipelines: [wgpu::ComputePipeline; 3],
    timestamps: bool,
}

impl Harness {
    fn new() -> Option<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let gpu = wgpu::Instance::new(descriptor);
        let Ok(adapter) = pollster::block_on(gpu.request_adapter(&Default::default())) else {
            assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
            eprintln!("Skipping particle lights: no GPU adapter");
            return None;
        };
        eprintln!("F7D1 adapter: {:?}", adapter.get_info());
        let timestamps = adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: adapter.limits(),
            required_features: if timestamps {
                wgpu::Features::TIMESTAMP_QUERY
            } else {
                wgpu::Features::empty()
            },
            ..Default::default()
        }))
        .unwrap();
        let code = shader::compile_wesl(
            "package::particle_lights",
            PARTICLE_LIGHT_WESL,
            PARTICLE_LIGHT_ENTRY_POINTS,
        )
        .unwrap();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particle light selection"),
            source: wgpu::ShaderSource::Wgsl(code.wgsl.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &(0..8)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: if binding == 3 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage {
                                read_only: binding < 5 || binding == 7,
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
        let pipelines = PARTICLE_LIGHT_ENTRY_POINTS
            .iter()
            .map(|entry| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
            .collect::<Vec<_>>()
            .try_into()
            .unwrap();
        Some(Self {
            device,
            queue,
            layout,
            pipelines,
            timestamps,
        })
    }

    fn buffer(&self, bytes: &[u8], uniform: bool) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytes,
                usage: (if uniform {
                    wgpu::BufferUsages::UNIFORM
                } else {
                    wgpu::BufferUsages::STORAGE
                }) | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
    }

    fn check_global(&self, width: u32, runs: u32, cap: u32) {
        let limits = self.device.limits();
        let work = GlobalLightWorkPlan::new(
            width,
            runs,
            cap,
            limits.max_storage_buffer_binding_size,
            limits.max_compute_workgroups_per_dimension,
        )
        .unwrap()
        .unwrap();
        let code = shader::compile_wesl(
            "package::global_lights",
            GLOBAL_PARTICLE_LIGHT_WESL,
            GLOBAL_LIGHT_ENTRY_POINTS,
        )
        .unwrap();
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("global light conformance"),
                source: wgpu::ShaderSource::Wgsl(code.wgsl.into()),
            });
        let layout = self
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &(0..5)
                    .map(|binding| wgpu::BindGroupLayoutEntry {
                        binding,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: if binding == 2 {
                                wgpu::BufferBindingType::Uniform
                            } else {
                                wgpu::BufferBindingType::Storage {
                                    read_only: binding == 0 || binding == 3,
                                }
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    })
                    .collect::<Vec<_>>(),
            });
        let pipeline_layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
        let pipelines: Vec<_> = GLOBAL_LIGHT_ENTRY_POINTS
            .iter()
            .map(|entry| {
                self.device
                    .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&pipeline_layout),
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    })
            })
            .collect();
        let scratch = [0, 1].map(|_| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: work.scratch_bytes_per_buffer,
                mapped_at_creation: false,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_SRC
                    | wgpu::BufferUsages::COPY_DST,
            })
        });
        let counts = self.buffer(&vec![0; runs as usize * 16], false);
        let counters = self.buffer(&encode(&GpuLightCounters::default()), false);
        let schedule: Vec<_> = work
            .merges
            .iter()
            .copied()
            .chain([work.finish, work.finish])
            .collect();
        let uniforms: Vec<_> = schedule
            .iter()
            .map(|d| self.buffer(&encode(d), true))
            .collect();
        let groups: Vec<_> = uniforms
            .iter()
            .enumerate()
            .map(|(index, uniform)| {
                let read = index.min(work.merges.len()) % 2;
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &layout,
                    entries: &[
                        &scratch[read],
                        &scratch[1 - read],
                        uniform,
                        &counts,
                        &counters,
                    ]
                    .iter()
                    .enumerate()
                    .map(|(binding, b)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
                })
            })
            .collect();
        let selected_bytes = u64::from(work.selected_capacity) * LIGHT_RECORD_BYTES;
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: selected_bytes + 16,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });
        // Sorted output runs carry canonical tokens independent of input/run order.
        // Reuse scratch through live -> empty -> reversed runs; odd runs and unequal
        // local counts must preserve priority/lumen/token/ordinal ordering and padding.
        for frame in 0..3 {
            let mut input = vec![GpuParticleLight::default(); work.scratch_records as usize];
            let mut source_counts = vec![bevy::math::UVec4::ZERO; runs as usize];
            let mut expected = Vec::new();
            let mut requested = 0;
            let mut candidates = 0;
            for slot in 0..runs {
                let token = if frame == 2 { runs - slot - 1 } else { slot };
                let count = if frame == 1 { 0 } else { width - token % width };
                for ordinal in 0..count {
                    let record = GpuParticleLight {
                        position_range: Vec4::new(token as f32, ordinal as f32, 0.0, 12.0),
                        color_intensity: Vec4::new(0.8, 0.2, 0.1, 1000.0 - (ordinal / 2) as f32),
                        priority: token % 3,
                        source_token: token,
                        particle_index: ordinal,
                        ..Default::default()
                    };
                    input[(slot * width + ordinal) as usize] = record;
                    expected.push(record);
                }
                // Include candidates dropped at the per-output quality cap, too.
                let extra = if frame == 1 { 0 } else { 7 };
                source_counts[slot as usize] =
                    bevy::math::UVec4::new(count + extra + 4, count + extra, count, extra);
                requested += count + extra + 4;
                candidates += count + extra;
            }
            expected.sort_by(|a, b| {
                b.priority
                    .cmp(&a.priority)
                    .then_with(|| b.color_intensity.w.total_cmp(&a.color_intensity.w))
                    .then_with(|| a.source_token.cmp(&b.source_token))
                    .then_with(|| a.particle_index.cmp(&b.particle_index))
            });
            expected.truncate(cap as usize);
            self.queue.write_buffer(&scratch[0], 0, &encode(&input));
            self.queue.write_buffer(&counts, 0, &encode(&source_counts));
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.clear_buffer(&counters, 0, None);
            for (index, group) in groups.iter().enumerate() {
                let merge_count = work.merges.len();
                let (pipeline, workgroups) = if index < merge_count {
                    let d = schedule[index];
                    (0, (d.input.z * d.input.w).div_ceil(64))
                } else if index == merge_count {
                    (1, runs.div_ceil(64))
                } else {
                    (2, 1)
                };
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipelines[pipeline]);
                pass.set_bind_group(0, group, &[]);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
            encoder.copy_buffer_to_buffer(
                &scratch[work.merges.len() % 2],
                0,
                &readback,
                0,
                selected_bytes,
            );
            encoder.copy_buffer_to_buffer(&counters, 0, &readback, selected_bytes, 16);
            self.queue.submit([encoder.finish()]);
            let (sender, receiver) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |r| sender.send(r).unwrap());
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(30)),
                })
                .unwrap();
            receiver.recv().unwrap().unwrap();
            let bytes = readback.slice(..).get_mapped_range();
            if !expected.is_empty() {
                assert_eq!(&bytes[..expected.len() * 48], encode(&expected));
            }
            let words: Vec<_> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w))
                .collect();
            let offset = selected_bytes as usize / 4;
            assert_eq!(
                &words[offset..],
                &[
                    requested,
                    candidates,
                    expected.len() as u32,
                    candidates - expected.len() as u32
                ]
            );
            for record in words[expected.len() * 12..offset].as_chunks::<12>().0 {
                assert_eq!(record[7], 0, "stale global record");
            }
            drop(bytes);
            readback.unmap();
        }
        eprintln!(
            "F7D2 global width={width} runs={runs} cap={cap} scratch_bytes={} passed live/empty/reordered",
            work.scratch_bytes_per_buffer * 2
        );
    }

    fn check(&self, count: u32, cap: u32, variant: u32, repeats: usize) {
        let mut source = EffectAsset::new("luminous embers", 4.0);
        let mut emitter = Emitter::basic_sprite("generic", 4.0);
        emitter.renderers.clear();
        let mut properties = ParticlePointLightProperties::new(2000.0, 12.0);
        properties.max_lights_by_quality.insert("high".into(), cap);
        if variant == 1 {
            properties.intensity_curve = Curve::new(vec![CurveKey::new(0.0, 1234.0)]);
            properties.color_source = ParticleLightColorSource::Constant([0.8, 0.2, 0.1]);
        } else if variant >= 2 {
            // More than the old eight-key GPU limit, including all 32 curve keys.
            properties.intensity_curve = Curve::new(
                (0..32)
                    .map(|i| CurveKey::new(i as f32 / 31.0, 2000.0 - i as f32 * 50.0))
                    .collect(),
            );
            properties.intensity_curve.interpolation = if variant == 2 {
                CurveInterpolation::Linear
            } else {
                CurveInterpolation::Step
            };
            properties.range_curve.interpolation = CurveInterpolation::Step;
        }
        emitter
            .scene_outputs
            .push(SceneOutputInstance::particle_point_light(properties));
        source.emitters.push(emitter);
        let compiled = aestra_compiler::EffectCompiler::default()
            .compile(&source)
            .unwrap();
        let emitter = &compiled.emitters[0];
        let mut output = emitter.scene_outputs[0].clone();
        let SceneOutputPlanKind::ParticlePointLight(plan) = &mut output.kind;
        let mut parameters = Vec::new();
        if variant >= 2 {
            let gradient = CompiledGradient::compile(&Gradient::new(
                (0..12)
                    .map(|i| ColorKey::new(i as f32 / 11.0, [i as f32 / 11.0, 0.4, 0.8, 0.0]))
                    .collect(),
            ));
            plan.color = if variant == 2 {
                ParticleLightColorPlan::Gradient(gradient)
            } else {
                parameters.push(RuntimeValue::Gradient(gradient));
                ParticleLightColorPlan::GradientParameter(aestra_runtime::ParameterSlot(0))
            };
        }
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::splat(2.0),
            bevy::math::Quat::from_rotation_y(0.4),
            Vec3::new(10.0, 20.0, 30.0),
        );
        let (gpu_plan, keys) = lower_plan(
            plan,
            &parameters,
            0,
            77,
            transform,
            self.device.limits().max_storage_buffer_binding_size,
        )
        .unwrap();
        // Prefix/suffix excluded by pool range; reverse stable ordinals relative to slots.
        let mut particles = vec![GpuParticle::default(); count as usize + 7];
        for i in 0..count {
            let mut p = GpuParticle {
                position: Vec3::new(i as f32 % 50.0, 2.0, 3.0),
                color: Vec4::new(0.9, 0.3, 0.1, 0.0),
                size: 0.0,
                normalized_age: (i % 101) as f32 / 101.0,
                packed_emitter_alive: 1,
                particle_index: count - i,
                ..Default::default()
            };
            if variant != 1 {
                match i % 23 {
                    0 => p.packed_emitter_alive = 0,
                    1 => p.packed_emitter_alive = 2, // retired trail, not live particle
                    2 => p.packed_emitter_alive = (1 << 16) | 1, // other emitter
                    3 => p.normalized_age = 1.0,
                    4 => p.normalized_age = f32::NAN,
                    5 => p.position.x = f32::INFINITY,
                    6 => p.color.x = f32::NAN,
                    7 => p.normalized_age = -0.2,
                    _ => (),
                }
            } else {
                p.color = Vec4::splat(f32::NAN); // constant RGB ignores particle color/alpha
            }
            particles[i as usize + 3] = p;
        }
        let mut requested = 0;
        let mut expected = Vec::new();
        for p in &particles[3..count as usize + 3] {
            if p.packed_emitter_alive != 1 {
                continue;
            }
            requested += 1;
            let sample = ParticleSample {
                emitter_index: 0,
                particle_index: p.particle_index,
                position: p.position.to_array(),
                color: p.color.to_array(),
                size: p.size,
                rotation: p.rotation,
                normalized_age: p.normalized_age,
            };
            if let Some(candidate) =
                output.particle_light_candidate(emitter, &sample, |slot| parameters.get(slot.0))
            {
                expected.push(candidate);
            }
        }
        expected.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| {
                    b.light
                        .intensity_lumens
                        .total_cmp(&a.light.intensity_lumens)
                })
                .then_with(|| a.identity.cmp(&b.identity))
        });
        let candidates = expected.len() as u32;
        expected.truncate(cap as usize);
        let limits = self.device.limits();
        let SceneOutputPlanKind::ParticlePointLight(light_plan) = &output.kind;
        let work = ParticleLightWorkPlan::for_output(
            light_plan,
            3,
            count,
            cap,
            limits.max_storage_buffer_binding_size,
            limits.max_compute_workgroups_per_dimension,
        )
        .unwrap()
        .unwrap();
        let particle_bytes = encode(&particles);
        let input = self.buffer(&particle_bytes, false);
        let plan_buffer = self.buffer(&encode(&gpu_plan), false);
        let key_buffer = self.buffer(&encode(&keys), false);
        let globals = self.buffer(&encode(&aestra_gpu::GpuRenderGlobals::default()), false);
        let scratch = [0, 1].map(|_| {
            self.device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: work.scratch_bytes_per_buffer,
                mapped_at_creation: false,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            })
        });
        let counters = self.buffer(&encode(&GpuLightCounters::default()), false);
        let schedule: Vec<_> = std::iter::once(work.evaluate)
            .chain(work.merges.iter().copied())
            .chain(std::iter::once(work.finish))
            .collect();
        let uniforms: Vec<_> = schedule
            .iter()
            .map(|params| self.buffer(&encode(params), true))
            .collect();
        let final_index = work.merges.len() % 2;
        let groups: Vec<_> = uniforms
            .iter()
            .enumerate()
            .map(|(index, uniform)| {
                let (read, write) = if index == 0 {
                    (1, 0)
                } else if index == uniforms.len() - 1 {
                    (final_index, 1 - final_index)
                } else {
                    ((index - 1) % 2, index % 2)
                };
                self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &self.layout,
                    entries: &[
                        &input,
                        &plan_buffer,
                        &key_buffer,
                        uniform,
                        &scratch[read],
                        &scratch[write],
                        &counters,
                        &globals,
                    ]
                    .iter()
                    .enumerate()
                    .map(|(binding, buffer)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: buffer.as_entire_binding(),
                    })
                    .collect::<Vec<_>>(),
                })
            })
            .collect();
        let selected_bytes = u64::from(work.selected_capacity) * LIGHT_RECORD_BYTES;
        let verify_particles = count <= 1025;
        let particle_copy_bytes = if verify_particles {
            particle_bytes.len() as u64
        } else {
            0
        };
        let queries = self.timestamps.then(|| {
            self.device.create_query_set(&wgpu::QuerySetDescriptor {
                label: None,
                ty: wgpu::QueryType::Timestamp,
                count: 2,
            })
        });
        let resolve = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 16,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: selected_bytes + 32 + particle_copy_bytes,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });
        let mut times = Vec::new();
        for iteration in 0..repeats {
            // A live -> empty -> reordered live sequence must remove old lights
            // and restore the same identities without relying on slot order.
            let empty = repeats == 3 && iteration == 1;
            let frame_bytes = if repeats == 3 {
                let mut frame_particles = particles.clone();
                if empty {
                    for particle in &mut frame_particles {
                        particle.packed_emitter_alive = 0;
                    }
                } else if iteration == 2 {
                    frame_particles[3..count as usize + 3].reverse();
                }
                Some(encode(&frame_particles))
            } else {
                None
            };
            if let Some(frame_bytes) = &frame_bytes {
                self.queue.write_buffer(&input, 0, frame_bytes);
            }
            let frame_expected = if empty { &expected[..0] } else { &expected[..] };
            let frame_candidates = if empty { 0 } else { candidates };
            let mut encoder = self.device.create_command_encoder(&Default::default());
            encoder.clear_buffer(&counters, 0, None);
            // Existing scratch deliberately reused without clearing: every output
            // slot must be initialized by each pass, including odd/partial runs.
            for (index, group) in groups.iter().enumerate() {
                let first = index == 0;
                let last = index == groups.len() - 1;
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: queries.as_ref().filter(|_| first || last).map(|query_set| {
                        wgpu::ComputePassTimestampWrites {
                            query_set,
                            beginning_of_pass_write_index: first.then_some(0),
                            end_of_pass_write_index: last.then_some(1),
                        }
                    }),
                });
                pass.set_pipeline(
                    &self.pipelines[if first {
                        0
                    } else if last {
                        2
                    } else {
                        1
                    }],
                );
                pass.set_bind_group(0, group, &[]);
                let params = schedule[index];
                pass.dispatch_workgroups(
                    if first {
                        params.input.w
                    } else if last {
                        1
                    } else {
                        (params.input.z * params.input.w).div_ceil(64)
                    },
                    1,
                    1,
                );
            }
            if let Some(queries) = &queries {
                encoder.resolve_query_set(queries, 0..2, &resolve, 0);
            }
            // Vulkan query resolve/copy requires a command-buffer boundary.
            let work_commands = encoder.finish();
            let mut copy = self.device.create_command_encoder(&Default::default());
            copy.copy_buffer_to_buffer(&scratch[final_index], 0, &readback, 0, selected_bytes);
            copy.copy_buffer_to_buffer(&counters, 0, &readback, selected_bytes, 16);
            if queries.is_some() {
                copy.copy_buffer_to_buffer(&resolve, 0, &readback, selected_bytes + 16, 16);
            }
            if verify_particles {
                copy.copy_buffer_to_buffer(
                    &input,
                    0,
                    &readback,
                    selected_bytes + 32,
                    particle_copy_bytes,
                );
            }
            self.queue.submit([work_commands, copy.finish()]);
            let (sender, receiver) = std::sync::mpsc::channel();
            readback
                .slice(..)
                .map_async(wgpu::MapMode::Read, move |result| {
                    sender.send(result).unwrap()
                });
            self.device
                .poll(wgpu::PollType::Wait {
                    submission_index: None,
                    timeout: Some(std::time::Duration::from_secs(60)),
                })
                .unwrap();
            receiver.recv().unwrap().unwrap();
            let bytes = readback.slice(..).get_mapped_range();
            let words: Vec<_> = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|w| u32::from_le_bytes(*w))
                .collect();
            let offset = selected_bytes as usize / 4;
            assert_eq!(
                &words[offset..offset + 4],
                &[
                    if empty { 0 } else { requested },
                    frame_candidates,
                    frame_expected.len() as u32,
                    frame_candidates - frame_expected.len() as u32
                ],
                "count={count}, cap={cap}, variant={variant}"
            );
            for (i, candidate) in frame_expected.iter().enumerate() {
                let record = &words[i * 12..i * 12 + 12];
                let position = transform.transform_point3(Vec3::from_array(candidate.position));
                let values = [
                    position.x,
                    position.y,
                    position.z,
                    candidate.light.range,
                    candidate.light.linear_color[0],
                    candidate.light.linear_color[1],
                    candidate.light.linear_color[2],
                    candidate.light.intensity_lumens,
                    candidate.light.radius,
                ];
                for (word, expected) in record[..9].iter().zip(values) {
                    let actual = f32::from_bits(*word);
                    assert!(
                        (actual - expected).abs() <= 0.001 * expected.abs().max(1.0),
                        "actual={actual}, expected={expected}"
                    );
                }
                assert_eq!(
                    &record[9..],
                    &[candidate.priority, 77, candidate.identity.particle_index]
                );
            }
            for record in words[frame_expected.len() * 12..offset].as_chunks::<12>().0 {
                assert_eq!(record[7], 0, "unused records must be invalid, not stale");
            }
            if verify_particles {
                assert_eq!(
                    &bytes[selected_bytes as usize + 32..],
                    frame_bytes.as_ref().unwrap_or(&particle_bytes)
                );
            }
            if queries.is_some() {
                let ticks: Vec<_> = bytes
                    [selected_bytes as usize + 16..selected_bytes as usize + 32]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|v| u64::from_le_bytes(*v))
                    .collect();
                assert!(
                    ticks[0] != 0 && ticks[1] >= ticks[0],
                    "invalid timestamps {ticks:?}"
                );
                if iteration > 0 {
                    times.push(
                        (ticks[1] - ticks[0]) as f64 * f64::from(self.queue.get_timestamp_period())
                            / 1e6,
                    );
                }
            }
            drop(bytes);
            readback.unmap();
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "F7D1 count={count} cap={cap} variant={variant} requested={requested} candidates={candidates} selected={} dropped={} scratch_bytes={} selected_bytes={} merge_passes={} measured_samples={} gpu_median_ms={:?} gpu_p95_ms={:?}",
            expected.len(),
            candidates - expected.len() as u32,
            work.scratch_bytes_per_buffer * 2,
            selected_bytes,
            work.merges.len(),
            times.len(),
            times.get(times.len() / 2),
            times.get((times.len() * 95 / 100).min(times.len().saturating_sub(1)))
        );
    }
}

#[test]
fn global_lights_merge_quality_capped_outputs_with_stable_priority_ties() {
    let Some(gpu) = Harness::new() else {
        return;
    };
    for (width, runs, cap) in [(1, 1, 1), (3, 5, 7), (65, 3, 129), (16, 65, 64)] {
        gpu.check_global(width, runs, cap);
    }
}

#[test]
fn particle_lights_match_cpu_with_stable_ties_curves_gradients_and_reused_scratch() {
    let Some(gpu) = Harness::new() else {
        return;
    };
    for (count, cap, variant) in [
        (1, 16, 0),
        (63, 1, 0),
        (64, 63, 1),
        (65, 65, 1),
        (129, 129, 0),
        (1025, 65, 2),
        (4096, 16, 3),
        (65536, 32, 0),
    ] {
        gpu.check(count, cap, variant, 3);
    }
}

#[test]
#[ignore = "native GPU selection profiling probe; run explicitly with --nocapture"]
fn particle_light_selection_profile() {
    let gpu = Harness::new().expect("profiling requires a native GPU");
    assert!(gpu.timestamps, "profiling requires GPU timestamps");
    for (count, cap, variant) in [(512, 16, 0), (4096, 32, 0), (65536, 64, 0), (262144, 64, 1)] {
        gpu.check(count, cap, variant, 21);
    }
}
