//! Shipping 0.19 extraction adapter; the actual payload/policy is version-shared.
use super::{
    clone_extraction::{clone_component, clone_resource},
    effect_inputs::GpuEffectBuffers,
    particle_light_inputs::{AestraParticleLightSettings, Inputs, ParticleLightMode},
    particle_light_transport::{ParticleLightReadbackFrame, ParticleLightReadbackSettings},
    stage_inputs::ExtractedStages,
};
use super::{draw_instance::GpuDrawInstance, extraction_cleanup, world_sdf::AestraWorldSdf};
use crate::render_settings::AestraRenderSettings;
pub(super) use bevy::render::{Extract, MainWorld, sync_world::MainEntity};
use bevy::{
    camera::primitives::Aabb,
    prelude::*,
    render::{
        ExtractSchedule, RenderApp,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        sync_component::SyncComponent,
    },
};

impl SyncComponent for GpuDrawInstance {
    type Target = Self;
}
impl ExtractComponent for GpuDrawInstance {
    type QueryData = (
        &'static Self,
        &'static ViewVisibility,
        &'static GlobalTransform,
        &'static Aabb,
    );
    type QueryFilter = ();
    type Out = Self;
    fn extract_component(
        (instance, visibility, transform, bounds): bevy::ecs::query::QueryItem<
            '_,
            '_,
            Self::QueryData,
        >,
    ) -> Option<Self::Out> {
        instance.extracted(visibility, transform, bounds)
    }
}
impl ExtractResource for AestraWorldSdf {
    type Source = Self;
    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

clone_component!(GpuEffectBuffers, SyncComponent, ExtractComponent);
clone_component!(ExtractedStages, SyncComponent, ExtractComponent);
clone_component!(Inputs, SyncComponent, ExtractComponent);
clone_resource!(AestraParticleLightSettings, ExtractResource);
clone_resource!(ParticleLightMode, ExtractResource);
clone_resource!(ParticleLightReadbackSettings, ExtractResource);
clone_resource!(ParticleLightReadbackFrame, ExtractResource);
clone_resource!(AestraRenderSettings, ExtractResource);

pub(super) fn install(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<GpuDrawInstance>::default(),
        ExtractComponentPlugin::<GpuEffectBuffers>::default(),
        ExtractComponentPlugin::<ExtractedStages>::default(),
        ExtractResourcePlugin::<AestraWorldSdf>::default(),
        ExtractResourcePlugin::<AestraRenderSettings>::default(),
    ));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(
            ExtractSchedule,
            extraction_cleanup::remove_missing_resource::<AestraWorldSdf>,
        );
        render
            .init_resource::<super::catchup_pacing::CatchupPacer>()
            .add_systems(
                ExtractSchedule,
                super::extraction_systems::extract_catchup_pacing,
            );
    }
}

pub(super) fn install_device_publication(app: &mut App) {
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(
            ExtractSchedule,
            super::capability_publication::publish_gpu_capabilities,
        );
    }
}

pub(super) fn install_particle_lights(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<Inputs>::default(),
        ExtractResourcePlugin::<AestraParticleLightSettings>::default(),
        ExtractResourcePlugin::<ParticleLightMode>::default(),
    ));
}

pub(super) fn install_light_readback(app: &mut App) {
    app.add_plugins((
        ExtractResourcePlugin::<ParticleLightReadbackSettings>::default(),
        ExtractResourcePlugin::<ParticleLightReadbackFrame>::default(),
    ));
}

#[cfg(test)]
fn test_app() -> App {
    use bevy::{
        ecs::schedule::ScheduleLabel,
        render::{Render, extract_plugin::ExtractPlugin},
    };
    let mut app = App::new();
    app.add_plugins(ExtractPlugin::default());
    app.sub_app_mut(RenderApp).update_schedule = Some(Render.intern());
    install(&mut app);
    install_particle_lights(&mut app);
    install_light_readback(&mut app);
    app
}

#[cfg(test)]
fn render_entity(app: &App, main: Entity) -> Entity {
    app.world()
        .get::<bevy::render::sync_world::RenderEntity>(main)
        .unwrap()
        .id()
}

#[cfg(test)]
#[path = "extraction_control_tests.rs"]
mod control_tests;
#[cfg(test)]
#[path = "extraction_state_tests.rs"]
mod state_tests;
#[cfg(test)]
#[path = "extraction_tests.rs"]
mod tests;
