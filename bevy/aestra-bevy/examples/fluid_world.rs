//! Fluids in the game's world (fluid F11): smoke flows around level geometry the game supplies, and a
//! projectile splashing into a pool reaches gameplay as an impact event.
//!
//! - The level — a basin, and an overhang on two pillars — is ordinary Bevy meshes. At startup their
//!   triangles are baked into one world SDF ([`sdf_from_meshes`]) and handed to Aestra
//!   ([`AestraWorldSdf`]); both fluids declare a World Collider, so they collide with it.
//! - A smoke plume rises under the overhang and spills around it.
//! - A pool of water fills the basin. The projectile is bound to the pool's Sphere Collider (its
//!   position and velocity, through an `AestraBindings` slot): the water parts around it, and when it
//!   hits the water the collider's force rises past its Impact Threshold — the pool effect raises an
//!   `impact` event, which reaches the gameplay system below as an [`AestraOutputEvent`] message.
//!
//! ```sh
//! cargo run -p aestra-bevy --example fluid_world --release
//! ```
//!
//! With `AESTRA_EXAMPLE_CAPTURE=<file.png>` the example saves a screenshot after four seconds, prints
//! what it heard, and exits.

use aestra_bevy::{
    AESTRA_FIELD_LINEAR_VELOCITY, AESTRA_FIELD_POSITION, AestraBindings, AestraEffectOutputs,
    AestraLinearVelocity, AestraOutputEvent, AestraPlugin, AestraWorldSdf, BindingFieldId,
    BindingUpdateMode, EffectAsset, EffectBinding, EffectPlaybackMode, EffectPlayer,
    ExtensionRegistry, HostFieldRef, ModuleParameters, ModuleTypeId, PropertySource, StageKind,
    Value, sdf_from_meshes,
};
use aestra_fluid::{
    EVENT_IMPACT, MODULE_BUOYANCY, MODULE_DENSITY_SOURCE, MODULE_GRID, MODULE_LIQUID_BLOCK,
    MODULE_LIQUID_GRID, MODULE_LIQUID_LOOK, MODULE_SPHERE_COLLIDER, MODULE_VORTICITY,
    MODULE_WORLD_COLLIDER, OUTPUT_FORCE,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

/// Where the pool and the smoke stand, in world space.
const POOL: Vec3 = Vec3::new(-70.0, 0.0, 0.0);
const SMOKE: Vec3 = Vec3::new(70.0, 0.0, 0.0);
/// The projectile's flight: from high to the left of the pool down into its middle, then again.
const LAUNCH: Vec3 = Vec3::new(-150.0, 90.0, 0.0);
const SPEED: f32 = 110.0;
const RADIUS: f32 = 4.0;
/// The force the water must put on the projectile to count as an impact.
const IMPACT: f32 = 20_000.0;

#[derive(Component)]
struct Projectile {
    /// Seconds into the current flight.
    flight: f32,
    /// Seconds of red after an impact.
    flash: f32,
}

#[derive(Component)]
struct Pool {
    collider: aestra_bevy::ModuleId,
}

#[derive(Component)]
struct Readout;

#[derive(Resource, Default)]
struct Heard {
    impacts: u32,
    strongest: f32,
    frames: u32,
}

fn main() {
    // The fluid extension, so the effects below compile and run.
    aestra_fluid::link();
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .init_resource::<Heard>()
        .add_systems(Startup, setup)
        .add_systems(Update, (fly, hear_impacts, show, capture))
        .run();
}

/// A module of `type_id`, in the effect's domain, with `inputs` set.
fn module(
    registry: &ExtensionRegistry,
    effect: &mut EffectAsset,
    type_id: &str,
    inputs: &[(&str, Value)],
) -> aestra_bevy::ModuleId {
    let mut module = registry
        .modules
        .instantiate(&ModuleTypeId::new(type_id))
        .expect("the fluid extension is linked");
    module.stage = StageKind::Simulation(effect.simulation_stages[0].name.clone());
    set(&mut module.parameters, inputs);
    let id = module.id;
    effect.simulation_stages[0].modules.push(module);
    id
}

fn set(parameters: &mut ModuleParameters, inputs: &[(&str, Value)]) {
    let ModuleParameters::Custom(values) = parameters else {
        unreachable!("plugin modules carry a generic payload");
    };
    for (name, value) in inputs {
        values.insert((*name).into(), value.clone());
    }
}

fn set_existing(effect: &mut EffectAsset, type_id: &str, inputs: &[(&str, Value)]) {
    let module = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == type_id)
        .expect("the effect has the module");
    set(&mut module.parameters, inputs);
}

/// Smoke rising from a source on the floor, flowing around the world.
fn smoke(registry: &ExtensionRegistry) -> EffectAsset {
    let mut effect = aestra_fluid::smoke_effect(registry);
    effect.name = "World Smoke".into();
    effect.duration = 8.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.emitters.clear();
    set_existing(
        &mut effect,
        MODULE_GRID,
        &[
            ("resolution", Value::U32(32)),
            ("cell_size", Value::Scalar(3.0)),
            ("center", Value::Vec3([0.0, 48.0, 0.0])),
            ("open_top", Value::Bool(true)),
            ("open_sides", Value::Bool(true)),
            ("density_dissipation", Value::Scalar(0.15)),
        ],
    );
    set_existing(
        &mut effect,
        MODULE_DENSITY_SOURCE,
        &[
            ("position", Value::Vec3([0.0, 8.0, 0.0])),
            ("radius", Value::Scalar(9.0)),
            ("velocity", Value::Vec3([0.0, 35.0, 0.0])),
            ("density_rate", Value::Scalar(3.0)),
        ],
    );
    set_existing(
        &mut effect,
        MODULE_BUOYANCY,
        &[("strength", Value::Scalar(3.0))],
    );
    set_existing(
        &mut effect,
        MODULE_VORTICITY,
        &[("strength", Value::Scalar(0.4))],
    );
    module(registry, &mut effect, MODULE_WORLD_COLLIDER, &[]);
    effect
}

/// Water filling the basin, and the projectile as a sphere collider that reports an impact.
fn pool(registry: &ExtensionRegistry) -> (EffectAsset, aestra_bevy::ModuleId) {
    let mut effect = aestra_fluid::liquid_effect(registry);
    effect.name = "World Pool".into();
    effect.duration = 8.0;
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.emitters.clear();
    set_existing(
        &mut effect,
        MODULE_LIQUID_GRID,
        &[
            ("resolution", Value::U32(32)),
            ("cell_size", Value::Scalar(2.5)),
            ("center", Value::Vec3([0.0, 40.0, 0.0])),
        ],
    );
    set_existing(
        &mut effect,
        MODULE_LIQUID_BLOCK,
        &[
            ("center", Value::Vec3([0.0, 14.0, 0.0])),
            ("size", Value::Vec3([64.0, 20.0, 64.0])),
        ],
    );
    module(registry, &mut effect, MODULE_WORLD_COLLIDER, &[]);
    module(
        registry,
        &mut effect,
        MODULE_LIQUID_LOOK,
        &[("steps", Value::U32(96))],
    );
    let mut projectile = EffectBinding::spatial("Projectile", BindingUpdateMode::Live);
    projectile
        .optional_fields
        .insert(BindingFieldId::new(AESTRA_FIELD_LINEAR_VELOCITY));
    let collider = module(
        registry,
        &mut effect,
        MODULE_SPHERE_COLLIDER,
        &[
            ("radius", Value::Scalar(RADIUS)),
            ("impact_threshold", Value::Scalar(IMPACT)),
        ],
    );
    let sphere = effect.simulation_stages[0]
        .modules
        .iter_mut()
        .find(|module| module.id == collider)
        .expect("just added");
    for (input, field) in [
        ("position", AESTRA_FIELD_POSITION),
        ("velocity", AESTRA_FIELD_LINEAR_VELOCITY),
    ] {
        sphere
            .property_sources
            .insert(input.into(), PropertySource::HostBinding);
        sphere
            .host_bindings
            .insert(input.into(), HostFieldRef::new(projectile.id, field));
    }
    effect.bindings.push(projectile);
    (effect, collider)
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let registry = ExtensionRegistry::linked();
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 95.0, 250.0).looking_at(Vec3::new(0.0, 35.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 8_000.0,
            ..default()
        },
        Transform::from_xyz(-50.0, 120.0, 80.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // The level: a basin (a floor and four walls) around the pool, and an overhang on two pillars
    // over the smoke. Drawn as meshes, and baked into the world the fluids collide with.
    let stone = materials.add(StandardMaterial {
        base_color: Color::srgb(0.45, 0.43, 0.4),
        perceptual_roughness: 0.9,
        ..default()
    });
    let pieces = [
        (Vec3::new(80.0, 4.0, 80.0), POOL + Vec3::new(0.0, 2.0, 0.0)),
        (
            Vec3::new(4.0, 30.0, 80.0),
            POOL + Vec3::new(-38.0, 15.0, 0.0),
        ),
        (
            Vec3::new(4.0, 30.0, 80.0),
            POOL + Vec3::new(38.0, 15.0, 0.0),
        ),
        (
            Vec3::new(72.0, 30.0, 4.0),
            POOL + Vec3::new(0.0, 15.0, -38.0),
        ),
        (
            Vec3::new(72.0, 30.0, 4.0),
            POOL + Vec3::new(0.0, 15.0, 38.0),
        ),
        (
            Vec3::new(56.0, 6.0, 56.0),
            SMOKE + Vec3::new(12.0, 50.0, 0.0),
        ),
        (
            Vec3::new(8.0, 47.0, 8.0),
            SMOKE + Vec3::new(34.0, 23.5, 20.0),
        ),
        (
            Vec3::new(8.0, 47.0, 8.0),
            SMOKE + Vec3::new(34.0, 23.5, -20.0),
        ),
    ];
    let mut level = Vec::new();
    for (size, at) in pieces {
        let mesh = Mesh::from(Cuboid::from_size(size));
        let transform = Transform::from_translation(at);
        level.push((mesh.clone(), GlobalTransform::from(transform)));
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(stone.clone()),
            transform,
        ));
    }
    let placed: Vec<_> = level
        .iter()
        .map(|(mesh, transform)| (mesh, *transform))
        .collect();
    let world = sdf_from_meshes(&placed, 2.0, 4.0).expect("the level bakes");
    info!(
        "world SDF: {:?} voxels of {} units",
        world.dims, world.voxel_size
    );
    commands.insert_resource(AestraWorldSdf::new(&world));

    // The projectile, bound to the pool's sphere collider.
    let projectile = commands
        .spawn((
            Projectile {
                flight: 0.0,
                flash: 0.0,
            },
            Mesh3d(meshes.add(Sphere::new(RADIUS))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.9, 0.8, 0.3),
                emissive: LinearRgba::rgb(1.5, 1.0, 0.2),
                ..default()
            })),
            Transform::from_translation(LAUNCH),
            AestraLinearVelocity::default(),
        ))
        .id();
    let (pool, collider) = pool(&registry);
    commands.spawn((
        EffectPlayer::new(&pool),
        AestraBindings::new().bind("Projectile", projectile),
        Pool { collider },
        Transform::from_translation(POOL),
    ));
    commands.spawn((
        EffectPlayer::new(&smoke(&registry)),
        Transform::from_translation(SMOKE),
    ));
    commands.spawn((
        Readout,
        Text::new("Impacts: 0"),
        Node {
            position_type: PositionType::Absolute,
            top: px(12.0),
            left: px(12.0),
            ..default()
        },
    ));
}

/// Flies the projectile into the pool's middle, over and over, reporting its velocity as a physics
/// integration would.
fn fly(
    time: Res<Time>,
    mut projectiles: Query<(
        &mut Projectile,
        &mut Transform,
        &mut AestraLinearVelocity,
        &MeshMaterial3d<StandardMaterial>,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let target = POOL + Vec3::new(0.0, 4.0, 0.0);
    let direction = (target - LAUNCH).normalize();
    let length = (target - LAUNCH).length() / SPEED;
    for (mut projectile, mut transform, mut velocity, material) in &mut projectiles {
        projectile.flight += time.delta_secs();
        // A flight, then a second's rest in the water, then another.
        if projectile.flight > length + 1.0 {
            projectile.flight = 0.0;
        }
        let travelled = projectile.flight.min(length);
        transform.translation = LAUNCH + direction * SPEED * travelled;
        velocity.0 = if projectile.flight < length {
            direction * SPEED
        } else {
            Vec3::ZERO
        };
        projectile.flash = (projectile.flash - time.delta_secs()).max(0.0);
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.emissive = if projectile.flash > 0.0 {
                LinearRgba::rgb(4.0, 0.3, 0.1)
            } else {
                LinearRgba::rgb(1.5, 1.0, 0.2)
            };
        }
    }
}

/// Gameplay hears of the splash: the pool raised an impact for the projectile's collider.
fn hear_impacts(
    mut events: MessageReader<AestraOutputEvent>,
    pools: Query<&Pool>,
    mut projectiles: Query<&mut Projectile>,
    mut heard: ResMut<Heard>,
) {
    for message in events.read() {
        let Ok(pool) = pools.get(message.effect) else {
            continue;
        };
        if message.event.kind != EVENT_IMPACT || message.event.source != Some(pool.collider) {
            continue;
        }
        heard.impacts += 1;
        info!(
            "impact #{}: the water pushed the projectile with {:.0}",
            heard.impacts, message.event.magnitude
        );
        for mut projectile in &mut projectiles {
            projectile.flash = 0.5;
        }
    }
}

/// The count of impacts, and the force the water puts on the projectile now.
fn show(
    pools: Query<(&Pool, Option<&AestraEffectOutputs>)>,
    mut readouts: Query<&mut Text, With<Readout>>,
    mut heard: ResMut<Heard>,
) {
    let force = pools
        .iter()
        .find_map(|(pool, outputs)| outputs?.get(OUTPUT_FORCE, pool.collider))
        .map_or(0.0, |force| Vec3::from_slice(force).length());
    heard.strongest = heard.strongest.max(force);
    for mut text in &mut readouts {
        text.0 = format!(
            "Impacts: {}\nForce on the projectile: {force:.0}",
            heard.impacts
        );
    }
}

/// `AESTRA_EXAMPLE_CAPTURE`: a screenshot after four seconds, a report, then exit.
fn capture(mut commands: Commands, mut heard: ResMut<Heard>, mut exit: MessageWriter<AppExit>) {
    let Ok(path) = std::env::var("AESTRA_EXAMPLE_CAPTURE") else {
        return;
    };
    heard.frames += 1;
    if heard.frames == 240 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
    if heard.frames == 250 {
        println!(
            "fluid_world: {} impacts heard; strongest force read {:.0}",
            heard.impacts, heard.strongest
        );
        exit.write(AppExit::Success);
    }
}
