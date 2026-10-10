//! Multi-pass history encoding; transient parameters reuse an existing globals lane.
use bevy::render::{
    diagnostic::RecordDiagnostics,
    render_resource::{
        BindGroup, Buffer, BufferInitDescriptor, BufferUsages, CommandEncoder,
        ComputePassDescriptor, ComputePipeline,
    },
    renderer::RenderDevice,
};

pub(super) struct Dispatch<'a> {
    passes: Vec<aestra_gpu::TrailPass>,
    upload: Buffer,
    pipelines: [&'a ComputePipeline; 9],
    emitters: u32,
}

impl<'a> Dispatch<'a> {
    pub fn workgroups(&self) -> u64 {
        self.passes
            .iter()
            .map(|pass| u64::from(pass.workgroups) * u64::from(self.emitters))
            .sum()
    }

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

    pub fn record(
        &self,
        encoder: &mut CommandEncoder,
        group: &BindGroup,
        globals: &Buffer,
        diagnostics: Option<&bevy::render::diagnostic::DiagnosticsRecorder>,
    ) {
        // Include all page/merge passes of each logical phase, not just the last
        // merge dispatched under a repeated diagnostic name.
        let names = [
            "aestra::gpu::trail_heads",
            "aestra::gpu::trail_reserve",
            "aestra::gpu::trail_allocate_sample",
            "aestra::gpu::trail_bounds",
        ];
        let mut index = 0;
        for (phase, stages) in self
            .passes
            .split_inclusive(|stage| matches!(stage.entry, 2 | 3 | 6 | 8))
            .enumerate()
        {
            let name = names
                .get(phase)
                .copied()
                .unwrap_or("aestra::gpu::trail_other");
            let span = diagnostics.time_span(encoder, name);
            for stage in stages {
                encoder.copy_buffer_to_buffer(&self.upload, index * 4, globals, 28, 4);
                index += 1;
                let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                    label: Some("aestra paged trail history"),
                    timestamp_writes: None,
                });
                pass.set_bind_group(0, group, &[]);
                pass.set_pipeline(self.pipelines[stage.entry]);
                pass.dispatch_workgroups(stage.workgroups, self.emitters, 1);
            }
            span.end(encoder);
        }
    }
}
