//! Engine-neutral projection of semantic material programs into an ordered modifier stack.

use crate::{MaterialCompileError, MaterialCompiler};
pub use aestra_core::material::{
    MaterialPresetCategory, MaterialPresetDefault, MaterialPresetDescriptor,
    MaterialPresetGraphNode, MaterialPresetGraphNodeKind, MaterialPresetGraphRecipe,
    MaterialPresetProgramOutput, MaterialPresetRecipe, MaterialPresetValueRef,
    MaterialStackModifierKind, MaterialStackProperty,
};
use aestra_core::{
    MaterialExpressionId, MaterialPresetId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialGraphFunction, MaterialInput,
        MaterialPresetSchemaVersion, MaterialProgram, MaterialValue, MaterialValueType,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialStackEntry {
    /// Stable authored identity used by future stack edit commands.
    pub expression: MaterialExpressionId,
    pub kind: MaterialStackModifierKind,
    pub enabled: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialStackInsertTarget {
    /// Final source-to-output stack index for the new modifier.
    pub index: usize,
    pub kind: MaterialStackModifierKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackInsertPlan {
    pub expression: MaterialExpressionId,
    pub index: usize,
    pub kind: MaterialStackModifierKind,
    pub replacement: MaterialProgram,
}

/// A typed modifier expression and its default support expressions, without any output rewiring.
///
/// This is the graph-authoring counterpart to stack insertion: callers may connect the returned
/// wrapper at any compatible semantic edge without requiring the whole program to be linear.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialExpressionWrapPlan {
    pub expression: MaterialExpressionId,
    pub kind: MaterialStackModifierKind,
    pub replacement: MaterialProgram,
}

#[derive(Debug, Error)]
pub enum MaterialExpressionWrapError {
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error("material expression {expression} is unavailable as a wrapper source")]
    SourceMissing { expression: MaterialExpressionId },
    #[error("{kind:?} cannot wrap material expression {expression}")]
    IncompatibleSource {
        kind: MaterialStackModifierKind,
        expression: MaterialExpressionId,
    },
}

pub const MATERIAL_PRESET_UV_DRIFT: MaterialPresetId =
    MaterialPresetId::from_u128(0xA357_0000_0000_4000_8000_0000_0000_0001);
pub const MATERIAL_PRESET_SOFT_DISSOLVE: MaterialPresetId =
    MaterialPresetId::from_u128(0xA357_0000_0000_4000_8000_0000_0000_0002);
pub const MATERIAL_PRESET_CONTRAST_SHAPE: MaterialPresetId =
    MaterialPresetId::from_u128(0xA357_0000_0000_4000_8000_0000_0000_0003);
pub const MATERIAL_PRESET_DISSOLVE: MaterialPresetId =
    MaterialPresetId::from_u128(0xA357_0000_0000_4000_8000_0000_0000_0004);

/// Extensible catalog shared by compiler validation, tools, and editor presentation.
#[derive(Debug, Clone, Default)]
pub struct MaterialPresetCatalog {
    presets: BTreeMap<MaterialPresetId, MaterialPresetDescriptor>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MaterialPresetCatalogError {
    #[error("material preset ID {preset} is already registered")]
    DuplicateId { preset: MaterialPresetId },
}

impl MaterialPresetCatalog {
    pub fn builtin() -> Self {
        let mut catalog = Self::default();
        for preset in builtin_material_presets() {
            catalog.register(preset);
        }
        catalog
    }

    pub fn register(
        &mut self,
        preset: MaterialPresetDescriptor,
    ) -> Option<MaterialPresetDescriptor> {
        self.presets.insert(preset.id, preset)
    }

    pub fn try_register(
        &mut self,
        preset: MaterialPresetDescriptor,
    ) -> Result<(), MaterialPresetCatalogError> {
        if self.presets.contains_key(&preset.id) {
            return Err(MaterialPresetCatalogError::DuplicateId { preset: preset.id });
        }
        self.presets.insert(preset.id, preset);
        Ok(())
    }

    pub fn with_project_presets(
        presets: impl IntoIterator<Item = MaterialPresetDescriptor>,
    ) -> Result<Self, MaterialPresetCatalogError> {
        let mut catalog = Self::builtin();
        for preset in presets {
            catalog.try_register(preset)?;
        }
        Ok(catalog)
    }

    pub fn get(&self, id: MaterialPresetId) -> Option<&MaterialPresetDescriptor> {
        self.presets.get(&id)
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &MaterialPresetDescriptor> {
        self.presets.values()
    }
}

pub fn builtin_material_presets() -> Vec<MaterialPresetDescriptor> {
    vec![
        MaterialPresetDescriptor {
            schema_version: MaterialPresetSchemaVersion::CURRENT,
            id: MATERIAL_PRESET_UV_DRIFT,
            display_name: "UV Drift".into(),
            description: "Adds directional UV motion with a subtle scale variation.".into(),
            category: MaterialPresetCategory::Motion,
            tags: vec!["animated".into(), "uv".into(), "drift".into()],
            recipe: MaterialPresetRecipe::Stack {
                modifiers: vec![
                    MaterialStackModifierKind::PanUv,
                    MaterialStackModifierKind::ScaleUv,
                ],
                defaults: vec![
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::Speed,
                        value: MaterialValue::Vec2([0.15, 0.05]),
                    },
                    MaterialPresetDefault {
                        step: 1,
                        property: MaterialStackProperty::Scale,
                        value: MaterialValue::Vec2([1.1, 1.1]),
                    },
                ],
            },
        },
        MaterialPresetDescriptor {
            schema_version: MaterialPresetSchemaVersion::CURRENT,
            id: MATERIAL_PRESET_SOFT_DISSOLVE,
            display_name: "Soft Dissolve".into(),
            description: "Combines a threshold dissolve with soft scene intersection.".into(),
            category: MaterialPresetCategory::Masking,
            tags: vec!["dissolve".into(), "soft-particle".into()],
            recipe: MaterialPresetRecipe::Stack {
                modifiers: vec![
                    MaterialStackModifierKind::Dissolve,
                    MaterialStackModifierKind::SoftParticle,
                ],
                defaults: vec![
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::Threshold,
                        value: MaterialValue::Float(0.45),
                    },
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::EdgeWidth,
                        value: MaterialValue::Float(0.08),
                    },
                    MaterialPresetDefault {
                        step: 1,
                        property: MaterialStackProperty::FadeDistance,
                        value: MaterialValue::Float(0.35),
                    },
                ],
            },
        },
        MaterialPresetDescriptor {
            schema_version: MaterialPresetSchemaVersion::CURRENT,
            id: MATERIAL_PRESET_CONTRAST_SHAPE,
            display_name: "Contrast Shape".into(),
            description: "Remaps a signal and applies a smooth contrast threshold.".into(),
            category: MaterialPresetCategory::Shaping,
            tags: vec!["contrast".into(), "mask".into(), "threshold".into()],
            recipe: MaterialPresetRecipe::Stack {
                modifiers: vec![
                    MaterialStackModifierKind::Remap,
                    MaterialStackModifierKind::Smoothstep,
                ],
                defaults: vec![
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::InputMinimum,
                        value: MaterialValue::Float(0.1),
                    },
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::InputMaximum,
                        value: MaterialValue::Float(0.9),
                    },
                    MaterialPresetDefault {
                        step: 1,
                        property: MaterialStackProperty::EdgeMinimum,
                        value: MaterialValue::Float(0.2),
                    },
                    MaterialPresetDefault {
                        step: 1,
                        property: MaterialStackProperty::EdgeMaximum,
                        value: MaterialValue::Float(0.8),
                    },
                ],
            },
        },
        MaterialPresetDescriptor {
            schema_version: MaterialPresetSchemaVersion::CURRENT,
            id: MATERIAL_PRESET_DISSOLVE,
            display_name: "Dissolve".into(),
            description: "Adds an artist-adjustable threshold with a narrow transition edge."
                .into(),
            category: MaterialPresetCategory::Masking,
            tags: vec!["dissolve".into(), "threshold".into(), "transition".into()],
            recipe: MaterialPresetRecipe::Stack {
                modifiers: vec![MaterialStackModifierKind::Dissolve],
                defaults: vec![
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::Threshold,
                        value: MaterialValue::Float(0.5),
                    },
                    MaterialPresetDefault {
                        step: 0,
                        property: MaterialStackProperty::EdgeWidth,
                        value: MaterialValue::Float(0.06),
                    },
                ],
            },
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialStackPresetTarget {
    pub index: usize,
    pub preset: MaterialPresetId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackPresetPlan {
    pub preset: MaterialPresetId,
    pub index: usize,
    pub expressions: Vec<MaterialExpressionId>,
    pub replacement: MaterialProgram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackRemovePlan {
    pub expression: MaterialExpressionId,
    pub index: usize,
    pub replacement: MaterialProgram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackEnabledPlan {
    pub expression: MaterialExpressionId,
    pub index: usize,
    pub enabled: bool,
    pub replacement: MaterialProgram,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackPropertyDescriptor {
    pub property: MaterialStackProperty,
    pub name: &'static str,
    pub value: MaterialValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialStackPropertyEditPlan {
    pub expression: MaterialExpressionId,
    pub property: MaterialStackProperty,
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

#[derive(Debug, Error)]
pub enum MaterialStackEditError {
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error("advanced material graphs cannot be edited as a stack")]
    Advanced,
    #[error("material expression {expression} is not present in the projected stack")]
    ExpressionMissing { expression: MaterialExpressionId },
    #[error("stack insertion index {index} is outside the projected stack")]
    TargetOutOfBounds { index: usize },
    #[error("{kind:?} cannot be inserted at stack index {index} without changing graph meaning")]
    IncompatibleInsertion {
        kind: MaterialStackModifierKind,
        index: usize,
    },
    #[error(
        "preset {preset:?} cannot be inserted at stack index {index} without changing graph meaning"
    )]
    IncompatiblePreset {
        preset: MaterialPresetId,
        index: usize,
    },
    #[error("material preset {preset} is not registered")]
    UnknownPreset { preset: MaterialPresetId },
    #[error("material modifier {expression} cannot be removed without changing graph meaning")]
    IncompatibleRemoval { expression: MaterialExpressionId },
    #[error("material modifier {expression} cannot be {operation} without changing graph meaning")]
    IncompatibleEnabledState {
        expression: MaterialExpressionId,
        operation: &'static str,
    },
    #[error("material modifier {expression} does not expose property {property:?}")]
    PropertyUnavailable {
        expression: MaterialExpressionId,
        property: MaterialStackProperty,
    },
    #[error("material modifier property {property:?} is not backed by an editable constant")]
    PropertyNotConstant { property: MaterialStackProperty },
    #[error("material modifier property {property:?} rejects the supplied value type")]
    PropertyTypeMismatch { property: MaterialStackProperty },
}

impl MaterialCompiler {
    pub fn material_preset_catalog(&self) -> MaterialPresetCatalog {
        MaterialPresetCatalog::builtin()
    }

    /// Creates one typed wrapper with useful defaults around `source` without changing a consumer.
    /// This remains valid for branched graphs; the authoring layer chooses and rewires one edge.
    pub fn plan_expression_wrap(
        &self,
        program: &MaterialProgram,
        kind: MaterialStackModifierKind,
        source: MaterialExpressionId,
    ) -> Result<MaterialExpressionWrapPlan, MaterialExpressionWrapError> {
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == source)
        {
            return Err(MaterialExpressionWrapError::SourceMissing { expression: source });
        }
        let mut replacement = program.clone();
        let expression = append_default_modifier(&mut replacement, kind, source).ok_or(
            MaterialExpressionWrapError::IncompatibleSource {
                kind,
                expression: source,
            },
        )?;
        self.compile(&replacement)?;
        Ok(MaterialExpressionWrapPlan {
            expression,
            kind,
            replacement,
        })
    }

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
                enabled: !program.disabled_expressions.contains(&expression),
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

    /// Reports every modifier and insertion edge that produces a valid linear stack.
    pub fn stack_insert_targets(
        &self,
        program: &MaterialProgram,
    ) -> Result<Vec<MaterialStackInsertTarget>, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        let mut targets = Vec::new();
        for index in 0..=entries.len() {
            for kind in MaterialStackModifierKind::INSERTABLE {
                if plan_stack_insert_inner(program, &entries, kind, index).is_some() {
                    targets.push(MaterialStackInsertTarget { index, kind });
                }
            }
        }
        Ok(targets)
    }

    /// Creates a modifier with useful defaults and inserts it at one compiler-approved edge.
    pub fn plan_stack_insert(
        &self,
        program: &MaterialProgram,
        kind: MaterialStackModifierKind,
        index: usize,
    ) -> Result<MaterialStackInsertPlan, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        if index > entries.len() {
            return Err(MaterialStackEditError::TargetOutOfBounds { index });
        }
        let (expression, replacement) = plan_stack_insert_inner(program, &entries, kind, index)
            .ok_or(MaterialStackEditError::IncompatibleInsertion { kind, index })?;
        Ok(MaterialStackInsertPlan {
            expression,
            index,
            kind,
            replacement,
        })
    }

    /// Reports preset/edge pairs whose complete modifier chain remains a valid linear stack.
    pub fn stack_preset_targets(
        &self,
        program: &MaterialProgram,
    ) -> Result<Vec<MaterialStackPresetTarget>, MaterialStackEditError> {
        let catalog = self.material_preset_catalog();
        self.stack_preset_targets_with_catalog(program, &catalog)
    }

    /// Reports compatible preset/edge pairs from an explicit extensible catalog.
    pub fn stack_preset_targets_with_catalog(
        &self,
        program: &MaterialProgram,
        catalog: &MaterialPresetCatalog,
    ) -> Result<Vec<MaterialStackPresetTarget>, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        let mut targets = Vec::new();
        for index in 0..=entries.len() {
            for preset in catalog.iter() {
                if plan_stack_preset_inner(program, &entries, preset, index).is_some() {
                    targets.push(MaterialStackPresetTarget {
                        index,
                        preset: preset.id,
                    });
                }
            }
        }
        Ok(targets)
    }

    /// Inserts and configures a complete preset as one validated program replacement.
    pub fn plan_stack_insert_preset(
        &self,
        program: &MaterialProgram,
        preset: MaterialPresetId,
        index: usize,
    ) -> Result<MaterialStackPresetPlan, MaterialStackEditError> {
        let catalog = self.material_preset_catalog();
        self.plan_stack_insert_preset_with_catalog(program, &catalog, preset, index)
    }

    /// Inserts a preset resolved from an explicit extensible catalog.
    pub fn plan_stack_insert_preset_with_catalog(
        &self,
        program: &MaterialProgram,
        catalog: &MaterialPresetCatalog,
        preset: MaterialPresetId,
        index: usize,
    ) -> Result<MaterialStackPresetPlan, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        if index > entries.len() {
            return Err(MaterialStackEditError::TargetOutOfBounds { index });
        }
        let descriptor = catalog
            .get(preset)
            .ok_or(MaterialStackEditError::UnknownPreset { preset })?;
        let (expressions, replacement) =
            plan_stack_preset_inner(program, &entries, descriptor, index)
                .ok_or(MaterialStackEditError::IncompatiblePreset { preset, index })?;
        Ok(MaterialStackPresetPlan {
            preset,
            index,
            expressions,
            replacement,
        })
    }

    /// Removes a modifier and reconnects its direct primary input to its consumer or output.
    pub fn plan_stack_remove(
        &self,
        program: &MaterialProgram,
        expression: MaterialExpressionId,
    ) -> Result<MaterialStackRemovePlan, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        let index = entries
            .iter()
            .position(|entry| entry.expression == expression)
            .ok_or(MaterialStackEditError::ExpressionMissing { expression })?;
        let replacement = plan_stack_remove_inner(program, &entries, index)
            .ok_or(MaterialStackEditError::IncompatibleRemoval { expression })?;
        Ok(MaterialStackRemovePlan {
            expression,
            index,
            replacement,
        })
    }

    /// Enables or bypasses a modifier without deleting its semantic identity or settings.
    pub fn plan_stack_set_enabled(
        &self,
        program: &MaterialProgram,
        expression: MaterialExpressionId,
        enabled: bool,
    ) -> Result<MaterialStackEnabledPlan, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        let index = entries
            .iter()
            .position(|entry| entry.expression == expression)
            .ok_or(MaterialStackEditError::ExpressionMissing { expression })?;
        let mut replacement = program.clone();
        if enabled {
            replacement
                .disabled_expressions
                .retain(|candidate| *candidate != expression);
        } else if !replacement.disabled_expressions.contains(&expression) {
            replacement.disabled_expressions.push(expression);
        }
        replacement
            .analyze()
            .map_err(|_| MaterialStackEditError::IncompatibleEnabledState {
                expression,
                operation: if enabled { "enabled" } else { "disabled" },
            })?;
        let projected = editable_stack_entries(self.project_stack(&replacement)?)?;
        if projected
            .iter()
            .map(|entry| entry.expression)
            .ne(entries.iter().map(|entry| entry.expression))
        {
            return Err(MaterialStackEditError::IncompatibleEnabledState {
                expression,
                operation: if enabled { "enabled" } else { "disabled" },
            });
        }
        Ok(MaterialStackEnabledPlan {
            expression,
            index,
            enabled,
            replacement,
        })
    }

    /// Reflects the literal settings owned by one projected modifier.
    ///
    /// Parameter- or input-driven sockets remain part of the semantic graph and are intentionally
    /// omitted: the stack inspector only claims ownership of authored constants.
    pub fn stack_modifier_properties(
        &self,
        program: &MaterialProgram,
        expression: MaterialExpressionId,
    ) -> Result<Vec<MaterialStackPropertyDescriptor>, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        if !entries.iter().any(|entry| entry.expression == expression) {
            return Err(MaterialStackEditError::ExpressionMissing { expression });
        }
        let operation = program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
            .ok_or(MaterialStackEditError::ExpressionMissing { expression })?;
        Ok(modifier_property_targets(&operation.kind)
            .into_iter()
            .filter_map(|(property, target)| {
                let value = program
                    .expressions
                    .iter()
                    .find(|candidate| candidate.id == target)
                    .and_then(|candidate| match &candidate.kind {
                        MaterialExpressionKind::Constant(value) => Some(value.clone()),
                        _ => None,
                    })?;
                Some(MaterialStackPropertyDescriptor {
                    property,
                    name: property.display_name(),
                    value,
                })
            })
            .collect())
    }

    /// Replaces one reflected literal setting while preserving all expression identities.
    pub fn plan_stack_set_property(
        &self,
        program: &MaterialProgram,
        expression: MaterialExpressionId,
        property: MaterialStackProperty,
        value: MaterialValue,
    ) -> Result<MaterialStackPropertyEditPlan, MaterialStackEditError> {
        let entries = editable_stack_entries(self.project_stack(program)?)?;
        if !entries.iter().any(|entry| entry.expression == expression) {
            return Err(MaterialStackEditError::ExpressionMissing { expression });
        }
        let operation = program
            .expressions
            .iter()
            .find(|candidate| candidate.id == expression)
            .ok_or(MaterialStackEditError::ExpressionMissing { expression })?;
        let target = modifier_property_targets(&operation.kind)
            .into_iter()
            .find_map(|(candidate, target)| (candidate == property).then_some(target))
            .ok_or(MaterialStackEditError::PropertyUnavailable {
                expression,
                property,
            })?;
        let mut replacement = program.clone();
        let target = replacement
            .expressions
            .iter_mut()
            .find(|candidate| candidate.id == target)
            .ok_or(MaterialStackEditError::PropertyNotConstant { property })?;
        let MaterialExpressionKind::Constant(current) = &mut target.kind else {
            return Err(MaterialStackEditError::PropertyNotConstant { property });
        };
        if !current.has_same_type(&value) || !value.is_valid() {
            return Err(MaterialStackEditError::PropertyTypeMismatch { property });
        }
        *current = value;
        replacement
            .analyze()
            .map_err(MaterialCompileError::Validation)?;
        let projected = editable_stack_entries(self.project_stack(&replacement)?)?;
        if projected
            .iter()
            .map(|entry| entry.expression)
            .ne(entries.iter().map(|entry| entry.expression))
        {
            return Err(MaterialStackEditError::PropertyUnavailable {
                expression,
                property,
            });
        }
        Ok(MaterialStackPropertyEditPlan {
            expression,
            property,
            replacement,
        })
    }
}

fn modifier_property_targets(
    kind: &MaterialExpressionKind,
) -> Vec<(MaterialStackProperty, MaterialExpressionId)> {
    match kind {
        MaterialExpressionKind::PanUv { speed, .. } => {
            vec![(MaterialStackProperty::Speed, *speed)]
        }
        MaterialExpressionKind::RotateUv { center, angle, .. } => vec![
            (MaterialStackProperty::Center, *center),
            (MaterialStackProperty::Angle, *angle),
        ],
        MaterialExpressionKind::ScaleUv { center, scale, .. } => vec![
            (MaterialStackProperty::Center, *center),
            (MaterialStackProperty::Scale, *scale),
        ],
        MaterialExpressionKind::Remap {
            input_min,
            input_max,
            output_min,
            output_max,
            ..
        } => vec![
            (MaterialStackProperty::InputMinimum, *input_min),
            (MaterialStackProperty::InputMaximum, *input_max),
            (MaterialStackProperty::OutputMinimum, *output_min),
            (MaterialStackProperty::OutputMaximum, *output_max),
        ],
        MaterialExpressionKind::Smoothstep {
            edge_min, edge_max, ..
        } => vec![
            (MaterialStackProperty::EdgeMinimum, *edge_min),
            (MaterialStackProperty::EdgeMaximum, *edge_max),
        ],
        MaterialExpressionKind::Fresnel { power, .. } => {
            vec![(MaterialStackProperty::Power, *power)]
        }
        MaterialExpressionKind::RadialMask {
            center,
            radius,
            softness,
            invert,
            ..
        } => vec![
            (MaterialStackProperty::Center, *center),
            (MaterialStackProperty::Radius, *radius),
            (MaterialStackProperty::Softness, *softness),
            (MaterialStackProperty::Invert, *invert),
        ],
        MaterialExpressionKind::Dissolve {
            threshold,
            edge_width,
            invert,
            ..
        }
        | MaterialExpressionKind::DissolveEdge {
            threshold,
            edge_width,
            invert,
            ..
        } => vec![
            (MaterialStackProperty::Threshold, *threshold),
            (MaterialStackProperty::EdgeWidth, *edge_width),
            (MaterialStackProperty::Invert, *invert),
        ],
        MaterialExpressionKind::DepthFade {
            fade_distance,
            invert,
            ..
        }
        | MaterialExpressionKind::SoftParticle {
            fade_distance,
            invert,
            ..
        } => vec![
            (MaterialStackProperty::FadeDistance, *fade_distance),
            (MaterialStackProperty::Invert, *invert),
        ],
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::FunctionInput(_)
        | MaterialExpressionKind::FunctionCall { .. }
        | MaterialExpressionKind::Add(_, _)
        | MaterialExpressionKind::Subtract(_, _)
        | MaterialExpressionKind::Multiply(_, _)
        | MaterialExpressionKind::Divide(_, _)
        | MaterialExpressionKind::Lerp { .. }
        | MaterialExpressionKind::Clamp { .. }
        | MaterialExpressionKind::SampleTexture { .. }
        | MaterialExpressionKind::ExtractComponent { .. } => Vec::new(),
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

fn editable_stack_entries(
    projection: MaterialStackProjection,
) -> Result<Vec<MaterialStackEntry>, MaterialStackEditError> {
    match projection {
        MaterialStackProjection::Stack { entries } => Ok(entries),
        MaterialStackProjection::Advanced { .. } => Err(MaterialStackEditError::Advanced),
    }
}

fn plan_stack_preset_inner(
    program: &MaterialProgram,
    entries: &[MaterialStackEntry],
    preset: &MaterialPresetDescriptor,
    index: usize,
) -> Option<(Vec<MaterialExpressionId>, MaterialProgram)> {
    match &preset.recipe {
        MaterialPresetRecipe::Stack {
            modifiers,
            defaults,
        } => plan_stack_recipe_inner(program, entries, modifiers, defaults, index),
        MaterialPresetRecipe::Graph(recipe) => {
            plan_graph_recipe_inner(program, entries, recipe, index)
        }
    }
}

fn plan_stack_recipe_inner(
    program: &MaterialProgram,
    entries: &[MaterialStackEntry],
    modifiers: &[MaterialStackModifierKind],
    defaults: &[MaterialPresetDefault],
    index: usize,
) -> Option<(Vec<MaterialExpressionId>, MaterialProgram)> {
    let mut replacement = program.clone();
    let mut projected = entries.to_vec();
    let mut expressions = Vec::with_capacity(modifiers.len());
    for (offset, kind) in modifiers.iter().copied().enumerate() {
        let insertion_index = index + offset;
        let (expression, next) =
            plan_stack_insert_inner(&replacement, &projected, kind, insertion_index)?;
        replacement = next;
        expressions.push(expression);
        projected =
            editable_stack_entries(MaterialCompiler.project_stack(&replacement).ok()?).ok()?;
    }
    apply_preset_defaults(&mut replacement, defaults, &expressions)?;
    replacement.analyze().ok()?;
    let projected =
        editable_stack_entries(MaterialCompiler.project_stack(&replacement).ok()?).ok()?;
    let mut expected = entries
        .iter()
        .map(|entry| entry.expression)
        .collect::<Vec<_>>();
    expected.splice(index..index, expressions.iter().copied());
    projected
        .iter()
        .map(|entry| entry.expression)
        .eq(expected)
        .then_some((expressions, replacement))
}

fn apply_preset_defaults(
    program: &mut MaterialProgram,
    defaults: &[MaterialPresetDefault],
    expressions: &[MaterialExpressionId],
) -> Option<()> {
    for default in defaults {
        let expression = *expressions.get(default.step)?;
        set_modifier_constant(program, expression, default.property, default.value.clone())?;
    }
    Some(())
}

fn plan_graph_recipe_inner(
    program: &MaterialProgram,
    entries: &[MaterialStackEntry],
    recipe: &MaterialPresetGraphRecipe,
    index: usize,
) -> Option<(Vec<MaterialExpressionId>, MaterialProgram)> {
    if entries.is_empty() || index > entries.len() {
        return None;
    }
    let existing = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let source = if index == 0 {
        primary_source(existing[&entries[0].expression])?
    } else {
        entries[index - 1].expression
    };
    let boundary = entries.get(index).map(|entry| entry.expression);
    if boundary.is_some_and(|expression| primary_source(existing[&expression]) != Some(source)) {
        return None;
    }
    let allowed_references = usize::from(boundary.is_some())
        + usize::from(program.outputs.color == source)
        + usize::from(program.outputs.alpha == source);
    if allowed_references == 0 || expression_reference_count(program, source) != allowed_references
    {
        return None;
    }

    let original_color = program.outputs.color;
    let original_alpha = program.outputs.alpha;
    let mut replacement = program.clone();
    let mut nodes = BTreeMap::new();
    let mut created = Vec::with_capacity(recipe.nodes.len());
    for node in &recipe.nodes {
        let kind = match &node.kind {
            MaterialPresetGraphNodeKind::Constant(value) => {
                MaterialExpressionKind::Constant(value.clone())
            }
            MaterialPresetGraphNodeKind::Input(input) => MaterialExpressionKind::Input(*input),
            MaterialPresetGraphNodeKind::Function(function) => graph_recipe_function(
                *function,
                &node.inputs,
                &nodes,
                source,
                original_color,
                original_alpha,
            )?,
        };
        let expression = append_expression(&mut replacement, kind);
        nodes.insert(node.name.clone(), expression);
        created.push(expression);
    }
    let output = resolve_preset_value(
        &recipe.output,
        &nodes,
        source,
        original_color,
        original_alpha,
    )?;
    if let Some(boundary) = boundary {
        let boundary = replacement
            .expressions
            .iter_mut()
            .find(|candidate| candidate.id == boundary)?;
        if !set_primary_source(&mut boundary.kind, output) {
            return None;
        }
    }
    if replacement.outputs.color == source {
        replacement.outputs.color = output;
    }
    if replacement.outputs.alpha == source {
        replacement.outputs.alpha = output;
    }
    for (target, value) in &recipe.program_outputs {
        let expression =
            resolve_preset_value(value, &nodes, source, original_color, original_alpha)?;
        match target {
            MaterialPresetProgramOutput::Color => replacement.outputs.color = expression,
            MaterialPresetProgramOutput::Alpha => replacement.outputs.alpha = expression,
        }
    }
    replacement.analyze().ok()?;
    Some((created, replacement))
}

fn resolve_preset_value(
    value: &MaterialPresetValueRef,
    nodes: &BTreeMap<String, MaterialExpressionId>,
    source: MaterialExpressionId,
    program_color: MaterialExpressionId,
    program_alpha: MaterialExpressionId,
) -> Option<MaterialExpressionId> {
    match value {
        MaterialPresetValueRef::Source => Some(source),
        MaterialPresetValueRef::ProgramColor => Some(program_color),
        MaterialPresetValueRef::ProgramAlpha => Some(program_alpha),
        MaterialPresetValueRef::Node(name) => nodes.get(name).copied(),
    }
}

fn graph_recipe_function(
    function: MaterialGraphFunction,
    inputs: &BTreeMap<String, MaterialPresetValueRef>,
    nodes: &BTreeMap<String, MaterialExpressionId>,
    source: MaterialExpressionId,
    program_color: MaterialExpressionId,
    program_alpha: MaterialExpressionId,
) -> Option<MaterialExpressionKind> {
    let input = |name: &str| {
        resolve_preset_value(
            inputs.get(name)?,
            nodes,
            source,
            program_color,
            program_alpha,
        )
    };
    Some(match function {
        MaterialGraphFunction::Add => MaterialExpressionKind::Add(input("A")?, input("B")?),
        MaterialGraphFunction::Subtract => {
            MaterialExpressionKind::Subtract(input("A")?, input("B")?)
        }
        MaterialGraphFunction::Multiply => {
            MaterialExpressionKind::Multiply(input("A")?, input("B")?)
        }
        MaterialGraphFunction::Divide => MaterialExpressionKind::Divide(input("A")?, input("B")?),
        MaterialGraphFunction::Lerp => MaterialExpressionKind::Lerp {
            start: input("Start")?,
            end: input("End")?,
            factor: input("Factor")?,
        },
        MaterialGraphFunction::Clamp => MaterialExpressionKind::Clamp {
            value: input("Value")?,
            min: input("Minimum")?,
            max: input("Maximum")?,
        },
        MaterialGraphFunction::Remap => MaterialExpressionKind::Remap {
            value: input("Value")?,
            input_min: input("SourceMinimum")?,
            input_max: input("SourceMaximum")?,
            output_min: input("TargetMinimum")?,
            output_max: input("TargetMaximum")?,
        },
        MaterialGraphFunction::Smoothstep => MaterialExpressionKind::Smoothstep {
            edge_min: input("LowerEdge")?,
            edge_max: input("UpperEdge")?,
            value: input("Value")?,
        },
        MaterialGraphFunction::Fresnel => MaterialExpressionKind::Fresnel {
            normal: input("Normal")?,
            view: input("ViewDirection")?,
            power: input("Power")?,
        },
        MaterialGraphFunction::RadialMask => MaterialExpressionKind::RadialMask {
            uv: input("Uv")?,
            center: input("Center")?,
            radius: input("Radius")?,
            softness: input("Softness")?,
            invert: input("Invert")?,
        },
        MaterialGraphFunction::Dissolve => MaterialExpressionKind::Dissolve {
            source: input("Source")?,
            threshold: input("Threshold")?,
            edge_width: input("EdgeWidth")?,
            invert: input("Invert")?,
        },
        MaterialGraphFunction::DissolveEdge => MaterialExpressionKind::DissolveEdge {
            source: input("Source")?,
            threshold: input("Threshold")?,
            edge_width: input("EdgeWidth")?,
            invert: input("Invert")?,
        },
        MaterialGraphFunction::DepthFade => MaterialExpressionKind::DepthFade {
            scene_depth: input("SceneDepth")?,
            pixel_depth: input("PixelDepth")?,
            fade_distance: input("FadeDistance")?,
            invert: input("Invert")?,
        },
        MaterialGraphFunction::SoftParticle => MaterialExpressionKind::SoftParticle {
            alpha: input("Alpha")?,
            scene_depth: input("SceneDepth")?,
            pixel_depth: input("PixelDepth")?,
            fade_distance: input("FadeDistance")?,
            invert: input("Invert")?,
        },
        MaterialGraphFunction::PanUv => MaterialExpressionKind::PanUv {
            uv: input("Uv")?,
            speed: input("Speed")?,
            time: input("Time")?,
        },
        MaterialGraphFunction::RotateUv => MaterialExpressionKind::RotateUv {
            uv: input("Uv")?,
            center: input("Center")?,
            angle: input("Angle")?,
        },
        MaterialGraphFunction::ScaleUv => MaterialExpressionKind::ScaleUv {
            uv: input("Uv")?,
            center: input("Center")?,
            scale: input("Scale")?,
        },
        MaterialGraphFunction::SampleTexture | MaterialGraphFunction::ExtractComponent => {
            return None;
        }
    })
}

fn set_modifier_constant(
    program: &mut MaterialProgram,
    expression: MaterialExpressionId,
    property: MaterialStackProperty,
    value: MaterialValue,
) -> Option<()> {
    let operation = program
        .expressions
        .iter()
        .find(|candidate| candidate.id == expression)?;
    let target = modifier_property_targets(&operation.kind)
        .into_iter()
        .find_map(|(candidate, target)| (candidate == property).then_some(target))?;
    let target = program
        .expressions
        .iter_mut()
        .find(|candidate| candidate.id == target)?;
    let MaterialExpressionKind::Constant(current) = &mut target.kind else {
        return None;
    };
    if !current.has_same_type(&value) || !value.is_valid() {
        return None;
    }
    *current = value;
    Some(())
}

fn plan_stack_insert_inner(
    program: &MaterialProgram,
    entries: &[MaterialStackEntry],
    kind: MaterialStackModifierKind,
    index: usize,
) -> Option<(MaterialExpressionId, MaterialProgram)> {
    if entries.is_empty() || index > entries.len() {
        return None;
    }
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let source = if index == 0 {
        primary_source(expressions[&entries[0].expression])?
    } else {
        entries[index - 1].expression
    };
    let boundary = entries.get(index).map(|entry| entry.expression);
    if boundary.is_some_and(|expression| primary_source(expressions[&expression]) != Some(source)) {
        return None;
    }
    let allowed_references = usize::from(boundary.is_some())
        + usize::from(program.outputs.color == source)
        + usize::from(program.outputs.alpha == source);
    if allowed_references == 0 || expression_reference_count(program, source) != allowed_references
    {
        return None;
    }

    let mut replacement = program.clone();
    let expression = append_default_modifier(&mut replacement, kind, source)?;
    if let Some(boundary) = boundary {
        let boundary = replacement
            .expressions
            .iter_mut()
            .find(|candidate| candidate.id == boundary)?;
        if !set_primary_source(&mut boundary.kind, expression) {
            return None;
        }
    }
    if replacement.outputs.color == source {
        replacement.outputs.color = expression;
    }
    if replacement.outputs.alpha == source {
        replacement.outputs.alpha = expression;
    }
    replacement.analyze().ok()?;
    let MaterialStackProjection::Stack { entries: projected } =
        MaterialCompiler.project_stack(&replacement).ok()?
    else {
        return None;
    };
    let mut expected = entries
        .iter()
        .map(|entry| entry.expression)
        .collect::<Vec<_>>();
    expected.insert(index, expression);
    projected
        .iter()
        .map(|entry| entry.expression)
        .eq(expected)
        .then_some((expression, replacement))
}

fn plan_stack_remove_inner(
    program: &MaterialProgram,
    entries: &[MaterialStackEntry],
    index: usize,
) -> Option<MaterialProgram> {
    let selected = entries.get(index)?.expression;
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let mut originally_reachable = BTreeSet::new();
    collect_reachable(
        program.outputs.color,
        &expressions,
        &mut originally_reachable,
    );
    collect_reachable(
        program.outputs.alpha,
        &expressions,
        &mut originally_reachable,
    );
    let source = primary_source(expressions[&selected])?;
    let boundary = entries
        .get(index + 1)
        .map(|entry| entry.expression)
        .filter(|expression| primary_source(expressions[expression]) == Some(selected));
    let allowed_references = usize::from(boundary.is_some())
        + usize::from(program.outputs.color == selected)
        + usize::from(program.outputs.alpha == selected);
    if allowed_references == 0
        || expression_reference_count(program, selected) != allowed_references
    {
        return None;
    }

    let mut replacement = program.clone();
    if let Some(boundary) = boundary {
        let boundary = replacement
            .expressions
            .iter_mut()
            .find(|candidate| candidate.id == boundary)?;
        if !set_primary_source(&mut boundary.kind, source) {
            return None;
        }
    }
    if replacement.outputs.color == selected {
        replacement.outputs.color = source;
    }
    if replacement.outputs.alpha == selected {
        replacement.outputs.alpha = source;
    }
    replacement
        .expressions
        .retain(|candidate| candidate.id != selected);
    let remaining = replacement
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let mut still_reachable = BTreeSet::new();
    collect_reachable(replacement.outputs.color, &remaining, &mut still_reachable);
    collect_reachable(replacement.outputs.alpha, &remaining, &mut still_reachable);
    replacement.expressions.retain(|candidate| {
        !originally_reachable.contains(&candidate.id) || still_reachable.contains(&candidate.id)
    });
    replacement
        .disabled_expressions
        .retain(|candidate| still_reachable.contains(candidate));
    replacement.analyze().ok()?;
    let MaterialStackProjection::Stack { entries: projected } =
        MaterialCompiler.project_stack(&replacement).ok()?
    else {
        return None;
    };
    let expected = entries
        .iter()
        .filter_map(|entry| (entry.expression != selected).then_some(entry.expression));
    projected
        .iter()
        .map(|entry| entry.expression)
        .eq(expected)
        .then_some(replacement)
}

fn expression_reference_count(
    program: &MaterialProgram,
    expression: MaterialExpressionId,
) -> usize {
    program
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
        + usize::from(program.outputs.alpha == expression)
}

pub(crate) fn append_default_modifier(
    program: &mut MaterialProgram,
    kind: MaterialStackModifierKind,
    source: MaterialExpressionId,
) -> Option<MaterialExpressionId> {
    let constant = |program: &mut MaterialProgram, value: MaterialValue| {
        append_expression(program, MaterialExpressionKind::Constant(value))
    };
    let operation = match kind {
        MaterialStackModifierKind::PanUv => {
            let speed = constant(program, MaterialValue::Vec2([0.0, 0.0]));
            let time = append_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::EffectTime),
            );
            MaterialExpressionKind::PanUv {
                uv: source,
                speed,
                time,
            }
        }
        MaterialStackModifierKind::RotateUv => {
            let center = constant(program, MaterialValue::Vec2([0.5, 0.5]));
            let angle = constant(program, MaterialValue::Float(0.0));
            MaterialExpressionKind::RotateUv {
                uv: source,
                center,
                angle,
            }
        }
        MaterialStackModifierKind::ScaleUv => {
            let center = constant(program, MaterialValue::Vec2([0.5, 0.5]));
            let scale = constant(program, MaterialValue::Vec2([1.0, 1.0]));
            MaterialExpressionKind::ScaleUv {
                uv: source,
                center,
                scale,
            }
        }
        MaterialStackModifierKind::Remap => {
            let input_min = constant(program, MaterialValue::Float(0.0));
            let input_max = constant(program, MaterialValue::Float(1.0));
            let output_min = constant(program, MaterialValue::Float(0.0));
            let output_max = constant(program, MaterialValue::Float(1.0));
            MaterialExpressionKind::Remap {
                value: source,
                input_min,
                input_max,
                output_min,
                output_max,
            }
        }
        MaterialStackModifierKind::Smoothstep => {
            let edge_min = constant(program, MaterialValue::Float(0.0));
            let edge_max = constant(program, MaterialValue::Float(1.0));
            MaterialExpressionKind::Smoothstep {
                edge_min,
                edge_max,
                value: source,
            }
        }
        // Fresnel is a scalar generator rather than a pass-through modifier. It is inserted by
        // the semantic Fresnel-edge command, which composes it with the selected color output.
        MaterialStackModifierKind::Fresnel => return None,
        MaterialStackModifierKind::RadialMask => {
            let center = constant(program, MaterialValue::Vec2([0.5, 0.5]));
            let radius = constant(program, MaterialValue::Float(0.5));
            let softness = constant(program, MaterialValue::Float(0.1));
            let invert = constant(program, MaterialValue::Bool(false));
            MaterialExpressionKind::RadialMask {
                uv: source,
                center,
                radius,
                softness,
                invert,
            }
        }
        MaterialStackModifierKind::Dissolve | MaterialStackModifierKind::DissolveEdge => {
            let threshold = constant(program, MaterialValue::Float(0.5));
            let edge_width = constant(program, MaterialValue::Float(0.1));
            let invert = constant(program, MaterialValue::Bool(false));
            if kind == MaterialStackModifierKind::Dissolve {
                MaterialExpressionKind::Dissolve {
                    source,
                    threshold,
                    edge_width,
                    invert,
                }
            } else {
                MaterialExpressionKind::DissolveEdge {
                    source,
                    threshold,
                    edge_width,
                    invert,
                }
            }
        }
        MaterialStackModifierKind::SoftParticle => {
            let scene_depth = append_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::SceneDepth),
            );
            let pixel_depth = append_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::PixelDepth),
            );
            let fade_distance = constant(program, MaterialValue::Float(0.5));
            let invert = constant(program, MaterialValue::Bool(false));
            MaterialExpressionKind::SoftParticle {
                alpha: source,
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            }
        }
        MaterialStackModifierKind::BaseTexture | MaterialStackModifierKind::DepthFade => {
            return None;
        }
    };
    Some(append_expression(program, operation))
}

fn append_expression(
    program: &mut MaterialProgram,
    kind: MaterialExpressionKind,
) -> MaterialExpressionId {
    let mut id = MaterialExpressionId::new();
    while program
        .expressions
        .iter()
        .any(|expression| expression.id == id)
    {
        id = MaterialExpressionId::new();
    }
    program.expressions.push(MaterialExpression { id, kind });
    id
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
        | MaterialExpressionKind::FunctionInput(_)
        | MaterialExpressionKind::FunctionCall { .. }
        | MaterialExpressionKind::Add(_, _)
        | MaterialExpressionKind::Subtract(_, _)
        | MaterialExpressionKind::Multiply(_, _)
        | MaterialExpressionKind::Divide(_, _)
        | MaterialExpressionKind::Lerp { .. }
        | MaterialExpressionKind::Clamp { .. }
        | MaterialExpressionKind::Fresnel { .. }
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
        | MaterialExpressionKind::FunctionInput(_)
        | MaterialExpressionKind::FunctionCall { .. }
        | MaterialExpressionKind::Add(_, _)
        | MaterialExpressionKind::Subtract(_, _)
        | MaterialExpressionKind::Multiply(_, _)
        | MaterialExpressionKind::Divide(_, _)
        | MaterialExpressionKind::Lerp { .. }
        | MaterialExpressionKind::Clamp { .. }
        | MaterialExpressionKind::Fresnel { .. }
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
        MaterialExpressionKind::Fresnel { .. } => MaterialStackModifierKind::Fresnel,
        MaterialExpressionKind::RadialMask { .. } => MaterialStackModifierKind::RadialMask,
        MaterialExpressionKind::Dissolve { .. } => MaterialStackModifierKind::Dissolve,
        MaterialExpressionKind::DissolveEdge { .. } => MaterialStackModifierKind::DissolveEdge,
        MaterialExpressionKind::DepthFade { .. } => MaterialStackModifierKind::DepthFade,
        MaterialExpressionKind::SoftParticle { .. } => MaterialStackModifierKind::SoftParticle,
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::FunctionInput(_)
        | MaterialExpressionKind::FunctionCall { .. }
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
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::FunctionInput(_) => Vec::new(),
        MaterialExpressionKind::FunctionCall { arguments, .. } => {
            arguments.values().copied().collect()
        }
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
        MaterialExpressionKind::SampleTexture { texture, uv } => vec![*texture, *uv],
        MaterialExpressionKind::ExtractComponent { value, .. } => vec![*value],
    }
}
