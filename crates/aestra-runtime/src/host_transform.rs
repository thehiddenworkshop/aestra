use crate::CompiledCurve;
use aestra_core::{EmitterTransform, HostTransformError, HostTransformTrack};

/// Effect-transform target using the same scalar evaluator as property curves.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledHostTransformTrack {
    source: HostTransformTrack,
    translation: [CompiledCurve; 3],
    scale: [CompiledCurve; 3],
}
impl CompiledHostTransformTrack {
    pub fn new(source: HostTransformTrack) -> Result<Self, HostTransformError> {
        source.validate()?;
        Ok(Self {
            translation: std::array::from_fn(|i| {
                CompiledCurve::compile(&source.curves.translation[i])
            }),
            scale: std::array::from_fn(|i| CompiledCurve::compile(&source.curves.scale[i])),
            source,
        })
    }
    pub fn source(&self) -> &HostTransformTrack {
        &self.source
    }
    pub fn sample(&self, time: f32) -> EmitterTransform {
        let time = self.source.sample_time(time);
        EmitterTransform {
            translation: std::array::from_fn(|i| self.translation[i].sample_at(time)),
            scale: std::array::from_fn(|i| self.scale[i].sample_at(time)),
            rotation: self.source.curves.rotation.sample_at(time),
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
        let mut source = HostTransformTrack::from_pose_keys(
            vec![
                HostTransformKey {
                    time: 0.0,
                    transform: start,
                },
                HostTransformKey {
                    time: 2.0,
                    transform: end,
                },
            ],
            false,
        );
        let track = CompiledHostTransformTrack::new(source.clone()).unwrap();
        let mid = track.sample(1.0);
        assert_eq!(mid.translation, [5.0, -3.0, 1.0]);
        assert_eq!(mid.scale, [2.0, 0.75, 1.5]);
        assert!((mid.rotation[2] - (std::f32::consts::PI / 8.0).sin()).abs() < 1e-6);
        assert!((mid.rotation[3] - (std::f32::consts::PI / 8.0).cos()).abs() < 1e-6);
        assert_eq!(track.sample(10.0), end);
        assert_eq!(track.sample(-1.0), start);
        assert_eq!(track.sample(f32::NAN), start);
        source.curves.set_pose(4.0, start, true);
        source.repeat = true;
        let looping = CompiledHostTransformTrack::new(source).unwrap();
        assert_eq!(looping.sample(4.0), start);
        assert_eq!(looping.sample(5.0), mid);
        assert_eq!(looping.sample(9.0), mid);
        assert_eq!(looping.sample(3.0).translation, mid.translation);
    }
}
