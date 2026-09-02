//! Engine-independent semantic material-program model.
//!
//! This module is additive to the legacy sprite [`crate::MaterialDefinition`]
//! contract. The compiler migration can therefore proceed without changing the
//! current effect format or renderer behavior in the same step.

use crate::{
    AssetId, BlendMode, Diagnostic, DiagnosticCode, DiagnosticSeverity, MaterialExpressionId,
    MaterialId, MaterialParameterId, MaterialProgramId, ParameterId, ValidationReport, Value,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use thiserror::Error;

pub const CURRENT_MATERIAL_SCHEMA_VERSION: u32 = 1;

/// Reflected parameter name used by the temporary legacy-sprite compatibility adapter.
///
/// Material 5 keeps feathered sprite coverage outside the semantic color/alpha graph while the
/// old sprite path is migrated. Backends may use this reflected value to populate their existing
/// sprite-coverage input without coupling migration code to a renderer implementation.
pub const LEGACY_SPRITE_SOFTNESS_PARAMETER: &str = "aestra.legacy.sprite_softness";

#[derive(Debug, Error)]
pub enum MaterialProgramError {
    #[error("could not read or write the material program: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not parse the material program: {0}")]
    Parse(#[from] ron::error::SpannedError),
    #[error("could not serialize the material program: {0}")]
    Serialize(#[from] ron::Error),
    #[error("material program validation failed: {0}")]
    Validation(#[from] ValidationReport),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct MaterialSchemaVersion(pub u32);

impl MaterialSchemaVersion {
    pub const CURRENT: Self = Self(CURRENT_MATERIAL_SCHEMA_VERSION);
}

impl Default for MaterialSchemaVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialProgramRef {
    BuiltIn(MaterialProgramId),
    Project(MaterialProgramId),
}

impl MaterialProgramRef {
    pub const fn id(self) -> MaterialProgramId {
        match self {
            Self::BuiltIn(id) | Self::Project(id) => id,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialDomain {
    Sprite,
    Mesh,
    Ribbon,
    Decal,
    Screen,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialDepthTest {
    Disabled,
    Less,
    LessEqual,
    Always,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialCullMode {
    None,
    Front,
    Back,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialRenderState {
    pub blend: BlendMode,
    pub depth_test: MaterialDepthTest,
    pub depth_write: bool,
    pub cull_mode: MaterialCullMode,
}

impl MaterialRenderState {
    pub const fn additive_sprite() -> Self {
        Self {
            blend: BlendMode::Additive,
            depth_test: MaterialDepthTest::LessEqual,
            depth_write: false,
            cull_mode: MaterialCullMode::None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialRenderStatePolicy {
    pub default: MaterialRenderState,
    pub allowed: Vec<MaterialRenderState>,
}

impl MaterialRenderStatePolicy {
    pub fn fixed(state: MaterialRenderState) -> Self {
        Self {
            default: state,
            allowed: vec![state],
        }
    }

    pub fn allows(&self, state: MaterialRenderState) -> bool {
        self.allowed.contains(&state)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialTextureColorSpace {
    SrgbColor,
    LinearData,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialFilterMode {
    Nearest,
    Linear,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialAddressMode {
    ClampToEdge,
    Repeat,
    MirrorRepeat,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialMipFilterMode {
    None,
    Nearest,
    Linear,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialSamplerDescriptor {
    pub filter: MaterialFilterMode,
    pub mip_filter: MaterialMipFilterMode,
    pub address_u: MaterialAddressMode,
    pub address_v: MaterialAddressMode,
}

impl Default for MaterialSamplerDescriptor {
    fn default() -> Self {
        Self {
            filter: MaterialFilterMode::Linear,
            mip_filter: MaterialMipFilterMode::Linear,
            address_u: MaterialAddressMode::ClampToEdge,
            address_v: MaterialAddressMode::ClampToEdge,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialTextureDescriptor {
    pub color_space: MaterialTextureColorSpace,
    pub sampler: MaterialSamplerDescriptor,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialValueType {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Texture2D(MaterialTextureDescriptor),
    Bool,
}

impl MaterialValueType {
    pub fn accepts(self, value: &MaterialValue) -> bool {
        matches!(
            (self, value),
            (Self::Float, MaterialValue::Float(_))
                | (Self::Vec2, MaterialValue::Vec2(_))
                | (Self::Vec3, MaterialValue::Vec3(_))
                | (Self::Vec4, MaterialValue::Vec4(_))
                | (Self::Color, MaterialValue::ColorSrgb(_))
                | (Self::Texture2D(_), MaterialValue::Texture2D(_))
                | (Self::Bool, MaterialValue::Bool(_))
        )
    }

    pub const fn is_numeric(self) -> bool {
        matches!(
            self,
            Self::Float | Self::Vec2 | Self::Vec3 | Self::Vec4 | Self::Color
        )
    }

    pub const fn accepts_effect_value(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (Self::Float, Value::Scalar(_))
                | (Self::Vec2, Value::Vec2(_))
                | (Self::Vec3, Value::Vec3(_))
                | (Self::Vec4 | Self::Color, Value::Vec4(_))
                | (Self::Texture2D(_), Value::Asset(_))
                | (Self::Bool, Value::Bool(_))
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MaterialValue {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// An editor/persistence sRGB literal. Compilation converts it to linear RGBA.
    ColorSrgb([f32; 4]),
    Texture2D(AssetId),
    Bool(bool),
}

impl MaterialValue {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::Vec2(value) => value.iter().all(|item| item.is_finite()),
            Self::Vec3(value) => value.iter().all(|item| item.is_finite()),
            Self::Vec4(value) | Self::ColorSrgb(value) => value.iter().all(|item| item.is_finite()),
            Self::Texture2D(asset) => !asset.is_nil(),
            Self::Bool(_) => true,
        }
    }

    pub fn has_same_type(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Float(_), Self::Float(_))
                | (Self::Vec2(_), Self::Vec2(_))
                | (Self::Vec3(_), Self::Vec3(_))
                | (Self::Vec4(_), Self::Vec4(_))
                | (Self::ColorSrgb(_), Self::ColorSrgb(_))
                | (Self::Texture2D(_), Self::Texture2D(_))
                | (Self::Bool(_), Self::Bool(_))
        )
    }

    pub const fn is_numeric(&self) -> bool {
        matches!(
            self,
            Self::Float(_) | Self::Vec2(_) | Self::Vec3(_) | Self::Vec4(_) | Self::ColorSrgb(_)
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialEvaluationDomain {
    ShaderStatic,
    Instance,
    Effect,
    Emitter,
}

/// The stage at which a material expression first requires evaluation.
///
/// Values may be promoted to a later domain, but a resource or other early-domain
/// socket cannot consume a value that is only available later.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MaterialExpressionDomain {
    ShaderStatic,
    Instance,
    Effect,
    Emitter,
    Particle,
    Vertex,
    Fragment,
}

impl From<MaterialEvaluationDomain> for MaterialExpressionDomain {
    fn from(value: MaterialEvaluationDomain) -> Self {
        match value {
            MaterialEvaluationDomain::ShaderStatic => Self::ShaderStatic,
            MaterialEvaluationDomain::Instance => Self::Instance,
            MaterialEvaluationDomain::Effect => Self::Effect,
            MaterialEvaluationDomain::Emitter => Self::Emitter,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialExpressionInfo {
    pub value_type: MaterialValueType,
    pub evaluation_domain: MaterialExpressionDomain,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialProgramAnalysis {
    pub expressions: BTreeMap<MaterialExpressionId, MaterialExpressionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialParameter {
    pub id: MaterialParameterId,
    pub name: String,
    pub value_type: MaterialValueType,
    pub evaluation_domain: MaterialEvaluationDomain,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<MaterialValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MaterialParameterValue {
    Constant(MaterialValue),
    EffectParameter(ParameterId),
    EmitterParameter(ParameterId),
    RandomRange {
        min: MaterialValue,
        max: MaterialValue,
        domain: MaterialEvaluationDomain,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialInstance {
    pub id: MaterialId,
    pub program: MaterialProgramRef,
    pub values: BTreeMap<MaterialParameterId, MaterialParameterValue>,
    pub render_state: MaterialRenderState,
}

impl MaterialInstance {
    pub fn validate_structure(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        if self.id.is_nil() {
            error(
                &mut report,
                DiagnosticCode::NilId,
                "material_instance.id",
                "material instance ID cannot be nil",
            );
        }
        if self.program.id().is_nil() {
            error(
                &mut report,
                DiagnosticCode::NilId,
                "material_instance.program",
                "material program reference cannot be nil",
            );
        }
        for (parameter, value) in &self.values {
            let path = format!("material_instance.values[{parameter}]");
            if parameter.is_nil() {
                error(
                    &mut report,
                    DiagnosticCode::NilId,
                    &path,
                    "material parameter reference cannot be nil",
                );
            }
            match value {
                MaterialParameterValue::Constant(value) => {
                    validate_instance_value(&mut report, &path, value);
                }
                MaterialParameterValue::EffectParameter(parameter)
                | MaterialParameterValue::EmitterParameter(parameter) => {
                    if parameter.is_nil() {
                        error(
                            &mut report,
                            DiagnosticCode::NilId,
                            &path,
                            "effect or emitter parameter reference cannot be nil",
                        );
                    }
                }
                MaterialParameterValue::RandomRange { min, max, domain } => {
                    if *domain == MaterialEvaluationDomain::ShaderStatic {
                        error(
                            &mut report,
                            DiagnosticCode::InvalidValue,
                            &path,
                            "random material values cannot use the shader-static domain",
                        );
                    }
                    if !min.has_same_type(max) {
                        error(
                            &mut report,
                            DiagnosticCode::ParameterTypeMismatch,
                            &path,
                            "random material range endpoints must have the same type",
                        );
                    }
                    if !min.is_numeric() || !max.is_numeric() {
                        error(
                            &mut report,
                            DiagnosticCode::InvalidValue,
                            &path,
                            "random material ranges require numeric endpoints",
                        );
                    }
                    validate_instance_value(&mut report, &path, min);
                    validate_instance_value(&mut report, &path, max);
                }
            }
        }
        report
    }

    pub fn validate(&self) -> Result<(), ValidationReport> {
        self.validate_structure().into_result()
    }

    pub fn validate_against(&self, program: &MaterialProgram) -> ValidationReport {
        let mut report = self.validate_structure();
        if self.program.id() != program.id {
            error(
                &mut report,
                DiagnosticCode::InvalidReference,
                "material_instance.program",
                format!(
                    "material instance references program {}, but was validated against {}",
                    self.program.id(),
                    program.id
                ),
            );
        }
        if !program.render_state_policy.allows(self.render_state) {
            error(
                &mut report,
                DiagnosticCode::InvalidValue,
                "material_instance.render_state",
                "material instance render state is not allowed by its program",
            );
        }

        for (parameter_id, value) in &self.values {
            let path = format!("material_instance.values[{parameter_id}]");
            let Some(parameter) = program
                .parameters
                .iter()
                .find(|parameter| parameter.id == *parameter_id)
            else {
                error(
                    &mut report,
                    DiagnosticCode::UnknownParameter,
                    path,
                    "material instance overrides an unknown program parameter",
                );
                continue;
            };
            validate_parameter_override(&mut report, &path, parameter, value);
        }

        for parameter in &program.parameters {
            if parameter.default.is_none() && !self.values.contains_key(&parameter.id) {
                error(
                    &mut report,
                    DiagnosticCode::InvalidReference,
                    format!("material_instance.values[{}]", parameter.id),
                    format!(
                        "material parameter '{}' has no default and requires an instance value",
                        parameter.name
                    ),
                );
            }
        }
        report
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialInput {
    Uv0,
    Uv1,
    LocalPosition,
    WorldPosition,
    Normal,
    Tangent,
    ViewDirection,
    ScreenUv,
    ParticleColor,
    ParticleOpacity,
    ParticleAge,
    ParticleNormalizedAge,
    ParticleLifetime,
    ParticleVelocity,
    ParticleSpeed,
    ParticleRandom,
    ParticleId,
    ParticleSize,
    ParticleRotation,
    EffectTime,
    EmitterTime,
    EffectNormalizedTime,
    EmitterNormalizedTime,
    SceneDepth,
    CameraPosition,
    CameraDirection,
    PixelDepth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MaterialExpressionKind {
    Constant(MaterialValue),
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    Add(MaterialExpressionId, MaterialExpressionId),
    Subtract(MaterialExpressionId, MaterialExpressionId),
    Multiply(MaterialExpressionId, MaterialExpressionId),
    Divide(MaterialExpressionId, MaterialExpressionId),
    Lerp {
        start: MaterialExpressionId,
        end: MaterialExpressionId,
        factor: MaterialExpressionId,
    },
    Clamp {
        value: MaterialExpressionId,
        min: MaterialExpressionId,
        max: MaterialExpressionId,
    },
    Remap {
        value: MaterialExpressionId,
        input_min: MaterialExpressionId,
        input_max: MaterialExpressionId,
        output_min: MaterialExpressionId,
        output_max: MaterialExpressionId,
    },
    Smoothstep {
        edge_min: MaterialExpressionId,
        edge_max: MaterialExpressionId,
        value: MaterialExpressionId,
    },
    RadialMask {
        uv: MaterialExpressionId,
        center: MaterialExpressionId,
        radius: MaterialExpressionId,
        softness: MaterialExpressionId,
        invert: MaterialExpressionId,
    },
    Dissolve {
        source: MaterialExpressionId,
        threshold: MaterialExpressionId,
        edge_width: MaterialExpressionId,
        invert: MaterialExpressionId,
    },
    DissolveEdge {
        source: MaterialExpressionId,
        threshold: MaterialExpressionId,
        edge_width: MaterialExpressionId,
        invert: MaterialExpressionId,
    },
    /// Fades a fragment as it approaches opaque scene geometry.
    ///
    /// Both depth inputs are linear view-space distances in the same units as
    /// `fade_distance`. A non-positive distance produces a deterministic hard
    /// intersection test.
    DepthFade {
        scene_depth: MaterialExpressionId,
        pixel_depth: MaterialExpressionId,
        fade_distance: MaterialExpressionId,
        invert: MaterialExpressionId,
    },
    PanUv {
        uv: MaterialExpressionId,
        speed: MaterialExpressionId,
        time: MaterialExpressionId,
    },
    RotateUv {
        uv: MaterialExpressionId,
        center: MaterialExpressionId,
        angle: MaterialExpressionId,
    },
    ScaleUv {
        uv: MaterialExpressionId,
        center: MaterialExpressionId,
        scale: MaterialExpressionId,
    },
    SampleTexture {
        texture: MaterialExpressionId,
        uv: MaterialExpressionId,
    },
    ExtractComponent {
        value: MaterialExpressionId,
        component: MaterialVectorComponent,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialVectorComponent {
    X,
    Y,
    Z,
    W,
}

impl MaterialExpressionKind {
    fn dependencies(&self) -> Vec<MaterialExpressionId> {
        match self {
            Self::Constant(_) | Self::Input(_) | Self::Parameter(_) => Vec::new(),
            Self::Add(left, right)
            | Self::Subtract(left, right)
            | Self::Multiply(left, right)
            | Self::Divide(left, right) => vec![*left, *right],
            Self::Lerp { start, end, factor } => vec![*start, *end, *factor],
            Self::Clamp { value, min, max } => vec![*value, *min, *max],
            Self::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            } => vec![*value, *input_min, *input_max, *output_min, *output_max],
            Self::Smoothstep {
                edge_min,
                edge_max,
                value,
            } => vec![*edge_min, *edge_max, *value],
            Self::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            } => vec![*uv, *center, *radius, *softness, *invert],
            Self::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            } => vec![*source, *threshold, *edge_width, *invert],
            Self::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            } => vec![*source, *threshold, *edge_width, *invert],
            Self::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => vec![*scene_depth, *pixel_depth, *fade_distance, *invert],
            Self::PanUv { uv, speed, time } => vec![*uv, *speed, *time],
            Self::RotateUv { uv, center, angle } => vec![*uv, *center, *angle],
            Self::ScaleUv { uv, center, scale } => vec![*uv, *center, *scale],
            Self::SampleTexture { texture, uv } => vec![*texture, *uv],
            Self::ExtractComponent { value, .. } => vec![*value],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialExpression {
    pub id: MaterialExpressionId,
    pub kind: MaterialExpressionKind,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialOutputs {
    pub color: MaterialExpressionId,
    pub alpha: MaterialExpressionId,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MaterialProgram {
    pub id: MaterialProgramId,
    pub schema_version: MaterialSchemaVersion,
    pub name: String,
    pub domain: MaterialDomain,
    pub render_state_policy: MaterialRenderStatePolicy,
    pub parameters: Vec<MaterialParameter>,
    pub expressions: Vec<MaterialExpression>,
    pub outputs: MaterialOutputs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialTextureSlot {
    pub parameter: MaterialParameterId,
    pub descriptor: MaterialTextureDescriptor,
    pub binding: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialSamplerSlot {
    pub descriptor: MaterialSamplerDescriptor,
    pub binding: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MaterialResourceLayout {
    pub textures: Vec<MaterialTextureSlot>,
    pub samplers: Vec<MaterialSamplerSlot>,
}

impl MaterialProgram {
    pub fn additive_sprite(name: impl Into<String>) -> Self {
        let color = MaterialExpressionId::new();
        let alpha = MaterialExpressionId::new();
        Self {
            id: MaterialProgramId::new(),
            schema_version: MaterialSchemaVersion::CURRENT,
            name: name.into(),
            domain: MaterialDomain::Sprite,
            render_state_policy: MaterialRenderStatePolicy::fixed(
                MaterialRenderState::additive_sprite(),
            ),
            parameters: Vec::new(),
            expressions: vec![
                MaterialExpression {
                    id: color,
                    kind: MaterialExpressionKind::Constant(MaterialValue::ColorSrgb([
                        1.0, 1.0, 1.0, 1.0,
                    ])),
                },
                MaterialExpression {
                    id: alpha,
                    kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
                },
            ],
            outputs: MaterialOutputs { color, alpha },
        }
    }

    pub fn normalized(&self) -> Self {
        let mut normalized = self.clone();
        normalized.parameters.sort_by_key(|parameter| parameter.id);
        normalized
            .expressions
            .sort_by_key(|expression| expression.id);
        normalized
            .render_state_policy
            .allowed
            .sort_by_key(|state| render_state_key(*state));
        normalized.render_state_policy.allowed.dedup();
        normalized
    }

    pub fn from_ron(source: &str) -> Result<Self, MaterialProgramError> {
        let program: Self = ron::from_str(source)?;
        program.validate()?;
        Ok(program.normalized())
    }

    pub fn load_ron(path: impl AsRef<Path>) -> Result<Self, MaterialProgramError> {
        Self::from_ron(&fs::read_to_string(path)?)
    }

    pub fn to_pretty_ron(&self) -> Result<String, MaterialProgramError> {
        self.validate()?;
        Ok(ron::ser::to_string_pretty(
            &self.normalized(),
            ron::ser::PrettyConfig::new().depth_limit(12),
        )?)
    }

    pub fn save_ron(&self, path: impl AsRef<Path>) -> Result<(), MaterialProgramError> {
        crate::model::atomic_write(path.as_ref(), self.to_pretty_ron()?.as_bytes())?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ValidationReport> {
        self.validation_report().into_result()
    }

    /// Returns deterministic structural and semantic diagnostics without
    /// requiring a renderer backend.
    pub fn validation_report(&self) -> ValidationReport {
        let mut report = self.validate_structure();
        self.analyze_semantics(&mut report);
        report
    }

    /// Infers the type and evaluation domain of every valid expression.
    pub fn analyze(&self) -> Result<MaterialProgramAnalysis, ValidationReport> {
        let mut report = self.validate_structure();
        let analysis = self.analyze_semantics(&mut report);
        if report.is_valid() {
            Ok(analysis)
        } else {
            Err(report)
        }
    }

    /// Performs GPU-independent identity and graph-structure checks.
    pub fn validate_structure(&self) -> ValidationReport {
        let mut report = ValidationReport::default();
        if self.id.is_nil() {
            error(
                &mut report,
                DiagnosticCode::NilId,
                "material_program.id",
                "material program ID cannot be nil",
            );
        }
        if self.schema_version != MaterialSchemaVersion::CURRENT {
            error(
                &mut report,
                DiagnosticCode::UnsupportedFormat,
                "material_program.schema_version",
                format!(
                    "material schema version {} is unsupported; expected {}",
                    self.schema_version.0, CURRENT_MATERIAL_SCHEMA_VERSION
                ),
            );
        }
        if self.name.trim().is_empty() {
            error(
                &mut report,
                DiagnosticCode::InvalidValue,
                "material_program.name",
                "material program name cannot be empty",
            );
        }
        if !self
            .render_state_policy
            .allows(self.render_state_policy.default)
        {
            error(
                &mut report,
                DiagnosticCode::InvalidValue,
                "material_program.render_state_policy.default",
                "default render state must be included in the allowed states",
            );
        }

        let mut parameter_ids = BTreeSet::new();
        for (index, parameter) in self.parameters.iter().enumerate() {
            let path = format!("material_program.parameters[{index}]");
            if parameter.id.is_nil() {
                error(
                    &mut report,
                    DiagnosticCode::NilId,
                    format!("{path}.id"),
                    "material parameter ID cannot be nil",
                );
            } else if !parameter_ids.insert(parameter.id) {
                error(
                    &mut report,
                    DiagnosticCode::DuplicateId,
                    format!("{path}.id"),
                    "material parameter ID must be unique",
                );
            }
            if parameter.name.trim().is_empty() {
                error(
                    &mut report,
                    DiagnosticCode::InvalidValue,
                    format!("{path}.name"),
                    "material parameter name cannot be empty",
                );
            }
            if let Some(default) = &parameter.default {
                if !parameter.value_type.accepts(default) {
                    error(
                        &mut report,
                        DiagnosticCode::ParameterTypeMismatch,
                        format!("{path}.default"),
                        "material parameter default does not match its declared type",
                    );
                } else if !default.is_valid() {
                    error(
                        &mut report,
                        DiagnosticCode::InvalidValue,
                        format!("{path}.default"),
                        "material parameter default must contain finite values and valid assets",
                    );
                }
            }
        }

        let mut expressions = BTreeMap::new();
        for (index, expression) in self.expressions.iter().enumerate() {
            let path = format!("material_program.expressions[{index}]");
            if expression.id.is_nil() {
                error(
                    &mut report,
                    DiagnosticCode::NilId,
                    format!("{path}.id"),
                    "material expression ID cannot be nil",
                );
            } else if expressions.insert(expression.id, expression).is_some() {
                error(
                    &mut report,
                    DiagnosticCode::DuplicateId,
                    format!("{path}.id"),
                    "material expression ID must be unique",
                );
            }
            if let MaterialExpressionKind::Constant(value) = &expression.kind
                && !value.is_valid()
            {
                error(
                    &mut report,
                    DiagnosticCode::InvalidValue,
                    format!("{path}.kind"),
                    "material constants must contain finite values and valid assets",
                );
            }
            if let MaterialExpressionKind::Parameter(parameter) = expression.kind
                && !parameter_ids.contains(&parameter)
            {
                error(
                    &mut report,
                    DiagnosticCode::UnknownParameter,
                    format!("{path}.kind"),
                    "material expression references an unknown parameter",
                );
            }
        }

        validate_output(
            &mut report,
            &expressions,
            self.outputs.color,
            "material_program.outputs.color",
        );
        validate_output(
            &mut report,
            &expressions,
            self.outputs.alpha,
            "material_program.outputs.alpha",
        );

        for (index, expression) in self.expressions.iter().enumerate() {
            for dependency in expression.kind.dependencies() {
                if !expressions.contains_key(&dependency) {
                    error(
                        &mut report,
                        DiagnosticCode::InvalidReference,
                        format!("material_program.expressions[{index}].kind"),
                        format!("material expression references missing expression {dependency}"),
                    );
                }
            }
        }

        let mut visit_state = BTreeMap::new();
        for expression in &self.expressions {
            detect_cycle(expression.id, &expressions, &mut visit_state, &mut report);
        }

        let mut reachable = BTreeSet::new();
        collect_reachable(self.outputs.color, &expressions, &mut reachable);
        collect_reachable(self.outputs.alpha, &expressions, &mut reachable);
        for (index, expression) in self.expressions.iter().enumerate() {
            if !reachable.contains(&expression.id) {
                report.push(Diagnostic {
                    severity: DiagnosticSeverity::Warning,
                    code: DiagnosticCode::UnreachableExpression,
                    path: format!("material_program.expressions[{index}]"),
                    message: "material expression is unreachable from the outputs".into(),
                });
            }
        }

        report
    }

    fn analyze_semantics(&self, report: &mut ValidationReport) -> MaterialProgramAnalysis {
        validate_material_domain(self, report);
        validate_render_state_policy(self, report);
        validate_parameter_domains(self, report);

        let parameters = self
            .parameters
            .iter()
            .map(|parameter| (parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let expressions = self
            .expressions
            .iter()
            .map(|expression| (expression.id, expression))
            .collect::<BTreeMap<_, _>>();
        let expression_indices = self
            .expressions
            .iter()
            .enumerate()
            .map(|(index, expression)| (expression.id, index))
            .collect::<BTreeMap<_, _>>();
        let mut analysis = MaterialProgramAnalysis::default();
        let mut visiting = BTreeSet::new();

        for expression in &self.expressions {
            infer_expression(
                self,
                expression.id,
                &parameters,
                &expressions,
                &expression_indices,
                &mut visiting,
                &mut analysis.expressions,
                report,
            );
        }

        validate_material_output_type(
            report,
            &analysis,
            self.outputs.color,
            "material_program.outputs.color",
            "Color",
            "Color, Vec3, or Vec4",
            |value_type| {
                matches!(
                    value_type,
                    MaterialValueType::Color | MaterialValueType::Vec3 | MaterialValueType::Vec4
                )
            },
        );
        validate_material_output_type(
            report,
            &analysis,
            self.outputs.alpha,
            "material_program.outputs.alpha",
            "Alpha",
            "Float",
            |value_type| value_type == MaterialValueType::Float,
        );

        analysis
    }
}

#[allow(clippy::too_many_arguments)]
fn infer_expression(
    program: &MaterialProgram,
    id: MaterialExpressionId,
    parameters: &BTreeMap<MaterialParameterId, &MaterialParameter>,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpression>,
    expression_indices: &BTreeMap<MaterialExpressionId, usize>,
    visiting: &mut BTreeSet<MaterialExpressionId>,
    inferred: &mut BTreeMap<MaterialExpressionId, MaterialExpressionInfo>,
    report: &mut ValidationReport,
) -> Option<MaterialExpressionInfo> {
    if let Some(info) = inferred.get(&id) {
        return Some(*info);
    }
    let expression = expressions.get(&id)?;
    if !visiting.insert(id) {
        return None;
    }
    let index = expression_indices.get(&id).copied().unwrap_or_default();
    let path = format!("material_program.expressions[{index}].kind");
    let mut dependency = |dependency: MaterialExpressionId| {
        infer_expression(
            program,
            dependency,
            parameters,
            expressions,
            expression_indices,
            visiting,
            inferred,
            report,
        )
    };

    let info = match &expression.kind {
        MaterialExpressionKind::Constant(value) => Some(MaterialExpressionInfo {
            value_type: material_value_type(value),
            evaluation_domain: MaterialExpressionDomain::ShaderStatic,
        }),
        MaterialExpressionKind::Input(input) => {
            if program.domain == MaterialDomain::Sprite && !sprite_domain_supports_input(*input) {
                error(
                    report,
                    DiagnosticCode::UnsupportedMaterialInput,
                    &path,
                    format!("material input {input:?} is not available in the Sprite domain"),
                );
            }
            Some(material_input_info(*input))
        }
        MaterialExpressionKind::Parameter(parameter) => {
            parameters
                .get(parameter)
                .map(|parameter| MaterialExpressionInfo {
                    value_type: parameter.value_type,
                    evaluation_domain: parameter.evaluation_domain.into(),
                })
        }
        MaterialExpressionKind::Add(left, right)
        | MaterialExpressionKind::Subtract(left, right) => {
            let left = dependency(*left);
            let right = dependency(*right);
            infer_matching_numeric_binary(report, &path, left, right)
        }
        MaterialExpressionKind::Multiply(left, right)
        | MaterialExpressionKind::Divide(left, right) => {
            let left = dependency(*left);
            let right = dependency(*right);
            infer_scaled_numeric_binary(report, &path, left, right)
        }
        MaterialExpressionKind::Lerp { start, end, factor } => {
            let start = dependency(*start);
            let end = dependency(*end);
            let factor = dependency(*factor);
            match (start, end, factor) {
                (Some(start), Some(end), Some(factor)) => {
                    let mut valid = true;
                    if start.value_type != end.value_type || !start.value_type.is_numeric() {
                        material_type_error(
                            report,
                            format!("{path}.start"),
                            format!(
                                "Lerp endpoints must have the same numeric type, received {:?} and {:?}",
                                start.value_type, end.value_type
                            ),
                        );
                        valid = false;
                    }
                    if factor.value_type != MaterialValueType::Float {
                        material_type_error(
                            report,
                            format!("{path}.factor"),
                            format!(
                                "Lerp factor expects Float but received {:?}",
                                factor.value_type
                            ),
                        );
                        valid = false;
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: start.value_type,
                        evaluation_domain: start
                            .evaluation_domain
                            .max(end.evaluation_domain)
                            .max(factor.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::Clamp { value, min, max } => {
            let value = dependency(*value);
            let min = dependency(*min);
            let max = dependency(*max);
            match (value, min, max) {
                (Some(value), Some(min), Some(max)) => {
                    if !value.value_type.is_numeric()
                        || value.value_type != min.value_type
                        || value.value_type != max.value_type
                    {
                        material_type_error(
                            report,
                            &path,
                            format!(
                                "Clamp value, minimum, and maximum must share one numeric type; received {:?}, {:?}, and {:?}",
                                value.value_type, min.value_type, max.value_type
                            ),
                        );
                        None
                    } else {
                        Some(MaterialExpressionInfo {
                            value_type: value.value_type,
                            evaluation_domain: value
                                .evaluation_domain
                                .max(min.evaluation_domain)
                                .max(max.evaluation_domain),
                        })
                    }
                }
                _ => None,
            }
        }
        MaterialExpressionKind::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => {
            let value = dependency(*value);
            let input_min = dependency(*input_min);
            let input_max = dependency(*input_max);
            let output_min = dependency(*output_min);
            let output_max = dependency(*output_max);
            match (value, input_min, input_max, output_min, output_max) {
                (
                    Some(value),
                    Some(input_min),
                    Some(input_max),
                    Some(output_min),
                    Some(output_max),
                ) => infer_promoted_numeric_inputs(
                    report,
                    &path,
                    "Remap",
                    [
                        ("value", value),
                        ("input_min", input_min),
                        ("input_max", input_max),
                        ("output_min", output_min),
                        ("output_max", output_max),
                    ],
                ),
                _ => None,
            }
        }
        MaterialExpressionKind::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => {
            let edge_min = dependency(*edge_min);
            let edge_max = dependency(*edge_max);
            let value = dependency(*value);
            match (edge_min, edge_max, value) {
                (Some(edge_min), Some(edge_max), Some(value)) => infer_promoted_numeric_inputs(
                    report,
                    &path,
                    "Smoothstep",
                    [
                        ("edge_min", edge_min),
                        ("edge_max", edge_max),
                        ("value", value),
                    ],
                ),
                _ => None,
            }
        }
        MaterialExpressionKind::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => {
            let uv = dependency(*uv);
            let center = dependency(*center);
            let radius = dependency(*radius);
            let softness = dependency(*softness);
            let invert = dependency(*invert);
            match (uv, center, radius, softness, invert) {
                (Some(uv), Some(center), Some(radius), Some(softness), Some(invert)) => {
                    let mut valid = true;
                    for (socket, info, expected) in [
                        ("uv", uv, MaterialValueType::Vec2),
                        ("center", center, MaterialValueType::Vec2),
                        ("radius", radius, MaterialValueType::Float),
                        ("softness", softness, MaterialValueType::Float),
                        ("invert", invert, MaterialValueType::Bool),
                    ] {
                        if info.value_type != expected {
                            material_type_error(
                                report,
                                format!("{path}.{socket}"),
                                format!(
                                    "RadialMask {socket} expects {expected:?} but received {:?}",
                                    info.value_type
                                ),
                            );
                            valid = false;
                        }
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Float,
                        evaluation_domain: uv
                            .evaluation_domain
                            .max(center.evaluation_domain)
                            .max(radius.evaluation_domain)
                            .max(softness.evaluation_domain)
                            .max(invert.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        } => {
            let source = dependency(*source);
            let threshold = dependency(*threshold);
            let edge_width = dependency(*edge_width);
            let invert = dependency(*invert);
            match (source, threshold, edge_width, invert) {
                (Some(source), Some(threshold), Some(edge_width), Some(invert)) => {
                    let mut valid = true;
                    for (socket, info, expected) in [
                        ("source", source, MaterialValueType::Float),
                        ("threshold", threshold, MaterialValueType::Float),
                        ("edge_width", edge_width, MaterialValueType::Float),
                        ("invert", invert, MaterialValueType::Bool),
                    ] {
                        if info.value_type != expected {
                            material_type_error(
                                report,
                                format!("{path}.{socket}"),
                                format!(
                                    "Dissolve {socket} expects {expected:?} but received {:?}",
                                    info.value_type
                                ),
                            );
                            valid = false;
                        }
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Float,
                        evaluation_domain: source
                            .evaluation_domain
                            .max(threshold.evaluation_domain)
                            .max(edge_width.evaluation_domain)
                            .max(invert.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => {
            let source = dependency(*source);
            let threshold = dependency(*threshold);
            let edge_width = dependency(*edge_width);
            let invert = dependency(*invert);
            match (source, threshold, edge_width, invert) {
                (Some(source), Some(threshold), Some(edge_width), Some(invert)) => {
                    let mut valid = true;
                    for (socket, info, expected) in [
                        ("source", source, MaterialValueType::Float),
                        ("threshold", threshold, MaterialValueType::Float),
                        ("edge_width", edge_width, MaterialValueType::Float),
                        ("invert", invert, MaterialValueType::Bool),
                    ] {
                        if info.value_type != expected {
                            material_type_error(
                                report,
                                format!("{path}.{socket}"),
                                format!(
                                    "DissolveEdge {socket} expects {expected:?} but received {:?}",
                                    info.value_type
                                ),
                            );
                            valid = false;
                        }
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Float,
                        evaluation_domain: source
                            .evaluation_domain
                            .max(threshold.evaluation_domain)
                            .max(edge_width.evaluation_domain)
                            .max(invert.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => {
            let scene_depth = dependency(*scene_depth);
            let pixel_depth = dependency(*pixel_depth);
            let fade_distance = dependency(*fade_distance);
            let invert = dependency(*invert);
            match (scene_depth, pixel_depth, fade_distance, invert) {
                (Some(scene_depth), Some(pixel_depth), Some(fade_distance), Some(invert)) => {
                    let mut valid = true;
                    for (socket, info, expected) in [
                        ("scene_depth", scene_depth, MaterialValueType::Float),
                        ("pixel_depth", pixel_depth, MaterialValueType::Float),
                        ("fade_distance", fade_distance, MaterialValueType::Float),
                        ("invert", invert, MaterialValueType::Bool),
                    ] {
                        if info.value_type != expected {
                            material_type_error(
                                report,
                                format!("{path}.{socket}"),
                                format!(
                                    "DepthFade {socket} expects {expected:?} but received {:?}",
                                    info.value_type
                                ),
                            );
                            valid = false;
                        }
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Float,
                        evaluation_domain: scene_depth
                            .evaluation_domain
                            .max(pixel_depth.evaluation_domain)
                            .max(fade_distance.evaluation_domain)
                            .max(invert.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::PanUv { uv, speed, time } => {
            let uv = dependency(*uv);
            let speed = dependency(*speed);
            let time = dependency(*time);
            match (uv, speed, time) {
                (Some(uv), Some(speed), Some(time)) => {
                    let mut valid = true;
                    if uv.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.uv"),
                            format!("PanUV UV expects Vec2 but received {:?}", uv.value_type),
                        );
                        valid = false;
                    }
                    if speed.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.speed"),
                            format!(
                                "PanUV speed expects Vec2 but received {:?}",
                                speed.value_type
                            ),
                        );
                        valid = false;
                    }
                    if time.value_type != MaterialValueType::Float {
                        material_type_error(
                            report,
                            format!("{path}.time"),
                            format!(
                                "PanUV time expects Float but received {:?}",
                                time.value_type
                            ),
                        );
                        valid = false;
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Vec2,
                        evaluation_domain: uv
                            .evaluation_domain
                            .max(speed.evaluation_domain)
                            .max(time.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::RotateUv { uv, center, angle } => {
            let uv = dependency(*uv);
            let center = dependency(*center);
            let angle = dependency(*angle);
            match (uv, center, angle) {
                (Some(uv), Some(center), Some(angle)) => {
                    let mut valid = true;
                    if uv.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.uv"),
                            format!("RotateUV UV expects Vec2 but received {:?}", uv.value_type),
                        );
                        valid = false;
                    }
                    if center.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.center"),
                            format!(
                                "RotateUV center expects Vec2 but received {:?}",
                                center.value_type
                            ),
                        );
                        valid = false;
                    }
                    if angle.value_type != MaterialValueType::Float {
                        material_type_error(
                            report,
                            format!("{path}.angle"),
                            format!(
                                "RotateUV angle expects Float radians but received {:?}",
                                angle.value_type
                            ),
                        );
                        valid = false;
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Vec2,
                        evaluation_domain: uv
                            .evaluation_domain
                            .max(center.evaluation_domain)
                            .max(angle.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::ScaleUv { uv, center, scale } => {
            let uv = dependency(*uv);
            let center = dependency(*center);
            let scale = dependency(*scale);
            match (uv, center, scale) {
                (Some(uv), Some(center), Some(scale)) => {
                    let mut valid = true;
                    if uv.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.uv"),
                            format!("ScaleUV UV expects Vec2 but received {:?}", uv.value_type),
                        );
                        valid = false;
                    }
                    if center.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.center"),
                            format!(
                                "ScaleUV center expects Vec2 but received {:?}",
                                center.value_type
                            ),
                        );
                        valid = false;
                    }
                    if scale.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.scale"),
                            format!(
                                "ScaleUV scale expects Vec2 but received {:?}",
                                scale.value_type
                            ),
                        );
                        valid = false;
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Vec2,
                        evaluation_domain: uv
                            .evaluation_domain
                            .max(center.evaluation_domain)
                            .max(scale.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::SampleTexture { texture, uv } => {
            let texture_id = *texture;
            let texture = dependency(texture_id);
            let uv = dependency(*uv);
            match (texture, uv) {
                (Some(texture), Some(uv)) => {
                    let mut valid = true;
                    if !matches!(texture.value_type, MaterialValueType::Texture2D(_)) {
                        material_type_error(
                            report,
                            format!("{path}.texture"),
                            format!(
                                "SampleTexture texture expects Texture2D but received {:?}",
                                texture.value_type
                            ),
                        );
                        valid = false;
                    }
                    if texture.evaluation_domain > MaterialExpressionDomain::Instance {
                        error(
                            report,
                            DiagnosticCode::EvaluationDomainMismatch,
                            format!("{path}.texture"),
                            "sampled texture resources must be available by the Instance domain",
                        );
                        valid = false;
                    }
                    if uv.value_type != MaterialValueType::Vec2 {
                        material_type_error(
                            report,
                            format!("{path}.uv"),
                            format!(
                                "SampleTexture UV expects Vec2 but received {:?}",
                                uv.value_type
                            ),
                        );
                        valid = false;
                    }
                    if !matches!(
                        expressions.get(&texture_id).map(|expression| &expression.kind),
                        Some(MaterialExpressionKind::Parameter(parameter))
                            if matches!(
                                parameters.get(parameter).map(|parameter| parameter.value_type),
                                Some(MaterialValueType::Texture2D(_))
                            )
                    ) {
                        error(
                            report,
                            DiagnosticCode::MissingResourceDeclaration,
                            format!("{path}.texture"),
                            "sampled textures must come from a declared Texture2D material parameter",
                        );
                        valid = false;
                    }
                    valid.then_some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Color,
                        evaluation_domain: MaterialExpressionDomain::Fragment
                            .max(texture.evaluation_domain)
                            .max(uv.evaluation_domain),
                    })
                }
                _ => None,
            }
        }
        MaterialExpressionKind::ExtractComponent { value, component } => {
            let value = dependency(*value);
            value.and_then(|value| {
                let component_count = match value.value_type {
                    MaterialValueType::Vec2 => 2,
                    MaterialValueType::Vec3 => 3,
                    MaterialValueType::Vec4 | MaterialValueType::Color => 4,
                    _ => 0,
                };
                let component_index = match component {
                    MaterialVectorComponent::X => 0,
                    MaterialVectorComponent::Y => 1,
                    MaterialVectorComponent::Z => 2,
                    MaterialVectorComponent::W => 3,
                };
                if component_index >= component_count {
                    material_type_error(
                        report,
                        &path,
                        format!(
                            "ExtractComponent {component:?} is unavailable on {:?}",
                            value.value_type
                        ),
                    );
                    None
                } else {
                    Some(MaterialExpressionInfo {
                        value_type: MaterialValueType::Float,
                        evaluation_domain: value.evaluation_domain,
                    })
                }
            })
        }
    };
    visiting.remove(&id);
    if let Some(info) = info {
        inferred.insert(id, info);
    }
    info
}

fn infer_matching_numeric_binary(
    report: &mut ValidationReport,
    path: &str,
    left: Option<MaterialExpressionInfo>,
    right: Option<MaterialExpressionInfo>,
) -> Option<MaterialExpressionInfo> {
    let (Some(left), Some(right)) = (left, right) else {
        return None;
    };
    if left.value_type != right.value_type || !left.value_type.is_numeric() {
        material_type_error(
            report,
            path,
            format!(
                "arithmetic inputs must have the same numeric type, received {:?} and {:?}",
                left.value_type, right.value_type
            ),
        );
        return None;
    }
    Some(MaterialExpressionInfo {
        value_type: left.value_type,
        evaluation_domain: left.evaluation_domain.max(right.evaluation_domain),
    })
}

fn infer_scaled_numeric_binary(
    report: &mut ValidationReport,
    path: &str,
    left: Option<MaterialExpressionInfo>,
    right: Option<MaterialExpressionInfo>,
) -> Option<MaterialExpressionInfo> {
    let (Some(left), Some(right)) = (left, right) else {
        return None;
    };
    let value_type = if left.value_type == right.value_type && left.value_type.is_numeric() {
        Some(left.value_type)
    } else if left.value_type == MaterialValueType::Float && right.value_type.is_numeric() {
        Some(right.value_type)
    } else if right.value_type == MaterialValueType::Float && left.value_type.is_numeric() {
        Some(left.value_type)
    } else {
        None
    };
    let Some(value_type) = value_type else {
        material_type_error(
            report,
            path,
            format!(
                "multiply/divide inputs must be matching numeric values or one Float scale, received {:?} and {:?}",
                left.value_type, right.value_type
            ),
        );
        return None;
    };
    Some(MaterialExpressionInfo {
        value_type,
        evaluation_domain: left.evaluation_domain.max(right.evaluation_domain),
    })
}

fn infer_promoted_numeric_inputs<const N: usize>(
    report: &mut ValidationReport,
    path: &str,
    operation: &str,
    inputs: [(&str, MaterialExpressionInfo); N],
) -> Option<MaterialExpressionInfo> {
    let value_type = inputs
        .iter()
        .map(|(_, info)| info.value_type)
        .find(|value_type| value_type.is_numeric() && *value_type != MaterialValueType::Float)
        .unwrap_or(MaterialValueType::Float);
    let mut valid = true;
    for (socket, info) in &inputs {
        if !info.value_type.is_numeric()
            || (info.value_type != MaterialValueType::Float && info.value_type != value_type)
        {
            material_type_error(
                report,
                format!("{path}.{socket}"),
                format!(
                    "{operation} {socket} expects Float or {value_type:?} but received {:?}",
                    info.value_type
                ),
            );
            valid = false;
        }
    }
    valid.then_some(MaterialExpressionInfo {
        value_type,
        evaluation_domain: inputs
            .iter()
            .map(|(_, info)| info.evaluation_domain)
            .max()
            .unwrap_or(MaterialExpressionDomain::ShaderStatic),
    })
}

fn validate_material_output_type(
    report: &mut ValidationReport,
    analysis: &MaterialProgramAnalysis,
    expression: MaterialExpressionId,
    path: &str,
    output: &str,
    expected: &str,
    accepts: impl FnOnce(MaterialValueType) -> bool,
) {
    let Some(info) = analysis.expressions.get(&expression) else {
        return;
    };
    if !accepts(info.value_type) {
        material_type_error(
            report,
            path,
            format!(
                "material output {output} expects {expected} but received {:?}",
                info.value_type
            ),
        );
    }
}

fn validate_material_domain(program: &MaterialProgram, report: &mut ValidationReport) {
    if program.domain != MaterialDomain::Sprite {
        error(
            report,
            DiagnosticCode::UnsupportedMaterialDomain,
            "material_program.domain",
            format!(
                "material domain {:?} is declared but not supported by the current material compiler",
                program.domain
            ),
        );
    }
}

fn validate_parameter_domains(program: &MaterialProgram, report: &mut ValidationReport) {
    for (index, parameter) in program.parameters.iter().enumerate() {
        let path = format!("material_program.parameters[{index}]");
        if parameter.evaluation_domain == MaterialEvaluationDomain::ShaderStatic
            && parameter.default.is_none()
        {
            error(
                report,
                DiagnosticCode::EvaluationDomainMismatch,
                format!("{path}.default"),
                "shader-static parameters require a program default",
            );
        }
        if matches!(parameter.value_type, MaterialValueType::Texture2D(_))
            && parameter.evaluation_domain != MaterialEvaluationDomain::Instance
        {
            error(
                report,
                DiagnosticCode::EvaluationDomainMismatch,
                format!("{path}.evaluation_domain"),
                "Texture2D resources must use the Instance evaluation domain",
            );
        }
    }
}

fn validate_render_state_policy(program: &MaterialProgram, report: &mut ValidationReport) {
    let mut states = BTreeSet::new();
    for (index, state) in program.render_state_policy.allowed.iter().enumerate() {
        let path = format!("material_program.render_state_policy.allowed[{index}]");
        if !states.insert(render_state_key(*state)) {
            error(
                report,
                DiagnosticCode::InvalidRenderState,
                &path,
                "allowed render states must not contain duplicates",
            );
        }
        if state.depth_test == MaterialDepthTest::Disabled && state.depth_write {
            error(
                report,
                DiagnosticCode::InvalidRenderState,
                &path,
                "depth writes require an enabled depth test",
            );
        }
        if state.blend == BlendMode::Additive && state.depth_write {
            error(
                report,
                DiagnosticCode::InvalidRenderState,
                &path,
                "additive material states cannot write depth",
            );
        }
        if program.domain == MaterialDomain::Sprite && state.cull_mode != MaterialCullMode::None {
            error(
                report,
                DiagnosticCode::InvalidRenderState,
                &path,
                "Sprite material states must disable face culling",
            );
        }
    }
}

fn material_value_type(value: &MaterialValue) -> MaterialValueType {
    match value {
        MaterialValue::Float(_) => MaterialValueType::Float,
        MaterialValue::Vec2(_) => MaterialValueType::Vec2,
        MaterialValue::Vec3(_) => MaterialValueType::Vec3,
        MaterialValue::Vec4(_) => MaterialValueType::Vec4,
        MaterialValue::ColorSrgb(_) => MaterialValueType::Color,
        MaterialValue::Texture2D(_) => MaterialValueType::Texture2D(MaterialTextureDescriptor {
            color_space: MaterialTextureColorSpace::SrgbColor,
            sampler: MaterialSamplerDescriptor::default(),
        }),
        MaterialValue::Bool(_) => MaterialValueType::Bool,
    }
}

fn material_input_info(input: MaterialInput) -> MaterialExpressionInfo {
    let (value_type, evaluation_domain) = match input {
        MaterialInput::Uv0 | MaterialInput::Uv1 | MaterialInput::ScreenUv => {
            (MaterialValueType::Vec2, MaterialExpressionDomain::Fragment)
        }
        MaterialInput::LocalPosition
        | MaterialInput::WorldPosition
        | MaterialInput::Normal
        | MaterialInput::Tangent
        | MaterialInput::ViewDirection
        | MaterialInput::CameraPosition
        | MaterialInput::CameraDirection => {
            (MaterialValueType::Vec3, MaterialExpressionDomain::Fragment)
        }
        MaterialInput::ParticleColor => {
            (MaterialValueType::Color, MaterialExpressionDomain::Particle)
        }
        MaterialInput::ParticleVelocity => {
            (MaterialValueType::Vec3, MaterialExpressionDomain::Particle)
        }
        MaterialInput::ParticleSize => {
            (MaterialValueType::Vec2, MaterialExpressionDomain::Particle)
        }
        MaterialInput::EffectTime | MaterialInput::EffectNormalizedTime => {
            (MaterialValueType::Float, MaterialExpressionDomain::Effect)
        }
        MaterialInput::EmitterTime | MaterialInput::EmitterNormalizedTime => {
            (MaterialValueType::Float, MaterialExpressionDomain::Emitter)
        }
        MaterialInput::ParticleOpacity
        | MaterialInput::ParticleAge
        | MaterialInput::ParticleNormalizedAge
        | MaterialInput::ParticleLifetime
        | MaterialInput::ParticleSpeed
        | MaterialInput::ParticleRandom
        | MaterialInput::ParticleId
        | MaterialInput::ParticleRotation => {
            (MaterialValueType::Float, MaterialExpressionDomain::Particle)
        }
        MaterialInput::SceneDepth | MaterialInput::PixelDepth => {
            (MaterialValueType::Float, MaterialExpressionDomain::Fragment)
        }
    };
    MaterialExpressionInfo {
        value_type,
        evaluation_domain,
    }
}

fn sprite_domain_supports_input(input: MaterialInput) -> bool {
    !matches!(
        input,
        MaterialInput::Uv1 | MaterialInput::Normal | MaterialInput::Tangent
    )
}

fn material_type_error(
    report: &mut ValidationReport,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    error(report, DiagnosticCode::MaterialTypeMismatch, path, message);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
}

fn validate_output(
    report: &mut ValidationReport,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpression>,
    output: MaterialExpressionId,
    path: &str,
) {
    if output.is_nil() || !expressions.contains_key(&output) {
        error(
            report,
            DiagnosticCode::InvalidReference,
            path,
            "material output must reference an existing expression",
        );
    }
}

fn detect_cycle(
    id: MaterialExpressionId,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpression>,
    states: &mut BTreeMap<MaterialExpressionId, VisitState>,
    report: &mut ValidationReport,
) {
    match states.get(&id) {
        Some(VisitState::Complete) => return,
        Some(VisitState::Visiting) => {
            error(
                report,
                DiagnosticCode::ReferenceCycle,
                "material_program.expressions",
                format!("material expression cycle reaches {id}"),
            );
            return;
        }
        None => {}
    }
    let Some(expression) = expressions.get(&id) else {
        return;
    };
    states.insert(id, VisitState::Visiting);
    for dependency in expression.kind.dependencies() {
        detect_cycle(dependency, expressions, states, report);
    }
    states.insert(id, VisitState::Complete);
}

fn collect_reachable(
    id: MaterialExpressionId,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpression>,
    reachable: &mut BTreeSet<MaterialExpressionId>,
) {
    if !reachable.insert(id) {
        return;
    }
    let Some(expression) = expressions.get(&id) else {
        return;
    };
    for dependency in expression.kind.dependencies() {
        collect_reachable(dependency, expressions, reachable);
    }
}

fn error(
    report: &mut ValidationReport,
    code: DiagnosticCode,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    report.push(Diagnostic::error(code, path, message));
}

fn validate_instance_value(report: &mut ValidationReport, path: &str, value: &MaterialValue) {
    if !value.is_valid() {
        error(
            report,
            DiagnosticCode::InvalidValue,
            path,
            "material instance value must contain finite values and valid assets",
        );
    }
}

fn validate_parameter_override(
    report: &mut ValidationReport,
    path: &str,
    parameter: &MaterialParameter,
    value: &MaterialParameterValue,
) {
    match value {
        MaterialParameterValue::Constant(value) => {
            if !parameter.value_type.accepts(value) {
                error(
                    report,
                    DiagnosticCode::ParameterTypeMismatch,
                    path,
                    format!(
                        "material parameter '{}' override does not match its declared type",
                        parameter.name
                    ),
                );
            }
        }
        MaterialParameterValue::EffectParameter(_) => {
            if parameter.evaluation_domain != MaterialEvaluationDomain::Effect {
                error(
                    report,
                    DiagnosticCode::InvalidValue,
                    path,
                    format!(
                        "material parameter '{}' does not allow effect-rate bindings",
                        parameter.name
                    ),
                );
            }
        }
        MaterialParameterValue::EmitterParameter(_) => {
            if parameter.evaluation_domain != MaterialEvaluationDomain::Emitter {
                error(
                    report,
                    DiagnosticCode::InvalidValue,
                    path,
                    format!(
                        "material parameter '{}' does not allow emitter-rate bindings",
                        parameter.name
                    ),
                );
            }
        }
        MaterialParameterValue::RandomRange { min, max, domain } => {
            if *domain != parameter.evaluation_domain {
                error(
                    report,
                    DiagnosticCode::InvalidValue,
                    path,
                    format!(
                        "material parameter '{}' random domain does not match its declaration",
                        parameter.name
                    ),
                );
            }
            if !parameter.value_type.accepts(min) || !parameter.value_type.accepts(max) {
                error(
                    report,
                    DiagnosticCode::ParameterTypeMismatch,
                    path,
                    format!(
                        "material parameter '{}' random range does not match its declared type",
                        parameter.name
                    ),
                );
            }
            if !parameter.value_type.is_numeric() {
                error(
                    report,
                    DiagnosticCode::InvalidValue,
                    path,
                    format!(
                        "material parameter '{}' does not support random ranges",
                        parameter.name
                    ),
                );
            }
        }
    }
}

fn render_state_key(state: MaterialRenderState) -> (u8, u8, bool, u8) {
    (
        state.blend as u8,
        state.depth_test as u8,
        state.depth_write,
        state.cull_mode as u8,
    )
}
