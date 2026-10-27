use super::*;
use aestra_core::{EffectAsset, Emitter};
use std::collections::BTreeMap;

fn effect(name: &str) -> Arc<aestra_runtime::CompiledEffect> {
    let mut asset = EffectAsset::new(name, 3.0);
    asset.emitters.push(Emitter::basic_sprite("Stars", 3.0));
    Arc::new(
        aestra_compiler::EffectCompiler::default()
            .compile(&asset)
            .unwrap(),
    )
}

#[test]
fn project_request_includes_inactive_dependencies_and_reuses_live_material_shaders() {
    let root = effect("Root");
    let child = effect("Future shell");
    let project = CompiledEffectProject {
        root,
        dependencies: BTreeMap::from([(child.source, child)]),
    };
    let request = EffectPipelinePreparation::for_project(&project);
    assert_eq!(request.status(), EffectPipelinePreparationStatus::Pending);
    assert_eq!(request.effects.len(), 2);
    assert!(request.effects.iter().all(|e| e.simulation_time() == 0.0));
    let mut shaders = Assets::default();
    let mut cache = MaterialShaderCache::default();
    let first = descriptors(&request.effects, &mut shaders, &mut cache).unwrap();
    assert_eq!(first.len(), 2);
    let count = shaders.len();
    let second = descriptors(&request.effects, &mut shaders, &mut cache).unwrap();
    assert_eq!(shaders.len(), count);
    for (a, b) in first.iter().zip(&second) {
        assert_eq!(
            a.material.as_ref().map(|m| m.shader.id()),
            b.material.as_ref().map(|m| m.shader.id())
        );
    }
    // Status belongs to the request, not the compiled artifact or a global ready bit.
    request.publish(EffectPipelinePreparationStatus::Ready { pipelines: 1 });
    assert!(matches!(
        EffectPipelinePreparation::extract_component(&request)
            .unwrap()
            .status(),
        EffectPipelinePreparationStatus::Ready { .. }
    ));
    assert_eq!(
        EffectPipelinePreparation::for_project(&project).status(),
        EffectPipelinePreparationStatus::Pending
    );
}

#[test]
fn unsupported_geometry_fails_instead_of_reporting_ready_for_a_partial_project() {
    let mut compiled = (*effect("Unsupported")).clone();
    compiled.emitters[0].renderers[0].kind = RendererPlanKind::Ribbon {
        width: 1.0,
        strand_count: 1,
    };
    let request = EffectPipelinePreparation::new([PresentedEffect::new(Arc::new(compiled))]);
    assert!(
        descriptors(
            &request.effects,
            &mut Assets::default(),
            &mut MaterialShaderCache::default()
        )
        .is_err()
    );
}

#[test]
fn no_render_app_fails_explicitly_and_never_advances_a_clock() {
    let mut app = App::new();
    install(&mut app);
    let entity = app
        .world_mut()
        .spawn(EffectPipelinePreparation::new([PresentedEffect::new(
            effect("Paused"),
        )]))
        .id();
    app.update();
    let request = app
        .world()
        .get::<EffectPipelinePreparation>(entity)
        .unwrap();
    assert!(matches!(
        request.status(),
        EffectPipelinePreparationStatus::Failed(_)
    ));
    assert_eq!(request.effects[0].simulation_time(), 0.0);
}
