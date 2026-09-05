use aestra_compiler::{MaterialCompiler, MaterialGraphOutputKind};
use aestra_core::{
    MaterialExpressionId,
    material::{
        MaterialDomain, MaterialExpression, MaterialExpressionKind as Kind, MaterialInput,
        MaterialProgram, MaterialValue,
    },
};
use aestra_gpu::material::{MaterialBackendCapabilities, MaterialShaderCompiler};

fn program() -> MaterialProgram {
    MaterialProgram::from_ron(include_str!(
        "../../../assets/materials/mesh_material_lab.aestra.material.ron"
    ))
    .unwrap()
}

fn append(program: &mut MaterialProgram, kind: Kind) -> MaterialExpressionId {
    let id = MaterialExpressionId::new();
    program.expressions.push(MaterialExpression { id, kind });
    id
}

#[test]
fn optional_vertex_output_round_trips_and_is_present_in_mesh_graph() {
    let mut program = program();
    program.outputs.vertex_offset = None;
    let graph = MaterialCompiler.project_graph(&program, None);
    let port = graph
        .outputs
        .iter()
        .find(|port| port.kind == MaterialGraphOutputKind::VertexOffset)
        .unwrap();
    assert!(port.source.is_nil());
    let offset = append(
        &mut program,
        Kind::Constant(MaterialValue::Vec3([2.0, -3.0, 4.0])),
    );
    program.outputs.vertex_offset = Some(offset);
    let roundtrip = MaterialProgram::from_ron(&program.to_pretty_ron().unwrap()).unwrap();
    assert_eq!(roundtrip.outputs.vertex_offset, Some(offset));
    let ir = MaterialCompiler.compile(&roundtrip).unwrap();
    assert!(ir.outputs.vertex_offset.is_some());
    assert!(!ir.source_map.eliminated.contains(&offset));
    let compiled = MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap();
    assert_eq!(compiled.vertex_offset_bounds, Some([2.0, 3.0, 4.0]));
    assert!(compiled.shader.wgsl.contains("fn aestra_material_vertex"));
    assert!(compiled.shader.wgsl.contains("fn vertex_mesh_wireframe"));
}

#[test]
fn dynamic_vertex_math_compiles_for_rendered_and_wireframe_with_safe_culling_fallback() {
    let mut program = program();
    let position = append(&mut program, Kind::Input(MaterialInput::LocalPosition));
    let age = append(
        &mut program,
        Kind::Input(MaterialInput::ParticleNormalizedAge),
    );
    let offset = append(&mut program, Kind::Multiply(position, age));
    program.outputs.vertex_offset = Some(offset);
    let ir = MaterialCompiler.compile(&program).unwrap();
    let compiled = MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap();
    assert!(compiled.has_vertex_offset);
    assert_eq!(compiled.vertex_offset_bounds, None);
    assert!(compiled.shader.wgsl.contains("fn aestra_vertex_offset"));
}

#[test]
fn vertex_output_rejects_wrong_domain_type_and_fragment_dependencies() {
    let mut program = program();
    let position = append(&mut program, Kind::Input(MaterialInput::LocalPosition));
    let derivative = append(&mut program, Kind::DerivativeX { value: position });
    program.outputs.vertex_offset = Some(derivative);
    assert!(MaterialCompiler.compile(&program).is_err());
    program.outputs.vertex_offset = Some(program.outputs.alpha);
    assert!(MaterialCompiler.compile(&program).is_err());
    program.outputs.vertex_offset = Some(position);
    program.domain = MaterialDomain::Sprite;
    assert!(MaterialCompiler.compile(&program).is_err());
}

#[test]
fn vertex_validation_follows_dependencies_but_not_disabled_operations() {
    let mut program = program();
    let position = append(&mut program, Kind::Input(MaterialInput::LocalPosition));
    let depth = append(&mut program, Kind::Input(MaterialInput::SceneDepth));
    let indirect_depth = append(&mut program, Kind::Multiply(position, depth));
    program.outputs.vertex_offset = Some(indirect_depth);
    assert!(MaterialCompiler.compile(&program).is_err());
    let derivative = append(&mut program, Kind::DerivativeY { value: position });
    program.outputs.vertex_offset = Some(derivative);
    assert!(MaterialCompiler.compile(&program).is_err());
    program.disabled_expressions.push(derivative);
    let ir = MaterialCompiler.compile(&program).unwrap();
    assert!(
        MaterialShaderCompiler
            .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
            .is_ok()
    );
}

#[test]
fn absent_vertex_offset_preserves_legacy_mesh_behavior() {
    let mut program = program();
    program.outputs.vertex_offset = None;
    let text = program.to_pretty_ron().unwrap();
    assert!(!text.contains("vertex_offset"));
    let program = MaterialProgram::from_ron(&text).unwrap();
    let ir = MaterialCompiler.compile(&program).unwrap();
    let compiled = MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap();
    assert!(!compiled.has_vertex_offset);
    assert_eq!(compiled.vertex_offset_bounds, Some([0.0; 3]));
    assert!(!compiled.shader.wgsl.contains("fn aestra_vertex_offset"));
}
