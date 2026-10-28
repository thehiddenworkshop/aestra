//! Blocking, test-only GPU transport for the partition feasibility oracle.
use super::*;
use bevy::{asset::RenderAssetUsages, image::Image, prelude::ColorToComponents};
use std::{borrow::Cow, path::Path, sync::mpsc, time::Duration};
use wgpu::{util::DeviceExt, *};

const SIZE: [u32; 2] = [257, 193];
const LAYERS: u32 = 48;
const BACKGROUND: [f32; 3] = [0.03125, 0.0625, 0.125];

struct Gpu {
    device: Device,
    queue: Queue,
    capture: RenderPipeline,
    sort: ComputePipeline,
    resolve: RenderPipeline,
    opaque: RenderPipeline,
    layers: Buffer,
    depth: Texture,
    depth_view: TextureView,
    output: Texture,
    output_view: TextureView,
}
struct Pool {
    nodes: Buffer,
    heads: Buffer,
    counter: Buffer,
    view: Buffer,
    capture: BindGroup,
    capture_depth: BindGroup,
    sorts: Vec<BindGroup>,
    resolve: BindGroup,
    resolve_depth: BindGroup,
    opaque: BindGroup,
    query: QuerySet,
    query_result: Buffer,
    readback: Buffer,
    rects: Vec<Rect>,
    capacity: u32,
    image_offset: u64,
    query_offset: u64,
}
struct Frame {
    image: Vec<u8>,
    attempts: Vec<u32>,
    phases_ms: [f64; 4],
}

fn data(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    values.into_iter().flat_map(u32::to_le_bytes).collect()
}
fn buffer(device: &Device, label: &str, bytes: &[u8], usage: BufferUsages) -> Buffer {
    device.create_buffer_init(&util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage,
    })
}
fn layer_values() -> Vec<[[f32; 4]; 3]> {
    (0..LAYERS)
        .map(|i| {
            let rect = match i % 4 {
                0 => [-1., -1., 1., 1.],
                1 => [-1., -0.75, 0.12, 1.],
                2 => [-0.12, -1., 1., 0.75],
                _ => [-0.65, -0.5, 0.72, 0.6],
            };
            let color = match i % 3 {
                0 => [0.03125, 0.015625, 0.0078125, 10. / 255.],
                1 => [0.03125, 0.125, 0.5, 0.],
                _ => [0., 0., 0., 4. / 255.],
            };
            [
                rect,
                color,
                [0.08 + 0.84 * i as f32 / (LAYERS - 1) as f32, 0., 0., 0.],
            ]
        })
        .collect()
}

fn render_pipeline(
    device: &Device,
    label: &str,
    source: &str,
    depth: bool,
    write: bool,
) -> RenderPipeline {
    let module = device.create_shader_module(ShaderModuleDescriptor {
        label: Some(label),
        source: ShaderSource::Wgsl(Cow::Borrowed(source)),
    });
    let targets = [Some(ColorTargetState {
        format: TextureFormat::Rgba16Float,
        blend: Some(BlendState {
            color: BlendComponent::OVER,
            alpha: BlendComponent::OVER,
        }),
        write_mask: if write {
            ColorWrites::ALL
        } else {
            ColorWrites::empty()
        },
    })];
    device.create_render_pipeline(&RenderPipelineDescriptor {
        label: Some(label),
        layout: None,
        vertex: VertexState {
            module: &module,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(FragmentState {
            module: &module,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: if depth { &[] } else { &targets },
        }),
        primitive: PrimitiveState::default(),
        depth_stencil: depth.then_some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    })
}

impl Gpu {
    fn new() -> (Self, AdapterInfo) {
        let instance = Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .expect("native GPU required; no silent skip");
        assert!(
            adapter.features().contains(Features::TIMESTAMP_QUERY),
            "timestamp support required for this explicit cost probe"
        );
        let info = adapter.get_info();
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            required_features: Features::TIMESTAMP_QUERY,
            required_limits: Limits::default(),
            ..Default::default()
        }))
        .unwrap();
        let [capture, sort, resolve] = shaders();
        let capture = render_pipeline(&device, "partition capture", &capture, false, false);
        let module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("production overflow sort"),
            source: ShaderSource::Wgsl(Cow::Owned(sort)),
        });
        let sort = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("partition sort"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let resolve = render_pipeline(&device, "partition resolve", &resolve, false, true);
        let opaque_source = format!(
            r#"{VIEW}
@group(0) @binding(0) var<uniform> view: View;
@vertex fn vertex(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {{
    return vec4(array(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.))[id],0.,1.);
}}
@fragment fn fragment(@builtin(position) p: vec4<f32>) -> @builtin(frag_depth) f32 {{
    let xy = vec2<u32>(p.xy); let size = vec2<u32>(view.viewport.zw);
    if xy.x < size.x / 2u && xy.y >= size.y / 3u && xy.y < 2u * size.y / 3u {{ return 0.61; }}
    return 0.0;
}}"#
        );
        let opaque = render_pipeline(&device, "opaque depth fixture", &opaque_source, true, false);
        let values = data(
            layer_values()
                .into_iter()
                .flatten()
                .flatten()
                .map(f32::to_bits),
        );
        let layers = buffer(
            &device,
            "immutable fixture layers",
            &values,
            BufferUsages::STORAGE,
        );
        let size = Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        };
        let depth = device.create_texture(&TextureDescriptor {
            label: Some("fixture opaque depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let output = device.create_texture(&TextureDescriptor {
            label: Some("linear HDR oracle target"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba16Float,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let output_view = output.create_view(&Default::default());
        (
            Self {
                device,
                queue,
                capture,
                sort,
                resolve,
                opaque,
                layers,
                depth,
                depth_view,
                output,
                output_view,
            },
            info,
        )
    }

    fn pool(&self, grid: [u32; 2], capacity: u32) -> Pool {
        let rects = regions(SIZE, grid, false, false).unwrap();
        let make = |label, size| {
            self.device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let nodes = make("reused node pool", u64::from(capacity) * 12);
        let heads = make("full target heads", u64::from(SIZE[0] * SIZE[1]) * 4);
        let counter = make("reused allocation counter", 4);
        let view = buffer(
            &self.device,
            "full projection; logical capacity",
            &data([
                0,
                0,
                (SIZE[0] as f32).to_bits(),
                (SIZE[1] as f32).to_bits(),
                capacity,
                0,
                0,
                0,
            ]),
            BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        );
        let settings = buffer(
            &self.device,
            "local sort threshold",
            &data([8, 0, 0, 0]),
            BufferUsages::UNIFORM,
        );
        let bind = |label, layout: &BindGroupLayout, entries: &[BindGroupEntry<'_>]| {
            self.device.create_bind_group(&BindGroupDescriptor {
                label: Some(label),
                layout,
                entries,
            })
        };
        let capture = bind(
            "capture pool",
            &self.capture.get_bind_group_layout(0),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: nodes.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: heads.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: counter.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: self.layers.as_entire_binding(),
                },
            ],
        );
        let capture_depth = bind(
            "capture depth",
            &self.capture.get_bind_group_layout(1),
            &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&self.depth_view),
            }],
        );
        let sorts = rects
            .iter()
            .map(|r| {
                let region = buffer(
                    &self.device,
                    "immutable dispatch region",
                    &data([r.x, r.y, r.w, r.h]),
                    BufferUsages::UNIFORM,
                );
                bind(
                    "sort pool/region",
                    &self.sort.get_bind_group_layout(0),
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: view.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: settings.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: nodes.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: heads.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 4,
                            resource: region.as_entire_binding(),
                        },
                    ],
                )
            })
            .collect();
        let resolve = bind(
            "resolve pool",
            &self.resolve.get_bind_group_layout(0),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: view.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: nodes.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: heads.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: counter.as_entire_binding(),
                },
            ],
        );
        let resolve_depth = bind(
            "resolve depth",
            &self.resolve.get_bind_group_layout(1),
            &[BindGroupEntry {
                binding: 0,
                resource: BindingResource::TextureView(&self.depth_view),
            }],
        );
        let opaque = bind(
            "opaque view",
            &self.opaque.get_bind_group_layout(0),
            &[BindGroupEntry {
                binding: 0,
                resource: view.as_entire_binding(),
            }],
        );
        let queries = rects.len() as u32 * 6;
        let query = self.device.create_query_set(&QuerySetDescriptor {
            label: Some("phase boundaries"),
            ty: QueryType::Timestamp,
            count: queries,
        });
        let query_result = self.device.create_buffer(&BufferDescriptor {
            label: Some("query resolve"),
            size: u64::from(queries) * 8,
            usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let image_offset = (rects.len() as u64 * 4 + 4 + heads.size()).div_ceil(256) * 256;
        let query_offset = (image_offset + row_bytes() * u64::from(SIZE[1])).div_ceil(256) * 256;
        let readback = self.device.create_buffer(&BufferDescriptor {
            label: Some("bounded probe readback"),
            size: query_offset + u64::from(queries) * 8,
            usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Pool {
            nodes,
            heads,
            counter,
            view,
            capture,
            capture_depth,
            sorts,
            resolve,
            resolve_depth,
            opaque,
            query,
            query_result,
            readback,
            rects,
            capacity,
            image_offset,
            query_offset,
        }
    }

    fn frame(&self, pool: &Pool, logical_capacity: u32, layer_count: u32, reversed: bool) -> Frame {
        self.frame_mode(pool, logical_capacity, layer_count, reversed, true)
    }

    fn frame_mode(
        &self,
        pool: &Pool,
        logical_capacity: u32,
        layer_count: u32,
        reversed: bool,
        detailed: bool,
    ) -> Frame {
        assert!(logical_capacity > 0 && logical_capacity <= pool.capacity && layer_count <= LAYERS);
        self.queue
            .write_buffer(&pool.view, 16, &logical_capacity.to_le_bytes());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let attachment = |clear| {
            Some(RenderPassColorAttachment {
                view: &self.output_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: if clear {
                        LoadOp::Clear(Color {
                            r: f64::from(BACKGROUND[0]),
                            g: f64::from(BACKGROUND[1]),
                            b: f64::from(BACKGROUND[2]),
                            a: 1.,
                        })
                    } else {
                        LoadOp::Load
                    },
                    store: StoreOp::Store,
                },
            })
        };
        {
            let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("initialize HDR background"),
                color_attachments: &[attachment(true)],
                ..Default::default()
            });
        }
        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("opaque depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(0.),
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });
            pass.set_pipeline(&self.opaque);
            pass.set_bind_group(0, &pool.opaque, &[]);
            pass.draw(0..3, 0..1);
        }
        for (i, r) in pool.rects.iter().enumerate() {
            let q = i as u32 * 6;
            encoder.clear_buffer(&pool.counter, 0, None);
            {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("scissored capture; full projection"),
                    color_attachments: &[attachment(false)],
                    timestamp_writes: (detailed || i == 0).then_some(RenderPassTimestampWrites {
                        query_set: &pool.query,
                        beginning_of_pass_write_index: Some(if detailed { q } else { 0 }),
                        end_of_pass_write_index: detailed.then_some(q + 1),
                    }),
                    ..Default::default()
                });
                pass.set_pipeline(&self.capture);
                pass.set_bind_group(0, &pool.capture, &[]);
                pass.set_bind_group(1, &pool.capture_depth, &[]);
                pass.set_scissor_rect(r.x, r.y, r.w, r.h);
                for step in 0..layer_count {
                    let layer = if reversed {
                        layer_count - 1 - step
                    } else {
                        step
                    };
                    pass.draw(0..6, layer..layer + 1);
                }
            }
            if detailed {
                encoder.copy_buffer_to_buffer(&pool.counter, 0, &pool.readback, i as u64 * 4, 4);
            }
            {
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("production link sort; region dispatch"),
                    timestamp_writes: detailed.then_some(ComputePassTimestampWrites {
                        query_set: &pool.query,
                        beginning_of_pass_write_index: Some(q + 2),
                        end_of_pass_write_index: Some(q + 3),
                    }),
                });
                pass.set_pipeline(&self.sort);
                pass.set_bind_group(0, &pool.sorts[i], &[]);
                pass.dispatch_workgroups(r.w.div_ceil(8), r.h.div_ceil(8), 1);
            }
            {
                let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("scissored production resolve"),
                    color_attachments: &[attachment(false)],
                    timestamp_writes: (detailed || i + 1 == pool.rects.len()).then_some(
                        RenderPassTimestampWrites {
                            query_set: &pool.query,
                            beginning_of_pass_write_index: detailed.then_some(q + 4),
                            end_of_pass_write_index: Some(if detailed { q + 5 } else { 1 }),
                        },
                    ),
                    ..Default::default()
                });
                pass.set_pipeline(&self.resolve);
                pass.set_bind_group(0, &pool.resolve, &[]);
                pass.set_bind_group(1, &pool.resolve_depth, &[]);
                pass.set_scissor_rect(r.x, r.y, r.w, r.h);
                pass.draw(0..3, 0..1);
            }
        }
        let stats_bytes = pool.rects.len() as u64 * 4;
        encoder.copy_buffer_to_buffer(&pool.counter, 0, &pool.readback, stats_bytes, 4);
        encoder.copy_buffer_to_buffer(
            &pool.heads,
            0,
            &pool.readback,
            stats_bytes + 4,
            pool.heads.size(),
        );
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: &self.output,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &pool.readback,
                layout: TexelCopyBufferLayout {
                    offset: pool.image_offset,
                    bytes_per_row: Some(row_bytes() as u32),
                    rows_per_image: Some(SIZE[1]),
                },
            },
            Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
        );
        let queries = if detailed {
            pool.rects.len() as u32 * 6
        } else {
            2
        };
        encoder.resolve_query_set(&pool.query, 0..queries, &pool.query_result, 0);
        encoder.copy_buffer_to_buffer(
            &pool.query_result,
            0,
            &pool.readback,
            pool.query_offset,
            u64::from(queries) * 8,
        );
        let submission = self.queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        pool.readback
            .slice(..)
            .map_async(MapMode::Read, move |r| tx.send(r).unwrap());
        self.device
            .poll(PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(Duration::from_secs(60)),
            })
            .unwrap();
        rx.recv_timeout(Duration::from_secs(60)).unwrap().unwrap();
        let bytes = pool.readback.slice(..).get_mapped_range().unwrap();
        assert!(
            bytes[stats_bytes as usize..(stats_bytes + 4 + pool.heads.size()) as usize]
                .iter()
                .all(|b| *b == 0),
            "resolve left stale heads/counter"
        );
        let attempts = if detailed {
            bytes[..stats_bytes as usize]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect()
        } else {
            Vec::new()
        };
        let image = (0..SIZE[1] as usize)
            .flat_map(|y| {
                let start = pool.image_offset as usize + y * row_bytes() as usize;
                bytes[start..start + SIZE[0] as usize * 8].to_vec()
            })
            .collect();
        let ticks: Vec<u64> = bytes
            [pool.query_offset as usize..(pool.query_offset + u64::from(queries) * 8) as usize]
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        let scale = f64::from(self.queue.get_timestamp_period()) / 1e6;
        let mut phases_ms = [0.; 4];
        for q in ticks.as_chunks::<6>().0 {
            for p in 0..3 {
                assert!(q[p * 2 + 1] >= q[p * 2]);
                phases_ms[p] += (q[p * 2 + 1] - q[p * 2]) as f64 * scale;
            }
        }
        assert!(ticks.last().unwrap() >= &ticks[0]);
        phases_ms[3] = (ticks.last().unwrap() - ticks[0]) as f64 * scale;
        drop(bytes);
        pool.readback.unmap();
        Frame {
            image,
            attempts,
            phases_ms,
        }
    }
}
fn row_bytes() -> u64 {
    u64::from(SIZE[0] * 8).div_ceil(256) * 256
}
fn dropped(frame: &Frame, capacity: u32) -> u64 {
    frame
        .attempts
        .iter()
        .map(|a| u64::from(a.saturating_sub(capacity)))
        .sum()
}
fn attempted(frame: &Frame) -> u64 {
    frame.attempts.iter().map(|v| u64::from(*v)).sum()
}
fn image(bytes: Vec<u8>) -> Image {
    Image::new(
        Extent3d {
            width: SIZE[0],
            height: SIZE[1],
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba16Float,
        RenderAssetUsages::default(),
    )
}
fn analytic_error(bytes: &[u8]) -> f32 {
    let image = image(bytes.to_vec());
    let mut maximum = 0.0_f32;
    for (x, y) in [
        (0, 0),
        (256, 192),
        (64, 96),
        (192, 96),
        (128, 96),
        (128, 32),
        (128, 160),
        (64, 48),
        (192, 144),
    ] {
        let ndc = [
            2. * (x as f32 + 0.5) / SIZE[0] as f32 - 1.,
            1. - 2. * (y as f32 + 0.5) / SIZE[1] as f32,
        ];
        let opaque = if x < SIZE[0] / 2 && y >= SIZE[1] / 3 && y < 2 * SIZE[1] / 3 {
            0.61
        } else {
            0.
        };
        let mut layers = layer_values();
        layers.sort_by(|a, b| b[2][0].total_cmp(&a[2][0]));
        let mut rgba = [0.; 4];
        for [rect, color, info] in layers {
            if ndc[0] < rect[0]
                || ndc[0] >= rect[2]
                || ndc[1] < rect[1]
                || ndc[1] >= rect[3]
                || info[0] < opaque
            {
                continue;
            }
            for c in 0..3 {
                rgba[c] += (1. - rgba[3]) * color[c];
            }
            rgba[3] += (1. - rgba[3]) * color[3];
        }
        let actual = image.get_color_at(x, y).unwrap().to_linear().to_f32_array();
        for c in 0..3 {
            maximum = maximum.max((actual[c] - (rgba[c] + (1. - rgba[3]) * BACKGROUND[c])).abs());
        }
    }
    maximum
}
fn percentiles(values: &[f64]) -> serde_json::Value {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    let at = |q: f64| values[((values.len() - 1) as f64 * q).ceil() as usize];
    json!({"samples":values.len(),"p50_ms":at(0.5),"p95_ms":at(0.95),"max_ms":at(1.)})
}

pub(super) fn run(root: &Path) -> serde_json::Value {
    let (gpu, adapter) = Gpu::new();
    let pixels = SIZE[0] * SIZE[1];
    let full = gpu.pool([1, 1], pixels * LAYERS);
    let reference = gpu.frame(&full, full.capacity, LAYERS, false);
    assert_eq!(dropped(&reference, full.capacity), 0);
    let reference_capacity = full.capacity;
    let reference_demand: u32 = attempted(&reference).try_into().unwrap();
    let error = analytic_error(&reference.image);
    assert!(error < 0.01, "invalid HDR reference: {error}");
    fs::write(root.join("reference.rgba16"), &reference.image).unwrap();
    full.nodes.destroy();
    full.heads.destroy();
    full.counter.destroy();
    drop(full);
    let mut rows = Vec::new();
    for (name, grid, capacity, healthy) in [
        ("full-reference", [1, 1], reference_capacity, true),
        ("full-tight-control", [1, 1], reference_demand, true),
        ("full-small-negative", [1, 1], pixels * 4, false),
        ("four-regions", [2, 2], 129 * 97 * LAYERS, true),
        ("sixteen-regions", [4, 4], pixels * 4, true),
    ] {
        let pool = gpu.pool(grid, capacity);
        let mut frame = gpu.frame(&pool, pool.capacity, LAYERS, false);
        let losses = dropped(&frame, pool.capacity);
        assert_eq!(
            attempted(&frame),
            u64::from(reference_demand),
            "scissor changed coverage: {name}"
        );
        if healthy {
            assert_eq!(losses, 0);
            assert_eq!(
                frame.image, reference.image,
                "partition seam/order mismatch: {name}"
            );
        } else {
            assert!(losses > 0);
            assert_ne!(
                frame.image, reference.image,
                "lossy negative incorrectly passed"
            );
        }
        let mut repeat = true;
        for reversed in [true, false] {
            let next = gpu.frame(&pool, pool.capacity, LAYERS, reversed);
            assert_eq!(attempted(&next), u64::from(reference_demand));
            if healthy {
                assert_eq!(dropped(&next, pool.capacity), 0);
                assert_eq!(
                    next.image, reference.image,
                    "arrival-order mismatch: {name}"
                );
            }
            repeat &= next.image == frame.image;
            frame = next;
        }
        let mut timings: Vec<[f64; 4]> = Vec::new();
        for i in 0..35 {
            let f = gpu.frame(&pool, pool.capacity, LAYERS, false);
            assert_eq!(attempted(&f), u64::from(reference_demand));
            if healthy {
                assert_eq!(dropped(&f, pool.capacity), 0);
                assert_eq!(f.image, reference.image);
            }
            if i >= 5 {
                timings.push(f.phases_ms);
            }
        }
        // Constant two-query span; omit per-region attempt copies and detailed
        // timestamps. Never interpret missing cost-lane counters as zero loss.
        let mut aggregate_only = Vec::new();
        for i in 0..35 {
            let f = gpu.frame_mode(&pool, pool.capacity, LAYERS, false, false);
            assert!(f.attempts.is_empty());
            if healthy {
                assert_eq!(f.image, reference.image);
            }
            if i >= 5 {
                aggregate_only.push(f.phases_ms[3]);
            }
        }
        fs::write(root.join(format!("{name}.rgba16")), &frame.image).unwrap();
        let before = pool.nodes.size() + pool.heads.size();
        if healthy {
            let overflow = gpu.frame(&pool, 8, LAYERS, false);
            assert!(dropped(&overflow, 8) > 0);
            let restored = gpu.frame(&pool, pool.capacity, LAYERS, false);
            assert_eq!(dropped(&restored, pool.capacity), 0);
            assert_eq!(restored.image, reference.image, "same-pool recovery failed");
            let empty = gpu.frame(&pool, pool.capacity, 0, false);
            assert!(empty.attempts.iter().all(|a| *a == 0));
            assert!(
                empty
                    .image
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .all(|p| p == &empty.image[..8])
            );
            let empty_image = image(empty.image);
            let center = empty_image
                .get_color_at(128, 96)
                .unwrap()
                .to_linear()
                .to_f32_array();
            assert_eq!(&center[..3], &BACKGROUND);
            assert_eq!(pool.nodes.size() + pool.heads.size(), before);
        }
        rows.push(json!({"name":name,"grid":grid,"regions":pool.rects.len(),"node_capacity":capacity,
            "node_head_bytes":before,"attempts_sum":frame.attempts.iter().map(|v|u64::from(*v)).sum::<u64>(),"attempts_peak_region":frame.attempts.iter().max(),"dropped_sum":losses,
            "expected_healthy":healthy,"exact_reference_match":frame.image==reference.image,"repeat_image_match":repeat,
            "draw_submissions":u64::from(LAYERS)*pool.rects.len() as u64,"submitted_vertices":u64::from(LAYERS)*6*pool.rects.len() as u64,
            "capture":percentiles(&timings.iter().map(|t|t[0]).collect::<Vec<_>>()),"sort":percentiles(&timings.iter().map(|t|t[1]).collect::<Vec<_>>()),"resolve":percentiles(&timings.iter().map(|t|t[2]).collect::<Vec<_>>()),"capture_through_resolve":percentiles(&timings.iter().map(|t|t[3]).collect::<Vec<_>>()),
            "same_pool_overflow_recovery_and_empty":healthy}));
        let row = rows.last_mut().unwrap();
        row["aggregate_only"] = percentiles(&aggregate_only);
        row["aggregate_timestamp_writes"] = json!(2);
        row["detailed_timestamp_writes"] = json!(pool.rects.len() * 6);
        pool.nodes.destroy();
        pool.heads.destroy();
        pool.counter.destroy();
    }
    // A fresh pool after destruction must not inherit any linked state.
    let fresh = gpu.pool([4, 4], pixels * 4);
    let restored = gpu.frame(&fresh, fresh.capacity, LAYERS, false);
    assert_eq!(restored.image, reference.image);
    assert_eq!(dropped(&restored, fresh.capacity), 0);
    fresh.nodes.destroy();
    fresh.heads.destroy();
    fresh.counter.destroy();
    gpu.depth.destroy();
    gpu.output.destroy();
    json!({"passed":true,"adapter":{"name":adapter.name,"backend":format!("{:?}",adapter.backend)},"size":SIZE,"layers":LAYERS,"format":"Rgba16Float","analytic_patch_max_error":error,"cases":rows,
        "cleanup":"Every frame checks zero heads/counter after resolve, including overflow. Healthy pools recover in place, drain empty, and retain allocation size. Buffers are explicitly destroyed after completed GPU submissions; fresh-pool output matches. Not driver VRAM/free-history proof.",
        "scope":"Isolated native raster/scissor experiment using production sort/resolve bodies with fixture interfaces and Bevy-0.20 codec. Full projection, odd extent, opaque depth, premultiplied alpha and additive HDR. Detailed timing uses six queries and an attempt copy per region; aggregate-only uses two queries/frame and no per-region counter copies. Missing aggregate counters are not zero loss; that lane checks images and final reset only. Spans exclude initialization, opaque fixture, CPU work and post-pass readback; inter-region reset clears remain included. No simulation, material/light evaluation, actual Bevy phase integration, foreign transparency, MSAA, arbitrary views or game-performance acceptance."})
}
