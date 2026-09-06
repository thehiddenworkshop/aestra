//! Shared presentation math; both the editor and runtime adapter feed EffectInstance tracks.
#[cfg(test)]
use aestra_core::EmitterTransform;
use bevy::prelude::*;

#[cfg(test)]
pub(crate) fn transform(value: EmitterTransform) -> Transform {
    Transform {
        translation: Vec3::from_array(value.translation),
        rotation: Quat::from_array(value.rotation),
        scale: Vec3::from_array(value.scale),
    }
}

#[cfg(test)]
pub(crate) fn matrix(value: EmitterTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from_array(value.scale),
        Quat::from_array(value.rotation),
        Vec3::from_array(value.translation),
    )
}

/// Time followed by the historical world matrix. Encoder copies (not queue writes)
/// upload each observation before its dispatch. Matrix begins at byte 4 here, and
/// byte 32 in GpuGlobals. Both offsets satisfy copy alignment without adding ABI lanes.
pub(crate) fn observation_bytes(
    times: &[f32],
    placement: Mat4,
    context: Option<&aestra_runtime::HostTransformContext>,
) -> Vec<u8> {
    times
        .iter()
        .flat_map(|&time| {
            let world = placement
                * context.map_or(Mat4::IDENTITY, |context| {
                    Mat4::from_cols_array(&context.matrix_at(time))
                });
            std::iter::once(time)
                .chain(world.to_cols_array())
                .flat_map(f32::to_le_bytes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_upload_contains_its_own_historical_pose_and_stable_placement() {
        let track = aestra_runtime::CompiledHostTransformTrack::new(
            aestra_core::HostTransformTrack::from_pose_keys(
                vec![
                    aestra_core::HostTransformKey {
                        time: 0.0,
                        transform: EmitterTransform::default(),
                    },
                    aestra_core::HostTransformKey {
                        time: 1.0,
                        transform: EmitterTransform {
                            translation: [2.0, 3.0, 4.0],
                            rotation: Quat::from_rotation_z(1.0).to_array(),
                            scale: [2.0, 1.0, 0.5],
                        },
                    },
                ],
                false,
            ),
        )
        .unwrap();
        let placement = Mat4::from_translation(Vec3::new(10.0, 20.0, 30.0));
        let context = aestra_runtime::HostTransformContext {
            motion: Some(std::sync::Arc::new(track.clone())),
            inherited: Default::default(),
        };
        for motion in [None, Some(&track)] {
            let bytes = observation_bytes(&[0.0, 0.5, 1.0], placement, motion.map(|_| &context));
            assert_eq!(bytes.len(), 3 * 68);
            for (i, record) in bytes.as_chunks::<68>().0.iter().enumerate() {
                let time = f32::from_le_bytes(record[0..4].try_into().unwrap());
                assert_eq!(time, i as f32 * 0.5);
                let actual = Mat4::from_cols_array(&std::array::from_fn(|j| {
                    f32::from_le_bytes(record[4 + j * 4..8 + j * 4].try_into().unwrap())
                }));
                let expected = placement
                    * motion.map_or(Mat4::IDENTITY, |m| transform(m.sample(time)).to_matrix());
                assert!(actual.abs_diff_eq(expected, 1e-6));
            }
        }
        let clip = EmitterTransform {
            rotation: Quat::from_rotation_z(0.7).to_array(),
            scale: [0.5, 2.0, 1.0],
            ..default()
        };
        let nested = aestra_runtime::HostTransformContext {
            motion: context.motion.clone(),
            inherited: std::sync::Arc::new(
                aestra_runtime::InheritedHostTransform::default()
                    .for_child(context.motion.clone(), clip, 0.5)
                    .for_child(context.motion.clone(), clip, -0.25),
            ),
        };
        let bytes = observation_bytes(&[0.0, 0.5, 1.0], placement, Some(&nested));
        for record in bytes.as_chunks::<68>().0 {
            let time = f32::from_le_bytes(record[..4].try_into().unwrap());
            let actual = Mat4::from_cols_array(&std::array::from_fn(|j| {
                f32::from_le_bytes(record[4 + j * 4..8 + j * 4].try_into().unwrap())
            }));
            let expected = placement
                * matrix(track.sample(time + 0.25))
                * matrix(clip)
                * matrix(track.sample(time - 0.25))
                * matrix(clip)
                * matrix(track.sample(time));
            assert!(actual.abs_diff_eq(expected, 1e-5));
        }
    }
}
