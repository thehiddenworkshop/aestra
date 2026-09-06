//! Effect-transform target and playback policy for shared animation curves.
use crate::{EmitterTransform, TransformCurve};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Legacy source/pose-editing view, not stored alongside the canonical curves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HostTransformKey {
    pub time: f32,
    pub transform: EmitterTransform,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HostTransformTrack {
    pub curves: TransformCurve,
    pub repeat: bool,
}

impl<'de> Deserialize<'de> for HostTransformTrack {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize, Default)]
        #[serde(default, deny_unknown_fields)]
        struct Source {
            keys: Vec<HostTransformKey>,
            curves: TransformCurve,
            repeat: bool,
        }
        let source = Source::deserialize(deserializer)?;
        if !source.keys.is_empty() {
            if !source.curves.key_times().is_empty() {
                return Err(serde::de::Error::custom(
                    "motion cannot contain both pose keys and curves",
                ));
            }
            Ok(Self::from_pose_keys(source.keys, source.repeat))
        } else {
            Ok(Self {
                curves: source.curves,
                repeat: source.repeat,
            })
        }
    }
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum HostTransformError {
    #[error("transform channels need 1–65536 keys starting at time zero")]
    Keys,
    #[error(
        "transform key {0} must have increasing finite time, finite values, positive scale and normalized rotation"
    )]
    Key(usize),
    #[error(
        "repeating transform animation needs a positive period and matching endpoint transforms"
    )]
    Repeat,
}

impl HostTransformTrack {
    pub fn from_pose_keys(keys: Vec<HostTransformKey>, repeat: bool) -> Self {
        // Preserve order/duplicates: malformed legacy tracks must fail validation.
        let mut curves = TransformCurve::default();
        for key in keys {
            for axis in 0..3 {
                curves.translation[axis].keys.push(crate::CurveKey::new(
                    key.time,
                    key.transform.translation[axis],
                ));
                curves.scale[axis]
                    .keys
                    .push(crate::CurveKey::new(key.time, key.transform.scale[axis]));
            }
            curves.rotation.keys.push(crate::QuaternionKey {
                time: key.time,
                value: key.transform.rotation,
            });
        }
        Self { curves, repeat }
    }
    pub fn keys(&self) -> Vec<HostTransformKey> {
        self.curves
            .key_times()
            .into_iter()
            .map(|time| HostTransformKey {
                time,
                transform: self.curves.sample_at(time),
            })
            .collect()
    }
    pub fn end_time(&self) -> f32 {
        self.curves.end_time()
    }
    pub fn sample_time(&self, time: f32) -> f32 {
        let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
        if self.repeat && self.end_time() > 0.0 {
            time.rem_euclid(self.end_time())
        } else {
            time.min(self.end_time())
        }
    }
    pub fn set_pose(&mut self, time: f32, pose: EmitterTransform) {
        let end = self.end_time();
        self.curves.set_pose(time, pose, false);
        if self.repeat && (time == 0.0 || time == end) {
            self.curves
                .set_pose(if time == 0.0 { end } else { 0.0 }, pose, false);
        }
    }
    pub fn validate(&self) -> Result<(), HostTransformError> {
        fn times(values: &[f32]) -> Result<(), HostTransformError> {
            if values.is_empty() || values.len() > 65536 || values[0] != 0.0 {
                return Err(HostTransformError::Keys);
            }
            for (index, time) in values.iter().enumerate() {
                if !time.is_finite() || (index > 0 && *time <= values[index - 1]) {
                    return Err(HostTransformError::Key(index));
                }
            }
            Ok(())
        }
        for (channel, curve) in self
            .curves
            .translation
            .iter()
            .chain(&self.curves.scale)
            .enumerate()
        {
            times(&curve.keys.iter().map(|key| key.time).collect::<Vec<_>>())?;
            if let Some(range) = curve.output_range
                && (!range.min.is_finite() || !range.max.is_finite() || range.min > range.max)
            {
                return Err(HostTransformError::Key(0));
            }
            for (index, key) in curve.keys.iter().enumerate() {
                let value = curve.output_value(key.value);
                if !key.value.is_finite() || !value.is_finite() || (channel >= 3 && value <= 0.0) {
                    return Err(HostTransformError::Key(index));
                }
            }
        }
        times(
            &self
                .curves
                .rotation
                .keys
                .iter()
                .map(|key| key.time)
                .collect::<Vec<_>>(),
        )?;
        for (index, key) in self.curves.rotation.keys.iter().enumerate() {
            if !(EmitterTransform {
                rotation: key.value,
                ..Default::default()
            })
            .is_valid()
            {
                return Err(HostTransformError::Key(index));
            }
        }
        if self.repeat {
            let first = self.curves.sample_at(0.0);
            let last = self.curves.sample_at(self.end_time());
            let dot: f32 = first
                .rotation
                .iter()
                .zip(last.rotation)
                .map(|(a, b)| a * b)
                .sum();
            if self.end_time() <= 0.0
                || (dot.abs() - 1.0).abs() > 1e-4
                || first
                    .translation
                    .iter()
                    .zip(last.translation)
                    .any(|(a, b)| (a - b).abs() > 1e-4)
                || first
                    .scale
                    .iter()
                    .zip(last.scale)
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
        let valid = HostTransformTrack::from_pose_keys(vec![key(0.0), key(1.0)], true);
        assert!(valid.validate().is_ok());
        for keys in [
            vec![],
            vec![key(0.1)],
            vec![key(0.0), key(0.0)],
            vec![key(0.0), key(f32::NAN)],
        ] {
            assert!(
                HostTransformTrack::from_pose_keys(keys, false)
                    .validate()
                    .is_err()
            );
        }
        assert!(
            HostTransformTrack::from_pose_keys(vec![key(0.0)], true)
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
                HostTransformTrack::from_pose_keys(
                    vec![HostTransformKey {
                        time: 0.0,
                        transform
                    }],
                    false
                )
                .validate()
                .is_err()
            );
        }
        let mut seam = valid.clone();
        seam.curves.translation[0].keys[1].value = 1.0;
        assert_eq!(seam.validate(), Err(HostTransformError::Repeat));
        seam = valid;
        seam.curves.rotation.keys[1].value[3] = -1.0;
        assert!(
            seam.validate().is_ok(),
            "quaternion sign does not change the pose"
        );
    }
}
