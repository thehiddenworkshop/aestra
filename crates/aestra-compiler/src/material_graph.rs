//! Backend-neutral, read-only projection of a semantic material program as a graph.

use crate::{MaterialCompiler, MaterialIrProgram, MaterialIrValueId};
use aestra_core::{
    MaterialExpressionId, MaterialParameterId, MaterialProgramId, ValidationReport,
    material::{
        MaterialExpressionDomain, MaterialExpressionKind, MaterialInput, MaterialProgram,
        MaterialValueType,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphNodeKind {
    Constant,
    Input(MaterialInput),
    Parameter(MaterialParameterId),
    Function(MaterialGraphFunction),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialGraphFunction {
    Add,
    Subtract,
    Multiply,
    Divide,
    Lerp,
    Clamp,
    Remap,
    Smoothstep,
    Fresnel,
    RadialMask,
    Dissolve,
    DissolveEdge,
    DepthFade,
    SoftParticle,
    PanUv,
    RotateUv,
    ScaleUv,
    SampleTexture,
    ExtractComponent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphPort {
    /// Stable semantic socket name within the node.
    pub name: String,
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
    /// Optional link into optimized compiler IR. Multiple expressions can alias one IR value.
    pub ir_value: Option<MaterialIrValueId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum MaterialGraphOutputKind {
    Color,
    Alpha,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialGraphOutput {
    pub kind: MaterialGraphOutputKind,
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
        let program = program.normalized();
        let diagnostics = program.validation_report();
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

        let nodes = program
            .expressions
            .iter()
            .map(|expression| {
                let inputs = expression_inputs(&expression.kind)
                    .into_iter()
                    .map(|(name, source)| {
                        let source_info = info.get(&source);
                        MaterialGraphPort {
                            name: name.into(),
                            source,
                            value_type: source_info.map(|info| info.value_type),
                            evaluation_domain: source_info.map(|info| info.evaluation_domain),
                        }
                    })
                    .collect();
                let expression_info = info.get(&expression.id);
                MaterialGraphNode {
                    expression: expression.id,
                    kind: node_kind(&expression.kind),
                    label: node_label(&expression.kind, &program),
                    inputs,
                    value_type: expression_info.map(|info| info.value_type),
                    evaluation_domain: expression_info.map(|info| info.evaluation_domain),
                    disabled: disabled.contains(&expression.id),
                    reachable: reachable.contains(&expression.id),
                    ir_value: ir_values
                        .and_then(|values| values.get(&expression.id))
                        .copied(),
                }
            })
            .collect::<Vec<_>>();

        let mut edges = nodes
            .iter()
            .flat_map(|node| {
                node.inputs.iter().map(|port| MaterialGraphEdge {
                    source: port.source,
                    target: MaterialGraphEdgeTarget::Input {
                        expression: node.expression,
                        port: port.name.clone(),
                    },
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
        .map(|(kind, source)| {
            let source_info = info.get(&source);
            MaterialGraphOutput {
                kind,
                source,
                value_type: source_info.map(|info| info.value_type),
                evaluation_domain: source_info.map(|info| info.evaluation_domain),
                ir_value: ir_values.and_then(|values| values.get(&source)).copied(),
            }
        })
        .collect::<Vec<_>>();
        edges.extend(outputs.iter().map(|output| MaterialGraphEdge {
            source: output.source,
            target: MaterialGraphEdgeTarget::Output(output.kind),
            value_type: output.value_type,
            evaluation_domain: output.evaluation_domain,
        }));

        MaterialGraphProjection {
            program: program.id,
            nodes,
            edges,
            outputs,
            diagnostics,
        }
    }
}

fn reachable_expressions(program: &MaterialProgram) -> BTreeSet<MaterialExpressionId> {
    let expressions = program
        .expressions
        .iter()
        .map(|expression| (expression.id, &expression.kind))
        .collect::<BTreeMap<_, _>>();
    let mut reachable = BTreeSet::new();
    let mut pending = vec![program.outputs.color, program.outputs.alpha];
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
        E::Add(..) => MaterialGraphFunction::Add,
        E::Subtract(..) => MaterialGraphFunction::Subtract,
        E::Multiply(..) => MaterialGraphFunction::Multiply,
        E::Divide(..) => MaterialGraphFunction::Divide,
        E::Lerp { .. } => MaterialGraphFunction::Lerp,
        E::Clamp { .. } => MaterialGraphFunction::Clamp,
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
        E::SampleTexture { .. } => MaterialGraphFunction::SampleTexture,
        E::ExtractComponent { .. } => MaterialGraphFunction::ExtractComponent,
    };
    MaterialGraphNodeKind::Function(function)
}

fn node_label(kind: &MaterialExpressionKind, program: &MaterialProgram) -> String {
    match kind {
        MaterialExpressionKind::Constant(value) => format!("Constant · {value:?}"),
        MaterialExpressionKind::Input(input) => format!("{input:?}"),
        MaterialExpressionKind::Parameter(id) => program
            .parameters
            .iter()
            .find(|parameter| parameter.id == *id)
            .map(|parameter| parameter.name.clone())
            .unwrap_or_else(|| format!("Parameter {id}")),
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

fn expression_inputs(kind: &MaterialExpressionKind) -> Vec<(&'static str, MaterialExpressionId)> {
    use MaterialExpressionKind as E;
    match kind {
        E::Constant(_) | E::Input(_) | E::Parameter(_) => Vec::new(),
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
        E::SampleTexture { texture, uv } => vec![("texture", *texture), ("uv", *uv)],
        E::ExtractComponent { value, .. } => vec![("value", *value)],
    }
}
