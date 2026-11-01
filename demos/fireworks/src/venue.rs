//! Host-owned scenery and launch mounts. Aestra effects remain reusable anywhere.
use super::ShowRoot;
use aestra_bevy::{
    AestraSet, EffectPlayer,
    preview::water::{self, SeaState, WaterMaterial, WaterMaterialPlugin},
};
use bevy::prelude::*;

pub const DECK_LAUNCH_Y: f32 = 2.1;
const WATER_SIDE: f32 = 4000.0;
pub type SceneAssets<'w> = (
    ResMut<'w, Assets<Mesh>>,
    ResMut<'w, Assets<StandardMaterial>>,
    Option<ResMut<'w, Assets<WaterMaterial>>>,
);

/// Existing spread coordinates select a deck mortar bank.
pub fn launch_origin(origin: Vec3) -> Vec3 {
    Vec3::new(origin.x * 0.2, DECK_LAUNCH_Y, origin.z * 0.125)
}

/// Aim from the compact barge toward the cue's original break position. This
/// preserves the aerial layout while the visible rockets share a real deck.
pub fn flight(origin: Vec3, velocity: Vec3, fuse: f32) -> (Vec3, Vec3) {
    let mount = launch_origin(origin);
    let offset = Vec3::new(origin.x - mount.x, 0.0, origin.z - mount.z);
    (mount, velocity + offset / fuse)
}

pub fn camera(index: usize) -> Transform {
    let (eye, target) = match index {
        1 => (Vec3::new(28.0, 9.0, 155.0), Vec3::new(0.0, 45.0, 0.0)),
        2 => (Vec3::new(-35.0, 7.0, 270.0), Vec3::new(0.0, 50.0, 0.0)),
        _ => (Vec3::new(12.0, 6.0, 190.0), Vec3::new(0.0, 50.0, 0.0)),
    };
    Transform::from_translation(eye).looking_at(target, Vec3::Y)
}

/// The host's burning rocket illuminates ordinary scene receivers, including
/// water. Attach to the authoritative rocket entity: pause and detonation then
/// follow gameplay ownership without a second clock or a particle readback.
pub fn rocket_light() -> PointLight {
    PointLight {
        intensity: 180_000.0,
        range: 90.0,
        radius: 0.18,
        color: Color::srgb(1.0, 0.72, 0.38),
        shadow_maps_enabled: false,
        ..default()
    }
}

#[derive(Resource)]
struct SeaSurface {
    material: Handle<WaterMaterial>,
    last_time: f32,
}

pub fn install(app: &mut App) {
    app.add_plugins(WaterMaterialPlugin)
        .add_systems(Update, drift.after(AestraSet::Playback));
}

fn drift(
    surface: Option<ResMut<SeaSurface>>,
    roots: Query<&EffectPlayer, With<ShowRoot>>,
    mut materials: ResMut<Assets<WaterMaterial>>,
) {
    let (Some(mut surface), Ok(clock)) = (surface, roots.single()) else {
        return;
    };
    let time = clock.elapsed();
    if time == surface.last_time {
        return;
    }
    surface.last_time = time;
    if let Some(mut material) = materials.get_mut(&surface.material) {
        material.extension.set_time(time);
    }
}

pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    water_materials: &mut Assets<WaterMaterial>,
    sea_state: SeaState,
) -> Entity {
    commands.insert_resource(GlobalAmbientLight {
        color: Color::srgb(0.55, 0.65, 0.85),
        brightness: 2.0,
        ..default()
    });
    commands.spawn((
        DirectionalLight {
            illuminance: 0.2,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-80.0, 100.0, -120.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    let mut plane = Plane3d::default()
        .mesh()
        .size(WATER_SIDE, WATER_SIDE)
        .build();
    plane
        .generate_tangents()
        .expect("sea plane has normals and UVs");
    let water = water_materials.add(water::material(sea_state));
    let receiver = commands
        .spawn((
            Mesh3d(meshes.add(plane)),
            MeshMaterial3d(water.clone()),
            Transform::from_xyz(0.0, -0.5, 0.0),
        ))
        .id();
    commands.insert_resource(SeaSurface {
        material: water,
        last_time: 0.0,
    });

    let hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.055, 0.07),
        perceptual_roughness: 0.55,
        metallic: 0.65,
        ..default()
    });
    let deck = materials.add(StandardMaterial {
        base_color: Color::srgb(0.07, 0.06, 0.045),
        perceptual_roughness: 0.8,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(13.0, 1.7, 8.0))),
        MeshMaterial3d(hull.clone()),
        Transform::from_xyz(0.0, 0.15, 0.0),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(12.5, 0.18, 7.5))),
        MeshMaterial3d(deck),
        Transform::from_xyz(0.0, 1.09, 0.0),
    ));
    let trim = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.2, 0.22),
        perceptual_roughness: 0.65,
        ..default()
    });
    for z in [-3.65, 3.65] {
        commands.spawn((
            Mesh3d(meshes.add(Cuboid::new(12.4, 0.12, 0.09))),
            MeshMaterial3d(trim.clone()),
            Transform::from_xyz(0.0, 1.21, z),
        ));
    }
    let rack = meshes.add(Cylinder::new(0.17, 0.9));
    for x in [-4.8, 0.0, 4.8] {
        for z in [-1.0, 0.0] {
            commands.spawn((
                Mesh3d(rack.clone()),
                MeshMaterial3d(hull.clone()),
                Transform::from_xyz(x, 1.63, z),
            ));
        }
    }
    // Cabin at the stern, leaving all launcher banks unobstructed.
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.8, 1.9, 1.8))),
        MeshMaterial3d(hull),
        Transform::from_xyz(-3.5, 2.1, -2.6),
    ));
    let warm = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        emissive: LinearRgba::new(3.0, 1.5, 0.35, 1.0),
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.8, 0.45, 0.02))),
        MeshMaterial3d(warm.clone()),
        Transform::from_xyz(-3.5, 2.35, -1.68),
    ));
    commands.spawn((
        PointLight {
            intensity: 80.0,
            range: 12.0,
            color: Color::srgb(1.0, 0.65, 0.25),
            ..default()
        },
        Transform::from_xyz(-3.5, 2.5, -1.0),
    ));
    for (x, color) in [
        (-6.1, LinearRgba::new(2.0, 0.02, 0.01, 1.0)),
        (6.1, LinearRgba::new(0.01, 1.2, 0.25, 1.0)),
    ] {
        let material = materials.add(StandardMaterial {
            base_color: Color::BLACK,
            emissive: color,
            ..default()
        });
        commands.spawn((
            Mesh3d(meshes.add(Sphere::new(0.13))),
            MeshMaterial3d(material),
            Transform::from_xyz(x, 1.55, 2.9),
        ));
    }
    // Sparse distant shore lights supply scale and a horizon without lighting the sky.
    let skyline = materials.add(StandardMaterial {
        base_color: Color::srgb(0.006, 0.009, 0.014),
        ..default()
    });
    let light = meshes.add(Cuboid::new(0.5, 0.5, 0.5));
    for i in 0..90 {
        let x = (i as f32 - 44.5) * 11.0;
        let height = 2.0 + (i * 17 % 13) as f32;
        if i % 3 == 0 {
            commands.spawn((
                Mesh3d(meshes.add(Cuboid::new(9.0, height, 7.0))),
                MeshMaterial3d(skyline.clone()),
                Transform::from_xyz(x, height * 0.5, -420.0),
            ));
        }
        commands.spawn((
            Mesh3d(light.clone()),
            MeshMaterial3d(warm.clone()),
            Transform::from_xyz(x, 1.0 + (i % 3) as f32 * 0.7, -410.0),
        ));
    }
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn water_motion_follows_effect_pause_and_restart_without_advancing_playback() {
        let effect = aestra_bevy::EffectCompiler::default()
            .compile(&aestra_bevy::EffectAsset::new("Water clock", 20.0))
            .unwrap();
        let mut app = App::new();
        app.init_resource::<Assets<WaterMaterial>>()
            .add_systems(Update, drift);
        let material = app
            .world_mut()
            .resource_mut::<Assets<WaterMaterial>>()
            .add(water::material(SeaState::Rough));
        app.insert_resource(SeaSurface {
            material: material.clone(),
            last_time: 0.0,
        });
        let root = app
            .world_mut()
            .spawn((
                ShowRoot,
                EffectPlayer::from_compiled(std::sync::Arc::new(effect)),
            ))
            .id();
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .seek(3.0);
        app.update();
        let phase = app
            .world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .unwrap()
            .extension
            .time();
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .playing = false;
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<WaterMaterial>>()
                .get(&material)
                .unwrap()
                .extension
                .time(),
            phase
        );
        assert_eq!(
            app.world().get::<EffectPlayer>(root).unwrap().elapsed(),
            3.0
        );
        app.world_mut()
            .get_mut::<EffectPlayer>(root)
            .unwrap()
            .restart();
        app.update();
        assert_eq!(
            app.world()
                .resource::<Assets<WaterMaterial>>()
                .get(&material)
                .unwrap()
                .extension
                .time(),
            0.0
        );
    }
    #[test]
    fn every_variety_launch_starts_over_a_deck_bank() {
        for cue in super::super::repertoire::CUES {
            let p = launch_origin(cue.origin);
            assert!(p.x.abs() <= 4.8 && p.z.abs() <= 1.0);
            assert_eq!(p.y, DECK_LAUNCH_Y);
            let (mount, velocity) = flight(cue.origin, cue.velocity, cue.fuse);
            let expected = cue.origin + cue.velocity * cue.fuse + Vec3::Y * DECK_LAUNCH_Y;
            assert!((mount + velocity * cue.fuse - expected).length() < 0.0001);
        }
    }
}
