//! Query resolution and host-readback copies must have a command-buffer boundary.
//! See https://github.com/gfx-rs/wgpu/issues/6406. Keeping resolve and copy in the
//! same encoder can read zero/stale values on Vulkan, even after waiting for submission.

pub(super) fn finish(
    device: &wgpu::Device,
    mut encoder: wgpu::CommandEncoder,
    queries: &wgpu::QuerySet,
    range: std::ops::Range<u32>,
    resolve: &wgpu::Buffer,
    readback: &wgpu::Buffer,
) -> [wgpu::CommandBuffer; 2] {
    let bytes = u64::from(range.end - range.start) * 8;
    encoder.resolve_query_set(queries, range, resolve, 0);
    let work = encoder.finish();
    let mut copy = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("trail benchmark timestamp readback"),
    });
    copy.copy_buffer_to_buffer(resolve, 0, readback, 0, bytes);
    // One submission, no extra CPU wait, and no changes inside the timed workload.
    [work, copy.finish()]
}

pub(super) fn valid_sequence(ticks: &[u64]) -> bool {
    !ticks.is_empty()
        && ticks.iter().all(|tick| *tick != 0)
        && ticks.windows(2).all(|pair| pair[0] <= pair[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_reordered_queries_are_not_measurements() {
        for ticks in [&[][..], &[0, 0], &[10, 0], &[10, 9], &[10, 12, 11, 13]] {
            assert!(!valid_sequence(ticks));
        }
        assert!(valid_sequence(&[10, 10, 12, 13]));
    }

    #[test]
    #[ignore = "native timestamp correctness probe; choose WGPU_BACKEND and run explicitly"]
    fn mixed_pass_queries_resolve_before_readback_copy() {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY);
        let gpu = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(gpu.request_adapter(&Default::default())).unwrap();
        eprintln!("timestamp probe: {:?}", adapter.get_info());
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .unwrap();
        let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: None,
            ty: wgpu::QueryType::Timestamp,
            count: 6,
        });
        let resolve = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 48,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 48,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        });
        let target = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = target.create_view(&Default::default());
        let mut previous_end = 0;
        // Reuse storage to catch stale reads as well as first-use zeroes.
        for iteration in 0..8 {
            let mut encoder = device.create_command_encoder(&Default::default());
            for stage in [0, 2, 4] {
                if stage == 2 {
                    drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        timestamp_writes: Some(wgpu::RenderPassTimestampWrites {
                            query_set: &queries,
                            beginning_of_pass_write_index: Some(stage),
                            end_of_pass_write_index: Some(stage + 1),
                        }),
                        ..Default::default()
                    }));
                } else {
                    drop(encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                            query_set: &queries,
                            beginning_of_pass_write_index: Some(stage),
                            end_of_pass_write_index: Some(stage + 1),
                        }),
                        ..Default::default()
                    }));
                }
            }
            let submission = queue.submit(finish(
                &device,
                encoder,
                &queries,
                0..6,
                &resolve,
                &readback,
            ));
            let (tx, rx) = std::sync::mpsc::channel();
            readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                tx.send(r).unwrap();
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
            let ticks: Vec<_> = bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| u64::from_le_bytes(*b))
                .collect();
            assert!(valid_sequence(&ticks), "iteration {iteration}: {ticks:?}");
            assert!(ticks[0] > previous_end, "stale query results: {ticks:?}");
            previous_end = ticks[5];
            drop(bytes);
            readback.unmap();
        }
    }
}
