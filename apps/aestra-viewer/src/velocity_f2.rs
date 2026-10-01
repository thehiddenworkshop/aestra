//! Generic distribution sketches, not production fireworks assets or budget gates.
use aestra_bevy::{
    EffectAsset, EffectId, EffectPlaybackMode, ModuleInstance, ScalarRange, VelocityDistribution,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    Peony,
    Ring,
    Palm,
    HemisphereFan,
    DoubleRing,
}

impl Probe {
    pub const ALL: [Self; 5] = [
        Self::Peony,
        Self::Ring,
        Self::Palm,
        Self::HemisphereFan,
        Self::DoubleRing,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Peony => "f2-peony",
            Self::Ring => "f2-ring",
            Self::Palm => "f2-palm",
            Self::HemisphereFan => "f2-hemisphere-fan",
            Self::DoubleRing => "f2-double-ring",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|probe| probe.name() == value)
    }
}

pub fn effect(probe: Probe) -> EffectAsset {
    let mut effect = super::fireworks_f0::effect();
    effect.id = EffectId::from_u128(20 + probe as u128);
    effect.name = format!("Velocity distribution sketch — {}", probe.name());
    effect.playback_mode = EffectPlaybackMode::Once;
    effect.events.clear();
    effect.emitters.truncate(1);
    let emitter = &mut effect.emitters[0];
    emitter.name = probe.name().into();
    emitter.max_particles = 256;
    emitter.transform.translation = [0.0, 24.0, 0.0];
    emitter.modules[0] = ModuleInstance::emission(0.0, 256);
    let mode = match probe {
        Probe::Peony => VelocityDistribution::Sphere,
        Probe::Ring | Probe::DoubleRing => VelocityDistribution::Ring,
        Probe::Palm => VelocityDistribution::Cone,
        Probe::HemisphereFan => VelocityDistribution::Hemisphere,
    };
    let axis = if matches!(probe, Probe::Ring | Probe::DoubleRing) {
        [0.0, 0.0, 1.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    emitter.modules[2] = ModuleInstance::initialize_with_distribution(
        ScalarRange::new(4.0, 4.0),
        ScalarRange::new(18.0, 22.0),
        mode,
        axis,
        45.0,
        ScalarRange::new(0.0, 0.0),
    );
    emitter.modules[3] = ModuleInstance::motion([0.0, -4.0, 0.0], 0.15, 0.0);
    super::fireworks_f0::fix_emitter_ids(emitter, 100);
    if probe == Probe::DoubleRing {
        let mut second = emitter.clone();
        second.name = "Tilted ring".into();
        if let aestra_bevy::ModuleParameters::Initialize { direction, .. } =
            &mut second.modules[2].parameters
        {
            *direction = [0.65, 0.0, 1.0];
        }
        super::fireworks_f0::fix_emitter_ids(&mut second, 200);
        effect.emitters.push(second);
    }
    effect
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_velocity_sketches_compile_with_stable_ids_and_no_event_workarounds() {
        for probe in Probe::ALL {
            let effect = effect(probe);
            assert_eq!(
                effect.to_pretty_ron().unwrap(),
                super::effect(probe).to_pretty_ron().unwrap()
            );
            assert!(effect.events.is_empty());
            let compiled = aestra_bevy::EffectCompiler::default()
                .compile(&effect)
                .unwrap();
            assert_eq!(
                compiled.emitters.len(),
                if probe == Probe::DoubleRing { 2 } else { 1 }
            );
        }
    }
}
