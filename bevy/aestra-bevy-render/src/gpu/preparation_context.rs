//! Render-world timing inputs, independent of host presentation/profile reporting.
use super::{
    effect_inputs::GpuEffectBuffers,
    timestamp_transport::{SimulationTimer, TimingBatch, TimingMailbox},
};
use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    render::{
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
    },
};

#[derive(Resource, Default, Clone)]
pub(super) struct PreparationMailboxes {
    pub compaction: TimingMailbox,
    pub culling: TimingMailbox,
}

#[derive(SystemParam)]
pub(super) struct TimingContext<'w, 's> {
    device: Res<'w, RenderDevice>,
    queue: Res<'w, RenderQueue>,
    pub mailboxes: Res<'w, PreparationMailboxes>,
    effects: Query<'w, 's, (&'static MainEntity, &'static GpuEffectBuffers)>,
}

impl TimingContext<'_, '_> {
    pub fn begin(&self, timer: &mut SimulationTimer) -> Option<TimingBatch> {
        timer.begin(&self.device, self.queue.get_timestamp_period())
    }

    pub fn owner(&self, batch: &mut TimingBatch, owner: Entity) -> Option<u32> {
        let (_, effect) = self.effects.iter().find(|(main, _)| main.id() == owner)?;
        batch.instance(owner, effect.statistics_token, effect.simulation_time)
    }
}
