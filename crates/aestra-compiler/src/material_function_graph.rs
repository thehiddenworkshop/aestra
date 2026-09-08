//! Function-native authoring projection. No fabricated program or effect is required.
use crate::{MaterialCompiler, MaterialFunctionLibrary};
use aestra_core::{
    MaterialExpressionId, MaterialFunctionId, MaterialFunctionInputId, MaterialFunctionOutputId,
    ValidationReport,
    material::{
        MaterialCustomWeslImplementation, MaterialExpression, MaterialExpressionKind,
        MaterialFunction, MaterialFunctionInput, MaterialFunctionOutput,
    },
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialFunctionGraphTarget {
    Input {
        expression: MaterialExpressionId,
        port: String,
    },
    Argument {
        expression: MaterialExpressionId,
        input: MaterialFunctionInputId,
    },
    Output(MaterialFunctionOutputId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialFunctionGraphEdge {
    pub source: MaterialExpressionId,
    pub target: MaterialFunctionGraphTarget,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MaterialFunctionBodyProjection {
    Graph {
        nodes: Vec<MaterialExpression>,
        edges: Vec<MaterialFunctionGraphEdge>,
    },
    /// Inspection only. Preserve source and entry points verbatim, never synthesize nodes.
    CustomWesl(MaterialCustomWeslImplementation),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialFunctionGraphProjection {
    pub function: MaterialFunctionId,
    pub name: String,
    pub inputs: Vec<MaterialFunctionInput>,
    pub outputs: Vec<MaterialFunctionOutput>,
    pub body: MaterialFunctionBodyProjection,
    pub diagnostics: ValidationReport,
}

impl MaterialCompiler {
    pub fn project_function_graph(
        &self,
        function: &MaterialFunction,
        library: &MaterialFunctionLibrary,
    ) -> MaterialFunctionGraphProjection {
        let mut candidate_library = library.clone();
        candidate_library.register_project(function.clone());
        let body = if let Some(source) = &function.custom_wesl {
            MaterialFunctionBodyProjection::CustomWesl(source.clone())
        } else {
            let mut edges = Vec::new();
            for node in &function.expressions {
                if let MaterialExpressionKind::FunctionCall { arguments, .. } = &node.kind {
                    // Keep even unknown ports/missing sources visible for diagnostic repair.
                    edges.extend(arguments.iter().map(|(&input, &source)| {
                        MaterialFunctionGraphEdge {
                            source,
                            target: MaterialFunctionGraphTarget::Argument {
                                expression: node.id,
                                input,
                            },
                        }
                    }));
                } else {
                    edges.extend(
                        crate::material_graph::expression_inputs(&node.kind)
                            .into_iter()
                            .map(|(port, source)| MaterialFunctionGraphEdge {
                                source,
                                target: MaterialFunctionGraphTarget::Input {
                                    expression: node.id,
                                    port: port.into(),
                                },
                            }),
                    );
                }
            }
            edges.extend(
                function
                    .outputs
                    .iter()
                    .map(|output| MaterialFunctionGraphEdge {
                        source: output.expression,
                        target: MaterialFunctionGraphTarget::Output(output.id),
                    }),
            );
            MaterialFunctionBodyProjection::Graph {
                nodes: function.expressions.clone(),
                edges,
            }
        };
        MaterialFunctionGraphProjection {
            function: function.id,
            name: function.name.clone(),
            inputs: function.inputs.clone(),
            outputs: function.outputs.clone(),
            body,
            diagnostics: candidate_library.validation_report(),
        }
    }
}
