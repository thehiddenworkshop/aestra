//! Host-authoritative projectile, attached wake, and a detached impact effect.
//! `cargo run -p aestra-bevy --example host_projectile`
//!
//! One entity per projectile, never one per spark. Replace the ballistic/fuse
//! logic with your game's physics/hit detection. Aestra only renders the result.
use aestra_bevy::{
    AestraBindings, AestraPlugin, AestraSet, AttachmentInherit, BindingUpdateMode, ColorKey,
    CompiledEffect, Curve, CurveKey, EffectAsset, EffectBinding, EffectCompiler,
    EffectPlaybackMode, EffectPlayer, Emitter, EmitterAttachment, EmitterShape, Gradient,
    ModuleInstance, PlaybackHistoryPolicy, ScalarRange,
};
use aestra_runtime::INPUT_STOP_EMITTING;
use bevy::prelude::*;
use std::sync::Arc;

#[derive(Resource)]
struct Art {
    wake: Arc<CompiledEffect>,
    impact: Arc<CompiledEffect>,
}
#[derive(Component)]
struct Projectile {
    velocity: Vec3,
    remaining: f32,
    wake: Entity,
}
#[derive(Component)]
struct VfxTail;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, AestraPlugin))
        .add_systems(Startup, setup)
        // Root-level projectile world poses are published here, before binding
        // snapshots. Hierarchical hosts must propagate their hierarchy first.
        .add_systems(
            Update,
            (launch, advance, drain)
                .chain()
                .before(AestraSet::ResolveHostInputs),
        )
        .run();
}

fn art() -> Art {
    let mut wake = EffectAsset::new("Attached projectile wake", 6.0);
    wake.playback_mode = EffectPlaybackMode::Once;
    let binding = EffectBinding::spatial("Projectile", BindingUpdateMode::Live);
    let mut emitter = Emitter::basic_sprite("Wake", 6.0);
    emitter.max_particles = 512;
    emitter.attachment = Some(EmitterAttachment {
        binding: binding.id,
        inherit: AttachmentInherit::Position,
    });
    emitter.modules = vec![
        ModuleInstance::emission(120.0, 0),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(0.6, 1.0),
            ScalarRange::new(0.0, 0.2),
            [0.0, 1.0, 0.0],
            180.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, 0.2, 0.0], 1.0, 0.0),
        appearance(),
    ];
    wake.emitters.push(emitter);
    wake.bindings.push(binding);
    let mut impact = EffectAsset::new("Host impact", 3.0);
    impact.playback_mode = EffectPlaybackMode::Once;
    let mut emitter = Emitter::basic_sprite("Impact sparks", 3.0);
    emitter.max_particles = 256;
    emitter.modules = vec![
        ModuleInstance::emission(0.0, 256),
        ModuleInstance::shape(EmitterShape::Point),
        ModuleInstance::initialize(
            ScalarRange::new(0.8, 1.5),
            ScalarRange::new(4.0, 9.0),
            [0.0, 1.0, 0.0],
            180.0,
            ScalarRange::new(0.0, 0.0),
        ),
        ModuleInstance::motion([0.0, -9.81, 0.0], 0.5, 0.0),
        appearance(),
    ];
    impact.emitters.push(emitter);
    let compiler = EffectCompiler::default();
    Art {
        wake: Arc::new(compiler.compile(&wake).unwrap()),
        impact: Arc::new(compiler.compile(&impact).unwrap()),
    }
}

fn appearance() -> ModuleInstance {
    ModuleInstance::appearance(
        Curve::new(vec![CurveKey::new(0.0, 0.3), CurveKey::new(1.0, 0.05)]),
        Curve::new(vec![CurveKey::new(0.0, 1.0), CurveKey::new(1.0, 0.0)]),
        Gradient::new(vec![
            ColorKey::new(0.0, [1.0, 0.65, 0.15, 1.0]),
            ColorKey::new(1.0, [0.5, 0.05, 0.01, 1.0]),
        ]),
    )
}

fn setup(mut commands: Commands) {
    commands.insert_resource(art());
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 12.0, 45.0).looking_at(Vec3::new(0.0, 10.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        Text::new("Host projectile | Space: launch | host fuse owns detonation"),
        Node {
            position_type: PositionType::Absolute,
            left: px(16),
            top: px(16),
            ..default()
        },
    ));
}

fn launch(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    art: Res<Art>,
    projectiles: Query<&Projectile>,
) {
    if !keys.just_pressed(KeyCode::Space) || projectiles.iter().count() >= 8 {
        return;
    }
    let pose = Transform::from_xyz(-10.0, 0.0, 0.0);
    let projectile = commands.spawn((pose, GlobalTransform::from(pose))).id();
    let wake = commands
        .spawn((
            EffectPlayer::from_compiled(art.wake.clone())
                .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly),
            AestraBindings::new().bind("Projectile", projectile),
            // Stable world origin: moving this owner would also move old particles.
            Transform::default(),
            VfxTail,
        ))
        .id();
    commands.entity(projectile).insert(Projectile {
        velocity: Vec3::new(8.0, 20.0, 0.0),
        remaining: 1.5,
        wake,
    });
}

fn advance(
    mut commands: Commands,
    time: Res<Time>,
    art: Res<Art>,
    mut projectiles: Query<(
        Entity,
        &mut Projectile,
        &mut Transform,
        &mut GlobalTransform,
    )>,
    mut players: Query<&mut EffectPlayer>,
) {
    for (entity, mut projectile, mut pose, mut world_pose) in &mut projectiles {
        let dt = time.delta_secs().min(projectile.remaining);
        pose.translation += projectile.velocity * dt + Vec3::NEG_Y * (0.5 * 9.81 * dt * dt);
        projectile.velocity.y -= 9.81 * dt;
        projectile.remaining -= dt;
        *world_pose = GlobalTransform::from(*pose);
        if projectile.remaining <= 0.0 {
            // A collision/cancel would use this same ownership boundary. Stop
            // births, keep the VFX entity for its tail, then remove gameplay state.
            players
                .get_mut(projectile.wake)
                .unwrap()
                .send_event(INPUT_STOP_EMITTING, vec![])
                .unwrap();
            commands.spawn((
                EffectPlayer::from_compiled(art.impact.clone())
                    .with_history_policy(PlaybackHistoryPolicy::PlaybackOnly),
                Transform::from_translation(pose.translation),
                VfxTail,
            ));
            commands.entity(entity).despawn();
        }
    }
}

fn drain(mut commands: Commands, tails: Query<(Entity, &EffectPlayer), With<VfxTail>>) {
    for (entity, player) in &tails {
        if player.elapsed() >= player.effect().duration {
            commands.entity(entity).despawn();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn art_has_one_live_attachment_and_a_finite_independent_impact() {
        let art = art();
        assert_eq!(art.wake.bindings[0].name, "Projectile");
        assert!(art.wake.emitters[0].attachment.is_some());
        assert!(art.impact.bindings.is_empty());
        assert_eq!(art.impact.playback_mode, EffectPlaybackMode::Once);
    }
}
