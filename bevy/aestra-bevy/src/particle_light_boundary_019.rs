//! Shipping extraction/shader boundary. Realization and admission remain shared.
use super::particle_light_gpu::{
    BridgeShader, ParticleLightGpuSettings, ParticleLightGpuSlot, ParticleLightGpuStatistics,
};
use crate::ParticleLightRealizationSettings;
pub(super) use bevy::render::{Extract, ExtractSchedule, MainWorld};
use bevy::{
    prelude::*,
    render::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        sync_component::SyncComponent,
    },
};

macro_rules! resource {
    ($ty:ty) => {
        impl ExtractResource for $ty {
            type Source = Self;
            fn extract_resource(source: &Self) -> Self {
                source.clone()
            }
        }
    };
}
resource!(ParticleLightGpuSettings);
resource!(ParticleLightGpuStatistics);
resource!(ParticleLightRealizationSettings);
resource!(BridgeShader);
impl SyncComponent for ParticleLightGpuSlot {
    type Target = Self;
}
impl ExtractComponent for ParticleLightGpuSlot {
    type QueryData = &'static Self;
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(
        item: bevy::ecs::query::QueryItem<'_, '_, Self::QueryData>,
    ) -> Option<Self> {
        Some(*item)
    }
}
pub(super) fn install(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<ParticleLightGpuSlot>::default(),
        ExtractResourcePlugin::<ParticleLightGpuSettings>::default(),
        ExtractResourcePlugin::<ParticleLightGpuStatistics>::default(),
        ExtractResourcePlugin::<ParticleLightRealizationSettings>::default(),
        ExtractResourcePlugin::<BridgeShader>::default(),
    ));
}
pub(super) fn shader() -> Shader {
    Shader::from_wgsl(include_str!("particle_light_gpu.wgsl"), file!())
}
