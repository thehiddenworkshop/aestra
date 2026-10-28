//! Canonical host plugin startup and machine-readable profiling.
use crate::{
    ActiveBackend, AestraEventInput, AestraRenderPlugin, AestraRenderSet, AestraSet, EffectPlayer,
    EffectRuntimeStatus, GpuCapabilities, PresentedEffect, bindings, input_events,
    playback::{advance_players, prepare_player_presentations, sync_player_presentations},
    project, project_profile,
};
use aestra_core::AssetKind;
use aestra_runtime::{CompiledEffect, EffectProfile, ParticleSample, ProfileValue};
use bevy::{asset::LoadState, prelude::*};
use std::collections::{BTreeMap, BTreeSet};

/// Installs Aestra's compiled effect playback into a Bevy application.
#[derive(Default)]
pub struct AestraPlugin;

impl Plugin for AestraPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AestraRenderPlugin)
            .add_message::<AestraEventInput>()
            .init_resource::<TextureAssetCache>()
            .init_resource::<crate::EffectPresentationCache>()
            .configure_sets(Update, AestraRenderSet::Prepare.after(AestraSet::Playback))
            .configure_sets(
                Update,
                AestraSet::ResolveHostInputs.before(AestraSet::Playback),
            );
        app.add_systems(
            Update,
            (
                bindings::apply_binding_traces,
                bindings::resolve_host_bindings,
                input_events::apply_event_inputs,
            )
                .chain()
                .in_set(AestraSet::ResolveHostInputs),
        );
        app.add_systems(
            Update,
            (
                prepare_player_presentations,
                prepare_effect_profiles,
                update_asset_diagnostics,
                advance_players,
                profile_effects,
                project::sync_project_instances,
                bindings::forward_project_bindings,
                sync_player_presentations,
            )
                .chain()
                .in_set(AestraSet::Playback),
        );
        app.add_systems(
            Update,
            project_profile::update_project_profiles
                .after(AestraRenderSet::Prepare)
                .in_set(AestraSet::Profile),
        );
    }
}

#[derive(Resource, Default)]
pub(crate) struct TextureAssetCache(BTreeMap<String, bevy::prelude::Handle<Image>>);

impl TextureAssetCache {
    pub(crate) fn load(
        &mut self,
        asset_server: &AssetServer,
        path: &str,
    ) -> bevy::prelude::Handle<Image> {
        self.0
            .entry(path.to_owned())
            .or_insert_with(|| asset_server.load(path.to_owned()))
            .clone()
    }
}

/// Live machine-readable profile for an [`EffectPlayer`].
#[derive(Component, Debug, Clone)]
pub struct EffectProfiler(pub EffectProfile);

pub(crate) fn prepare_effect_profiles(
    mut commands: Commands,
    capabilities: Res<GpuCapabilities>,
    players: Query<(Entity, &EffectPlayer, &EffectRuntimeStatus), Without<EffectProfiler>>,
) {
    for (entity, player, runtime) in &players {
        commands.entity(entity).insert(EffectProfiler(bevy_profile(
            player.effect(),
            &capabilities,
            runtime,
        )));
    }
}

fn update_asset_diagnostics(
    asset_server: Res<AssetServer>,
    images: Res<Assets<Image>>,
    mut texture_cache: bevy::prelude::ResMut<TextureAssetCache>,
    mut players: Query<(&EffectPlayer, &mut EffectProfiler)>,
) {
    const PREFIX: &str = "texture asset '";
    for (player, mut profiler) in &mut players {
        profiler
            .0
            .platform_warnings
            .retain(|warning| !warning.starts_with(PREFIX));
        let referenced = player
            .effect()
            .emitters
            .iter()
            .flat_map(|emitter| emitter.renderers.iter())
            .filter_map(|renderer| player.effect().material(renderer.material))
            .filter_map(|material| material.texture)
            .chain(
                player
                    .effect()
                    .flipbooks
                    .iter()
                    .map(|flipbook| flipbook.texture),
            )
            .collect::<BTreeSet<_>>();
        let mut texture_bytes = 0_u64;
        let mut all_loaded = true;
        for asset in &player.effect().assets {
            if asset.kind != AssetKind::Texture {
                continue;
            }
            if !referenced.contains(&asset.source) {
                continue;
            }
            let handle = texture_cache.load(&asset_server, &asset.path);
            match asset_server.get_load_state(handle.id()) {
                Some(LoadState::Failed(error)) => {
                    all_loaded = false;
                    profiler.0.platform_warnings.push(format!(
                        "texture asset '{}' ({}) failed to load: {error}; using the missing-texture fallback",
                        asset.name, asset.path
                    ));
                }
                Some(LoadState::Loaded) => {
                    let Some(image) = images.get(&handle) else {
                        all_loaded = false;
                        continue;
                    };
                    texture_bytes = texture_bytes
                        .saturating_add(image.data.as_ref().map_or(0, |data| data.len()) as u64);
                }
                Some(LoadState::NotLoaded | LoadState::Loading) | None => {
                    all_loaded = false;
                }
            }
        }
        profiler.0.texture_memory_bytes = if referenced.is_empty() {
            ProfileValue::Estimated(0)
        } else if all_loaded {
            ProfileValue::Measured(texture_bytes)
        } else {
            ProfileValue::Unavailable
        };
    }
}

pub(crate) fn bevy_profile(
    effect: &CompiledEffect,
    capabilities: &GpuCapabilities,
    runtime: &EffectRuntimeStatus,
) -> EffectProfile {
    let mut profile = EffectProfile::from_compiled(effect);
    profile.platform_warnings = capabilities.limitations.clone();
    if runtime.active == ActiveBackend::CpuReference
        && runtime.reason.contains("fallback")
        && !profile.platform_warnings.contains(&runtime.reason)
    {
        profile.platform_warnings.push(runtime.reason.clone());
    }
    profile
}

#[allow(clippy::type_complexity)]
fn profile_effects(
    capabilities: Res<GpuCapabilities>,
    mut players: Query<(
        &EffectPlayer,
        &PresentedEffect,
        &mut EffectProfiler,
        &EffectRuntimeStatus,
        Option<&crate::gpu::GpuTrailStatistics>,
    )>,
) {
    let _span = bevy::log::info_span!("aestra::runtime::advance").entered();
    for (player, presented, mut profiler, runtime, trails) in &mut players {
        if !profiler.0.matches_compiled(player.effect()) {
            profiler.0 = bevy_profile(player.effect(), &capabilities, runtime);
        }
        record_presented_profile(
            &mut profiler.0,
            player.effect(),
            presented.samples(),
            presented.cpu_evaluation_time(),
            runtime.active,
        );
        profiler
            .0
            .record_trail_usage(trails.and_then(|s| s.usage(&presented.instance)));
    }
}

pub(crate) fn record_presented_profile(
    profile: &mut EffectProfile,
    effect: &CompiledEffect,
    samples: &[ParticleSample],
    cpu_evaluation_time: Option<std::time::Duration>,
    backend: ActiveBackend,
) {
    profile.cpu_time_ns = ProfileValue::Unavailable;
    profile.gpu_simulation_time_ns = ProfileValue::Unavailable;
    profile.gpu_trail_compaction_time_ns = ProfileValue::Unavailable;
    profile.gpu_trail_culling_time_ns = ProfileValue::Unavailable;
    profile.alive_particles = ProfileValue::Unavailable;
    profile.submitted_instances = ProfileValue::Unavailable;
    profile.submitted_vertices = ProfileValue::Unavailable;
    profile.submitted_primitives = ProfileValue::Unavailable;
    for emitter in &mut profile.emitters {
        emitter.alive_particles = 0;
    }
    if let Some(elapsed) = cpu_evaluation_time {
        profile.record_cpu_frame(elapsed, samples);
        profile.record_submitted_frame(effect, samples);
    } else if matches!(
        backend,
        ActiveBackend::GpuReadback | ActiveBackend::CpuReference
    ) {
        profile.record_particle_frame(samples);
        profile.record_submitted_frame(effect, samples);
    }
}
