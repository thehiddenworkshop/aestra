//! Immutable, time-aware presentation transforms for nested effect instances.
use crate::CompiledHostTransformTrack;
use aestra_core::EmitterTransform;
use glam::{Mat4, Quat, Vec3};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
struct Ancestor {
    motion: Option<Arc<CompiledHostTransformTrack>>,
    /// Ancestor simulation seconds = leaf simulation seconds + this offset.
    time_offset: f32,
    placement: EmitterTransform,
}

/// Static clip placements interleaved with historical ancestor motion.
/// Never bake the current parent pose into a child's stable host placement.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InheritedHostTransform(Vec<Ancestor>);

impl InheritedHostTransform {
    /// Append a child placement and map its historical clock back to its parent.
    /// Obtain the offset from `CompiledEffectClip::map_instance_time` for the
    /// current clip occurrence, not by subtracting rounded per-frame clocks.
    pub fn for_child(
        &self,
        parent_motion: Option<Arc<CompiledHostTransformTrack>>,
        placement: EmitterTransform,
        parent_time_offset: f32,
    ) -> Self {
        let mut ancestors = self.0.clone();
        for ancestor in &mut ancestors {
            ancestor.time_offset += parent_time_offset;
        }
        ancestors.push(Ancestor {
            motion: parent_motion,
            placement,
            time_offset: parent_time_offset,
        });
        Self(ancestors)
    }
}

/// Complete immutable replay context, including the leaf's own motion.
#[derive(Debug, Clone, PartialEq)]
pub struct HostTransformContext {
    pub motion: Option<Arc<CompiledHostTransformTrack>>,
    pub inherited: Arc<InheritedHostTransform>,
}

impl HostTransformContext {
    pub fn is_identity(&self) -> bool {
        self.motion.is_none() && self.inherited.0.is_empty()
    }

    /// Column-major affine matrix. Keeping the matrix avoids losing shear when
    /// composing rotated, nonuniformly scaled parents and clips.
    pub fn matrix_at(&self, time: f32) -> [f32; 16] {
        let mut result = Mat4::IDENTITY;
        for ancestor in &self.inherited.0 {
            if let Some(motion) = &ancestor.motion {
                result *= matrix(motion.sample(time + ancestor.time_offset));
            }
            result *= matrix(ancestor.placement);
        }
        if let Some(motion) = &self.motion {
            result *= matrix(motion.sample(time));
        }
        result.to_cols_array()
    }
}

fn matrix(transform: EmitterTransform) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from_array(transform.scale),
        Quat::from_array(transform.rotation),
        Vec3::from_array(transform.translation),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::{HostTransformKey, HostTransformTrack};

    fn motion(axis: usize) -> Arc<CompiledHostTransformTrack> {
        let mut end = EmitterTransform::default();
        end.translation[axis] = 20.0;
        Arc::new(
            CompiledHostTransformTrack::new(HostTransformTrack::from_pose_keys(
                vec![
                    HostTransformKey {
                        time: 0.0,
                        transform: EmitterTransform::default(),
                    },
                    HostTransformKey {
                        time: 20.0,
                        transform: end,
                    },
                ],
                false,
            ))
            .unwrap(),
        )
    }

    #[test]
    fn nested_history_composes_each_clock_and_keeps_affine_shear() {
        let root = motion(0);
        let parent = motion(1);
        let leaf = motion(2);
        let a = EmitterTransform {
            scale: [2.0, 0.5, 1.0],
            ..Default::default()
        };
        let b = EmitterTransform {
            translation: [1.0, 2.0, 3.0],
            rotation: Quat::from_rotation_z(0.7).to_array(),
            ..Default::default()
        };
        let inherited = InheritedHostTransform::default()
            .for_child(Some(root.clone()), a, 2.0)
            .for_child(Some(parent.clone()), b, -0.5);
        let context = HostTransformContext {
            motion: Some(leaf.clone()),
            inherited: Arc::new(inherited),
        };
        for t in [0.0, 0.25, 1.0, 4.0, 0.25] {
            let expected = matrix(root.sample(t + 1.5))
                * matrix(a)
                * matrix(parent.sample(t - 0.5))
                * matrix(b)
                * matrix(leaf.sample(t));
            let actual = Mat4::from_cols_array(&context.matrix_at(t));
            assert!(actual.abs_diff_eq(expected, 1e-6));
            assert!(
                actual.x_axis.truncate().dot(actual.y_axis.truncate()).abs() > 0.1,
                "nonuniform scale followed by rotation must retain shear"
            );
        }
    }
}
