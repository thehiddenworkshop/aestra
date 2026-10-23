//! Deterministic CPU material evaluation shared by previews and bounded raster workers.
#[cfg(test)]
#[path = "tests/preview_pixels.rs"]
mod tests;
use crate::material_graph::MaterialGraphPreviewTarget;
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialInput, MaterialInstance,
        MaterialParameterValue, MaterialProgram, MaterialValue, MaterialValueType,
        MaterialVectorComponent,
    },
};
use bevy::{
    asset::RenderAssetUsages,
    image::ImageSampler,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::collections::{BTreeMap, BTreeSet};
pub(super) const MATERIAL_PREVIEW_SIZE: u32 = crate::feathers::node_graph::NODE_PREVIEW_SIZE as u32;

#[derive(Debug, Clone, Copy)]
pub(super) enum PreviewValue {
    Numeric([f32; 4], usize),
    Bool(bool),
    Texture,
}

impl PreviewValue {
    pub(super) fn scalar(value: f32) -> Self {
        Self::Numeric([value, value, value, value], 1)
    }

    pub(super) fn numeric(self) -> Option<([f32; 4], usize)> {
        match self {
            Self::Numeric(value, lanes) => Some((value, lanes)),
            Self::Bool(value) => Some(([if value { 1.0 } else { 0.0 }; 4], 1)),
            Self::Texture => None,
        }
    }

    pub(super) fn boolean(self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(value),
            Self::Numeric(value, _) => Some(value[0] >= 0.5),
            Self::Texture => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct MaterialPreviewContext {
    pub(super) uv: Vec2,
    pub(super) normal: Vec3,
}

pub(super) struct MaterialPreviewEvaluator<'a> {
    pub(super) program: &'a MaterialProgram,
    pub(super) instance: Option<&'a MaterialInstance>,
    /// The function library, so graph `FunctionCall` nodes can be inlined and previewed rather
    /// than showing as unrenderable. Custom-WESL functions still can't be interpreted on the CPU.
    pub(super) functions: &'a aestra_compiler::MaterialFunctionLibrary,
}

impl MaterialPreviewEvaluator<'_> {
    pub(super) fn evaluate(
        &self,
        expression: MaterialExpressionId,
        context: MaterialPreviewContext,
    ) -> Option<PreviewValue> {
        self.evaluate_inner(
            expression,
            &self.program.expressions,
            &BTreeMap::new(),
            context,
            &mut BTreeMap::new(),
            &mut BTreeSet::new(),
        )
    }

    /// Evaluates `expression` within a scope: the `expressions` being interpreted (the program or a
    /// function body) and `bindings` for that scope's `FunctionInput`s.
    pub(super) fn evaluate_inner(
        &self,
        expression: MaterialExpressionId,
        expressions: &[MaterialExpression],
        bindings: &BTreeMap<aestra_core::MaterialFunctionInputId, PreviewValue>,
        context: MaterialPreviewContext,
        memo: &mut BTreeMap<MaterialExpressionId, PreviewValue>,
        visiting: &mut BTreeSet<MaterialExpressionId>,
    ) -> Option<PreviewValue> {
        if let Some(value) = memo.get(&expression) {
            return Some(*value);
        }
        if !visiting.insert(expression) {
            return None;
        }
        let expression_value = expressions
            .iter()
            .find(|candidate| candidate.id == expression)?;
        if self.program.disabled_expressions.contains(&expression)
            && let Some(source) = expression_value.kind.bypass_input()
        {
            visiting.remove(&expression);
            return self.evaluate_inner(source, expressions, bindings, context, memo, visiting);
        }
        let mut read =
            |source| self.evaluate_inner(source, expressions, bindings, context, memo, visiting);
        let value = match &expression_value.kind {
            MaterialExpressionKind::Constant(value) => preview_value(value),
            MaterialExpressionKind::Input(input) => Some(preview_input(*input, context)),
            MaterialExpressionKind::Parameter(parameter) => self.parameter(*parameter),
            // A function body's input reads the value bound for it by the enclosing call.
            MaterialExpressionKind::FunctionInput(input) => bindings.get(input).copied(),
            // Inline a graph function: bind each input to its argument (or default), then
            // evaluate the selected output in the function's own scope with a fresh memo.
            MaterialExpressionKind::FunctionCall {
                function,
                arguments,
                output,
            } => {
                let function = self.functions.get(*function)?;
                if function.custom_wesl.is_some() {
                    // Raw WESL can't be interpreted on the CPU; leave it unrenderable.
                    None
                } else {
                    let mut argument_values = BTreeMap::new();
                    for input in &function.inputs {
                        let value = match arguments.get(&input.id) {
                            Some(argument) => read(*argument)?,
                            None => input.default.as_ref().and_then(preview_value)?,
                        };
                        argument_values.insert(input.id, value);
                    }
                    let output_expression = function
                        .outputs
                        .iter()
                        .find(|candidate| candidate.id == *output)?
                        .expression;
                    self.evaluate_inner(
                        output_expression,
                        &function.expressions,
                        &argument_values,
                        context,
                        &mut BTreeMap::new(),
                        &mut BTreeSet::new(),
                    )
                }
            }
            // Compiler-only form; never authored, and not interpretable here.
            MaterialExpressionKind::CustomWeslCall { .. } => None,
            MaterialExpressionKind::Add(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| left + right)
            }
            MaterialExpressionKind::Subtract(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| left - right)
            }
            MaterialExpressionKind::Multiply(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| left * right)
            }
            MaterialExpressionKind::Divide(left, right) => {
                preview_binary(read(*left)?, read(*right)?, |left, right| {
                    if right.abs() <= f32::EPSILON {
                        0.0
                    } else {
                        left / right
                    }
                })
            }
            MaterialExpressionKind::Lerp { start, end, factor } => {
                let factor = read(*factor)?.numeric()?.0;
                preview_ternary_numeric(read(*start)?, read(*end)?, |start, end, index| {
                    start + (end - start) * factor[index]
                })
            }
            MaterialExpressionKind::Clamp { value, min, max } => {
                let min = read(*min)?.numeric()?.0;
                let max = read(*max)?.numeric()?.0;
                preview_unary_numeric(read(*value)?, |value, index| {
                    value.clamp(min[index].min(max[index]), min[index].max(max[index]))
                })
            }
            MaterialExpressionKind::Select {
                condition,
                if_false,
                if_true,
            } => {
                let selected = if read(*condition)?.boolean()? {
                    *if_true
                } else {
                    *if_false
                };
                read(selected)
            }
            MaterialExpressionKind::Remap {
                value,
                input_min,
                input_max,
                output_min,
                output_max,
            } => {
                let input_min = read(*input_min)?.numeric()?.0;
                let input_max = read(*input_max)?.numeric()?.0;
                let output_min = read(*output_min)?.numeric()?.0;
                let output_max = read(*output_max)?.numeric()?.0;
                preview_unary_numeric(read(*value)?, |value, index| {
                    let denominator = input_max[index] - input_min[index];
                    if denominator.abs() <= f32::EPSILON {
                        output_min[index]
                    } else {
                        let t = (value - input_min[index]) / denominator;
                        output_min[index] + (output_max[index] - output_min[index]) * t
                    }
                })
            }
            MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value,
            } => {
                let edge_min = read(*edge_min)?.numeric()?.0;
                let edge_max = read(*edge_max)?.numeric()?.0;
                preview_unary_numeric(read(*value)?, |value, index| {
                    let denominator = edge_max[index] - edge_min[index];
                    if denominator.abs() <= f32::EPSILON {
                        if value >= edge_min[index] { 1.0 } else { 0.0 }
                    } else {
                        let t = ((value - edge_min[index]) / denominator).clamp(0.0, 1.0);
                        t * t * (3.0 - 2.0 * t)
                    }
                })
            }
            MaterialExpressionKind::NormalMap {
                sample,
                strength,
                flip_y,
                normal,
                tangent,
                bitangent,
            } => {
                let sample = read(*sample)?.numeric()?.0;
                let result = aestra_compiler::evaluate_normal_map(
                    [sample[0], sample[1], sample[2]],
                    read(*strength)?.numeric()?.0[0],
                    read(*flip_y)?.boolean()?,
                    preview_vec3(read(*normal)?)?.to_array(),
                    preview_vec3(read(*tangent)?)?.to_array(),
                    preview_vec3(read(*bitangent)?)?.to_array(),
                );
                Some(PreviewValue::Numeric(
                    [result[0], result[1], result[2], 1.0],
                    3,
                ))
            }
            MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            } => {
                let normal = preview_vec3(read(*normal)?)?.normalize_or_zero();
                let view = preview_vec3(read(*view)?)?.normalize_or_zero();
                let power = read(*power)?.numeric()?.0[0].max(0.0);
                Some(PreviewValue::scalar(
                    (1.0 - normal.dot(view).clamp(0.0, 1.0)).powf(power),
                ))
            }
            MaterialExpressionKind::RadialMask {
                uv,
                center,
                radius,
                softness,
                invert,
            } => {
                let uv = preview_vec2(read(*uv)?)?;
                let center = preview_vec2(read(*center)?)?;
                let radius = read(*radius)?.numeric()?.0[0].max(0.0);
                let softness = read(*softness)?.numeric()?.0[0].max(0.0);
                let invert = read(*invert)?.boolean()?;
                let distance = uv.distance(center);
                let mask = if softness <= f32::EPSILON {
                    if distance <= radius { 1.0 } else { 0.0 }
                } else {
                    let t = ((radius - distance) / softness).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                Some(PreviewValue::scalar(if invert { 1.0 - mask } else { mask }))
            }
            MaterialExpressionKind::Dissolve {
                source,
                threshold,
                edge_width,
                invert,
            } => {
                let source = read(*source)?.numeric()?.0[0];
                let threshold = read(*threshold)?.numeric()?.0[0];
                let edge = read(*edge_width)?.numeric()?.0[0].max(0.0);
                let invert = read(*invert)?.boolean()?;
                let mask = if edge <= f32::EPSILON {
                    if source >= threshold { 1.0 } else { 0.0 }
                } else {
                    ((source - threshold) / edge + 0.5).clamp(0.0, 1.0)
                };
                Some(PreviewValue::scalar(if invert { 1.0 - mask } else { mask }))
            }
            MaterialExpressionKind::DissolveEdge {
                source,
                threshold,
                edge_width,
                invert,
            } => {
                let source = read(*source)?.numeric()?.0[0];
                let threshold = read(*threshold)?.numeric()?.0[0];
                let edge = read(*edge_width)?.numeric()?.0[0].max(0.0);
                let invert = read(*invert)?.boolean()?;
                let distance = if invert {
                    threshold - source
                } else {
                    source - threshold
                };
                Some(PreviewValue::scalar(if edge <= f32::EPSILON {
                    0.0
                } else {
                    (1.0 - distance.abs() / edge).clamp(0.0, 1.0)
                }))
            }
            MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => Some(PreviewValue::scalar(preview_depth_fade(
                read(*scene_depth)?,
                read(*pixel_depth)?,
                read(*fade_distance)?,
                read(*invert)?,
            )?)),
            MaterialExpressionKind::SoftParticle {
                alpha,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            } => {
                let alpha = read(*alpha)?.numeric()?.0[0];
                Some(PreviewValue::scalar(
                    alpha
                        * preview_depth_fade(
                            read(*scene_depth)?,
                            read(*pixel_depth)?,
                            read(*fade_distance)?,
                            read(*invert)?,
                        )?,
                ))
            }
            MaterialExpressionKind::PanUv { uv, speed, time } => {
                let uv = preview_vec2(read(*uv)?)?;
                let speed = preview_vec2(read(*speed)?)?;
                let time = read(*time)?.numeric()?.0[0];
                Some(preview_vec2_value(uv + speed * time))
            }
            MaterialExpressionKind::RotateUv { uv, center, angle } => {
                let uv = preview_vec2(read(*uv)?)?;
                let center = preview_vec2(read(*center)?)?;
                let angle = read(*angle)?.numeric()?.0[0];
                let (sin, cos) = angle.sin_cos();
                let offset = uv - center;
                Some(preview_vec2_value(
                    center
                        + Vec2::new(
                            offset.x * cos - offset.y * sin,
                            offset.x * sin + offset.y * cos,
                        ),
                ))
            }
            MaterialExpressionKind::ScaleUv { uv, center, scale } => {
                let uv = preview_vec2(read(*uv)?)?;
                let center = preview_vec2(read(*center)?)?;
                let scale = preview_vec2(read(*scale)?)?;
                Some(preview_vec2_value(center + (uv - center) * scale))
            }
            MaterialExpressionKind::DerivativeX { value }
            | MaterialExpressionKind::DerivativeY { value } => {
                let (_, lanes) = read(*value)?.numeric()?;
                Some(PreviewValue::Numeric([0.0; 4], lanes))
            }
            MaterialExpressionKind::SampleTexture { texture, uv } => {
                let _texture = read(*texture)?;
                let uv = preview_vec2(read(*uv)?)?;
                Some(preview_checker_sample(uv, 0.0))
            }
            MaterialExpressionKind::SampleTextureLevel { texture, uv, level } => {
                let _texture = read(*texture)?;
                let uv = preview_vec2(read(*uv)?)?;
                let level = read(*level)?.numeric()?.0[0];
                Some(preview_checker_sample(uv, level))
            }
            MaterialExpressionKind::SampleTextureGradient {
                texture,
                uv,
                ddx,
                ddy,
            } => {
                let _texture = read(*texture)?;
                let uv = preview_vec2(read(*uv)?)?;
                let _ddx = preview_vec2(read(*ddx)?)?;
                let _ddy = preview_vec2(read(*ddy)?)?;
                Some(preview_checker_sample(uv, 0.0))
            }
            MaterialExpressionKind::ExtractComponent { value, component } => {
                let value = read(*value)?.numeric()?.0;
                let index = match component {
                    MaterialVectorComponent::X => 0,
                    MaterialVectorComponent::Y => 1,
                    MaterialVectorComponent::Z => 2,
                    MaterialVectorComponent::W => 3,
                };
                Some(PreviewValue::scalar(value[index]))
            }
        }?;
        visiting.remove(&expression);
        memo.insert(expression, value);
        Some(value)
    }

    pub(super) fn parameter(
        &self,
        parameter: aestra_core::MaterialParameterId,
    ) -> Option<PreviewValue> {
        let definition = self
            .program
            .parameters
            .iter()
            .find(|candidate| candidate.id == parameter)?;
        let authored = self
            .instance
            .and_then(|instance| instance.values.get(&parameter));
        match authored {
            Some(MaterialParameterValue::Constant(value)) => preview_value(value),
            Some(MaterialParameterValue::RandomRange { min, max, .. }) => {
                preview_binary(preview_value(min)?, preview_value(max)?, |min, max| {
                    (min + max) * 0.5
                })
            }
            Some(MaterialParameterValue::EffectParameter(_))
            | Some(MaterialParameterValue::EmitterParameter(_))
            | None => definition.default.as_ref().and_then(preview_value),
        }
    }
}

pub(super) fn preview_value(value: &MaterialValue) -> Option<PreviewValue> {
    Some(match value {
        MaterialValue::Float(value) => PreviewValue::scalar(*value),
        MaterialValue::Vec2(value) => PreviewValue::Numeric([value[0], value[1], 0.0, 1.0], 2),
        MaterialValue::Vec3(value) => PreviewValue::Numeric([value[0], value[1], value[2], 1.0], 3),
        MaterialValue::Vec4(value) => PreviewValue::Numeric(*value, 4),
        MaterialValue::ColorSrgb(value) => PreviewValue::Numeric(
            [
                srgb_to_linear(value[0]),
                srgb_to_linear(value[1]),
                srgb_to_linear(value[2]),
                value[3],
            ],
            4,
        ),
        MaterialValue::Texture2D(_) => PreviewValue::Texture,
        MaterialValue::Bool(value) => PreviewValue::Bool(*value),
    })
}

pub(super) fn preview_input(input: MaterialInput, context: MaterialPreviewContext) -> PreviewValue {
    match input {
        MaterialInput::RibbonUv => PreviewValue::Numeric([context.uv.x, context.uv.y, 0.0, 0.0], 2),
        MaterialInput::RibbonDirection => PreviewValue::Numeric([1.0, 0.0, 0.0, 0.0], 3),
        MaterialInput::Tangent | MaterialInput::Bitangent => {
            // Synthetic preview geometry has a right-handed basis, with no imported UV seams.
            let normal = context.normal.normalize_or_zero();
            let axis = if normal.y.abs() < 0.99 {
                Vec3::Y
            } else {
                Vec3::X
            };
            let tangent = axis.cross(normal).normalize_or_zero();
            let direction = if input == MaterialInput::Tangent {
                tangent
            } else {
                normal.cross(tangent)
            };
            PreviewValue::Numeric([direction.x, direction.y, direction.z, 1.0], 3)
        }
        MaterialInput::Uv0 | MaterialInput::Uv1 | MaterialInput::ScreenUv => {
            preview_vec2_value(context.uv)
        }
        MaterialInput::Normal | MaterialInput::CameraDirection | MaterialInput::ViewDirection => {
            PreviewValue::Numeric(
                if input == MaterialInput::ViewDirection {
                    [0.0, 0.0, 1.0, 1.0]
                } else {
                    [context.normal.x, context.normal.y, context.normal.z, 1.0]
                },
                3,
            )
        }
        MaterialInput::LocalPosition | MaterialInput::WorldPosition => PreviewValue::Numeric(
            [context.uv.x * 2.0 - 1.0, context.uv.y * 2.0 - 1.0, 0.0, 1.0],
            3,
        ),
        MaterialInput::ParticleColor => PreviewValue::Numeric(
            [
                context.uv.x,
                0.25 + context.uv.y * 0.55,
                1.0 - context.uv.x,
                1.0,
            ],
            4,
        ),
        MaterialInput::ParticleVelocity => PreviewValue::Numeric([1.0, 0.35, 0.0, 1.0], 3),
        MaterialInput::CameraPosition => PreviewValue::Numeric([0.0, 0.0, 3.0, 1.0], 3),
        MaterialInput::ParticleOpacity => PreviewValue::scalar(1.0),
        MaterialInput::ParticleNormalizedAge
        | MaterialInput::EffectNormalizedTime
        | MaterialInput::EmitterNormalizedTime => PreviewValue::scalar(context.uv.x),
        MaterialInput::SceneDepth => PreviewValue::scalar(1.0 + context.uv.x),
        MaterialInput::PixelDepth => PreviewValue::scalar(1.0),
        // CPU swatches do not have a scene-light provider. Do not counterfeit illumination.
        MaterialInput::ScenePointIrradiance => PreviewValue::Numeric([0.0, 0.0, 0.0, 1.0], 3),
        MaterialInput::ParticleRandom => PreviewValue::scalar(
            ((context.uv.x * 91.7).sin() * (context.uv.y * 43.1).cos() * 43758.547)
                .fract()
                .abs(),
        ),
        MaterialInput::ParticleAge | MaterialInput::EffectTime | MaterialInput::EmitterTime => {
            PreviewValue::scalar(1.0)
        }
        MaterialInput::ParticleLifetime => PreviewValue::scalar(2.0),
        MaterialInput::ParticleSpeed => PreviewValue::scalar(1.0),
        MaterialInput::ParticleId => PreviewValue::scalar((context.uv.x * 8.0).floor()),
        MaterialInput::ParticleSize => PreviewValue::scalar(1.0),
        MaterialInput::ParticleRotation => PreviewValue::scalar(0.0),
    }
}

pub(super) fn preview_binary(
    left: PreviewValue,
    right: PreviewValue,
    operation: impl Fn(f32, f32) -> f32,
) -> Option<PreviewValue> {
    let (left, left_lanes) = left.numeric()?;
    let (right, right_lanes) = right.numeric()?;
    let lanes = left_lanes.max(right_lanes);
    Some(PreviewValue::Numeric(
        std::array::from_fn(|index| {
            operation(
                left[index.min(left_lanes - 1)],
                right[index.min(right_lanes - 1)],
            )
        }),
        lanes,
    ))
}

pub(super) fn preview_unary_numeric(
    value: PreviewValue,
    operation: impl Fn(f32, usize) -> f32,
) -> Option<PreviewValue> {
    let (value, lanes) = value.numeric()?;
    Some(PreviewValue::Numeric(
        std::array::from_fn(|index| operation(value[index.min(lanes - 1)], index)),
        lanes,
    ))
}

pub(super) fn preview_ternary_numeric(
    left: PreviewValue,
    right: PreviewValue,
    operation: impl Fn(f32, f32, usize) -> f32,
) -> Option<PreviewValue> {
    let (left, left_lanes) = left.numeric()?;
    let (right, right_lanes) = right.numeric()?;
    let lanes = left_lanes.max(right_lanes);
    Some(PreviewValue::Numeric(
        std::array::from_fn(|index| {
            operation(
                left[index.min(left_lanes - 1)],
                right[index.min(right_lanes - 1)],
                index,
            )
        }),
        lanes,
    ))
}

pub(super) fn preview_vec2(value: PreviewValue) -> Option<Vec2> {
    let (value, lanes) = value.numeric()?;
    Some(Vec2::new(value[0], value[if lanes > 1 { 1 } else { 0 }]))
}

pub(super) fn preview_vec3(value: PreviewValue) -> Option<Vec3> {
    let (value, lanes) = value.numeric()?;
    Some(Vec3::new(
        value[0],
        value[if lanes > 1 { 1 } else { 0 }],
        value[if lanes > 2 { 2 } else { 0 }],
    ))
}

pub(super) fn preview_vec2_value(value: Vec2) -> PreviewValue {
    PreviewValue::Numeric([value.x, value.y, 0.0, 1.0], 2)
}

pub(super) fn preview_checker_sample(uv: Vec2, level: f32) -> PreviewValue {
    let frequency = 8.0 / 2.0_f32.powf(level.max(0.0));
    let cell = ((uv.x * frequency).floor() as i32 + (uv.y * frequency).floor() as i32) & 1;
    let color = if cell == 0 {
        [0.08, 0.20, 0.42, 1.0]
    } else {
        [0.88, 0.32, 0.08, 1.0]
    };
    PreviewValue::Numeric(color, 4)
}

pub(super) fn preview_depth_fade(
    scene_depth: PreviewValue,
    pixel_depth: PreviewValue,
    fade_distance: PreviewValue,
    invert: PreviewValue,
) -> Option<f32> {
    let scene = scene_depth.numeric()?.0[0];
    let pixel = pixel_depth.numeric()?.0[0];
    let distance = fade_distance.numeric()?.0[0];
    let fade = if distance <= f32::EPSILON {
        if scene >= pixel { 1.0 } else { 0.0 }
    } else {
        ((scene - pixel) / distance).clamp(0.0, 1.0)
    };
    Some(if invert.boolean()? { 1.0 - fade } else { fade })
}

pub(super) fn render_material_graph_preview(
    program: &MaterialProgram,
    instance: Option<&MaterialInstance>,
    functions: &aestra_compiler::MaterialFunctionLibrary,
    target: MaterialGraphPreviewTarget,
    value_type: Option<MaterialValueType>,
) -> Image {
    let rgba = render_material_preview_pixels(
        program,
        instance,
        functions,
        target,
        value_type,
        MATERIAL_PREVIEW_SIZE,
        false,
        || false,
    )
    .expect("unchecked previews cannot fail");
    let mut image = Image::new(
        Extent3d {
            width: MATERIAL_PREVIEW_SIZE,
            height: MATERIAL_PREVIEW_SIZE,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        rgba,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::linear();
    image
}

// Shared pixel path: graph/preset previews keep their existing permissive semantics;
// browser previews reject unsupported samples and can cancel between scanlines.
#[allow(
    clippy::too_many_arguments,
    reason = "Preserve the shared graph/preset/asset raster contract"
)]
pub(super) fn render_material_preview_pixels(
    program: &MaterialProgram,
    instance: Option<&MaterialInstance>,
    functions: &aestra_compiler::MaterialFunctionLibrary,
    target: MaterialGraphPreviewTarget,
    value_type: Option<MaterialValueType>,
    size: u32,
    strict: bool,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<u8>, String> {
    let evaluator = MaterialPreviewEvaluator {
        program,
        instance,
        functions,
    };
    let uses_sphere = preview_uses_surface_normal(program, target);
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        if cancelled() {
            return Err("Cancelled".into());
        }
        for x in 0..size {
            let uv = Vec2::new(
                (x as f32 + 0.5) / size as f32,
                1.0 - (y as f32 + 0.5) / size as f32,
            );
            let sphere = uv * 2.0 - Vec2::ONE;
            let radius_squared = sphere.length_squared();
            let inside = !uses_sphere || radius_squared <= 1.0;
            let normal = if radius_squared <= 1.0 {
                Vec3::new(sphere.x, sphere.y, (1.0 - radius_squared).sqrt())
            } else {
                Vec3::Z
            };
            let context = MaterialPreviewContext { uv, normal };
            let sample = if inside {
                match target {
                    MaterialGraphPreviewTarget::Expression(expression) => evaluator
                        .evaluate(expression, context)
                        .and_then(|value| preview_rgba(value, value_type)),
                    MaterialGraphPreviewTarget::Output => {
                        preview_output_rgba(&evaluator, program, context)
                    }
                }
            } else {
                None
            };
            if strict && inside && sample.is_none_or(|rgba| rgba.iter().any(|v| !v.is_finite())) {
                return Err("Material output cannot be evaluated with saved defaults".into());
            }
            let checker = if ((x / 10) + (y / 10)) & 1 == 0 {
                0.10
            } else {
                0.16
            };
            let color = sample.unwrap_or([checker, checker, checker, 1.0]);
            let alpha = color[3].clamp(0.0, 1.0);
            let composite = std::array::from_fn::<_, 3, _>(|channel| {
                color[channel].clamp(0.0, 1.0) * alpha + checker * (1.0 - alpha)
            });
            rgba.extend_from_slice(&[
                (composite[0] * 255.0).round() as u8,
                (composite[1] * 255.0).round() as u8,
                (composite[2] * 255.0).round() as u8,
                255,
            ]);
        }
    }
    Ok(rgba)
}

pub(super) fn preview_rgba(
    value: PreviewValue,
    value_type: Option<MaterialValueType>,
) -> Option<[f32; 4]> {
    match value {
        PreviewValue::Bool(value) => {
            let value = if value { 1.0 } else { 0.0 };
            Some([value, value, value, 1.0])
        }
        PreviewValue::Texture => None,
        PreviewValue::Numeric(value, lanes) => {
            let linear = match value_type {
                Some(MaterialValueType::Float) | Some(MaterialValueType::Bool) => {
                    [value[0], value[0], value[0], 1.0]
                }
                Some(MaterialValueType::Vec2) => [value[0], value[1], 0.0, 1.0],
                Some(MaterialValueType::Vec3) | Some(MaterialValueType::Color) => {
                    [value[0], value[1], value[2], 1.0]
                }
                Some(MaterialValueType::Vec4) => value,
                Some(MaterialValueType::Texture2D(_)) | None => match lanes {
                    1 => [value[0], value[0], value[0], 1.0],
                    2 => [value[0], value[1], 0.0, 1.0],
                    3 => [value[0], value[1], value[2], 1.0],
                    _ => value,
                },
            };
            Some([
                linear_to_srgb(linear[0]),
                linear_to_srgb(linear[1]),
                linear_to_srgb(linear[2]),
                linear[3],
            ])
        }
    }
}

pub(super) fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

pub(super) fn linear_to_srgb(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

pub(super) fn preview_uses_surface_normal(
    program: &MaterialProgram,
    target: MaterialGraphPreviewTarget,
) -> bool {
    fn visits_normal(
        program: &MaterialProgram,
        expression: MaterialExpressionId,
        visited: &mut BTreeSet<MaterialExpressionId>,
    ) -> bool {
        if !visited.insert(expression) {
            return false;
        }
        let Some(expression) = program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
        else {
            return false;
        };
        if matches!(
            expression.kind,
            MaterialExpressionKind::Input(MaterialInput::Normal | MaterialInput::ViewDirection)
                | MaterialExpressionKind::Fresnel { .. }
                | MaterialExpressionKind::NormalMap { .. }
        ) {
            return true;
        }
        preview_dependencies(&expression.kind)
            .into_iter()
            .any(|dependency| visits_normal(program, dependency, visited))
    }

    match target {
        MaterialGraphPreviewTarget::Expression(expression) => {
            visits_normal(program, expression, &mut BTreeSet::new())
        }
        MaterialGraphPreviewTarget::Output => {
            visits_normal(program, program.outputs.color, &mut BTreeSet::new())
                || visits_normal(program, program.outputs.alpha, &mut BTreeSet::new())
        }
    }
}

pub(super) fn preview_output_rgba(
    evaluator: &MaterialPreviewEvaluator<'_>,
    program: &MaterialProgram,
    context: MaterialPreviewContext,
) -> Option<[f32; 4]> {
    let color = evaluator.evaluate(program.outputs.color, context)?;
    let mut rgba = preview_rgba(color, Some(MaterialValueType::Color))?;
    rgba[3] = evaluator
        .evaluate(program.outputs.alpha, context)?
        .numeric()?
        .0[0]
        .clamp(0.0, 1.0);
    Some(rgba)
}

pub(super) fn preview_dependencies(kind: &MaterialExpressionKind) -> Vec<MaterialExpressionId> {
    match kind {
        MaterialExpressionKind::NormalMap {
            sample,
            strength,
            flip_y,
            normal,
            tangent,
            bitangent,
        } => vec![*sample, *strength, *flip_y, *normal, *tangent, *bitangent],
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::FunctionInput(_) => Vec::new(),
        MaterialExpressionKind::FunctionCall { arguments, .. } => {
            arguments.values().copied().collect()
        }
        MaterialExpressionKind::CustomWeslCall { arguments, .. } => arguments
            .iter()
            .map(|argument| argument.expression)
            .collect(),
        MaterialExpressionKind::Add(left, right)
        | MaterialExpressionKind::Subtract(left, right)
        | MaterialExpressionKind::Multiply(left, right)
        | MaterialExpressionKind::Divide(left, right) => vec![*left, *right],
        MaterialExpressionKind::Lerp { start, end, factor } => vec![*start, *end, *factor],
        MaterialExpressionKind::Clamp { value, min, max } => vec![*value, *min, *max],
        MaterialExpressionKind::Select {
            condition,
            if_false,
            if_true,
        } => vec![*condition, *if_false, *if_true],
        MaterialExpressionKind::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => vec![*value, *input_min, *input_max, *output_min, *output_max],
        MaterialExpressionKind::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => vec![*edge_min, *edge_max, *value],
        MaterialExpressionKind::Fresnel {
            normal,
            view,
            power,
        } => vec![*normal, *view, *power],
        MaterialExpressionKind::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => vec![*uv, *center, *radius, *softness, *invert],
        MaterialExpressionKind::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        }
        | MaterialExpressionKind::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => vec![*source, *threshold, *edge_width, *invert],
        MaterialExpressionKind::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![*scene_depth, *pixel_depth, *fade_distance, *invert],
        MaterialExpressionKind::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![*alpha, *scene_depth, *pixel_depth, *fade_distance, *invert],
        MaterialExpressionKind::PanUv { uv, speed, time } => vec![*uv, *speed, *time],
        MaterialExpressionKind::RotateUv { uv, center, angle } => vec![*uv, *center, *angle],
        MaterialExpressionKind::ScaleUv { uv, center, scale } => vec![*uv, *center, *scale],
        MaterialExpressionKind::DerivativeX { value }
        | MaterialExpressionKind::DerivativeY { value } => vec![*value],
        MaterialExpressionKind::SampleTexture { texture, uv } => vec![*texture, *uv],
        MaterialExpressionKind::SampleTextureLevel { texture, uv, level } => {
            vec![*texture, *uv, *level]
        }
        MaterialExpressionKind::SampleTextureGradient {
            texture,
            uv,
            ddx,
            ddy,
        } => vec![*texture, *uv, *ddx, *ddy],
        MaterialExpressionKind::ExtractComponent { value, .. } => vec![*value],
    }
}
