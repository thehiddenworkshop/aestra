//! Shared production function wire identity, independent of pointer insertion dispatch.
use crate::feathers::node_graph::{geometry::GraphNodeKey, insertion::Wire};
use crate::material_graph::input_target;
use aestra_authoring::{MaterialConnectionTarget, MaterialExpressionInput};
use aestra_compiler::MaterialFunctionGraphTarget;
use aestra_core::MaterialExpressionId;

pub(crate) fn function_wire(
    source: MaterialExpressionId,
    target: &MaterialFunctionGraphTarget,
) -> Option<Wire> {
    use crate::feathers::node_graph::geometry::GraphGeometryPort;
    Some(match target {
        MaterialFunctionGraphTarget::Output(output) => Wire {
            source: GraphNodeKey::Expression(source),
            target: GraphNodeKey::FunctionOutputs,
            port: GraphGeometryPort::FunctionOutput(*output),
        },
        MaterialFunctionGraphTarget::Argument { expression, input } => Wire::material(
            source,
            MaterialConnectionTarget::ExpressionInput {
                expression: *expression,
                input: MaterialExpressionInput::FunctionArgument(*input),
            },
        ),
        MaterialFunctionGraphTarget::Input { expression, port } => {
            Wire::material(source, input_target(*expression, port)?)
        }
    })
}
