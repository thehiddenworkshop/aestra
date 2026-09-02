//! Engine-neutral projection of semantic material programs into an ordered modifier stack.

use crate::{MaterialCompileError, MaterialCompiler};
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpressionKind, MaterialProgram},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialStackModifierKind {
    BaseTexture,
    PanUv,
    RotateUv,
    ScaleUv,
    Remap,
    Smoothstep,
    RadialMask,
    Dissolve,
    DissolveEdge,
    DepthFade,
    SoftParticle,
}

impl MaterialStackModifierKind {
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::BaseTexture => "Base Texture",
            Self::PanUv => "UV Pan",
            Self::RotateUv => "UV Rotate",
            Self::ScaleUv => "UV Scale",
            Self::Remap => "Remap",
            Self::Smoothstep => "Smoothstep",
            Self::RadialMask => "Radial Mask",
            Self::Dissolve => "Dissolve",
            Self::DissolveEdge => "Dissolve Edge",
            Self::DepthFade => "Depth Fade",
            Self::SoftParticle => "Soft Particle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialStackEntry {
    /// Stable authored identity used by future stack edit commands.
    pub expression: MaterialExpressionId,
    pub kind: MaterialStackModifierKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialStackFallbackReason {
    /// One modifier consumes more than one independently transformed semantic branch, or feeds
    /// multiple later modifiers. Reordering it as a single stack row would change graph meaning.
    Branched { expression: MaterialExpressionId },
    /// The outputs contain multiple independent semantic chains.
    MultipleRoots {
        expressions: Vec<MaterialExpressionId>,
    },
}

impl MaterialStackFallbackReason {
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Branched { .. } => "Advanced · branched graph",
            Self::MultipleRoots { .. } => "Advanced · multiple modifier chains",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialStackProjection {
    Stack { entries: Vec<MaterialStackEntry> },
    Advanced { reason: MaterialStackFallbackReason },
}

impl MaterialCompiler {
    /// Projects the reachable semantic operations into a linear, source-to-output stack.
    ///
    /// Constants, parameters, inputs, and generic arithmetic remain implementation details. If
    /// semantic operations form multiple branches, the projection explicitly falls back to the
    /// advanced representation instead of implying an unsafe ordering.
    pub fn project_stack(
        &self,
        program: &MaterialProgram,
    ) -> Result<MaterialStackProjection, MaterialCompileError> {
        let program = program.normalized();
        program
            .analyze()
            .map_err(MaterialCompileError::Validation)?;
        let expressions = program
            .expressions
            .iter()
            .map(|expression| (expression.id, &expression.kind))
            .collect::<BTreeMap<_, _>>();
        let mut reachable = BTreeSet::new();
        collect_reachable(program.outputs.color, &expressions, &mut reachable);
        collect_reachable(program.outputs.alpha, &expressions, &mut reachable);
        let modifiers = reachable
            .iter()
            .filter_map(|id| modifier_kind(expressions[id]).map(|kind| (*id, kind)))
            .collect::<BTreeMap<_, _>>();

        if modifiers.is_empty() {
            return Ok(MaterialStackProjection::Stack {
                entries: Vec::new(),
            });
        }

        let modifier_ids = modifiers.keys().copied().collect::<BTreeSet<_>>();
        let mut upstream = BTreeMap::<MaterialExpressionId, BTreeSet<MaterialExpressionId>>::new();
        for modifier in modifier_ids.iter().copied() {
            let mut nearest = BTreeSet::new();
            for dependency in dependencies(expressions[&modifier]) {
                collect_nearest_modifiers(
                    dependency,
                    &expressions,
                    &modifier_ids,
                    &mut BTreeSet::new(),
                    &mut nearest,
                );
            }
            if nearest.len() > 1 {
                return Ok(MaterialStackProjection::Advanced {
                    reason: MaterialStackFallbackReason::Branched {
                        expression: modifier,
                    },
                });
            }
            upstream.insert(modifier, nearest);
        }

        let mut downstream = modifier_ids
            .iter()
            .map(|id| (*id, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for (consumer, producers) in &upstream {
            for producer in producers {
                downstream.get_mut(producer).unwrap().insert(*consumer);
            }
        }
        if let Some(expression) = downstream
            .iter()
            .find_map(|(id, consumers)| (consumers.len() > 1).then_some(*id))
        {
            return Ok(MaterialStackProjection::Advanced {
                reason: MaterialStackFallbackReason::Branched { expression },
            });
        }

        let roots = upstream
            .iter()
            .filter_map(|(id, producers)| producers.is_empty().then_some(*id))
            .collect::<Vec<_>>();
        if roots.len() != 1 {
            return Ok(MaterialStackProjection::Advanced {
                reason: MaterialStackFallbackReason::MultipleRoots { expressions: roots },
            });
        }

        let mut ordered = Vec::with_capacity(modifiers.len());
        let mut current = Some(roots[0]);
        while let Some(expression) = current {
            ordered.push(MaterialStackEntry {
                expression,
                kind: modifiers[&expression],
            });
            current = downstream[&expression].iter().next().copied();
        }
        debug_assert_eq!(ordered.len(), modifiers.len());
        Ok(MaterialStackProjection::Stack { entries: ordered })
    }
}

fn modifier_kind(kind: &MaterialExpressionKind) -> Option<MaterialStackModifierKind> {
    Some(match kind {
        MaterialExpressionKind::SampleTexture { .. } => MaterialStackModifierKind::BaseTexture,
        MaterialExpressionKind::PanUv { .. } => MaterialStackModifierKind::PanUv,
        MaterialExpressionKind::RotateUv { .. } => MaterialStackModifierKind::RotateUv,
        MaterialExpressionKind::ScaleUv { .. } => MaterialStackModifierKind::ScaleUv,
        MaterialExpressionKind::Remap { .. } => MaterialStackModifierKind::Remap,
        MaterialExpressionKind::Smoothstep { .. } => MaterialStackModifierKind::Smoothstep,
        MaterialExpressionKind::RadialMask { .. } => MaterialStackModifierKind::RadialMask,
        MaterialExpressionKind::Dissolve { .. } => MaterialStackModifierKind::Dissolve,
        MaterialExpressionKind::DissolveEdge { .. } => MaterialStackModifierKind::DissolveEdge,
        MaterialExpressionKind::DepthFade { .. } => MaterialStackModifierKind::DepthFade,
        MaterialExpressionKind::SoftParticle { .. } => MaterialStackModifierKind::SoftParticle,
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::Add(_, _)
        | MaterialExpressionKind::Subtract(_, _)
        | MaterialExpressionKind::Multiply(_, _)
        | MaterialExpressionKind::Divide(_, _)
        | MaterialExpressionKind::Lerp { .. }
        | MaterialExpressionKind::Clamp { .. }
        | MaterialExpressionKind::ExtractComponent { .. } => return None,
    })
}

fn collect_reachable(
    id: MaterialExpressionId,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpressionKind>,
    reachable: &mut BTreeSet<MaterialExpressionId>,
) {
    if !reachable.insert(id) {
        return;
    }
    for dependency in dependencies(expressions[&id]) {
        collect_reachable(dependency, expressions, reachable);
    }
}

fn collect_nearest_modifiers(
    id: MaterialExpressionId,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpressionKind>,
    modifiers: &BTreeSet<MaterialExpressionId>,
    visited: &mut BTreeSet<MaterialExpressionId>,
    nearest: &mut BTreeSet<MaterialExpressionId>,
) {
    if !visited.insert(id) {
        return;
    }
    if modifiers.contains(&id) {
        nearest.insert(id);
        return;
    }
    for dependency in dependencies(expressions[&id]) {
        collect_nearest_modifiers(dependency, expressions, modifiers, visited, nearest);
    }
}

fn dependencies(kind: &MaterialExpressionKind) -> Vec<MaterialExpressionId> {
    match kind {
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_) => Vec::new(),
        MaterialExpressionKind::Add(left, right)
        | MaterialExpressionKind::Subtract(left, right)
        | MaterialExpressionKind::Multiply(left, right)
        | MaterialExpressionKind::Divide(left, right) => vec![*left, *right],
        MaterialExpressionKind::Lerp { start, end, factor } => vec![*start, *end, *factor],
        MaterialExpressionKind::Clamp { value, min, max } => vec![*value, *min, *max],
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
        MaterialExpressionKind::SampleTexture { texture, uv } => vec![*texture, *uv],
        MaterialExpressionKind::ExtractComponent { value, .. } => vec![*value],
    }
}
