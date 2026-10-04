//! Opt-in shadowless scene lights from the globally selected GPU prefix.
//! This is presentation, not an event stream or one entity per source particle.
use crate::{ActiveBackend, EffectOutputContext, EffectRuntimeStatus, PresentedEffect};
use aestra_bevy_render::gpu::particle_lights::ParticleLightSource;
pub use aestra_bevy_render::gpu::{
    particle_light_readback::{
        AestraParticleLightReadbackPlugin, ParticleLightReadback, ParticleLightReadbackFrame,
        ParticleLightReadbackSettings, ParticleLightReadbackStatistics, ParticleLightSnapshot,
        SelectedParticleLight,
    },
    particle_lights::{AestraParticleLightSettings, ParticleLightMode},
};
use bevy::{camera::visibility::RenderLayers, prelude::*, transform::TransformSystems};
use std::{collections::BTreeMap, sync::Arc, time::Instant};

/// Independent host safety clamps. Shadows remain off; disable the entire GPU
/// selection/transport/realization path with `AestraParticleLightSettings.max_lights = 0`.
#[derive(Resource, Clone, Debug, bevy::render::extract_resource::ExtractResource)]
pub struct ParticleLightRealizationSettings {
    pub max_lumens: f32,
    pub max_range: f32,
}
impl Default for ParticleLightRealizationSettings {
    fn default() -> Self {
        Self {
            max_lumens: 1_000_000.0,
            max_range: 200.0,
        }
    }
}
#[derive(Component)]
pub struct ParticleLightProxy;

/// Last/current bounded-pool observations. Timing is transport-to-main-world,
/// not a claim about final display latency or total clustered-light GPU cost.
#[derive(Resource, Default, Clone, Debug)]
pub struct ParticleLightStatistics {
    pub active: usize,
    pub allocated: usize,
    pub peak_active: usize,
    pub updates: u64,
    pub stale_sources: u64,
    /// Packets superseded by main-world configuration/generation changes before
    /// the pipelined render world has caught up with the host.
    pub discarded_packets: u64,
    pub expired: u64,
    pub truncated: u64,
    pub last_sequence: u64,
    pub copied_bytes: u64,
    pub update_age_seconds: f64,
    pub max_update_age_seconds: f64,
    pub frame_lag: u64,
    pub max_frame_lag: u64,
    pub pool_update_ms: f64,
    pub max_pool_update_ms: f64,
    pub readback: ParticleLightReadbackStatistics,
}
type Key = (Arc<ParticleLightSource>, u32);
struct Slot {
    entity: Entity,
    key: Option<Key>,
}
#[derive(Resource, Default)]
struct Pool {
    slots: Vec<Slot>,
    snapshot: Option<Arc<ParticleLightSnapshot>>,
    signature: Option<(
        ParticleLightReadbackSettings,
        AestraParticleLightSettings,
        ParticleLightMode,
    )>,
}

/// Add after `AestraPlugin`; additionally opt into a nonzero global selection
/// cap. Storage and entities scale with that cap, never particle capacity.
pub struct AestraParticleLightPlugin;
impl Plugin for AestraParticleLightPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(AestraParticleLightReadbackPlugin)
            .init_resource::<ParticleLightMode>()
            .init_resource::<ParticleLightRealizationSettings>()
            .init_resource::<ParticleLightStatistics>()
            .init_resource::<Pool>()
            .add_systems(PostUpdate, realize.before(TransformSystems::Propagate));
        super::particle_light_gpu::install(app);
    }
}

pub(super) fn source_layers(world: &World, source: &ParticleLightSource) -> Option<RenderLayers> {
    let owner = world.get::<PresentedEffect>(source.owner)?;
    let root = world.get::<PresentedEffect>(source.root)?;
    let runtime = world.get::<EffectRuntimeStatus>(source.owner)?;
    let instance = &owner.instance;
    if runtime.active != ActiveBackend::Gpu
        || root.instance.history_epoch() != source.root_epoch
        || instance.history_epoch() != source.owner_epoch
        || instance.history_revision() != source.revision
        || instance.seed() != source.seed
        || owner.effect().source != source.effect
        || !source.artifact.matches(owner.effect())
        || world.get::<Visibility>(source.owner) == Some(&Visibility::Hidden)
        || world.get::<Visibility>(source.root) == Some(&Visibility::Hidden)
    {
        return None;
    }
    match world.get::<EffectOutputContext>(source.owner) {
        Some(context)
            if context.root == source.root
                && context.clip_path == source.clip_path
                && context.playback_epoch == source.root_epoch => {}
        None if source.root == source.owner && source.clip_path.is_empty() => {}
        _ => return None,
    }
    let emitter = owner
        .effect()
        .emitters
        .iter()
        .find(|e| e.source == source.emitter && e.region == source.region && e.enabled)?;
    emitter.scene_outputs.iter().find(|o| {
        o.source == source.output && {
            let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(plan) = &o.kind;
            plan.max_lights != 0
        }
    })?;
    Some(
        world
            .get::<RenderLayers>(source.owner)
            .cloned()
            .unwrap_or_default(),
    )
}

fn rendered(
    light: &SelectedParticleLight,
    settings: &ParticleLightRealizationSettings,
) -> PointLight {
    PointLight {
        color: Color::linear_rgb(
            light.linear_color.x,
            light.linear_color.y,
            light.linear_color.z,
        ),
        intensity: light.lumens.min(settings.max_lumens),
        range: light.range.min(settings.max_range),
        radius: light.radius.min(light.range.min(settings.max_range)),
        shadow_maps_enabled: false,
        contact_shadows_enabled: false,
        ..default()
    }
}

fn realize(world: &mut World) {
    let start = Instant::now();
    let settings = world.resource::<ParticleLightRealizationSettings>().clone();
    let transport = world.resource::<ParticleLightReadbackSettings>().clone();
    let selection = *world.resource::<AestraParticleLightSettings>();
    let mode = *world.resource::<ParticleLightMode>();
    let cap = if mode == ParticleLightMode::PortableAsync {
        selection.max_lights
    } else {
        0
    };
    let frame = world.resource::<ParticleLightReadbackFrame>().0;
    let (generation, active, latest, readback) = world.resource::<ParticleLightReadback>().take();
    world.resource_scope(|world, mut pool: Mut<Pool>| {
        world.resource_scope(|world, mut stats: Mut<ParticleLightStatistics>| {
            stats.readback = readback;
            let signature = (transport.clone(), selection, mode);
            if pool.signature.as_ref() != Some(&signature) {
                pool.snapshot = None;
                pool.signature = Some(signature);
                // A snapshot captured under an earlier host budget is not reused.
            }
            if let Some(latest) = latest {
                if active
                    && latest.generation == generation
                    && packet_matches(&latest, &transport, selection)
                {
                    pool.snapshot = Some(latest);
                } else {
                    stats.discarded_packets += 1;
                }
            }
            if !active
                || pool
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.generation != generation)
            {
                pool.snapshot = None;
            }
            let snapshot = pool.snapshot.clone();
            apply(
                world,
                &mut pool,
                &mut stats,
                &settings,
                &transport,
                cap,
                frame,
                snapshot.as_deref(),
            );
            stats.pool_update_ms = start.elapsed().as_secs_f64() * 1000.0;
            stats.max_pool_update_ms = stats.max_pool_update_ms.max(stats.pool_update_ms);
        });
    });
}

fn packet_matches(
    snapshot: &ParticleLightSnapshot,
    transport: &ParticleLightReadbackSettings,
    selection: AestraParticleLightSettings,
) -> bool {
    &snapshot.settings == transport && snapshot.selection == selection
}

#[allow(clippy::too_many_arguments)]
fn apply(
    world: &mut World,
    pool: &mut Pool,
    stats: &mut ParticleLightStatistics,
    settings: &ParticleLightRealizationSettings,
    transport: &ParticleLightReadbackSettings,
    cap: u32,
    frame: u64,
    snapshot: Option<&ParticleLightSnapshot>,
) {
    let valid = settings.max_lumens.is_finite()
        && settings.max_lumens > 0.0
        && settings.max_range.is_finite()
        && settings.max_range > 0.0;
    let limit = if valid {
        cap.min(transport.max_lights) as usize
    } else {
        0
    };
    pool.slots.retain(|s| {
        let valid = world.get::<ParticleLightProxy>(s.entity).is_some()
            && world.get::<PointLight>(s.entity).is_some()
            && world.get::<Transform>(s.entity).is_some()
            && world.get::<Visibility>(s.entity).is_some();
        if !valid {
            world.despawn(s.entity);
        }
        valid
    });
    for slot in pool.slots.drain(limit.min(pool.slots.len())..) {
        world.despawn(slot.entity);
    }
    let snapshot = snapshot.filter(|s| {
        let fresh = s.captured_at.elapsed() <= transport.max_age
            && frame.saturating_sub(s.frame) <= transport.max_frame_lag;
        if !fresh {
            stats.expired += 1;
            pool.snapshot = None;
        }
        fresh
    });
    if limit == 0 {
        pool.snapshot = None;
    }
    let mut desired = BTreeMap::new();
    if let Some(s) = snapshot.filter(|_| limit > 0) {
        let mut layers = BTreeMap::new();
        let new = stats.last_sequence != s.sequence;
        if new {
            stats.updates += 1;
            stats.last_sequence = s.sequence;
            stats.copied_bytes = s.copied_bytes;
            stats.update_age_seconds = s.captured_at.elapsed().as_secs_f64();
            stats.max_update_age_seconds =
                stats.max_update_age_seconds.max(stats.update_age_seconds);
            stats.frame_lag = frame.saturating_sub(s.frame);
            stats.max_frame_lag = stats.max_frame_lag.max(stats.frame_lag);
            stats.truncated += u64::from(s.counters[2].saturating_sub(limit as u32));
        }
        for light in s.lights.iter().take(limit) {
            let token = light.source_token as usize;
            let Some(source) = s.manifest.get(token) else {
                continue;
            };
            let (identity, layers) = layers
                .entry(token)
                .or_insert_with(|| (Arc::new(source.clone()), source_layers(world, source)));
            if let Some(layers) = layers {
                desired.insert(
                    (identity.clone(), light.particle_index),
                    (
                        rendered(light, settings),
                        Transform::from_translation(light.position),
                        layers.clone(),
                    ),
                );
            } else if new {
                stats.stale_sources += 1;
            }
        }
    }
    // Preserve identities even when selection order changes; reuse idle slots
    // before allocating. A second light pool handles representative flashes.
    for slot in &mut pool.slots {
        if let Some(value) = slot.key.as_ref().and_then(|key| desired.remove(key)) {
            world
                .entity_mut(slot.entity)
                .insert((value.0, value.1, value.2, Visibility::Visible));
        } else {
            slot.key = None;
            world.entity_mut(slot.entity).insert(Visibility::Hidden);
            world.get_mut::<PointLight>(slot.entity).unwrap().intensity = 0.0;
        }
    }
    let mut idle = pool
        .slots
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.key.is_none().then_some(i))
        .collect::<Vec<_>>()
        .into_iter();
    for (key, (light, transform, layers)) in desired {
        if let Some(index) = idle.next() {
            let slot = &mut pool.slots[index];
            slot.key = Some(key);
            world
                .entity_mut(slot.entity)
                .insert((light, transform, layers, Visibility::Visible));
        } else if pool.slots.len() < limit {
            let entity = world
                .spawn((
                    ParticleLightProxy,
                    light,
                    transform,
                    layers,
                    Visibility::Visible,
                ))
                .id();
            pool.slots.push(Slot {
                entity,
                key: Some(key),
            });
        }
    }
    stats.active = pool.slots.iter().filter(|s| s.key.is_some()).count();
    stats.allocated = pool.slots.len();
    stats.peak_active = stats.peak_active.max(stats.active);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CompatibilityReport, CompatibilityTarget, EffectAsset, EffectCompiler, Emitter,
        ParticlePointLightProperties, PlaybackHistoryPolicy, SceneOutputInstance,
    };
    use aestra_bevy_render::gpu::particle_lights::ParticleLightArtifact;
    use std::time::Duration;

    fn fixture() -> (World, Entity, ParticleLightSnapshot) {
        let mut asset = EffectAsset::new("generic moving lights", 10.0);
        let mut emitter = Emitter::basic_sprite("embers", 10.0);
        emitter
            .scene_outputs
            .push(SceneOutputInstance::particle_point_light(
                ParticlePointLightProperties::new(4000.0, 12.0),
            ));
        asset.emitters.push(emitter);
        let effect = Arc::new(EffectCompiler::default().compile(&asset).unwrap());
        let mut world = World::new();
        let owner = world
            .spawn((
                PresentedEffect::new(effect.clone())
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly),
                RenderLayers::layer(7),
                EffectRuntimeStatus {
                    active: ActiveBackend::Gpu,
                    reason: String::new(),
                    compatibility: CompatibilityReport::compatible(CompatibilityTarget::NativeGpu),
                },
            ))
            .id();
        let source = ParticleLightSource {
            root: owner,
            root_epoch: 0,
            clip_path: vec![],
            owner,
            owner_epoch: 0,
            revision: 0,
            effect: effect.source,
            seed: 0,
            emitter: effect.emitters[0].source,
            region: effect.emitters[0].region,
            output: effect.emitters[0].scene_outputs[0].source,
            artifact: ParticleLightArtifact(effect),
        };
        // Default seed is part of the qualified identity.
        let seed = world.get::<PresentedEffect>(owner).unwrap().instance.seed();
        let snapshot = ParticleLightSnapshot {
            generation: 0,
            sequence: 1,
            frame: 1,
            captured_at: Instant::now(),
            manifest: Arc::from([ParticleLightSource { seed, ..source }]),
            lights: (0..3)
                .map(|i| SelectedParticleLight {
                    position: Vec3::new(i as f32, 2.0, 0.0),
                    range: 12.0,
                    linear_color: Vec3::ONE,
                    lumens: 4000.0,
                    radius: 20.0,
                    source_token: 0,
                    particle_index: i,
                })
                .collect(),
            counters: [3, 3, 3, 0],
            copied_bytes: 160,
            settings: ParticleLightReadbackSettings::default(),
            selection: AestraParticleLightSettings {
                max_lights: 2,
                ..default()
            },
        };
        (world, owner, snapshot)
    }
    fn apply_test(
        world: &mut World,
        pool: &mut Pool,
        stats: &mut ParticleLightStatistics,
        snapshot: &ParticleLightSnapshot,
        cap: u32,
    ) {
        apply(
            world,
            pool,
            stats,
            &ParticleLightRealizationSettings {
                max_lumens: 1000.0,
                max_range: 8.0,
            },
            &ParticleLightReadbackSettings::default(),
            cap,
            1,
            Some(snapshot),
        );
    }
    #[test]
    fn pipelined_packets_must_match_current_byte_and_light_budgets() {
        let (_, _, snapshot) = fixture();
        assert!(packet_matches(
            &snapshot,
            &snapshot.settings,
            snapshot.selection
        ));
        let mut transport = snapshot.settings.clone();
        transport.max_staging_bytes = 0;
        assert!(!packet_matches(&snapshot, &transport, snapshot.selection));
        let mut selection = snapshot.selection;
        selection.max_scratch_bytes = 0;
        assert!(!packet_matches(&snapshot, &snapshot.settings, selection));
        selection = snapshot.selection;
        selection.max_lights = 1;
        assert!(!packet_matches(&snapshot, &snapshot.settings, selection));
    }
    #[test]
    fn pool_caps_reuses_identities_clamps_and_refreshes_layers() {
        let (mut world, owner, mut snapshot) = fixture();
        let mut pool = Pool::default();
        let mut stats = ParticleLightStatistics::default();
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
        assert_eq!((stats.active, stats.allocated), (2, 2));
        assert_eq!(stats.truncated, 1);
        let entities: Vec<_> = pool.slots.iter().map(|s| s.entity).collect();
        snapshot.sequence += 1;
        snapshot.lights.swap(0, 1);
        snapshot.lights[0].position.x = 5.0;
        world.entity_mut(owner).insert(RenderLayers::layer(9));
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
        assert_eq!(
            pool.slots.iter().map(|s| s.entity).collect::<Vec<_>>(),
            entities
        );
        let moved = pool
            .slots
            .iter()
            .find(|s| s.key.as_ref().unwrap().1 == 1)
            .unwrap()
            .entity;
        assert_eq!(world.get::<Transform>(moved).unwrap().translation.x, 5.0);
        for entity in entities {
            let light = world.get::<PointLight>(entity).unwrap();
            assert_eq!(
                (light.intensity, light.range, light.radius),
                (1000.0, 8.0, 8.0)
            );
            assert!(!light.shadow_maps_enabled && !light.contact_shadows_enabled);
            assert_eq!(
                *world.get::<RenderLayers>(entity).unwrap(),
                RenderLayers::layer(9)
            );
        }
        // Drop one selection, then fill its idle slot with another ordinal.
        snapshot.sequence += 1;
        snapshot.lights.remove(0);
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
        assert_eq!(stats.allocated, 2);
        // Global disable removes entities, not just their brightness.
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 0);
        assert_eq!((stats.active, stats.allocated), (0, 0));
    }
    #[test]
    fn late_results_fail_closed_after_seed_restart_replacement_backend_or_removal() {
        for edit in 0..7 {
            let (mut world, owner, snapshot) = fixture();
            let mut pool = Pool::default();
            let mut stats = ParticleLightStatistics::default();
            apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
            match edit {
                0 => world
                    .get_mut::<PresentedEffect>(owner)
                    .unwrap()
                    .instance
                    .seek(0.0),
                1 => world
                    .get_mut::<PresentedEffect>(owner)
                    .unwrap()
                    .instance
                    .set_seed(99),
                2 => world
                    .get_mut::<PresentedEffect>(owner)
                    .unwrap()
                    .instance
                    .invalidate_history(),
                3 => {
                    world.get_mut::<EffectRuntimeStatus>(owner).unwrap().active =
                        ActiveBackend::CpuReference;
                }
                4 => {
                    world.entity_mut(owner).insert(Visibility::Hidden);
                }
                5 => {
                    let compiled = world
                        .get::<PresentedEffect>(owner)
                        .unwrap()
                        .effect()
                        .as_ref()
                        .clone();
                    world
                        .entity_mut(owner)
                        .insert(PresentedEffect::new(Arc::new(compiled)));
                }
                _ => {
                    world.despawn(owner);
                }
            }
            apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
            assert_eq!(stats.active, 0, "edit {edit}");
            assert!(
                pool.slots
                    .iter()
                    .all(|s| world.get::<PointLight>(s.entity).unwrap().intensity == 0.0)
            );
        }
    }
    #[test]
    fn stale_age_and_frame_expire_and_deleted_proxy_recovers() {
        let (mut world, _, mut snapshot) = fixture();
        let mut pool = Pool::default();
        let mut stats = ParticleLightStatistics::default();
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
        let removed = pool.slots[0].entity;
        world.despawn(removed);
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
        assert_eq!((stats.active, stats.allocated), (2, 2));
        assert!(pool.slots.iter().all(|s| s.entity != removed));
        snapshot.captured_at = Instant::now() - Duration::from_secs(1);
        apply_test(&mut world, &mut pool, &mut stats, &snapshot, 2);
        assert_eq!(stats.active, 0);
        snapshot.captured_at = Instant::now();
        apply(
            &mut world,
            &mut pool,
            &mut stats,
            &ParticleLightRealizationSettings::default(),
            &ParticleLightReadbackSettings::default(),
            2,
            100,
            Some(&snapshot),
        );
        assert_eq!(stats.active, 0);
    }
}
