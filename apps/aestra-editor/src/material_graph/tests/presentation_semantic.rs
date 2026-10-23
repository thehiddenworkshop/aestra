use super::*;
use aestra_compiler::MaterialCompiler;
use aestra_core::{
    MaterialExpressionId,
    material::{MaterialExpression, MaterialExpressionKind, MaterialValue},
};

fn body() -> MaterialFunction {
    let mut function = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap();
    function.custom_wesl = None;
    function.expressions.push(MaterialExpression {
        id: function.outputs[0].expression,
        kind: MaterialExpressionKind::FunctionInput(function.inputs[0].id),
    });
    function.normalized()
}

#[test]
fn material_resolution_uses_document_identity_not_inventory_order_or_view_camera() {
    let first = MaterialProgram::additive_sprite("first");
    let second = MaterialProgram::additive_sprite("second");
    let graph = material_graph_view_key(second.id);
    let expected = program(&graph, [second.clone()], |_| true).unwrap();
    assert_eq!(
        program(&graph, [first, second.clone()], |id| {
            assert_eq!(id, second.id);
            true
        }),
        Some(expected)
    );
    assert!(
        program(&format!("{graph}#tool"), [second.clone()], |_| panic!(
            "No document match"
        ))
        .is_none()
    );
    assert!(program(&graph, [], |_| panic!("Missing inventory entry")).is_none());
}

#[test]
fn missing_project_sources_reject_snapshots_but_source_free_builtins_remain_valid() {
    let project = MaterialProgram::additive_sprite("project");
    let graph = material_graph_view_key(project.id);
    assert!(program(&graph, [project], |_| false).is_none());
    let builtin = MaterialProgram::built_in(MaterialProgramRef::BuiltIn(
        MaterialProgram::DEFAULT_MESH_ID,
    ))
    .unwrap();
    let graph = material_graph_view_key(builtin.id);
    assert_eq!(
        program(&graph, [builtin.clone()], |_| false),
        program(&graph, [builtin], |_| true)
    );
}

#[test]
fn material_snapshot_keys_match_real_compiler_projection_with_inline_and_explicit_constants() {
    let mut material = MaterialProgram::additive_sprite("inline");
    let old_alpha = material.outputs.alpha;
    let inline = MaterialExpressionId::new();
    let multiply = MaterialExpressionId::new();
    material.expressions.extend([
        MaterialExpression {
            id: inline,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(0.5)),
        },
        MaterialExpression {
            id: multiply,
            kind: MaterialExpressionKind::Multiply(old_alpha, inline),
        },
    ]);
    material.outputs.alpha = multiply;
    for explicit in [false, true] {
        if explicit {
            material.node_constants.push(inline);
        }
        let (_, keys) = program(
            &material_graph_view_key(material.id),
            [material.clone()],
            |_| true,
        )
        .unwrap();
        let projection = MaterialCompiler.project_graph(&material, None);
        let projected = projection
            .nodes
            .iter()
            .map(|node| material_graph_expression_node_key(node.expression))
            .chain([MATERIAL_GRAPH_OUTPUT_NODE_KEY.into()])
            .collect::<BTreeSet<_>>();
        assert_eq!(keys, projected);
        assert_eq!(
            keys.contains(&material_graph_expression_node_key(inline)),
            explicit
        );
        assert!(!keys.contains(&material_graph_expression_node_key(old_alpha)));
    }
}

#[test]
fn function_resolution_requires_matching_root_identity_editable_body_and_ready_source() {
    let first = body();
    let mut second = first.clone();
    second.id = MaterialFunctionId::new();
    let root = Path::new("project");
    let graph = function_graph_memory_key(root, second.id);
    let (_, keys) = function(&graph, root, [first, second.clone()], |id| {
        assert_eq!(id, second.id);
        true
    })
    .unwrap();
    assert_eq!(
        keys,
        second
            .expressions
            .iter()
            .map(|e| e.id.to_string())
            .chain(["outputs".into()])
            .collect()
    );
    assert!(
        function(&graph, Path::new("other"), [second.clone()], |_| panic!(
            "Wrong root"
        ))
        .is_none()
    );
    assert!(function(&graph, root, [second.clone()], |_| false).is_none());
    assert!(function(&graph, root, [], |_| panic!("Missing function")).is_none());
    second.custom_wesl = MaterialFunction::from_ron(include_str!(
        "../../../../../assets/test/materials/pulse_wave.aestra.material-function.ron"
    ))
    .unwrap()
    .custom_wesl;
    assert!(function(&graph, root, [second], |_| panic!("Readonly body")).is_none());
}

#[test]
fn semantic_stamps_are_stable_and_invalidate_content_edits_even_with_identical_canvas_keys() {
    let mut material = MaterialProgram::additive_sprite("before");
    let graph = material_graph_view_key(material.id);
    let original = program(&graph, [material.clone()], |_| true).unwrap();
    assert_eq!(
        program(&graph, [material.clone()], |_| true),
        Some(original.clone())
    );
    material.name = "after".into();
    let changed = program(&graph, [material], |_| true).unwrap();
    assert_ne!(original.0, changed.0);
    assert_eq!(original.1, changed.1);
    let mut f = body();
    let root = Path::new("project");
    let graph = function_graph_memory_key(root, f.id);
    let original = function(&graph, root, [f.clone()], |_| true).unwrap();
    assert_eq!(
        function(&graph, root, [f.clone()], |_| true),
        Some(original.clone())
    );
    f.inputs[0].default = Some(MaterialValue::Float(0.25));
    let changed = function(&graph, root, [f], |_| true).unwrap();
    assert_ne!(original.0, changed.0);
    assert_eq!(original.1, changed.1);
}
