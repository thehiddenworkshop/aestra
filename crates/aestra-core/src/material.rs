//! Engine-independent semantic material-program model.
//!
//! This module is additive to the legacy sprite [`crate::MaterialDefinition`]
//! contract. The compiler migration can therefore proceed without changing the
//! current effect format or renderer behavior in the same step.

use crate::{
    AssetId, BlendMode, Diagnostic, DiagnosticCode, DiagnosticSeverity, MaterialExpressionId,
    MaterialId, MaterialParameterId, MaterialProgramId, ParameterId, ValidationReport,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
use thiserror::Error;

pub const CURRENT_MATERIAL_SCHEMA_VERSION: u32 = 1;

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
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MaterialEvaluationDomain {
    ShaderStatic,
    Instance,
    Effect,
    Emitter,
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
    SampleTexture {
        texture: MaterialExpressionId,
        uv: MaterialExpressionId,
    },
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
            Self::SampleTexture { texture, uv } => vec![*texture, *uv],
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
        self.validate_structure().into_result()
    }

    /// Performs GPU-independent structural checks. Full expression type and
    /// backend-capability validation belongs to later material milestones.
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
                    code: DiagnosticCode::InvalidReference,
                    path: format!("material_program.expressions[{index}]"),
                    message: "material expression is unreachable from the outputs".into(),
                });
            }
        }

        report
    }
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
