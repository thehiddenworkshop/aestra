use aestra_core::{
    Diagnostic, DiagnosticCode, EffectAsset, EmitterId, MaterialExpressionId, MaterialId,
    MaterialParameterId, MaterialProgramId, RendererId, ValidationReport, Value,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialInstance, MaterialParameterValue,
        MaterialProgram, MaterialProgramRef, MaterialRenderState, MaterialValueType,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use thiserror::Error;

const DEFAULT_MATERIAL_HISTORY_LIMIT: usize = 256;

/// One transactional authoring boundary for project material programs and the
/// effect-local instances that consume them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialAuthoringDocument {
    pub effect: EffectAsset,
    pub programs: Vec<MaterialProgram>,
}

impl MaterialAuthoringDocument {
    pub fn new(effect: EffectAsset, programs: Vec<MaterialProgram>) -> Self {
        Self { effect, programs }
    }

    pub fn validation_report(&self) -> ValidationReport {
        let mut report = self.effect.validation_report();
        report.diagnostics.retain(|diagnostic| {
            !matches!(
                diagnostic.code,
                DiagnosticCode::MissingModule | DiagnosticCode::MissingRenderer
            )
        });

        let mut program_ids = BTreeSet::new();
        for (index, program) in self.programs.iter().enumerate() {
            let path = format!("material_document.programs[{index}]");
            if !program_ids.insert(program.id) {
                report.push(Diagnostic::error(
                    DiagnosticCode::DuplicateId,
                    format!("{path}.id"),
                    format!("material program ID {} must be unique", program.id),
                ));
            }
            append_prefixed_report(&mut report, &path, program.validation_report());
        }

        for (index, instance) in self.effect.material_instances.iter().enumerate() {
            let path = format!("effect.material_instances[{index}]");
            match self
                .programs
                .iter()
                .find(|program| program.id == instance.program.id())
            {
                Some(program) => {
                    append_prefixed_report(&mut report, &path, instance.validate_against(program));
                    validate_effect_parameter_bindings(
                        &mut report,
                        &self.effect,
                        instance,
                        program,
                        &path,
                    );
                }
                None if matches!(instance.program, MaterialProgramRef::Project(_)) => {
                    report.push(Diagnostic::error(
                        DiagnosticCode::InvalidReference,
                        format!("{path}.program"),
                        format!(
                            "material instance references missing project program {}",
                            instance.program.id()
                        ),
                    ));
                }
                None => {}
            }
        }
        report.diagnostics.sort();
        report.diagnostics.dedup();
        report
    }

    pub fn validate(&self) -> Result<(), ValidationReport> {
        self.validation_report().into_result()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialOutputSocket {
    Color,
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialExpressionInput {
    Left,
    Right,
    Start,
    End,
    Factor,
    Value,
    Minimum,
    Maximum,
    InputMinimum,
    InputMaximum,
    OutputMinimum,
    OutputMaximum,
    EdgeMinimum,
    EdgeMaximum,
    Normal,
    View,
    Power,
    Radius,
    Softness,
    Threshold,
    EdgeWidth,
    SceneDepth,
    PixelDepth,
    FadeDistance,
    Invert,
    Speed,
    Time,
    Center,
    Angle,
    Scale,
    Texture,
    Uv,
    Source,
    SourceAlpha,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialCommand {
    AddMaterialProgram {
        program: MaterialProgram,
        index: usize,
    },
    RemoveMaterialProgram {
        id: MaterialProgramId,
    },
    ReplaceMaterialProgram {
        id: MaterialProgramId,
        program: MaterialProgram,
    },
    AddMaterialInstance {
        instance: MaterialInstance,
        index: usize,
    },
    RemoveMaterialInstance {
        id: MaterialId,
    },
    ReplaceMaterialInstance {
        id: MaterialId,
        instance: MaterialInstance,
    },
    SetMaterialInstanceParameter {
        instance: MaterialId,
        parameter: MaterialParameterId,
        value: Option<MaterialParameterValue>,
    },
    SetMaterialInstanceRenderState {
        instance: MaterialId,
        render_state: MaterialRenderState,
    },
    AssignRendererMaterial {
        emitter: EmitterId,
        renderer: RendererId,
        material: MaterialId,
    },
    SetMaterialOutput {
        program: MaterialProgramId,
        output: MaterialOutputSocket,
        expression: MaterialExpressionId,
    },
    AddMaterialExpression {
        program: MaterialProgramId,
        expression: MaterialExpression,
        index: usize,
    },
    RemoveMaterialExpression {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
    },
    ReplaceMaterialExpression {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        replacement: MaterialExpression,
    },
    SetMaterialExpressionInline {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        inline: bool,
    },
    RewireMaterialExpressionInput {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        input: MaterialExpressionInput,
        source: MaterialExpressionId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialTransaction {
    pub label: String,
    pub commands: Vec<MaterialCommand>,
}

impl MaterialTransaction {
    pub fn new(label: impl Into<String>, commands: Vec<MaterialCommand>) -> Self {
        Self {
            label: label.into(),
            commands,
        }
    }

    pub fn single(label: impl Into<String>, command: MaterialCommand) -> Self {
        Self::new(label, vec![command])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaterialSemanticTarget {
    Program(MaterialProgramId),
    Instance(MaterialId),
    Expression(MaterialExpressionId),
    Renderer(RendererId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialChangeKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialSemanticChange {
    pub kind: MaterialChangeKind,
    pub target: MaterialSemanticTarget,
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialDiff {
    pub changes: Vec<MaterialSemanticChange>,
}

impl MaterialDiff {
    pub fn between(before: &MaterialAuthoringDocument, after: &MaterialAuthoringDocument) -> Self {
        let mut changes = Vec::new();
        diff_programs(&mut changes, &before.programs, &after.programs);
        diff_instances(
            &mut changes,
            &before.effect.material_instances,
            &after.effect.material_instances,
        );
        diff_renderer_assignments(&mut changes, &before.effect, &after.effect);
        Self { changes }
    }

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

#[derive(Debug, Error)]
pub enum MaterialCommandError {
    #[error("{kind} '{id}' was not found")]
    NotFound { kind: &'static str, id: String },
    #[error("index {index} is outside {collection} with length {len}")]
    IndexOutOfBounds {
        collection: &'static str,
        index: usize,
        len: usize,
    },
    #[error("replacement {kind} must preserve identity {expected}, received {actual}")]
    IdentityChanged {
        kind: &'static str,
        expected: String,
        actual: String,
    },
    #[error("input {input:?} does not exist on this material expression")]
    InvalidExpressionInput { input: MaterialExpressionInput },
    #[error("material transaction validation failed: {0}")]
    Validation(#[from] ValidationReport),
}

#[derive(Debug, Clone)]
pub struct MaterialTransactionOutcome {
    pub inverse: MaterialTransaction,
    pub diff: MaterialDiff,
}

#[derive(Debug, Default)]
pub struct MaterialCommandExecutor;

impl MaterialCommandExecutor {
    pub fn execute(
        document: &mut MaterialAuthoringDocument,
        transaction: &MaterialTransaction,
    ) -> Result<MaterialTransactionOutcome, MaterialCommandError> {
        let before = document.clone();
        let mut working = before.clone();
        let mut inverse_commands = Vec::new();
        for command in &transaction.commands {
            let mut inverse = apply_command(&mut working, command)?;
            inverse.extend(inverse_commands);
            inverse_commands = inverse;
        }
        working.validate()?;
        let diff = MaterialDiff::between(&before, &working);
        *document = working;
        Ok(MaterialTransactionOutcome {
            inverse: MaterialTransaction::new(
                format!("Undo {}", transaction.label),
                inverse_commands,
            ),
            diff,
        })
    }
}

#[derive(Debug, Clone)]
struct MaterialHistoryEntry {
    label: String,
    forward: MaterialTransaction,
    inverse: MaterialTransaction,
}

#[derive(Debug, Clone)]
pub struct MaterialHistoryResult {
    pub label: String,
    pub diff: MaterialDiff,
}

#[derive(Debug, Clone)]
pub struct MaterialCommandHistory {
    undo: VecDeque<MaterialHistoryEntry>,
    redo: Vec<MaterialHistoryEntry>,
    limit: usize,
}

impl Default for MaterialCommandHistory {
    fn default() -> Self {
        Self::with_limit(DEFAULT_MATERIAL_HISTORY_LIMIT)
    }
}

impl MaterialCommandHistory {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit: limit.max(1),
        }
    }

    pub fn execute(
        &mut self,
        document: &mut MaterialAuthoringDocument,
        transaction: MaterialTransaction,
    ) -> Result<MaterialDiff, MaterialCommandError> {
        let outcome = MaterialCommandExecutor::execute(document, &transaction)?;
        if outcome.diff.is_empty() {
            return Ok(outcome.diff);
        }
        self.undo.push_back(MaterialHistoryEntry {
            label: transaction.label.clone(),
            forward: transaction,
            inverse: outcome.inverse,
        });
        while self.undo.len() > self.limit {
            self.undo.pop_front();
        }
        self.redo.clear();
        Ok(outcome.diff)
    }

    pub fn undo(
        &mut self,
        document: &mut MaterialAuthoringDocument,
    ) -> Result<Option<MaterialHistoryResult>, MaterialCommandError> {
        let Some(entry) = self.undo.pop_back() else {
            return Ok(None);
        };
        match MaterialCommandExecutor::execute(document, &entry.inverse) {
            Ok(outcome) => {
                let result = MaterialHistoryResult {
                    label: entry.label.clone(),
                    diff: outcome.diff,
                };
                self.redo.push(entry);
                Ok(Some(result))
            }
            Err(error) => {
                self.undo.push_back(entry);
                Err(error)
            }
        }
    }

    pub fn redo(
        &mut self,
        document: &mut MaterialAuthoringDocument,
    ) -> Result<Option<MaterialHistoryResult>, MaterialCommandError> {
        let Some(entry) = self.redo.pop() else {
            return Ok(None);
        };
        match MaterialCommandExecutor::execute(document, &entry.forward) {
            Ok(outcome) => {
                let result = MaterialHistoryResult {
                    label: entry.label.clone(),
                    diff: outcome.diff,
                };
                self.undo.push_back(entry);
                Ok(Some(result))
            }
            Err(error) => {
                self.redo.push(entry);
                Err(error)
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

fn apply_command(
    document: &mut MaterialAuthoringDocument,
    command: &MaterialCommand,
) -> Result<Vec<MaterialCommand>, MaterialCommandError> {
    let inverse = match command {
        MaterialCommand::AddMaterialProgram { program, index } => {
            checked_insert(
                &mut document.programs,
                *index,
                program.clone(),
                "material programs",
            )?;
            vec![MaterialCommand::RemoveMaterialProgram { id: program.id }]
        }
        MaterialCommand::RemoveMaterialProgram { id } => {
            let index = program_index(document, *id)?;
            let program = document.programs.remove(index);
            vec![MaterialCommand::AddMaterialProgram { program, index }]
        }
        MaterialCommand::ReplaceMaterialProgram { id, program } => {
            ensure_identity("material program", *id, program.id)?;
            let current = program_mut(document, *id)?;
            let previous = std::mem::replace(current, program.clone());
            vec![MaterialCommand::ReplaceMaterialProgram {
                id: *id,
                program: previous,
            }]
        }
        MaterialCommand::AddMaterialInstance { instance, index } => {
            checked_insert(
                &mut document.effect.material_instances,
                *index,
                instance.clone(),
                "material instances",
            )?;
            vec![MaterialCommand::RemoveMaterialInstance { id: instance.id }]
        }
        MaterialCommand::RemoveMaterialInstance { id } => {
            let index = instance_index(document, *id)?;
            let instance = document.effect.material_instances.remove(index);
            vec![MaterialCommand::AddMaterialInstance { instance, index }]
        }
        MaterialCommand::ReplaceMaterialInstance { id, instance } => {
            ensure_identity("material instance", *id, instance.id)?;
            let current = instance_mut(document, *id)?;
            let previous = std::mem::replace(current, instance.clone());
            vec![MaterialCommand::ReplaceMaterialInstance {
                id: *id,
                instance: previous,
            }]
        }
        MaterialCommand::SetMaterialInstanceParameter {
            instance,
            parameter,
            value,
        } => {
            let instance = instance_mut(document, *instance)?;
            let previous = match value {
                Some(value) => instance.values.insert(*parameter, value.clone()),
                None => instance.values.remove(parameter),
            };
            vec![MaterialCommand::SetMaterialInstanceParameter {
                instance: instance.id,
                parameter: *parameter,
                value: previous,
            }]
        }
        MaterialCommand::SetMaterialInstanceRenderState {
            instance,
            render_state,
        } => {
            let instance = instance_mut(document, *instance)?;
            let previous = std::mem::replace(&mut instance.render_state, *render_state);
            vec![MaterialCommand::SetMaterialInstanceRenderState {
                instance: instance.id,
                render_state: previous,
            }]
        }
        MaterialCommand::AssignRendererMaterial {
            emitter,
            renderer,
            material,
        } => {
            let renderer = renderer_mut(&mut document.effect, *emitter, *renderer)?;
            let previous = std::mem::replace(&mut renderer.material, *material);
            vec![MaterialCommand::AssignRendererMaterial {
                emitter: *emitter,
                renderer: renderer.id,
                material: previous,
            }]
        }
        MaterialCommand::SetMaterialOutput {
            program,
            output,
            expression,
        } => {
            let program = program_mut(document, *program)?;
            let target = match output {
                MaterialOutputSocket::Color => &mut program.outputs.color,
                MaterialOutputSocket::Alpha => &mut program.outputs.alpha,
            };
            let previous = std::mem::replace(target, *expression);
            vec![MaterialCommand::SetMaterialOutput {
                program: program.id,
                output: *output,
                expression: previous,
            }]
        }
        MaterialCommand::AddMaterialExpression {
            program,
            expression,
            index,
        } => {
            let program = program_mut(document, *program)?;
            checked_insert(
                &mut program.expressions,
                *index,
                expression.clone(),
                "material expressions",
            )?;
            vec![MaterialCommand::RemoveMaterialExpression {
                program: program.id,
                expression: expression.id,
            }]
        }
        MaterialCommand::RemoveMaterialExpression {
            program,
            expression,
        } => {
            let program = program_mut(document, *program)?;
            let index = expression_index(program, *expression)?;
            let expression = program.expressions.remove(index);
            let was_inline = program.inline_constants.contains(&expression.id);
            program
                .inline_constants
                .retain(|candidate| *candidate != expression.id);
            let mut inverse = vec![MaterialCommand::AddMaterialExpression {
                program: program.id,
                expression: expression.clone(),
                index,
            }];
            if was_inline {
                inverse.push(MaterialCommand::SetMaterialExpressionInline {
                    program: program.id,
                    expression: expression.id,
                    inline: true,
                });
            }
            inverse
        }
        MaterialCommand::ReplaceMaterialExpression {
            program,
            expression,
            replacement,
        } => {
            ensure_identity("material expression", *expression, replacement.id)?;
            let program = program_mut(document, *program)?;
            let index = expression_index(program, *expression)?;
            let previous = std::mem::replace(&mut program.expressions[index], replacement.clone());
            vec![MaterialCommand::ReplaceMaterialExpression {
                program: program.id,
                expression: *expression,
                replacement: previous,
            }]
        }
        MaterialCommand::SetMaterialExpressionInline {
            program,
            expression,
            inline,
        } => {
            let program = program_mut(document, *program)?;
            expression_index(program, *expression)?;
            let previous = program.inline_constants.contains(expression);
            if *inline && !previous {
                program.inline_constants.push(*expression);
            } else if !*inline && previous {
                program
                    .inline_constants
                    .retain(|candidate| candidate != expression);
            }
            vec![MaterialCommand::SetMaterialExpressionInline {
                program: program.id,
                expression: *expression,
                inline: previous,
            }]
        }
        MaterialCommand::RewireMaterialExpressionInput {
            program,
            expression,
            input,
            source,
        } => {
            let program = program_mut(document, *program)?;
            let index = expression_index(program, *expression)?;
            let previous =
                rewire_expression(&mut program.expressions[index].kind, *input, *source)?;
            vec![MaterialCommand::RewireMaterialExpressionInput {
                program: program.id,
                expression: *expression,
                input: *input,
                source: previous,
            }]
        }
    };
    Ok(inverse)
}

pub(crate) fn rewire_expression(
    expression: &mut MaterialExpressionKind,
    input: MaterialExpressionInput,
    source: MaterialExpressionId,
) -> Result<MaterialExpressionId, MaterialCommandError> {
    let target = match (expression, input) {
        (MaterialExpressionKind::Add(left, _), MaterialExpressionInput::Left)
        | (MaterialExpressionKind::Subtract(left, _), MaterialExpressionInput::Left)
        | (MaterialExpressionKind::Multiply(left, _), MaterialExpressionInput::Left)
        | (MaterialExpressionKind::Divide(left, _), MaterialExpressionInput::Left) => left,
        (MaterialExpressionKind::Add(_, right), MaterialExpressionInput::Right)
        | (MaterialExpressionKind::Subtract(_, right), MaterialExpressionInput::Right)
        | (MaterialExpressionKind::Multiply(_, right), MaterialExpressionInput::Right)
        | (MaterialExpressionKind::Divide(_, right), MaterialExpressionInput::Right) => right,
        (MaterialExpressionKind::Lerp { start, .. }, MaterialExpressionInput::Start) => start,
        (MaterialExpressionKind::Lerp { end, .. }, MaterialExpressionInput::End) => end,
        (MaterialExpressionKind::Lerp { factor, .. }, MaterialExpressionInput::Factor) => factor,
        (MaterialExpressionKind::Clamp { value, .. }, MaterialExpressionInput::Value) => value,
        (MaterialExpressionKind::Clamp { min, .. }, MaterialExpressionInput::Minimum) => min,
        (MaterialExpressionKind::Clamp { max, .. }, MaterialExpressionInput::Maximum) => max,
        (MaterialExpressionKind::Remap { value, .. }, MaterialExpressionInput::Value) => value,
        (
            MaterialExpressionKind::Remap { input_min, .. },
            MaterialExpressionInput::InputMinimum,
        ) => input_min,
        (
            MaterialExpressionKind::Remap { input_max, .. },
            MaterialExpressionInput::InputMaximum,
        ) => input_max,
        (
            MaterialExpressionKind::Remap { output_min, .. },
            MaterialExpressionInput::OutputMinimum,
        ) => output_min,
        (
            MaterialExpressionKind::Remap { output_max, .. },
            MaterialExpressionInput::OutputMaximum,
        ) => output_max,
        (
            MaterialExpressionKind::Smoothstep { edge_min, .. },
            MaterialExpressionInput::EdgeMinimum,
        ) => edge_min,
        (
            MaterialExpressionKind::Smoothstep { edge_max, .. },
            MaterialExpressionInput::EdgeMaximum,
        ) => edge_max,
        (MaterialExpressionKind::Smoothstep { value, .. }, MaterialExpressionInput::Value) => value,
        (MaterialExpressionKind::Fresnel { normal, .. }, MaterialExpressionInput::Normal) => normal,
        (MaterialExpressionKind::Fresnel { view, .. }, MaterialExpressionInput::View) => view,
        (MaterialExpressionKind::Fresnel { power, .. }, MaterialExpressionInput::Power) => power,
        (MaterialExpressionKind::RadialMask { uv, .. }, MaterialExpressionInput::Uv) => uv,
        (MaterialExpressionKind::RadialMask { center, .. }, MaterialExpressionInput::Center) => {
            center
        }
        (MaterialExpressionKind::RadialMask { radius, .. }, MaterialExpressionInput::Radius) => {
            radius
        }
        (
            MaterialExpressionKind::RadialMask { softness, .. },
            MaterialExpressionInput::Softness,
        ) => softness,
        (MaterialExpressionKind::RadialMask { invert, .. }, MaterialExpressionInput::Invert) => {
            invert
        }
        (MaterialExpressionKind::Dissolve { source, .. }, MaterialExpressionInput::Source) => {
            source
        }
        (
            MaterialExpressionKind::Dissolve { threshold, .. },
            MaterialExpressionInput::Threshold,
        ) => threshold,
        (
            MaterialExpressionKind::Dissolve { edge_width, .. },
            MaterialExpressionInput::EdgeWidth,
        ) => edge_width,
        (MaterialExpressionKind::Dissolve { invert, .. }, MaterialExpressionInput::Invert) => {
            invert
        }
        (MaterialExpressionKind::DissolveEdge { source, .. }, MaterialExpressionInput::Source) => {
            source
        }
        (
            MaterialExpressionKind::DissolveEdge { threshold, .. },
            MaterialExpressionInput::Threshold,
        ) => threshold,
        (
            MaterialExpressionKind::DissolveEdge { edge_width, .. },
            MaterialExpressionInput::EdgeWidth,
        ) => edge_width,
        (MaterialExpressionKind::DissolveEdge { invert, .. }, MaterialExpressionInput::Invert) => {
            invert
        }
        (
            MaterialExpressionKind::DepthFade { scene_depth, .. },
            MaterialExpressionInput::SceneDepth,
        ) => scene_depth,
        (
            MaterialExpressionKind::DepthFade { pixel_depth, .. },
            MaterialExpressionInput::PixelDepth,
        ) => pixel_depth,
        (
            MaterialExpressionKind::DepthFade { fade_distance, .. },
            MaterialExpressionInput::FadeDistance,
        ) => fade_distance,
        (MaterialExpressionKind::DepthFade { invert, .. }, MaterialExpressionInput::Invert) => {
            invert
        }
        (
            MaterialExpressionKind::SoftParticle { alpha, .. },
            MaterialExpressionInput::SourceAlpha,
        ) => alpha,
        (
            MaterialExpressionKind::SoftParticle { scene_depth, .. },
            MaterialExpressionInput::SceneDepth,
        ) => scene_depth,
        (
            MaterialExpressionKind::SoftParticle { pixel_depth, .. },
            MaterialExpressionInput::PixelDepth,
        ) => pixel_depth,
        (
            MaterialExpressionKind::SoftParticle { fade_distance, .. },
            MaterialExpressionInput::FadeDistance,
        ) => fade_distance,
        (MaterialExpressionKind::SoftParticle { invert, .. }, MaterialExpressionInput::Invert) => {
            invert
        }
        (MaterialExpressionKind::PanUv { uv, .. }, MaterialExpressionInput::Uv) => uv,
        (MaterialExpressionKind::PanUv { speed, .. }, MaterialExpressionInput::Speed) => speed,
        (MaterialExpressionKind::PanUv { time, .. }, MaterialExpressionInput::Time) => time,
        (MaterialExpressionKind::RotateUv { uv, .. }, MaterialExpressionInput::Uv) => uv,
        (MaterialExpressionKind::RotateUv { center, .. }, MaterialExpressionInput::Center) => {
            center
        }
        (MaterialExpressionKind::RotateUv { angle, .. }, MaterialExpressionInput::Angle) => angle,
        (MaterialExpressionKind::ScaleUv { uv, .. }, MaterialExpressionInput::Uv) => uv,
        (MaterialExpressionKind::ScaleUv { center, .. }, MaterialExpressionInput::Center) => center,
        (MaterialExpressionKind::ScaleUv { scale, .. }, MaterialExpressionInput::Scale) => scale,
        (
            MaterialExpressionKind::SampleTexture { texture, .. },
            MaterialExpressionInput::Texture,
        ) => texture,
        (MaterialExpressionKind::SampleTexture { uv, .. }, MaterialExpressionInput::Uv) => uv,
        (
            MaterialExpressionKind::ExtractComponent { value, .. },
            MaterialExpressionInput::Source,
        ) => value,
        (_, input) => return Err(MaterialCommandError::InvalidExpressionInput { input }),
    };
    Ok(std::mem::replace(target, source))
}

pub(crate) fn material_expression_input_source(
    expression: &MaterialExpressionKind,
    input: MaterialExpressionInput,
) -> Result<MaterialExpressionId, MaterialCommandError> {
    // Reuse the authoritative mutable socket map; the probe mutation is discarded.
    let mut probe = expression.clone();
    rewire_expression(&mut probe, input, MaterialExpressionId::from_u128(0))
}

fn diff_programs(
    changes: &mut Vec<MaterialSemanticChange>,
    before: &[MaterialProgram],
    after: &[MaterialProgram],
) {
    let before = before
        .iter()
        .map(|item| (item.id, item))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .iter()
        .map(|item| (item.id, item))
        .collect::<BTreeMap<_, _>>();
    for (id, program) in &before {
        match after.get(id) {
            None => push_change(
                changes,
                MaterialChangeKind::Removed,
                MaterialSemanticTarget::Program(*id),
                "material_document.programs",
                Some(program.name.clone()),
                None,
            ),
            Some(replacement) if *program != *replacement => {
                push_change(
                    changes,
                    MaterialChangeKind::Modified,
                    MaterialSemanticTarget::Program(*id),
                    format!("material_document.programs[{id}]"),
                    Some(program.name.clone()),
                    Some(replacement.name.clone()),
                );
                diff_expressions(changes, *id, program, replacement);
                diff_program_outputs(changes, *id, program, replacement);
            }
            Some(_) => {}
        }
    }
    for (id, program) in &after {
        if !before.contains_key(id) {
            push_change(
                changes,
                MaterialChangeKind::Added,
                MaterialSemanticTarget::Program(*id),
                "material_document.programs",
                None,
                Some(program.name.clone()),
            );
        }
    }
}

fn diff_program_outputs(
    changes: &mut Vec<MaterialSemanticChange>,
    program_id: MaterialProgramId,
    before: &MaterialProgram,
    after: &MaterialProgram,
) {
    for (socket, before, after) in [
        ("color", before.outputs.color, after.outputs.color),
        ("alpha", before.outputs.alpha, after.outputs.alpha),
    ] {
        if before != after {
            push_change(
                changes,
                MaterialChangeKind::Modified,
                MaterialSemanticTarget::Program(program_id),
                format!("material_document.programs[{program_id}].outputs.{socket}"),
                Some(before.to_string()),
                Some(after.to_string()),
            );
        }
    }
}

fn diff_expressions(
    changes: &mut Vec<MaterialSemanticChange>,
    program: MaterialProgramId,
    before: &MaterialProgram,
    after: &MaterialProgram,
) {
    let before_items = before
        .expressions
        .iter()
        .map(|item| (item.id, item))
        .collect::<BTreeMap<_, _>>();
    let after_items = after
        .expressions
        .iter()
        .map(|item| (item.id, item))
        .collect::<BTreeMap<_, _>>();
    for (id, expression) in &before_items {
        match after_items.get(id) {
            None => push_change(
                changes,
                MaterialChangeKind::Removed,
                MaterialSemanticTarget::Expression(*id),
                format!("material_programs[{program}].expressions"),
                Some(format!("{:?}", expression.kind)),
                None,
            ),
            Some(replacement) if *expression != *replacement => push_change(
                changes,
                MaterialChangeKind::Modified,
                MaterialSemanticTarget::Expression(*id),
                format!("material_programs[{program}].expressions[{id}]"),
                Some(format!("{:?}", expression.kind)),
                Some(format!("{:?}", replacement.kind)),
            ),
            Some(_) => {}
        }
    }
    for (id, expression) in &after_items {
        if !before_items.contains_key(id) {
            push_change(
                changes,
                MaterialChangeKind::Added,
                MaterialSemanticTarget::Expression(*id),
                format!("material_programs[{program}].expressions"),
                None,
                Some(format!("{:?}", expression.kind)),
            );
        }
    }
}

fn diff_instances(
    changes: &mut Vec<MaterialSemanticChange>,
    before: &[MaterialInstance],
    after: &[MaterialInstance],
) {
    let before = before
        .iter()
        .map(|item| (item.id, item))
        .collect::<BTreeMap<_, _>>();
    let after = after
        .iter()
        .map(|item| (item.id, item))
        .collect::<BTreeMap<_, _>>();
    for (id, instance) in &before {
        match after.get(id) {
            None => push_change(
                changes,
                MaterialChangeKind::Removed,
                MaterialSemanticTarget::Instance(*id),
                "effect.material_instances",
                Some(format!("{:?}", instance.program)),
                None,
            ),
            Some(replacement) if *instance != *replacement => {
                push_change(
                    changes,
                    MaterialChangeKind::Modified,
                    MaterialSemanticTarget::Instance(*id),
                    format!("effect.material_instances[{id}]"),
                    Some(format!("{:?}", instance.values)),
                    Some(format!("{:?}", replacement.values)),
                );
                diff_instance_parameters(changes, *id, instance, replacement);
            }
            Some(_) => {}
        }
    }
    for (id, instance) in &after {
        if !before.contains_key(id) {
            push_change(
                changes,
                MaterialChangeKind::Added,
                MaterialSemanticTarget::Instance(*id),
                "effect.material_instances",
                None,
                Some(format!("{:?}", instance.program)),
            );
        }
    }
}

fn diff_instance_parameters(
    changes: &mut Vec<MaterialSemanticChange>,
    instance_id: MaterialId,
    before: &MaterialInstance,
    after: &MaterialInstance,
) {
    for (parameter, value) in &before.values {
        let path = format!("effect.material_instances[{instance_id}].values[{parameter}]");
        match after.values.get(parameter) {
            None => push_change(
                changes,
                MaterialChangeKind::Removed,
                MaterialSemanticTarget::Instance(instance_id),
                path,
                Some(format!("{value:?}")),
                None,
            ),
            Some(replacement) if value != replacement => push_change(
                changes,
                MaterialChangeKind::Modified,
                MaterialSemanticTarget::Instance(instance_id),
                path,
                Some(format!("{value:?}")),
                Some(format!("{replacement:?}")),
            ),
            Some(_) => {}
        }
    }
    for (parameter, value) in &after.values {
        if !before.values.contains_key(parameter) {
            push_change(
                changes,
                MaterialChangeKind::Added,
                MaterialSemanticTarget::Instance(instance_id),
                format!("effect.material_instances[{instance_id}].values[{parameter}]"),
                None,
                Some(format!("{value:?}")),
            );
        }
    }
}

fn diff_renderer_assignments(
    changes: &mut Vec<MaterialSemanticChange>,
    before: &EffectAsset,
    after: &EffectAsset,
) {
    let before = before
        .emitters
        .iter()
        .flat_map(|emitter| emitter.renderers.iter())
        .map(|renderer| (renderer.id, renderer.material))
        .collect::<BTreeMap<_, _>>();
    for renderer in after
        .emitters
        .iter()
        .flat_map(|emitter| emitter.renderers.iter())
    {
        if let Some(previous) = before.get(&renderer.id)
            && *previous != renderer.material
        {
            push_change(
                changes,
                MaterialChangeKind::Modified,
                MaterialSemanticTarget::Renderer(renderer.id),
                format!("effect.renderers[{}].material", renderer.id),
                Some(previous.to_string()),
                Some(renderer.material.to_string()),
            );
        }
    }
}

fn push_change(
    changes: &mut Vec<MaterialSemanticChange>,
    kind: MaterialChangeKind,
    target: MaterialSemanticTarget,
    path: impl Into<String>,
    before: Option<String>,
    after: Option<String>,
) {
    changes.push(MaterialSemanticChange {
        kind,
        target,
        path: path.into(),
        before,
        after,
    });
}

fn append_prefixed_report(report: &mut ValidationReport, prefix: &str, nested: ValidationReport) {
    for mut diagnostic in nested.diagnostics {
        let suffix = diagnostic
            .path
            .strip_prefix("material_program")
            .or_else(|| diagnostic.path.strip_prefix("material_instance"));
        diagnostic.path = suffix.map_or_else(
            || format!("{prefix}.{}", diagnostic.path),
            |suffix| format!("{prefix}{suffix}"),
        );
        report.push(diagnostic);
    }
}

fn validate_effect_parameter_bindings(
    report: &mut ValidationReport,
    effect: &EffectAsset,
    instance: &MaterialInstance,
    program: &MaterialProgram,
    path: &str,
) {
    for (parameter_id, value) in &instance.values {
        let (MaterialParameterValue::EffectParameter(binding)
        | MaterialParameterValue::EmitterParameter(binding)) = value
        else {
            continue;
        };
        let Some(effect_parameter) = effect.parameters.iter().find(|item| item.id == *binding)
        else {
            continue;
        };
        let Some(material_parameter) = program
            .parameters
            .iter()
            .find(|item| item.id == *parameter_id)
        else {
            continue;
        };
        if !effect_value_matches_material_type(
            &effect_parameter.default,
            material_parameter.value_type,
        ) {
            report.push(Diagnostic::error(
                DiagnosticCode::ParameterTypeMismatch,
                format!("{path}.values[{parameter_id}]"),
                format!(
                    "material parameter '{}' expects {:?}, but effect parameter '{}' provides {:?}",
                    material_parameter.name,
                    material_parameter.value_type,
                    effect_parameter.name,
                    effect_parameter.default.value_type()
                ),
            ));
        }
    }
}

fn effect_value_matches_material_type(value: &Value, value_type: MaterialValueType) -> bool {
    value_type.accepts_effect_value(value)
}

fn checked_insert<T>(
    values: &mut Vec<T>,
    index: usize,
    value: T,
    collection: &'static str,
) -> Result<(), MaterialCommandError> {
    if index > values.len() {
        return Err(MaterialCommandError::IndexOutOfBounds {
            collection,
            index,
            len: values.len(),
        });
    }
    values.insert(index, value);
    Ok(())
}

fn ensure_identity<T: PartialEq + ToString>(
    kind: &'static str,
    expected: T,
    actual: T,
) -> Result<(), MaterialCommandError> {
    if expected != actual {
        return Err(MaterialCommandError::IdentityChanged {
            kind,
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

fn program_index(
    document: &MaterialAuthoringDocument,
    id: MaterialProgramId,
) -> Result<usize, MaterialCommandError> {
    document
        .programs
        .iter()
        .position(|program| program.id == id)
        .ok_or_else(|| not_found("material program", id))
}

fn program_mut(
    document: &mut MaterialAuthoringDocument,
    id: MaterialProgramId,
) -> Result<&mut MaterialProgram, MaterialCommandError> {
    let index = program_index(document, id)?;
    Ok(&mut document.programs[index])
}

fn instance_index(
    document: &MaterialAuthoringDocument,
    id: MaterialId,
) -> Result<usize, MaterialCommandError> {
    document
        .effect
        .material_instances
        .iter()
        .position(|instance| instance.id == id)
        .ok_or_else(|| not_found("material instance", id))
}

fn instance_mut(
    document: &mut MaterialAuthoringDocument,
    id: MaterialId,
) -> Result<&mut MaterialInstance, MaterialCommandError> {
    let index = instance_index(document, id)?;
    Ok(&mut document.effect.material_instances[index])
}

fn expression_index(
    program: &MaterialProgram,
    id: MaterialExpressionId,
) -> Result<usize, MaterialCommandError> {
    program
        .expressions
        .iter()
        .position(|expression| expression.id == id)
        .ok_or_else(|| not_found("material expression", id))
}

fn renderer_mut(
    effect: &mut EffectAsset,
    emitter: EmitterId,
    renderer: RendererId,
) -> Result<&mut aestra_core::RendererInstance, MaterialCommandError> {
    let emitter = effect
        .emitters
        .iter_mut()
        .find(|item| item.id == emitter)
        .ok_or_else(|| not_found("emitter", emitter))?;
    emitter
        .renderers
        .iter_mut()
        .find(|item| item.id == renderer)
        .ok_or_else(|| not_found("renderer", renderer))
}

fn not_found(kind: &'static str, id: impl ToString) -> MaterialCommandError {
    MaterialCommandError::NotFound {
        kind,
        id: id.to_string(),
    }
}
