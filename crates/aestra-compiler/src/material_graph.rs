//! Backend-neutral, read-only projection of a semantic material program as a graph.

use crate::{
    MaterialCompileError, MaterialCompiler, MaterialFunctionLibrary, MaterialIrProgram,
    MaterialIrValueId, MaterialStackModifierKind, material_stack::append_default_modifier,
};
pub use aestra_core::material::MaterialGraphFunction;
use aestra_core::{
    MaterialExpressionId, MaterialFunctionInputId, MaterialFunctionOutputId, MaterialParameterId,
    MaterialProgramId, ValidationReport,
    material::{
        MaterialExpression, MaterialExpressionDomain, MaterialExpressionKind, MaterialFunctionRef,
        MaterialInput, MaterialProgram, MaterialValue, MaterialValueType, MaterialVectorComponent,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphNodeKind {
    Constant,
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    FunctionInput(MaterialFunctionInputId),
    FunctionCall(MaterialFunctionRef),
    Function(MaterialGraphFunction),
    ExtractComponent(MaterialVectorComponent),
}

/// A compiler-owned node kind that can be created by graph authoring clients.
///
/// Keeping this catalog outside the editor gives UI, CLI, and AI clients the same list of
/// constructible nodes and the same typed defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphCreateKind {
    Constant(MaterialValueType),
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    FunctionCall {
        function: MaterialFunctionRef,
        output: MaterialFunctionOutputId,
    },
    Function(MaterialGraphFunction),
    ExtractComponent(MaterialVectorComponent),
}

impl MaterialGraphCreateKind {
    pub const fn consumes_source(self) -> bool {
        matches!(
            self,
            Self::FunctionCall { .. } | Self::Function(_) | Self::ExtractComponent(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphNodeDescriptor {
    pub kind: MaterialGraphCreateKind,
    pub label: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialGraphNodeCreationPlan {
    pub expression: MaterialExpressionId,
    pub created_expressions: Vec<MaterialExpressionId>,
    pub replacement: MaterialProgram,
}

#[derive(Debug, Error)]
pub enum MaterialGraphNodeCreationError {
    #[error(transparent)]
    Compile(#[from] MaterialCompileError),
    #[error("source material expression {expression} is unavailable")]
    SourceMissing { expression: MaterialExpressionId },
    #[error("{kind:?} cannot consume material expression {expression}")]
    IncompatibleSource {
        kind: MaterialGraphCreateKind,
        expression: MaterialExpressionId,
    },
    #[error("Sample Texture requires a declared Texture2D material parameter")]
    TextureParameterMissing,
    #[error("texture constants are resources and cannot be created as literal graph nodes")]
    TextureConstantUnsupported,
    #[error("material function {function:?} is unavailable")]
    FunctionUnavailable { function: MaterialFunctionRef },
    #[error("material function {function:?} has no output {output}")]
    FunctionOutputUnavailable {
        function: MaterialFunctionRef,
        output: MaterialFunctionOutputId,
    },
    #[error("material function input '{input}' requires a resource connection")]
    FunctionInputDefaultUnavailable { input: String },
}

const MATERIAL_INPUTS: [MaterialInput; 28] = [
    MaterialInput::Uv0,
    MaterialInput::Uv1,
    MaterialInput::LocalPosition,
    MaterialInput::WorldPosition,
    MaterialInput::Normal,
    MaterialInput::Tangent,
    MaterialInput::Bitangent,
    MaterialInput::ViewDirection,
    MaterialInput::ScreenUv,
    MaterialInput::ParticleColor,
    MaterialInput::ParticleOpacity,
    MaterialInput::ParticleAge,
    MaterialInput::ParticleNormalizedAge,
    MaterialInput::ParticleLifetime,
    MaterialInput::ParticleVelocity,
    MaterialInput::ParticleSpeed,
    MaterialInput::ParticleRandom,
    MaterialInput::ParticleId,
    MaterialInput::ParticleSize,
    MaterialInput::ParticleRotation,
    MaterialInput::EffectTime,
    MaterialInput::EmitterTime,
    MaterialInput::EffectNormalizedTime,
    MaterialInput::EmitterNormalizedTime,
    MaterialInput::SceneDepth,
    MaterialInput::CameraPosition,
    MaterialInput::CameraDirection,
    MaterialInput::PixelDepth,
];

const MATERIAL_FUNCTIONS: [MaterialGraphFunction; 23] = [
    MaterialGraphFunction::Add,
    MaterialGraphFunction::Subtract,
    MaterialGraphFunction::Multiply,
    MaterialGraphFunction::Divide,
    MaterialGraphFunction::Lerp,
    MaterialGraphFunction::Clamp,
    MaterialGraphFunction::Select,
    MaterialGraphFunction::Remap,
    MaterialGraphFunction::Smoothstep,
    MaterialGraphFunction::Fresnel,
    MaterialGraphFunction::RadialMask,
    MaterialGraphFunction::Dissolve,
    MaterialGraphFunction::DissolveEdge,
    MaterialGraphFunction::DepthFade,
    MaterialGraphFunction::SoftParticle,
    MaterialGraphFunction::PanUv,
    MaterialGraphFunction::RotateUv,
    MaterialGraphFunction::ScaleUv,
    MaterialGraphFunction::DerivativeX,
    MaterialGraphFunction::DerivativeY,
    MaterialGraphFunction::SampleTexture,
    MaterialGraphFunction::SampleTextureLevel,
    MaterialGraphFunction::SampleTextureGradient,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphPort {
    /// Stable semantic socket name within the node.
    pub name: String,
    /// Stable function signature identity when this is a function-call input.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function_input: Option<MaterialFunctionInputId>,
    pub source: MaterialExpressionId,
    /// The connected value's analyzed type. Absent when validation failed.
    pub value_type: Option<MaterialValueType>,
    /// The connected value's analyzed evaluation domain. Absent when validation failed.
    pub evaluation_domain: Option<MaterialExpressionDomain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphNode {
    pub expression: MaterialExpressionId,
    pub kind: MaterialGraphNodeKind,
    pub label: String,
    pub inputs: Vec<MaterialGraphPort>,
    pub value_type: Option<MaterialValueType>,
    pub evaluation_domain: Option<MaterialExpressionDomain>,
    pub disabled: bool,
    pub reachable: bool,
    /// Function-resolution error associated with this node, when one is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_message: Option<String>,
    /// Optional link into optimized compiler IR. Multiple expressions can alias one IR value.
    pub ir_value: Option<MaterialIrValueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaterialGraphOutputKind {
    Color,
    Alpha,
    VertexOffset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphOutput {
    pub kind: MaterialGraphOutputKind,
    /// Nil only for an unconnected optional output; no edge is emitted for that socket.
    pub source: MaterialExpressionId,
    pub value_type: Option<MaterialValueType>,
    pub evaluation_domain: Option<MaterialExpressionDomain>,
    pub ir_value: Option<MaterialIrValueId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphEdgeTarget {
    Input {
        expression: MaterialExpressionId,
        port: String,
    },
    FunctionInput {
        expression: MaterialExpressionId,
        input: MaterialFunctionInputId,
    },
    Output(MaterialGraphOutputKind),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphEdge {
    pub source: MaterialExpressionId,
    pub target: MaterialGraphEdgeTarget,
    pub value_type: Option<MaterialValueType>,
    pub evaluation_domain: Option<MaterialExpressionDomain>,
}

/// Deterministic graph data suitable for editors, CLI tools, and AI inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphProjection {
    pub program: MaterialProgramId,
    pub nodes: Vec<MaterialGraphNode>,
    pub edges: Vec<MaterialGraphEdge>,
    pub outputs: Vec<MaterialGraphOutput>,
    pub diagnostics: ValidationReport,
}

impl MaterialCompiler {
    /// Returns the canonical graph-node catalog for one program.
    pub fn graph_node_catalog(
        &self,
        program: &MaterialProgram,
    ) -> Vec<MaterialGraphNodeDescriptor> {
        self.graph_node_catalog_with_functions(program, &MaterialFunctionLibrary::default())
    }

    /// Returns the canonical graph-node catalog, including reusable built-in and project
    /// material functions from the supplied library.
    pub fn graph_node_catalog_with_functions(
        &self,
        program: &MaterialProgram,
        functions: &MaterialFunctionLibrary,
    ) -> Vec<MaterialGraphNodeDescriptor> {
        let mut nodes = [
            MaterialValueType::Float,
            MaterialValueType::Vec2,
            MaterialValueType::Vec3,
            MaterialValueType::Vec4,
            MaterialValueType::Color,
            MaterialValueType::Bool,
        ]
        .into_iter()
        .map(|value_type| MaterialGraphNodeDescriptor {
            kind: MaterialGraphCreateKind::Constant(value_type),
            label: constant_label(value_type).to_owned(),
            category: "Constants".to_owned(),
        })
        .collect::<Vec<_>>();
        nodes.extend(
            MATERIAL_INPUTS
                .into_iter()
                .map(|input| MaterialGraphNodeDescriptor {
                    kind: MaterialGraphCreateKind::Input(input),
                    label: format!("{input:?}"),
                    category: "Inputs".to_owned(),
                }),
        );
        nodes.extend(
            program
                .parameters
                .iter()
                .map(|parameter| MaterialGraphNodeDescriptor {
                    kind: MaterialGraphCreateKind::Parameter(parameter.id),
                    label: parameter.name.clone(),
                    category: "Parameters".to_owned(),
                }),
        );
        nodes.extend(
            MATERIAL_FUNCTIONS
                .into_iter()
                .map(|function| MaterialGraphNodeDescriptor {
                    kind: MaterialGraphCreateKind::Function(function),
                    label: function.display_name().to_owned(),
                    category: function.category().to_owned(),
                }),
        );
        nodes.extend(
            functions
                .iter_with_references()
                .flat_map(|(reference, function)| {
                    function
                        .outputs
                        .iter()
                        .map(move |output| MaterialGraphNodeDescriptor {
                            kind: MaterialGraphCreateKind::FunctionCall {
                                function: reference,
                                output: output.id,
                            },
                            label: if function.outputs.len() == 1 {
                                function.name.clone()
                            } else {
                                format!("{} · {}", function.name, output.name)
                            },
                            category: match reference {
                                MaterialFunctionRef::BuiltIn(_) => "Built-in Functions".to_owned(),
                                MaterialFunctionRef::Project(_) => "Project Functions".to_owned(),
                            },
                        })
                }),
        );
        nodes.extend(
            [
                MaterialVectorComponent::X,
                MaterialVectorComponent::Y,
                MaterialVectorComponent::Z,
                MaterialVectorComponent::W,
            ]
            .into_iter()
            .map(|component| MaterialGraphNodeDescriptor {
                kind: MaterialGraphCreateKind::ExtractComponent(component),
                label: format!("Extract {component:?}"),
                category: "Math".to_owned(),
            }),
        );
        nodes
    }

    /// Builds one graph node and any typed default expressions it requires without rewiring a
    /// consumer. The returned replacement is fully compiler-validated.
    pub fn plan_graph_node_creation(
        &self,
        program: &MaterialProgram,
        kind: MaterialGraphCreateKind,
        source: Option<MaterialExpressionId>,
    ) -> Result<MaterialGraphNodeCreationPlan, MaterialGraphNodeCreationError> {
        self.plan_graph_node_creation_with_functions(
            program,
            kind,
            source,
            &MaterialFunctionLibrary::default(),
        )
    }

    /// Builds a graph node using the complete reusable function environment.
    pub fn plan_graph_node_creation_with_functions(
        &self,
        program: &MaterialProgram,
        kind: MaterialGraphCreateKind,
        source: Option<MaterialExpressionId>,
        functions: &MaterialFunctionLibrary,
    ) -> Result<MaterialGraphNodeCreationPlan, MaterialGraphNodeCreationError> {
        if let Some(source) = source
            && !program
                .expressions
                .iter()
                .any(|expression| expression.id == source)
        {
            return Err(MaterialGraphNodeCreationError::SourceMissing { expression: source });
        }
        let mut replacement = program.clone();
        let first_created = replacement.expressions.len();
        let expression = match kind {
            MaterialGraphCreateKind::Constant(value_type) => {
                let value = default_value(value_type, false)
                    .ok_or(MaterialGraphNodeCreationError::TextureConstantUnsupported)?;
                append_graph_expression(&mut replacement, MaterialExpressionKind::Constant(value))
            }
            MaterialGraphCreateKind::Input(input) => {
                append_graph_expression(&mut replacement, MaterialExpressionKind::Input(input))
            }
            MaterialGraphCreateKind::Parameter(parameter) => append_graph_expression(
                &mut replacement,
                MaterialExpressionKind::Parameter(parameter),
            ),
            MaterialGraphCreateKind::FunctionCall { function, output } => {
                append_graph_function_call(
                    self,
                    &mut replacement,
                    functions,
                    function,
                    output,
                    source,
                )?
            }
            MaterialGraphCreateKind::Function(function) => {
                append_graph_function(&mut replacement, function, source)?
            }
            MaterialGraphCreateKind::ExtractComponent(component) => {
                let value = source.unwrap_or_else(|| {
                    append_graph_constant(
                        &mut replacement,
                        MaterialValue::Vec4([0.0, 0.0, 0.0, 1.0]),
                    )
                });
                append_graph_expression(
                    &mut replacement,
                    MaterialExpressionKind::ExtractComponent { value, component },
                )
            }
        };
        if !matches!(kind, MaterialGraphCreateKind::Constant(_)) {
            let inline_constants = replacement.expressions[first_created..]
                .iter()
                .filter_map(|candidate| {
                    (candidate.id != expression
                        && matches!(&candidate.kind, MaterialExpressionKind::Constant(_)))
                    .then_some(candidate.id)
                })
                .collect::<Vec<_>>();
            replacement.inline_constants.extend(inline_constants);
        }
        self.compile_with_functions(&replacement, functions)?;
        let created_expressions = replacement.expressions[first_created..]
            .iter()
            .map(|expression| expression.id)
            .collect();
        Ok(MaterialGraphNodeCreationPlan {
            expression,
            created_expressions,
            replacement,
        })
    }

    /// Projects every authored expression, including disabled and unreachable expressions.
    ///
    /// Projection never fails: invalid programs remain inspectable, while ports whose types could
    /// not be proven carry `None`. Passing compiled IR adds source-map links without making the
    /// graph dependent on a renderer backend.
    pub fn project_graph(
        &self,
        program: &MaterialProgram,
        ir: Option<&MaterialIrProgram>,
    ) -> MaterialGraphProjection {
        self.project_graph_with_functions(program, ir, &MaterialFunctionLibrary::default())
    }

    /// Projects a program with signature-aware material-function nodes and ports.
    pub fn project_graph_with_functions(
        &self,
        program: &MaterialProgram,
        ir: Option<&MaterialIrProgram>,
        functions: &MaterialFunctionLibrary,
    ) -> MaterialGraphProjection {
        let program = program.normalized();
        let mut diagnostics = program.validation_report();
        diagnostics
            .diagnostics
            .extend(functions.validation_report().diagnostics);
        if let Err(MaterialCompileError::Validation(report)) =
            self.compile_with_functions(&program, functions)
        {
            diagnostics.diagnostics.extend(report.diagnostics);
        }
        diagnostics.diagnostics.sort();
        diagnostics.diagnostics.dedup();
        let analysis = program.analyze().ok();
        let info = analysis
            .as_ref()
            .map(|analysis| &analysis.expressions)
            .cloned()
            .unwrap_or_default();
        let reachable = reachable_expressions(&program);
        let disabled = program
            .disabled_expressions
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let ir_values = ir
            .filter(|ir| ir.source == program.id)
            .map(|ir| &ir.source_map.values);

        let inline_constants = program
            .inline_constants
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let nodes = program
            .expressions
            .iter()
            .filter(|expression| !inline_constants.contains(&expression.id))
            .map(|expression| {
                let inputs = expression_ports(&expression.kind, functions)
                    .into_iter()
                    .map(|(name, source, function_input)| {
                        let source_info = info.get(&source);
                        let ir_info = ir_values
                            .and_then(|values| values.get(&source))
                            .and_then(|value| ir.and_then(|ir| ir.value(*value)));
                        let signature_type = function_input.and_then(|input| {
                            let MaterialExpressionKind::FunctionCall { function, .. } =
                                &expression.kind
                            else {
                                return None;
                            };
                            functions
                                .get(*function)
                                .and_then(|function| {
                                    function
                                        .inputs
                                        .iter()
                                        .find(|candidate| candidate.id == input)
                                })
                                .map(|input| input.value_type)
                        });
                        MaterialGraphPort {
                            name,
                            function_input,
                            source,
                            value_type: signature_type.or_else(|| {
                                source_info
                                    .map(|info| info.value_type)
                                    .or_else(|| ir_info.map(|info| info.value_type))
                            }),
                            evaluation_domain: source_info
                                .map(|info| info.evaluation_domain)
                                .or_else(|| ir_info.map(|info| info.evaluation_domain)),
                        }
                    })
                    .collect();
                let expression_info = info.get(&expression.id);
                let ir_info = ir_values
                    .and_then(|values| values.get(&expression.id))
                    .and_then(|value| ir.and_then(|ir| ir.value(*value)));
                let signature_type = match &expression.kind {
                    MaterialExpressionKind::FunctionCall {
                        function, output, ..
                    } => functions
                        .get(*function)
                        .and_then(|function| {
                            function
                                .outputs
                                .iter()
                                .find(|candidate| candidate.id == *output)
                        })
                        .map(|output| output.value_type),
                    _ => None,
                };
                let validation_message = match &expression.kind {
                    MaterialExpressionKind::FunctionCall { function, .. } => diagnostics
                        .diagnostics
                        .iter()
                        .find(|diagnostic| {
                            let id = function.id().to_string();
                            diagnostic.path.contains(&expression.id.to_string())
                                || diagnostic.path.contains(&id)
                                || diagnostic.message.contains(&id)
                        })
                        .map(|diagnostic| diagnostic.message.clone()),
                    _ => None,
                };
                MaterialGraphNode {
                    expression: expression.id,
                    kind: node_kind(&expression.kind),
                    label: node_label(&expression.kind, &program, functions),
                    inputs,
                    value_type: signature_type.or_else(|| {
                        expression_info
                            .map(|info| info.value_type)
                            .or_else(|| ir_info.map(|info| info.value_type))
                    }),
                    evaluation_domain: expression_info
                        .map(|info| info.evaluation_domain)
                        .or_else(|| ir_info.map(|info| info.evaluation_domain)),
                    disabled: disabled.contains(&expression.id),
                    reachable: reachable.contains(&expression.id),
                    validation_message,
                    ir_value: ir_values
                        .and_then(|values| values.get(&expression.id))
                        .copied(),
                }
            })
            .collect::<Vec<_>>();

        let mut edges = nodes
            .iter()
            .flat_map(|node| {
                node.inputs
                    .iter()
                    .filter(|port| !inline_constants.contains(&port.source))
                    .map(|port| MaterialGraphEdge {
                        source: port.source,
                        target: port.function_input.map_or_else(
                            || MaterialGraphEdgeTarget::Input {
                                expression: node.expression,
                                port: port.name.clone(),
                            },
                            |input| MaterialGraphEdgeTarget::FunctionInput {
                                expression: node.expression,
                                input,
                            },
                        ),
                        value_type: port.value_type,
                        evaluation_domain: port.evaluation_domain,
                    })
            })
            .collect::<Vec<_>>();
        let outputs = [
            (MaterialGraphOutputKind::Color, program.outputs.color),
            (MaterialGraphOutputKind::Alpha, program.outputs.alpha),
        ]
        .into_iter()
        .chain(
            (program.domain == aestra_core::material::MaterialDomain::Mesh).then_some((
                MaterialGraphOutputKind::VertexOffset,
                program
                    .outputs
                    .vertex_offset
                    .unwrap_or(MaterialExpressionId::from_u128(0)),
            )),
        )
        .map(|(kind, source)| {
            let source_info = info.get(&source);
            let ir_info = ir_values
                .and_then(|values| values.get(&source))
                .and_then(|value| ir.and_then(|ir| ir.value(*value)));
            MaterialGraphOutput {
                kind,
                source,
                value_type: source_info
                    .map(|info| info.value_type)
                    .or_else(|| ir_info.map(|info| info.value_type))
                    .or_else(|| {
                        (kind == MaterialGraphOutputKind::VertexOffset)
                            .then_some(MaterialValueType::Vec3)
                    }),
                evaluation_domain: source_info
                    .map(|info| info.evaluation_domain)
                    .or_else(|| ir_info.map(|info| info.evaluation_domain)),
                ir_value: ir_values.and_then(|values| values.get(&source)).copied(),
            }
        })
        .collect::<Vec<_>>();
        edges.extend(
            outputs
                .iter()
                .filter(|output| !output.source.is_nil())
                .map(|output| MaterialGraphEdge {
                    source: output.source,
                    target: MaterialGraphEdgeTarget::Output(output.kind),
                    value_type: output.value_type,
                    evaluation_domain: output.evaluation_domain,
                }),
        );

        MaterialGraphProjection {
            program: program.id,
            nodes,
            edges,
            outputs,
            diagnostics,
        }
    }
}

fn constant_label(value_type: MaterialValueType) -> &'static str {
    match value_type {
        MaterialValueType::Float => "Float",
        MaterialValueType::Vec2 => "Vector 2",
        MaterialValueType::Vec3 => "Vector 3",
        MaterialValueType::Vec4 => "Vector 4",
        MaterialValueType::Color => "Color",
        MaterialValueType::Bool => "Boolean",
        MaterialValueType::Texture2D(_) => "Texture",
    }
}

fn append_graph_function_call(
    compiler: &MaterialCompiler,
    program: &mut MaterialProgram,
    functions: &MaterialFunctionLibrary,
    reference: MaterialFunctionRef,
    output: MaterialFunctionOutputId,
    source: Option<MaterialExpressionId>,
) -> Result<MaterialExpressionId, MaterialGraphNodeCreationError> {
    let function =
        functions
            .get(reference)
            .ok_or(MaterialGraphNodeCreationError::FunctionUnavailable {
                function: reference,
            })?;
    if !function
        .outputs
        .iter()
        .any(|candidate| candidate.id == output)
    {
        return Err(MaterialGraphNodeCreationError::FunctionOutputUnavailable {
            function: reference,
            output,
        });
    }
    let inputs = function.inputs.clone();
    let source_type = source.and_then(|source| {
        compiler
            .compile_with_functions(program, functions)
            .ok()
            .and_then(|ir| {
                ir.source_map
                    .values
                    .get(&source)
                    .copied()
                    .map(|id| (ir, id))
            })
            .and_then(|(ir, id)| ir.value(id).map(|value| value.value_type))
    });
    let source_input = source.and_then(|_| {
        source_type.and_then(|source_type| {
            inputs
                .iter()
                .find(|input| input.value_type == source_type)
                .map(|input| input.id)
        })
    });
    if let Some(source) = source
        && source_input.is_none()
    {
        return Err(MaterialGraphNodeCreationError::IncompatibleSource {
            kind: MaterialGraphCreateKind::FunctionCall {
                function: reference,
                output,
            },
            expression: source,
        });
    }

    let mut arguments = BTreeMap::new();
    for input in inputs {
        let expression = if Some(input.id) == source_input {
            source.expect("a selected function input requires a source")
        } else {
            let value = default_value(input.value_type, false).ok_or_else(|| {
                MaterialGraphNodeCreationError::FunctionInputDefaultUnavailable {
                    input: input.name.clone(),
                }
            })?;
            append_graph_constant(program, value)
        };
        arguments.insert(input.id, expression);
    }
    Ok(append_graph_expression(
        program,
        MaterialExpressionKind::FunctionCall {
            function: reference,
            arguments,
            output,
        },
    ))
}

fn append_graph_function(
    program: &mut MaterialProgram,
    function: MaterialGraphFunction,
    source: Option<MaterialExpressionId>,
) -> Result<MaterialExpressionId, MaterialGraphNodeCreationError> {
    let source_type = source.and_then(|source| {
        program
            .analyze()
            .ok()
            .and_then(|analysis| analysis.expressions.get(&source).copied())
            .map(|info| info.value_type)
    });
    let numeric_source = |program: &mut MaterialProgram| {
        source.unwrap_or_else(|| append_graph_constant(program, MaterialValue::Float(0.0)))
    };
    let expression = match function {
        MaterialGraphFunction::Add | MaterialGraphFunction::Subtract => {
            let left = numeric_source(program);
            let right = append_graph_constant(
                program,
                default_value(source_type.unwrap_or(MaterialValueType::Float), false)
                    .unwrap_or(MaterialValue::Float(0.0)),
            );
            if function == MaterialGraphFunction::Add {
                MaterialExpressionKind::Add(left, right)
            } else {
                MaterialExpressionKind::Subtract(left, right)
            }
        }
        MaterialGraphFunction::Multiply | MaterialGraphFunction::Divide => {
            let left = numeric_source(program);
            let right = append_graph_constant(program, MaterialValue::Float(1.0));
            if function == MaterialGraphFunction::Multiply {
                MaterialExpressionKind::Multiply(left, right)
            } else {
                MaterialExpressionKind::Divide(left, right)
            }
        }
        MaterialGraphFunction::Lerp => {
            let start = numeric_source(program);
            let end = append_graph_constant(
                program,
                default_value(source_type.unwrap_or(MaterialValueType::Float), true)
                    .unwrap_or(MaterialValue::Float(1.0)),
            );
            let factor = append_graph_constant(program, MaterialValue::Float(0.5));
            MaterialExpressionKind::Lerp { start, end, factor }
        }
        MaterialGraphFunction::Clamp => {
            let value = numeric_source(program);
            let value_type = source_type.unwrap_or(MaterialValueType::Float);
            let min = append_graph_constant(
                program,
                default_value(value_type, false).unwrap_or(MaterialValue::Float(0.0)),
            );
            let max = append_graph_constant(
                program,
                default_value(value_type, true).unwrap_or(MaterialValue::Float(1.0)),
            );
            MaterialExpressionKind::Clamp { value, min, max }
        }
        MaterialGraphFunction::Select => {
            if matches!(source_type, Some(MaterialValueType::Texture2D(_))) {
                return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression: source.expect("source type requires a source"),
                });
            }
            let value_type = source_type.unwrap_or(MaterialValueType::Float);
            let if_false = source.unwrap_or_else(|| {
                append_graph_constant(
                    program,
                    default_value(value_type, false).unwrap_or(MaterialValue::Float(0.0)),
                )
            });
            let if_true = append_graph_constant(
                program,
                default_value(value_type, true).unwrap_or(MaterialValue::Float(1.0)),
            );
            let condition = append_graph_constant(program, MaterialValue::Bool(false));
            MaterialExpressionKind::Select {
                condition,
                if_false,
                if_true,
            }
        }
        MaterialGraphFunction::Fresnel => {
            let normal = source.unwrap_or_else(|| {
                append_graph_expression(
                    program,
                    MaterialExpressionKind::Input(MaterialInput::Normal),
                )
            });
            let view = append_graph_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::ViewDirection),
            );
            let power = append_graph_constant(program, MaterialValue::Float(5.0));
            MaterialExpressionKind::Fresnel {
                normal,
                view,
                power,
            }
        }
        MaterialGraphFunction::DepthFade => {
            let scene_depth = source.unwrap_or_else(|| {
                append_graph_expression(
                    program,
                    MaterialExpressionKind::Input(MaterialInput::SceneDepth),
                )
            });
            let pixel_depth = append_graph_expression(
                program,
                MaterialExpressionKind::Input(MaterialInput::PixelDepth),
            );
            let fade_distance = append_graph_constant(program, MaterialValue::Float(0.5));
            let invert = append_graph_constant(program, MaterialValue::Bool(false));
            MaterialExpressionKind::DepthFade {
                scene_depth,
                pixel_depth,
                fade_distance,
                invert,
            }
        }
        MaterialGraphFunction::DerivativeX | MaterialGraphFunction::DerivativeY => {
            let value = source.unwrap_or_else(|| {
                append_graph_expression(program, MaterialExpressionKind::Input(MaterialInput::Uv0))
            });
            if function == MaterialGraphFunction::DerivativeX {
                MaterialExpressionKind::DerivativeX { value }
            } else {
                MaterialExpressionKind::DerivativeY { value }
            }
        }
        MaterialGraphFunction::SampleTexture => {
            let parameter = program
                .parameters
                .iter()
                .find(|parameter| matches!(parameter.value_type, MaterialValueType::Texture2D(_)))
                .map(|parameter| parameter.id)
                .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
            let default_texture = || MaterialExpressionKind::Parameter(parameter);
            let (texture, uv) = match source_type {
                Some(MaterialValueType::Texture2D(_)) => (
                    source.expect("source type requires a source"),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
                _ => (
                    append_graph_expression(program, default_texture()),
                    source.unwrap_or_else(|| {
                        append_graph_expression(
                            program,
                            MaterialExpressionKind::Input(MaterialInput::Uv0),
                        )
                    }),
                ),
            };
            MaterialExpressionKind::SampleTexture { texture, uv }
        }
        MaterialGraphFunction::SampleTextureLevel => {
            let parameter = program
                .parameters
                .iter()
                .find(|parameter| matches!(parameter.value_type, MaterialValueType::Texture2D(_)))
                .map(|parameter| parameter.id)
                .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
            let (texture, uv) = match source_type {
                Some(MaterialValueType::Texture2D(_)) => (
                    source.expect("source type requires a source"),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
                _ => (
                    append_graph_expression(program, MaterialExpressionKind::Parameter(parameter)),
                    source.unwrap_or_else(|| {
                        append_graph_expression(
                            program,
                            MaterialExpressionKind::Input(MaterialInput::Uv0),
                        )
                    }),
                ),
            };
            let level = append_graph_constant(program, MaterialValue::Float(0.0));
            MaterialExpressionKind::SampleTextureLevel { texture, uv, level }
        }
        MaterialGraphFunction::SampleTextureGradient => {
            let parameter = program
                .parameters
                .iter()
                .find(|parameter| matches!(parameter.value_type, MaterialValueType::Texture2D(_)))
                .map(|parameter| parameter.id)
                .ok_or(MaterialGraphNodeCreationError::TextureParameterMissing)?;
            let (texture, uv) = match source_type {
                Some(MaterialValueType::Texture2D(_)) => (
                    source.expect("source type requires a source"),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
                Some(MaterialValueType::Vec2) => (
                    append_graph_expression(program, MaterialExpressionKind::Parameter(parameter)),
                    source.expect("source type requires a source"),
                ),
                Some(_) => {
                    return Err(MaterialGraphNodeCreationError::IncompatibleSource {
                        kind: MaterialGraphCreateKind::Function(function),
                        expression: source.expect("source type requires a source"),
                    });
                }
                None => (
                    append_graph_expression(program, MaterialExpressionKind::Parameter(parameter)),
                    append_graph_expression(
                        program,
                        MaterialExpressionKind::Input(MaterialInput::Uv0),
                    ),
                ),
            };
            let ddx =
                append_graph_expression(program, MaterialExpressionKind::DerivativeX { value: uv });
            let ddy =
                append_graph_expression(program, MaterialExpressionKind::DerivativeY { value: uv });
            MaterialExpressionKind::SampleTextureGradient {
                texture,
                uv,
                ddx,
                ddy,
            }
        }
        MaterialGraphFunction::ExtractComponent => {
            let value = source.unwrap_or_else(|| {
                append_graph_constant(program, MaterialValue::Vec4([0.0, 0.0, 0.0, 1.0]))
            });
            MaterialExpressionKind::ExtractComponent {
                value,
                component: MaterialVectorComponent::X,
            }
        }
        function => {
            let kind = graph_function_modifier(function)
                .expect("all non-special graph functions map to a stack modifier");
            let source = source.unwrap_or_else(|| default_modifier_source(program, kind));
            return append_default_modifier(program, kind, source).ok_or(
                MaterialGraphNodeCreationError::IncompatibleSource {
                    kind: MaterialGraphCreateKind::Function(function),
                    expression: source,
                },
            );
        }
    };
    Ok(append_graph_expression(program, expression))
}

fn graph_function_modifier(function: MaterialGraphFunction) -> Option<MaterialStackModifierKind> {
    Some(match function {
        MaterialGraphFunction::Remap => MaterialStackModifierKind::Remap,
        MaterialGraphFunction::Smoothstep => MaterialStackModifierKind::Smoothstep,
        MaterialGraphFunction::RadialMask => MaterialStackModifierKind::RadialMask,
        MaterialGraphFunction::Dissolve => MaterialStackModifierKind::Dissolve,
        MaterialGraphFunction::DissolveEdge => MaterialStackModifierKind::DissolveEdge,
        MaterialGraphFunction::SoftParticle => MaterialStackModifierKind::SoftParticle,
        MaterialGraphFunction::PanUv => MaterialStackModifierKind::PanUv,
        MaterialGraphFunction::RotateUv => MaterialStackModifierKind::RotateUv,
        MaterialGraphFunction::ScaleUv => MaterialStackModifierKind::ScaleUv,
        MaterialGraphFunction::Add
        | MaterialGraphFunction::Subtract
        | MaterialGraphFunction::Multiply
        | MaterialGraphFunction::Divide
        | MaterialGraphFunction::Lerp
        | MaterialGraphFunction::Clamp
        | MaterialGraphFunction::Select
        | MaterialGraphFunction::Fresnel
        | MaterialGraphFunction::DepthFade
        | MaterialGraphFunction::DerivativeX
        | MaterialGraphFunction::DerivativeY
        | MaterialGraphFunction::SampleTexture
        | MaterialGraphFunction::SampleTextureLevel
        | MaterialGraphFunction::SampleTextureGradient
        | MaterialGraphFunction::ExtractComponent => return None,
    })
}

fn default_modifier_source(
    program: &mut MaterialProgram,
    kind: MaterialStackModifierKind,
) -> MaterialExpressionId {
    match kind {
        MaterialStackModifierKind::PanUv
        | MaterialStackModifierKind::RotateUv
        | MaterialStackModifierKind::ScaleUv
        | MaterialStackModifierKind::RadialMask => {
            append_graph_expression(program, MaterialExpressionKind::Input(MaterialInput::Uv0))
        }
        MaterialStackModifierKind::Dissolve
        | MaterialStackModifierKind::DissolveEdge
        | MaterialStackModifierKind::SoftParticle => {
            append_graph_constant(program, MaterialValue::Float(1.0))
        }
        MaterialStackModifierKind::Remap | MaterialStackModifierKind::Smoothstep => {
            append_graph_constant(program, MaterialValue::Float(0.0))
        }
        MaterialStackModifierKind::BaseTexture
        | MaterialStackModifierKind::Fresnel
        | MaterialStackModifierKind::DepthFade => {
            unreachable!("generator modifiers are created by specialized graph factories")
        }
    }
}

fn default_value(value_type: MaterialValueType, one: bool) -> Option<MaterialValue> {
    let scalar = if one { 1.0 } else { 0.0 };
    match value_type {
        MaterialValueType::Float => Some(MaterialValue::Float(scalar)),
        MaterialValueType::Vec2 => Some(MaterialValue::Vec2([scalar; 2])),
        MaterialValueType::Vec3 => Some(MaterialValue::Vec3([scalar; 3])),
        MaterialValueType::Vec4 => Some(MaterialValue::Vec4([scalar; 4])),
        MaterialValueType::Color => Some(MaterialValue::ColorSrgb([scalar, scalar, scalar, 1.0])),
        MaterialValueType::Bool => Some(MaterialValue::Bool(one)),
        MaterialValueType::Texture2D(_) => None,
    }
}

fn append_graph_constant(
    program: &mut MaterialProgram,
    value: MaterialValue,
) -> MaterialExpressionId {
    append_graph_expression(program, MaterialExpressionKind::Constant(value))
}

fn append_graph_expression(
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

fn reachable_expressions(program: &MaterialProgram) -> BTreeSet<MaterialExpressionId> {
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let mut reachable = BTreeSet::new();
    let mut pending = program.outputs.roots().collect::<Vec<_>>();
    while let Some(id) = pending.pop() {
        if !reachable.insert(id) {
            continue;
        }
        if let Some(kind) = expressions.get(&id) {
            pending.extend(expression_inputs(kind).into_iter().map(|(_, id)| id));
        }
    }
    reachable
}

fn node_kind(kind: &MaterialExpressionKind) -> MaterialGraphNodeKind {
    use MaterialExpressionKind as E;
    let function = match kind {
        E::Constant(_) => return MaterialGraphNodeKind::Constant,
        E::Input(input) => return MaterialGraphNodeKind::Input(*input),
        E::Parameter(parameter) => return MaterialGraphNodeKind::Parameter(*parameter),
        E::FunctionInput(input) => return MaterialGraphNodeKind::FunctionInput(*input),
        E::FunctionCall { function, .. } => {
            return MaterialGraphNodeKind::FunctionCall(*function);
        }
        E::CustomWeslCall { function, .. } => {
            return MaterialGraphNodeKind::FunctionCall(MaterialFunctionRef::Project(*function));
        }
        E::Add(..) => MaterialGraphFunction::Add,
        E::Subtract(..) => MaterialGraphFunction::Subtract,
        E::Multiply(..) => MaterialGraphFunction::Multiply,
        E::Divide(..) => MaterialGraphFunction::Divide,
        E::Lerp { .. } => MaterialGraphFunction::Lerp,
        E::Clamp { .. } => MaterialGraphFunction::Clamp,
        E::Select { .. } => MaterialGraphFunction::Select,
        E::Remap { .. } => MaterialGraphFunction::Remap,
        E::Smoothstep { .. } => MaterialGraphFunction::Smoothstep,
        E::Fresnel { .. } => MaterialGraphFunction::Fresnel,
        E::RadialMask { .. } => MaterialGraphFunction::RadialMask,
        E::Dissolve { .. } => MaterialGraphFunction::Dissolve,
        E::DissolveEdge { .. } => MaterialGraphFunction::DissolveEdge,
        E::DepthFade { .. } => MaterialGraphFunction::DepthFade,
        E::SoftParticle { .. } => MaterialGraphFunction::SoftParticle,
        E::PanUv { .. } => MaterialGraphFunction::PanUv,
        E::RotateUv { .. } => MaterialGraphFunction::RotateUv,
        E::ScaleUv { .. } => MaterialGraphFunction::ScaleUv,
        E::DerivativeX { .. } => MaterialGraphFunction::DerivativeX,
        E::DerivativeY { .. } => MaterialGraphFunction::DerivativeY,
        E::SampleTexture { .. } => MaterialGraphFunction::SampleTexture,
        E::SampleTextureLevel { .. } => MaterialGraphFunction::SampleTextureLevel,
        E::SampleTextureGradient { .. } => MaterialGraphFunction::SampleTextureGradient,
        E::ExtractComponent { .. } => MaterialGraphFunction::ExtractComponent,
    };
    MaterialGraphNodeKind::Function(function)
}

fn node_label(
    kind: &MaterialExpressionKind,
    program: &MaterialProgram,
    functions: &MaterialFunctionLibrary,
) -> String {
    match kind {
        MaterialExpressionKind::Constant(value) => format!("Constant · {value:?}"),
        MaterialExpressionKind::Input(input) => format!("{input:?}"),
        MaterialExpressionKind::Parameter(id) => program
            .parameters
            .iter()
            .find(|parameter| parameter.id == *id)
            .map(|parameter| parameter.name.clone())
            .unwrap_or_else(|| format!("Parameter {id}")),
        MaterialExpressionKind::FunctionInput(id) => format!("Function input {id}"),
        MaterialExpressionKind::FunctionCall { function, .. } => functions
            .get(*function)
            .map(|function| function.name.clone())
            .unwrap_or_else(|| format!("Missing function {}", function.id())),
        MaterialExpressionKind::CustomWeslCall { entry_point, .. } => entry_point.clone(),
        MaterialExpressionKind::ExtractComponent { component, .. } => {
            format!("Extract {component:?}")
        }
        _ => format!(
            "{:?}",
            match node_kind(kind) {
                MaterialGraphNodeKind::Function(function) => function,
                _ => unreachable!(),
            }
        ),
    }
}

fn expression_ports(
    kind: &MaterialExpressionKind,
    functions: &MaterialFunctionLibrary,
) -> Vec<(
    String,
    MaterialExpressionId,
    Option<MaterialFunctionInputId>,
)> {
    if let MaterialExpressionKind::FunctionCall {
        function,
        arguments,
        ..
    } = kind
    {
        if let Some(function) = functions.get(*function) {
            return function
                .inputs
                .iter()
                .filter_map(|input| {
                    arguments
                        .get(&input.id)
                        .copied()
                        .map(|source| (input.name.clone(), source, Some(input.id)))
                })
                .collect();
        }
        return arguments
            .iter()
            .map(|(&input, &source)| (format!("Missing input {input}"), source, Some(input)))
            .collect();
    }
    expression_inputs(kind)
        .into_iter()
        .map(|(name, source)| (name.to_owned(), source, None))
        .collect()
}

fn expression_inputs(kind: &MaterialExpressionKind) -> Vec<(&'static str, MaterialExpressionId)> {
    use MaterialExpressionKind as E;
    match kind {
        E::Constant(_) | E::Input(_) | E::Parameter(_) | E::FunctionInput(_) => Vec::new(),
        E::FunctionCall { arguments, .. } => arguments
            .values()
            .copied()
            .map(|expression| ("argument", expression))
            .collect(),
        E::CustomWeslCall { arguments, .. } => arguments
            .iter()
            .map(|argument| ("argument", argument.expression))
            .collect(),
        E::Add(left, right)
        | E::Subtract(left, right)
        | E::Multiply(left, right)
        | E::Divide(left, right) => {
            vec![("left", *left), ("right", *right)]
        }
        E::Lerp { start, end, factor } => {
            vec![("start", *start), ("end", *end), ("factor", *factor)]
        }
        E::Clamp { value, min, max } => vec![("value", *value), ("min", *min), ("max", *max)],
        E::Select {
            condition,
            if_false,
            if_true,
        } => vec![
            ("condition", *condition),
            ("false", *if_false),
            ("true", *if_true),
        ],
        E::Remap {
            value,
            input_min,
            input_max,
            output_min,
            output_max,
        } => vec![
            ("value", *value),
            ("input_min", *input_min),
            ("input_max", *input_max),
            ("output_min", *output_min),
            ("output_max", *output_max),
        ],
        E::Smoothstep {
            edge_min,
            edge_max,
            value,
        } => vec![
            ("edge_min", *edge_min),
            ("edge_max", *edge_max),
            ("value", *value),
        ],
        E::Fresnel {
            normal,
            view,
            power,
        } => vec![("normal", *normal), ("view", *view), ("power", *power)],
        E::RadialMask {
            uv,
            center,
            radius,
            softness,
            invert,
        } => vec![
            ("uv", *uv),
            ("center", *center),
            ("radius", *radius),
            ("softness", *softness),
            ("invert", *invert),
        ],
        E::Dissolve {
            source,
            threshold,
            edge_width,
            invert,
        }
        | E::DissolveEdge {
            source,
            threshold,
            edge_width,
            invert,
        } => vec![
            ("source", *source),
            ("threshold", *threshold),
            ("edge_width", *edge_width),
            ("invert", *invert),
        ],
        E::DepthFade {
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![
            ("scene_depth", *scene_depth),
            ("pixel_depth", *pixel_depth),
            ("fade_distance", *fade_distance),
            ("invert", *invert),
        ],
        E::SoftParticle {
            alpha,
            scene_depth,
            pixel_depth,
            fade_distance,
            invert,
        } => vec![
            ("alpha", *alpha),
            ("scene_depth", *scene_depth),
            ("pixel_depth", *pixel_depth),
            ("fade_distance", *fade_distance),
            ("invert", *invert),
        ],
        E::PanUv { uv, speed, time } => vec![("uv", *uv), ("speed", *speed), ("time", *time)],
        E::RotateUv { uv, center, angle } => {
            vec![("uv", *uv), ("center", *center), ("angle", *angle)]
        }
        E::ScaleUv { uv, center, scale } => {
            vec![("uv", *uv), ("center", *center), ("scale", *scale)]
        }
        E::DerivativeX { value } | E::DerivativeY { value } => vec![("value", *value)],
        E::SampleTexture { texture, uv } => vec![("texture", *texture), ("uv", *uv)],
        E::SampleTextureLevel { texture, uv, level } => {
            vec![("texture", *texture), ("uv", *uv), ("level", *level)]
        }
        E::SampleTextureGradient {
            texture,
            uv,
            ddx,
            ddy,
        } => vec![
            ("texture", *texture),
            ("uv", *uv),
            ("ddx", *ddx),
            ("ddy", *ddy),
        ],
        E::ExtractComponent { value, .. } => vec![("value", *value)],
    }
}
