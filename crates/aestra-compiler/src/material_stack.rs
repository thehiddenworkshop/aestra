//! Engine-neutral projection of semantic material programs into an ordered modifier stack.

use crate::{MaterialCompileError, MaterialCompiler};
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpressionKind, MaterialProgram, MaterialValueType},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialStackMoveTarget {
    /// Final source-to-output stack index after the move.
    pub index: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackMovePlan {
    pub expression: MaterialExpressionId,
    pub from_index: usize,
    pub to_index: usize,
    /// Complete replacement preserving every program and expression identity.
    pub replacement: MaterialProgram,
}

#[derive(Debug, Error)]
pub enum MaterialStackMoveError {
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error("advanced material graphs cannot be reordered as a stack")]
    Advanced,
    #[error("material expression {expression} is not present in the projected stack")]
    ExpressionMissing { expression: MaterialExpressionId },
    #[error("stack target index {index} is outside the projected stack")]
    TargetOutOfBounds { index: usize },
    #[error(
        "material modifier cannot move to stack index {index} without changing graph type or domain"
    )]
    IncompatibleTarget { index: usize },
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

    /// Returns only destinations that can be represented by rewiring a direct, homogeneous
    /// modifier chain and that still pass the complete material validator.
    pub fn stack_move_targets(
        &self,
        program: &MaterialProgram,
        expression: MaterialExpressionId,
    ) -> Result<Vec<MaterialStackMoveTarget>, MaterialStackMoveError> {
        let entries = stack_entries(self.project_stack(program)?)?;
        let from_index = entries
            .iter()
            .position(|entry| entry.expression == expression)
            .ok_or(MaterialStackMoveError::ExpressionMissing { expression })?;
        let mut targets = Vec::new();
        for index in 0..entries.len() {
            if index != from_index
                && plan_stack_move_inner(program, &entries, from_index, index).is_some()
            {
                targets.push(MaterialStackMoveTarget { index });
            }
        }
        Ok(targets)
    }

    /// Plans a safe stack move without mutating the source program.
    ///
    /// The returned replacement can be committed as one `ReplaceMaterialProgram` authoring
    /// command, making undo restore the exact original graph. Stable expression IDs and authored
    /// expression storage order are preserved; only primary chain sockets and a direct terminal
    /// consumer/output are rewired.
    pub fn plan_stack_move(
        &self,
        program: &MaterialProgram,
        expression: MaterialExpressionId,
        target_index: usize,
    ) -> Result<MaterialStackMovePlan, MaterialStackMoveError> {
        let entries = stack_entries(self.project_stack(program)?)?;
        if target_index >= entries.len() {
            return Err(MaterialStackMoveError::TargetOutOfBounds {
                index: target_index,
            });
        }
        let from_index = entries
            .iter()
            .position(|entry| entry.expression == expression)
            .ok_or(MaterialStackMoveError::ExpressionMissing { expression })?;
        let replacement = plan_stack_move_inner(program, &entries, from_index, target_index)
            .ok_or(MaterialStackMoveError::IncompatibleTarget {
                index: target_index,
            })?;
        Ok(MaterialStackMovePlan {
            expression,
            from_index,
            to_index: target_index,
            replacement,
        })
    }
}

fn stack_entries(
    projection: MaterialStackProjection,
) -> Result<Vec<MaterialStackEntry>, MaterialStackMoveError> {
    match projection {
        MaterialStackProjection::Stack { entries } => Ok(entries),
        MaterialStackProjection::Advanced { .. } => Err(MaterialStackMoveError::Advanced),
    }
}

fn plan_stack_move_inner(
    program: &MaterialProgram,
    entries: &[MaterialStackEntry],
    from_index: usize,
    target_index: usize,
) -> Option<MaterialProgram> {
    if from_index == target_index {
        return None;
    }
    let analysis = program.analyze().ok()?;
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let selected = entries.get(from_index)?.expression;
    let selected_type = analysis.expressions.get(&selected)?.value_type;
    if primary_value_type(selected, &expressions, &analysis.expressions)? != selected_type {
        return None;
    }

    let mut first = from_index;
    while first > 0 {
        let producer = entries[first - 1].expression;
        let consumer = entries[first].expression;
        if !chain_compatible(
            producer,
            consumer,
            selected_type,
            &expressions,
            &analysis.expressions,
        ) {
            break;
        }
        first -= 1;
    }
    let mut last = from_index;
    while last + 1 < entries.len() {
        let producer = entries[last].expression;
        let consumer = entries[last + 1].expression;
        if !chain_compatible(
            producer,
            consumer,
            selected_type,
            &expressions,
            &analysis.expressions,
        ) {
            break;
        }
        last += 1;
    }
    if target_index < first || target_index > last || first == last {
        return None;
    }

    let chain = entries[first..=last]
        .iter()
        .map(|entry| entry.expression)
        .collect::<Vec<_>>();
    let old_tail = *chain.last()?;
    let base_source = primary_source(expressions[&chain[0]])?;
    let boundary = entries
        .get(last + 1)
        .map(|entry| entry.expression)
        .filter(|expression| primary_source(expressions[expression]) == Some(old_tail));

    for (index, expression) in chain.iter().copied().enumerate() {
        let expected_primary = chain.get(index + 1).copied().or(boundary);
        let allowed_references = usize::from(expected_primary.is_some())
            + usize::from(program.outputs.color == expression)
            + usize::from(program.outputs.alpha == expression);
        let actual_references = program
            .expressions
            .iter()
            .map(|candidate| {
                dependencies(&candidate.kind)
                    .into_iter()
                    .filter(|source| *source == expression)
                    .count()
            })
            .sum::<usize>()
            + usize::from(program.outputs.color == expression)
            + usize::from(program.outputs.alpha == expression);
        if actual_references != allowed_references || allowed_references == 0 {
            return None;
        }
    }

    let mut reordered = chain;
    let relative_from = from_index - first;
    let relative_target = target_index - first;
    let moved = reordered.remove(relative_from);
    reordered.insert(relative_target, moved);
    let new_tail = *reordered.last()?;

    let mut replacement = program.clone();
    for (index, expression) in reordered.iter().copied().enumerate() {
        let source = if index == 0 {
            base_source
        } else {
            reordered[index - 1]
        };
        let kind = replacement
            .expressions
            .iter_mut()
            .find(|candidate| candidate.id == expression)
            .map(|candidate| &mut candidate.kind)?;
        if !set_primary_source(kind, source) {
            return None;
        }
    }
    if let Some(boundary) = boundary {
        let kind = replacement
            .expressions
            .iter_mut()
            .find(|candidate| candidate.id == boundary)
            .map(|candidate| &mut candidate.kind)?;
        if !set_primary_source(kind, new_tail) {
            return None;
        }
    }
    if replacement.outputs.color == old_tail {
        replacement.outputs.color = new_tail;
    }
    if replacement.outputs.alpha == old_tail {
        replacement.outputs.alpha = new_tail;
    }
    replacement.analyze().ok()?;

    let MaterialStackProjection::Stack { entries: projected } =
        MaterialCompiler.project_stack(&replacement).ok()?
    else {
        return None;
    };
    let expected = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            if (first..=last).contains(&index) {
                reordered[index - first]
            } else {
                entry.expression
            }
        })
        .collect::<Vec<_>>();
    (projected.iter().map(|entry| entry.expression).eq(expected)).then_some(replacement)
}

fn chain_compatible(
    producer: MaterialExpressionId,
    consumer: MaterialExpressionId,
    value_type: MaterialValueType,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpressionKind>,
    analysis: &BTreeMap<MaterialExpressionId, aestra_core::material::MaterialExpressionInfo>,
) -> bool {
    analysis
        .get(&producer)
        .is_some_and(|info| info.value_type == value_type)
        && analysis
            .get(&consumer)
            .is_some_and(|info| info.value_type == value_type)
        && primary_source(expressions[&consumer]) == Some(producer)
        && primary_value_type(consumer, expressions, analysis) == Some(value_type)
}

fn primary_value_type(
    expression: MaterialExpressionId,
    expressions: &BTreeMap<MaterialExpressionId, &MaterialExpressionKind>,
    analysis: &BTreeMap<MaterialExpressionId, aestra_core::material::MaterialExpressionInfo>,
) -> Option<MaterialValueType> {
    primary_source(expressions[&expression])
        .and_then(|source| analysis.get(&source))
        .map(|info| info.value_type)
}

fn primary_source(kind: &MaterialExpressionKind) -> Option<MaterialExpressionId> {
    match kind {
        MaterialExpressionKind::Remap { value, .. }
        | MaterialExpressionKind::Smoothstep { value, .. } => Some(*value),
        MaterialExpressionKind::RadialMask { uv, .. }
        | MaterialExpressionKind::PanUv { uv, .. }
        | MaterialExpressionKind::RotateUv { uv, .. }
        | MaterialExpressionKind::ScaleUv { uv, .. }
        | MaterialExpressionKind::SampleTexture { uv, .. } => Some(*uv),
        MaterialExpressionKind::Dissolve { source, .. }
        | MaterialExpressionKind::DissolveEdge { source, .. } => Some(*source),
        MaterialExpressionKind::SoftParticle { alpha, .. } => Some(*alpha),
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::Add(_, _)
        | MaterialExpressionKind::Subtract(_, _)
        | MaterialExpressionKind::Multiply(_, _)
        | MaterialExpressionKind::Divide(_, _)
        | MaterialExpressionKind::Lerp { .. }
        | MaterialExpressionKind::Clamp { .. }
        | MaterialExpressionKind::DepthFade { .. }
        | MaterialExpressionKind::ExtractComponent { .. } => None,
    }
}

fn set_primary_source(kind: &mut MaterialExpressionKind, source: MaterialExpressionId) -> bool {
    match kind {
        MaterialExpressionKind::Remap { value, .. }
        | MaterialExpressionKind::Smoothstep { value, .. } => *value = source,
        MaterialExpressionKind::RadialMask { uv, .. }
        | MaterialExpressionKind::PanUv { uv, .. }
        | MaterialExpressionKind::RotateUv { uv, .. }
        | MaterialExpressionKind::ScaleUv { uv, .. }
        | MaterialExpressionKind::SampleTexture { uv, .. } => *uv = source,
        MaterialExpressionKind::Dissolve { source: value, .. }
        | MaterialExpressionKind::DissolveEdge { source: value, .. } => *value = source,
        MaterialExpressionKind::SoftParticle { alpha, .. } => *alpha = source,
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::Add(_, _)
        | MaterialExpressionKind::Subtract(_, _)
        | MaterialExpressionKind::Multiply(_, _)
        | MaterialExpressionKind::Divide(_, _)
        | MaterialExpressionKind::Lerp { .. }
        | MaterialExpressionKind::Clamp { .. }
        | MaterialExpressionKind::DepthFade { .. }
        | MaterialExpressionKind::ExtractComponent { .. } => return false,
    }
    true
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
