use super::*;

#[test]
fn smoke_material_lab_is_discoverable_editable_and_native_without_demo_code() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-project");
    let catalog = ProjectEffectCatalog::scan(&root);
    let effect =
        aestra_core::EffectAsset::load_ron(root.join("effects/smoke_material_lab.aestra.ron"))
            .unwrap();
    let program =
        MaterialProgram::load_ron(root.join("materials/advected_smoke.aestra.material.ron"))
            .unwrap();
    let function = aestra_core::material::MaterialFunction::load_ron(
        root.join("materials/advected_smoke_density.aestra.material-function.ron"),
    )
    .unwrap();
    assert_eq!(
        function.custom_wesl.as_ref().unwrap().source,
        aestra_gpu::shader::ADVECTED_SMOKE_WESL
    );
    assert!(
        catalog
            .material_functions()
            .unwrap()
            .iter()
            .any(|f| f.id == function.id)
    );
    assert_eq!(catalog.material_program(program.id).unwrap(), program);
    for name in [
        "Smoke pattern velocity",
        "Smoke edge feather",
        "Smoke billow breakup",
        "Smoke depth variation",
        "Smoke density",
    ] {
        assert!(
            effect
                .parameters
                .iter()
                .any(|p| p.exposed && p.name == name)
        );
    }
    let compiled = catalog.compile_project(&effect).unwrap();
    assert!(compiled.root.point_lights.is_empty());
    assert!(compiled.root.particle_outputs().next().is_none());
    let presented = aestra_bevy_render::PresentedEffect::new(compiled.root.clone());
    for emitter in &compiled.root.emitters {
        for renderer in &emitter.renderers {
            assert!(
                presented
                    .material_binding_for_emitter(renderer.material, emitter.source)
                    .is_some()
            );
        }
    }
    let mut session = crate::test_support::session_with_timing_slack();
    session.open_compiled_effect(
        "effects/smoke_material_lab.aestra.ron",
        effect,
        compiled.root,
    );
    let target = crate::material_document::MaterialEditingTarget::Program {
        root: root.clone(),
        id: program.id,
    };
    let (_, projection, _, _) = selected_projection_for(&target, &session, &catalog).unwrap();
    let options =
        material_graph_palette_options(&session, &catalog, &target, program.id, &projection, None);
    assert!(options.iter().any(|o| matches!(o.descriptor.kind, MaterialGraphCreateKind::FunctionCall { function: aestra_core::material::MaterialFunctionRef::Project(id), .. } if id == function.id)));
    activate_material_graph_target(&mut session, &catalog, &target, program.id).unwrap();
    assert_eq!(session.standalone_material(), Some(program.id));
}
