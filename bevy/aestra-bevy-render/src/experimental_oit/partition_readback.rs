//! Bounded per-region health readback, independent from the ordinary OIT probe.
use super::*;
use bevy::{core_pipeline::oit::OitBuffers, render::renderer::RenderContext};
use std::sync::atomic::{AtomicBool, Ordering};
use wgpu::util::DeviceExt;

const STRIDE: u64 = 256;
const BYTES: u64 = STRIDE * 64;
struct Slot {
    output: wgpu::Buffer,
    staging: wgpu::Buffer,
    busy: Arc<AtomicBool>,
}

#[derive(Resource, Default)]
pub(super) struct Readbacks(Mutex<Storage>);

#[derive(Default)]
struct Storage {
    pipeline: Option<wgpu::ComputePipeline>,
    slots: Vec<Slot>,
    sequence: u64,
    regions: Option<(Vec<Region>, RegionUniforms)>,
}

/// One immutable aligned buffer shared by sort and health bindings. Recreated
/// only when the admitted region list changes, independent of readback slots.
#[derive(Clone)]
pub(super) struct RegionUniforms {
    pub(super) buffer: wgpu::Buffer,
    pub(super) stride: u64,
}

impl RegionUniforms {
    pub(super) fn binding(&self, index: usize) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.buffer,
            offset: index as u64 * self.stride,
            size: std::num::NonZeroU64::new(16),
        })
    }
}

pub(super) struct Frame {
    output: wgpu::Buffer,
    staging: wgpu::Buffer,
    busy: Arc<AtomicBool>,
    pipeline: wgpu::ComputePipeline,
    groups: Vec<wgpu::BindGroup>,
    sequence: u64,
    capacity: u32,
}

impl Readbacks {
    pub(super) fn regions(&self, context: &RenderContext, regions: &[Region]) -> RegionUniforms {
        let mut storage = self.0.lock().unwrap();
        if let Some((key, uniforms)) = &storage.regions
            && key == regions
        {
            return uniforms.clone();
        }
        let device = context.render_device().wgpu_device();
        let stride = u64::from(device.limits().min_uniform_buffer_offset_alignment).max(16);
        let mut bytes = vec![0; stride as usize * regions.len()];
        for (i, region) in regions.iter().enumerate() {
            for (j, word) in [region.x, region.y, region.w, region.h]
                .into_iter()
                .enumerate()
            {
                let offset = i * stride as usize + j * 4;
                bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
            }
        }
        let uniforms = RegionUniforms {
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Aestra immutable OIT region bounds"),
                contents: &bytes,
                usage: wgpu::BufferUsages::UNIFORM,
            }),
            stride,
        };
        storage.regions = Some((regions.to_vec(), uniforms.clone()));
        uniforms
    }

    pub(super) fn begin(
        &self,
        context: &mut RenderContext,
        buffers: &OitBuffers,
        size: UVec2,
        sorted_max: u32,
        regions: &[Region],
        uniforms: &RegionUniforms,
    ) -> Option<Frame> {
        self.0
            .lock()
            .unwrap()
            .begin(context, buffers, size, sorted_max, regions, uniforms)
    }
}

impl Storage {
    fn begin(
        &mut self,
        context: &mut RenderContext,
        buffers: &OitBuffers,
        size: UVec2,
        sorted_max: u32,
        regions: &[Region],
        uniforms: &RegionUniforms,
    ) -> Option<Frame> {
        let device = context.render_device().wgpu_device();
        if self.pipeline.is_none() {
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Aestra partition health"),
                source: wgpu::ShaderSource::Wgsl(include_str!("diagnostics.wgsl").into()),
            });
            self.pipeline = Some(device.create_compute_pipeline(
                &wgpu::ComputePipelineDescriptor {
                    label: Some("Aestra partition health"),
                    layout: None,
                    module: &shader,
                    entry_point: Some("region_main"),
                    compilation_options: default(),
                    cache: None,
                },
            ));
        }
        let index = match self
            .slots
            .iter()
            .position(|s| !s.busy.load(Ordering::Acquire))
        {
            Some(i) => i,
            None if self.slots.len() < 3 => {
                let make = |usage| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("Aestra partition health slot"),
                        size: BYTES,
                        usage,
                        mapped_at_creation: false,
                    })
                };
                self.slots.push(Slot {
                    output: make(
                        wgpu::BufferUsages::STORAGE
                            | wgpu::BufferUsages::COPY_DST
                            | wgpu::BufferUsages::COPY_SRC,
                    ),
                    staging: make(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
                    busy: Arc::new(AtomicBool::new(false)),
                });
                self.slots.len() - 1
            }
            _ => return None,
        };
        let slot = &self.slots[index];
        let (nodes, heads, counter) = (
            buffers.nodes.buffer()?,
            buffers.heads.buffer()?,
            buffers.atomic_counter.buffer()?,
        );
        let capacity = buffers.nodes.capacity() as u32;
        let bytes: Vec<_> = [size.x * size.y, capacity, sorted_max, size.x]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Aestra partition health params"),
            contents: &bytes,
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pipeline = self.pipeline.as_ref().unwrap();
        let groups = (0..regions.len())
            .map(|i| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("Aestra partition health bindings"),
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: nodes.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: heads.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: counter.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: uniform.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &slot.output,
                                offset: i as u64 * STRIDE,
                                size: std::num::NonZeroU64::new(16),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: uniforms.binding(i),
                        },
                    ],
                })
            })
            .collect();
        slot.busy.store(true, Ordering::Release);
        self.sequence += 1;
        let frame = Frame {
            output: slot.output.clone(),
            staging: slot.staging.clone(),
            busy: slot.busy.clone(),
            pipeline: pipeline.clone(),
            groups,
            sequence: self.sequence,
            capacity,
        };
        context
            .command_encoder()
            .clear_buffer(&frame.output, 0, None);
        Some(frame)
    }
}

impl Frame {
    pub(super) fn measure(&self, context: &mut RenderContext, region: Region, index: usize) {
        let mut pass = context
            .command_encoder()
            .begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Aestra partition health"),
                ..default()
            });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.groups[index], &[]);
        pass.dispatch_workgroups(region.w.div_ceil(64), region.h, 1);
    }

    pub(super) fn finish(
        self,
        context: &mut RenderContext,
        status: ExperimentalOitPartitionStatus,
        generation: u64,
        empty_phase: bool,
    ) {
        let count = self.groups.len();
        let bytes = count as u64 * STRIDE;
        let encoder = context.command_encoder();
        encoder.copy_buffer_to_buffer(&self.output, 0, &self.staging, 0, bytes);
        let staging = self.staging.clone();
        encoder.map_buffer_on_submit(
            &self.staging,
            wgpu::MapMode::Read,
            0..bytes,
            move |result| {
                if result.is_ok() {
                    if let Ok(mapped) = staging.slice(0..bytes).get_mapped_range() {
                        let regions: Vec<[u32; 4]> = (0..count)
                            .map(|i| {
                                std::array::from_fn(|j| {
                                    let offset = i * STRIDE as usize + j * 4;
                                    u32::from_le_bytes(
                                        mapped[offset..offset + 4].try_into().unwrap(),
                                    )
                                })
                            })
                            .collect();
                        status.publish(
                            generation,
                            ExperimentalOitPartitionSample {
                                sequence: self.sequence,
                                empty_phase,
                                attempted_fragments: regions.iter().map(|r| u64::from(r[0])).sum(),
                                dropped_fragments: regions
                                    .iter()
                                    .map(|r| u64::from(r[0].saturating_sub(self.capacity)))
                                    .sum(),
                                invalid_links: regions.iter().map(|r| u64::from(r[2])).sum(),
                                regions,
                                node_capacity: self.capacity,
                            },
                        );
                    }
                    staging.unmap();
                }
                self.busy.store(false, Ordering::Release);
            },
        );
    }
}
