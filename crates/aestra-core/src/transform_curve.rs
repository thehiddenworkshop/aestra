//! Shared transform animation: independent scalar curves and a typed rotation curve.
use crate::{Curve, CurveId, CurveInterpolation, CurveKey, EmitterTransform};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuaternionKey {
    pub time: f32,
    pub value: [f32; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuaternionCurve {
    pub keys: Vec<QuaternionKey>,
    pub interpolation: CurveInterpolation,
}

impl QuaternionCurve {
    pub fn sample_at(&self, time: f32) -> [f32; 4] {
        let after = self.keys.partition_point(|key| key.time <= time);
        if after == 0 {
            return self
                .keys
                .first()
                .map_or([0.0, 0.0, 0.0, 1.0], |key| key.value);
        }
        let a = &self.keys[after - 1];
        if after == self.keys.len() || time == a.time {
            return a.value;
        }
        if self.interpolation == CurveInterpolation::Step {
            return a.value;
        }
        let b = &self.keys[after];
        let t = self
            .interpolation
            .weight((time - a.time) / (b.time - a.time));
        if t == 0.0 {
            return a.value;
        }
        if t == 1.0 {
            return b.value;
        }
        let mut end = b.value;
        let mut dot: f32 = a.value.iter().zip(end).map(|(x, y)| x * y).sum();
        if dot < 0.0 {
            end = end.map(|x| -x);
            dot = -dot;
        }
        let (wa, wb) = if dot > 0.9995 {
            (1.0 - t, t)
        } else {
            let angle = dot.clamp(-1.0, 1.0).acos();
            (
                ((1.0 - t) * angle).sin() / angle.sin(),
                (t * angle).sin() / angle.sin(),
            )
        };
        let value: [f32; 4] = std::array::from_fn(|i| a.value[i] * wa + end[i] * wb);
        let length = value.iter().map(|v| v * v).sum::<f32>().sqrt();
        value.map(|v| v / length)
    }
}

/// Canonical transform channels. Pose markers are the union of channel key times,
/// never a second stored set of pose keys. Effect transforms use seconds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransformCurve {
    pub translation: [Curve; 3],
    pub rotation: QuaternionCurve,
    pub scale: [Curve; 3],
}

impl Default for TransformCurve {
    fn default() -> Self {
        // Field-scoped identities keep legacy conversion deterministic on reload.
        let channel = |index| Curve {
            id: CurveId::from_u128(0xa3574a0000004000800000000000e000 + index),
            keys: Vec::new(),
            output_range: None,
            interpolation: CurveInterpolation::Linear,
        };
        Self {
            translation: std::array::from_fn(|i| channel(i as u128)),
            rotation: QuaternionCurve {
                keys: Vec::new(),
                interpolation: CurveInterpolation::Linear,
            },
            scale: std::array::from_fn(|i| channel(3 + i as u128)),
        }
    }
}

impl TransformCurve {
    pub fn sample_at(&self, time: f32) -> EmitterTransform {
        EmitterTransform {
            translation: std::array::from_fn(|i| self.translation[i].sample_at(time)),
            rotation: self.rotation.sample_at(time),
            scale: std::array::from_fn(|i| self.scale[i].sample_at(time)),
        }
    }
    pub fn key_times(&self) -> Vec<f32> {
        let mut times: Vec<_> = self
            .translation
            .iter()
            .chain(&self.scale)
            .flat_map(|curve| curve.keys.iter().map(|key| key.time))
            .chain(self.rotation.keys.iter().map(|key| key.time))
            .collect();
        times.sort_by(f32::total_cmp);
        times.dedup();
        times
    }
    pub fn end_time(&self) -> f32 {
        self.translation
            .iter()
            .chain(&self.scale)
            .filter_map(|curve| curve.keys.last().map(|key| key.time))
            .chain(self.rotation.keys.last().map(|key| key.time))
            .fold(0.0, f32::max)
    }
    /// Insert into all channels, or edit only changed components without adding
    /// incidental keys to unrelated channels.
    pub fn set_pose(&mut self, time: f32, pose: EmitterTransform, insert_all: bool) {
        let previous = self.sample_at(time);
        for axis in 0..3 {
            if insert_all || previous.translation[axis] != pose.translation[axis] {
                set_scalar_key(&mut self.translation[axis], time, pose.translation[axis]);
            }
            if insert_all || previous.scale[axis] != pose.scale[axis] {
                set_scalar_key(&mut self.scale[axis], time, pose.scale[axis]);
            }
        }
        if insert_all || previous.rotation != pose.rotation {
            if let Some(key) = self.rotation.keys.iter_mut().find(|key| key.time == time) {
                key.value = pose.rotation;
            } else {
                self.rotation.keys.push(QuaternionKey {
                    time,
                    value: pose.rotation,
                });
                self.rotation.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
        }
    }
    pub fn retime(&mut self, old: f32, time: f32) {
        for curve in self.translation.iter_mut().chain(&mut self.scale) {
            for key in &mut curve.keys {
                if key.time == old {
                    key.time = time;
                }
            }
            curve.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
        for key in &mut self.rotation.keys {
            if key.time == old {
                key.time = time;
            }
        }
        self.rotation.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
    pub fn remove_time(&mut self, time: f32) {
        for curve in self.translation.iter_mut().chain(&mut self.scale) {
            curve.keys.retain(|key| key.time != time);
        }
        self.rotation.keys.retain(|key| key.time != time);
    }
}

fn set_scalar_key(curve: &mut Curve, time: f32, value: f32) {
    if curve.output_range.is_some() {
        let values: Vec<_> = curve
            .keys
            .iter()
            .map(|key| curve.output_value(key.value))
            .collect();
        for (key, value) in curve.keys.iter_mut().zip(values) {
            key.value = value;
        }
        curve.output_range = None;
    }
    if let Some(key) = curve.keys.iter_mut().find(|key| key.time == time) {
        key.value = value;
    } else {
        curve.keys.push(CurveKey::new(time, value));
        curve.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
    }
}
