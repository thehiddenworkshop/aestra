use crate::{
    AssetId, CurveId, Diagnostic, DiagnosticCode, EffectId, EmitterId, EventId, GradientId,
    MaterialId, ModuleId, ParameterId, RendererId, ValidationReport,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::Path};
use tempfile::NamedTempFile;
use thiserror::Error;

pub const MODULE_EMISSION: &str = "aestra.emission.rate";
pub const MODULE_SHAPE: &str = "aestra.spawn.shape";
pub const MODULE_INITIALIZE: &str = "aestra.spawn.initialize";
pub const MODULE_MOTION: &str = "aestra.update.motion";
pub const MODULE_APPEARANCE: &str = "aestra.update.appearance";
pub const RENDERER_SPRITE: &str = "aestra.renderer.sprite";
pub const RENDERER_RIBBON: &str = "aestra.renderer.ribbon";
pub const RENDERER_MESH: &str = "aestra.renderer.mesh";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectAsset {
    pub format_version: u32,
    pub id: EffectId,
    pub name: String,
    pub duration: f32,
    pub looping: bool,
    #[serde(default)]
    pub parameters: Vec<EffectParameter>,
    #[serde(default)]
    pub emitters: Vec<Emitter>,
    #[serde(default)]
    pub events: Vec<EventLink>,
    #[serde(default)]
    pub dependencies: Vec<AssetId>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

impl EffectAsset {
    pub fn new(name: impl Into<String>, duration: f32) -> Self {
        Self {
            format_version: crate::CURRENT_FORMAT_VERSION,
            id: EffectId::new(),
            name: name.into(),
            duration,
            looping: true,
            parameters: Vec::new(),
            emitters: Vec::new(),
            events: Vec::new(),
            dependencies: Vec::new(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn validation_report(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        if self.format_version != crate::CURRENT_FORMAT_VERSION {
            report.push(Diagnostic::error(
                DiagnosticCode::UnsupportedFormat,
                "effect.format_version",
                format!(
                    "effect format version {} is unsupported; expected {}",
                    self.format_version,
                    crate::CURRENT_FORMAT_VERSION
                ),
            ));
        }
        if self.id.is_nil() {
            report.push(Diagnostic::error(
                DiagnosticCode::NilId,
                "effect.id",
                "effect ID cannot be nil",
            ));
        }
        if !self.duration.is_finite() || self.duration <= 0.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidDuration,
                "effect.duration",
                format!(
                    "effect duration must be positive and finite, got {}",
                    self.duration
                ),
            ));
        }

        let mut semantic_ids = BTreeMap::<u128, String>::new();
        register_id(
            &mut report,
            &mut semantic_ids,
            self.id.as_uuid().as_u128(),
            "effect.id".into(),
        );
        for (index, parameter) in self.parameters.iter().enumerate() {
            let path = format!("effect.parameters[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                parameter.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            validate_value(
                &parameter.default,
                &format!("{path}.default"),
                &mut report,
                &mut semantic_ids,
            );
            if matches!(parameter.default, Value::Parameter(_)) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidValue,
                    format!("{path}.default"),
                    "effect parameter defaults must be concrete values",
                ));
            }
        }
        for (index, emitter) in self.emitters.iter().enumerate() {
            let emitter_path = format!("effect.emitters[{index}]");
            emitter.validate(&emitter_path, self.duration, &mut report, &mut semantic_ids);
            for (module_index, module) in emitter.modules.iter().enumerate() {
                for (input, parameter_id) in &module.bindings {
                    let path = format!("{emitter_path}.modules[{module_index}].bindings.{input}");
                    let Some(expected) = module.parameter_type(input) else {
                        report.push(Diagnostic::error(
                            DiagnosticCode::UnknownParameter,
                            path,
                            format!(
                                "module '{}' has no input named '{input}'",
                                module.module_type.0
                            ),
                        ));
                        continue;
                    };
                    let Some(parameter) = self
                        .parameters
                        .iter()
                        .find(|parameter| parameter.id == *parameter_id)
                    else {
                        report.push(Diagnostic::error(
                            DiagnosticCode::InvalidReference,
                            path,
                            format!("binding references missing parameter {parameter_id}"),
                        ));
                        continue;
                    };
                    let actual = parameter.default.value_type();
                    if actual != expected {
                        report.push(Diagnostic::error(
                            DiagnosticCode::ParameterTypeMismatch,
                            path,
                            format!(
                                "input '{input}' expects {expected:?}, but parameter '{}' is {actual:?}",
                                parameter.name
                            ),
                        ));
                    }
                }
            }
        }
        for (index, event) in self.events.iter().enumerate() {
            let path = format!("effect.events[{index}]");
            register_id(
                &mut report,
                &mut semantic_ids,
                event.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
            if !self.emitters.iter().any(|item| item.id == event.source) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    format!("{path}.source"),
                    format!("event references missing source emitter {}", event.source),
                ));
            }
            if !self.emitters.iter().any(|item| item.id == event.target) {
                report.push(Diagnostic::error(
                    DiagnosticCode::InvalidReference,
                    format!("{path}.target"),
                    format!("event references missing target emitter {}", event.target),
                ));
            }
        }
        report
    }

    pub fn validate(&self) -> Result<(), ValidationReport> {
        self.validation_report().into_result()
    }

    pub fn from_ron(source: &str) -> Result<Self, AssetError> {
        let asset: Self = ron::from_str(source)?;
        asset.validate()?;
        Ok(asset)
    }

    pub fn load_ron(path: impl AsRef<Path>) -> Result<Self, AssetError> {
        Self::from_ron(&fs::read_to_string(path)?)
    }

    pub fn to_pretty_ron(&self) -> Result<String, AssetError> {
        self.validate()?;
        Ok(ron::ser::to_string_pretty(
            self,
            ron::ser::PrettyConfig::new().depth_limit(12),
        )?)
    }

    pub fn save_ron(&self, path: impl AsRef<Path>) -> Result<(), AssetError> {
        atomic_write(path.as_ref(), self.to_pretty_ron()?.as_bytes())?;
        Ok(())
    }
}

fn atomic_write(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    // The temporary file lives beside the destination so persistence is a same-filesystem atomic
    // replacement. If writing or syncing fails, NamedTempFile removes it and leaves the previous
    // effect untouched.
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    let persisted = temporary.persist(path).map_err(|error| error.error)?;
    persisted.sync_all()?;

    // Persist the directory entry on platforms where directory handles can be synchronized.
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EffectParameter {
    pub id: ParameterId,
    pub name: String,
    pub default: Value,
    pub exposed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Emitter {
    pub id: EmitterId,
    pub name: String,
    pub enabled: bool,
    pub start_time: f32,
    pub duration: f32,
    pub max_particles: u32,
    pub simulation_domain: SimulationDomain,
    pub modules: Vec<ModuleInstance>,
    pub renderers: Vec<RendererInstance>,
}

impl Emitter {
    pub fn basic_sprite(name: impl Into<String>, duration: f32) -> Self {
        Self {
            id: EmitterId::new(),
            name: name.into(),
            enabled: true,
            start_time: 0.0,
            duration,
            max_particles: 128,
            simulation_domain: SimulationDomain::Particle,
            modules: vec![
                ModuleInstance::emission(24.0, 12),
                ModuleInstance::shape(EmitterShape::Circle { radius: 12.0 }),
                ModuleInstance::initialize(
                    ScalarRange::new(0.6, 1.2),
                    ScalarRange::new(35.0, 90.0),
                    90.0,
                    360.0,
                    ScalarRange::new(-2.0, 2.0),
                ),
                ModuleInstance::motion([0.0, -18.0], 0.6, 4.0),
                ModuleInstance::appearance(
                    Curve::new(vec![
                        CurveKey::new(0.0, 4.0),
                        CurveKey::new(0.35, 10.0),
                        CurveKey::new(1.0, 1.0),
                    ]),
                    Curve::new(vec![
                        CurveKey::new(0.0, 0.0),
                        CurveKey::new(0.12, 1.0),
                        CurveKey::new(1.0, 0.0),
                    ]),
                    Gradient::new(vec![
                        ColorKey::new(0.0, [0.35, 0.75, 1.0, 1.0]),
                        ColorKey::new(0.5, [0.62, 0.3, 1.0, 1.0]),
                        ColorKey::new(1.0, [0.15, 0.05, 0.4, 0.0]),
                    ]),
                ),
            ],
            renderers: vec![RendererInstance::sprite(BlendMode::Additive, 0.5)],
        }
    }

    pub fn regenerate_ids(&mut self) {
        self.id = EmitterId::new();
        for module in &mut self.modules {
            module.regenerate_ids();
        }
        for renderer in &mut self.renderers {
            renderer.id = RendererId::new();
        }
    }

    pub fn spawn_rate(&self) -> f32 {
        self.emission().0
    }

    pub fn spawn_rate_mut(&mut self) -> &mut f32 {
        match &mut self.module_mut(MODULE_EMISSION).parameters {
            ModuleParameters::Emission { spawn_rate, .. } => spawn_rate,
            _ => unreachable!("validated emission module has emission parameters"),
        }
    }

    pub fn burst_count(&self) -> u32 {
        self.emission().1
    }

    pub fn burst_count_mut(&mut self) -> &mut u32 {
        match &mut self.module_mut(MODULE_EMISSION).parameters {
            ModuleParameters::Emission { burst_count, .. } => burst_count,
            _ => unreachable!("validated emission module has emission parameters"),
        }
    }

    pub fn shape(&self) -> &EmitterShape {
        match &self.module(MODULE_SHAPE).parameters {
            ModuleParameters::Shape { shape } => shape,
            _ => unreachable!("validated shape module has shape parameters"),
        }
    }

    pub fn lifetime(&self) -> ScalarRange {
        self.initialize().0
    }

    pub fn lifetime_mut(&mut self) -> &mut ScalarRange {
        match &mut self.module_mut(MODULE_INITIALIZE).parameters {
            ModuleParameters::Initialize { lifetime, .. } => lifetime,
            _ => unreachable!("validated initialize module has initialize parameters"),
        }
    }

    pub fn speed(&self) -> ScalarRange {
        self.initialize().1
    }

    pub fn direction_degrees(&self) -> f32 {
        self.initialize().2
    }

    pub fn spread_degrees(&self) -> f32 {
        self.initialize().3
    }

    pub fn angular_velocity(&self) -> ScalarRange {
        self.initialize().4
    }

    pub fn gravity(&self) -> [f32; 2] {
        self.motion().0
    }

    pub fn drag(&self) -> f32 {
        self.motion().1
    }

    pub fn turbulence(&self) -> f32 {
        self.motion().2
    }

    pub fn size_curve(&self) -> &Curve {
        self.appearance().0
    }

    pub fn size_curve_mut(&mut self) -> &mut Curve {
        match &mut self.module_mut(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance { size, .. } => size,
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    pub fn opacity_curve(&self) -> &Curve {
        self.appearance().1
    }

    pub fn opacity_curve_mut(&mut self) -> &mut Curve {
        match &mut self.module_mut(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance { opacity, .. } => opacity,
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    pub fn color_gradient(&self) -> &Gradient {
        self.appearance().2
    }

    pub fn color_gradient_mut(&mut self) -> &mut Gradient {
        match &mut self.module_mut(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance { color, .. } => color,
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    pub fn module_by_type(&self, module_type: &str) -> Option<&ModuleInstance> {
        self.modules
            .iter()
            .find(|module| module.module_type.0 == module_type)
    }

    pub fn module_by_id(&self, id: ModuleId) -> Option<&ModuleInstance> {
        self.modules.iter().find(|module| module.id == id)
    }

    pub fn module_by_id_mut(&mut self, id: ModuleId) -> Option<&mut ModuleInstance> {
        self.modules.iter_mut().find(|module| module.id == id)
    }

    fn emission(&self) -> (f32, u32) {
        match &self.module(MODULE_EMISSION).parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => (*spawn_rate, *burst_count),
            _ => unreachable!("validated emission module has emission parameters"),
        }
    }

    fn initialize(&self) -> (ScalarRange, ScalarRange, f32, f32, ScalarRange) {
        match &self.module(MODULE_INITIALIZE).parameters {
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction_degrees,
                spread_degrees,
                angular_velocity,
            } => (
                *lifetime,
                *speed,
                *direction_degrees,
                *spread_degrees,
                *angular_velocity,
            ),
            _ => unreachable!("validated initialize module has initialize parameters"),
        }
    }

    fn motion(&self) -> ([f32; 2], f32, f32) {
        match &self.module(MODULE_MOTION).parameters {
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => (*gravity, *drag, *turbulence),
            _ => unreachable!("validated motion module has motion parameters"),
        }
    }

    fn appearance(&self) -> (&Curve, &Curve, &Gradient) {
        match &self.module(MODULE_APPEARANCE).parameters {
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => (size, opacity, color),
            _ => unreachable!("validated appearance module has appearance parameters"),
        }
    }

    fn module(&self, module_type: &str) -> &ModuleInstance {
        self.modules
            .iter()
            .find(|module| module.module_type.0 == module_type)
            .unwrap_or_else(|| panic!("emitter is missing required module {module_type}"))
    }

    fn module_mut(&mut self, module_type: &str) -> &mut ModuleInstance {
        self.modules
            .iter_mut()
            .find(|module| module.module_type.0 == module_type)
            .unwrap_or_else(|| panic!("emitter is missing required module {module_type}"))
    }

    fn validate(
        &self,
        path: &str,
        effect_duration: f32,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        if !self.start_time.is_finite()
            || self.start_time < 0.0
            || !self.duration.is_finite()
            || self.duration <= 0.0
            || self.start_time + self.duration > effect_duration + f32::EPSILON
        {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidTiming,
                path,
                format!("emitter '{}' has invalid timing", self.name),
            ));
        }
        if self.max_particles == 0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidCapacity,
                format!("{path}.max_particles"),
                "emitter capacity must be greater than zero",
            ));
        }

        for required in [
            MODULE_EMISSION,
            MODULE_SHAPE,
            MODULE_INITIALIZE,
            MODULE_MOTION,
            MODULE_APPEARANCE,
        ] {
            let count = self
                .modules
                .iter()
                .filter(|module| module.module_type.0 == required)
                .count();
            if count == 0 {
                report.push(Diagnostic::error(
                    DiagnosticCode::MissingModule,
                    format!("{path}.modules"),
                    format!("emitter is missing required module '{required}'"),
                ));
            }
        }

        for (index, module) in self.modules.iter().enumerate() {
            module.validate(&format!("{path}.modules[{index}]"), report, semantic_ids);
        }
        if self.renderers.is_empty() {
            report.push(Diagnostic::error(
                DiagnosticCode::MissingRenderer,
                format!("{path}.renderers"),
                "emitter must have at least one renderer",
            ));
        }
        for (index, renderer) in self.renderers.iter().enumerate() {
            renderer.validate(&format!("{path}.renderers[{index}]"), report, semantic_ids);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SimulationDomain {
    Particle,
    Strip,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StageKind {
    EffectSpawn,
    EffectUpdate,
    EmitterSpawn,
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
    Simulation(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct ModuleTypeId(pub String);

impl ModuleTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleInstance {
    pub id: ModuleId,
    pub module_type: ModuleTypeId,
    pub stage: StageKind,
    pub enabled: bool,
    pub parameters: ModuleParameters,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, ParameterId>,
}

impl ModuleInstance {
    pub fn emission(spawn_rate: f32, burst_count: u32) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_EMISSION),
            stage: StageKind::EmitterUpdate,
            enabled: true,
            parameters: ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            },
            bindings: BTreeMap::new(),
        }
    }

    pub fn shape(shape: EmitterShape) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_SHAPE),
            stage: StageKind::ParticleSpawn,
            enabled: true,
            parameters: ModuleParameters::Shape { shape },
            bindings: BTreeMap::new(),
        }
    }

    pub fn initialize(
        lifetime: ScalarRange,
        speed: ScalarRange,
        direction_degrees: f32,
        spread_degrees: f32,
        angular_velocity: ScalarRange,
    ) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_INITIALIZE),
            stage: StageKind::ParticleSpawn,
            enabled: true,
            parameters: ModuleParameters::Initialize {
                lifetime,
                speed,
                direction_degrees,
                spread_degrees,
                angular_velocity,
            },
            bindings: BTreeMap::new(),
        }
    }

    pub fn motion(gravity: [f32; 2], drag: f32, turbulence: f32) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_MOTION),
            stage: StageKind::ParticleUpdate,
            enabled: true,
            parameters: ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            },
            bindings: BTreeMap::new(),
        }
    }

    pub fn appearance(size: Curve, opacity: Curve, color: Gradient) -> Self {
        Self {
            id: ModuleId::new(),
            module_type: ModuleTypeId::new(MODULE_APPEARANCE),
            stage: StageKind::ParticleUpdate,
            enabled: true,
            parameters: ModuleParameters::Appearance {
                size,
                opacity,
                color,
            },
            bindings: BTreeMap::new(),
        }
    }

    /// Assign fresh semantic IDs to a cloned module and its nested authored values.
    pub fn regenerate_ids(&mut self) {
        self.id = ModuleId::new();
        match &mut self.parameters {
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                size.id = CurveId::new();
                opacity.id = CurveId::new();
                color.id = GradientId::new();
            }
            ModuleParameters::Custom(values) => {
                for value in values.values_mut() {
                    value.regenerate_ids();
                }
            }
            _ => {}
        }
    }

    pub fn parameter_type(&self, parameter: &str) -> Option<ValueType> {
        match (&self.parameters, parameter) {
            (ModuleParameters::Emission { .. }, "spawn_rate") => Some(ValueType::Scalar),
            (ModuleParameters::Emission { .. }, "burst_count") => Some(ValueType::U32),
            (ModuleParameters::Shape { .. }, "shape") => Some(ValueType::Shape),
            (ModuleParameters::Initialize { .. }, "lifetime" | "speed" | "angular_velocity") => {
                Some(ValueType::Range)
            }
            (ModuleParameters::Initialize { .. }, "direction_degrees" | "spread_degrees") => {
                Some(ValueType::Scalar)
            }
            (ModuleParameters::Motion { .. }, "gravity") => Some(ValueType::Vec2),
            (ModuleParameters::Motion { .. }, "drag" | "turbulence") => Some(ValueType::Scalar),
            (ModuleParameters::Appearance { .. }, "size" | "opacity") => Some(ValueType::Curve),
            (ModuleParameters::Appearance { .. }, "color") => Some(ValueType::Gradient),
            (ModuleParameters::Custom(values), name) => values.get(name).map(Value::value_type),
            _ => None,
        }
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        let expected = match &self.parameters {
            ModuleParameters::Emission { .. } => (MODULE_EMISSION, StageKind::EmitterUpdate),
            ModuleParameters::Shape { .. } => (MODULE_SHAPE, StageKind::ParticleSpawn),
            ModuleParameters::Initialize { .. } => (MODULE_INITIALIZE, StageKind::ParticleSpawn),
            ModuleParameters::Motion { .. } => (MODULE_MOTION, StageKind::ParticleUpdate),
            ModuleParameters::Appearance { .. } => (MODULE_APPEARANCE, StageKind::ParticleUpdate),
            ModuleParameters::Custom(values) => {
                if self.module_type.0.trim().is_empty() {
                    invalid_value(report, path, "custom module type cannot be empty");
                }
                for (name, value) in values {
                    validate_value(
                        value,
                        &format!("{path}.parameters.{name}"),
                        report,
                        semantic_ids,
                    );
                }
                return;
            }
        };
        if self.module_type.0 != expected.0 {
            report.push(Diagnostic::error(
                DiagnosticCode::InvalidValue,
                format!("{path}.module_type"),
                format!(
                    "module parameters require type '{}', found '{}'",
                    expected.0, self.module_type.0
                ),
            ));
        }
        if self.stage != expected.1 {
            report.push(Diagnostic::error(
                DiagnosticCode::StageMismatch,
                format!("{path}.stage"),
                format!(
                    "module '{}' is assigned to the wrong stage",
                    self.module_type.0
                ),
            ));
        }
        match &self.parameters {
            ModuleParameters::Emission { spawn_rate, .. }
                if !spawn_rate.is_finite() || *spawn_rate < 0.0 =>
            {
                invalid_value(report, path, "spawn rate must be finite and non-negative");
            }
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction_degrees,
                spread_degrees,
                angular_velocity,
            } => {
                validate_range(*lifetime, path, "lifetime", report);
                validate_range(*speed, path, "speed", report);
                validate_range(*angular_velocity, path, "angular_velocity", report);
                if [*direction_degrees, *spread_degrees]
                    .iter()
                    .any(|value| !value.is_finite())
                {
                    invalid_value(report, path, "direction and spread must be finite");
                }
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } if gravity.iter().any(|value| !value.is_finite())
                || !drag.is_finite()
                || *drag < 0.0
                || !turbulence.is_finite() =>
            {
                invalid_value(
                    report,
                    path,
                    "motion parameters must be finite and drag non-negative",
                );
            }
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                validate_curve(size, &format!("{path}.size"), report);
                validate_curve(opacity, &format!("{path}.opacity"), report);
                validate_gradient(color, &format!("{path}.color"), report);
                register_id(
                    report,
                    semantic_ids,
                    size.id.as_uuid().as_u128(),
                    format!("{path}.size.id"),
                );
                register_id(
                    report,
                    semantic_ids,
                    opacity.id.as_uuid().as_u128(),
                    format!("{path}.opacity.id"),
                );
                register_id(
                    report,
                    semantic_ids,
                    color.id.as_uuid().as_u128(),
                    format!("{path}.color.id"),
                );
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModuleParameters {
    Emission {
        spawn_rate: f32,
        burst_count: u32,
    },
    Shape {
        shape: EmitterShape,
    },
    Initialize {
        lifetime: ScalarRange,
        speed: ScalarRange,
        direction_degrees: f32,
        spread_degrees: f32,
        angular_velocity: ScalarRange,
    },
    Motion {
        gravity: [f32; 2],
        drag: f32,
        turbulence: f32,
    },
    Appearance {
        size: Curve,
        opacity: Curve,
        color: Gradient,
    },
    Custom(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Value {
    Bool(bool),
    U32(u32),
    Scalar(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    Text(String),
    Range(ScalarRange),
    Curve(Curve),
    Gradient(Gradient),
    Shape(EmitterShape),
    Parameter(ParameterId),
    Asset(AssetId),
    Material(MaterialId),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ValueType {
    Bool,
    U32,
    Scalar,
    Vec2,
    Vec3,
    Vec4,
    Text,
    Range,
    Curve,
    Gradient,
    Shape,
    Parameter,
    Asset,
    Material,
}

impl Value {
    pub fn value_type(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::U32(_) => ValueType::U32,
            Self::Scalar(_) => ValueType::Scalar,
            Self::Vec2(_) => ValueType::Vec2,
            Self::Vec3(_) => ValueType::Vec3,
            Self::Vec4(_) => ValueType::Vec4,
            Self::Text(_) => ValueType::Text,
            Self::Range(_) => ValueType::Range,
            Self::Curve(_) => ValueType::Curve,
            Self::Gradient(_) => ValueType::Gradient,
            Self::Shape(_) => ValueType::Shape,
            Self::Parameter(_) => ValueType::Parameter,
            Self::Asset(_) => ValueType::Asset,
            Self::Material(_) => ValueType::Material,
        }
    }

    fn regenerate_ids(&mut self) {
        match self {
            Value::Curve(curve) => curve.id = CurveId::new(),
            Value::Gradient(gradient) => gradient.id = GradientId::new(),
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum EmitterShape {
    Point,
    Circle { radius: f32 },
    Ring { radius: f32 },
    Cone { radius: f32, depth: f32 },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BlendMode {
    Alpha,
    Additive,
    Multiply,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(transparent)]
pub struct RendererTypeId(pub String);

impl RendererTypeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RendererInstance {
    pub id: RendererId,
    pub renderer_type: RendererTypeId,
    pub enabled: bool,
    pub blend: BlendMode,
    pub properties: RendererProperties,
}

impl RendererInstance {
    pub fn sprite(blend: BlendMode, softness: f32) -> Self {
        Self {
            id: RendererId::new(),
            renderer_type: RendererTypeId::new(RENDERER_SPRITE),
            enabled: true,
            blend,
            properties: RendererProperties::Sprite { softness },
        }
    }

    fn validate(
        &self,
        path: &str,
        report: &mut ValidationReport,
        semantic_ids: &mut BTreeMap<u128, String>,
    ) {
        register_id(
            report,
            semantic_ids,
            self.id.as_uuid().as_u128(),
            format!("{path}.id"),
        );
        let expected_type = match &self.properties {
            RendererProperties::Sprite { softness } => {
                if !softness.is_finite() || *softness < 0.0 {
                    invalid_value(
                        report,
                        path,
                        "sprite softness must be finite and non-negative",
                    );
                }
                Some(RENDERER_SPRITE)
            }
            RendererProperties::Ribbon { width } => {
                if !width.is_finite() || *width <= 0.0 {
                    invalid_value(report, path, "ribbon width must be positive and finite");
                }
                Some(RENDERER_RIBBON)
            }
            RendererProperties::Mesh { asset } => {
                if asset.is_nil() {
                    invalid_value(report, path, "mesh renderer asset cannot be nil");
                }
                Some(RENDERER_MESH)
            }
            RendererProperties::Custom(values) => {
                if self.renderer_type.0.trim().is_empty() {
                    invalid_value(report, path, "custom renderer type cannot be empty");
                }
                for (name, value) in values {
                    validate_value(
                        value,
                        &format!("{path}.properties.{name}"),
                        report,
                        semantic_ids,
                    );
                }
                None
            }
        };
        if let Some(expected_type) = expected_type
            && self.renderer_type.0 != expected_type
        {
            invalid_value(
                report,
                &format!("{path}.renderer_type"),
                &format!(
                    "renderer properties require type '{expected_type}', found '{}'",
                    self.renderer_type.0
                ),
            );
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RendererProperties {
    Sprite { softness: f32 },
    Ribbon { width: f32 },
    Mesh { asset: AssetId },
    Custom(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventLink {
    pub id: EventId,
    pub source: EmitterId,
    pub trigger: EventTrigger,
    pub target: EmitterId,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EventTrigger {
    OnSpawn,
    OnDeath,
    OnCollision,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScalarRange {
    pub min: f32,
    pub max: f32,
}

impl ScalarRange {
    pub const fn new(min: f32, max: f32) -> Self {
        Self { min, max }
    }

    pub fn sample(self, random: f32) -> f32 {
        self.min + (self.max - self.min) * random.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub id: CurveId,
    pub keys: Vec<CurveKey>,
}

impl Curve {
    pub fn new(keys: Vec<CurveKey>) -> Self {
        Self {
            id: CurveId::new(),
            keys,
        }
    }

    pub fn sample(&self, time: f32) -> f32 {
        let Some(first) = self.keys.first() else {
            return 0.0;
        };
        let time = time.clamp(0.0, 1.0);
        if time <= first.time {
            return first.value;
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if time <= b.time {
                let span = (b.time - a.time).max(f32::EPSILON);
                let x = ((time - a.time) / span).clamp(0.0, 1.0);
                let smooth = x * x * (3.0 - 2.0 * x);
                return a.value + (b.value - a.value) * smooth;
            }
        }
        self.keys.last().map_or(0.0, |key| key.value)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CurveKey {
    pub time: f32,
    pub value: f32,
}

impl CurveKey {
    pub const fn new(time: f32, value: f32) -> Self {
        Self { time, value }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gradient {
    pub id: GradientId,
    pub keys: Vec<ColorKey>,
}

impl Gradient {
    pub fn new(keys: Vec<ColorKey>) -> Self {
        Self {
            id: GradientId::new(),
            keys,
        }
    }

    pub fn sample(&self, time: f32) -> [f32; 4] {
        let Some(first) = self.keys.first() else {
            return [1.0; 4];
        };
        let time = time.clamp(0.0, 1.0);
        if time <= first.time {
            return first.color;
        }
        for pair in self.keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if time <= b.time {
                let x = ((time - a.time) / (b.time - a.time).max(f32::EPSILON)).clamp(0.0, 1.0);
                return std::array::from_fn(|index| {
                    a.color[index] + (b.color[index] - a.color[index]) * x
                });
            }
        }
        self.keys.last().map_or([1.0; 4], |key| key.color)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ColorKey {
    pub time: f32,
    pub color: [f32; 4],
}

impl ColorKey {
    pub const fn new(time: f32, color: [f32; 4]) -> Self {
        Self { time, color }
    }
}

#[derive(Debug, Error)]
pub enum AssetError {
    #[error("could not read or write the effect asset: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse the effect asset: {0}")]
    Parse(#[from] ron::error::SpannedError),
    #[error("could not serialize the effect asset: {0}")]
    Serialize(#[from] ron::Error),
    #[error("effect validation failed: {0}")]
    Validation(#[from] ValidationReport),
}

fn register_id(
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
    id: u128,
    path: String,
) {
    if id == 0 {
        report.push(Diagnostic::error(
            DiagnosticCode::NilId,
            &path,
            "semantic ID cannot be nil",
        ));
    }
    if let Some(previous) = semantic_ids.insert(id, path.clone()) {
        report.push(Diagnostic::error(
            DiagnosticCode::DuplicateId,
            path,
            format!("semantic ID is already used at {previous}"),
        ));
    }
}

fn invalid_value(report: &mut ValidationReport, path: &str, message: &str) {
    report.push(Diagnostic::error(
        DiagnosticCode::InvalidValue,
        path,
        message,
    ));
}

fn validate_range(range: ScalarRange, path: &str, field: &str, report: &mut ValidationReport) {
    if !range.min.is_finite() || !range.max.is_finite() || range.min > range.max {
        invalid_value(report, path, &format!("{field} range is invalid"));
    }
}

fn validate_curve(curve: &Curve, path: &str, report: &mut ValidationReport) {
    if curve.keys.is_empty()
        || curve.keys.iter().any(|key| {
            !key.time.is_finite() || !key.value.is_finite() || !(0.0..=1.0).contains(&key.time)
        })
        || curve
            .keys
            .windows(2)
            .any(|pair| pair[0].time > pair[1].time)
    {
        invalid_value(report, path, "curve keys are invalid or empty");
    }
}

fn validate_gradient(gradient: &Gradient, path: &str, report: &mut ValidationReport) {
    if gradient.keys.is_empty()
        || gradient.keys.iter().any(|key| {
            !key.time.is_finite()
                || !(0.0..=1.0).contains(&key.time)
                || key.color.iter().any(|channel| !channel.is_finite())
        })
        || gradient
            .keys
            .windows(2)
            .any(|pair| pair[0].time > pair[1].time)
    {
        invalid_value(report, path, "gradient keys are invalid or empty");
    }
}

fn validate_value(
    value: &Value,
    path: &str,
    report: &mut ValidationReport,
    semantic_ids: &mut BTreeMap<u128, String>,
) {
    match value {
        Value::Scalar(value) if !value.is_finite() => {
            invalid_value(report, path, "scalar value must be finite");
        }
        Value::Vec2(value) if value.iter().any(|item| !item.is_finite()) => {
            invalid_value(report, path, "vector value must be finite");
        }
        Value::Vec3(value) if value.iter().any(|item| !item.is_finite()) => {
            invalid_value(report, path, "vector value must be finite");
        }
        Value::Vec4(value) if value.iter().any(|item| !item.is_finite()) => {
            invalid_value(report, path, "vector value must be finite");
        }
        Value::Range(range) => validate_range(*range, path, "value", report),
        Value::Curve(curve) => {
            validate_curve(curve, path, report);
            register_id(
                report,
                semantic_ids,
                curve.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
        }
        Value::Gradient(gradient) => {
            validate_gradient(gradient, path, report);
            register_id(
                report,
                semantic_ids,
                gradient.id.as_uuid().as_u128(),
                format!("{path}.id"),
            );
        }
        Value::Shape(shape) => match shape {
            EmitterShape::Circle { radius } | EmitterShape::Ring { radius }
                if !radius.is_finite() || *radius < 0.0 =>
            {
                invalid_value(report, path, "shape radius must be finite and non-negative");
            }
            EmitterShape::Cone { radius, depth }
                if !radius.is_finite() || *radius < 0.0 || !depth.is_finite() || *depth <= 0.0 =>
            {
                invalid_value(
                    report,
                    path,
                    "cone radius must be non-negative and depth must be positive",
                );
            }
            _ => {}
        },
        Value::Parameter(id) if id.is_nil() => {
            invalid_value(report, path, "parameter reference cannot be nil");
        }
        Value::Asset(id) if id.is_nil() => {
            invalid_value(report, path, "asset reference cannot be nil");
        }
        Value::Material(id) if id.is_nil() => {
            invalid_value(report, path, "material reference cannot be nil");
        }
        _ => {}
    }
}
