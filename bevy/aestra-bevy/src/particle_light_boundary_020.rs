//! Candidate app-labeled extraction and WESL boundary; not selected by shipping.
use super::particle_light_gpu::{
    BridgeShader, ParticleLightGpuSettings, ParticleLightGpuSlot, ParticleLightGpuStatistics,
};
use crate::ParticleLightRealizationSettings;
pub(super) use bevy::extract::{Extract, ExtractSchedule, MainWorld};
use bevy::{
    extract::{
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        sync_component::SyncComponent,
    },
    prelude::*,
    render::RenderApp,
};

macro_rules! resource {
    ($ty:ty) => {
        impl ExtractResource<RenderApp> for $ty {
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
impl SyncComponent<RenderApp> for ParticleLightGpuSlot {
    type Target = Self;
}
impl ExtractComponent<RenderApp> for ParticleLightGpuSlot {
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
        ExtractComponentPlugin::<ParticleLightGpuSlot, RenderApp>::default(),
        ExtractResourcePlugin::<ParticleLightGpuSettings, RenderApp>::default(),
        ExtractResourcePlugin::<ParticleLightGpuStatistics, RenderApp>::default(),
        ExtractResourcePlugin::<ParticleLightRealizationSettings, RenderApp>::default(),
        ExtractResourcePlugin::<BridgeShader, RenderApp>::default(),
    ));
}
pub(super) fn shader() -> Shader {
    let source = include_str!("particle_light_gpu.wgsl").replace(
        "#import bevy_pbr::mesh_view_types::ClusteredLight",
        "import bevy_pbr::render::mesh_view_types::ClusteredLight;",
    );
    assert!(
        !source.contains('#'),
        "clustered-light shader dialect changed"
    );
    Shader::from_wesl(source, file!())
}
