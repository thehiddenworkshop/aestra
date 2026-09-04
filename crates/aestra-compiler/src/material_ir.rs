use aestra_core::{
    AssetId, MaterialExpressionId, MaterialFunctionId, MaterialParameterId, MaterialProgramId,
    ValidationReport,
    material::{
        MaterialDomain, MaterialEvaluationDomain, MaterialExpressionDomain, MaterialExpressionInfo,
        MaterialExpressionKind, MaterialInput, MaterialParameter, MaterialProgram,
        MaterialRenderStatePolicy, MaterialValue, MaterialValueType, MaterialVectorComponent,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterialIrValueId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialIrConstant {
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// Linear RGBA. Authored sRGB literals are converted during lowering.
    ColorLinear([f32; 4]),
    Texture2D(AssetId),
    Bool(bool),
}

impl MaterialIrConstant {
    fn is_finite(&self) -> bool {
        match self {
            Self::Float(value) => value.is_finite(),
            Self::Vec2(value) => value.iter().all(|item| item.is_finite()),
            Self::Vec3(value) => value.iter().all(|item| item.is_finite()),
            Self::Vec4(value) | Self::ColorLinear(value) => {
                value.iter().all(|item| item.is_finite())
            }
            Self::Texture2D(_) | Self::Bool(_) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialIrInstruction {
    Constant(MaterialIrConstant),
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    CustomWeslCall {
        function: MaterialFunctionId,
        entry_point: String,
        source: String,
        arguments: Vec<MaterialIrValueId>,
    },
    Add(MaterialIrValueId, MaterialIrValueId),
    Subtract(MaterialIrValueId, MaterialIrValueId),
    Multiply(MaterialIrValueId, MaterialIrValueId),
    Divide(MaterialIrValueId, MaterialIrValueId),
    Lerp {
        start: MaterialIrValueId,
        end: MaterialIrValueId,
        factor: MaterialIrValueId,
    },
    Clamp {
        value: MaterialIrValueId,
        min: MaterialIrValueId,
        max: MaterialIrValueId,
    },
    Select {
        condition: MaterialIrValueId,
        if_false: MaterialIrValueId,
        if_true: MaterialIrValueId,
    },
    Remap {
        value: MaterialIrValueId,
        input_min: MaterialIrValueId,
        input_max: MaterialIrValueId,
        output_min: MaterialIrValueId,
        output_max: MaterialIrValueId,
    },
    Smoothstep {
        edge_min: MaterialIrValueId,
        edge_max: MaterialIrValueId,
        value: MaterialIrValueId,
    },
    Fresnel {
        normal: MaterialIrValueId,
        view: MaterialIrValueId,
        power: MaterialIrValueId,
    },
    RadialMask {
        uv: MaterialIrValueId,
        center: MaterialIrValueId,
        radius: MaterialIrValueId,
        softness: MaterialIrValueId,
        invert: MaterialIrValueId,
    },
    Dissolve {
        source: MaterialIrValueId,
        threshold: MaterialIrValueId,
        edge_width: MaterialIrValueId,
        invert: MaterialIrValueId,
    },
    DissolveEdge {
        source: MaterialIrValueId,
        threshold: MaterialIrValueId,
        edge_width: MaterialIrValueId,
        invert: MaterialIrValueId,
    },
    DepthFade {
        scene_depth: MaterialIrValueId,
        pixel_depth: MaterialIrValueId,
        fade_distance: MaterialIrValueId,
        invert: MaterialIrValueId,
    },
    SoftParticle {
        alpha: MaterialIrValueId,
        scene_depth: MaterialIrValueId,
        pixel_depth: MaterialIrValueId,
        fade_distance: MaterialIrValueId,
        invert: MaterialIrValueId,
    },
    PanUv {
        uv: MaterialIrValueId,
        speed: MaterialIrValueId,
        time: MaterialIrValueId,
    },
    RotateUv {
        uv: MaterialIrValueId,
        center: MaterialIrValueId,
        angle: MaterialIrValueId,
    },
    ScaleUv {
        uv: MaterialIrValueId,
        center: MaterialIrValueId,
        scale: MaterialIrValueId,
    },
    DerivativeX {
        value: MaterialIrValueId,
    },
    DerivativeY {
        value: MaterialIrValueId,
    },
    SampleTexture {
        texture: MaterialIrValueId,
        uv: MaterialIrValueId,
        #[serde(default)]
        sampling: MaterialTextureSamplingMode,
    },
    ExtractComponent {
        value: MaterialIrValueId,
        component: MaterialVectorComponent,
    },
}

/// Texture-coordinate evaluation contract carried into backend-neutral material IR.
///
/// Implicit derivatives are common-subexpression safe because material IR is straight-line SSA:
/// identical resource and UV operands are evaluated once within the same fragment invocation.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub enum MaterialTextureSamplingMode {
    #[default]
    ImplicitDerivatives,
    ExplicitLod {
        level: MaterialIrValueId,
    },
    ExplicitGradient {
        ddx: MaterialIrValueId,
        ddy: MaterialIrValueId,
    },
}

impl MaterialTextureSamplingMode {
    const fn common_subexpression_safe(self) -> bool {
        matches!(
            self,
            Self::ImplicitDerivatives | Self::ExplicitLod { .. } | Self::ExplicitGradient { .. }
        )
    }

    fn append_operands(self, operands: &mut Vec<MaterialIrValueId>) {
        match self {
            Self::ImplicitDerivatives => {}
            Self::ExplicitLod { level } => operands.push(level),
            Self::ExplicitGradient { ddx, ddy } => operands.extend([ddx, ddy]),
        }
    }
}

impl MaterialIrInstruction {
    fn dependencies(&self) -> Vec<MaterialIrValueId> {
        match self {
            Self::Constant(_) | Self::Input(_) | Self::Parameter(_) => Vec::new(),
            Self::CustomWeslCall { arguments, .. } => arguments.clone(),
            Self::Add(left, right)
            | Self::Subtract(left, right)
            | Self::Multiply(left, right)
            | Self::Divide(left, right) => vec![*left, *right],
            Self::Lerp { start, end, factor } => vec![*start, *end, *factor],
            Self::Clamp { value, min, max } => vec![*value, *min, *max],
            Self::Select {
                condition,
                if_false,
                if_true,
            } => vec![*condition, *if_false, *if_true],
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
            Self::Fresnel {
                normal,
                view,
                power,
            } => vec![*normal, *view, *power],
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
            Self::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => vec![*alpha, *scene_depth, *pixel_depth, *fade_distance, *invert],
            Self::PanUv { uv, speed, time } => vec![*uv, *speed, *time],
            Self::RotateUv { uv, center, angle } => vec![*uv, *center, *angle],
            Self::ScaleUv { uv, center, scale } => vec![*uv, *center, *scale],
            Self::DerivativeX { value } | Self::DerivativeY { value } => vec![*value],
            Self::SampleTexture {
                texture,
                uv,
                sampling,
            } => {
                let mut dependencies = vec![*texture, *uv];
                sampling.append_operands(&mut dependencies);
                dependencies
            }
            Self::ExtractComponent { value, .. } => vec![*value],
        }
    }

    fn remap(&mut self, ids: &BTreeMap<MaterialIrValueId, MaterialIrValueId>) {
        let remap = |id: &mut MaterialIrValueId| {
            *id = ids[id];
        };
        match self {
            Self::Constant(_) | Self::Input(_) | Self::Parameter(_) => {}
            Self::CustomWeslCall { arguments, .. } => {
                for argument in arguments {
                    remap(argument);
                }
            }
            Self::Add(left, right)
            | Self::Subtract(left, right)
            | Self::Multiply(left, right)
            | Self::Divide(left, right) => {
                remap(left);
                remap(right);
            }
            Self::Lerp { start, end, factor } => {
                remap(start);
                remap(end);
                remap(factor);
            }
            Self::Clamp { value, min, max } => {
                remap(value);
                remap(min);
                remap(max);
            }
            Self::Select {
                condition,
                if_false,
                if_true,
            } => {
                remap(condition);
                remap(if_false);
                remap(if_true);
            }
            Self::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            } => {
                remap(value);
                remap(input_min);
                remap(input_max);
                remap(output_min);
                remap(output_max);
            }
            Self::Smoothstep {
                edge_min,
                edge_max,
                value,
            } => {
                remap(edge_min);
                remap(edge_max);
                remap(value);
            }
            Self::Fresnel {
                normal,
                view,
                power,
            } => {
                remap(normal);
                remap(view);
                remap(power);
            }
            Self::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            } => {
                remap(uv);
                remap(center);
                remap(radius);
                remap(softness);
                remap(invert);
            }
            Self::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            } => {
                remap(source);
                remap(threshold);
                remap(edge_width);
                remap(invert);
            }
            Self::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            } => {
                remap(source);
                remap(threshold);
                remap(edge_width);
                remap(invert);
            }
            Self::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => {
                remap(scene_depth);
                remap(pixel_depth);
                remap(fade_distance);
                remap(invert);
            }
            Self::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => {
                remap(alpha);
                remap(scene_depth);
                remap(pixel_depth);
                remap(fade_distance);
                remap(invert);
            }
            Self::PanUv { uv, speed, time } => {
                remap(uv);
                remap(speed);
                remap(time);
            }
            Self::RotateUv { uv, center, angle } => {
                remap(uv);
                remap(center);
                remap(angle);
            }
            Self::ScaleUv { uv, center, scale } => {
                remap(uv);
                remap(center);
                remap(scale);
            }
            Self::DerivativeX { value } | Self::DerivativeY { value } => remap(value),
            Self::SampleTexture {
                texture,
                uv,
                sampling,
            } => {
                remap(texture);
                remap(uv);
                if let MaterialTextureSamplingMode::ExplicitLod { level } = sampling {
                    remap(level);
                }
                if let MaterialTextureSamplingMode::ExplicitGradient { ddx, ddy } = sampling {
                    remap(ddx);
                    remap(ddy);
                }
            }
            Self::ExtractComponent { value, .. } => remap(value),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialIrValue {
    pub id: MaterialIrValueId,
    pub value_type: MaterialValueType,
    pub evaluation_domain: MaterialExpressionDomain,
    pub instruction: MaterialIrInstruction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialIrParameter {
    pub source: MaterialParameterId,
    pub name: String,
    pub value_type: MaterialValueType,
    pub evaluation_domain: MaterialEvaluationDomain,
    pub default: Option<MaterialIrConstant>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialIrOutputs {
    pub color: MaterialIrValueId,
    pub alpha: MaterialIrValueId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialIrSourceMap {
    pub values: BTreeMap<MaterialExpressionId, MaterialIrValueId>,
    pub expressions: BTreeMap<MaterialIrValueId, Vec<MaterialExpressionId>>,
    pub eliminated: BTreeSet<MaterialExpressionId>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialIrOptimizationStats {
    pub constant_folds: usize,
    pub trivial_simplifications: usize,
    /// Shader-static parameter reads replaced by their typed program defaults.
    pub specialized_parameter_reads: usize,
    /// Select expressions whose unreachable branch was discarded at compile time.
    pub pruned_static_branches: usize,
    /// Dynamic inputs, parameter reads, texture samples, and custom calls removed from the live IR.
    pub pruned_features: usize,
    /// Enabled semantic texture samples before specialization, CSE, and dead-value removal.
    #[serde(default)]
    pub texture_samples_authored: usize,
    /// Reachable texture samples removed by specialization, CSE, or dead-value removal.
    #[serde(default)]
    pub texture_samples_eliminated: usize,
    /// Texture samples that remain in the optimized material IR.
    #[serde(default)]
    pub texture_samples_live: usize,
    /// Pure expressions that reuse an already-lowered IR value.
    pub common_subexpressions: usize,
    pub eliminated_values: usize,
    pub eliminated_expressions: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialIrProgram {
    pub source: MaterialProgramId,
    pub name: String,
    pub domain: MaterialDomain,
    pub render_state_policy: MaterialRenderStatePolicy,
    pub parameters: Vec<MaterialIrParameter>,
    pub values: Vec<MaterialIrValue>,
    pub outputs: MaterialIrOutputs,
    pub source_map: MaterialIrSourceMap,
    pub optimizations: MaterialIrOptimizationStats,
}

impl MaterialIrProgram {
    pub fn value(&self, id: MaterialIrValueId) -> Option<&MaterialIrValue> {
        self.values
            .get(id.0 as usize)
            .filter(|value| value.id == id)
    }
}

#[derive(Debug, Error)]
pub enum MaterialCompileError {
    #[error("material program validation failed: {0}")]
    Validation(ValidationReport),
}

impl MaterialCompileError {
    pub fn report(&self) -> &ValidationReport {
        match self {
            Self::Validation(report) => report,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MaterialCompiler;

impl MaterialCompiler {
    pub fn compile(
        &self,
        program: &MaterialProgram,
    ) -> Result<MaterialIrProgram, MaterialCompileError> {
        self.compile_with_functions(program, &crate::MaterialFunctionLibrary::default())
    }

    pub(crate) fn compile_expanded(
        &self,
        program: &MaterialProgram,
    ) -> Result<MaterialIrProgram, MaterialCompileError> {
        let normalized = program.normalized();
        let semantic_features = semantic_feature_stats(&normalized);
        let analysis = normalized
            .analyze()
            .map_err(MaterialCompileError::Validation)?;
        let expressions = normalized
            .expressions
            .iter()
            .map(|expression| (expression.id, &expression.kind))
            .collect::<BTreeMap<_, _>>();
        let parameters = normalized
            .parameters
            .iter()
            .map(|parameter| (parameter.id, parameter))
            .collect::<BTreeMap<_, _>>();
        let mut builder = MaterialIrBuilder {
            analysis: &analysis.expressions,
            expressions,
            parameters,
            disabled_expressions: normalized.disabled_expressions.iter().copied().collect(),
            values: Vec::new(),
            source_map: MaterialIrSourceMap::default(),
            optimizations: MaterialIrOptimizationStats::default(),
            common_subexpressions: BTreeMap::new(),
        };
        let color = builder.lower(normalized.outputs.color);
        let alpha = builder.lower(normalized.outputs.alpha);
        let unreachable = normalized
            .expressions
            .iter()
            .map(|expression| expression.id)
            .filter(|id| !builder.source_map.values.contains_key(id))
            .collect::<Vec<_>>();
        builder
            .source_map
            .eliminated
            .extend(unreachable.iter().copied());
        builder.optimizations.eliminated_expressions += unreachable.len();

        let (values, outputs, source_map, mut optimizations) =
            builder.finish(MaterialIrOutputs { color, alpha });
        optimizations.pruned_features = semantic_features
            .total
            .saturating_sub(ir_feature_count(&values));
        optimizations.texture_samples_authored = semantic_features.texture_samples;
        optimizations.texture_samples_live = ir_texture_sample_count(&values);
        optimizations.texture_samples_eliminated = optimizations
            .texture_samples_authored
            .saturating_sub(optimizations.texture_samples_live);
        let parameters = normalized
            .parameters
            .iter()
            .map(|parameter| MaterialIrParameter {
                source: parameter.id,
                name: parameter.name.clone(),
                value_type: parameter.value_type,
                evaluation_domain: parameter.evaluation_domain,
                default: parameter.default.as_ref().map(lower_constant),
            })
            .collect();
        Ok(MaterialIrProgram {
            source: normalized.id,
            name: normalized.name,
            domain: normalized.domain,
            render_state_policy: normalized.render_state_policy,
            parameters,
            values,
            outputs,
            source_map,
            optimizations,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SemanticFeatureStats {
    total: usize,
    texture_samples: usize,
}

fn semantic_feature_stats(program: &MaterialProgram) -> SemanticFeatureStats {
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let parameters = program
        .parameters
        .iter()
        .map(|parameter| (parameter.id, parameter))
        .collect::<BTreeMap<_, _>>();
    let authored_texture_samples = program
        .expressions
        .iter()
        .filter(|expression| {
            !program.disabled_expressions.contains(&expression.id)
                && matches!(
                    expression.kind,
                    MaterialExpressionKind::SampleTexture { .. }
                        | MaterialExpressionKind::SampleTextureLevel { .. }
                        | MaterialExpressionKind::SampleTextureGradient { .. }
                )
        })
        .count();
    let mut pending = vec![program.outputs.color, program.outputs.alpha];
    let mut visited = BTreeSet::new();
    let mut inputs = BTreeSet::new();
    let mut dynamic_parameters = BTreeSet::new();
    let mut reachable_texture_samples = 0usize;
    let mut custom_calls = 0usize;
    while let Some(expression) = pending.pop() {
        if !visited.insert(expression) {
            continue;
        }
        let Some(kind) = expressions.get(&expression).copied() else {
            continue;
        };
        if program.disabled_expressions.contains(&expression) {
            if let Some(source) = kind.bypass_input() {
                pending.push(source);
            }
            continue;
        }
        match kind {
            MaterialExpressionKind::Input(input) => {
                inputs.insert(material_input_key(*input));
            }
            MaterialExpressionKind::Parameter(parameter) => {
                if parameters.get(parameter).is_some_and(|definition| {
                    definition.evaluation_domain != MaterialEvaluationDomain::ShaderStatic
                }) {
                    dynamic_parameters.insert(*parameter);
                }
            }
            MaterialExpressionKind::SampleTexture { .. }
            | MaterialExpressionKind::SampleTextureLevel { .. }
            | MaterialExpressionKind::SampleTextureGradient { .. } => {
                reachable_texture_samples += 1;
            }
            MaterialExpressionKind::CustomWeslCall { .. } => custom_calls += 1,
            _ => {}
        }
        pending.extend(kind.dependencies());
    }
    SemanticFeatureStats {
        total: inputs.len() + dynamic_parameters.len() + reachable_texture_samples + custom_calls,
        texture_samples: authored_texture_samples,
    }
}

fn ir_feature_count(values: &[MaterialIrValue]) -> usize {
    let mut inputs = BTreeSet::new();
    let mut parameters = BTreeSet::new();
    let mut texture_samples = 0usize;
    let mut custom_calls = 0usize;
    for value in values {
        match &value.instruction {
            MaterialIrInstruction::Input(input) => {
                inputs.insert(material_input_key(*input));
            }
            MaterialIrInstruction::Parameter(parameter) => {
                parameters.insert(*parameter);
            }
            MaterialIrInstruction::SampleTexture { .. } => texture_samples += 1,
            MaterialIrInstruction::CustomWeslCall { .. } => custom_calls += 1,
            _ => {}
        }
    }
    inputs.len() + parameters.len() + texture_samples + custom_calls
}

fn ir_texture_sample_count(values: &[MaterialIrValue]) -> usize {
    values
        .iter()
        .filter(|value| {
            matches!(
                value.instruction,
                MaterialIrInstruction::SampleTexture { .. }
            )
        })
        .count()
}

struct MaterialIrBuilder<'a> {
    analysis: &'a BTreeMap<MaterialExpressionId, MaterialExpressionInfo>,
    expressions: BTreeMap<MaterialExpressionId, &'a MaterialExpressionKind>,
    parameters: BTreeMap<MaterialParameterId, &'a MaterialParameter>,
    disabled_expressions: BTreeSet<MaterialExpressionId>,
    values: Vec<MaterialIrValue>,
    source_map: MaterialIrSourceMap,
    optimizations: MaterialIrOptimizationStats,
    common_subexpressions: BTreeMap<MaterialIrCseKey, MaterialIrValueId>,
}

impl MaterialIrBuilder<'_> {
    fn lower(&mut self, expression: MaterialExpressionId) -> MaterialIrValueId {
        if let Some(id) = self.source_map.values.get(&expression) {
            return *id;
        }
        let kind = self.expressions[&expression].clone();
        if self.disabled_expressions.contains(&expression) {
            let source = kind
                .bypass_input()
                .expect("validated disabled expression has a bypass input");
            let alias = self.lower(source);
            self.source_map.values.insert(expression, alias);
            self.source_map.eliminated.insert(expression);
            self.optimizations.eliminated_expressions += 1;
            return alias;
        }
        let info = self.analysis[&expression];
        let instruction = match kind {
            MaterialExpressionKind::Constant(value) => {
                MaterialIrInstruction::Constant(lower_constant(&value))
            }
            MaterialExpressionKind::Input(input) => MaterialIrInstruction::Input(input),
            MaterialExpressionKind::Parameter(parameter) => {
                let definition = self.parameters[&parameter];
                if definition.evaluation_domain == MaterialEvaluationDomain::ShaderStatic {
                    self.optimizations.specialized_parameter_reads += 1;
                    MaterialIrInstruction::Constant(lower_constant(
                        definition
                            .default
                            .as_ref()
                            .expect("validated shader-static parameter has a default"),
                    ))
                } else {
                    MaterialIrInstruction::Parameter(parameter)
                }
            }
            MaterialExpressionKind::FunctionInput(_)
            | MaterialExpressionKind::FunctionCall { .. } => {
                unreachable!("validated material functions must be inlined before IR lowering")
            }
            MaterialExpressionKind::CustomWeslCall {
                function,
                entry_point,
                source,
                arguments,
                ..
            } => MaterialIrInstruction::CustomWeslCall {
                function,
                entry_point,
                source,
                arguments: arguments
                    .iter()
                    .map(|argument| self.lower(argument.expression))
                    .collect(),
            },
            MaterialExpressionKind::Add(left, right) => {
                let left = self.lower(left);
                let right = self.lower(right);
                if let Some(constant) = self.fold_binary(left, right, fold_add) {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else if let Some(alias) = self.add_identity(left, right, info.value_type) {
                    self.optimizations.trivial_simplifications += 1;
                    self.source_map.values.insert(expression, alias);
                    return alias;
                } else {
                    MaterialIrInstruction::Add(left, right)
                }
            }
            MaterialExpressionKind::Subtract(left, right) => {
                let left = self.lower(left);
                let right = self.lower(right);
                if let Some(constant) = self.fold_binary(left, right, fold_subtract) {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else {
                    MaterialIrInstruction::Subtract(left, right)
                }
            }
            MaterialExpressionKind::Multiply(left, right) => {
                let left = self.lower(left);
                let right = self.lower(right);
                if let Some(constant) = self.fold_binary(left, right, fold_multiply) {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else if let Some(alias) = self.multiply_identity(left, right, info.value_type) {
                    self.optimizations.trivial_simplifications += 1;
                    self.source_map.values.insert(expression, alias);
                    return alias;
                } else if self.is_zero(left) || self.is_zero(right) {
                    self.optimizations.trivial_simplifications += 1;
                    MaterialIrInstruction::Constant(zero_constant(info.value_type))
                } else {
                    MaterialIrInstruction::Multiply(left, right)
                }
            }
            MaterialExpressionKind::Divide(left, right) => {
                let left = self.lower(left);
                let right = self.lower(right);
                if let Some(constant) = self.fold_binary(left, right, fold_divide) {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else {
                    MaterialIrInstruction::Divide(left, right)
                }
            }
            MaterialExpressionKind::Lerp { start, end, factor } => {
                let start = self.lower(start);
                let end = self.lower(end);
                let factor = self.lower(factor);
                if let Some(constant) = self.fold_ternary(start, end, factor, fold_lerp) {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else {
                    MaterialIrInstruction::Lerp { start, end, factor }
                }
            }
            MaterialExpressionKind::Clamp { value, min, max } => {
                let value = self.lower(value);
                let min = self.lower(min);
                let max = self.lower(max);
                if let Some(constant) = self.fold_ternary(value, min, max, fold_clamp) {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else {
                    MaterialIrInstruction::Clamp { value, min, max }
                }
            }
            MaterialExpressionKind::Select {
                condition,
                if_false,
                if_true,
            } => {
                let condition = self.lower(condition);
                if let Some(MaterialIrConstant::Bool(selected_true)) = self.constant(condition) {
                    let selected = if *selected_true { if_true } else { if_false };
                    let alias = self.lower(selected);
                    self.source_map.values.insert(expression, alias);
                    self.source_map.eliminated.insert(expression);
                    self.optimizations.pruned_static_branches += 1;
                    self.optimizations.eliminated_expressions += 1;
                    return alias;
                }
                MaterialIrInstruction::Select {
                    condition,
                    if_false: self.lower(if_false),
                    if_true: self.lower(if_true),
                }
            }
            MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            } => {
                let value = self.lower(value);
                let input_min = self.lower(input_min);
                let input_max = self.lower(input_max);
                let output_min = self.lower(output_min);
                let output_max = self.lower(output_max);
                MaterialIrInstruction::Remap {
                    value,
                    input_min,
                    input_max,
                    output_min,
                    output_max,
                }
            }
            MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            } => MaterialIrInstruction::Smoothstep {
                edge_min: self.lower(edge_min),
                edge_max: self.lower(edge_max),
                value: self.lower(value),
            },
            MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            } => MaterialIrInstruction::Fresnel {
                normal: self.lower(normal),
                view: self.lower(view),
                power: self.lower(power),
            },
            MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            } => MaterialIrInstruction::RadialMask {
                uv: self.lower(uv),
                center: self.lower(center),
                radius: self.lower(radius),
                softness: self.lower(softness),
                invert: self.lower(invert),
            },
            MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            } => MaterialIrInstruction::Dissolve {
                source: self.lower(source),
                threshold: self.lower(threshold),
                edge_width: self.lower(edge_width),
                invert: self.lower(invert),
            },
            MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            } => MaterialIrInstruction::DissolveEdge {
                source: self.lower(source),
                threshold: self.lower(threshold),
                edge_width: self.lower(edge_width),
                invert: self.lower(invert),
            },
            MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => MaterialIrInstruction::DepthFade {
                scene_depth: self.lower(scene_depth),
                pixel_depth: self.lower(pixel_depth),
                fade_distance: self.lower(fade_distance),
                invert: self.lower(invert),
            },
            MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => MaterialIrInstruction::SoftParticle {
                alpha: self.lower(alpha),
                scene_depth: self.lower(scene_depth),
                pixel_depth: self.lower(pixel_depth),
                fade_distance: self.lower(fade_distance),
                invert: self.lower(invert),
            },
            MaterialExpressionKind::PanUv { uv, speed, time } => {
                let uv = self.lower(uv);
                let speed = self.lower(speed);
                let time = self.lower(time);
                MaterialIrInstruction::PanUv { uv, speed, time }
            }
            MaterialExpressionKind::RotateUv { uv, center, angle } => {
                let uv = self.lower(uv);
                let center = self.lower(center);
                let angle = self.lower(angle);
                MaterialIrInstruction::RotateUv { uv, center, angle }
            }
            MaterialExpressionKind::ScaleUv { uv, center, scale } => {
                let uv = self.lower(uv);
                let center = self.lower(center);
                let scale = self.lower(scale);
                MaterialIrInstruction::ScaleUv { uv, center, scale }
            }
            MaterialExpressionKind::DerivativeX { value } => {
                let value = self.lower(value);
                MaterialIrInstruction::DerivativeX { value }
            }
            MaterialExpressionKind::DerivativeY { value } => {
                let value = self.lower(value);
                MaterialIrInstruction::DerivativeY { value }
            }
            MaterialExpressionKind::SampleTexture { texture, uv } => {
                let texture = self.lower(texture);
                let uv = self.lower(uv);
                MaterialIrInstruction::SampleTexture {
                    texture,
                    uv,
                    sampling: MaterialTextureSamplingMode::ImplicitDerivatives,
                }
            }
            MaterialExpressionKind::SampleTextureLevel { texture, uv, level } => {
                let texture = self.lower(texture);
                let uv = self.lower(uv);
                let level = self.lower(level);
                MaterialIrInstruction::SampleTexture {
                    texture,
                    uv,
                    sampling: MaterialTextureSamplingMode::ExplicitLod { level },
                }
            }
            MaterialExpressionKind::SampleTextureGradient {
                texture,
                uv,
                ddx,
                ddy,
            } => {
                let texture = self.lower(texture);
                let uv = self.lower(uv);
                let ddx = self.lower(ddx);
                let ddy = self.lower(ddy);
                MaterialIrInstruction::SampleTexture {
                    texture,
                    uv,
                    sampling: MaterialTextureSamplingMode::ExplicitGradient { ddx, ddy },
                }
            }
            MaterialExpressionKind::ExtractComponent { value, component } => {
                let value = self.lower(value);
                if let Some(constant) = self
                    .constant(value)
                    .and_then(|constant| extract_component(constant, component))
                {
                    self.optimizations.constant_folds += 1;
                    MaterialIrInstruction::Constant(constant)
                } else {
                    MaterialIrInstruction::ExtractComponent { value, component }
                }
            }
        };
        let common_subexpression =
            MaterialIrCseKey::new(info.value_type, info.evaluation_domain, &instruction);
        if let Some(existing) = common_subexpression
            .as_ref()
            .and_then(|key| self.common_subexpressions.get(key))
            .copied()
        {
            self.source_map.values.insert(expression, existing);
            self.optimizations.common_subexpressions += 1;
            return existing;
        }

        let id = MaterialIrValueId(self.values.len() as u32);
        self.values.push(MaterialIrValue {
            id,
            value_type: info.value_type,
            evaluation_domain: info.evaluation_domain,
            instruction,
        });
        if let Some(key) = common_subexpression {
            self.common_subexpressions.insert(key, id);
        }
        self.source_map.values.insert(expression, id);
        id
    }

    fn finish(
        mut self,
        outputs: MaterialIrOutputs,
    ) -> (
        Vec<MaterialIrValue>,
        MaterialIrOutputs,
        MaterialIrSourceMap,
        MaterialIrOptimizationStats,
    ) {
        let mut live = BTreeSet::new();
        collect_live(outputs.color, &self.values, &mut live);
        collect_live(outputs.alpha, &self.values, &mut live);
        let mut remap = BTreeMap::new();
        let mut values = Vec::with_capacity(live.len());
        for value in &self.values {
            if live.contains(&value.id) {
                remap.insert(value.id, MaterialIrValueId(values.len() as u32));
                values.push(value.clone());
            }
        }
        self.optimizations.eliminated_values += self.values.len() - values.len();
        for value in &mut values {
            value.id = remap[&value.id];
            value.instruction.remap(&remap);
        }
        for (expression, id) in std::mem::take(&mut self.source_map.values) {
            if let Some(remapped) = remap.get(&id) {
                self.source_map.values.insert(expression, *remapped);
                self.source_map
                    .expressions
                    .entry(*remapped)
                    .or_default()
                    .push(expression);
            } else {
                self.source_map.eliminated.insert(expression);
                self.optimizations.eliminated_expressions += 1;
            }
        }
        for sources in self.source_map.expressions.values_mut() {
            sources.sort();
            sources.dedup();
        }
        (
            values,
            MaterialIrOutputs {
                color: remap[&outputs.color],
                alpha: remap[&outputs.alpha],
            },
            self.source_map,
            self.optimizations,
        )
    }

    fn constant(&self, id: MaterialIrValueId) -> Option<&MaterialIrConstant> {
        match &self.values[id.0 as usize].instruction {
            MaterialIrInstruction::Constant(value) => Some(value),
            _ => None,
        }
    }

    fn fold_binary(
        &self,
        left: MaterialIrValueId,
        right: MaterialIrValueId,
        fold: impl FnOnce(&MaterialIrConstant, &MaterialIrConstant) -> Option<MaterialIrConstant>,
    ) -> Option<MaterialIrConstant> {
        fold(self.constant(left)?, self.constant(right)?)
    }

    fn fold_ternary(
        &self,
        first: MaterialIrValueId,
        second: MaterialIrValueId,
        third: MaterialIrValueId,
        fold: impl FnOnce(
            &MaterialIrConstant,
            &MaterialIrConstant,
            &MaterialIrConstant,
        ) -> Option<MaterialIrConstant>,
    ) -> Option<MaterialIrConstant> {
        fold(
            self.constant(first)?,
            self.constant(second)?,
            self.constant(third)?,
        )
    }

    fn add_identity(
        &self,
        left: MaterialIrValueId,
        right: MaterialIrValueId,
        value_type: MaterialValueType,
    ) -> Option<MaterialIrValueId> {
        if self.is_zero(left) && self.values[right.0 as usize].value_type == value_type {
            Some(right)
        } else if self.is_zero(right) && self.values[left.0 as usize].value_type == value_type {
            Some(left)
        } else {
            None
        }
    }

    fn multiply_identity(
        &self,
        left: MaterialIrValueId,
        right: MaterialIrValueId,
        value_type: MaterialValueType,
    ) -> Option<MaterialIrValueId> {
        if self.is_one(left) && self.values[right.0 as usize].value_type == value_type {
            Some(right)
        } else if self.is_one(right) && self.values[left.0 as usize].value_type == value_type {
            Some(left)
        } else {
            None
        }
    }

    fn is_zero(&self, id: MaterialIrValueId) -> bool {
        self.constant(id).is_some_and(is_zero)
    }

    fn is_one(&self, id: MaterialIrValueId) -> bool {
        self.constant(id).is_some_and(is_one)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialIrCseKey {
    value_type: MaterialIrValueTypeKey,
    evaluation_domain: MaterialExpressionDomain,
    instruction: MaterialIrInstructionKey,
}

impl MaterialIrCseKey {
    fn new(
        value_type: MaterialValueType,
        evaluation_domain: MaterialExpressionDomain,
        instruction: &MaterialIrInstruction,
    ) -> Option<Self> {
        Some(Self {
            value_type: MaterialIrValueTypeKey::from(value_type),
            evaluation_domain,
            instruction: MaterialIrInstructionKey::from_instruction(instruction)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum MaterialIrValueTypeKey {
    Float,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Texture2d {
        color_space: u8,
        filter: u8,
        mip_filter: u8,
        address_u: u8,
        address_v: u8,
    },
    Bool,
}

impl From<MaterialValueType> for MaterialIrValueTypeKey {
    fn from(value: MaterialValueType) -> Self {
        match value {
            MaterialValueType::Float => Self::Float,
            MaterialValueType::Vec2 => Self::Vec2,
            MaterialValueType::Vec3 => Self::Vec3,
            MaterialValueType::Vec4 => Self::Vec4,
            MaterialValueType::Color => Self::Color,
            MaterialValueType::Texture2D(descriptor) => Self::Texture2d {
                color_space: descriptor.color_space as u8,
                filter: descriptor.sampler.filter as u8,
                mip_filter: descriptor.sampler.mip_filter as u8,
                address_u: descriptor.sampler.address_u as u8,
                address_v: descriptor.sampler.address_v as u8,
            },
            MaterialValueType::Bool => Self::Bool,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MaterialIrInstructionKey {
    opcode: u8,
    operands: Vec<MaterialIrValueId>,
    payload: MaterialIrInstructionPayloadKey,
}

impl MaterialIrInstructionKey {
    fn from_instruction(instruction: &MaterialIrInstruction) -> Option<Self> {
        let (opcode, mut operands, payload) = match instruction {
            MaterialIrInstruction::Constant(value) => (
                0,
                Vec::new(),
                MaterialIrInstructionPayloadKey::Constant(value.into()),
            ),
            MaterialIrInstruction::Input(input) => (
                1,
                Vec::new(),
                MaterialIrInstructionPayloadKey::Input(material_input_key(*input)),
            ),
            MaterialIrInstruction::Parameter(parameter) => (
                2,
                Vec::new(),
                MaterialIrInstructionPayloadKey::Parameter(*parameter),
            ),
            // A custom function needs an explicit purity contract before calls may be merged.
            MaterialIrInstruction::CustomWeslCall { .. } => return None,
            MaterialIrInstruction::Add(left, right) => (
                3,
                vec![*left, *right],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Subtract(left, right) => (
                4,
                vec![*left, *right],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Multiply(left, right) => (
                5,
                vec![*left, *right],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Divide(left, right) => (
                6,
                vec![*left, *right],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Lerp { start, end, factor } => (
                7,
                vec![*start, *end, *factor],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Clamp { value, min, max } => (
                8,
                vec![*value, *min, *max],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Select {
                condition,
                if_false,
                if_true,
            } => (
                22,
                vec![*condition, *if_false, *if_true],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            } => (
                9,
                vec![*value, *input_min, *input_max, *output_min, *output_max],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Smoothstep {
                edge_min,
                edge_max,
                value,
            } => (
                10,
                vec![*edge_min, *edge_max, *value],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Fresnel {
                normal,
                view,
                power,
            } => (
                11,
                vec![*normal, *view, *power],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            } => (
                12,
                vec![*uv, *center, *radius, *softness, *invert],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            } => (
                13,
                vec![*source, *threshold, *edge_width, *invert],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            } => (
                14,
                vec![*source, *threshold, *edge_width, *invert],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => (
                15,
                vec![*scene_depth, *pixel_depth, *fade_distance, *invert],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => (
                16,
                vec![*alpha, *scene_depth, *pixel_depth, *fade_distance, *invert],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::PanUv { uv, speed, time } => (
                17,
                vec![*uv, *speed, *time],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::RotateUv { uv, center, angle } => (
                18,
                vec![*uv, *center, *angle],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::ScaleUv { uv, center, scale } => (
                19,
                vec![*uv, *center, *scale],
                MaterialIrInstructionPayloadKey::None,
            ),
            MaterialIrInstruction::DerivativeX { value } => {
                (23, vec![*value], MaterialIrInstructionPayloadKey::None)
            }
            MaterialIrInstruction::DerivativeY { value } => {
                (24, vec![*value], MaterialIrInstructionPayloadKey::None)
            }
            MaterialIrInstruction::SampleTexture {
                texture,
                uv,
                sampling,
            } if sampling.common_subexpression_safe() => (
                20,
                {
                    let mut operands = vec![*texture, *uv];
                    sampling.append_operands(&mut operands);
                    operands
                },
                MaterialIrInstructionPayloadKey::TextureSampling(*sampling),
            ),
            MaterialIrInstruction::SampleTexture { .. } => return None,
            MaterialIrInstruction::ExtractComponent { value, component } => (
                21,
                vec![*value],
                MaterialIrInstructionPayloadKey::Component(*component as u8),
            ),
        };
        if matches!(opcode, 3 | 5) {
            operands.sort_unstable();
        }
        Some(Self {
            opcode,
            operands,
            payload,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum MaterialIrInstructionPayloadKey {
    None,
    Constant(MaterialIrConstantKey),
    Input(u8),
    Parameter(MaterialParameterId),
    Component(u8),
    TextureSampling(MaterialTextureSamplingMode),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum MaterialIrConstantKey {
    Float(u32),
    Vec2([u32; 2]),
    Vec3([u32; 3]),
    Vec4([u32; 4]),
    ColorLinear([u32; 4]),
    Texture2d(AssetId),
    Bool(bool),
}

impl From<&MaterialIrConstant> for MaterialIrConstantKey {
    fn from(value: &MaterialIrConstant) -> Self {
        match value {
            MaterialIrConstant::Float(value) => Self::Float(value.to_bits()),
            MaterialIrConstant::Vec2(value) => Self::Vec2(value.map(f32::to_bits)),
            MaterialIrConstant::Vec3(value) => Self::Vec3(value.map(f32::to_bits)),
            MaterialIrConstant::Vec4(value) => Self::Vec4(value.map(f32::to_bits)),
            MaterialIrConstant::ColorLinear(value) => Self::ColorLinear(value.map(f32::to_bits)),
            MaterialIrConstant::Texture2D(asset) => Self::Texture2d(*asset),
            MaterialIrConstant::Bool(value) => Self::Bool(*value),
        }
    }
}

const fn material_input_key(input: MaterialInput) -> u8 {
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

fn collect_live(
    id: MaterialIrValueId,
    values: &[MaterialIrValue],
    live: &mut BTreeSet<MaterialIrValueId>,
) {
    if !live.insert(id) {
        return;
    }
    for dependency in values[id.0 as usize].instruction.dependencies() {
        collect_live(dependency, values, live);
    }
}

fn lower_constant(value: &MaterialValue) -> MaterialIrConstant {
    match value {
        MaterialValue::Float(value) => MaterialIrConstant::Float(*value),
        MaterialValue::Vec2(value) => MaterialIrConstant::Vec2(*value),
        MaterialValue::Vec3(value) => MaterialIrConstant::Vec3(*value),
        MaterialValue::Vec4(value) => MaterialIrConstant::Vec4(*value),
        MaterialValue::ColorSrgb(value) => MaterialIrConstant::ColorLinear([
            srgb_to_linear(value[0]),
            srgb_to_linear(value[1]),
            srgb_to_linear(value[2]),
            value[3],
        ]),
        MaterialValue::Texture2D(asset) => MaterialIrConstant::Texture2D(*asset),
        MaterialValue::Bool(value) => MaterialIrConstant::Bool(*value),
    }
}

fn extract_component(
    value: &MaterialIrConstant,
    component: MaterialVectorComponent,
) -> Option<MaterialIrConstant> {
    let index = match component {
        MaterialVectorComponent::X => 0,
        MaterialVectorComponent::Y => 1,
        MaterialVectorComponent::Z => 2,
        MaterialVectorComponent::W => 3,
    };
    let value = match value {
        MaterialIrConstant::Vec2(value) => value.get(index).copied(),
        MaterialIrConstant::Vec3(value) => value.get(index).copied(),
        MaterialIrConstant::Vec4(value) | MaterialIrConstant::ColorLinear(value) => {
            value.get(index).copied()
        }
        _ => None,
    }?;
    Some(MaterialIrConstant::Float(value))
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn fold_add(left: &MaterialIrConstant, right: &MaterialIrConstant) -> Option<MaterialIrConstant> {
    fold_same(left, right, |left, right| left + right)
}

fn fold_subtract(
    left: &MaterialIrConstant,
    right: &MaterialIrConstant,
) -> Option<MaterialIrConstant> {
    fold_same(left, right, |left, right| left - right)
}

fn fold_multiply(
    left: &MaterialIrConstant,
    right: &MaterialIrConstant,
) -> Option<MaterialIrConstant> {
    fold_same(left, right, |left, right| left * right)
        .or_else(|| fold_scale(left, right, |value, scale| value * scale))
        .filter(MaterialIrConstant::is_finite)
}

fn fold_divide(
    left: &MaterialIrConstant,
    right: &MaterialIrConstant,
) -> Option<MaterialIrConstant> {
    fold_same(left, right, |left, right| left / right)
        .or_else(|| fold_scale(left, right, |value, scale| value / scale))
        .filter(MaterialIrConstant::is_finite)
}

fn fold_lerp(
    start: &MaterialIrConstant,
    end: &MaterialIrConstant,
    factor: &MaterialIrConstant,
) -> Option<MaterialIrConstant> {
    let MaterialIrConstant::Float(factor) = factor else {
        return None;
    };
    fold_same(start, end, |start, end| start + (end - start) * factor)
}

fn fold_clamp(
    value: &MaterialIrConstant,
    min: &MaterialIrConstant,
    max: &MaterialIrConstant,
) -> Option<MaterialIrConstant> {
    fold_three_same(value, min, max, |value, min, max| {
        (min <= max).then(|| value.clamp(min, max))
    })
}

fn fold_same(
    left: &MaterialIrConstant,
    right: &MaterialIrConstant,
    operation: impl Fn(f32, f32) -> f32 + Copy,
) -> Option<MaterialIrConstant> {
    let value = match (left, right) {
        (MaterialIrConstant::Float(left), MaterialIrConstant::Float(right)) => {
            MaterialIrConstant::Float(operation(*left, *right))
        }
        (MaterialIrConstant::Vec2(left), MaterialIrConstant::Vec2(right)) => {
            MaterialIrConstant::Vec2(zip(left, right, operation))
        }
        (MaterialIrConstant::Vec3(left), MaterialIrConstant::Vec3(right)) => {
            MaterialIrConstant::Vec3(zip(left, right, operation))
        }
        (MaterialIrConstant::Vec4(left), MaterialIrConstant::Vec4(right)) => {
            MaterialIrConstant::Vec4(zip(left, right, operation))
        }
        (MaterialIrConstant::ColorLinear(left), MaterialIrConstant::ColorLinear(right)) => {
            MaterialIrConstant::ColorLinear(zip(left, right, operation))
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn fold_scale(
    left: &MaterialIrConstant,
    right: &MaterialIrConstant,
    operation: impl Fn(f32, f32) -> f32 + Copy,
) -> Option<MaterialIrConstant> {
    match (left, right) {
        (value, MaterialIrConstant::Float(scale)) => {
            map_numeric(value, |value| operation(value, *scale))
        }
        (MaterialIrConstant::Float(scale), value) => {
            map_numeric(value, |value| operation(*scale, value))
        }
        _ => None,
    }
}

fn fold_three_same(
    value: &MaterialIrConstant,
    min: &MaterialIrConstant,
    max: &MaterialIrConstant,
    operation: impl Fn(f32, f32, f32) -> Option<f32> + Copy,
) -> Option<MaterialIrConstant> {
    match (value, min, max) {
        (
            MaterialIrConstant::Float(value),
            MaterialIrConstant::Float(min),
            MaterialIrConstant::Float(max),
        ) => operation(*value, *min, *max).map(MaterialIrConstant::Float),
        (
            MaterialIrConstant::Vec2(value),
            MaterialIrConstant::Vec2(min),
            MaterialIrConstant::Vec2(max),
        ) => zip_three(value, min, max, operation).map(MaterialIrConstant::Vec2),
        (
            MaterialIrConstant::Vec3(value),
            MaterialIrConstant::Vec3(min),
            MaterialIrConstant::Vec3(max),
        ) => zip_three(value, min, max, operation).map(MaterialIrConstant::Vec3),
        (
            MaterialIrConstant::Vec4(value),
            MaterialIrConstant::Vec4(min),
            MaterialIrConstant::Vec4(max),
        ) => zip_three(value, min, max, operation).map(MaterialIrConstant::Vec4),
        (
            MaterialIrConstant::ColorLinear(value),
            MaterialIrConstant::ColorLinear(min),
            MaterialIrConstant::ColorLinear(max),
        ) => zip_three(value, min, max, operation).map(MaterialIrConstant::ColorLinear),
        _ => None,
    }
}

fn map_numeric(
    value: &MaterialIrConstant,
    operation: impl Fn(f32) -> f32 + Copy,
) -> Option<MaterialIrConstant> {
    let value = match value {
        MaterialIrConstant::Float(value) => MaterialIrConstant::Float(operation(*value)),
        MaterialIrConstant::Vec2(value) => MaterialIrConstant::Vec2(value.map(operation)),
        MaterialIrConstant::Vec3(value) => MaterialIrConstant::Vec3(value.map(operation)),
        MaterialIrConstant::Vec4(value) => MaterialIrConstant::Vec4(value.map(operation)),
        MaterialIrConstant::ColorLinear(value) => {
            MaterialIrConstant::ColorLinear(value.map(operation))
        }
        MaterialIrConstant::Texture2D(_) | MaterialIrConstant::Bool(_) => return None,
    };
    value.is_finite().then_some(value)
}

fn zip<const N: usize>(
    left: &[f32; N],
    right: &[f32; N],
    operation: impl Fn(f32, f32) -> f32 + Copy,
) -> [f32; N] {
    std::array::from_fn(|index| operation(left[index], right[index]))
}

fn zip_three<const N: usize>(
    value: &[f32; N],
    min: &[f32; N],
    max: &[f32; N],
    operation: impl Fn(f32, f32, f32) -> Option<f32> + Copy,
) -> Option<[f32; N]> {
    let mut result = [0.0; N];
    for index in 0..N {
        result[index] = operation(value[index], min[index], max[index])?;
    }
    Some(result)
}

fn is_zero(value: &MaterialIrConstant) -> bool {
    match value {
        MaterialIrConstant::Float(value) => *value == 0.0,
        MaterialIrConstant::Vec2(value) => value.iter().all(|value| *value == 0.0),
        MaterialIrConstant::Vec3(value) => value.iter().all(|value| *value == 0.0),
        MaterialIrConstant::Vec4(value) | MaterialIrConstant::ColorLinear(value) => {
            value.iter().all(|value| *value == 0.0)
        }
        MaterialIrConstant::Texture2D(_) | MaterialIrConstant::Bool(_) => false,
    }
}

fn is_one(value: &MaterialIrConstant) -> bool {
    match value {
        MaterialIrConstant::Float(value) => *value == 1.0,
        MaterialIrConstant::Vec2(value) => value.iter().all(|value| *value == 1.0),
        MaterialIrConstant::Vec3(value) => value.iter().all(|value| *value == 1.0),
        MaterialIrConstant::Vec4(value) | MaterialIrConstant::ColorLinear(value) => {
            value.iter().all(|value| *value == 1.0)
        }
        MaterialIrConstant::Texture2D(_) | MaterialIrConstant::Bool(_) => false,
    }
}

fn zero_constant(value_type: MaterialValueType) -> MaterialIrConstant {
    match value_type {
        MaterialValueType::Float => MaterialIrConstant::Float(0.0),
        MaterialValueType::Vec2 => MaterialIrConstant::Vec2([0.0; 2]),
        MaterialValueType::Vec3 => MaterialIrConstant::Vec3([0.0; 3]),
        MaterialValueType::Vec4 => MaterialIrConstant::Vec4([0.0; 4]),
        MaterialValueType::Color => MaterialIrConstant::ColorLinear([0.0; 4]),
        MaterialValueType::Texture2D(_) | MaterialValueType::Bool => {
            unreachable!("validated arithmetic only produces numeric values")
        }
    }
}
