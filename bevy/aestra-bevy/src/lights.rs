//! Opt-in bounded realization of engine-neutral scene-light intents.
use crate::{AestraOutputEvent, AestraSet, EffectClipId, EffectPlayer, EventOrigin};
use aestra_runtime::{PointLightPulse, PointLightSample, TransientPointLight};
use bevy::{camera::visibility::RenderLayers, prelude::*};

pub const MAX_TRANSIENT_LIGHTS: usize = 64;
const MAX_REQUESTS_PER_FRAME: usize = 1024;

/// Stable identity for one representative pulse in a root playback epoch.
/// Particle conversion is for FirstPerTick, not one light for every EachEvent position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientLightKey {
    pub clip_path: Vec<EffectClipId>,
    pub output: String,
    pub emitter: usize,
    pub tick: u64,
}

#[derive(Message, Debug, Clone)]
pub struct AestraLightOutput {
    pub root: Entity,
    pub playback_epoch: u32,
    pub key: TransientLightKey,
    pub light: TransientPointLight,
}

impl AestraLightOutput {
    /// Host binding from a selected particle cue to one light, irrespective of
    /// `event.magnitude`. Never interprets a legacy output as a spatial particle.
    pub fn from_particle(output: &AestraOutputEvent, pulse: PointLightPulse) -> Option<Self> {
        let spatial = output.particle.as_ref()?;
        let playback_epoch = output.playback_epoch?;
        let EventOrigin::Emitter(emitter) = output.event.origin else {
            return None;
        };
        let light = TransientPointLight {
            world_position: spatial.world_position?,
            root_time_seconds: spatial.root_time_seconds,
            pulse,
        };
        light.is_valid().then(|| Self {
            root: output.effect,
            playback_epoch,
            key: TransientLightKey {
                clip_path: output.clip_path.clone(),
                output: output.event.kind.clone(),
                emitter,
                tick: output.event.tick,
            },
            light,
        })
    }
}

/// Global host override. At saturation new intents are dropped, never one proxy
/// per source particle. Limits are clamped to portable adapter ceilings.
#[derive(Resource, Debug, Clone)]
pub struct TransientLightSettings {
    pub enabled: bool,
    pub max_lights: usize,
    pub max_requests_per_frame: usize,
    pub max_lumens: f32,
    pub max_range: f32,
}
impl Default for TransientLightSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_lights: 16,
            max_requests_per_frame: 128,
            max_lumens: 1_000_000.0,
            max_range: 200.0,
        }
    }
}

/// Cumulative admission counters plus the current bounded pool size.
#[derive(Resource, Debug, Default, Clone)]
pub struct TransientLightStatistics {
    pub allocated: usize,
    pub active: usize,
    pub peak_active: usize,
    pub accepted: u64,
    pub duplicate: u64,
    pub budget_dropped: u64,
    pub invalid: u64,
    pub stale: u64,
    pub expired: u64,
    pub disabled: u64,
}

/// Adapter-owned pooled entity. Hosts can inspect lights, not use it as a cue ID.
#[derive(Component)]
pub struct TransientLightProxy;

struct Slot {
    entity: Entity,
    request: Option<AestraLightOutput>,
}
#[derive(Resource, Default)]
struct Pool(Vec<Slot>);

/// Add explicitly after `AestraPlugin`. No lights exist until an admitted intent.
/// Shadows are always disabled in this first adapter slice.
pub struct AestraTransientLightPlugin;
impl Plugin for AestraTransientLightPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AestraLightOutput>()
            .init_resource::<TransientLightSettings>()
            .init_resource::<TransientLightStatistics>()
            .init_resource::<Pool>()
            .configure_sets(Update, AestraSet::SceneOutputs.after(AestraSet::Playback))
            .add_systems(Update, realize.in_set(AestraSet::SceneOutputs));
    }
}

fn rendered(sample: PointLightSample, settings: &TransientLightSettings) -> PointLight {
    PointLight {
        color: Color::linear_rgb(
            sample.linear_color[0],
            sample.linear_color[1],
            sample.linear_color[2],
        ),
        intensity: sample.intensity_lumens.min(settings.max_lumens),
        range: sample.range.min(settings.max_range),
        radius: sample.radius.min(sample.range.min(settings.max_range)),
        shadow_maps_enabled: false,
        contact_shadows_enabled: false,
        ..default()
    }
}

fn realize(
    mut commands: Commands,
    settings: Res<TransientLightSettings>,
    mut stats: ResMut<TransientLightStatistics>,
    mut pool: ResMut<Pool>,
    mut requests: MessageReader<AestraLightOutput>,
    roots: Query<(&EffectPlayer, Option<&RenderLayers>)>,
    mut proxies: Query<
        (&mut PointLight, &mut Transform, &mut Visibility),
        With<TransientLightProxy>,
    >,
) {
    let valid_settings = settings.max_lumens.is_finite()
        && settings.max_lumens >= 0.0
        && settings.max_range.is_finite()
        && settings.max_range > 0.0;
    let enabled = settings.enabled && valid_settings;
    let limit = settings.max_lights.min(MAX_TRANSIENT_LIGHTS);
    // A host deleting a pooled entity cannot leave an unusable slot at saturation.
    pool.0.retain(|slot| {
        if proxies.contains(slot.entity) {
            true
        } else {
            // Also clean up a still-live proxy whose required components the host removed.
            commands.entity(slot.entity).try_despawn();
            false
        }
    });
    let keep = limit.min(pool.0.len());
    for slot in pool.0.drain(keep..) {
        commands.entity(slot.entity).try_despawn();
    }
    for slot in &mut pool.0 {
        let sample = slot.request.as_ref().and_then(|request| {
            let (player, layers) = roots.get(request.root).ok()?;
            (enabled && player.instance().history_epoch() == request.playback_epoch)
                .then(|| {
                    request
                        .light
                        .sample(player.instance().time())
                        .map(|sample| (sample, layers.cloned().unwrap_or_default()))
                })
                .flatten()
        });
        let Ok((mut light, mut transform, mut visibility)) = proxies.get_mut(slot.entity) else {
            continue;
        };
        if let Some((sample, layers)) = sample {
            *light = rendered(sample, &settings);
            commands.entity(slot.entity).insert(layers);
            transform.translation =
                Vec3::from_array(slot.request.as_ref().unwrap().light.world_position);
            *visibility = Visibility::Visible;
        } else {
            slot.request = None;
            light.intensity = 0.0;
            *visibility = Visibility::Hidden;
        }
    }
    let request_limit = settings.max_requests_per_frame.min(MAX_REQUESTS_PER_FRAME);
    stats.budget_dropped += requests.len().saturating_sub(request_limit) as u64;
    for request in requests.read().take(request_limit) {
        if !enabled {
            stats.disabled += 1;
            continue;
        }
        if !request.light.is_valid()
            || request.key.clip_path.len() > 63
            || request.key.output.len() > 256
        {
            stats.invalid += 1;
            continue;
        }
        let Ok((player, layers)) = roots.get(request.root) else {
            stats.stale += 1;
            continue;
        };
        if player.instance().history_epoch() != request.playback_epoch {
            stats.stale += 1;
            continue;
        }
        let Some(sample) = request.light.sample(player.instance().time()) else {
            stats.expired += 1;
            continue;
        };
        if pool.0.iter().any(|slot| {
            slot.request.as_ref().is_some_and(|old| {
                old.root == request.root
                    && old.playback_epoch == request.playback_epoch
                    && old.key == request.key
            })
        }) {
            stats.duplicate += 1;
            continue;
        }
        let reusable = pool.0.iter().position(|slot| slot.request.is_none());
        let light = rendered(sample, &settings);
        let transform = Transform::from_translation(Vec3::from_array(request.light.world_position));
        if let Some(index) = reusable {
            let slot = &mut pool.0[index];
            if let Ok((mut proxy, mut pose, mut visibility)) = proxies.get_mut(slot.entity) {
                *proxy = light;
                *pose = transform;
                *visibility = Visibility::Visible;
            }
            commands
                .entity(slot.entity)
                .insert(layers.cloned().unwrap_or_default());
            slot.request = Some(request.clone());
        } else if pool.0.len() < limit {
            let entity = commands
                .spawn((
                    TransientLightProxy,
                    light,
                    transform,
                    Visibility::Visible,
                    layers.cloned().unwrap_or_default(),
                ))
                .id();
            pool.0.push(Slot {
                entity,
                request: Some(request.clone()),
            });
        } else {
            stats.budget_dropped += 1;
            continue;
        }
        stats.accepted += 1;
    }
    // Drop excess intents in O(1), never defer an unbounded backlog to later frames.
    requests.clear();
    stats.allocated = pool.0.len();
    stats.active = pool.0.iter().filter(|slot| slot.request.is_some()).count();
    stats.peak_active = stats.peak_active.max(stats.active);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EffectAsset, EffectCompiler};
    use std::sync::Arc;

    fn app() -> (App, Entity) {
        let mut app = App::new();
        app.add_plugins(AestraTransientLightPlugin);
        let effect = Arc::new(
            EffectCompiler::default()
                .compile(&EffectAsset::new("light", 10.0))
                .unwrap(),
        );
        let mut player = EffectPlayer::from_compiled(effect);
        player.playing = false;
        let root = app
            .world_mut()
            .spawn((
                player,
                Transform::from_xyz(100.0, 0.0, 0.0),
                RenderLayers::layer(7),
            ))
            .id();
        (app, root)
    }
    fn request(app: &App, root: Entity, tick: u64) -> AestraLightOutput {
        let player = app.world().get::<EffectPlayer>(root).unwrap();
        AestraLightOutput {
            root,
            playback_epoch: player.instance().history_epoch(),
            key: TransientLightKey {
                clip_path: vec![],
                output: "impact".into(),
                emitter: 0,
                tick,
            },
            light: TransientPointLight {
                world_position: [4.0, 2.0, 1.0],
                root_time_seconds: player.instance().time(),
                pulse: PointLightPulse::flash([1.0, 0.2, 0.1], 1000.0, 20.0, 0.5),
            },
        }
    }
    fn send(app: &mut App, request: AestraLightOutput) {
        app.world_mut().write_message(request);
    }
    fn entities(app: &mut App) -> Vec<Entity> {
        app.world_mut()
            .query_filtered::<Entity, With<TransientLightProxy>>()
            .iter(app.world())
            .collect()
    }

    #[test]
    fn pool_is_bounded_reused_world_space_shadowless_and_paused_with_root() {
        let (mut app, root) = app();
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .max_lights = 2;
        for tick in 1..=12 {
            let light = request(&app, root, tick);
            send(&mut app, light);
        }
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().accepted,
            2
        );
        assert_eq!(
            app.world()
                .resource::<TransientLightStatistics>()
                .budget_dropped,
            10
        );
        let pool = entities(&mut app);
        assert_eq!(pool.len(), 2);
        for entity in &pool {
            assert_eq!(
                app.world().get::<Transform>(*entity).unwrap().translation,
                Vec3::new(4.0, 2.0, 1.0)
            );
            assert!(app.world().get::<ChildOf>(*entity).is_none());
            assert_eq!(
                app.world().get::<RenderLayers>(*entity),
                Some(&RenderLayers::layer(7))
            );
            let light = app.world().get::<PointLight>(*entity).unwrap();
            assert_eq!(light.intensity, 1000.0);
            assert!(!light.shadow_maps_enabled && !light.contact_shadows_enabled);
        }
        app.update();
        assert_eq!(
            app.world().get::<PointLight>(pool[0]).unwrap().intensity,
            1000.0
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .advance_clock(0.1);
        app.world_mut()
            .entity_mut(root)
            .insert(RenderLayers::layer(9));
        app.update();
        assert!((app.world().get::<PointLight>(pool[0]).unwrap().intensity - 250.0).abs() < 0.001);
        assert_eq!(
            app.world().get::<RenderLayers>(pool[0]),
            Some(&RenderLayers::layer(9))
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .advance_clock(0.5);
        app.update();
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
        assert_eq!(
            app.world().get::<PointLight>(pool[0]).unwrap().intensity,
            0.0
        );
        let next = request(&app, root, 20);
        send(&mut app, next);
        app.update();
        assert_eq!(
            entities(&mut app),
            pool,
            "reuse, do not churn entities between bursts"
        );
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .max_lights = 1;
        app.update();
        assert_eq!(entities(&mut app).len(), 1);
        app.world_mut().entity_mut(root).despawn();
        app.update();
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
    }

    #[test]
    fn seek_restart_duplicates_delayed_expired_and_disabled_intents_are_safe() {
        let (mut app, root) = app();
        let first = request(&app, root, 1);
        send(&mut app, first.clone());
        send(&mut app, first.clone());
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().duplicate,
            1
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .seek_simulation_time(0.1);
        send(&mut app, first);
        app.update();
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
        assert_eq!(app.world().resource::<TransientLightStatistics>().stale, 1);
        let mut delayed = request(&app, root, 2);
        delayed.light.root_time_seconds = 0.0;
        send(&mut app, delayed.clone());
        app.update();
        let entity = entities(&mut app)[0];
        assert!(
            (app.world().get::<PointLight>(entity).unwrap().intensity - 250.0).abs() < 0.001,
            "delayed delivery must not restart the flash at its peak"
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .restart();
        app.update();
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
        let next = request(&app, root, 3);
        send(&mut app, next);
        app.update();
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .enabled = false;
        let next = request(&app, root, 4);
        send(&mut app, next);
        app.update();
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 0);
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().disabled,
            1
        );
        app.world_mut()
            .resource_mut::<TransientLightSettings>()
            .enabled = true;
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .advance_clock(1.0);
        delayed.playback_epoch = app
            .world()
            .get::<EffectPlayer>(root)
            .unwrap()
            .instance()
            .history_epoch();
        send(&mut app, delayed);
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().expired,
            1
        );
    }

    #[test]
    fn frame_limits_invalid_data_and_external_deletion_do_not_break_the_pool() {
        let (mut app, root) = app();
        {
            let mut settings = app.world_mut().resource_mut::<TransientLightSettings>();
            settings.max_requests_per_frame = 2;
            settings.max_lights = 1;
            settings.max_lumens = 100.0;
            settings.max_range = 5.0;
        }
        let mut invalid = request(&app, root, 0);
        invalid.light.world_position[0] = f32::NAN;
        send(&mut app, invalid);
        for tick in 1..=10 {
            let light = request(&app, root, tick);
            send(&mut app, light);
        }
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().invalid,
            1
        );
        assert_eq!(
            app.world()
                .resource::<TransientLightStatistics>()
                .budget_dropped,
            9
        );
        let old = entities(&mut app)[0];
        assert_eq!(app.world().get::<PointLight>(old).unwrap().intensity, 100.0);
        assert_eq!(app.world().get::<PointLight>(old).unwrap().range, 5.0);
        app.world_mut().entity_mut(old).despawn();
        let next = request(&app, root, 20);
        send(&mut app, next);
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().allocated,
            1
        );
        assert_ne!(entities(&mut app)[0], old);
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().accepted,
            2,
            "excess frame intents are discarded, not queued"
        );
        let broken = entities(&mut app)[0];
        app.world_mut().entity_mut(broken).remove::<PointLight>();
        let next = request(&app, root, 21);
        send(&mut app, next);
        app.update();
        assert!(
            app.world().get_entity(broken).is_err(),
            "partial proxy removal must not orphan an entity"
        );
        assert_eq!(entities(&mut app).len(), 1);
    }

    #[test]
    fn root_epochs_are_isolated_and_host_limits_cannot_exceed_adapter_ceilings() {
        let (mut app, first) = app();
        let effect = app
            .world()
            .get::<EffectPlayer>(first)
            .unwrap()
            .effect()
            .clone();
        let mut player = EffectPlayer::from_compiled(effect);
        player.playing = false;
        let second = app.world_mut().spawn((player, RenderLayers::layer(9))).id();
        for root in [first, second] {
            let intent = request(&app, root, 1);
            send(&mut app, intent);
        }
        app.update();
        assert_eq!(app.world().resource::<TransientLightStatistics>().active, 2);
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().duplicate,
            0
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(first)
            .unwrap()
            .restart();
        app.update();
        assert_eq!(
            app.world().resource::<TransientLightStatistics>().active,
            1,
            "one root restart must not clear another root's light"
        );
        {
            let mut settings = app.world_mut().resource_mut::<TransientLightSettings>();
            settings.max_lights = usize::MAX;
            settings.max_requests_per_frame = usize::MAX;
        }
        for tick in 0..=MAX_REQUESTS_PER_FRAME {
            let intent = request(&app, first, tick as u64 + 10);
            send(&mut app, intent);
        }
        app.update();
        let stats = app.world().resource::<TransientLightStatistics>();
        assert_eq!(stats.active, MAX_TRANSIENT_LIGHTS);
        assert_eq!(stats.allocated, MAX_TRANSIENT_LIGHTS);
        assert_eq!(stats.accepted, MAX_TRANSIENT_LIGHTS as u64 + 1);
        assert_eq!(
            stats.budget_dropped,
            (MAX_REQUESTS_PER_FRAME + 2 - MAX_TRANSIENT_LIGHTS) as u64
        );
        assert_eq!(entities(&mut app).len(), MAX_TRANSIENT_LIGHTS);
    }
}
