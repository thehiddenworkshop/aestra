//! Candidate 0.20 app-labeled adapter, selected at the coherent engine switch.
use super::{
    clone_extraction::{clone_component, clone_resource},
    effect_inputs::GpuEffectBuffers,
    particle_light_inputs::{AestraParticleLightSettings, Inputs, ParticleLightMode},
    particle_light_transport::{ParticleLightReadbackFrame, ParticleLightReadbackSettings},
    stage_inputs::ExtractedStages,
};
use super::{draw_instance::GpuDrawInstance, extraction_cleanup, world_sdf::AestraWorldSdf};
use crate::render_settings::AestraRenderSettings;
pub(super) use bevy::extract::{Extract, MainWorld, sync_world::MainEntity};
use bevy::{
    camera::primitives::Aabb,
    extract::{
        ExtractSchedule,
        extract_component::{ExtractComponent, ExtractComponentPlugin},
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        sync_component::SyncComponent,
    },
    prelude::*,
    render::RenderApp,
};

impl SyncComponent<RenderApp> for GpuDrawInstance {
    type Target = Self;
}
impl ExtractComponent<RenderApp> for GpuDrawInstance {
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
impl ExtractResource<RenderApp> for AestraWorldSdf {
    type Source = Self;
    fn extract_resource(source: &Self) -> Self {
        source.clone()
    }
}

clone_component!(
    GpuEffectBuffers,
    SyncComponent<RenderApp>,
    ExtractComponent<RenderApp>
);
clone_component!(
    ExtractedStages,
    SyncComponent<RenderApp>,
    ExtractComponent<RenderApp>
);
clone_component!(
    Inputs,
    SyncComponent<RenderApp>,
    ExtractComponent<RenderApp>
);
clone_resource!(AestraParticleLightSettings, ExtractResource<RenderApp>);
clone_resource!(ParticleLightMode, ExtractResource<RenderApp>);
clone_resource!(ParticleLightReadbackSettings, ExtractResource<RenderApp>);
clone_resource!(ParticleLightReadbackFrame, ExtractResource<RenderApp>);
clone_resource!(AestraRenderSettings, ExtractResource<RenderApp>);

pub(super) fn install(app: &mut App) {
    app.add_plugins((
        ExtractComponentPlugin::<GpuDrawInstance, RenderApp>::default(),
        ExtractComponentPlugin::<GpuEffectBuffers, RenderApp>::default(),
        ExtractComponentPlugin::<ExtractedStages, RenderApp>::default(),
        ExtractResourcePlugin::<AestraWorldSdf, RenderApp>::default(),
        ExtractResourcePlugin::<AestraRenderSettings, RenderApp>::default(),
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
        ExtractComponentPlugin::<Inputs, RenderApp>::default(),
        ExtractResourcePlugin::<AestraParticleLightSettings, RenderApp>::default(),
        ExtractResourcePlugin::<ParticleLightMode, RenderApp>::default(),
    ));
}

pub(super) fn install_light_readback(app: &mut App) {
    app.add_plugins((
        ExtractResourcePlugin::<ParticleLightReadbackSettings, RenderApp>::default(),
        ExtractResourcePlugin::<ParticleLightReadbackFrame, RenderApp>::default(),
    ));
}

#[cfg(test)]
pub(super) fn test_app() -> App {
    use bevy::{
        ecs::schedule::{ScheduleLabel, SystemSet},
        extract::ExtractPlugin,
        render::{Render, RenderSystems},
    };
    let mut app = App::new();
    app.add_plugins(ExtractPlugin::<RenderApp>::new(
        |_, _| {},
        Render::base_schedule,
        Render.intern(),
        RenderSystems::ExtractCommands.intern(),
        RenderSystems::PostCleanup.intern(),
    ));
    app.sub_app_mut(RenderApp).update_schedule = Some(Render.intern());
    install(&mut app);
    install_particle_lights(&mut app);
    install_light_readback(&mut app);
    app
}

#[cfg(test)]
fn render_entity(app: &App, main: Entity) -> Entity {
    app.world()
        .get::<bevy::extract::sync_world::SubEntity<RenderApp>>(main)
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

#[test]
fn removing_actual_draw_without_render_app_is_safe_in_020() {
    use bevy::extract::sync_component::SyncComponentPlugin;
    let mut app = App::new();
    app.add_plugins(SyncComponentPlugin::<GpuDrawInstance, RenderApp>::default());
    let owner = app.world_mut().spawn_empty().id();
    let entity = app.world_mut().spawn(tests::draw(owner)).id();
    app.world_mut()
        .entity_mut(entity)
        .remove::<GpuDrawInstance>();
    app.world_mut().despawn(entity);
}
