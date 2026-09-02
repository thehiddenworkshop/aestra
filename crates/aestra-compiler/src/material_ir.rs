use aestra_core::{
    AssetId, MaterialExpressionId, MaterialParameterId, MaterialProgramId, ValidationReport,
    material::{
        MaterialDomain, MaterialEvaluationDomain, MaterialExpressionDomain, MaterialExpressionInfo,
        MaterialExpressionKind, MaterialInput, MaterialProgram, MaterialRenderStatePolicy,
        MaterialValue, MaterialValueType, MaterialVectorComponent,
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
    SampleTexture {
        texture: MaterialIrValueId,
        uv: MaterialIrValueId,
    },
    ExtractComponent {
        value: MaterialIrValueId,
        component: MaterialVectorComponent,
    },
}

impl MaterialIrInstruction {
    fn dependencies(&self) -> Vec<MaterialIrValueId> {
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
            Self::PanUv { uv, speed, time } => vec![*uv, *speed, *time],
            Self::RotateUv { uv, center, angle } => vec![*uv, *center, *angle],
            Self::ScaleUv { uv, center, scale } => vec![*uv, *center, *scale],
            Self::SampleTexture { texture, uv } => vec![*texture, *uv],
            Self::ExtractComponent { value, .. } => vec![*value],
        }
    }

    fn remap(&mut self, ids: &BTreeMap<MaterialIrValueId, MaterialIrValueId>) {
        let remap = |id: &mut MaterialIrValueId| {
            *id = ids[id];
        };
        match self {
            Self::Constant(_) | Self::Input(_) | Self::Parameter(_) => {}
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
            Self::SampleTexture { texture, uv } => {
                remap(texture);
                remap(uv);
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
        let normalized = program.normalized();
        let analysis = normalized
            .analyze()
            .map_err(MaterialCompileError::Validation)?;
        let expressions = normalized
            .expressions
            .iter()
            .map(|expression| (expression.id, &expression.kind))
            .collect::<BTreeMap<_, _>>();
        let mut builder = MaterialIrBuilder {
            analysis: &analysis.expressions,
            expressions,
            values: Vec::new(),
            source_map: MaterialIrSourceMap::default(),
            optimizations: MaterialIrOptimizationStats::default(),
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

        let (values, outputs, source_map, optimizations) =
            builder.finish(MaterialIrOutputs { color, alpha });
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

struct MaterialIrBuilder<'a> {
    analysis: &'a BTreeMap<MaterialExpressionId, MaterialExpressionInfo>,
    expressions: BTreeMap<MaterialExpressionId, &'a MaterialExpressionKind>,
    values: Vec<MaterialIrValue>,
    source_map: MaterialIrSourceMap,
    optimizations: MaterialIrOptimizationStats,
}

impl MaterialIrBuilder<'_> {
    fn lower(&mut self, expression: MaterialExpressionId) -> MaterialIrValueId {
        if let Some(id) = self.source_map.values.get(&expression) {
            return *id;
        }
        let kind = self.expressions[&expression].clone();
        let info = self.analysis[&expression];
        let instruction = match kind {
            MaterialExpressionKind::Constant(value) => {
                MaterialIrInstruction::Constant(lower_constant(&value))
            }
            MaterialExpressionKind::Input(input) => MaterialIrInstruction::Input(input),
            MaterialExpressionKind::Parameter(parameter) => {
                MaterialIrInstruction::Parameter(parameter)
            }
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
            MaterialExpressionKind::SampleTexture { texture, uv } => {
                let texture = self.lower(texture);
                let uv = self.lower(uv);
                MaterialIrInstruction::SampleTexture { texture, uv }
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
        let id = MaterialIrValueId(self.values.len() as u32);
        self.values.push(MaterialIrValue {
            id,
            value_type: info.value_type,
            evaluation_domain: info.evaluation_domain,
            instruction,
        });
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
