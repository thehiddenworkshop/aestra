use aestra_compiler::{EffectCompiler, MaterialCompiler};
use aestra_core::{
    EffectAsset, Emitter, MaterialExpressionId,
    material::{
        MaterialExpression, MaterialExpressionKind, MaterialInput, MaterialProgram, MaterialValue,
    },
};
use aestra_gpu::{
    GpuEffectArtifact, GpuParticle,
    material::{CompiledMaterialProgram, MaterialBackendCapabilities, MaterialShaderCompiler},
    particle_attributes::{GpuParticleAttributes as A, prune_particle_attributes},
};
use aestra_runtime::EffectInstance;
use encase::ShaderType;
use std::sync::Arc;

fn artifact() -> GpuEffectArtifact {
    let mut effect = EffectAsset::new("Attributes", 2.0);
    effect
        .emitters
        .push(Emitter::basic_sprite("Emitter", effect.duration));
    GpuEffectArtifact::from_instance(&EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&effect).unwrap(),
    )))
    .unwrap()
}

fn material(input: Option<MaterialInput>) -> CompiledMaterialProgram {
    let mut program = MaterialProgram::additive_sprite("Attributes");
    if let Some(input) = input {
        let output = if input == MaterialInput::ParticleColor {
            program.outputs.color
        } else {
            program.outputs.alpha
        };
        program
            .expressions
            .iter_mut()
            .find(|expression| expression.id == output)
            .unwrap()
            .kind = MaterialExpressionKind::Input(input);
    }
    let ir = MaterialCompiler.compile(&program).unwrap();
    MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap()
}

#[test]
fn live_material_inputs_legacy_tint_and_wireframe_keep_their_dependencies() {
    let mut renderer = artifact().renderers[0];
    let geometry = A::POSITION | A::SIZE | A::ROTATION;
    for (input, extra) in [
        (None, 0),
        (Some(MaterialInput::ParticleColor), A::COLOR | A::OPACITY),
        (Some(MaterialInput::ParticleOpacity), A::OPACITY),
        (
            Some(MaterialInput::ParticleNormalizedAge),
            A::NORMALIZED_AGE,
        ),
    ] {
        let compiled = material(input);
        assert_eq!(
            A::for_renderer(&renderer, Some(&compiled.reflection), false),
            A(geometry | extra)
        );
        assert_eq!(
            A::for_renderer(&renderer, Some(&compiled.reflection), true),
            A(geometry | A::COLOR)
        );
    }
    assert_eq!(
        A::for_renderer(&renderer, None, false),
        A(geometry | A::COLOR | A::OPACITY)
    );
    renderer.particle_color = 0;
    assert_eq!(A::for_renderer(&renderer, None, false), A(geometry));
    assert_eq!(
        A::for_renderer(
            &renderer,
            Some(&material(Some(MaterialInput::ParticleColor)).reflection),
            false
        ),
        A(geometry)
    );
}

#[test]
fn flipbook_age_is_only_live_for_lifetime_driven_consumers() {
    let mut renderer = artifact().renderers[0];
    renderer.renderer_kind = 1;
    renderer.frame_count = 8;
    renderer.textured = 1;
    assert_ne!(
        A::for_renderer(&renderer, None, false).0 & A::NORMALIZED_AGE,
        0
    );
    renderer.flipbook_flags = 1; // Effect-time playback, including random start.
    assert_eq!(
        A::for_renderer(&renderer, None, false).0 & A::NORMALIZED_AGE,
        0
    );
    renderer.flipbook_flags = 2;
    assert_ne!(
        A::for_renderer(&renderer, None, false).0 & A::NORMALIZED_AGE,
        0
    );
    assert_eq!(
        A::for_renderer(&renderer, None, true).0 & A::NORMALIZED_AGE,
        0
    );
    assert_eq!(
        A::for_renderer(&renderer, Some(&material(None).reflection), false).0 & A::NORMALIZED_AGE,
        0
    );
    renderer.frame_count = 1;
    assert_eq!(
        A::for_renderer(&renderer, None, false).0 & A::NORMALIZED_AGE,
        0
    );
}

#[test]
fn emitter_requirements_union_consumers_and_refresh_without_sticky_omissions() {
    let mut artifact = artifact();
    assert_eq!(
        artifact.emitters[0].omitted_attributes, 0,
        "full readback remains the default"
    );
    artifact.renderers.push(artifact.renderers[0]);
    prune_particle_attributes(
        &mut artifact.emitters,
        &mut artifact.renderers,
        &[A(7 | A::OPACITY), A(7 | A::NORMALIZED_AGE)],
    );
    assert_eq!(artifact.emitters[0].omitted_attributes, A::COLOR);
    assert_ne!(
        artifact.renderers[0].attribute_flags.x & A::NORMALIZED_AGE,
        0
    );
    assert_ne!(artifact.renderers[1].attribute_flags.x & A::OPACITY, 0);
    prune_particle_attributes(
        &mut artifact.emitters,
        &mut artifact.renderers,
        &[A::ALL, A::ALL],
    );
    assert_eq!(artifact.emitters[0].omitted_attributes, 0);
    assert_eq!(artifact.renderers[0].attribute_flags.x, 0);
    prune_particle_attributes(&mut artifact.emitters, &mut [], &[]);
    assert_eq!(artifact.emitters[0].omitted_attributes, A::ALL.0);
    assert_eq!(
        GpuParticle::min_size().get(),
        64,
        "particle storage/readback ABI must not shrink"
    );
}

#[test]
fn static_branch_pruning_removes_particle_dependencies_and_inspector_counts_them() {
    let mut program = MaterialProgram::additive_sprite("Static branch");
    let condition = MaterialExpressionId::new();
    let color = MaterialExpressionId::new();
    let selected = MaterialExpressionId::new();
    program.expressions.extend([
        MaterialExpression {
            id: condition,
            kind: MaterialExpressionKind::Constant(MaterialValue::Bool(false)),
        },
        MaterialExpression {
            id: color,
            kind: MaterialExpressionKind::Input(MaterialInput::ParticleColor),
        },
        MaterialExpression {
            id: selected,
            kind: MaterialExpressionKind::Select {
                condition,
                if_true: color,
                if_false: program.outputs.color,
            },
        },
    ]);
    program.outputs.color = selected;
    let ir = MaterialCompiler.compile(&program).unwrap();
    assert_eq!(ir.optimizations.pruned_static_branches, 1);
    let compiled = MaterialShaderCompiler
        .compile(&ir, &MaterialBackendCapabilities::portable_minimum())
        .unwrap();
    assert_eq!(
        A::for_renderer(&artifact().renderers[0], Some(&compiled.reflection), false),
        A(7)
    );

    let mut effect = EffectAsset::new("Inspector estimate", 2.0);
    effect
        .emitters
        .push(Emitter::basic_sprite("Emitter", effect.duration));
    let instance = EffectInstance::new(Arc::new(
        EffectCompiler::default().compile(&effect).unwrap(),
    ));
    let summary = aestra_gpu::particle_attributes::estimate_particle_attributes(&instance).unwrap();
    assert_eq!(summary.live, 5); // Geometry + legacy color/opacity, no flipbook age.
    assert_eq!(summary.omitted, 1);
}
