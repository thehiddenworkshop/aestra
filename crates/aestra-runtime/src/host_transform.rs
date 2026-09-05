use aestra_core::{EmitterTransform, HostTransformError, HostTransformTrack};

/// Validated, immutable motion data. Sharing it across instances does not share playback state.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledHostTransformTrack(HostTransformTrack);

impl CompiledHostTransformTrack {
    pub fn new(track: HostTransformTrack) -> Result<Self, HostTransformError> {
        track.validate()?;
        Ok(Self(track))
    }

    pub fn source(&self) -> &HostTransformTrack {
        &self.0
    }

    /// Linear translation/scale, shortest-arc quaternion slerp. Repeating tracks
    /// use their own period even during continuous effect playback.
    pub fn sample(&self, time: f32) -> EmitterTransform {
        let keys = &self.0.keys;
        let end = keys.last().unwrap();
        let time = if time.is_finite() { time.max(0.0) } else { 0.0 };
        let time = if self.0.repeat {
            time.rem_euclid(end.time)
        } else {
            time.min(end.time)
        };
        let after = keys.partition_point(|key| key.time <= time);
        if after == 0 {
            return keys[0].transform;
        }
        if after == keys.len() {
            return end.transform;
        }
        let a = &keys[after - 1];
        let b = &keys[after];
        let t = (time - a.time) / (b.time - a.time);
        let lerp = |x: f32, y: f32| x * (1.0 - t) + y * t;
        let mut rotation_b = b.transform.rotation;
        let mut dot: f32 = a
            .transform
            .rotation
            .iter()
            .zip(rotation_b)
            .map(|(x, y)| x * y)
            .sum();
        if dot < 0.0 {
            rotation_b = rotation_b.map(|x| -x);
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
        let rotation: [f32; 4] =
            std::array::from_fn(|i| a.transform.rotation[i] * wa + rotation_b[i] * wb);
        let length = rotation.iter().map(|x| x * x).sum::<f32>().sqrt();
        EmitterTransform {
            translation: std::array::from_fn(|i| {
                lerp(a.transform.translation[i], b.transform.translation[i])
            }),
            scale: std::array::from_fn(|i| lerp(a.transform.scale[i], b.transform.scale[i])),
            rotation: rotation.map(|x| x / length),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aestra_core::HostTransformKey;

    #[test]
    fn samples_translation_scale_shortest_arc_and_endpoints() {
        let start = EmitterTransform::default();
        let end = EmitterTransform {
            translation: [10.0, -6.0, 2.0],
            scale: [3.0, 0.5, 2.0],
            rotation: [
                0.0,
                0.0,
                -std::f32::consts::FRAC_1_SQRT_2,
                -std::f32::consts::FRAC_1_SQRT_2,
            ],
        };
        let mut source = HostTransformTrack {
            keys: vec![
                HostTransformKey {
                    time: 0.0,
                    transform: start,
                },
                HostTransformKey {
                    time: 2.0,
                    transform: end,
                },
            ],
            repeat: false,
        };
        let track = CompiledHostTransformTrack::new(source.clone()).unwrap();
        let mid = track.sample(1.0);
        assert_eq!(mid.translation, [5.0, -3.0, 1.0]);
        assert_eq!(mid.scale, [2.0, 0.75, 1.5]);
        assert!((mid.rotation[2] - (std::f32::consts::PI / 8.0).sin()).abs() < 1e-6);
        assert!((mid.rotation[3] - (std::f32::consts::PI / 8.0).cos()).abs() < 1e-6);
        assert_eq!(track.sample(10.0), end);
        assert_eq!(track.sample(-1.0), start);
        assert_eq!(track.sample(f32::NAN), start);
        source.keys.push(HostTransformKey {
            time: 4.0,
            transform: start,
        });
        source.repeat = true;
        let looping = CompiledHostTransformTrack::new(source).unwrap();
        assert_eq!(looping.sample(4.0), start);
        assert_eq!(looping.sample(5.0), mid);
        assert_eq!(looping.sample(9.0), mid);
        assert_eq!(looping.sample(3.0).translation, mid.translation);
    }
}
