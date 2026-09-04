//! UI-independent, high-level material transformations built from baseline commands.

use crate::{
    MaterialAuthoringDocument, MaterialCommand, MaterialCommandError, MaterialCommandExecutor,
    MaterialDiff, MaterialExpressionInput, MaterialOutputSocket, MaterialTransaction,
    material_authoring::{material_expression_input_source, rewire_expression},
};
use aestra_compiler::{
    MaterialCompileError, MaterialCompiler, MaterialGraphCreateKind, MaterialGraphEdgeTarget,
    MaterialGraphNodeCreationError, MaterialGraphOutputKind, MaterialPresetCatalog,
    MaterialStackEditError, MaterialStackModifierKind, MaterialStackProjection,
};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    MaterialId, MaterialParameterId, MaterialPresetId, MaterialProgramId, ParameterId,
    material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialFunction,
        MaterialFunctionInput, MaterialFunctionOutput, MaterialFunctionRef, MaterialParameterValue,
        MaterialProgram, MaterialSchemaVersion, MaterialValue, MaterialValueType,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Stable placement for an inserted material operation or preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialInsertionPoint {
    Start,
    Before(MaterialExpressionId),
    After(MaterialExpressionId),
    End,
}

/// Stable destination for a material expression connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialConnectionTarget {
    ExpressionInput {
        expression: MaterialExpressionId,
        input: MaterialExpressionInput,
    },
    ProgramOutput(MaterialOutputSocket),
}

/// A complete material-instance parameter source, including removal of an instance override.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialParameterBinding {
    ProgramDefault,
    Constant(MaterialValue),
    EffectParameter(ParameterId),
    EmitterParameter(ParameterId),
    RandomRange {
        min: MaterialValue,
        max: MaterialValue,
        domain: MaterialEvaluationDomain,
    },
}

/// High-level intensity source for a semantic Fresnel edge.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MaterialFresnelIntensity {
    Constant(f32),
    ParticleNormalizedAge { scale: f32 },
}

impl MaterialParameterBinding {
    fn into_override(self) -> Option<MaterialParameterValue> {
        match self {
            Self::ProgramDefault => None,
            Self::Constant(value) => Some(MaterialParameterValue::Constant(value)),
            Self::EffectParameter(parameter) => {
                Some(MaterialParameterValue::EffectParameter(parameter))
            }
            Self::EmitterParameter(parameter) => {
                Some(MaterialParameterValue::EmitterParameter(parameter))
            }
            Self::RandomRange { min, max, domain } => {
                Some(MaterialParameterValue::RandomRange { min, max, domain })
            }
        }
    }

    fn referenced_parameter(&self) -> Option<ParameterId> {
        match self {
            Self::EffectParameter(parameter) | Self::EmitterParameter(parameter) => {
                Some(*parameter)
            }
            Self::ProgramDefault | Self::Constant(_) | Self::RandomRange { .. } => None,
        }
    }
}

/// A semantic material edit request suitable for editor, CLI, and tool clients.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialToolCommand {
    BindMaterialParameter {
        instance: MaterialId,
        parameter: MaterialParameterId,
        binding: MaterialParameterBinding,
    },
    ReplaceMaterialExpression {
        program: MaterialProgramId,
        expression: MaterialExpressionId,
        replacement: MaterialExpressionKind,
    },
    WrapMaterialExpression {
        program: MaterialProgramId,
        target: MaterialConnectionTarget,
        kind: MaterialStackModifierKind,
    },
    CreateMaterialExpression {
        program: MaterialProgramId,
        source: MaterialExpressionId,
        kind: MaterialStackModifierKind,
    },
    CreateMaterialGraphNode {
        program: MaterialProgramId,
        kind: MaterialGraphCreateKind,
        source: Option<MaterialExpressionId>,
        target: Option<MaterialConnectionTarget>,
    },
    ConnectMaterialExpression {
        program: MaterialProgramId,
        source: MaterialExpressionId,
        target: MaterialConnectionTarget,
    },
    DuplicateMaterialExpressions {
        program: MaterialProgramId,
        expressions: Vec<MaterialExpressionId>,
    },
    DeleteMaterialExpressions {
        program: MaterialProgramId,
        expressions: Vec<MaterialExpressionId>,
    },
    ExtractMaterialFunction {
        program: MaterialProgramId,
        function: MaterialFunctionId,
        name: String,
        expressions: Vec<MaterialExpressionId>,
    },
    DisconnectMaterialConnection {
        program: MaterialProgramId,
        target: MaterialConnectionTarget,
    },
    InsertMaterialOperation {
        program: MaterialProgramId,
        kind: MaterialStackModifierKind,
        placement: MaterialInsertionPoint,
    },
    ApplyMaterialPreset {
        program: MaterialProgramId,
        preset: MaterialPresetId,
        placement: MaterialInsertionPoint,
    },
    /// Adds an emissive-style edge to the color output without exposing expression identities.
    AddFresnelEdge {
        program: MaterialProgramId,
        color: [f32; 4],
        power: f32,
        intensity: MaterialFresnelIntensity,
    },
}

/// A validated baseline transaction and the semantic changes it will make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialToolPlan {
    pub command: MaterialToolCommand,
    pub transaction: MaterialTransaction,
    pub diff: MaterialDiff,
    pub created_expressions: Vec<MaterialExpressionId>,
}

impl MaterialToolPlan {
    /// Returns the planned replacement for `program`, when the command replaces that program.
    pub fn replacement_program(&self, program: MaterialProgramId) -> Option<&MaterialProgram> {
        self.transaction
            .commands
            .iter()
            .find_map(|command| match command {
                MaterialCommand::ReplaceMaterialProgram {
                    id,
                    program: replacement,
                } if *id == program => Some(replacement),
                _ => None,
            })
    }

    pub fn created_function(&self) -> Option<&MaterialFunction> {
        self.transaction
            .commands
            .iter()
            .find_map(|command| match command {
                MaterialCommand::AddMaterialFunction { function, .. } => Some(function),
                _ => None,
            })
    }
}

#[derive(Debug, Error)]
pub enum MaterialToolError {
    #[error("material program '{0}' was not found")]
    ProgramNotFound(MaterialProgramId),
    #[error("material instance '{0}' was not found")]
    InstanceNotFound(MaterialId),
    #[error("material parameter '{parameter}' was not found in program '{program}'")]
    ParameterNotFound {
        program: MaterialProgramId,
        parameter: MaterialParameterId,
    },
    #[error("effect or emitter binding parameter '{0}' was not found")]
    BindingParameterNotFound(ParameterId),
    #[error("effect or emitter binding parameter '{0}' is not exposed")]
    BindingParameterNotExposed(ParameterId),
    #[error("material insertion anchor '{0}' is not present in the editable stack")]
    InsertionAnchorNotFound(MaterialExpressionId),
    #[error("source material expression '{0}' was not found")]
    SourceExpressionNotFound(MaterialExpressionId),
    #[error("{kind:?} cannot consume source material expression {expression}")]
    IncompatibleSource {
        kind: MaterialStackModifierKind,
        expression: MaterialExpressionId,
    },
    #[error("destination material expression '{0}' was not found")]
    DestinationExpressionNotFound(MaterialExpressionId),
    #[error("material expression selection is empty")]
    EmptyExpressionSelection,
    #[error("material function name cannot be empty")]
    EmptyFunctionName,
    #[error("material function ID '{0}' already exists")]
    FunctionAlreadyExists(MaterialFunctionId),
    #[error("selected material expressions must form one connected subgraph")]
    DisconnectedFunctionSelection,
    #[error("selected material expressions do not produce a value outside the selection")]
    FunctionSelectionHasNoOutput,
    #[error("material function boundary expression '{0}' has no proven value type")]
    FunctionBoundaryTypeUnavailable(MaterialExpressionId),
    #[error("material expression '{0}' cannot be deleted without invalidating a consumer")]
    ExpressionCannotBeDeleted(MaterialExpressionId),
    #[error("material connection {0:?} cannot be reset to a typed default")]
    ConnectionCannotBeDisconnected(MaterialConnectionTarget),
    #[error("{kind:?} cannot wrap {target:?} without changing graph meaning")]
    IncompatibleWrap {
        kind: MaterialStackModifierKind,
        target: MaterialConnectionTarget,
    },
    #[error("{kind:?} has more than one valid way to wrap {target:?}")]
    AmbiguousWrap {
        kind: MaterialStackModifierKind,
        target: MaterialConnectionTarget,
    },
    #[error("invalid Fresnel edge settings: {0}")]
    InvalidFresnelSettings(&'static str),
    #[error(transparent)]
    GraphNode(#[from] MaterialGraphNodeCreationError),
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error(transparent)]
    StackEdit(#[from] MaterialStackEditError),
    #[error(transparent)]
    Transaction(#[from] MaterialCommandError),
}

/// Plans high-level material edits without mutating the source document.
#[derive(Debug, Default)]
pub struct MaterialToolPlanner;

impl MaterialToolPlanner {
    pub fn plan(
        document: &MaterialAuthoringDocument,
        command: MaterialToolCommand,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        Self::plan_with_preset_catalog(
            document,
            command,
            &MaterialCompiler.material_preset_catalog(),
        )
    }

    pub fn plan_with_preset_catalog(
        document: &MaterialAuthoringDocument,
        command: MaterialToolCommand,
        preset_catalog: &MaterialPresetCatalog,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        match command {
            MaterialToolCommand::BindMaterialParameter {
                instance,
                parameter,
                binding,
            } => Self::plan_bind_material_parameter(document, instance, parameter, binding),
            MaterialToolCommand::ReplaceMaterialExpression {
                program,
                expression,
                replacement,
            } => Self::plan_replace_material_expression(document, program, expression, replacement),
            MaterialToolCommand::WrapMaterialExpression {
                program,
                target,
                kind,
            } => Self::plan_wrap_material_expression(document, program, target, kind),
            MaterialToolCommand::CreateMaterialExpression {
                program,
                source,
                kind,
            } => Self::plan_create_material_expression(document, program, source, kind),
            MaterialToolCommand::CreateMaterialGraphNode {
                program,
                kind,
                source,
                target,
            } => Self::plan_create_material_graph_node(document, program, kind, source, target),
            MaterialToolCommand::ConnectMaterialExpression {
                program,
                source,
                target,
            } => Self::plan_connect_material_expression(document, program, source, target),
            MaterialToolCommand::DuplicateMaterialExpressions {
                program,
                expressions,
            } => Self::plan_duplicate_material_expressions(document, program, expressions),
            MaterialToolCommand::DeleteMaterialExpressions {
                program,
                expressions,
            } => Self::plan_delete_material_expressions(document, program, expressions),
            MaterialToolCommand::ExtractMaterialFunction {
                program,
                function,
                name,
                expressions,
            } => {
                Self::plan_extract_material_function(document, program, function, name, expressions)
            }
            MaterialToolCommand::DisconnectMaterialConnection { program, target } => {
                Self::plan_disconnect_material_connection(document, program, target)
            }
            MaterialToolCommand::InsertMaterialOperation {
                program,
                kind,
                placement,
            } => Self::plan_insert_material_operation(document, program, kind, placement),
            MaterialToolCommand::ApplyMaterialPreset {
                program,
                preset,
                placement,
            } => Self::plan_apply_material_preset(
                document,
                preset_catalog,
                program,
                preset,
                placement,
            ),
            MaterialToolCommand::AddFresnelEdge {
                program,
                color,
                power,
                intensity,
            } => Self::plan_add_fresnel_edge(document, program, color, power, intensity),
        }
    }

    fn plan_bind_material_parameter(
        document: &MaterialAuthoringDocument,
        instance_id: MaterialId,
        parameter_id: MaterialParameterId,
        binding: MaterialParameterBinding,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let instance = document
            .effect
            .material_instances
            .iter()
            .find(|instance| instance.id == instance_id)
            .ok_or(MaterialToolError::InstanceNotFound(instance_id))?;
        let program_id = instance.program.id();
        let program = find_program(document, program_id)?;
        if !program
            .parameters
            .iter()
            .any(|parameter| parameter.id == parameter_id)
        {
            return Err(MaterialToolError::ParameterNotFound {
                program: program_id,
                parameter: parameter_id,
            });
        }
        if let Some(binding_parameter) = binding.referenced_parameter() {
            let parameter = document
                .effect
                .parameters
                .iter()
                .find(|parameter| parameter.id == binding_parameter)
                .ok_or(MaterialToolError::BindingParameterNotFound(
                    binding_parameter,
                ))?;
            if !parameter.exposed {
                return Err(MaterialToolError::BindingParameterNotExposed(
                    binding_parameter,
                ));
            }
        }

        let command = MaterialToolCommand::BindMaterialParameter {
            instance: instance_id,
            parameter: parameter_id,
            binding: binding.clone(),
        };
        let transaction = MaterialTransaction::single(
            "Bind material parameter",
            MaterialCommand::SetMaterialInstanceParameter {
                instance: instance_id,
                parameter: parameter_id,
                value: binding.into_override(),
            },
        );
        validate_plan(document, command, transaction, Vec::new())
    }

    fn plan_replace_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        expression_id: MaterialExpressionId,
        replacement: MaterialExpressionKind,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == expression_id)
        {
            return Err(MaterialToolError::DestinationExpressionNotFound(
                expression_id,
            ));
        }

        let command = MaterialToolCommand::ReplaceMaterialExpression {
            program: program_id,
            expression: expression_id,
            replacement: replacement.clone(),
        };
        let transaction = MaterialTransaction::single(
            "Replace material expression",
            MaterialCommand::ReplaceMaterialExpression {
                program: program_id,
                expression: expression_id,
                replacement: MaterialExpression {
                    id: expression_id,
                    kind: replacement,
                },
            },
        );
        validate_plan(document, command, transaction, Vec::new())
    }

    fn plan_wrap_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        target: MaterialConnectionTarget,
        kind: MaterialStackModifierKind,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let previous_source = connection_source(program, target)?;
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == previous_source)
        {
            return Err(MaterialToolError::SourceExpressionNotFound(previous_source));
        }

        let mut wrap = MaterialCompiler
            .plan_expression_wrap(program, kind, previous_source)
            .map_err(|_| MaterialToolError::IncompatibleWrap { kind, target })?;
        apply_connection(&mut wrap.replacement, target, wrap.expression)?;
        let wraps_requested_edge = connection_source(&wrap.replacement, target)
            .is_ok_and(|source| source == wrap.expression);
        let consumes_previous_source = wrap
            .replacement
            .expressions
            .iter()
            .find(|expression| expression.id == wrap.expression)
            .and_then(|expression| expression.kind.bypass_input())
            == Some(previous_source);
        let changes_only_requested_edge =
            wrap_changes_only_requested_edge(program, &wrap.replacement, target, wrap.expression);
        if !wraps_requested_edge || !consumes_previous_source || !changes_only_requested_edge {
            return Err(MaterialToolError::IncompatibleWrap { kind, target });
        }
        let command = MaterialToolCommand::WrapMaterialExpression {
            program: program_id,
            target,
            kind,
        };
        let transaction = MaterialTransaction::new(
            format!("Wrap material expression with {}", kind.display_name()),
            wrap_transaction_commands(program, &wrap.replacement, target, wrap.expression),
        );
        validate_plan(document, command, transaction, vec![wrap.expression])
    }

    fn plan_create_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        source: MaterialExpressionId,
        kind: MaterialStackModifierKind,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let created = MaterialCompiler
            .plan_expression_wrap(program, kind, source)
            .map_err(|_| MaterialToolError::IncompatibleSource {
                kind,
                expression: source,
            })?;
        let command = MaterialToolCommand::CreateMaterialExpression {
            program: program_id,
            source,
            kind,
        };
        let transaction = MaterialTransaction::new(
            format!("Create {} material expression", kind.display_name()),
            append_expression_commands(program, &created.replacement),
        );
        validate_plan(document, command, transaction, vec![created.expression])
    }

    fn plan_create_material_graph_node(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        kind: MaterialGraphCreateKind,
        source: Option<MaterialExpressionId>,
        target: Option<MaterialConnectionTarget>,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        if let Some(target) = target {
            connection_source(program, target)?;
        }
        let functions = document.material_function_library();
        let mut created = MaterialCompiler
            .plan_graph_node_creation_with_functions(program, kind, source, &functions)?;
        if let Some(target) = target {
            apply_connection(&mut created.replacement, target, created.expression)?;
        }
        let command = MaterialToolCommand::CreateMaterialGraphNode {
            program: program_id,
            kind,
            source,
            target,
        };
        let mut commands = append_expression_commands(program, &created.replacement);
        commands.extend(
            created
                .replacement
                .inline_constants
                .iter()
                .filter(|expression| !program.inline_constants.contains(expression))
                .map(|expression| MaterialCommand::SetMaterialExpressionInline {
                    program: program_id,
                    expression: *expression,
                    inline: true,
                }),
        );
        if let Some(target) = target {
            commands.push(connection_command(program_id, target, created.expression));
        }
        let transaction = MaterialTransaction::new(
            format!("Create {} material graph node", graph_node_name(kind)),
            commands,
        );
        validate_plan(document, command, transaction, vec![created.expression])
    }

    fn plan_connect_material_expression(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        source: MaterialExpressionId,
        target: MaterialConnectionTarget,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        if !program
            .expressions
            .iter()
            .any(|expression| expression.id == source)
        {
            return Err(MaterialToolError::SourceExpressionNotFound(source));
        }
        connection_source(program, target)?;
        let baseline = connection_command(program_id, target, source);
        let command = MaterialToolCommand::ConnectMaterialExpression {
            program: program_id,
            source,
            target,
        };
        let transaction = MaterialTransaction::single("Connect material expression", baseline);
        validate_plan(document, command, transaction, Vec::new())
    }

    fn plan_duplicate_material_expressions(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        expressions: Vec<MaterialExpressionId>,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let selected = expressions.iter().copied().collect::<BTreeSet<_>>();
        if selected.is_empty() {
            return Err(MaterialToolError::EmptyExpressionSelection);
        }
        for expression in &selected {
            if !program
                .expressions
                .iter()
                .any(|candidate| candidate.id == *expression)
            {
                return Err(MaterialToolError::SourceExpressionNotFound(*expression));
            }
        }

        let ordered = program
            .expressions
            .iter()
            .filter(|expression| selected.contains(&expression.id))
            .collect::<Vec<_>>();
        let mut replacement = program.clone();
        let mut remapped = BTreeMap::new();
        for expression in &ordered {
            let duplicate = next_expression_id(&replacement, remapped.values().copied());
            remapped.insert(expression.id, duplicate);
        }
        let mut created_expressions = Vec::with_capacity(ordered.len());
        for expression in ordered {
            let id = remapped[&expression.id];
            let mut kind = expression.kind.clone();
            remap_expression_sources(&mut kind, &remapped);
            replacement
                .expressions
                .push(MaterialExpression { id, kind });
            created_expressions.push(id);
        }

        let command = MaterialToolCommand::DuplicateMaterialExpressions {
            program: program_id,
            expressions,
        };
        let transaction = MaterialTransaction::single(
            "Duplicate material expressions",
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: replacement,
            },
        );
        validate_plan(document, command, transaction, created_expressions)
    }

    fn plan_delete_material_expressions(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        expressions: Vec<MaterialExpressionId>,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let selected = expressions.iter().copied().collect::<BTreeSet<_>>();
        if selected.is_empty() {
            return Err(MaterialToolError::EmptyExpressionSelection);
        }
        for expression in &selected {
            if !program
                .expressions
                .iter()
                .any(|candidate| candidate.id == *expression)
            {
                return Err(MaterialToolError::DestinationExpressionNotFound(
                    *expression,
                ));
            }
        }

        let functions = document.material_function_library();
        let projection = MaterialCompiler.project_graph_with_functions(program, None, &functions);
        let mut replacement = program.clone();
        let mut created_expressions = Vec::new();
        for edge in &projection.edges {
            if !selected.contains(&edge.source) || target_is_selected(&edge.target, &selected) {
                continue;
            }
            let Some(target) = graph_connection_target(&edge.target) else {
                continue;
            };
            let source = match surviving_bypass_source(
                program,
                &selected,
                edge.source,
                &mut BTreeSet::new(),
            ) {
                Some(source) => source,
                None => {
                    let value = edge
                        .value_type
                        .and_then(default_material_value)
                        .ok_or(MaterialToolError::ExpressionCannotBeDeleted(edge.source))?;
                    let expression = append_unique_expression(
                        &mut replacement,
                        MaterialExpressionKind::Constant(value),
                    );
                    created_expressions.push(expression);
                    expression
                }
            };
            apply_connection(&mut replacement, target, source)?;
        }
        replacement
            .expressions
            .retain(|expression| !selected.contains(&expression.id));

        let command = MaterialToolCommand::DeleteMaterialExpressions {
            program: program_id,
            expressions,
        };
        let transaction = MaterialTransaction::single(
            "Delete material expressions",
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: replacement,
            },
        );
        validate_plan(document, command, transaction, created_expressions)
    }

    fn plan_extract_material_function(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        function_id: MaterialFunctionId,
        name: String,
        expressions: Vec<MaterialExpressionId>,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err(MaterialToolError::EmptyFunctionName);
        }
        if document
            .material_functions
            .iter()
            .any(|function| function.id == function_id)
        {
            return Err(MaterialToolError::FunctionAlreadyExists(function_id));
        }
        let program = find_program(document, program_id)?;
        let mut selected = expressions.into_iter().collect::<BTreeSet<_>>();
        if selected.is_empty() {
            return Err(MaterialToolError::EmptyExpressionSelection);
        }
        for expression in &selected {
            if !program
                .expressions
                .iter()
                .any(|candidate| candidate.id == *expression)
            {
                return Err(MaterialToolError::SourceExpressionNotFound(*expression));
            }
        }

        // Inline constants are implementation details of their owning node. Absorb them into the
        // function instead of exposing surprising constant-valued signature ports.
        loop {
            let mut added = false;
            let selected_snapshot = selected.clone();
            for expression in program
                .expressions
                .iter()
                .filter(|expression| selected_snapshot.contains(&expression.id))
            {
                for dependency in expression.kind.dependencies() {
                    if program.inline_constants.contains(&dependency) && selected.insert(dependency)
                    {
                        added = true;
                    }
                }
            }
            if !added {
                break;
            }
        }

        let mut adjacency = selected
            .iter()
            .copied()
            .map(|expression| (expression, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for expression in program
            .expressions
            .iter()
            .filter(|expression| selected.contains(&expression.id))
        {
            for dependency in expression
                .kind
                .dependencies()
                .into_iter()
                .filter(|dependency| selected.contains(dependency))
            {
                adjacency
                    .get_mut(&expression.id)
                    .expect("selected expression has adjacency")
                    .insert(dependency);
                adjacency
                    .get_mut(&dependency)
                    .expect("selected dependency has adjacency")
                    .insert(expression.id);
            }
        }
        let mut visited = BTreeSet::new();
        let mut pending = vec![*selected.iter().next().expect("selection is not empty")];
        while let Some(expression) = pending.pop() {
            if visited.insert(expression) {
                pending.extend(adjacency[&expression].iter().copied());
            }
        }
        if visited.len() != selected.len() {
            return Err(MaterialToolError::DisconnectedFunctionSelection);
        }

        let functions = document.material_function_library();
        let compiler = MaterialCompiler;
        let ir = compiler.compile_with_functions(program, &functions)?;
        let projection = compiler.project_graph_with_functions(program, Some(&ir), &functions);
        let boundary_inputs = program
            .expressions
            .iter()
            .filter(|expression| selected.contains(&expression.id))
            .flat_map(|expression| expression.kind.dependencies())
            .filter(|source| !selected.contains(source))
            .collect::<BTreeSet<_>>();
        let boundary_edges = projection
            .edges
            .iter()
            .filter(|edge| {
                selected.contains(&edge.source) && !target_is_selected(&edge.target, &selected)
            })
            .cloned()
            .collect::<Vec<_>>();
        let boundary_outputs = boundary_edges
            .iter()
            .map(|edge| edge.source)
            .collect::<BTreeSet<_>>();
        if boundary_outputs.is_empty() {
            return Err(MaterialToolError::FunctionSelectionHasNoOutput);
        }

        let node_labels = projection
            .nodes
            .iter()
            .map(|node| (node.expression, node.label.clone()))
            .collect::<BTreeMap<_, _>>();
        let expression_type = |expression: MaterialExpressionId| {
            ir.source_map
                .values
                .get(&expression)
                .and_then(|value| ir.value(*value))
                .map(|value| value.value_type)
                .or_else(|| {
                    projection
                        .nodes
                        .iter()
                        .find(|node| node.expression == expression)
                        .and_then(|node| node.value_type)
                })
        };

        let mut used_names = BTreeSet::new();
        let mut input_bindings = BTreeMap::new();
        let mut function_inputs = Vec::new();
        let mut function_input_expressions = Vec::new();
        let mut remap = BTreeMap::new();
        for source in boundary_inputs {
            let value_type = expression_type(source)
                .ok_or(MaterialToolError::FunctionBoundaryTypeUnavailable(source))?;
            let input_id = MaterialFunctionInputId::new();
            let expression_id = MaterialExpressionId::new();
            let base = node_labels
                .get(&source)
                .map(|label| concise_signature_name(label))
                .unwrap_or_else(|| "Input".to_owned());
            let input_name = unique_signature_name(base, &mut used_names);
            function_inputs.push(MaterialFunctionInput {
                id: input_id,
                name: input_name,
                value_type,
            });
            function_input_expressions.push(MaterialExpression {
                id: expression_id,
                kind: MaterialExpressionKind::FunctionInput(input_id),
            });
            input_bindings.insert(input_id, source);
            remap.insert(source, expression_id);
        }

        let mut function_expressions = function_input_expressions;
        for expression in program
            .expressions
            .iter()
            .filter(|expression| selected.contains(&expression.id))
        {
            let mut expression = expression.clone();
            remap_expression_sources(&mut expression.kind, &remap);
            function_expressions.push(expression);
        }

        let mut used_output_names = BTreeSet::new();
        let mut output_calls = BTreeMap::new();
        let mut function_outputs = Vec::new();
        let mut replacement = program.clone();
        replacement
            .expressions
            .retain(|expression| !selected.contains(&expression.id));
        replacement
            .inline_constants
            .retain(|expression| !selected.contains(expression));
        for source in boundary_outputs {
            let value_type = expression_type(source)
                .ok_or(MaterialToolError::FunctionBoundaryTypeUnavailable(source))?;
            let output_id = MaterialFunctionOutputId::new();
            let base = node_labels
                .get(&source)
                .map(|label| concise_signature_name(label))
                .unwrap_or_else(|| "Output".to_owned());
            let output_name = unique_signature_name(base, &mut used_output_names);
            function_outputs.push(MaterialFunctionOutput {
                id: output_id,
                name: output_name,
                value_type,
                expression: source,
            });
            let call = append_unique_expression(
                &mut replacement,
                MaterialExpressionKind::FunctionCall {
                    function: MaterialFunctionRef::Project(function_id),
                    arguments: input_bindings.clone(),
                    output: output_id,
                },
            );
            output_calls.insert(source, call);
        }
        for edge in boundary_edges {
            let Some(target) = graph_connection_target(&edge.target) else {
                continue;
            };
            apply_connection(&mut replacement, target, output_calls[&edge.source])?;
        }

        let function = MaterialFunction {
            id: function_id,
            schema_version: MaterialSchemaVersion::CURRENT,
            name: name.clone(),
            inputs: function_inputs,
            outputs: function_outputs,
            expressions: function_expressions,
            custom_wesl: None,
        };
        let created_expressions = output_calls.values().copied().collect::<Vec<_>>();
        let command = MaterialToolCommand::ExtractMaterialFunction {
            program: program_id,
            function: function_id,
            name: name.clone(),
            expressions: selected.iter().copied().collect(),
        };
        let transaction = MaterialTransaction::new(
            format!("Extract material function {name}"),
            vec![
                MaterialCommand::AddMaterialFunction {
                    function,
                    index: document.material_functions.len(),
                },
                MaterialCommand::ReplaceMaterialProgram {
                    id: program_id,
                    program: replacement,
                },
            ],
        );
        validate_plan(document, command, transaction, created_expressions)
    }

    fn plan_disconnect_material_connection(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        target: MaterialConnectionTarget,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        connection_source(program, target)?;
        let functions = document.material_function_library();
        let projection = MaterialCompiler.project_graph_with_functions(program, None, &functions);
        let value_type = projection.edges.iter().find_map(|edge| {
            (graph_connection_target(&edge.target) == Some(target))
                .then_some(edge.value_type)
                .flatten()
        });
        let value = value_type
            .and_then(default_material_value)
            .ok_or(MaterialToolError::ConnectionCannotBeDisconnected(target))?;
        let mut replacement = program.clone();
        let expression =
            append_unique_expression(&mut replacement, MaterialExpressionKind::Constant(value));
        apply_connection(&mut replacement, target, expression)?;

        let command = MaterialToolCommand::DisconnectMaterialConnection {
            program: program_id,
            target,
        };
        let transaction = MaterialTransaction::single(
            "Reset material connection",
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: replacement,
            },
        );
        validate_plan(document, command, transaction, vec![expression])
    }

    fn plan_insert_material_operation(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        kind: MaterialStackModifierKind,
        placement: MaterialInsertionPoint,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let target_index = resolve_insertion_point(program, placement)?;
        let insert_plan = MaterialCompiler.plan_stack_insert(program, kind, target_index)?;
        let command = MaterialToolCommand::InsertMaterialOperation {
            program: program_id,
            kind,
            placement,
        };
        let transaction = MaterialTransaction::single(
            format!("Insert {} material operation", kind.display_name()),
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: insert_plan.replacement,
            },
        );
        validate_plan(document, command, transaction, vec![insert_plan.expression])
    }

    fn plan_apply_material_preset(
        document: &MaterialAuthoringDocument,
        catalog: &MaterialPresetCatalog,
        program_id: MaterialProgramId,
        preset: MaterialPresetId,
        placement: MaterialInsertionPoint,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        let program = find_program(document, program_id)?;
        let target_index = resolve_insertion_point(program, placement)?;
        let compiler = MaterialCompiler;
        let preset_plan = compiler.plan_stack_insert_preset_with_catalog(
            program,
            catalog,
            preset,
            target_index,
        )?;
        let preset_name = catalog
            .get(preset)
            .map(|descriptor| descriptor.display_name.clone())
            .unwrap_or_else(|| preset.to_string());
        let command = MaterialToolCommand::ApplyMaterialPreset {
            program: program_id,
            preset,
            placement,
        };
        let transaction = MaterialTransaction::single(
            format!("Apply {preset_name} preset"),
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: preset_plan.replacement,
            },
        );

        validate_plan(document, command, transaction, preset_plan.expressions)
    }

    fn plan_add_fresnel_edge(
        document: &MaterialAuthoringDocument,
        program_id: MaterialProgramId,
        color: [f32; 4],
        power: f32,
        intensity: MaterialFresnelIntensity,
    ) -> Result<MaterialToolPlan, MaterialToolError> {
        if !color.into_iter().all(f32::is_finite) {
            return Err(MaterialToolError::InvalidFresnelSettings(
                "color components must be finite",
            ));
        }
        if !power.is_finite() || power <= 0.0 {
            return Err(MaterialToolError::InvalidFresnelSettings(
                "power must be finite and greater than zero",
            ));
        }
        let intensity_scale = match intensity {
            MaterialFresnelIntensity::Constant(value) => value,
            MaterialFresnelIntensity::ParticleNormalizedAge { scale } => scale,
        };
        if !intensity_scale.is_finite() || intensity_scale < 0.0 {
            return Err(MaterialToolError::InvalidFresnelSettings(
                "intensity must be finite and non-negative",
            ));
        }

        let program = find_program(document, program_id)?;
        let mut replacement = program.clone();
        let source_color = replacement.outputs.color;
        let normal = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Input(aestra_core::material::MaterialInput::Normal),
        );
        let view = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Input(aestra_core::material::MaterialInput::ViewDirection),
        );
        let power_expression = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Constant(MaterialValue::Float(power)),
        );
        let fresnel = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Fresnel {
                normal,
                view,
                power: power_expression,
            },
        );
        let color_expression = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Constant(MaterialValue::ColorSrgb(color)),
        );
        let colored_edge = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Multiply(color_expression, fresnel),
        );
        let intensity_expression = match intensity {
            MaterialFresnelIntensity::Constant(value) => append_unique_expression(
                &mut replacement,
                MaterialExpressionKind::Constant(MaterialValue::Float(value)),
            ),
            MaterialFresnelIntensity::ParticleNormalizedAge { scale } => {
                let age = append_unique_expression(
                    &mut replacement,
                    MaterialExpressionKind::Input(
                        aestra_core::material::MaterialInput::ParticleNormalizedAge,
                    ),
                );
                let scale = append_unique_expression(
                    &mut replacement,
                    MaterialExpressionKind::Constant(MaterialValue::Float(scale)),
                );
                append_unique_expression(
                    &mut replacement,
                    MaterialExpressionKind::Multiply(age, scale),
                )
            }
        };
        let driven_edge = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Multiply(colored_edge, intensity_expression),
        );
        let output = append_unique_expression(
            &mut replacement,
            MaterialExpressionKind::Add(source_color, driven_edge),
        );
        replacement.outputs.color = output;

        let created_expressions = replacement.expressions[program.expressions.len()..]
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>();
        let command = MaterialToolCommand::AddFresnelEdge {
            program: program_id,
            color,
            power,
            intensity,
        };
        let transaction = MaterialTransaction::single(
            "Add Fresnel edge",
            MaterialCommand::ReplaceMaterialProgram {
                id: program_id,
                program: replacement,
            },
        );
        validate_plan(document, command, transaction, created_expressions)
    }
}

fn append_unique_expression(
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

fn next_expression_id(
    program: &MaterialProgram,
    reserved: impl IntoIterator<Item = MaterialExpressionId>,
) -> MaterialExpressionId {
    let reserved = reserved.into_iter().collect::<BTreeSet<_>>();
    loop {
        let id = MaterialExpressionId::new();
        if !reserved.contains(&id)
            && !program
                .expressions
                .iter()
                .any(|expression| expression.id == id)
        {
            return id;
        }
    }
}

fn remap_expression_sources(
    kind: &mut MaterialExpressionKind,
    remapped: &BTreeMap<MaterialExpressionId, MaterialExpressionId>,
) {
    let remap = |source: &mut MaterialExpressionId| {
        if let Some(replacement) = remapped.get(source) {
            *source = *replacement;
        }
    };
    match kind {
        MaterialExpressionKind::Constant(_)
        | MaterialExpressionKind::Input(_)
        | MaterialExpressionKind::Parameter(_)
        | MaterialExpressionKind::FunctionInput(_) => {}
        MaterialExpressionKind::FunctionCall { arguments, .. } => {
            for source in arguments.values_mut() {
                remap(source);
            }
        }
        MaterialExpressionKind::CustomWeslCall { arguments, .. } => {
            for argument in arguments {
                remap(&mut argument.expression);
            }
        }
        MaterialExpressionKind::Add(left, right)
        | MaterialExpressionKind::Subtract(left, right)
        | MaterialExpressionKind::Multiply(left, right)
        | MaterialExpressionKind::Divide(left, right) => {
            remap(left);
            remap(right);
        }
        MaterialExpressionKind::Lerp { start, end, factor } => {
            remap(start);
            remap(end);
            remap(factor);
        }
        MaterialExpressionKind::Clamp { value, min, max } => {
            remap(value);
            remap(min);
            remap(max);
        }
        MaterialExpressionKind::Select {
            condition,
            if_false,
            if_true,
        } => {
            remap(condition);
            remap(if_false);
            remap(if_true);
        }
        MaterialExpressionKind::Remap {
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
        MaterialExpressionKind::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => {
            remap(edge_min);
            remap(edge_max);
            remap(value);
        }
        MaterialExpressionKind::Fresnel {
            normal,
            view,
            power,
        } => {
            remap(normal);
            remap(view);
            remap(power);
        }
        MaterialExpressionKind::RadialMask {
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
        } => {
            remap(source);
            remap(threshold);
            remap(edge_width);
            remap(invert);
        }
        MaterialExpressionKind::DepthFade {
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
        MaterialExpressionKind::SoftParticle {
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
        MaterialExpressionKind::PanUv { uv, speed, time } => {
            remap(uv);
            remap(speed);
            remap(time);
        }
        MaterialExpressionKind::RotateUv { uv, center, angle } => {
            remap(uv);
            remap(center);
            remap(angle);
        }
        MaterialExpressionKind::ScaleUv { uv, center, scale } => {
            remap(uv);
            remap(center);
            remap(scale);
        }
        MaterialExpressionKind::SampleTexture { texture, uv } => {
            remap(texture);
            remap(uv);
        }
        MaterialExpressionKind::SampleTextureLevel { texture, uv, level } => {
            remap(texture);
            remap(uv);
            remap(level);
        }
        MaterialExpressionKind::ExtractComponent { value, .. } => remap(value),
    }
}

fn target_is_selected(
    target: &MaterialGraphEdgeTarget,
    selected: &BTreeSet<MaterialExpressionId>,
) -> bool {
    match target {
        MaterialGraphEdgeTarget::Input { expression, .. }
        | MaterialGraphEdgeTarget::FunctionInput { expression, .. } => {
            selected.contains(expression)
        }
        MaterialGraphEdgeTarget::Output(_) => false,
    }
}

fn surviving_bypass_source(
    program: &MaterialProgram,
    selected: &BTreeSet<MaterialExpressionId>,
    expression: MaterialExpressionId,
    visiting: &mut BTreeSet<MaterialExpressionId>,
) -> Option<MaterialExpressionId> {
    if !selected.contains(&expression) {
        return Some(expression);
    }
    if !visiting.insert(expression) {
        return None;
    }
    let source = program
        .expressions
        .iter()
        .find(|candidate| candidate.id == expression)?
        .kind
        .bypass_input()?;
    let result = surviving_bypass_source(program, selected, source, visiting);
    visiting.remove(&expression);
    result
}

fn graph_connection_target(target: &MaterialGraphEdgeTarget) -> Option<MaterialConnectionTarget> {
    match target {
        MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Color) => Some(
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color),
        ),
        MaterialGraphEdgeTarget::Output(MaterialGraphOutputKind::Alpha) => Some(
            MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha),
        ),
        MaterialGraphEdgeTarget::FunctionInput { expression, input } => {
            Some(MaterialConnectionTarget::ExpressionInput {
                expression: *expression,
                input: MaterialExpressionInput::FunctionArgument(*input),
            })
        }
        MaterialGraphEdgeTarget::Input { expression, port } => {
            let input = match port.as_str() {
                "left" => MaterialExpressionInput::Left,
                "right" => MaterialExpressionInput::Right,
                "start" => MaterialExpressionInput::Start,
                "end" => MaterialExpressionInput::End,
                "factor" => MaterialExpressionInput::Factor,
                "value" => MaterialExpressionInput::Value,
                "min" => MaterialExpressionInput::Minimum,
                "max" => MaterialExpressionInput::Maximum,
                "condition" => MaterialExpressionInput::Condition,
                "false" => MaterialExpressionInput::IfFalse,
                "true" => MaterialExpressionInput::IfTrue,
                "input_min" => MaterialExpressionInput::InputMinimum,
                "input_max" => MaterialExpressionInput::InputMaximum,
                "output_min" => MaterialExpressionInput::OutputMinimum,
                "output_max" => MaterialExpressionInput::OutputMaximum,
                "edge_min" => MaterialExpressionInput::EdgeMinimum,
                "edge_max" => MaterialExpressionInput::EdgeMaximum,
                "normal" => MaterialExpressionInput::Normal,
                "view" => MaterialExpressionInput::View,
                "power" => MaterialExpressionInput::Power,
                "radius" => MaterialExpressionInput::Radius,
                "softness" => MaterialExpressionInput::Softness,
                "threshold" => MaterialExpressionInput::Threshold,
                "edge_width" => MaterialExpressionInput::EdgeWidth,
                "scene_depth" => MaterialExpressionInput::SceneDepth,
                "pixel_depth" => MaterialExpressionInput::PixelDepth,
                "fade_distance" => MaterialExpressionInput::FadeDistance,
                "invert" => MaterialExpressionInput::Invert,
                "speed" => MaterialExpressionInput::Speed,
                "time" => MaterialExpressionInput::Time,
                "center" => MaterialExpressionInput::Center,
                "angle" => MaterialExpressionInput::Angle,
                "scale" => MaterialExpressionInput::Scale,
                "texture" => MaterialExpressionInput::Texture,
                "uv" => MaterialExpressionInput::Uv,
                "source" => MaterialExpressionInput::Source,
                "alpha" => MaterialExpressionInput::SourceAlpha,
                _ => return None,
            };
            Some(MaterialConnectionTarget::ExpressionInput {
                expression: *expression,
                input,
            })
        }
    }
}

fn default_material_value(value_type: MaterialValueType) -> Option<MaterialValue> {
    match value_type {
        MaterialValueType::Float => Some(MaterialValue::Float(0.0)),
        MaterialValueType::Vec2 => Some(MaterialValue::Vec2([0.0; 2])),
        MaterialValueType::Vec3 => Some(MaterialValue::Vec3([0.0; 3])),
        MaterialValueType::Vec4 => Some(MaterialValue::Vec4([0.0; 4])),
        MaterialValueType::Color => Some(MaterialValue::ColorSrgb([0.0, 0.0, 0.0, 1.0])),
        MaterialValueType::Bool => Some(MaterialValue::Bool(false)),
        MaterialValueType::Texture2D(_) => None,
    }
}

fn wrap_changes_only_requested_edge(
    before: &MaterialProgram,
    after: &MaterialProgram,
    target: MaterialConnectionTarget,
    wrapper: MaterialExpressionId,
) -> bool {
    if after.expressions.len() <= before.expressions.len()
        || after.expressions[..before.expressions.len()]
            .iter()
            .map(|expression| expression.id)
            .ne(before.expressions.iter().map(|expression| expression.id))
    {
        return false;
    }
    let mut expected = before.clone();
    expected
        .expressions
        .extend_from_slice(&after.expressions[before.expressions.len()..]);
    if apply_connection(&mut expected, target, wrapper).is_err() {
        return false;
    }
    expected == *after
}

fn wrap_transaction_commands(
    before: &MaterialProgram,
    after: &MaterialProgram,
    target: MaterialConnectionTarget,
    wrapper: MaterialExpressionId,
) -> Vec<MaterialCommand> {
    let mut commands = append_expression_commands(before, after);
    commands.push(connection_command(before.id, target, wrapper));
    commands
}

fn append_expression_commands(
    before: &MaterialProgram,
    after: &MaterialProgram,
) -> Vec<MaterialCommand> {
    after.expressions[before.expressions.len()..]
        .iter()
        .cloned()
        .enumerate()
        .map(
            |(offset, expression)| MaterialCommand::AddMaterialExpression {
                program: before.id,
                expression,
                index: before.expressions.len() + offset,
            },
        )
        .collect()
}

fn apply_connection(
    program: &mut MaterialProgram,
    target: MaterialConnectionTarget,
    source: MaterialExpressionId,
) -> Result<(), MaterialCommandError> {
    match target {
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color) => {
            program.outputs.color = source;
        }
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha) => {
            program.outputs.alpha = source;
        }
        MaterialConnectionTarget::ExpressionInput { expression, input } => {
            let destination = program
                .expressions
                .iter_mut()
                .find(|candidate| candidate.id == expression)
                .ok_or_else(|| MaterialCommandError::NotFound {
                    kind: "material expression",
                    id: expression.to_string(),
                })?;
            rewire_expression(&mut destination.kind, input, source)?;
        }
    }
    Ok(())
}

fn connection_command(
    program: MaterialProgramId,
    target: MaterialConnectionTarget,
    source: MaterialExpressionId,
) -> MaterialCommand {
    match target {
        MaterialConnectionTarget::ExpressionInput { expression, input } => {
            MaterialCommand::RewireMaterialExpressionInput {
                program,
                expression,
                input,
                source,
            }
        }
        MaterialConnectionTarget::ProgramOutput(output) => MaterialCommand::SetMaterialOutput {
            program,
            output,
            expression: source,
        },
    }
}

fn find_program(
    document: &MaterialAuthoringDocument,
    program_id: MaterialProgramId,
) -> Result<&MaterialProgram, MaterialToolError> {
    document
        .programs
        .iter()
        .find(|program| program.id == program_id)
        .ok_or(MaterialToolError::ProgramNotFound(program_id))
}

fn graph_node_name(kind: MaterialGraphCreateKind) -> String {
    match kind {
        MaterialGraphCreateKind::Constant(value_type) => format!("{value_type:?} constant"),
        MaterialGraphCreateKind::Input(input) => format!("{input:?}"),
        MaterialGraphCreateKind::Parameter(parameter) => format!("parameter {parameter}"),
        MaterialGraphCreateKind::FunctionCall { function, .. } => {
            format!("material function {}", function.id())
        }
        MaterialGraphCreateKind::Function(function) => function.display_name().to_owned(),
        MaterialGraphCreateKind::ExtractComponent(component) => format!("Extract {component:?}"),
    }
}

fn concise_signature_name(label: &str) -> String {
    let name = label
        .split('·')
        .next()
        .unwrap_or(label)
        .trim()
        .trim_start_matches("Constant")
        .trim()
        .to_owned();
    if name.is_empty() {
        "Value".to_owned()
    } else {
        name
    }
}

fn unique_signature_name(base: String, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    for suffix in 2.. {
        let candidate = format!("{base} {suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("an unused material function signature name always exists")
}

fn connection_source(
    program: &MaterialProgram,
    target: MaterialConnectionTarget,
) -> Result<MaterialExpressionId, MaterialToolError> {
    match target {
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Color) => {
            Ok(program.outputs.color)
        }
        MaterialConnectionTarget::ProgramOutput(MaterialOutputSocket::Alpha) => {
            Ok(program.outputs.alpha)
        }
        MaterialConnectionTarget::ExpressionInput { expression, input } => {
            let destination = program
                .expressions
                .iter()
                .find(|candidate| candidate.id == expression)
                .ok_or(MaterialToolError::DestinationExpressionNotFound(expression))?;
            material_expression_input_source(&destination.kind, input)
                .map_err(MaterialToolError::Transaction)
        }
    }
}

fn resolve_insertion_point(
    program: &MaterialProgram,
    placement: MaterialInsertionPoint,
) -> Result<usize, MaterialToolError> {
    let projection = MaterialCompiler
        .project_stack(program)
        .map_err(MaterialStackEditError::from)?;
    let MaterialStackProjection::Stack { entries } = projection else {
        return Err(MaterialStackEditError::Advanced.into());
    };
    match placement {
        MaterialInsertionPoint::Start => Ok(0),
        MaterialInsertionPoint::End => Ok(entries.len()),
        MaterialInsertionPoint::Before(anchor) => entries
            .iter()
            .position(|entry| entry.expression == anchor)
            .ok_or(MaterialToolError::InsertionAnchorNotFound(anchor)),
        MaterialInsertionPoint::After(anchor) => entries
            .iter()
            .position(|entry| entry.expression == anchor)
            .map(|index| index + 1)
            .ok_or(MaterialToolError::InsertionAnchorNotFound(anchor)),
    }
}

fn validate_plan(
    document: &MaterialAuthoringDocument,
    command: MaterialToolCommand,
    transaction: MaterialTransaction,
    created_expressions: Vec<MaterialExpressionId>,
) -> Result<MaterialToolPlan, MaterialToolError> {
    let mut preview = document.clone();
    let outcome = MaterialCommandExecutor::execute(&mut preview, &transaction)?;
    Ok(MaterialToolPlan {
        command,
        transaction,
        diff: outcome.diff,
        created_expressions,
    })
}
