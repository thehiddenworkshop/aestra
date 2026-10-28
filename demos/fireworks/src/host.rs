//! Gameplay owns rockets; Aestra owns their detached visual wakes and burst tails.
use super::{
    ShowRoot,
    project::{Options, SHOW_SEED},
    readiness::Readiness,
};
use aestra_bevy::*;
use aestra_project::ProjectAssetIndex;
use aestra_runtime::INPUT_STOP_EMITTING;
use bevy::prelude::*;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Message)]
#[cfg_attr(not(feature = "fireworks-audio"), allow(dead_code))]
pub struct HostBreak {
    pub position: Vec3,
    pub seed: u64,
}

const SHOW_SECONDS: f32 = 40.0;
const GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

#[derive(Component)]
struct Rocket {
    origin: Vec3,
    velocity: Vec3,
    age: f32,
    fuse: f32,
    wake: Entity,
    family: usize,
    seed: u64,
}
#[derive(Component)]
struct Tail;
#[derive(Resource, Default)]
pub struct HostShow {
    next: usize,
    serial: u64,
    detonated: u64,
    peak_owners: usize,
    peak_rockets: usize,
    capacity_wait_updates: u64,
    current_rockets: usize,
    current_vfx: usize,
    owner_limit: Option<usize>,
}

impl HostShow {
    pub(super) fn measuring_launches(&self, options: &Options) -> bool {
        self.next
            < if options.launch_pattern == "overlap" {
                12
            } else {
                5
            }
            || self.detonated < self.serial
            || self.current_rockets > 0
    }

    pub(super) fn has_vfx(&self) -> bool {
        self.current_vfx > 0
    }

    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"launches":self.serial,"detonations":self.detonated,
            "scheduled_launches":self.next,"peak_gameplay_and_vfx_owners":self.peak_owners,
            "peak_gameplay_rockets":self.peak_rockets,"owner_limit":self.owner_limit,
            "capacity_wait_updates":self.capacity_wait_updates,
            "current_gameplay_rockets":self.current_rockets,"current_vfx_owners":self.current_vfx})
    }
}

/// This is a clock/catalog only: no clip schedules or particle-driven rockets.
pub fn compile(options: &Options) -> Result<Arc<CompiledEffectProject>, String> {
    let mut root = EffectAsset::new("Host-owned rocket demonstration", SHOW_SECONDS);
    root.playback_mode = EffectPlaybackMode::Once;
    let root = EffectCompiler::default()
        .compile(&root)
        .map_err(|e| e.to_string())?;
    let mut dependencies = BTreeMap::new();
    let index = ProjectAssetIndex::scan(&options.project_root);
    for (family, name) in ["point_star", "gold_tail"].into_iter().enumerate() {
        let variant = if options.tuned_smoke {
            "tuned_smoke"
        } else if options.star_lights {
            "lit_smoke"
        } else {
            "smoke"
        };
        let path = options
            .project_root
            .join(format!("effects/fireworks_{name}_{variant}.aestra.ron"));
        let source =
            EffectAsset::from_ron(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        for asset in split(&source, &options.tier, family)? {
            let resolved = index
                .resolve_effect_project(&asset)
                .map_err(|e| e.to_string())?;
            let compiled = EffectCompiler::default()
                .with_tier(QualityTier::preset(&options.tier).expect("validated tier"))
                .compile_resolved_project(&resolved)
                .map_err(|e| e.to_string())?;
            dependencies.insert(compiled.root.source, compiled.root);
        }
    }
    Ok(Arc::new(CompiledEffectProject {
        root: Arc::new(root),
        dependencies,
    }))
}

fn id(family: usize, burst: bool) -> EffectId {
    EffectId::from_u128(
        0xa3574a00_0000_4000_8000_000000f89000 + family as u128 * 2 + u128::from(burst),
    )
}

/// Adapt calibrated editable art at load, then share immutable compiled effects.
/// No material, particle, or trail sampling logic is implemented by the host.
fn split(source: &EffectAsset, tier: &str, family: usize) -> Result<[EffectAsset; 2], String> {
    let source = source.with_particle_budget_profile(tier);
    let launch = source
        .emitters
        .iter()
        .find(|e| e.name == "Launch shell")
        .ok_or("Launch shell missing")?
        .id;
    let mut burst = source.clone();
    burst.id = id(family, true);
    burst.name = format!("{} — host detonation", source.name);
    burst.particle_budgets.clear();
    burst
        .emitters
        .retain(|e| !matches!(e.name.as_str(), "Launch shell" | "Launch smoke"));
    // The host starts this effect at its authoritative detonation position. Replace
    // launch-death births with one initial burst; keep star/ember/smoke links intact.
    for emitter in &mut burst.emitters {
        if let Some(link) = source.events.iter().find(|l| {
            l.source == launch && l.target == emitter.id && l.trigger == EventTrigger::OnDeath
        }) {
            for module in &mut emitter.modules {
                if let ModuleParameters::Emission {
                    spawn_rate,
                    burst_count,
                } = &mut module.parameters
                {
                    *spawn_rate = 0.0;
                    *burst_count = link.count;
                }
            }
        }
    }
    burst.events.retain(|l| l.source != launch);
    let flash = burst
        .emitters
        .iter()
        .find(|e| e.name == "Burst flash")
        .ok_or("Burst flash missing")?
        .id;
    burst
        .particle_outputs
        .retain(|route| route.trigger == EventTrigger::OnDeath);
    for route in &mut burst.particle_outputs {
        route.source = flash;
        route.trigger = EventTrigger::OnSpawn;
    }

    let mut wake = source;
    wake.id = id(family, false);
    wake.name = "Host rocket wake".into();
    wake.duration = 18.0;
    wake.particle_budgets.clear();
    wake.emitters
        .retain(|e| matches!(e.name.as_str(), "Launch shell" | "Launch smoke"));
    wake.events.clear();
    wake.particle_outputs.clear();
    wake.point_lights.clear();
    let binding = EffectBinding::spatial("Rocket", BindingUpdateMode::Live);
    for emitter in &mut wake.emitters {
        let smoke = emitter.name == "Launch smoke";
        emitter.duration = 18.0;
        emitter.max_particles = if smoke { 1024 } else { 64 };
        emitter.attachment = Some(EmitterAttachment {
            binding: binding.id,
            inherit: AttachmentInherit::Position,
        });
        // Each particle is deposited at the live rocket, then evolves independently.
        for module in &mut emitter.modules {
            match &mut module.parameters {
                ModuleParameters::Emission {
                    spawn_rate,
                    burst_count,
                } => {
                    *spawn_rate = if smoke { 45.0 } else { 60.0 };
                    *burst_count = 0;
                }
                ModuleParameters::Initialize {
                    lifetime, speed, ..
                } => {
                    *lifetime = if smoke {
                        ScalarRange::new(7.0, 9.0)
                    } else {
                        ScalarRange::new(0.08, 0.1)
                    };
                    *speed = ScalarRange::new(0.0, 0.0);
                }
                _ => {}
            }
        }
        // A short bright head plus the retained smoke material; the host does not
        // manufacture per-frame trail history or carry old smoke with the rocket.
        if !smoke {
            emitter
                .renderers
                .retain(|r| matches!(r.properties, RendererProperties::Sprite));
        }
    }
    wake.bindings = vec![binding];
    Ok([wake, burst])
}

pub fn install(app: &mut App) {
    app.init_resource::<HostShow>()
        .add_message::<HostBreak>()
        .add_systems(
            Update,
            update
                .before(AestraSet::ResolveHostInputs)
                .after(super::controls),
        );
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    options: Res<Options>,
    readiness: Res<Readiness>,
    project: Res<super::project::ShowProject>,
    mut state: ResMut<HostShow>,
    roots: Query<&EffectPlayer, (With<ShowRoot>, Without<Tail>)>,
    mut rockets: Query<
        (
            Entity,
            &mut Rocket,
            &mut Transform,
            &mut GlobalTransform,
            &mut AestraLinearVelocity,
        ),
        Without<EffectPlayer>,
    >,
    mut tails: Query<(Entity, &mut EffectPlayer), With<Tail>>,
    mut breaks: MessageWriter<HostBreak>,
    reporting: Option<Res<super::report::Report>>,
) {
    if !options.host_driven || !readiness.started {
        return;
    }
    let Ok(clock) = roots.single() else {
        return;
    };
    let interactive = reporting.is_none();
    if interactive && keys.just_pressed(KeyCode::KeyR) {
        for (entity, ..) in &rockets {
            commands.entity(entity).despawn();
        }
        for (entity, _) in &tails {
            commands.entity(entity).despawn();
        }
        *state = HostShow::default();
        return;
    }
    if !clock.playing && clock.elapsed() < clock.effect().duration {
        return;
    }
    for (entity, player) in &tails {
        if player.elapsed() >= player.effect().duration {
            commands.entity(entity).despawn();
        }
    }
    let owners = rockets.iter().count() + tails.iter().count();
    state.owner_limit = options.max_owners;
    state.current_rockets = rockets.iter().count();
    state.current_vfx = tails.iter().count();
    state.peak_owners = state.peak_owners.max(owners);
    state.peak_rockets = state.peak_rockets.max(rockets.iter().count());
    for (entity, mut rocket, mut transform, mut global, mut velocity) in &mut rockets {
        rocket.age = (rocket.age + time.delta_secs()).min(rocket.fuse);
        let position = position(rocket.origin, rocket.velocity, rocket.age);
        transform.translation = position;
        // These host entities are root-level. Publish their current world pose now,
        // before ResolveHostInputs, rather than reading last frame's propagation.
        *global = GlobalTransform::from(*transform);
        velocity.0 = rocket.velocity + GRAVITY * rocket.age;
        if rocket.age >= rocket.fuse {
            if let Ok((_, mut wake)) = tails.get_mut(rocket.wake) {
                wake.send_event(INPUT_STOP_EMITTING, vec![])
                    .expect("built-in stop input");
            }
            let mut burst = EffectPlayer::from_compiled(
                project.0.effect(id(rocket.family, true)).unwrap().clone(),
            )
            .with_history_policy(options.history);
            burst.set_seed(rocket.seed);
            if options.launch_pattern == "overlap" && rocket.family == 0 {
                let parameter = burst
                    .effect()
                    .parameters
                    .iter()
                    .find(|p| p.name == "Main stars color")
                    .expect("calibrated star palette")
                    .source;
                burst
                    .set_parameter(
                        parameter,
                        Value::Gradient(aestra_bevy::Gradient::new(vec![
                            ColorKey::new(0.0, [0.25, 0.6, 1.0, 1.0]),
                            ColorKey::new(0.65, [0.12, 0.4, 1.0, 1.0]),
                            ColorKey::new(1.0, [0.04, 0.08, 0.25, 1.0]),
                        ])),
                    )
                    .expect("gradient parameter");
            }
            breaks.write(HostBreak {
                position,
                seed: rocket.seed,
            });
            state.detonated += 1;
            commands.spawn((burst, Tail, Transform::from_translation(position)));
            commands.entity(entity).despawn();
        }
    }
    let overlap = options.launch_pattern == "overlap";
    let scheduled = state.next < if overlap { 12 } else { 5 }
        && clock.elapsed() >= state.next as f32 * if overlap { 0.35 } else { 3.0 };
    let requested = scheduled || (interactive && keys.just_pressed(KeyCode::Enter));
    let admitted = options.max_owners.is_none_or(|limit| owners + 2 <= limit);
    if clock.playing && requested && !admitted {
        state.capacity_wait_updates += 1;
    }
    if clock.playing && requested && admitted {
        let family = state.serial as usize % 2;
        let lane = (state.serial % 3) as f32 - 1.0;
        let origin = Vec3::new(
            lane * 24.0,
            0.0,
            if overlap {
                (state.serial % 2) as f32 * -8.0
            } else {
                0.0
            },
        );
        let velocity = Vec3::new(lane * 5.0, 42.0, 0.0);
        let seed = SHOW_SEED.wrapping_add(state.serial);
        let entity = commands
            .spawn((
                Transform::from_translation(origin),
                GlobalTransform::from_translation(origin),
                AestraLinearVelocity(velocity),
            ))
            .id();
        let mut player =
            EffectPlayer::from_compiled(project.0.effect(id(family, false)).unwrap().clone())
                .with_history_policy(options.history);
        player.set_seed(seed);
        let wake = commands
            .spawn((
                player,
                Tail,
                AestraBindings::new().bind("Rocket", entity),
                Transform::default(),
            ))
            .id();
        commands.entity(entity).insert(Rocket {
            origin,
            velocity,
            age: 0.0,
            fuse: if overlap {
                [1.25, 1.45, 1.7][state.serial as usize % 3]
            } else {
                1.3
            },
            wake,
            family,
            seed,
        });
        state.serial += 1;
        state.peak_owners = state.peak_owners.max(owners + 2);
        if scheduled {
            state.next += 1;
        }
    }
}

fn position(origin: Vec3, velocity: Vec3, age: f32) -> Vec3 {
    origin + velocity * age + GRAVITY * (0.5 * age * age)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlap_admission_is_only_bounded_when_the_host_explicitly_requests_it() {
        let options = Options::parse([
            "--launch-pattern".into(),
            "overlap".into(),
            "--tier".into(),
            "low".into(),
            "--max-owners".into(),
            "24".into(),
        ])
        .unwrap();
        let catalog = compile(&options).unwrap();
        let mut app = App::new();
        app.insert_resource(options)
            .insert_resource(super::super::project::ShowProject(catalog.clone()))
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<HostShow>()
            .init_resource::<Readiness>()
            .add_message::<HostBreak>()
            .add_systems(Update, update);
        app.world_mut().resource_mut::<Readiness>().started = true;
        let clock = app
            .world_mut()
            .spawn((ShowRoot, EffectPlayer::from_compiled(catalog.root.clone())))
            .id();
        for frame in 0..20 {
            app.world_mut()
                .get_mut::<EffectPlayer>(clock)
                .unwrap()
                .seek(frame as f32 * 0.35);
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(0.35));
            app.update();
        }
        let state = app.world().resource::<HostShow>();
        assert_eq!(state.serial, 12);
        assert_eq!(state.detonated, 12);
        assert_eq!(state.next, 12);
        assert_eq!(state.peak_owners, 24);
        assert!(state.peak_rockets > 1);
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<Rocket>>()
                .iter(world)
                .count(),
            0
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<Tail>>()
                .iter(world)
                .count(),
            24
        );
        // Retained tails still occupy their budget: a manual request cannot
        // overwrite a live owner or allocate beyond the host ceiling.
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world().resource::<HostShow>().serial, 12);
        assert_eq!(app.world().resource::<HostShow>().capacity_wait_updates, 1);
        app.world_mut().resource_mut::<Options>().max_owners = None;
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.reset_all();
        keys.press(KeyCode::Enter);
        app.update();
        assert_eq!(app.world().resource::<HostShow>().serial, 13);
        assert_eq!(app.world().resource::<HostShow>().peak_owners, 26);
        assert_eq!(app.world().resource::<HostShow>().owner_limit, None);
    }
    #[test]
    fn host_bursts_keep_authored_star_outputs_and_wakes_have_no_light_sources() {
        for (tier, cap) in [("high", 32), ("medium", 16), ("low", 24)] {
            let options =
                Options::parse(["--star-lights".into(), "--tier".into(), tier.into()]).unwrap();
            let project = compile(&options).unwrap();
            for family in 0..2 {
                let wake = project.effect(id(family, false)).unwrap();
                assert!(wake.emitters.iter().all(|e| e.scene_outputs.is_empty()));
                let burst = project.effect(id(family, true)).unwrap();
                for emitter in &burst.emitters {
                    assert_eq!(
                        emitter.scene_outputs.len(),
                        usize::from(emitter.name == "Main stars")
                    );
                    for output in &emitter.scene_outputs {
                        let aestra_runtime::SceneOutputPlanKind::ParticlePointLight(light) =
                            &output.kind;
                        assert_eq!(light.max_lights, cap);
                    }
                }
            }
        }
    }
    #[test]
    fn gameplay_detonates_once_pauses_restarts_and_leaves_finite_tails() {
        let options = Options::parse([]).unwrap();
        let catalog = compile(&options).unwrap();
        let mut app = App::new();
        app.insert_resource(options)
            .insert_resource(super::super::project::ShowProject(catalog.clone()))
            .init_resource::<Time>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<HostShow>()
            .init_resource::<Readiness>()
            .add_message::<HostBreak>()
            .add_systems(Update, update);
        app.world_mut().resource_mut::<Readiness>().started = true;
        let clock = app
            .world_mut()
            .spawn((ShowRoot, EffectPlayer::from_compiled(catalog.root.clone())))
            .id();
        app.update();
        let (rocket, wake) = {
            let world = app.world_mut();
            let (entity, rocket) = world.query::<(Entity, &Rocket)>().single(world).unwrap();
            (entity, rocket.wake)
        };
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.5));
        app.update();
        let pose = *app.world().get::<Transform>(rocket).unwrap();
        assert!(pose.translation.y > 15.0);
        assert_eq!(
            pose.translation,
            app.world()
                .get::<GlobalTransform>(rocket)
                .unwrap()
                .translation()
        );
        assert_eq!(
            app.world().get::<Transform>(wake).unwrap().translation,
            Vec3::ZERO
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(clock)
            .unwrap()
            .playing = false;
        app.update();
        assert_eq!(*app.world().get::<Transform>(rocket).unwrap(), pose);
        app.world_mut()
            .get_mut::<EffectPlayer>(clock)
            .unwrap()
            .playing = true;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(2));
        app.update();
        assert!(app.world().get_entity(rocket).is_err());
        assert!(
            app.world()
                .get::<EffectPlayer>(wake)
                .unwrap()
                .instance()
                .emission_cutoffs()
                .stop_tick
                .is_some()
        );
        assert_eq!(app.world().resource::<Messages<HostBreak>>().len(), 1);
        {
            let world = app.world_mut();
            let mut tails = world.query_filtered::<&mut EffectPlayer, With<Tail>>();
            assert_eq!(
                tails.iter(world).count(),
                2,
                "wake and burst outlive gameplay entity"
            );
            for mut tail in tails.iter_mut(world) {
                let duration = tail.effect().duration;
                tail.seek(duration);
            }
        }
        app.update();
        assert_eq!(
            app.world().resource::<Messages<HostBreak>>().len(),
            1,
            "no second detonation"
        );
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<Tail>>()
                .iter(world)
                .count(),
            0
        );
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyR);
        app.update();
        assert_eq!(app.world().resource::<HostShow>().serial, 0);
        assert!(app.world().get_entity(clock).is_ok());
    }
    #[test]
    fn host_catalog_has_no_choreography_and_each_tier_compiles_with_detached_wakes() {
        for tier in ["high", "medium", "low"] {
            let options = Options::parse(["--tier".into(), tier.into()]).unwrap();
            let project = compile(&options).unwrap();
            assert!(project.root.effect_clips.is_empty());
            assert!(project.root.emitters.is_empty());
            assert_eq!(project.dependencies.len(), 4);
            for family in 0..2 {
                let wake = project.effect(id(family, false)).unwrap();
                assert_eq!(wake.bindings.len(), 1);
                assert!(wake.emitters.iter().all(|e| e.attachment.is_some()));
                let burst = project.effect(id(family, true)).unwrap();
                assert!(burst.emitters.iter().all(|e| e.name != "Launch shell"));
            }
        }
    }
    #[test]
    fn host_trajectory_uses_launch_inputs_and_exact_fuse_pose() {
        let origin = Vec3::new(10.0, 5.0, -2.0);
        let velocity = Vec3::new(4.0, 42.0, 0.0);
        assert_eq!(position(origin, velocity, 0.0), origin);
        assert!((position(origin, velocity, 1.3).x - 15.2).abs() < 0.0001);
        assert!(position(origin, velocity, 1.3).y > 45.0);
    }
}
