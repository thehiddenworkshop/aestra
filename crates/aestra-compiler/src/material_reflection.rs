//! Engine-neutral material metadata for generated authoring controls.

use crate::{MaterialCompileError, MaterialCompiler, MaterialIrInstruction, MaterialIrProgram};
use aestra_core::{
    MaterialId, MaterialParameterId, MaterialProgramId, ValidationReport,
    material::{
        MaterialDomain, MaterialEvaluationDomain, MaterialInput, MaterialInstance,
        MaterialParameterValue, MaterialProgram, MaterialRenderState, MaterialRenderStatePolicy,
        MaterialTextureDescriptor, MaterialValue, MaterialValueType,
    },
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialControlKind {
    Number,
    Vector2,
    Vector3,
    Vector4,
    Color,
    Texture,
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialControlSource {
    Constant,
    EffectParameter,
    EmitterParameter,
    RandomRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialControlValueOrigin {
    ProgramDefault,
    InstanceOverride,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialResourceConstraint {
    Texture2D(MaterialTextureDescriptor),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialControlDescriptor {
    pub id: MaterialParameterId,
    pub name: String,
    pub value_type: MaterialValueType,
    pub evaluation_domain: MaterialEvaluationDomain,
    pub control: MaterialControlKind,
    pub default_value: Option<MaterialValue>,
    /// The instance override, or a constant synthesized from the program default.
    pub current_value: Option<MaterialParameterValue>,
    pub value_origin: MaterialControlValueOrigin,
    pub supported_sources: Vec<MaterialControlSource>,
    pub resource_constraint: Option<MaterialResourceConstraint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialInputRequirements {
    pub vertex: Vec<MaterialInput>,
    pub particle: Vec<MaterialInput>,
    pub scene: Vec<MaterialInput>,
}

impl MaterialInputRequirements {
    pub fn all(&self) -> impl Iterator<Item = MaterialInput> + '_ {
        self.vertex
            .iter()
            .chain(&self.particle)
            .chain(&self.scene)
            .copied()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialControlCatalog {
    pub program: MaterialProgramId,
    pub material: Option<MaterialId>,
    pub name: String,
    pub domain: MaterialDomain,
    pub parameters: Vec<MaterialControlDescriptor>,
    pub required_inputs: MaterialInputRequirements,
    pub render_state_policy: MaterialRenderStatePolicy,
    pub current_render_state: MaterialRenderState,
}

#[derive(Debug, Error)]
pub enum MaterialControlReflectionError {
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error("material instance does not satisfy its program: {0}")]
    InvalidInstance(ValidationReport),
}

impl MaterialCompiler {
    /// Reflects one material program, optionally resolving an effect-local instance into current
    /// control values. The result is independent of renderer and graphics APIs.
    pub fn reflect_controls(
        &self,
        program: &MaterialProgram,
        instance: Option<&MaterialInstance>,
    ) -> Result<MaterialControlCatalog, MaterialControlReflectionError> {
        if let Some(instance) = instance {
            let report = instance.validate_against(program);
            if !report.is_valid() {
                return Err(MaterialControlReflectionError::InvalidInstance(report));
            }
        }
        let ir = self.compile(program)?;
        let normalized = program.normalized();
        let parameters = normalized
            .parameters
            .iter()
            .map(|parameter| {
                let authored = instance.and_then(|instance| instance.values.get(&parameter.id));
                let (current_value, value_origin) = match authored {
                    Some(value) => (
                        Some(value.clone()),
                        MaterialControlValueOrigin::InstanceOverride,
                    ),
                    None => match &parameter.default {
                        Some(value) => (
                            Some(MaterialParameterValue::Constant(value.clone())),
                            MaterialControlValueOrigin::ProgramDefault,
                        ),
                        None => (None, MaterialControlValueOrigin::Required),
                    },
                };
                MaterialControlDescriptor {
                    id: parameter.id,
                    name: parameter.name.clone(),
                    value_type: parameter.value_type,
                    evaluation_domain: parameter.evaluation_domain,
                    control: control_kind(parameter.value_type),
                    default_value: parameter.default.clone(),
                    current_value,
                    value_origin,
                    supported_sources: supported_sources(
                        parameter.value_type,
                        parameter.evaluation_domain,
                    ),
                    resource_constraint: resource_constraint(parameter.value_type),
                }
            })
            .collect();
        Ok(MaterialControlCatalog {
            program: normalized.id,
            material: instance.map(|instance| instance.id),
            name: normalized.name,
            domain: normalized.domain,
            parameters,
            required_inputs: reflect_material_inputs(&ir),
            render_state_policy: normalized.render_state_policy.clone(),
            current_render_state: instance
                .map_or(normalized.render_state_policy.default, |instance| {
                    instance.render_state
                }),
        })
    }
}

/// Collects the optimized program's live semantic inputs into stable authoring categories.
pub fn reflect_material_inputs(ir: &MaterialIrProgram) -> MaterialInputRequirements {
    let mut requirements = MaterialInputRequirements::default();
    for value in &ir.values {
        let MaterialIrInstruction::Input(input) = value.instruction else {
            continue;
        };
        let target = match input_category(input) {
            MaterialInputCategory::Vertex => &mut requirements.vertex,
            MaterialInputCategory::Particle => &mut requirements.particle,
            MaterialInputCategory::Scene => &mut requirements.scene,
        };
        if !target.contains(&input) {
            target.push(input);
        }
    }
    requirements.vertex.sort_by_key(|input| input_rank(*input));
    requirements
        .particle
        .sort_by_key(|input| input_rank(*input));
    requirements.scene.sort_by_key(|input| input_rank(*input));
    requirements
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MaterialInputCategory {
    Vertex,
    Particle,
    Scene,
}

fn input_category(input: MaterialInput) -> MaterialInputCategory {
    match input {
        MaterialInput::Uv0
        | MaterialInput::Uv1
        | MaterialInput::LocalPosition
        | MaterialInput::WorldPosition
        | MaterialInput::Normal
        | MaterialInput::Tangent
        | MaterialInput::ViewDirection
        | MaterialInput::ScreenUv => MaterialInputCategory::Vertex,
        MaterialInput::ParticleColor
        | MaterialInput::ParticleOpacity
        | MaterialInput::ParticleAge
        | MaterialInput::ParticleNormalizedAge
        | MaterialInput::ParticleLifetime
        | MaterialInput::ParticleVelocity
        | MaterialInput::ParticleSpeed
        | MaterialInput::ParticleRandom
        | MaterialInput::ParticleId
        | MaterialInput::ParticleSize
        | MaterialInput::ParticleRotation => MaterialInputCategory::Particle,
        MaterialInput::EffectTime
        | MaterialInput::EmitterTime
        | MaterialInput::EffectNormalizedTime
        | MaterialInput::EmitterNormalizedTime
        | MaterialInput::SceneDepth
        | MaterialInput::CameraPosition
        | MaterialInput::CameraDirection
        | MaterialInput::PixelDepth => MaterialInputCategory::Scene,
    }
}

fn control_kind(value_type: MaterialValueType) -> MaterialControlKind {
    match value_type {
        MaterialValueType::Float => MaterialControlKind::Number,
        MaterialValueType::Vec2 => MaterialControlKind::Vector2,
        MaterialValueType::Vec3 => MaterialControlKind::Vector3,
        MaterialValueType::Vec4 => MaterialControlKind::Vector4,
        MaterialValueType::Color => MaterialControlKind::Color,
        MaterialValueType::Texture2D(_) => MaterialControlKind::Texture,
        MaterialValueType::Bool => MaterialControlKind::Toggle,
    }
}

fn supported_sources(
    value_type: MaterialValueType,
    domain: MaterialEvaluationDomain,
) -> Vec<MaterialControlSource> {
    let mut sources = vec![MaterialControlSource::Constant];
    match domain {
        MaterialEvaluationDomain::Effect => sources.push(MaterialControlSource::EffectParameter),
        MaterialEvaluationDomain::Emitter => sources.push(MaterialControlSource::EmitterParameter),
        MaterialEvaluationDomain::ShaderStatic | MaterialEvaluationDomain::Instance => {}
    }
    if domain != MaterialEvaluationDomain::ShaderStatic && value_type.is_numeric() {
        sources.push(MaterialControlSource::RandomRange);
    }
    sources
}

fn resource_constraint(value_type: MaterialValueType) -> Option<MaterialResourceConstraint> {
    match value_type {
        MaterialValueType::Texture2D(descriptor) => {
            Some(MaterialResourceConstraint::Texture2D(descriptor))
        }
        MaterialValueType::Float
        | MaterialValueType::Vec2
        | MaterialValueType::Vec3
        | MaterialValueType::Vec4
        | MaterialValueType::Color
        | MaterialValueType::Bool => None,
    }
}

fn input_rank(input: MaterialInput) -> u8 {
    match input {
        MaterialInput::Uv0 => 0,
        MaterialInput::Uv1 => 1,
        MaterialInput::LocalPosition => 2,
        MaterialInput::WorldPosition => 3,
        MaterialInput::Normal => 4,
        MaterialInput::Tangent => 5,
        MaterialInput::ViewDirection => 6,
        MaterialInput::ScreenUv => 7,
        MaterialInput::ParticleColor => 8,
        MaterialInput::ParticleOpacity => 9,
        MaterialInput::ParticleAge => 10,
        MaterialInput::ParticleNormalizedAge => 11,
        MaterialInput::ParticleLifetime => 12,
        MaterialInput::ParticleVelocity => 13,
        MaterialInput::ParticleSpeed => 14,
        MaterialInput::ParticleRandom => 15,
        MaterialInput::ParticleId => 16,
        MaterialInput::ParticleSize => 17,
        MaterialInput::ParticleRotation => 18,
        MaterialInput::EffectTime => 19,
        MaterialInput::EmitterTime => 20,
        MaterialInput::EffectNormalizedTime => 21,
        MaterialInput::EmitterNormalizedTime => 22,
        MaterialInput::SceneDepth => 23,
        MaterialInput::CameraPosition => 24,
        MaterialInput::CameraDirection => 25,
        MaterialInput::PixelDepth => 26,
    }
}
