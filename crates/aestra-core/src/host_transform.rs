//! Portable, explicitly supplied host motion. This is not an implicit live-motion recorder.
use crate::EmitterTransform;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostTransformKey {
    pub time: f32,
    pub transform: EmitterTransform,
}

/// Motion relative to a stable host placement, sampled in effect simulation seconds.
/// Outside the key range, hold the endpoint unless `repeat` is set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostTransformTrack {
    pub keys: Vec<HostTransformKey>,
    #[serde(default)]
    pub repeat: bool,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum HostTransformError {
    #[error("host transform track needs 1–65536 keys starting at time zero")]
    Keys,
    #[error(
        "host transform key {0} must have increasing finite time and a finite transform with positive scale and normalized rotation"
    )]
    Key(usize),
    #[error(
        "repeating host transform track needs a positive period and matching endpoint transforms"
    )]
    Repeat,
}

impl HostTransformTrack {
    pub fn validate(&self) -> Result<(), HostTransformError> {
        if self.keys.is_empty() || self.keys.len() > 65536 || self.keys[0].time != 0.0 {
            return Err(HostTransformError::Keys);
        }
        for (index, key) in self.keys.iter().enumerate() {
            if !key.time.is_finite()
                || !key.transform.is_valid()
                || (index > 0 && key.time <= self.keys[index - 1].time)
            {
                return Err(HostTransformError::Key(index));
            }
        }
        if self.repeat {
            let first = self.keys[0].transform;
            let last = self.keys.last().unwrap();
            let rotation_dot: f32 = first
                .rotation
                .iter()
                .zip(last.transform.rotation)
                .map(|(a, b)| a * b)
                .sum();
            if last.time <= 0.0
                || (rotation_dot.abs() - 1.0).abs() > 1e-4
                || first
                    .translation
                    .iter()
                    .zip(last.transform.translation)
                    .any(|(a, b)| (a - b).abs() > 1e-4)
                || first
                    .scale
                    .iter()
                    .zip(last.transform.scale)
                    .any(|(a, b)| (a - b).abs() > 1e-4)
            {
                return Err(HostTransformError::Repeat);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(time: f32) -> HostTransformKey {
        HostTransformKey {
            time,
            transform: EmitterTransform::default(),
        }
    }

    #[test]
    fn validates_order_transforms_and_continuous_repeat_seam() {
        let valid = HostTransformTrack {
            keys: vec![key(0.0), key(1.0)],
            repeat: true,
        };
        assert!(valid.validate().is_ok());
        for keys in [
            vec![],
            vec![key(0.1)],
            vec![key(0.0), key(0.0)],
            vec![key(0.0), key(f32::NAN)],
        ] {
            assert!(
                HostTransformTrack {
                    keys,
                    repeat: false
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            HostTransformTrack {
                keys: vec![key(0.0)],
                repeat: true
            }
            .validate()
            .is_err()
        );
        for transform in [
            EmitterTransform {
                translation: [f32::INFINITY, 0.0, 0.0],
                ..Default::default()
            },
            EmitterTransform {
                rotation: [0.0; 4],
                ..Default::default()
            },
            EmitterTransform {
                scale: [0.0, 1.0, 1.0],
                ..Default::default()
            },
        ] {
            assert!(
                HostTransformTrack {
                    keys: vec![HostTransformKey {
                        time: 0.0,
                        transform
                    }],
                    repeat: false
                }
                .validate()
                .is_err()
            );
        }
        let mut seam = valid.clone();
        seam.keys[1].transform.translation[0] = 1.0;
        assert_eq!(seam.validate(), Err(HostTransformError::Repeat));
        seam = valid;
        seam.keys[1].transform.rotation[3] = -1.0;
        assert!(
            seam.validate().is_ok(),
            "quaternion sign does not change the pose"
        );
    }
}
