use super::*;
use aestra_compiler::MaterialCompiler;
use aestra_core::material::MaterialProgram;
fn graph() -> MaterialGraphProjection {
    let p = MaterialProgram::from_ron(crate::MATERIAL_GRAPH_LAB_PROGRAM_SOURCE).unwrap();
    MaterialCompiler.project_graph(&p, MaterialCompiler.compile(&p).ok().as_ref())
}
#[test]
fn bootstrap_is_deterministic_and_outputs_follow_all_visible_nodes() {
    let g = graph();
    let previews = MaterialGraphPreviewState::default();
    let a = layout_graph(&g, &previews);
    let b = layout_graph(&g, &previews);
    assert_eq!(a.nodes, b.nodes);
    assert_eq!(a.output, b.output);
    assert_eq!(a.size, b.size);
    assert!(a.nodes.values().all(|p| p.x < a.output.x));
    assert!(a.size.x >= 720.0 && a.size.y >= 420.0);
}
#[test]
fn expression_previews_increase_row_spacing_and_outputs_have_bounded_canvas() {
    let g = graph();
    let mut p = MaterialGraphPreviewState::default();
    let a = layout_graph(&g, &p);
    p.set_visible(
        g.program,
        g.nodes
            .iter()
            .map(|n| MaterialGraphPreviewTarget::Expression(n.expression))
            .chain([MaterialGraphPreviewTarget::Output]),
        true,
    );
    let b = layout_graph(&g, &p);
    assert_eq!(a.output.x, b.output.x);
    assert!(b.size.y > a.size.y);
    assert!(b.size.y >= b.output.y + 130.0 + MATERIAL_PREVIEW_LAYOUT_HEIGHT + CANVAS_PADDING);
}
#[test]
fn empty_projection_retains_output_and_minimum_canvas() {
    let mut g = graph();
    g.nodes.clear();
    g.edges.clear();
    let a = layout_graph(&g, &MaterialGraphPreviewState::default());
    assert!(a.nodes.is_empty());
    assert_eq!(
        a.output,
        Vec2::new(CANVAS_PADDING + 282.0, CANVAS_PADDING + 88.0)
    );
    assert!(a.size.x >= 720.0 && a.size.y >= 420.0);
}
#[test]
fn depth_traversal_handles_fan_in_missing_inputs_and_cycles_without_recursion_loop() {
    let [a, b, c] = std::array::from_fn(|_| MaterialExpressionId::new());
    let inputs = BTreeMap::from([
        (a, vec![]),
        (b, vec![a, a]),
        (c, vec![a, b, MaterialExpressionId::new()]),
    ]);
    let mut memo = BTreeMap::new();
    assert_eq!(
        expression_depth(c, &inputs, &mut memo, &mut BTreeSet::new()),
        2
    );
    let cyclic = BTreeMap::from([(a, vec![b]), (b, vec![a])]);
    let mut memo = BTreeMap::new();
    assert!(expression_depth(a, &cyclic, &mut memo, &mut BTreeSet::new()) <= 2);
}
