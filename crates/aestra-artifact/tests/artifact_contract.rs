use aestra_artifact::{
    ARTIFACT_MAGIC, ArtifactError, CURRENT_ARTIFACT_VERSION, decode_effect, encode_effect,
};
use aestra_compiler::EffectCompiler;
use aestra_core::{
    AssetId, AssetKind, ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload, Curve,
    CurveKey, EffectAsset, EffectClip, EffectClipSeed, EffectId, EffectParameter,
    EffectPlaybackMode, Emitter, MaterialId, MaterialParameterId, MaterialProgramId,
    ModuleParameters, ParameterId, RendererId, UvRect, Value,
    material::{
        MaterialEvaluationDomain, MaterialExpression, MaterialExpressionKind, MaterialInput,
        MaterialInstance, MaterialParameter, MaterialParameterValue, MaterialProgram,
        MaterialProgramRef, MaterialRenderState, MaterialSamplerDescriptor,
        MaterialTextureColorSpace, MaterialTextureDescriptor, MaterialValue, MaterialValueType,
    },
};
use aestra_gpu::GpuEffectArtifact;
use aestra_runtime::{
    CompiledAsset, CompiledFlipbook, CompiledMaterial, CompiledParameterOverride, EffectInstance,
    Expression, MaterialColorPlan, ParameterSlot, RendererCapability, RendererPlan,
    RendererPlanKind, RuntimeValue,
};

#[test]
fn compiled_effect_round_trip_preserves_runtime_and_gpu_behavior() {
    let mut compiled = compiled_fixture();
    compiled.optimizations.material_common_subexpressions = 7;
    compiled.optimizations.material_specialized_parameter_reads = 11;
    compiled.optimizations.material_pruned_static_branches = 13;
    compiled.optimizations.material_pruned_features = 17;
    compiled.optimizations.material_texture_samples_authored = 19;
    compiled.optimizations.material_texture_samples_eliminated = 7;
    compiled.optimizations.material_texture_samples_live = 12;
    let bytes = encode_effect(&compiled).unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(text.contains(ARTIFACT_MAGIC));
    assert!(text.contains(&format!("format_version:{CURRENT_ARTIFACT_VERSION}")));
    assert!(text.contains("material_programs"));
    assert!(text.contains("material_instances"));
    assert!(text.contains("SampleTextureLevel"));

    let reloaded = decode_effect(&bytes).unwrap();
    assert_eq!(reloaded, compiled);

    for time in [0.0, 0.4, 1.1, 1.9] {
        let mut original_samples = Vec::new();
        let mut reloaded_samples = Vec::new();
        aestra_runtime::evaluate(&compiled, time, 0x5eed, &mut original_samples);
        aestra_runtime::evaluate(&reloaded, time, 0x5eed, &mut reloaded_samples);
        assert_eq!(reloaded_samples, original_samples);
    }

    let parameter = compiled.parameters[0].source;
    let mut original_instance = EffectInstance::with_seed(compiled.into(), 0x5eed);
    let mut reloaded_instance = EffectInstance::with_seed(reloaded.into(), 0x5eed);
    original_instance
        .set_parameter(parameter, Value::Scalar(14.0))
        .unwrap();
    reloaded_instance
        .set_parameter(parameter, Value::Scalar(14.0))
        .unwrap();
    original_instance.advance(1.1);
    reloaded_instance.advance(1.1);

    let mut original_samples = Vec::new();
    let mut reloaded_samples = Vec::new();
    original_instance.evaluate(&mut original_samples);
    reloaded_instance.evaluate(&mut reloaded_samples);
    assert_eq!(reloaded_samples, original_samples);

    let original_gpu = GpuEffectArtifact::from_instance(&original_instance).unwrap();
    let reloaded_gpu = GpuEffectArtifact::from_instance(&reloaded_instance).unwrap();
    assert_eq!(reloaded_gpu.total_slots, original_gpu.total_slots);
    assert_eq!(
        reloaded_gpu.bounds_half_extents,
        original_gpu.bounds_half_extents
    );
    assert_eq!(
        format!("{:?}", reloaded_gpu.emitters),
        format!("{:?}", original_gpu.emitters)
    );
    assert_eq!(
        format!("{:?}", reloaded_gpu.renderers),
        format!("{:?}", original_gpu.renderers)
    );
}

#[test]
fn current_artifacts_without_material_optimizer_statistics_decode_with_zero_defaults() {
    let mut compiled = compiled_fixture();
    compiled.optimizations.material_common_subexpressions = 7;
    compiled.optimizations.material_specialized_parameter_reads = 11;
    compiled.optimizations.material_pruned_static_branches = 13;
    compiled.optimizations.material_pruned_features = 17;
    compiled.optimizations.material_texture_samples_authored = 19;
    compiled.optimizations.material_texture_samples_eliminated = 7;
    compiled.optimizations.material_texture_samples_live = 12;
    let text = String::from_utf8(encode_effect(&compiled).unwrap()).unwrap();
    let legacy = text
        .replacen(",material_common_subexpressions:7", "", 1)
        .replacen(",material_specialized_parameter_reads:11", "", 1)
        .replacen(",material_pruned_static_branches:13", "", 1)
        .replacen(",material_pruned_features:17", "", 1)
        .replacen(",material_texture_samples_authored:19", "", 1)
        .replacen(",material_texture_samples_eliminated:7", "", 1)
        .replacen(",material_texture_samples_live:12", "", 1);
    assert_ne!(legacy, text);

    let decoded = decode_effect(legacy.as_bytes()).unwrap();

    assert_eq!(decoded.optimizations.material_common_subexpressions, 0);
    assert_eq!(
        decoded.optimizations.material_specialized_parameter_reads,
        0
    );
    assert_eq!(decoded.optimizations.material_pruned_static_branches, 0);
    assert_eq!(decoded.optimizations.material_pruned_features, 0);
    assert_eq!(decoded.optimizations.material_texture_samples_authored, 0);
    assert_eq!(decoded.optimizations.material_texture_samples_eliminated, 0);
    assert_eq!(decoded.optimizations.material_texture_samples_live, 0);
}

#[test]
fn artifact_rejects_wrong_magic_and_future_versions_structurally() {
    let bytes = encode_effect(&compiled_fixture()).unwrap();
    let text = String::from_utf8(bytes).unwrap();

    let wrong_magic = text.replacen(ARTIFACT_MAGIC, "NOT-AESTRA", 1);
    assert!(matches!(
        decode_effect(wrong_magic.as_bytes()),
        Err(ArtifactError::InvalidMagic { found }) if found == "NOT-AESTRA"
    ));

    let future = text.replacen(
        &format!("format_version:{CURRENT_ARTIFACT_VERSION}"),
        "format_version:999",
        1,
    );
    assert!(matches!(
        decode_effect(future.as_bytes()),
        Err(ArtifactError::UnsupportedVersion { found: 999 })
    ));

    let legacy = text.replacen(
        &format!("format_version:{CURRENT_ARTIFACT_VERSION}"),
        "format_version:1",
        1,
    );
    assert!(matches!(
        decode_effect(legacy.as_bytes()),
        Err(ArtifactError::UnsupportedVersion { found: 1 })
    ));

    let invalid_slot = text.replacen("Parameter(0)", "Parameter(99)", 1);
    assert_ne!(invalid_slot, text);
    assert!(matches!(
        decode_effect(invalid_slot.as_bytes()),
        Err(ArtifactError::InvalidData { path, message })
            if path.contains("spawn_rate") && message.contains("out of bounds")
    ));
}

fn compiled_fixture() -> aestra_runtime::CompiledEffect {
    let parameter = EffectParameter {
        id: ParameterId::from_u128(0x100),
        name: "Spawn Rate".into(),
        default: Value::Scalar(7.0),
        exposed: true,
    };
    let mut effect = EffectAsset::new("Artifact fixture", 2.0);
    effect.id = EffectId::from_u128(0x101);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    effect.parameters.push(parameter.clone());

    let mut emitter = Emitter::basic_sprite("Artifact emitter", effect.duration);
    emitter.id = aestra_core::EmitterId::from_u128(0x102);
    emitter.max_particles = 48;
    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => {
                *spawn_rate = 7.0;
                *burst_count = 3;
                module.bindings.insert("spawn_rate".into(), parameter.id);
            }
            ModuleParameters::Appearance { size, .. } => {
                *size = Curve::new(vec![
                    CurveKey::new(0.0, 0.5),
                    CurveKey::new(0.45, 2.0),
                    CurveKey::new(1.0, 0.1),
                ]);
            }
            _ => {}
        }
    }
    let semantic_material = MaterialId::from_u128(0x110);
    let semantic_parameter = MaterialParameterId::from_u128(0x111);
    let semantic_texture_parameter = MaterialParameterId::from_u128(0x113);
    let texture = AssetId::from_u128(0x106);
    let mut semantic_program = MaterialProgram::additive_sprite("Artifact semantic material");
    semantic_program.id = MaterialProgramId::from_u128(0x112);
    semantic_program.parameters.push(MaterialParameter {
        id: semantic_parameter,
        name: "Intensity".into(),
        value_type: MaterialValueType::Float,
        evaluation_domain: MaterialEvaluationDomain::Effect,
        default: Some(MaterialValue::Float(1.0)),
    });
    semantic_program.parameters.push(MaterialParameter {
        id: semantic_texture_parameter,
        name: "Texture".into(),
        value_type: MaterialValueType::Texture2D(MaterialTextureDescriptor {
            color_space: MaterialTextureColorSpace::SrgbColor,
            sampler: MaterialSamplerDescriptor::default(),
        }),
        evaluation_domain: MaterialEvaluationDomain::Instance,
        default: Some(MaterialValue::Texture2D(texture)),
    });
    let texture_expression = aestra_core::MaterialExpressionId::from_u128(0x114);
    let uv_expression = aestra_core::MaterialExpressionId::from_u128(0x115);
    let level_expression = aestra_core::MaterialExpressionId::from_u128(0x116);
    let sample_expression = aestra_core::MaterialExpressionId::from_u128(0x117);
    semantic_program.expressions.extend([
        MaterialExpression {
            id: texture_expression,
            kind: MaterialExpressionKind::Parameter(semantic_texture_parameter),
        },
        MaterialExpression {
            id: uv_expression,
            kind: MaterialExpressionKind::Input(MaterialInput::Uv0),
        },
        MaterialExpression {
            id: level_expression,
            kind: MaterialExpressionKind::Constant(MaterialValue::Float(1.0)),
        },
        MaterialExpression {
            id: sample_expression,
            kind: MaterialExpressionKind::SampleTextureLevel {
                texture: texture_expression,
                uv: uv_expression,
                level: level_expression,
            },
        },
    ]);
    semantic_program.outputs.color = sample_expression;
    emitter.renderers[0].material = semantic_material;
    effect.material_instances.push(MaterialInstance {
        id: semantic_material,
        program: MaterialProgramRef::Project(semantic_program.id),
        values: std::collections::BTreeMap::from([(
            semantic_parameter,
            MaterialParameterValue::EffectParameter(parameter.id),
        )]),
        render_state: MaterialRenderState::additive_sprite(),
    });
    effect.emitters.push(emitter);

    let mut event = ChoreographyEvent::new(
        "Impact",
        1.0,
        ChoreographyEventPayload::GameplayNotify {
            topic: "artifact.impact".into(),
        },
    );
    event.id = ChoreographyEventId::from_u128(0x103);
    effect.choreography_events.push(event);

    let child = EffectId::from_u128(0x104);
    let mut clip = EffectClip::new(child, 0.25, 1.5);
    clip.id = aestra_core::EffectClipId::from_u128(0x105);
    clip.source_offset = 0.1;
    clip.seed = EffectClipSeed::Fixed(42);
    effect.effect_clips.push(clip);

    let mut compiled = EffectCompiler::default()
        .compile_with_material_programs(
            &effect,
            &std::collections::BTreeMap::from([(semantic_program.id, semantic_program)]),
        )
        .unwrap();
    compiled.effect_clips[0].parameter_overrides = vec![CompiledParameterOverride {
        source: parameter.id,
        slot: ParameterSlot(0),
        value: RuntimeValue::Scalar(11.0),
    }];

    let flipbook = AssetId::from_u128(0x107);
    let material = MaterialId::from_u128(0x108);
    compiled.assets = vec![
        CompiledAsset {
            source: texture,
            name: "Particle texture".into(),
            kind: AssetKind::Texture,
            path: "textures/particle.png".into(),
        },
        CompiledAsset {
            source: flipbook,
            name: "Particle flipbook".into(),
            kind: AssetKind::Flipbook,
            path: "flipbooks/particle.ron".into(),
        },
    ];
    compiled.flipbooks = vec![CompiledFlipbook {
        source: flipbook,
        name: "Particle flipbook".into(),
        texture,
        frames: vec![
            UvRect {
                min: [0.0, 0.0],
                max: [0.5, 1.0],
            },
            UvRect {
                min: [0.5, 0.0],
                max: [1.0, 1.0],
            },
        ],
        frame_rate: 12.0,
        looping: true,
    }];
    compiled.materials = vec![CompiledMaterial {
        source: material,
        name: "Artifact material".into(),
        blend: aestra_core::BlendMode::Additive,
        softness: Expression::Constant(0.4),
        color: MaterialColorPlan::ParticleColor,
        texture: Some(texture),
        uv: UvRect::FULL,
    }];
    compiled.emitters[0].renderers = vec![
        RendererPlan {
            source: RendererId::from_u128(0x109),
            material,
            kind: RendererPlanKind::Sprite,
        },
        RendererPlan {
            source: RendererId::from_u128(0x10a),
            material,
            kind: RendererPlanKind::Flipbook {
                flipbook,
                time_source: aestra_core::FlipbookTimeSource::ParticleAge,
                playback: aestra_core::FlipbookPlaybackMode::PingPong,
                random_start: true,
            },
        },
    ];
    compiled
        .requirements
        .renderers
        .insert(RendererCapability::FlipbookParticles);
    compiled
}
