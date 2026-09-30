//! Multi-pass history encoding; transient parameters reuse an existing globals lane.
use super::*;

pub(super) struct Dispatch<'a> {
    passes: Vec<aestra_gpu::TrailPass>,
    upload: Buffer,
    pipelines: [&'a ComputePipeline; 9],
    emitters: u32,
}

impl<'a> Dispatch<'a> {
    pub fn new(
        device: &RenderDevice,
        plan: aestra_gpu::TrailScratchPlan,
        pipelines: [&'a ComputePipeline; 9],
        emitters: u32,
    ) -> Self {
        let passes = plan.passes();
        let bytes: Vec<_> = passes
            .iter()
            .flat_map(|pass| pass.parameter.to_le_bytes())
            .collect();
        let upload = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra paged trail stage parameters"),
            contents: &bytes,
            usage: BufferUsages::COPY_SRC,
        });
        Self {
            passes,
            upload,
            pipelines,
            emitters,
        }
    }

    pub fn record(&self, encoder: &mut CommandEncoder, group: &BindGroup, globals: &Buffer) {
        for (index, stage) in self.passes.iter().enumerate() {
            encoder.copy_buffer_to_buffer(&self.upload, index as u64 * 4, globals, 28, 4);
            let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("aestra paged trail history"),
                timestamp_writes: None,
            });
            pass.set_bind_group(0, group, &[]);
            pass.set_pipeline(self.pipelines[stage.entry]);
            pass.dispatch_workgroups(stage.workgroups, self.emitters, 1);
        }
    }
}
