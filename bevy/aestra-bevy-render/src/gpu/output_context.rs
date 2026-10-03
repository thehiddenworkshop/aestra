//! Main-world routing of validated native particle output records.
use super::*;
use aestra_core::{EffectClipId, EffectId};

/// Routing identity maintained by a project host on each child presentation.
/// The renderer first validates the child's GPU epoch, then qualifies the host
/// message with this root epoch. No transient presentation entity escapes.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct EffectOutputContext {
    pub root: Entity,
    pub clip_path: Vec<EffectClipId>,
    pub playback_epoch: u32,
}

/// Context for a native particle route. `event.value` remains effect-local XYZ;
/// `event.tick` remains source simulation time, not wall-clock delivery time.
#[derive(Debug, Clone, PartialEq)]
pub struct ParticleOutputContext {
    pub source_effect: EffectId,
    pub seed: u64,
    pub root_time_seconds: f32,
    /// Clip/ancestor/leaf authored motion is sampled at the event tick. The ECS
    /// root placement is sampled at delivery, not from a recorded pose history.
    /// Missing placement or nonfinite data produces None, never a fake origin.
    pub world_position: Option<[f32; 3]>,
}

pub(super) fn particle_event(
    owner: Entity,
    presented: &PresentedEffect,
    routing: Option<&EffectOutputContext>,
    placement: Option<&GlobalTransform>,
    event: aestra_runtime::EffectOutputEvent,
) -> AestraOutputEvent {
    let time = event.tick as f32 * aestra_runtime::StatefulSimulation::TICK_DT;
    let pose = presented.instance.host_transform_context();
    let world_position = placement.and_then(|placement| {
        let local: [f32; 3] = event.value.as_slice().try_into().ok()?;
        let point = (Mat4::from(placement.affine()) * Mat4::from_cols_array(&pose.matrix_at(time)))
            .transform_point3(Vec3::from_array(local));
        point.is_finite().then(|| point.to_array())
    });
    let mut output =
        AestraOutputEvent::root(owner, event).in_epoch(presented.instance.history_epoch());
    if let Some(routing) = routing {
        output.effect = routing.root;
        output.clip_path.clone_from(&routing.clip_path);
        output.playback_epoch = Some(routing.playback_epoch);
    }
    output.particle = Some(ParticleOutputContext {
        source_effect: presented.effect().source,
        seed: presented.instance.seed(),
        root_time_seconds: pose.inherited.root_time_at(time),
        world_position,
    });
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{EffectAsset, EmitterTransform, HostTransformKey, HostTransformTrack};
    use aestra_runtime::{
        CompiledHostTransformTrack, EffectOutputEvent, EventOrigin, InheritedHostTransform,
    };

    fn fixture() -> PresentedEffect {
        let effect = aestra_compiler::EffectCompiler::default()
            .compile(&EffectAsset::new("cue", 10.0))
            .unwrap();
        let mut presented = PresentedEffect::new(Arc::new(effect));
        let end = EmitterTransform {
            translation: [10.0, 0.0, 0.0],
            ..default()
        };
        let motion = Arc::new(
            CompiledHostTransformTrack::new(HostTransformTrack::from_pose_keys(
                vec![
                    HostTransformKey {
                        time: 0.0,
                        transform: default(),
                    },
                    HostTransformKey {
                        time: 10.0,
                        transform: end,
                    },
                ],
                false,
            ))
            .unwrap(),
        );
        let inherited = InheritedHostTransform::default()
            .for_child(
                Some(motion),
                EmitterTransform {
                    scale: [2.0, 0.5, 1.0],
                    ..default()
                },
                2.0,
            )
            .for_child(
                None,
                EmitterTransform {
                    rotation: Quat::from_rotation_z(0.7).to_array(),
                    ..default()
                },
                -0.5,
            );
        presented
            .instance
            .set_inherited_host_transform(Arc::new(inherited));
        presented.instance.set_seed(23);
        presented.instance.set_playback_time(8.0); // delivery time differs from source tick
        presented
    }

    #[test]
    fn nested_cues_keep_local_payload_and_route_historical_affine_position_to_root() {
        let presented = fixture();
        let routing = EffectOutputContext {
            root: Entity::from_raw_u32(12).unwrap(),
            clip_path: vec![EffectClipId::new(), EffectClipId::new()],
            playback_epoch: 99,
        };
        let placement = GlobalTransform::from(Transform {
            translation: Vec3::new(12.0, 3.0, -7.0),
            rotation: Quat::from_rotation_y(0.35),
            scale: Vec3::new(0.9, 1.0, 0.8),
        });
        let event = EffectOutputEvent::new(
            "break",
            EventOrigin::Emitter(0),
            "",
            vec![1.0, 2.0, 3.0],
            4.0,
            60,
        );
        let output = particle_event(
            Entity::PLACEHOLDER,
            &presented,
            Some(&routing),
            Some(&placement),
            event.clone(),
        );
        assert_eq!(output.effect, routing.root);
        assert_eq!(output.clip_path, routing.clip_path);
        assert_eq!(output.playback_epoch, Some(99));
        assert_eq!(output.event, event);
        let spatial = output.particle.unwrap();
        assert_eq!(spatial.source_effect, presented.effect().source);
        assert_eq!(spatial.seed, 23);
        assert_eq!(spatial.root_time_seconds, 2.5);
        // Independent matrix oracle: historical root X motion at 2.5, followed
        // by nonuniform parent scale and rotated leaf (including shear).
        let expected = (Mat4::from(placement.affine())
            * Mat4::from_translation(Vec3::new(2.5, 0.0, 0.0))
            * Mat4::from_scale(Vec3::new(2.0, 0.5, 1.0))
            * Mat4::from_rotation_z(0.7))
        .transform_point3(Vec3::new(1.0, 2.0, 3.0));
        assert!(Vec3::from_array(spatial.world_position.unwrap()).abs_diff_eq(expected, 1e-5));
    }

    #[test]
    fn root_routes_and_missing_or_invalid_placement_are_explicit() {
        let presented = fixture();
        let event =
            EffectOutputEvent::new("launch", EventOrigin::Emitter(0), "", vec![0.0; 3], 1.0, 1);
        let output = particle_event(Entity::PLACEHOLDER, &presented, None, None, event.clone());
        assert!(output.clip_path.is_empty());
        assert_eq!(
            output.playback_epoch,
            Some(presented.instance.history_epoch())
        );
        assert!(output.particle.unwrap().world_position.is_none());
        let mut invalid = event;
        invalid.value[0] = f32::NAN;
        let output = particle_event(
            Entity::PLACEHOLDER,
            &presented,
            None,
            Some(&GlobalTransform::IDENTITY),
            invalid,
        );
        assert!(output.particle.unwrap().world_position.is_none());
        assert!(
            AestraOutputEvent::root(Entity::PLACEHOLDER, output.event)
                .particle
                .is_none()
        );
    }
}
