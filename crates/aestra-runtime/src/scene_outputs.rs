//! Engine-neutral scene-light realization and compiled bindings.
#[cfg(test)]
use aestra_core::CurveKey;
pub use aestra_core::{MAX_LIGHT_CURVE_KEYS, PointLightPulse, PointLightSample};

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

/// Compiled binding to one unambiguous FirstPerTick particle output route.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledPointLightBinding {
    pub source: aestra_core::EventRouteId,
    pub route: usize,
    pub pulse: PointLightPulse,
    pub color_parameter: Option<(crate::ParameterSlot, f32)>,
}

impl CompiledPointLightBinding {
    /// Resolve occurrence color from the instance's actual parameter values.
    /// Invalid live overrides fail closed; never silently use a different color.
    pub fn resolve(&self, parameters: &[crate::RuntimeValue]) -> Option<PointLightPulse> {
        self.resolve_with(|slot| parameters.get(slot.0))
    }

    pub fn resolve_with<'a>(
        &self,
        mut parameter: impl FnMut(crate::ParameterSlot) -> Option<&'a crate::RuntimeValue>,
    ) -> Option<PointLightPulse> {
        let mut pulse = self.pulse.clone();
        if let Some((slot, age)) = self.color_parameter {
            if !age.is_finite() || !(0.0..=1.0).contains(&age) {
                return None;
            }
            let crate::RuntimeValue::Gradient(gradient) = parameter(slot)? else {
                return None;
            };
            let [r, g, b, _] = gradient.sample(age);
            pulse.linear_color = [r, g, b];
        }
        pulse.is_valid().then_some(pulse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightBindingError {
    MissingSource,
    InvalidColor,
}

impl crate::CompiledEffect {
    pub fn point_light_for_output<'a>(
        &self,
        event: &crate::EffectOutputEvent,
        parameter: impl FnMut(crate::ParameterSlot) -> Option<&'a crate::RuntimeValue>,
    ) -> Result<Option<(aestra_core::EventRouteId, PointLightPulse)>, LightBindingError> {
        let crate::EventOrigin::Emitter(emitter) = event.origin else {
            return Ok(None);
        };
        let Some(binding) = self.point_lights.iter().find(|binding| {
            matches!(self.event_routes.get(binding.route), Some(crate::CompiledEventRoute::ParticleOutput(route))
                if route.source == emitter && route.output == event.kind)
        }) else { return Ok(None); };
        binding
            .resolve_with(parameter)
            .map(|pulse| Some((binding.source, pulse)))
            .ok_or(LightBindingError::InvalidColor)
    }
}

impl crate::CompiledEffectProject {
    /// Resolve by stable source/path even after a temporary child presentation has
    /// expired. Only the final clip's own exposed overrides apply to its source.
    /// Root live values are current delivery-time values, not historical snapshots.
    pub fn point_light_for_output(
        &self,
        path: &[aestra_core::EffectClipId],
        source: aestra_core::EffectId,
        event: &crate::EffectOutputEvent,
        root_parameters: &[crate::RuntimeValue],
    ) -> Result<Option<(aestra_core::EventRouteId, PointLightPulse)>, LightBindingError> {
        if path.len() > 63 {
            return Err(LightBindingError::MissingSource);
        }
        let mut effect = &self.root;
        let mut overrides: &[crate::CompiledParameterOverride] = &[];
        for id in path {
            let clip = effect
                .effect_clips
                .iter()
                .find(|clip| clip.source_clip == *id)
                .ok_or(LightBindingError::MissingSource)?;
            overrides = &clip.parameter_overrides;
            effect = self
                .effect(clip.source.id)
                .ok_or(LightBindingError::MissingSource)?;
        }
        if effect.source != source {
            return Err(LightBindingError::MissingSource);
        }
        effect.point_light_for_output(event, |slot| {
            if path.is_empty() {
                root_parameters.get(slot.0)
            } else {
                overrides
                    .iter()
                    .find(|value| value.slot == slot)
                    .map(|value| &value.value)
                    .or_else(|| {
                        effect
                            .parameters
                            .get(slot.0)
                            .map(|parameter| &parameter.default)
                    })
            }
        })
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
