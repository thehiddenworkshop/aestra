//! Histories observe the *presented result* of every fixed tick, including event births.
//! Live frames advance incrementally. Seeking restores only a checkpoint shared by
//! particles, domains and histories; independently replaying analytic heads is incorrect.
use super::*;

#[derive(Default)]
pub(super) struct History {
    pub tick: Option<u32>,
    pub epoch: Option<u32>,
    pub checkpoints: trail_checkpoints::TrailCheckpoints,
    key: Vec<u64>,
    context: Option<Arc<trail_checkpoints::TrailContext>>,
}

impl History {
    pub fn sync(&mut self, effect: &GpuEffectBuffers, states: &[StatefulPersistentState]) {
        let key = states
            .iter()
            .map(|state| state.fingerprint)
            .collect::<Vec<_>>();
        if self.key != key {
            *self = Self::default();
            self.key = key;
        }
        if self.context.as_deref() != Some(&effect.checkpoint_context) {
            self.checkpoints = default();
            self.context = Some(effect.checkpoint_context.clone());
        }
    }
}

pub(super) type Histories = BTreeMap<Entity, (AssetId<ShaderBuffer>, History)>;

/// Borrowed per-effect encoder inputs. It uses the existing simulation layout and
/// history ABI: no CPU particle readback or separate copy of live particle state.
pub(super) struct Observer<'a> {
    pub history: &'a mut History,
    pub effect: &'a GpuEffectBuffers,
    pub group: &'a BindGroup,
    pub reset: &'a ComputePipeline,
    pub simulate: &'a ComputePipeline,
    pub update: &'a ComputePipeline,
    pub ribbons: Option<&'a ComputePipeline>,
    pub globals: &'a Buffer,
    pub render_globals: &'a Buffer,
    pub buffers: [&'a Buffer; 6],
    pub memory_budget: u64,
}

impl Observer<'_> {
    pub fn reset_history(&mut self, encoder: &mut CommandEncoder) {
        // Clear only history storage, not parent presentation. Unused ring slots
        // otherwise retain old samples after a restart and contaminate snapshots.
        // This GPU clear happens only on reset, never on ordinary live frames.
        encoder.clear_buffer(
            self.buffers[0],
            u64::from(self.effect.total_slots) * 48,
            None,
        );
        encoder.clear_buffer(
            self.buffers[5],
            u64::from(self.effect.total_slots) * 12,
            None,
        );
        self.history.tick = None;
    }

    pub fn restore(&mut self, encoder: &mut CommandEncoder, tick: u32) {
        let time = tick as f32 * STATEFUL_TICK_DT;
        assert_eq!(
            self.history
                .checkpoints
                .restore(encoder, &self.buffers, time),
            Some(time)
        );
        trail_checkpoints::rebase_epoch(
            encoder,
            self.globals,
            self.buffers[5],
            self.buffers[3],
            &self.effect.trail_roots,
        );
        self.history.tick = Some(tick);
        self.history.epoch = Some(self.effect.history_epoch);
    }

    /// Reset analytic counts and present analytic emitters at the same canonical
    /// time as stateful heads. The caller presents stateful heads next, then records.
    pub fn prepare(&self, device: &RenderDevice, encoder: &mut CommandEncoder, tick: u32) {
        let time = tick as f32 * STATEFUL_TICK_DT;
        let placement = Mat4::from_cols_array(&std::array::from_fn(|i| {
            f32::from_bits(self.effect.checkpoint_context.key[6 + i])
        }));
        let bytes = crate::host_transform::observation_bytes(
            &[time],
            placement,
            self.effect.checkpoint_context.motion.as_deref(),
        );
        let upload = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("aestra stateful trail observation"),
            contents: &bytes,
            usage: BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(&upload, 0, self.globals, 0, 4);
        encoder.copy_buffer_to_buffer(&upload, 4, self.globals, 32, 64);
        encoder.copy_buffer_to_buffer(&upload, 0, self.render_globals, 64, 4);
        encoder.copy_buffer_to_buffer(&upload, 4, self.render_globals, 0, 64);
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra fixed-tick trail presentation reset"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, self.group, &[]);
        pass.set_pipeline(self.reset);
        pass.dispatch_workgroups(1, 1, 1);
        if !self.effect.stateful_only {
            pass.set_pipeline(self.simulate);
            pass.dispatch_workgroups(self.effect.workgroups, 1, 1);
        }
    }

    pub fn record(&mut self, encoder: &mut CommandEncoder, tick: u32) {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some("aestra stateful trail history"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, self.group, &[]);
        pass.set_pipeline(self.update);
        pass.dispatch_workgroups(self.effect.trail_workgroups, 1, 1);
        if self.effect.has_ribbons
            && let Some(ribbons) = self.ribbons
        {
            pass.set_pipeline(ribbons);
            pass.dispatch_workgroups(self.effect.ribbon_workgroups, 1, 1);
        }
        self.history.tick = Some(tick);
        self.history.epoch = Some(self.effect.history_epoch);
    }

    pub fn capture(&mut self, device: &RenderDevice, encoder: &mut CommandEncoder, tick: u32) {
        self.history.checkpoints.capture(
            device,
            encoder,
            &self.buffers,
            tick as f32 * STATEFUL_TICK_DT,
            self.memory_budget
                .saturating_sub(self.history.checkpoints.bytes()),
        );
    }
}
