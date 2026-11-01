//! Opt-in launch sampling, independent of shape and velocity geometry.
use crate::{ModuleId, ModuleInstance, ModuleParameters, ModuleTypeId, StageKind, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MODULE_PARTICLE_SAMPLING: &str = "aestra.spawn.particle_sampling";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u32)]
pub enum SamplingMode {
    /// Keep the historical sampler, including its historical density.
    #[default]
    Legacy = 0,
    /// Independent uniform area/volume/solid-angle sampling.
    Uniform = 1,
    /// Stratified height and low-discrepancy azimuth; sphere directions use a rotated Fibonacci layout.
    Stratified = 2,
}
impl SamplingMode {
    pub const ALL: [Self; 3] = [Self::Legacy, Self::Uniform, Self::Stratified];
    pub fn name(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Uniform => "uniform",
            Self::Stratified => "stratified",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParticleSampling {
    pub positions: SamplingMode,
    pub directions: SamplingMode,
    /// Repeating pattern length for direct/continuous emission. Event links use
    /// their requested fan-out per source instead. This is not a birth limit.
    pub sequence_count: u32,
    /// Fraction of a stratum (0..1); never rejects or adds births.
    pub jitter: f32,
    pub seed: u32,
}
impl Default for ParticleSampling {
    fn default() -> Self {
        Self {
            positions: SamplingMode::Uniform,
            directions: SamplingMode::Legacy,
            sequence_count: 64,
            jitter: 0.12,
            seed: 17,
        }
    }
}
impl ParticleSampling {
    pub fn is_valid(self) -> bool {
        // Up to 2^24 strata can be represented by the f32 coordinate contract.
        (1..=16_777_216).contains(&self.sequence_count)
            && self.jitter.is_finite()
            && (0.0..=1.0).contains(&self.jitter)
    }
    pub fn values(self) -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                "position_sampling".into(),
                Value::Text(self.positions.name().into()),
            ),
            (
                "direction_sampling".into(),
                Value::Text(self.directions.name().into()),
            ),
            ("sequence_count".into(), Value::U32(self.sequence_count)),
            ("jitter".into(), Value::Scalar(self.jitter)),
            ("seed".into(), Value::U32(self.seed)),
        ])
    }
    pub fn from_values(values: &BTreeMap<String, Value>) -> Option<Self> {
        let mode = |key| match values.get(key)? {
            Value::Text(v) => SamplingMode::ALL.into_iter().find(|mode| mode.name() == v),
            _ => None,
        };
        let Value::U32(sequence_count) = values.get("sequence_count")? else {
            return None;
        };
        let Value::U32(seed) = values.get("seed")? else {
            return None;
        };
        let Value::Scalar(jitter) = values.get("jitter")? else {
            return None;
        };
        let result = Self {
            positions: mode("position_sampling")?,
            directions: mode("direction_sampling")?,
            sequence_count: *sequence_count,
            jitter: *jitter,
            seed: *seed,
        };
        result.is_valid().then_some(result)
    }
}
impl ModuleInstance {
    pub fn particle_sampling(settings: ParticleSampling) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_PARTICLE_SAMPLING),
            stage: StageKind::ParticleSpawn,
            enabled: true,
            parameters: ModuleParameters::Custom(settings.values()),
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
            host_bindings: BTreeMap::new(),
            label: None,
            schema_version: None,
        }
    }
    pub fn particle_sampling_settings(&self) -> Option<ParticleSampling> {
        if self.module_type.0 != MODULE_PARTICLE_SAMPLING {
            return None;
        }
        let ModuleParameters::Custom(values) = &self.parameters else {
            return None;
        };
        ParticleSampling::from_values(values)
    }
}
