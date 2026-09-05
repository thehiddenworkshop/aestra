//! Versioned, engine-neutral persistence for compiled Aestra effects.
//!
//! The artifact schema deliberately mirrors semantic runtime data through explicit DTOs instead
//! of serializing [`aestra_runtime::CompiledEffect`] directly. This keeps the persisted contract
//! independent from Rust's in-memory representation and makes version changes reviewable.

use aestra_core::{
    AssetId, AssetKind, BlendMode, ChoreographyEventId, ChoreographyEventPayload, ColorKey, Curve,
    CurveKey, EffectAssetRef, EffectClipId, EffectClipSeed, EffectId, EffectPlaybackMode,
    EmitterId, EmitterRegionId, EmitterShape, EmitterTransform, FlipbookPlaybackMode,
    FlipbookTimeSource, Gradient, MaterialId, ModuleId, ParameterId, PropertyEvaluationDomain,
    RendererId, ScalarRange, UvRect, ValueType, Vec3Range,
    material::{MaterialInstance, MaterialProgram},
};
use aestra_runtime::{
    CompiledAsset, CompiledChoreographyEvent, CompiledCurve, CompiledEffect, CompiledEffectClip,
    CompiledEmitter, CompiledFlipbook, CompiledGradient, CompiledMaterial, CompiledParameter,
    CompiledParameterOverride, CompiledVec3Curve, EffectRequirements, ExecutionPlan, Expression,
    Instruction, IrLocation, MaterialColorPlan, OptimizationStats, ParameterSlot,
    ParticleAttribute, ParticleLayout, RendererCapability, RendererPlan, RendererPlanKind,
    RuntimeStage, RuntimeValue, ScalarSource, SimulationSeekMode, VectorSource,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

pub const ARTIFACT_MAGIC: &str = "AESTRA-COMPILED";
pub const CURRENT_ARTIFACT_VERSION: u32 = 2;

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("could not serialize the compiled artifact: {0}")]
    Serialize(#[from] ron::Error),
    #[error("compiled artifact is not valid UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("could not parse the compiled artifact: {0}")]
    Parse(#[from] ron::error::SpannedError),
    #[error("compiled artifact magic '{found}' is invalid; expected '{ARTIFACT_MAGIC}'")]
    InvalidMagic { found: String },
    #[error(
        "compiled artifact format version {found} is unsupported; expected \
         {CURRENT_ARTIFACT_VERSION}"
    )]
    UnsupportedVersion { found: u32 },
    #[error("compiled artifact field '{path}' is invalid: {message}")]
    InvalidData { path: String, message: String },
}

/// Encodes one immutable compiled effect into the versioned prototype artifact format.
pub fn encode_effect(effect: &CompiledEffect) -> Result<Vec<u8>, ArtifactError> {
    let envelope = ArtifactEnvelopeV1 {
        magic: ARTIFACT_MAGIC.to_owned(),
        format_version: CURRENT_ARTIFACT_VERSION,
        effect: EffectV1::try_from(effect)?,
    };
    Ok(ron::ser::to_string(&envelope)?.into_bytes())
}

/// Decodes one versioned prototype artifact into a runtime-ready compiled effect.
pub fn decode_effect(bytes: &[u8]) -> Result<CompiledEffect, ArtifactError> {
    let text = std::str::from_utf8(bytes)?;
    let envelope: ArtifactEnvelopeV1 = ron::de::from_str(text)?;
    if envelope.magic != ARTIFACT_MAGIC {
        return Err(ArtifactError::InvalidMagic {
            found: envelope.magic,
        });
    }
    if envelope.format_version != CURRENT_ARTIFACT_VERSION {
        return Err(ArtifactError::UnsupportedVersion {
            found: envelope.format_version,
        });
    }
    envelope.effect.try_into()
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtifactEnvelopeV1 {
    magic: String,
    format_version: u32,
    effect: EffectV1,
}

#[derive(Debug, Serialize, Deserialize)]
struct EffectV1 {
    source: EffectId,
    name: String,
    duration: f32,
    playback_mode: EffectPlaybackMode,
    seek_mode: SeekModeV1,
    assets: Vec<AssetV1>,
    flipbooks: Vec<FlipbookV1>,
    materials: Vec<MaterialV1>,
    material_programs: Vec<SemanticMaterialProgramV2>,
    material_instances: Vec<SemanticMaterialInstanceV2>,
    parameters: Vec<ParameterV1>,
    particle_layout: ParticleLayoutV1,
    emitters: Vec<EmitterV1>,
    effect_clips: Vec<EffectClipV1>,
    choreography_events: Vec<ChoreographyEventV1>,
    requirements: RequirementsV1,
    max_particles: u64,
    source_map: Vec<SourceMapEntryV1>,
    optimizations: OptimizationStatsV1,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum SeekModeV1 {
    StatelessDirect,
    CheckpointRestore,
    RestartReplay,
}

#[derive(Debug, Serialize, Deserialize)]
struct AssetV1 {
    source: AssetId,
    name: String,
    kind: AssetKind,
    path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FlipbookV1 {
    source: AssetId,
    name: String,
    texture: AssetId,
    frames: Vec<UvRect>,
    frame_rate: f32,
    looping: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct MaterialV1 {
    source: MaterialId,
    name: String,
    blend: BlendMode,
    softness: ExpressionV1<f32>,
    color: MaterialColorV1,
    texture: Option<AssetId>,
    uv: UvRect,
}

/// Semantic programs keep their own versioned RON schema inside the compiled artifact envelope.
/// This avoids duplicating the complete typed expression DAG in the artifact codec while keeping
/// it engine-neutral and independently migratable.
#[derive(Debug, Serialize, Deserialize)]
struct SemanticMaterialProgramV2 {
    ron: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct SemanticMaterialInstanceV2 {
    ron: String,
}

#[derive(Debug, Serialize, Deserialize)]
enum MaterialColorV1 {
    ParticleColor,
    Value(ExpressionV1<[f32; 4]>),
}

#[derive(Debug, Serialize, Deserialize)]
struct ParameterV1 {
    source: ParameterId,
    name: String,
    value_type: ValueType,
    default: RuntimeValueV1,
}

#[derive(Debug, Serialize, Deserialize)]
enum RuntimeValueV1 {
    Bool(bool),
    U32(u32),
    Scalar(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec3Range(Vec3Range),
    Vec3Curve(Vec3CurveV1),
    Vec4([f32; 4]),
    Text(String),
    Range(ScalarRange),
    Curve(CurveV1),
    Gradient(GradientV1),
    Shape(EmitterShape),
    Asset(AssetId),
    Material(MaterialId),
}

#[derive(Debug, Serialize, Deserialize)]
struct CurveV1 {
    keys: Vec<CurveKey>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Vec3CurveV1 {
    curves: [CurveV1; 3],
}

#[derive(Debug, Serialize, Deserialize)]
struct GradientV1 {
    keys: Vec<ColorKey>,
}

#[derive(Debug, Serialize, Deserialize)]
enum ExpressionV1<T> {
    Constant(T),
    Parameter(u32),
}

#[derive(Debug, Serialize, Deserialize)]
enum ScalarSourceV1 {
    Constant(ExpressionV1<f32>),
    RandomRange(ExpressionV1<ScalarRange>),
    Curve {
        value: ExpressionV1<CurveV1>,
        domain: PropertyEvaluationDomain,
    },
}

#[derive(Debug, Serialize, Deserialize)]
enum VectorSourceV1 {
    Constant(ExpressionV1<[f32; 3]>),
    RandomRange(ExpressionV1<Vec3Range>),
    Curve {
        value: ExpressionV1<Vec3CurveV1>,
        domain: PropertyEvaluationDomain,
    },
}

#[derive(Debug, Serialize, Deserialize)]
enum InstructionV1 {
    Emit {
        source: ModuleId,
        spawn_rate: ScalarSourceV1,
        burst_count: ExpressionV1<u32>,
    },
    SampleShape {
        source: ModuleId,
        shape: ExpressionV1<EmitterShape>,
    },
    Initialize {
        source: ModuleId,
        lifetime: ExpressionV1<ScalarRange>,
        speed: ExpressionV1<ScalarRange>,
        direction: ExpressionV1<[f32; 3]>,
        spread_degrees: ExpressionV1<f32>,
        angular_velocity: ExpressionV1<ScalarRange>,
    },
    Motion {
        source: ModuleId,
        gravity: VectorSourceV1,
        drag: ScalarSourceV1,
        turbulence: ScalarSourceV1,
    },
    Appearance {
        source: ModuleId,
        size: ExpressionV1<CurveV1>,
        opacity: ExpressionV1<CurveV1>,
        color: ExpressionV1<GradientV1>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ExecutionPlanV1 {
    emitter_update: Vec<InstructionV1>,
    particle_spawn: Vec<InstructionV1>,
    particle_update: Vec<InstructionV1>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RendererPlanV1 {
    source: RendererId,
    material: MaterialId,
    kind: RendererPlanKindV1,
}

#[derive(Debug, Serialize, Deserialize)]
enum RendererPlanKindV1 {
    Sprite,
    Mesh {
        asset: AssetId,
    },
    Flipbook {
        flipbook: AssetId,
        time_source: FlipbookTimeSource,
        playback: FlipbookPlaybackMode,
        random_start: bool,
    },
    Ribbon {
        width: f32,
    },
    Trail {
        width: f32,
        sample_interval: f32,
        lifetime: f32,
        max_points: u32,
        #[serde(default)]
        max_trails: u32,
        #[serde(default)]
        sampling: aestra_core::TrailSamplingMode,
        #[serde(default = "aestra_core::default_trail_sample_distance")]
        sample_distance: f32,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct EmitterV1 {
    source: EmitterId,
    region: EmitterRegionId,
    name: String,
    enabled: bool,
    transform: EmitterTransform,
    start_time: f32,
    source_offset: f32,
    source_duration: f32,
    duration: f32,
    seed_index: u32,
    max_particles: u32,
    execution: ExecutionPlanV1,
    renderers: Vec<RendererPlanV1>,
}

#[derive(Debug, Serialize, Deserialize)]
struct EffectClipV1 {
    source_clip: EffectClipId,
    source: EffectAssetRef,
    start_time: f32,
    source_offset: f32,
    duration: f32,
    transform: EmitterTransform,
    seed: EffectClipSeed,
    parameter_overrides: Vec<ParameterOverrideV1>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ParameterOverrideV1 {
    source: ParameterId,
    slot: u32,
    value: RuntimeValueV1,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChoreographyEventV1 {
    source: ChoreographyEventId,
    name: String,
    time: f32,
    payload: ChoreographyEventPayload,
}

#[derive(Debug, Serialize, Deserialize)]
struct ParticleLayoutV1 {
    attributes: Vec<ParticleAttributeV1>,
    transient_attributes: Vec<ParticleAttributeV1>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ParticleAttributeV1 {
    Position,
    Velocity,
    Age,
    Lifetime,
    NormalizedAge,
    Rotation,
    AngularVelocity,
    Size,
    Color,
}

#[derive(Debug, Serialize, Deserialize)]
struct RequirementsV1 {
    max_particles: u64,
    renderers: Vec<RendererCapabilityV1>,
    gpu_simulation: bool,
    native_gpu_presentation: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
// Names are part of the serialized artifact contract.
#[allow(clippy::enum_variant_names)]
enum RendererCapabilityV1 {
    MeshParticles,
    SpriteParticles,
    FlipbookParticles,
    RibbonParticles,
}

#[derive(Debug, Serialize, Deserialize)]
struct SourceMapEntryV1 {
    source: ModuleId,
    emitter_index: u32,
    stage: RuntimeStageV1,
    instruction_index: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum RuntimeStageV1 {
    EmitterUpdate,
    ParticleSpawn,
    ParticleUpdate,
}

#[derive(Debug, Serialize, Deserialize)]
struct OptimizationStatsV1 {
    constant_expressions: u64,
    runtime_parameter_reads: u64,
    eliminated_attributes: u64,
    #[serde(default)]
    material_common_subexpressions: u64,
    #[serde(default)]
    material_specialized_parameter_reads: u64,
    #[serde(default)]
    material_pruned_static_branches: u64,
    #[serde(default)]
    material_pruned_features: u64,
    #[serde(default)]
    material_texture_samples_authored: u64,
    #[serde(default)]
    material_texture_samples_eliminated: u64,
    #[serde(default)]
    material_texture_samples_live: u64,
    #[serde(default)]
    material_function_calls_authored: u64,
    #[serde(default)]
    material_function_calls_eliminated: u64,
    #[serde(default)]
    material_function_calls_live: u64,
}

impl TryFrom<&CompiledEffect> for EffectV1 {
    type Error = ArtifactError;

    fn try_from(effect: &CompiledEffect) -> Result<Self, Self::Error> {
        Ok(Self {
            source: effect.source,
            name: effect.name.clone(),
            duration: effect.duration,
            playback_mode: effect.playback_mode,
            seek_mode: effect.seek_mode.into(),
            assets: effect.assets.iter().map(AssetV1::from).collect(),
            flipbooks: effect.flipbooks.iter().map(FlipbookV1::from).collect(),
            materials: effect
                .materials
                .iter()
                .enumerate()
                .map(|(index, material)| MaterialV1::encode(material, index))
                .collect::<Result<_, _>>()?,
            material_programs: effect
                .material_programs
                .iter()
                .enumerate()
                .map(|(index, program)| SemanticMaterialProgramV2::encode(program, index))
                .collect::<Result<_, _>>()?,
            material_instances: effect
                .material_instances
                .iter()
                .enumerate()
                .map(|(index, instance)| SemanticMaterialInstanceV2::encode(instance, index))
                .collect::<Result<_, _>>()?,
            parameters: effect.parameters.iter().map(ParameterV1::from).collect(),
            particle_layout: ParticleLayoutV1::from(&effect.particle_layout),
            emitters: effect
                .emitters
                .iter()
                .enumerate()
                .map(|(index, emitter)| EmitterV1::encode(emitter, index))
                .collect::<Result<_, _>>()?,
            effect_clips: effect
                .effect_clips
                .iter()
                .enumerate()
                .map(|(index, clip)| EffectClipV1::encode(clip, index))
                .collect::<Result<_, _>>()?,
            choreography_events: effect
                .choreography_events
                .iter()
                .map(ChoreographyEventV1::from)
                .collect(),
            requirements: RequirementsV1::encode(&effect.requirements)?,
            max_particles: encode_u64(effect.max_particles, "effect.max_particles")?,
            source_map: effect
                .source_map
                .iter()
                .map(|(source, location)| SourceMapEntryV1::encode(*source, *location))
                .collect::<Result<_, _>>()?,
            optimizations: OptimizationStatsV1::encode(effect.optimizations)?,
        })
    }
}

impl TryFrom<EffectV1> for CompiledEffect {
    type Error = ArtifactError;

    fn try_from(effect: EffectV1) -> Result<Self, Self::Error> {
        require_finite_positive(effect.duration, "effect.duration")?;
        let parameters = effect
            .parameters
            .into_iter()
            .enumerate()
            .map(|(index, parameter)| parameter.decode(index))
            .collect::<Result<Vec<_>, _>>()?;
        let mut parameter_slots = BTreeMap::new();
        for (index, parameter) in parameters.iter().enumerate() {
            if parameter_slots
                .insert(parameter.source, ParameterSlot(index))
                .is_some()
            {
                return invalid(
                    format!("effect.parameters[{index}].source"),
                    "duplicate parameter identity",
                );
            }
        }

        let materials = effect
            .materials
            .into_iter()
            .enumerate()
            .map(|(index, material)| material.decode(index, &parameters))
            .collect::<Result<_, _>>()?;
        let material_programs = effect
            .material_programs
            .into_iter()
            .enumerate()
            .map(|(index, program)| program.decode(index))
            .collect::<Result<Vec<_>, _>>()?;
        let material_instances = effect
            .material_instances
            .into_iter()
            .enumerate()
            .map(|(index, instance)| instance.decode(index))
            .collect::<Result<Vec<_>, _>>()?;
        let mut program_ids = BTreeSet::new();
        for (index, program) in material_programs.iter().enumerate() {
            if !program_ids.insert(program.id) {
                return invalid(
                    format!("effect.material_programs[{index}].id"),
                    "duplicate semantic material program identity",
                );
            }
        }
        let mut instance_ids = BTreeSet::new();
        for (index, instance) in material_instances.iter().enumerate() {
            if !instance_ids.insert(instance.id) {
                return invalid(
                    format!("effect.material_instances[{index}].id"),
                    "duplicate semantic material instance identity",
                );
            }
            if !program_ids.contains(&instance.program.id()) {
                return invalid(
                    format!("effect.material_instances[{index}].program"),
                    "semantic material instance references a program absent from the artifact",
                );
            }
            let program = material_programs
                .iter()
                .find(|program| program.id == instance.program.id())
                .expect("program identity was checked above");
            let report = instance.validate_against(program);
            if !report.is_valid() {
                return invalid(
                    format!("effect.material_instances[{index}]"),
                    report.to_string(),
                );
            }
        }
        let emitters = effect
            .emitters
            .into_iter()
            .enumerate()
            .map(|(index, emitter)| emitter.decode(index, &parameters))
            .collect::<Result<_, _>>()?;
        let effect_clips = effect
            .effect_clips
            .into_iter()
            .enumerate()
            .map(|(index, clip)| clip.decode(index))
            .collect::<Result<_, _>>()?;
        let mut source_map = BTreeMap::new();
        for (index, entry) in effect.source_map.into_iter().enumerate() {
            let (source, location) = entry.decode(index)?;
            if source_map.insert(source, location).is_some() {
                return invalid(
                    format!("effect.source_map[{index}].source"),
                    "duplicate module identity",
                );
            }
        }

        Ok(Self {
            source: effect.source,
            name: effect.name,
            duration: effect.duration,
            playback_mode: effect.playback_mode,
            seek_mode: effect.seek_mode.into(),
            assets: effect.assets.into_iter().map(CompiledAsset::from).collect(),
            flipbooks: effect
                .flipbooks
                .into_iter()
                .enumerate()
                .map(|(index, flipbook)| flipbook.decode(index))
                .collect::<Result<_, _>>()?,
            materials,
            material_programs,
            material_instances,
            parameters,
            parameter_slots,
            particle_layout: effect.particle_layout.into(),
            emitters,
            effect_clips,
            choreography_events: effect
                .choreography_events
                .into_iter()
                .enumerate()
                .map(|(index, event)| event.decode(index))
                .collect::<Result<_, _>>()?,
            requirements: effect.requirements.decode()?,
            max_particles: decode_usize(effect.max_particles, "effect.max_particles")?,
            source_map,
            optimizations: effect.optimizations.decode()?,
        })
    }
}

impl From<SimulationSeekMode> for SeekModeV1 {
    fn from(value: SimulationSeekMode) -> Self {
        match value {
            SimulationSeekMode::StatelessDirect => Self::StatelessDirect,
            SimulationSeekMode::CheckpointRestore => Self::CheckpointRestore,
            SimulationSeekMode::RestartReplay => Self::RestartReplay,
        }
    }
}

impl From<SeekModeV1> for SimulationSeekMode {
    fn from(value: SeekModeV1) -> Self {
        match value {
            SeekModeV1::StatelessDirect => Self::StatelessDirect,
            SeekModeV1::CheckpointRestore => Self::CheckpointRestore,
            SeekModeV1::RestartReplay => Self::RestartReplay,
        }
    }
}

impl From<&CompiledAsset> for AssetV1 {
    fn from(asset: &CompiledAsset) -> Self {
        Self {
            source: asset.source,
            name: asset.name.clone(),
            kind: asset.kind,
            path: asset.path.clone(),
        }
    }
}

impl From<AssetV1> for CompiledAsset {
    fn from(asset: AssetV1) -> Self {
        Self {
            source: asset.source,
            name: asset.name,
            kind: asset.kind,
            path: asset.path,
        }
    }
}

impl From<&CompiledFlipbook> for FlipbookV1 {
    fn from(flipbook: &CompiledFlipbook) -> Self {
        Self {
            source: flipbook.source,
            name: flipbook.name.clone(),
            texture: flipbook.texture,
            frames: flipbook.frames.clone(),
            frame_rate: flipbook.frame_rate,
            looping: flipbook.looping,
        }
    }
}

impl FlipbookV1 {
    fn decode(self, index: usize) -> Result<CompiledFlipbook, ArtifactError> {
        require_finite_positive(
            self.frame_rate,
            format!("effect.flipbooks[{index}].frame_rate"),
        )?;
        Ok(CompiledFlipbook {
            source: self.source,
            name: self.name,
            texture: self.texture,
            frames: self.frames,
            frame_rate: self.frame_rate,
            looping: self.looping,
        })
    }
}

impl MaterialV1 {
    fn encode(material: &CompiledMaterial, index: usize) -> Result<Self, ArtifactError> {
        Ok(Self {
            source: material.source,
            name: material.name.clone(),
            blend: material.blend,
            softness: encode_expression(
                &material.softness,
                Clone::clone,
                format!("effect.materials[{index}].softness"),
            )?,
            color: match &material.color {
                MaterialColorPlan::ParticleColor => MaterialColorV1::ParticleColor,
                MaterialColorPlan::Value(value) => MaterialColorV1::Value(encode_expression(
                    value,
                    Clone::clone,
                    format!("effect.materials[{index}].color"),
                )?),
            },
            texture: material.texture,
            uv: material.uv,
        })
    }

    fn decode(
        self,
        index: usize,
        parameters: &[CompiledParameter],
    ) -> Result<CompiledMaterial, ArtifactError> {
        Ok(CompiledMaterial {
            source: self.source,
            name: self.name,
            blend: self.blend,
            softness: decode_expression(
                self.softness,
                Ok,
                parameters,
                ValueType::Scalar,
                format!("effect.materials[{index}].softness"),
            )?,
            color: match self.color {
                MaterialColorV1::ParticleColor => MaterialColorPlan::ParticleColor,
                MaterialColorV1::Value(value) => MaterialColorPlan::Value(decode_expression(
                    value,
                    Ok,
                    parameters,
                    ValueType::Vec4,
                    format!("effect.materials[{index}].color"),
                )?),
            },
            texture: self.texture,
            uv: self.uv,
        })
    }
}

impl SemanticMaterialProgramV2 {
    fn encode(program: &MaterialProgram, index: usize) -> Result<Self, ArtifactError> {
        let ron = program
            .to_pretty_ron()
            .map_err(|error| ArtifactError::InvalidData {
                path: format!("effect.material_programs[{index}]"),
                message: error.to_string(),
            })?;
        Ok(Self { ron })
    }

    fn decode(self, index: usize) -> Result<MaterialProgram, ArtifactError> {
        MaterialProgram::from_ron(&self.ron).map_err(|error| ArtifactError::InvalidData {
            path: format!("effect.material_programs[{index}]"),
            message: error.to_string(),
        })
    }
}

impl SemanticMaterialInstanceV2 {
    fn encode(instance: &MaterialInstance, index: usize) -> Result<Self, ArtifactError> {
        let report = instance.validate_structure();
        if !report.is_valid() {
            return invalid(
                format!("effect.material_instances[{index}]"),
                report.to_string(),
            );
        }
        let ron = ron::ser::to_string(instance)?;
        Ok(Self { ron })
    }

    fn decode(self, index: usize) -> Result<MaterialInstance, ArtifactError> {
        let instance = ron::de::from_str::<MaterialInstance>(&self.ron)?;
        let report = instance.validate_structure();
        if !report.is_valid() {
            return invalid(
                format!("effect.material_instances[{index}]"),
                report.to_string(),
            );
        }
        Ok(instance)
    }
}

impl From<&CompiledParameter> for ParameterV1 {
    fn from(parameter: &CompiledParameter) -> Self {
        Self {
            source: parameter.source,
            name: parameter.name.clone(),
            value_type: parameter.value_type,
            default: RuntimeValueV1::from(&parameter.default),
        }
    }
}

impl ParameterV1 {
    fn decode(self, index: usize) -> Result<CompiledParameter, ArtifactError> {
        let default = self
            .default
            .decode(format!("effect.parameters[{index}].default"))?;
        if default.value_type() != self.value_type {
            return invalid(
                format!("effect.parameters[{index}].default"),
                format!(
                    "declared {:?} but the default is {:?}",
                    self.value_type,
                    default.value_type()
                ),
            );
        }
        Ok(CompiledParameter {
            source: self.source,
            name: self.name,
            value_type: self.value_type,
            default,
        })
    }
}

impl From<&RuntimeValue> for RuntimeValueV1 {
    fn from(value: &RuntimeValue) -> Self {
        match value {
            RuntimeValue::Bool(value) => Self::Bool(*value),
            RuntimeValue::U32(value) => Self::U32(*value),
            RuntimeValue::Scalar(value) => Self::Scalar(*value),
            RuntimeValue::Vec2(value) => Self::Vec2(*value),
            RuntimeValue::Vec3(value) => Self::Vec3(*value),
            RuntimeValue::Vec3Range(value) => Self::Vec3Range(*value),
            RuntimeValue::Vec3Curve(value) => Self::Vec3Curve(Vec3CurveV1::from(value)),
            RuntimeValue::Vec4(value) => Self::Vec4(*value),
            RuntimeValue::Text(value) => Self::Text(value.clone()),
            RuntimeValue::Range(value) => Self::Range(*value),
            RuntimeValue::Curve(value) => Self::Curve(CurveV1::from(value)),
            RuntimeValue::Gradient(value) => Self::Gradient(GradientV1::from(value)),
            RuntimeValue::Shape(value) => Self::Shape(*value),
            RuntimeValue::Asset(value) => Self::Asset(*value),
            RuntimeValue::Material(value) => Self::Material(*value),
        }
    }
}

impl RuntimeValueV1 {
    fn decode(self, path: impl Into<String>) -> Result<RuntimeValue, ArtifactError> {
        let path = path.into();
        Ok(match self {
            Self::Bool(value) => RuntimeValue::Bool(value),
            Self::U32(value) => RuntimeValue::U32(value),
            Self::Scalar(value) => {
                require_finite(value, &path)?;
                RuntimeValue::Scalar(value)
            }
            Self::Vec2(value) => {
                require_finite_slice(&value, &path)?;
                RuntimeValue::Vec2(value)
            }
            Self::Vec3(value) => {
                require_finite_slice(&value, &path)?;
                RuntimeValue::Vec3(value)
            }
            Self::Vec3Range(value) => {
                require_finite_slice(&value.min, &path)?;
                require_finite_slice(&value.max, &path)?;
                RuntimeValue::Vec3Range(value)
            }
            Self::Vec3Curve(value) => RuntimeValue::Vec3Curve(value.decode(&path)?),
            Self::Vec4(value) => {
                require_finite_slice(&value, &path)?;
                RuntimeValue::Vec4(value)
            }
            Self::Text(value) => RuntimeValue::Text(value),
            Self::Range(value) => {
                require_finite_slice(&[value.min, value.max], &path)?;
                RuntimeValue::Range(value)
            }
            Self::Curve(value) => RuntimeValue::Curve(value.decode(&path)?),
            Self::Gradient(value) => RuntimeValue::Gradient(value.decode(&path)?),
            Self::Shape(value) => RuntimeValue::Shape(value),
            Self::Asset(value) => RuntimeValue::Asset(value),
            Self::Material(value) => RuntimeValue::Material(value),
        })
    }
}

impl From<&CompiledCurve> for CurveV1 {
    fn from(curve: &CompiledCurve) -> Self {
        let mut keys = Vec::new();
        if let Some((time, value)) = curve.first() {
            keys.push(CurveKey::new(time, value));
            keys.extend(
                curve
                    .segments()
                    .iter()
                    .map(|segment| CurveKey::new(segment.end_time, segment.end_value)),
            );
        }
        Self { keys }
    }
}

impl CurveV1 {
    fn decode(self, path: &str) -> Result<CompiledCurve, ArtifactError> {
        validate_curve_keys(&self.keys, path)?;
        Ok(CompiledCurve::compile(&Curve::new(self.keys)))
    }
}

impl From<&CompiledVec3Curve> for Vec3CurveV1 {
    fn from(curve: &CompiledVec3Curve) -> Self {
        Self {
            curves: std::array::from_fn(|axis| CurveV1::from(&curve.curves[axis])),
        }
    }
}

impl Vec3CurveV1 {
    fn decode(self, path: &str) -> Result<CompiledVec3Curve, ArtifactError> {
        let [x, y, z] = self.curves;
        Ok(CompiledVec3Curve {
            curves: [
                x.decode(&format!("{path}.x"))?,
                y.decode(&format!("{path}.y"))?,
                z.decode(&format!("{path}.z"))?,
            ],
        })
    }
}

impl From<&CompiledGradient> for GradientV1 {
    fn from(gradient: &CompiledGradient) -> Self {
        let mut keys = Vec::new();
        if let Some((time, color)) = gradient.first() {
            keys.push(ColorKey::new(time, color));
            keys.extend(
                gradient
                    .segments()
                    .iter()
                    .map(|segment| ColorKey::new(segment.end_time, segment.end_color)),
            );
        }
        Self { keys }
    }
}

impl GradientV1 {
    fn decode(self, path: &str) -> Result<CompiledGradient, ArtifactError> {
        validate_gradient_keys(&self.keys, path)?;
        Ok(CompiledGradient::compile(&Gradient::new(self.keys)))
    }
}

fn encode_expression<T, U>(
    expression: &Expression<T>,
    map: impl FnOnce(&T) -> U,
    path: impl Into<String>,
) -> Result<ExpressionV1<U>, ArtifactError> {
    Ok(match expression {
        Expression::Constant(value) => ExpressionV1::Constant(map(value)),
        Expression::Parameter(slot) => ExpressionV1::Parameter(encode_u32(slot.0, path.into())?),
    })
}

fn decode_expression<T, U>(
    expression: ExpressionV1<T>,
    map: impl FnOnce(T) -> Result<U, ArtifactError>,
    parameters: &[CompiledParameter],
    expected: ValueType,
    path: impl Into<String>,
) -> Result<Expression<U>, ArtifactError> {
    let path = path.into();
    Ok(match expression {
        ExpressionV1::Constant(value) => Expression::Constant(map(value)?),
        ExpressionV1::Parameter(slot) => {
            let slot = slot as usize;
            let Some(parameter) = parameters.get(slot) else {
                return invalid(path, format!("parameter slot {slot} is out of bounds"));
            };
            if parameter.value_type != expected {
                return invalid(
                    path,
                    format!(
                        "parameter slot {slot} has type {:?}; expected {expected:?}",
                        parameter.value_type
                    ),
                );
            }
            Expression::Parameter(ParameterSlot(slot))
        }
    })
}

impl ScalarSourceV1 {
    fn encode(source: &ScalarSource, path: &str) -> Result<Self, ArtifactError> {
        Ok(match source {
            ScalarSource::Constant(value) => Self::Constant(encode_expression(
                value,
                Clone::clone,
                format!("{path}.constant"),
            )?),
            ScalarSource::RandomRange(value) => Self::RandomRange(encode_expression(
                value,
                Clone::clone,
                format!("{path}.random_range"),
            )?),
            ScalarSource::Curve { value, domain } => Self::Curve {
                value: encode_expression(
                    value,
                    |curve| CurveV1::from(curve),
                    format!("{path}.curve.value"),
                )?,
                domain: *domain,
            },
        })
    }

    fn decode(
        self,
        parameters: &[CompiledParameter],
        path: &str,
    ) -> Result<ScalarSource, ArtifactError> {
        Ok(match self {
            Self::Constant(value) => ScalarSource::Constant(decode_expression(
                value,
                |value| {
                    require_finite(value, path)?;
                    Ok(value)
                },
                parameters,
                ValueType::Scalar,
                format!("{path}.constant"),
            )?),
            Self::RandomRange(value) => ScalarSource::RandomRange(decode_expression(
                value,
                |value| {
                    require_finite_slice(&[value.min, value.max], path)?;
                    Ok(value)
                },
                parameters,
                ValueType::Range,
                format!("{path}.random_range"),
            )?),
            Self::Curve { value, domain } => ScalarSource::Curve {
                value: decode_expression(
                    value,
                    |value| value.decode(&format!("{path}.curve.value")),
                    parameters,
                    ValueType::Curve,
                    format!("{path}.curve.value"),
                )?,
                domain,
            },
        })
    }
}

impl VectorSourceV1 {
    fn encode(source: &VectorSource, path: &str) -> Result<Self, ArtifactError> {
        Ok(match source {
            VectorSource::Constant(value) => Self::Constant(encode_expression(
                value,
                Clone::clone,
                format!("{path}.constant"),
            )?),
            VectorSource::RandomRange(value) => Self::RandomRange(encode_expression(
                value,
                Clone::clone,
                format!("{path}.random_range"),
            )?),
            VectorSource::Curve { value, domain } => Self::Curve {
                value: encode_expression(
                    value,
                    |curve| Vec3CurveV1::from(curve),
                    format!("{path}.curve.value"),
                )?,
                domain: *domain,
            },
        })
    }

    fn decode(
        self,
        parameters: &[CompiledParameter],
        path: &str,
    ) -> Result<VectorSource, ArtifactError> {
        Ok(match self {
            Self::Constant(value) => VectorSource::Constant(decode_expression(
                value,
                |value| {
                    require_finite_slice(&value, path)?;
                    Ok(value)
                },
                parameters,
                ValueType::Vec3,
                format!("{path}.constant"),
            )?),
            Self::RandomRange(value) => VectorSource::RandomRange(decode_expression(
                value,
                |value| {
                    require_finite_slice(&value.min, path)?;
                    require_finite_slice(&value.max, path)?;
                    Ok(value)
                },
                parameters,
                ValueType::Vec3Range,
                format!("{path}.random_range"),
            )?),
            Self::Curve { value, domain } => VectorSource::Curve {
                value: decode_expression(
                    value,
                    |value| value.decode(&format!("{path}.curve.value")),
                    parameters,
                    ValueType::Vec3Curve,
                    format!("{path}.curve.value"),
                )?,
                domain,
            },
        })
    }
}

impl InstructionV1 {
    fn encode(instruction: &Instruction, path: &str) -> Result<Self, ArtifactError> {
        Ok(match instruction {
            Instruction::Emit {
                source,
                spawn_rate,
                burst_count,
            } => Self::Emit {
                source: *source,
                spawn_rate: ScalarSourceV1::encode(spawn_rate, &format!("{path}.spawn_rate"))?,
                burst_count: encode_expression(
                    burst_count,
                    Clone::clone,
                    format!("{path}.burst_count"),
                )?,
            },
            Instruction::SampleShape { source, shape } => Self::SampleShape {
                source: *source,
                shape: encode_expression(shape, Clone::clone, format!("{path}.shape"))?,
            },
            Instruction::Initialize {
                source,
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            } => Self::Initialize {
                source: *source,
                lifetime: encode_expression(lifetime, Clone::clone, format!("{path}.lifetime"))?,
                speed: encode_expression(speed, Clone::clone, format!("{path}.speed"))?,
                direction: encode_expression(direction, Clone::clone, format!("{path}.direction"))?,
                spread_degrees: encode_expression(
                    spread_degrees,
                    Clone::clone,
                    format!("{path}.spread_degrees"),
                )?,
                angular_velocity: encode_expression(
                    angular_velocity,
                    Clone::clone,
                    format!("{path}.angular_velocity"),
                )?,
            },
            Instruction::Motion {
                source,
                gravity,
                drag,
                turbulence,
            } => Self::Motion {
                source: *source,
                gravity: VectorSourceV1::encode(gravity, &format!("{path}.gravity"))?,
                drag: ScalarSourceV1::encode(drag, &format!("{path}.drag"))?,
                turbulence: ScalarSourceV1::encode(turbulence, &format!("{path}.turbulence"))?,
            },
            Instruction::Appearance {
                source,
                size,
                opacity,
                color,
            } => Self::Appearance {
                source: *source,
                size: encode_expression(
                    size,
                    |curve| CurveV1::from(curve),
                    format!("{path}.size"),
                )?,
                opacity: encode_expression(
                    opacity,
                    |curve| CurveV1::from(curve),
                    format!("{path}.opacity"),
                )?,
                color: encode_expression(
                    color,
                    |gradient| GradientV1::from(gradient),
                    format!("{path}.color"),
                )?,
            },
        })
    }

    fn decode(
        self,
        parameters: &[CompiledParameter],
        path: &str,
    ) -> Result<Instruction, ArtifactError> {
        Ok(match self {
            Self::Emit {
                source,
                spawn_rate,
                burst_count,
            } => Instruction::Emit {
                source,
                spawn_rate: spawn_rate.decode(parameters, &format!("{path}.spawn_rate"))?,
                burst_count: decode_expression(
                    burst_count,
                    Ok,
                    parameters,
                    ValueType::U32,
                    format!("{path}.burst_count"),
                )?,
            },
            Self::SampleShape { source, shape } => Instruction::SampleShape {
                source,
                shape: decode_expression(
                    shape,
                    Ok,
                    parameters,
                    ValueType::Shape,
                    format!("{path}.shape"),
                )?,
            },
            Self::Initialize {
                source,
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            } => Instruction::Initialize {
                source,
                lifetime: decode_range_expression(
                    lifetime,
                    parameters,
                    &format!("{path}.lifetime"),
                )?,
                speed: decode_range_expression(speed, parameters, &format!("{path}.speed"))?,
                direction: decode_expression(
                    direction,
                    |value| {
                        require_finite_slice(&value, format!("{path}.direction"))?;
                        Ok(value)
                    },
                    parameters,
                    ValueType::Vec3,
                    format!("{path}.direction"),
                )?,
                spread_degrees: decode_expression(
                    spread_degrees,
                    |value| {
                        require_finite(value, format!("{path}.spread_degrees"))?;
                        Ok(value)
                    },
                    parameters,
                    ValueType::Scalar,
                    format!("{path}.spread_degrees"),
                )?,
                angular_velocity: decode_range_expression(
                    angular_velocity,
                    parameters,
                    &format!("{path}.angular_velocity"),
                )?,
            },
            Self::Motion {
                source,
                gravity,
                drag,
                turbulence,
            } => Instruction::Motion {
                source,
                gravity: gravity.decode(parameters, &format!("{path}.gravity"))?,
                drag: drag.decode(parameters, &format!("{path}.drag"))?,
                turbulence: turbulence.decode(parameters, &format!("{path}.turbulence"))?,
            },
            Self::Appearance {
                source,
                size,
                opacity,
                color,
            } => Instruction::Appearance {
                source,
                size: decode_expression(
                    size,
                    |value| value.decode(&format!("{path}.size")),
                    parameters,
                    ValueType::Curve,
                    format!("{path}.size"),
                )?,
                opacity: decode_expression(
                    opacity,
                    |value| value.decode(&format!("{path}.opacity")),
                    parameters,
                    ValueType::Curve,
                    format!("{path}.opacity"),
                )?,
                color: decode_expression(
                    color,
                    |value| value.decode(&format!("{path}.color")),
                    parameters,
                    ValueType::Gradient,
                    format!("{path}.color"),
                )?,
            },
        })
    }
}

fn decode_range_expression(
    expression: ExpressionV1<ScalarRange>,
    parameters: &[CompiledParameter],
    path: &str,
) -> Result<Expression<ScalarRange>, ArtifactError> {
    decode_expression(
        expression,
        |value| {
            require_finite_slice(&[value.min, value.max], path)?;
            Ok(value)
        },
        parameters,
        ValueType::Range,
        path,
    )
}

impl ExecutionPlanV1 {
    fn encode(plan: &ExecutionPlan, path: &str) -> Result<Self, ArtifactError> {
        Ok(Self {
            emitter_update: encode_instruction_stage(
                &plan.emitter_update,
                &format!("{path}.emitter_update"),
            )?,
            particle_spawn: encode_instruction_stage(
                &plan.particle_spawn,
                &format!("{path}.particle_spawn"),
            )?,
            particle_update: encode_instruction_stage(
                &plan.particle_update,
                &format!("{path}.particle_update"),
            )?,
        })
    }

    fn decode(
        self,
        parameters: &[CompiledParameter],
        path: &str,
    ) -> Result<ExecutionPlan, ArtifactError> {
        Ok(ExecutionPlan {
            emitter_update: decode_instruction_stage(
                self.emitter_update,
                parameters,
                &format!("{path}.emitter_update"),
            )?,
            particle_spawn: decode_instruction_stage(
                self.particle_spawn,
                parameters,
                &format!("{path}.particle_spawn"),
            )?,
            particle_update: decode_instruction_stage(
                self.particle_update,
                parameters,
                &format!("{path}.particle_update"),
            )?,
        })
    }
}

fn encode_instruction_stage(
    instructions: &[Instruction],
    path: &str,
) -> Result<Vec<InstructionV1>, ArtifactError> {
    instructions
        .iter()
        .enumerate()
        .map(|(index, instruction)| InstructionV1::encode(instruction, &format!("{path}[{index}]")))
        .collect()
}

fn decode_instruction_stage(
    instructions: Vec<InstructionV1>,
    parameters: &[CompiledParameter],
    path: &str,
) -> Result<Vec<Instruction>, ArtifactError> {
    instructions
        .into_iter()
        .enumerate()
        .map(|(index, instruction)| instruction.decode(parameters, &format!("{path}[{index}]")))
        .collect()
}

impl From<&RendererPlan> for RendererPlanV1 {
    fn from(renderer: &RendererPlan) -> Self {
        Self {
            source: renderer.source,
            material: renderer.material,
            kind: match renderer.kind {
                RendererPlanKind::Sprite => RendererPlanKindV1::Sprite,
                RendererPlanKind::Mesh { asset } => RendererPlanKindV1::Mesh { asset },
                RendererPlanKind::Ribbon { width } => RendererPlanKindV1::Ribbon { width },
                RendererPlanKind::Trail {
                    width,
                    sample_interval,
                    lifetime,
                    max_points,
                    max_trails,
                    sampling,
                    sample_distance,
                } => RendererPlanKindV1::Trail {
                    width,
                    sample_interval,
                    lifetime,
                    max_points,
                    max_trails,
                    sampling,
                    sample_distance,
                },
                RendererPlanKind::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => RendererPlanKindV1::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                },
            },
        }
    }
}

impl From<RendererPlanV1> for RendererPlan {
    fn from(renderer: RendererPlanV1) -> Self {
        Self {
            source: renderer.source,
            material: renderer.material,
            kind: match renderer.kind {
                RendererPlanKindV1::Sprite => RendererPlanKind::Sprite,
                RendererPlanKindV1::Mesh { asset } => RendererPlanKind::Mesh { asset },
                RendererPlanKindV1::Ribbon { width } => RendererPlanKind::Ribbon { width },
                RendererPlanKindV1::Trail {
                    width,
                    sample_interval,
                    lifetime,
                    max_points,
                    max_trails,
                    sampling,
                    sample_distance,
                } => RendererPlanKind::Trail {
                    width,
                    sample_interval,
                    lifetime,
                    max_points,
                    max_trails,
                    sampling,
                    sample_distance,
                },
                RendererPlanKindV1::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                } => RendererPlanKind::Flipbook {
                    flipbook,
                    time_source,
                    playback,
                    random_start,
                },
            },
        }
    }
}

impl EmitterV1 {
    fn encode(emitter: &CompiledEmitter, index: usize) -> Result<Self, ArtifactError> {
        Ok(Self {
            source: emitter.source,
            region: emitter.region,
            name: emitter.name.clone(),
            enabled: emitter.enabled,
            transform: emitter.transform,
            start_time: emitter.start_time,
            source_offset: emitter.source_offset,
            source_duration: emitter.source_duration,
            duration: emitter.duration,
            seed_index: emitter.seed_index,
            max_particles: emitter.max_particles,
            execution: ExecutionPlanV1::encode(
                &emitter.execution,
                &format!("effect.emitters[{index}].execution"),
            )?,
            renderers: emitter.renderers.iter().map(RendererPlanV1::from).collect(),
        })
    }

    fn decode(
        self,
        index: usize,
        parameters: &[CompiledParameter],
    ) -> Result<CompiledEmitter, ArtifactError> {
        let path = format!("effect.emitters[{index}]");
        require_finite_non_negative(self.start_time, format!("{path}.start_time"))?;
        require_finite_non_negative(self.source_offset, format!("{path}.source_offset"))?;
        require_finite_positive(self.source_duration, format!("{path}.source_duration"))?;
        require_finite_positive(self.duration, format!("{path}.duration"))?;
        if self.max_particles == 0 {
            return invalid(format!("{path}.max_particles"), "must be greater than zero");
        }
        Ok(CompiledEmitter {
            source: self.source,
            region: self.region,
            name: self.name,
            enabled: self.enabled,
            transform: self.transform,
            start_time: self.start_time,
            source_offset: self.source_offset,
            source_duration: self.source_duration,
            duration: self.duration,
            seed_index: self.seed_index,
            max_particles: self.max_particles,
            execution: self
                .execution
                .decode(parameters, &format!("{path}.execution"))?,
            renderers: self.renderers.into_iter().map(RendererPlan::from).collect(),
        })
    }
}

impl EffectClipV1 {
    fn encode(clip: &CompiledEffectClip, index: usize) -> Result<Self, ArtifactError> {
        Ok(Self {
            source_clip: clip.source_clip,
            source: clip.source,
            start_time: clip.start_time,
            source_offset: clip.source_offset,
            duration: clip.duration,
            transform: clip.transform,
            seed: clip.seed,
            parameter_overrides: clip
                .parameter_overrides
                .iter()
                .enumerate()
                .map(|(override_index, parameter)| {
                    ParameterOverrideV1::encode(parameter, index, override_index)
                })
                .collect::<Result<_, _>>()?,
        })
    }

    fn decode(self, index: usize) -> Result<CompiledEffectClip, ArtifactError> {
        let path = format!("effect.effect_clips[{index}]");
        require_finite_non_negative(self.start_time, format!("{path}.start_time"))?;
        require_finite_non_negative(self.source_offset, format!("{path}.source_offset"))?;
        require_finite_positive(self.duration, format!("{path}.duration"))?;
        Ok(CompiledEffectClip {
            source_clip: self.source_clip,
            source: self.source,
            start_time: self.start_time,
            source_offset: self.source_offset,
            duration: self.duration,
            transform: self.transform,
            seed: self.seed,
            parameter_overrides: self
                .parameter_overrides
                .into_iter()
                .enumerate()
                .map(|(override_index, parameter)| parameter.decode(index, override_index))
                .collect::<Result<_, _>>()?,
        })
    }
}

impl ParameterOverrideV1 {
    fn encode(
        parameter: &CompiledParameterOverride,
        clip_index: usize,
        override_index: usize,
    ) -> Result<Self, ArtifactError> {
        Ok(Self {
            source: parameter.source,
            slot: encode_u32(
                parameter.slot.0,
                format!(
                    "effect.effect_clips[{clip_index}].parameter_overrides[{override_index}].slot"
                ),
            )?,
            value: RuntimeValueV1::from(&parameter.value),
        })
    }

    fn decode(
        self,
        clip_index: usize,
        override_index: usize,
    ) -> Result<CompiledParameterOverride, ArtifactError> {
        let path =
            format!("effect.effect_clips[{clip_index}].parameter_overrides[{override_index}]");
        Ok(CompiledParameterOverride {
            source: self.source,
            slot: ParameterSlot(self.slot as usize),
            value: self.value.decode(format!("{path}.value"))?,
        })
    }
}

impl From<&CompiledChoreographyEvent> for ChoreographyEventV1 {
    fn from(event: &CompiledChoreographyEvent) -> Self {
        Self {
            source: event.source,
            name: event.name.clone(),
            time: event.time,
            payload: event.payload.clone(),
        }
    }
}

impl ChoreographyEventV1 {
    fn decode(self, index: usize) -> Result<CompiledChoreographyEvent, ArtifactError> {
        require_finite_non_negative(
            self.time,
            format!("effect.choreography_events[{index}].time"),
        )?;
        Ok(CompiledChoreographyEvent {
            source: self.source,
            name: self.name,
            time: self.time,
            payload: self.payload,
        })
    }
}

impl From<&ParticleLayout> for ParticleLayoutV1 {
    fn from(layout: &ParticleLayout) -> Self {
        Self {
            attributes: layout
                .attributes
                .iter()
                .copied()
                .map(ParticleAttributeV1::from)
                .collect(),
            transient_attributes: layout
                .transient_attributes
                .iter()
                .copied()
                .map(ParticleAttributeV1::from)
                .collect(),
        }
    }
}

impl From<ParticleLayoutV1> for ParticleLayout {
    fn from(layout: ParticleLayoutV1) -> Self {
        Self {
            attributes: layout
                .attributes
                .into_iter()
                .map(ParticleAttribute::from)
                .collect(),
            transient_attributes: layout
                .transient_attributes
                .into_iter()
                .map(ParticleAttribute::from)
                .collect(),
        }
    }
}

impl From<ParticleAttribute> for ParticleAttributeV1 {
    fn from(attribute: ParticleAttribute) -> Self {
        match attribute {
            ParticleAttribute::Position => Self::Position,
            ParticleAttribute::Velocity => Self::Velocity,
            ParticleAttribute::Age => Self::Age,
            ParticleAttribute::Lifetime => Self::Lifetime,
            ParticleAttribute::NormalizedAge => Self::NormalizedAge,
            ParticleAttribute::Rotation => Self::Rotation,
            ParticleAttribute::AngularVelocity => Self::AngularVelocity,
            ParticleAttribute::Size => Self::Size,
            ParticleAttribute::Color => Self::Color,
        }
    }
}

impl From<ParticleAttributeV1> for ParticleAttribute {
    fn from(attribute: ParticleAttributeV1) -> Self {
        match attribute {
            ParticleAttributeV1::Position => Self::Position,
            ParticleAttributeV1::Velocity => Self::Velocity,
            ParticleAttributeV1::Age => Self::Age,
            ParticleAttributeV1::Lifetime => Self::Lifetime,
            ParticleAttributeV1::NormalizedAge => Self::NormalizedAge,
            ParticleAttributeV1::Rotation => Self::Rotation,
            ParticleAttributeV1::AngularVelocity => Self::AngularVelocity,
            ParticleAttributeV1::Size => Self::Size,
            ParticleAttributeV1::Color => Self::Color,
        }
    }
}

impl RequirementsV1 {
    fn encode(requirements: &EffectRequirements) -> Result<Self, ArtifactError> {
        Ok(Self {
            max_particles: encode_u64(
                requirements.max_particles,
                "effect.requirements.max_particles",
            )?,
            renderers: requirements
                .renderers
                .iter()
                .copied()
                .map(RendererCapabilityV1::from)
                .collect(),
            gpu_simulation: requirements.gpu_simulation,
            native_gpu_presentation: requirements.native_gpu_presentation,
        })
    }

    fn decode(self) -> Result<EffectRequirements, ArtifactError> {
        Ok(EffectRequirements {
            max_particles: decode_usize(self.max_particles, "effect.requirements.max_particles")?,
            renderers: self
                .renderers
                .into_iter()
                .map(RendererCapability::from)
                .collect::<BTreeSet<_>>(),
            gpu_simulation: self.gpu_simulation,
            native_gpu_presentation: self.native_gpu_presentation,
        })
    }
}

impl From<RendererCapability> for RendererCapabilityV1 {
    fn from(capability: RendererCapability) -> Self {
        match capability {
            RendererCapability::MeshParticles => Self::MeshParticles,
            RendererCapability::RibbonParticles => Self::RibbonParticles,
            RendererCapability::SpriteParticles => Self::SpriteParticles,
            RendererCapability::FlipbookParticles => Self::FlipbookParticles,
        }
    }
}

impl From<RendererCapabilityV1> for RendererCapability {
    fn from(capability: RendererCapabilityV1) -> Self {
        match capability {
            RendererCapabilityV1::MeshParticles => Self::MeshParticles,
            RendererCapabilityV1::RibbonParticles => Self::RibbonParticles,
            RendererCapabilityV1::SpriteParticles => Self::SpriteParticles,
            RendererCapabilityV1::FlipbookParticles => Self::FlipbookParticles,
        }
    }
}

impl SourceMapEntryV1 {
    fn encode(source: ModuleId, location: IrLocation) -> Result<Self, ArtifactError> {
        Ok(Self {
            source,
            emitter_index: encode_u32(
                location.emitter_index,
                format!("effect.source_map[{source}].emitter_index"),
            )?,
            stage: location.stage.into(),
            instruction_index: encode_u32(
                location.instruction_index,
                format!("effect.source_map[{source}].instruction_index"),
            )?,
        })
    }

    fn decode(self, _index: usize) -> Result<(ModuleId, IrLocation), ArtifactError> {
        Ok((
            self.source,
            IrLocation {
                emitter_index: self.emitter_index as usize,
                stage: self.stage.into(),
                instruction_index: self.instruction_index as usize,
            },
        ))
    }
}

impl From<RuntimeStage> for RuntimeStageV1 {
    fn from(stage: RuntimeStage) -> Self {
        match stage {
            RuntimeStage::EmitterUpdate => Self::EmitterUpdate,
            RuntimeStage::ParticleSpawn => Self::ParticleSpawn,
            RuntimeStage::ParticleUpdate => Self::ParticleUpdate,
        }
    }
}

impl From<RuntimeStageV1> for RuntimeStage {
    fn from(stage: RuntimeStageV1) -> Self {
        match stage {
            RuntimeStageV1::EmitterUpdate => Self::EmitterUpdate,
            RuntimeStageV1::ParticleSpawn => Self::ParticleSpawn,
            RuntimeStageV1::ParticleUpdate => Self::ParticleUpdate,
        }
    }
}

impl OptimizationStatsV1 {
    fn encode(stats: OptimizationStats) -> Result<Self, ArtifactError> {
        Ok(Self {
            material_function_calls_authored: encode_u64(
                stats.material_function_calls_authored,
                "effect.optimizations.material_function_calls_authored",
            )?,
            material_function_calls_eliminated: encode_u64(
                stats.material_function_calls_eliminated,
                "effect.optimizations.material_function_calls_eliminated",
            )?,
            material_function_calls_live: encode_u64(
                stats.material_function_calls_live,
                "effect.optimizations.material_function_calls_live",
            )?,
            constant_expressions: encode_u64(
                stats.constant_expressions,
                "effect.optimizations.constant_expressions",
            )?,
            runtime_parameter_reads: encode_u64(
                stats.runtime_parameter_reads,
                "effect.optimizations.runtime_parameter_reads",
            )?,
            eliminated_attributes: encode_u64(
                stats.eliminated_attributes,
                "effect.optimizations.eliminated_attributes",
            )?,
            material_common_subexpressions: encode_u64(
                stats.material_common_subexpressions,
                "effect.optimizations.material_common_subexpressions",
            )?,
            material_specialized_parameter_reads: encode_u64(
                stats.material_specialized_parameter_reads,
                "effect.optimizations.material_specialized_parameter_reads",
            )?,
            material_pruned_static_branches: encode_u64(
                stats.material_pruned_static_branches,
                "effect.optimizations.material_pruned_static_branches",
            )?,
            material_pruned_features: encode_u64(
                stats.material_pruned_features,
                "effect.optimizations.material_pruned_features",
            )?,
            material_texture_samples_authored: encode_u64(
                stats.material_texture_samples_authored,
                "effect.optimizations.material_texture_samples_authored",
            )?,
            material_texture_samples_eliminated: encode_u64(
                stats.material_texture_samples_eliminated,
                "effect.optimizations.material_texture_samples_eliminated",
            )?,
            material_texture_samples_live: encode_u64(
                stats.material_texture_samples_live,
                "effect.optimizations.material_texture_samples_live",
            )?,
        })
    }

    fn decode(self) -> Result<OptimizationStats, ArtifactError> {
        Ok(OptimizationStats {
            material_function_calls_authored: decode_usize(
                self.material_function_calls_authored,
                "effect.optimizations.material_function_calls_authored",
            )?,
            material_function_calls_eliminated: decode_usize(
                self.material_function_calls_eliminated,
                "effect.optimizations.material_function_calls_eliminated",
            )?,
            material_function_calls_live: decode_usize(
                self.material_function_calls_live,
                "effect.optimizations.material_function_calls_live",
            )?,
            constant_expressions: decode_usize(
                self.constant_expressions,
                "effect.optimizations.constant_expressions",
            )?,
            runtime_parameter_reads: decode_usize(
                self.runtime_parameter_reads,
                "effect.optimizations.runtime_parameter_reads",
            )?,
            eliminated_attributes: decode_usize(
                self.eliminated_attributes,
                "effect.optimizations.eliminated_attributes",
            )?,
            material_common_subexpressions: decode_usize(
                self.material_common_subexpressions,
                "effect.optimizations.material_common_subexpressions",
            )?,
            material_specialized_parameter_reads: decode_usize(
                self.material_specialized_parameter_reads,
                "effect.optimizations.material_specialized_parameter_reads",
            )?,
            material_pruned_static_branches: decode_usize(
                self.material_pruned_static_branches,
                "effect.optimizations.material_pruned_static_branches",
            )?,
            material_pruned_features: decode_usize(
                self.material_pruned_features,
                "effect.optimizations.material_pruned_features",
            )?,
            material_texture_samples_authored: decode_usize(
                self.material_texture_samples_authored,
                "effect.optimizations.material_texture_samples_authored",
            )?,
            material_texture_samples_eliminated: decode_usize(
                self.material_texture_samples_eliminated,
                "effect.optimizations.material_texture_samples_eliminated",
            )?,
            material_texture_samples_live: decode_usize(
                self.material_texture_samples_live,
                "effect.optimizations.material_texture_samples_live",
            )?,
        })
    }
}

fn encode_u32(value: usize, path: impl Into<String>) -> Result<u32, ArtifactError> {
    u32::try_from(value).map_err(|_| ArtifactError::InvalidData {
        path: path.into(),
        message: format!("value {value} exceeds the artifact's 32-bit index range"),
    })
}

fn encode_u64(value: usize, path: impl Into<String>) -> Result<u64, ArtifactError> {
    u64::try_from(value).map_err(|_| ArtifactError::InvalidData {
        path: path.into(),
        message: format!("value {value} exceeds the artifact's 64-bit size range"),
    })
}

fn decode_usize(value: u64, path: impl Into<String>) -> Result<usize, ArtifactError> {
    usize::try_from(value).map_err(|_| ArtifactError::InvalidData {
        path: path.into(),
        message: format!("value {value} exceeds this runtime's addressable size range"),
    })
}

fn require_finite(value: f32, path: impl Into<String>) -> Result<(), ArtifactError> {
    if value.is_finite() {
        Ok(())
    } else {
        invalid(path, "must be finite")
    }
}

fn require_finite_slice(values: &[f32], path: impl Into<String>) -> Result<(), ArtifactError> {
    if values.iter().all(|value| value.is_finite()) {
        Ok(())
    } else {
        invalid(path, "all components must be finite")
    }
}

fn require_finite_non_negative(value: f32, path: impl Into<String>) -> Result<(), ArtifactError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        invalid(path, "must be finite and non-negative")
    }
}

fn require_finite_positive(value: f32, path: impl Into<String>) -> Result<(), ArtifactError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        invalid(path, "must be finite and greater than zero")
    }
}

fn validate_curve_keys(keys: &[CurveKey], path: &str) -> Result<(), ArtifactError> {
    let mut previous = None;
    for (index, key) in keys.iter().enumerate() {
        require_finite_slice(&[key.time, key.value], format!("{path}.keys[{index}]"))?;
        if !(0.0..=1.0).contains(&key.time) {
            return invalid(
                format!("{path}.keys[{index}].time"),
                "must be between zero and one",
            );
        }
        if previous.is_some_and(|previous| key.time <= previous) {
            return invalid(
                format!("{path}.keys[{index}].time"),
                "must be strictly greater than the previous key time",
            );
        }
        previous = Some(key.time);
    }
    Ok(())
}

fn validate_gradient_keys(keys: &[ColorKey], path: &str) -> Result<(), ArtifactError> {
    let mut previous = None;
    for (index, key) in keys.iter().enumerate() {
        require_finite(key.time, format!("{path}.keys[{index}].time"))?;
        require_finite_slice(&key.color, format!("{path}.keys[{index}].color"))?;
        if !(0.0..=1.0).contains(&key.time) {
            return invalid(
                format!("{path}.keys[{index}].time"),
                "must be between zero and one",
            );
        }
        if previous.is_some_and(|previous| key.time <= previous) {
            return invalid(
                format!("{path}.keys[{index}].time"),
                "must be strictly greater than the previous key time",
            );
        }
        previous = Some(key.time);
    }
    Ok(())
}

fn invalid<T>(path: impl Into<String>, message: impl Into<String>) -> Result<T, ArtifactError> {
    Err(ArtifactError::InvalidData {
        path: path.into(),
        message: message.into(),
    })
}
