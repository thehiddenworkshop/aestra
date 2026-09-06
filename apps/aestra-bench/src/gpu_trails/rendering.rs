//! Controlled A/B rendering experiment, not a claim about whole application frame time.
use super::{CaseReport, Comparison, Report, Timing, encode, group};
use crate::Config;
use aestra_gpu::{GpuParticle, GpuRenderGlobals, GpuTrailCullParams, shader};
use glam::{Mat4, UVec4, Vec3, Vec4};
use wgpu::util::DeviceExt;

const OWNERS: u32 = crate::TRAIL_OWNER_CAPACITY;
const QUERY_COUNT: u32 = 4 + 2 * crate::MAX_TRAIL_VIEWS as u32;
const TIMESTAMP_BYTES: u64 = QUERY_COUNT as u64 * 8;
const POINTS: u32 = 64;
const CANDIDATES: u32 = OWNERS * (POINTS - 1);
const WIDTH: u32 = 1024;
const HEIGHT: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Path {
    Full,
    Compact,
}
impl Path {
    fn index(self) -> usize {
        usize::from(self == Self::Compact)
    }
}

struct Harness {
    adapter: wgpu::AdapterInfo,
    device: wgpu::Device,
    queue: wgpu::Queue,
    compact: [wgpu::ComputePipeline; 3],
    compact_layout: wgpu::BindGroupLayout,
    cull: wgpu::ComputePipeline,
    render: wgpu::RenderPipeline,
    texture: wgpu::TextureView,
    sampler: wgpu::Sampler,
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
}

impl Harness {
    fn new() -> Self {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
            .expect("native GPU required");
        println!("render A/B adapter={:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .expect("timestamp queries required");
        let compile = |source: &str, entries: &[&str]| {
            let code = shader::compile_wesl("package::benchmark", source, entries).unwrap();
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(code.wgsl.into()),
            })
        };
        let compact_module = compile(
            &shader::trail_compact_wesl(),
            &["classify_trail", "prefix_trail", "scatter_trail"],
        );
        // Explicit layout: all three passes use the same group despite differing live bindings.
        let compact_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&compact_layout)],
            immediate_size: 0,
        });
        let compact = ["classify_trail", "prefix_trail", "scatter_trail"].map(|entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: None,
                layout: Some(&layout),
                module: &compact_module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        });
        let cull_module = compile(&shader::trail_cull_wesl(), &["cull_trail"]);
        let cull = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: None,
            layout: None,
            module: &cull_module,
            entry_point: Some("cull_trail"),
            compilation_options: Default::default(),
            cache: None,
        });
        let render_module = compile(shader::SPRITE_RENDER_WESL, &["vertex", "fragment_alpha"]);
        let render = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("production trail alpha rendering"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &render_module,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &render_module,
                entry_point: Some("fragment_alpha"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let texture = device.create_texture(&wgpu::TextureDescriptor {
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
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            texture.as_image_copy(),
            &[255; 4],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            texture.size(),
        );
        let texture = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&Default::default());
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: QUERY_COUNT,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: TIMESTAMP_BYTES,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: TIMESTAMP_BYTES + crate::MAX_TRAIL_VIEWS as u64 * 16,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Self {
            adapter: adapter.get_info(),
            device,
            queue,
            compact,
            compact_layout,
            cull,
            render,
            texture,
            sampler,
            queries,
            resolve,
            readback,
        }
    }

    fn buffer(&self, bytes: Vec<u8>, uniform: bool) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: &bytes,
                usage: if uniform {
                    wgpu::BufferUsages::UNIFORM
                } else {
                    wgpu::BufferUsages::STORAGE
                        | wgpu::BufferUsages::COPY_SRC
                        | wgpu::BufferUsages::INDIRECT
                },
            })
    }

    fn map(&self, buffer: &wgpu::Buffer, submission: wgpu::SubmissionIndex) -> Vec<u8> {
        let (tx, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .unwrap();
        let bytes = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        bytes
    }
}

struct View {
    camera: wgpu::BindGroup,
    culling: [wgpu::BindGroup; 2],
    indirect: [wgpu::Buffer; 2],
    target: wgpu::Texture,
    attachment: wgpu::TextureView,
}

struct Scene {
    compact: wgpu::BindGroup,
    rendering: [wgpu::BindGroup; 2],
    views: Vec<View>,
    expected: u32,
}

impl Scene {
    fn new(h: &Harness, active: u32, seed: u32, view_count: usize) -> Self {
        let mut asset = aestra_core::EffectAsset::new("Render benchmark", 3.0);
        asset
            .emitters
            .push(aestra_core::Emitter::basic_sprite("Trail", 3.0));
        let effect = aestra_runtime::EffectInstance::new(std::sync::Arc::new(
            aestra_compiler::EffectCompiler::default()
                .compile(&asset)
                .unwrap(),
        ));
        let mut renderer = aestra_gpu::GpuEffectArtifact::from_instance(&effect)
            .unwrap()
            .renderers[0];
        renderer.renderer_kind = 4;
        renderer.frame_count = POINTS;
        renderer.frame_rate = 1.0;
        renderer.playback_mode = OWNERS;
        renderer.flipbook_flags = 0;
        renderer.attribute_flags.z = 0;
        renderer.attribute_flags.y = 0.012f32.to_bits();
        renderer.tint = Vec4::ONE;
        renderer.particle_color = 1;
        renderer.textured = 0;
        renderer.softness = 0.2;
        let renderers = h.buffer(encode(&vec![renderer]), false);
        let globals = h.buffer(
            encode(&GpuRenderGlobals {
                time: 1.0,
                seed,
                ..Default::default()
            }),
            false,
        );
        let mut records = vec![GpuParticle::default(); (1 + OWNERS * POINTS) as usize];
        records[0] = GpuParticle {
            packed_emitter_alive: 1,
            rotation: 1.0,
            particle_index: seed,
            size: 1.0,
            position: Vec3::splat(-1.0),
            color: Vec4::ONE,
            ..Default::default()
        };
        let mut words = vec![0u32; records.len() * 3];
        words[0] = 1;
        for i in 0..active {
            // Non-contiguous sparse owners exercise prefix offsets. Dense owners overlap in
            // four differently colored layers, making ordering errors visible with alpha blending.
            let owner = owner_slot(i, active);
            let base = (1 + owner * POINTS) as usize;
            let tile = i % 256;
            let origin = Vec3::new(
                (tile % 16) as f32 * 0.11 - 0.88,
                (tile / 16) as f32 * 0.11 - 0.85,
                0.5,
            );
            let color = Vec4::new(
                0.2 + (i % 7) as f32 * 0.1,
                0.3 + (i % 5) as f32 * 0.12,
                0.9 - (i % 3) as f32 * 0.2,
                0.65,
            );
            let point = |t: f32| GpuParticle {
                packed_emitter_alive: 1,
                position: origin + Vec3::new(t * 0.09, (t * 6.0).sin() * 0.025, 0.0),
                rotation: 0.5 + t * 0.5,
                size: 1.0,
                color,
                ..Default::default()
            };
            records[base] = point(1.0);
            words[base * 3 + 1] = POINTS - 1;
            for p in 1..POINTS as usize {
                let t = (p - 1) as f32 / (POINTS - 1) as f32;
                records[base + p] = point(t);
                words[(base + p) * 3] = t.to_bits();
            }
            words[(base + 1) * 3 + 1] = 1.0f32.to_bits();
        }
        let particles = h.buffer(encode(&records), false);
        let aux = h.buffer(encode(&words), false);
        let params = h.buffer(encode(&UVec4::new(0, CANDIDATES, OWNERS, POINTS - 1)), true);
        let scratch = h.buffer(encode(&vec![0u32; (CANDIDATES + OWNERS) as usize]), false);
        let full = h.buffer(encode(&vec![4u32, CANDIDATES, 0, 0]), false);
        let output = h.buffer(encode(&vec![0u32; (CANDIDATES + 4) as usize]), false);
        let compact = group(
            &h.device,
            &h.compact_layout,
            &[
                &particles, &renderers, &globals, &aux, &params, &scratch, &output,
            ],
        );
        let rendering = [Path::Full, Path::Compact].map(|path| {
            let params = h.buffer(encode(&UVec4::new(0, 0, path.index() as u32, 0)), false);
            h.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &h.render.get_bind_group_layout(1),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: renderers.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: particles.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: if path == Path::Compact {
                            output.as_entire_binding()
                        } else {
                            full.as_entire_binding()
                        },
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: globals.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(&h.texture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&h.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: aux.as_entire_binding(),
                    },
                ],
            })
        });
        let views = (0..view_count)
            .map(|v| {
                let clip = Mat4::from_scale(Vec3::new(0.9, 0.9, 1.0))
                    * Mat4::from_rotation_z(v as f32 * 0.025);
                let uniform = h.buffer(encode(&[clip, clip, clip.inverse(), Mat4::IDENTITY]), true);
                let camera = group(&h.device, &h.render.get_bind_group_layout(0), &[&uniform]);
                let params = h.buffer(
                    encode(&GpuTrailCullParams {
                        clip_from_world: clip,
                        renderer_index: 0,
                        instance_count: CANDIDATES,
                        epoch: 1,
                        _padding: 0,
                    }),
                    true,
                );
                let indirect = [0, 1].map(|_| h.buffer(encode(&vec![0u32; 4]), false));
                let culling = [Path::Full, Path::Compact].map(|path| {
                    group(
                        &h.device,
                        &h.cull.get_bind_group_layout(0),
                        &[
                            &particles,
                            &renderers,
                            &params,
                            &indirect[path.index()],
                            &globals,
                            &aux,
                            if path == Path::Compact {
                                &output
                            } else {
                                &full
                            },
                        ],
                    )
                });
                let target = h.device.create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: WIDTH,
                        height: HEIGHT,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                });
                let attachment = target.create_view(&Default::default());
                View {
                    camera,
                    culling,
                    indirect,
                    target,
                    attachment,
                }
            })
            .collect();
        Self {
            compact,
            rendering,
            views,
            expected: active * (POINTS - 1),
        }
    }

    fn run(&self, h: &Harness, path: Path, views: usize) -> [u64; 4] {
        let queries = &h.queries;
        let resolve = &h.resolve;
        let readback = &h.readback;
        let mut encoder = h.device.create_command_encoder(&Default::default());
        if path == Path::Compact {
            for (stage, pipeline) in h.compact.iter().enumerate() {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: (stage != 1).then_some(wgpu::ComputePassTimestampWrites {
                        query_set: queries,
                        beginning_of_pass_write_index: (stage == 0).then_some(0),
                        end_of_pass_write_index: (stage == 2).then_some(1),
                    }),
                });
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &self.compact, &[]);
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
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: queries,
                    beginning_of_pass_write_index: Some(2),
                    end_of_pass_write_index: Some(3),
                }),
            });
            pass.set_pipeline(&h.cull);
            for view in self.views.iter().take(views) {
                pass.set_bind_group(0, &view.culling[path.index()], &[]);
                pass.dispatch_workgroups(1, 1, 1);
            }
        }
        for (v, view) in self.views.iter().take(views).enumerate() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view.attachment,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                    query_set: queries,
                    beginning_of_pass_write_index: Some(4 + v as u32 * 2),
                    end_of_pass_write_index: Some(5 + v as u32 * 2),
                }),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&h.render);
            pass.set_bind_group(0, &view.camera, &[]);
            pass.set_bind_group(1, &self.rendering[path.index()], &[]);
            pass.draw_indirect(&view.indirect[path.index()], 0);
        }
        // Resolve only written queries: full-range drawing has no compaction queries.
        let first = if path == Path::Compact { 0 } else { 2 };
        let end = 4 + views as u32 * 2;
        let bytes = u64::from(end - first) * 8;
        for (index, view) in self.views.iter().take(views).enumerate() {
            encoder.copy_buffer_to_buffer(
                &view.indirect[path.index()],
                0,
                readback,
                TIMESTAMP_BYTES + index as u64 * 16,
                16,
            );
        }
        let commands =
            super::timestamps::finish(&h.device, encoder, queries, first..end, resolve, readback);
        let data = h.map(readback, h.queue.submit(commands));
        for view in 0..views {
            let offset = TIMESTAMP_BYTES as usize + view * 16;
            let draw: Vec<_> = data[offset..offset + 16]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_le_bytes(*bytes))
                .collect();
            assert_eq!(
                draw,
                [
                    4,
                    if path == Path::Compact {
                        self.expected
                    } else {
                        CANDIDATES
                    },
                    0,
                    0
                ],
                "{path:?}, view {view}: incorrect draw command"
            );
        }
        let ticks: Vec<_> = data[..bytes as usize]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        // Reject incomplete/reordered observations, rather than publishing zero or
        // underflowed durations. Backend timestamp ordering is not universally guaranteed.
        assert!(
            super::timestamps::valid_sequence(&ticks),
            "invalid GPU timestamp sequence for {path:?}: {ticks:?}; timing is unavailable on this adapter/backend"
        );
        let duration = |start: u32, end: u32| {
            (ticks[(end - first) as usize]
                .checked_sub(ticks[(start - first) as usize])
                .expect("validated timestamp sequence") as f64
                * f64::from(h.queue.get_timestamp_period()))
            .round() as u64
        };
        let compaction = if path == Path::Compact {
            duration(0, 1)
        } else {
            0
        };
        let drawing = (0..views as u32)
            .map(|v| duration(4 + v * 2, 5 + v * 2))
            .sum();
        [
            duration(first, end - 1),
            compaction,
            duration(2, 3),
            drawing,
        ]
    }

    fn images(&self, h: &Harness, views: usize) -> Vec<u8> {
        let size = u64::from(WIDTH * HEIGHT * 4);
        let buffer = h.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size * views as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = h.device.create_command_encoder(&Default::default());
        for (v, view) in self.views.iter().take(views).enumerate() {
            encoder.copy_texture_to_buffer(
                view.target.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: size * v as u64,
                        bytes_per_row: Some(WIDTH * 4),
                        rows_per_image: Some(HEIGHT),
                    },
                },
                view.target.size(),
            );
        }
        h.map(&buffer, h.queue.submit([encoder.finish()]))
    }
}

pub(super) fn run(config: &Config) -> Report {
    let h = Harness::new();
    let experiment = if config.gpu_trails.as_deref() == Some("sweep") {
        "trail-occupancy-view-sweep"
    } else {
        "trail-rendering-ab"
    };
    let mut report = Report::new(experiment, config, h.adapter.clone());
    report.target_size = Some([WIDTH, HEIGHT]);
    println!(
        "render_ab,width={WIDTH},height={HEIGHT},warmup={},samples={},alpha_blending=true",
        config.warmup, config.frames
    );
    println!(
        "case,views,path,candidates_per_view,submitted_per_view,total_median_ns,total_p95_ns,compaction_median_ns,culling_median_ns,draw_sum_median_ns"
    );
    for &active in &config.trail_owners {
        let case = format!("owners-{active}");
        let scene = Scene::new(
            &h,
            active,
            config.seed as u32,
            *config.trail_views.iter().max().unwrap(),
        );
        for &views in &config.trail_views {
            let mut samples: [Vec<[u64; 4]>; 2] = Default::default();
            let mut images: [Option<Vec<u8>>; 2] = Default::default();
            for frame in 0..config.warmup + config.frames {
                // Paired A/B and B/A iterations limit warmup/order/thermal bias.
                let order = if frame % 2 == 0 {
                    [Path::Compact, Path::Full]
                } else {
                    [Path::Full, Path::Compact]
                };
                for path in order {
                    let times = scene.run(&h, path, views);
                    if frame >= config.warmup {
                        samples[path.index()].push(times);
                    }
                    if frame == config.warmup {
                        images[path.index()] = Some(scene.images(&h, views));
                    }
                }
            }
            let full = images[0].as_ref().unwrap();
            let compact = images[1].as_ref().unwrap();
            assert_eq!(full.len(), compact.len());
            let differing = full.iter().zip(compact).filter(|(a, b)| a != b).count();
            assert_eq!(differing, 0, "{case}, {views} views: changed image bytes");
            for image in full.as_chunks::<{ (WIDTH * HEIGHT * 4) as usize }>().0 {
                let visible = image
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .filter(|p| p[3] > 0 && p[..3].iter().any(|v| *v > 0))
                    .count();
                assert!(visible > 100, "comparison must not accept blank rendering");
            }
            for path in [Path::Full, Path::Compact] {
                let timing =
                    |field| Timing::new(samples[path.index()].iter().map(|s| s[field]).collect());
                let total = timing(0);
                let compaction = timing(1);
                let culling = timing(2);
                let drawing = timing(3);
                let submitted = if path == Path::Full {
                    CANDIDATES
                } else {
                    scene.expected
                };
                println!(
                    "{case},{views},{path:?},{CANDIDATES},{},{},{},{},{},{}",
                    submitted,
                    total.median_ns,
                    total.p95_ns,
                    compaction.median_ns,
                    culling.median_ns,
                    drawing.median_ns
                );
                report.cases.push(CaseReport {
                    case: case.clone(),
                    active_owners: active,
                    occupancy_percent: f64::from(active) * 100.0 / f64::from(OWNERS),
                    views,
                    path: format!("{path:?}").to_lowercase(),
                    candidates_per_view: CANDIDATES,
                    submitted_per_view: submitted,
                    total: Some(total),
                    compaction,
                    culling,
                    drawing: Some(drawing),
                    image_equivalence: Some(true),
                });
            }
            let comparison = Comparison::new(active, OWNERS, views, &samples[0], &samples[1]);
            println!(
                "comparison,{active},{views},median_saving_percent={:?},p95_saving_percent={:?},paired_median_saving_ns={}",
                comparison.median_saving_percent,
                comparison.p95_saving_percent,
                comparison.paired_median_saving_ns
            );
            report.comparisons.push(comparison);
            println!("image_equivalence,{case},{views},identical_nonblank=true");
        }
    }
    report
}

fn owner_slot(index: u32, active: u32) -> u32 {
    index * OWNERS / active
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_occupancies_spread_unique_owners_across_the_pool() {
        for active in 1..=OWNERS {
            let slots: Vec<_> = (0..active).map(|i| owner_slot(i, active)).collect();
            assert!(slots.iter().all(|slot| *slot < OWNERS));
            assert!(slots.windows(2).all(|pair| pair[0] < pair[1]));
        }
        assert_eq!(owner_slot(50, 51), 1003);
    }
    #[test]
    fn eight_views_fit_the_timestamp_and_indirect_readback_regions() {
        assert_eq!(QUERY_COUNT, 20);
        assert_eq!(TIMESTAMP_BYTES, 160);
        assert_eq!(4 + 2 * crate::MAX_TRAIL_VIEWS as u32, QUERY_COUNT);
    }
}
