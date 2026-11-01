//! V1 opt-in particle acceleration from a shared, seeded spatial potential.
use crate::{ModuleId, ModuleInstance, ModuleParameters, ModuleTypeId, StageKind, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub const MODULE_SPATIAL_FLOW: &str = "aestra.update.spatial_flow";
pub const MAX_FLOW_OCTAVES: u32 = 3;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowMode {
    Noise,
    Curl,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowSpace {
    Effect,
    World,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpatialFlow {
    pub mode: FlowMode,
    pub space: FlowSpace,
    pub scale: f32,
    /// Maximum absolute acceleration per axis, in the selected space's units/s².
    pub strength: f32,
    /// Translation of the potential, in selected-space units per effect second.
    pub evolution: [f32; 3],
    pub offset: [f32; 3],
    pub seed: u32,
    pub octaves: u32,
}
impl Default for SpatialFlow {
    fn default() -> Self {
        Self {
            mode: FlowMode::Curl,
            space: FlowSpace::Effect,
            scale: 0.15,
            strength: 1.0,
            evolution: [0.12, 0.04, 0.08],
            offset: [0.0; 3],
            seed: 17,
            octaves: 1,
        }
    }
}
impl SpatialFlow {
    pub fn is_valid(self) -> bool {
        self.scale.is_finite()
            && (0.001..=64.0).contains(&self.scale)
            && self.strength.is_finite()
            && (0.0..=1000.0).contains(&self.strength)
            && (1..=MAX_FLOW_OCTAVES).contains(&self.octaves)
            && self
                .offset
                .into_iter()
                .chain(self.evolution)
                .all(|v| v.is_finite() && v.abs() <= 65536.0)
    }
    pub fn values(self) -> BTreeMap<String, Value> {
        BTreeMap::from([
            (
                "flow_mode".into(),
                Value::Text(
                    match self.mode {
                        FlowMode::Noise => "noise",
                        FlowMode::Curl => "curl",
                    }
                    .into(),
                ),
            ),
            (
                "flow_space".into(),
                Value::Text(
                    match self.space {
                        FlowSpace::Effect => "effect",
                        FlowSpace::World => "world",
                    }
                    .into(),
                ),
            ),
            ("scale".into(), Value::Scalar(self.scale)),
            ("strength".into(), Value::Scalar(self.strength)),
            ("evolution".into(), Value::Vec3(self.evolution)),
            ("offset".into(), Value::Vec3(self.offset)),
            ("seed".into(), Value::U32(self.seed)),
            ("octaves".into(), Value::U32(self.octaves)),
        ])
    }
    pub fn from_values(values: &BTreeMap<String, Value>) -> Option<Self> {
        let scalar = |key| match values.get(key)? {
            Value::Scalar(v) => Some(*v),
            _ => None,
        };
        let vector = |key| match values.get(key)? {
            Value::Vec3(v) => Some(*v),
            _ => None,
        };
        let word = |key| match values.get(key)? {
            Value::U32(v) => Some(*v),
            _ => None,
        };
        let mode = match values.get("flow_mode")? {
            Value::Text(v) if v == "noise" => FlowMode::Noise,
            Value::Text(v) if v == "curl" => FlowMode::Curl,
            _ => return None,
        };
        let space = match values.get("flow_space")? {
            Value::Text(v) if v == "effect" => FlowSpace::Effect,
            Value::Text(v) if v == "world" => FlowSpace::World,
            _ => return None,
        };
        let flow = Self {
            mode,
            space,
            scale: scalar("scale")?,
            strength: scalar("strength")?,
            evolution: vector("evolution")?,
            offset: vector("offset")?,
            seed: word("seed")?,
            octaves: word("octaves")?,
        };
        flow.is_valid().then_some(flow)
    }
}
impl ModuleInstance {
    /// Uses the existing self-describing property payload, so the normal schema
    /// inspector, serialization and undo path edit the same settings hosts use.
    pub fn spatial_flow(flow: SpatialFlow) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_SPATIAL_FLOW),
            stage: StageKind::ParticleUpdate,
            enabled: true,
            parameters: ModuleParameters::Custom(flow.values()),
            property_sources: BTreeMap::new(),
            property_source_values: BTreeMap::new(),
            bindings: BTreeMap::new(),
            host_bindings: BTreeMap::new(),
            label: None,
            schema_version: None,
        }
    }
    pub fn spatial_flow_settings(&self) -> Option<SpatialFlow> {
        if self.module_type.0 != MODULE_SPATIAL_FLOW {
            return None;
        }
        let ModuleParameters::Custom(values) = &self.parameters else {
            return None;
        };
        SpatialFlow::from_values(values)
    }
}
