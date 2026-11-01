use aestra_compiler::{MaterialCompiler, MaterialFunctionLibrary, MaterialGraphCreateKind};
use aestra_core::{
    material::{MaterialFunctionRef, MaterialProgram},
    noise::NoiseFunction,
};

#[test]
fn noise_catalog_nodes_roundtrip_and_compile_with_typed_editable_defaults() {
    let functions = MaterialFunctionLibrary::default();
    functions.validation_report().into_result().unwrap();
    let base = MaterialProgram::additive_sprite("Noise nodes");
    let catalog = MaterialCompiler.graph_node_catalog_with_functions(&base, &functions);
    assert_eq!(catalog.iter().filter(|n| n.category == "Noise").count(), 8);
    for kind in NoiseFunction::ALL {
        let reference = MaterialFunctionRef::BuiltIn(kind.id());
        let function = functions.get(reference).unwrap();
        assert_eq!(functions.builtin_noise(reference), Some(kind));
        let plan = MaterialCompiler
            .plan_graph_node_creation_with_functions(
                &base,
                MaterialGraphCreateKind::FunctionCall {
                    function: reference,
                    output: function.outputs[0].id,
                },
                None,
                &functions,
            )
            .unwrap();
        let mut program = plan.replacement;
        program.outputs.alpha = program.expressions.last().unwrap().id;
        let saved = program.to_pretty_ron().unwrap();
        let restored = MaterialProgram::from_ron(&saved).unwrap();
        assert_eq!(restored, program.normalized());
        MaterialCompiler
            .compile_with_functions(&restored, &functions)
            .unwrap();
        let mut overridden = function.clone();
        overridden.name = "Host override".into();
        let mut overridden_library = functions.clone();
        overridden_library.register_builtin(overridden);
        assert!(overridden_library.builtin_noise(reference).is_none());
    }
}
