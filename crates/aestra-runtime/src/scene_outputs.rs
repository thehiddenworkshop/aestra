//! Engine-neutral, representative scene-light intents. No host entities or audio.
use aestra_core::{Curve, CurveKey};

pub const MAX_LIGHT_CURVE_KEYS: usize = 32;

/// One representative point-light pulse, not one light per particle.
/// RGB is normalized linear color (0..1), intensity is lumens; radius/range use
/// the host's world units. Put radiance scale in intensity, not RGB channels.
/// Curves use normalized pulse age. Shadows are an adapter/host policy, off by default.
#[derive(Debug, Clone, PartialEq)]
pub struct PointLightPulse {
    pub linear_color: [f32; 3],
    pub intensity_lumens: Curve,
    pub range: Curve,
    pub radius: f32,
    pub duration_seconds: f32,
}

impl PointLightPulse {
    /// Immediate flash with a rapid fade, independent of particle/trail lifetime.
    pub fn flash(color: [f32; 3], lumens: f32, range: f32, duration: f32) -> Self {
        Self {
            linear_color: color,
            intensity_lumens: Curve::new(vec![
                CurveKey::new(0.0, lumens),
                CurveKey::new(0.2, lumens * 0.25),
                CurveKey::new(1.0, 0.0),
            ]),
            range: Curve::new(vec![CurveKey::new(0.0, range), CurveKey::new(1.0, range)]),
            radius: 0.1,
            duration_seconds: duration,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.linear_color
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.radius.is_finite()
            && self.radius >= 0.0
            && self.duration_seconds.is_finite()
            && self.duration_seconds > 0.0
            && valid_curve(&self.intensity_lumens, false)
            && valid_curve(&self.range, true)
    }

    /// Sample using occurrence time, never delivery-frame wall time. Future and
    /// expired pulses are absent; adapters decide whether to queue future intents.
    pub fn sample(&self, age_seconds: f32) -> Option<PointLightSample> {
        if !self.is_valid()
            || !age_seconds.is_finite()
            || age_seconds < 0.0
            || age_seconds >= self.duration_seconds
        {
            return None;
        }
        let age = age_seconds / self.duration_seconds;
        Some(PointLightSample {
            linear_color: self.linear_color,
            intensity_lumens: self.intensity_lumens.sample(age),
            range: self.range.sample(age),
            radius: self.radius,
        })
    }
}

fn valid_curve(curve: &Curve, positive: bool) -> bool {
    let valid_value =
        |value: f32| value.is_finite() && if positive { value > 0.0 } else { value >= 0.0 };
    !curve.keys.is_empty()
        && curve.keys.len() <= MAX_LIGHT_CURVE_KEYS
        && curve.keys.iter().all(|key| {
            key.time.is_finite()
                && (0.0..=1.0).contains(&key.time)
                && key.value.is_finite()
                && valid_value(curve.output_value(key.value))
        })
        && curve
            .keys
            .windows(2)
            .all(|pair| pair[0].time < pair[1].time)
        && curve.output_range.is_none_or(|range| {
            valid_value(range.min) && valid_value(range.max) && range.min <= range.max
        })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointLightSample {
    pub linear_color: [f32; 3],
    pub intensity_lumens: f32,
    pub range: f32,
    pub radius: f32,
}

/// A scene-output intent at a fixed world-space event position and root-clock time.
/// Hosts attach their instance/epoch/duplicate identity in the adapter envelope.
#[derive(Debug, Clone, PartialEq)]
pub struct TransientPointLight {
    pub world_position: [f32; 3],
    pub root_time_seconds: f32,
    pub pulse: PointLightPulse,
}

impl TransientPointLight {
    pub fn is_valid(&self) -> bool {
        self.world_position.iter().all(|v| v.is_finite())
            && self.root_time_seconds.is_finite()
            && self.root_time_seconds >= 0.0
            && self.pulse.is_valid()
    }
    pub fn sample(&self, root_time_seconds: f32) -> Option<PointLightSample> {
        self.is_valid()
            .then(|| {
                self.pulse
                    .sample(root_time_seconds - self.root_time_seconds)
            })
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pulse_uses_occurrence_time_and_has_a_bounded_independent_envelope() {
        let pulse = PointLightPulse::flash([1.0, 0.2, 0.1], 1000.0, 20.0, 0.5);
        let light = TransientPointLight {
            world_position: [1.0, 2.0, 3.0],
            root_time_seconds: 2.0,
            pulse,
        };
        assert_eq!(light.sample(2.0).unwrap().intensity_lumens, 1000.0);
        assert!((light.sample(2.1).unwrap().intensity_lumens - 250.0).abs() < 0.001);
        assert!(light.sample(1.9).is_none());
        assert!(light.sample(2.5).is_none());
        assert!(light.sample(f32::NAN).is_none());
        assert_eq!(
            light.sample(2.1),
            light.sample(2.1),
            "paused clocks freeze the pulse"
        );
    }
    #[test]
    fn invalid_nonfinite_and_unbounded_intents_are_rejected() {
        let valid = PointLightPulse::flash([1.0; 3], 1000.0, 20.0, 0.5);
        for field in 0..7 {
            let mut pulse = valid.clone();
            match field {
                0 => pulse.linear_color[0] = f32::NAN,
                1 => pulse.duration_seconds = 0.0,
                2 => pulse.radius = -1.0,
                3 => pulse.range.keys[0].value = 0.0,
                4 => pulse.intensity_lumens.keys[0].value = f32::INFINITY,
                5 => pulse.range.keys.swap(0, 1),
                _ => pulse.range.keys = vec![CurveKey::new(0.0, 1.0); MAX_LIGHT_CURVE_KEYS + 1],
            }
            assert!(!pulse.is_valid());
        }
        let mut light = TransientPointLight {
            world_position: [0.0; 3],
            root_time_seconds: 0.0,
            pulse: valid,
        };
        light.world_position[2] = f32::INFINITY;
        assert!(light.sample(0.0).is_none());
    }
}
