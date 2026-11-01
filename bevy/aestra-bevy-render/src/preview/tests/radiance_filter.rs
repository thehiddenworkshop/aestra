//! Actual production mip shader: constant radiance and sparse HDR energy.
use std::{borrow::Cow, sync::mpsc, time::Duration};
use wgpu::*;

#[test]
#[ignore = "native HDR radiance-filter oracle; run alone on a GPU"]
fn native_filter_preserves_constant_radiance_and_edge_impulses_through_odd_mips() {
    let instance = Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))
        .expect("native GPU required");
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let shader = device.create_shader_module(ShaderModuleDescriptor {
        label: Some("production planar HDR filter"),
        source: ShaderSource::Wgsl(Cow::Borrowed(include_str!("../planar/filter.wgsl"))),
    });
    let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
        label: Some("production planar HDR filter"),
        layout: None,
        module: &shader,
        entry_point: Some("downsample"),
        compilation_options: Default::default(),
        cache: None,
    });
    for [width, height] in [[5, 3], [1, 7], [7, 1], [4, 4]] {
        for impulse in [false, true] {
            let size = Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            };
            let levels = width.max(height).ilog2() + 1;
            let texture = device.create_texture(&TextureDescriptor {
                label: Some("HDR energy oracle"),
                size,
                mip_level_count: levels,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba16Float,
                usage: TextureUsages::TEXTURE_BINDING
                    | TextureUsages::STORAGE_BINDING
                    | TextureUsages::COPY_DST
                    | TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            // Powers of two are exactly representable in RGBA16F. The sparse
            // impulse is at the final edge, which naive 2x2 odd reduction loses.
            let mut data = Vec::new();
            for pixel in 0..width * height {
                let value: [u16; 4] = if impulse && pixel + 1 != width * height {
                    [0; 4]
                } else {
                    [0x4c00, 0x4800, 0x4400, 0x3c00]
                };
                data.extend(value.into_iter().flat_map(u16::to_le_bytes));
            }
            queue.write_texture(
                texture.as_image_copy(),
                &data,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 8),
                    rows_per_image: Some(height),
                },
                size,
            );
            let mut encoder = device.create_command_encoder(&Default::default());
            for level in 1..levels {
                let view = |mip| {
                    texture.create_view(&TextureViewDescriptor {
                        base_mip_level: mip,
                        mip_level_count: Some(1),
                        ..Default::default()
                    })
                };
                let input = view(level - 1);
                let output = view(level);
                let group = device.create_bind_group(&BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[
                        BindGroupEntry {
                            binding: 0,
                            resource: BindingResource::TextureView(&input),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(&output),
                        },
                    ],
                });
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(
                    (width >> level).max(1).div_ceil(8),
                    (height >> level).max(1).div_ceil(8),
                    1,
                );
            }
            let readback = device.create_buffer(&BufferDescriptor {
                label: None,
                size: 256,
                usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: levels - 1,
                    origin: Origin3d::ZERO,
                    aspect: TextureAspect::All,
                },
                TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(256),
                        rows_per_image: Some(1),
                    },
                },
                Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
            let submission = queue.submit([encoder.finish()]);
            let (tx, rx) = mpsc::channel();
            readback
                .slice(..)
                .map_async(MapMode::Read, move |result| tx.send(result).unwrap());
            device
                .poll(PollType::Wait {
                    submission_index: Some(submission),
                    timeout: Some(Duration::from_secs(30)),
                })
                .unwrap();
            rx.recv().unwrap().unwrap();
            let mapped = readback.slice(..).get_mapped_range().unwrap();
            let count = if impulse {
                (width * height) as f32
            } else {
                1.0
            };
            for (channel, value) in [16.0, 8.0, 4.0].into_iter().enumerate() {
                let offset = channel * 2;
                let half = u16::from_le_bytes([mapped[offset], mapped[offset + 1]]);
                let actual = f32::from_bits(
                    (((u32::from(half) >> 10) + 112) << 23) | ((u32::from(half) & 1023) << 13),
                );
                let expected = value / count;
                assert!(
                    (actual - expected).abs() <= expected * 0.003,
                    "{width}x{height} impulse={impulse}: {actual} != {expected}"
                );
            }
            drop(mapped);
            readback.unmap();
        }
    }
}
