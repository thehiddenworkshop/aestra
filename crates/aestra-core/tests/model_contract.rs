use aestra_core::{
    AssetDefinition, ChoreographyEvent, ChoreographyEventPayload, Curve, CurveKey, DiagnosticCode,
    EffectAsset, EffectClip, EffectClipSeed, EffectId, EffectMarker, EffectParameter,
    EffectPlaybackMode, Emitter, EmitterId, EmitterRegionId, EmitterShape, EmitterTransform,
    FlipbookDefinition, MODULE_EMISSION, MODULE_MOTION, MODULE_SHAPE, MarkerTimeReference,
    MaterialProperties, ModuleParameters, ParameterId, PropertyEvaluationDomain, PropertySource,
    PropertySourceValue, RendererInstance, ScalarRange, Value, Vec3Curve, Vec3Range,
};

#[test]
fn emitter_regions_split_and_join_without_changing_source_time() {
    let emitter = Emitter::basic_sprite("Emitter", 2.0);
    let implicit = emitter.timeline_regions();
    assert_eq!(implicit.len(), 1);
    assert_eq!(implicit[0].start_time, 0.0);
    assert_eq!(implicit[0].source_offset, 0.0);
    assert_eq!(implicit[0].duration, 2.0);

    let regions = emitter
        .split_timeline_region(implicit[0].id, 0.75, EmitterRegionId::from_u128(0x51))
        .unwrap();
    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].duration, 0.75);
    assert_eq!(regions[1].start_time, 0.75);
    assert_eq!(regions[1].source_offset, 0.75);
    assert_eq!(regions[1].duration, 1.25);

    let mut split = emitter.clone();
    split.regions = regions;
    let joined = split
        .join_timeline_region_with_next(split.regions[0].id)
        .unwrap();
    assert!(
        joined.is_empty(),
        "joining the original split restores legacy form"
    );
}

#[test]
fn semantic_ids_survive_round_trip() {
    let mut effect = EffectAsset::new("Round Trip", 1.5);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 1.5));
    let effect_id = effect.id;
    let emitter_id = effect.emitters[0].id;

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();

    assert_eq!(decoded.id, effect_id);
    assert_eq!(decoded.emitters[0].id, emitter_id);
}

#[test]
fn playback_modes_round_trip_and_legacy_looping_boole_still_load() {
    let mut effect = EffectAsset::new("Continuous", 1.0);
    effect.playback_mode = EffectPlaybackMode::LoopContinuous;
    let encoded = effect.to_pretty_ron().unwrap();
    assert!(encoded.contains("playback_mode: LoopContinuous"));
    assert_eq!(EffectAsset::from_ron(&encoded).unwrap(), effect);

    let legacy = encoded.replace("playback_mode: LoopContinuous", "looping: false");
    assert_eq!(
        EffectAsset::from_ron(&legacy).unwrap().playback_mode,
        EffectPlaybackMode::Once
    );
}

#[test]
fn normalized_curve_output_ranges_preserve_shape_and_output_units() {
    let curve = Curve::normalized(
        vec![
            CurveKey::new(0.0, 0.0),
            CurveKey::new(0.5, 1.0),
            CurveKey::new(1.0, 0.0),
        ],
        ScalarRange::new(10.0, 30.0),
    );

    assert_eq!(curve.sample(0.0), 10.0);
    assert_eq!(curve.sample(0.5), 30.0);
    assert_eq!(curve.output_range(), ScalarRange::new(10.0, 30.0));

    let encoded = ron::to_string(&curve).unwrap();
    let decoded: Curve = ron::from_str(&encoded).unwrap();
    assert_eq!(decoded, curve);
}

#[test]
fn legacy_curve_normalization_does_not_change_sampled_output() {
    let mut curve = Curve::new(vec![
        CurveKey::new(0.0, 4.0),
        CurveKey::new(0.5, 12.0),
        CurveKey::new(1.0, 8.0),
    ]);
    let samples = [0.0, 0.25, 0.5, 0.75, 1.0].map(|time| curve.sample(time));

    curve.normalize_output();

    assert_eq!(curve.output_range(), ScalarRange::new(4.0, 12.0));
    assert_eq!(
        [0.0, 0.25, 0.5, 0.75, 1.0].map(|time| curve.sample(time)),
        samples
    );
}

#[test]
fn missing_property_sources_are_inferred_and_explicit_overrides_persist() {
    let mut effect = EffectAsset::new("Legacy sources", 1.5);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 1.5));
    for module in &mut effect.emitters[0].modules {
        module.property_sources.clear();
    }
    let legacy = effect.to_pretty_ron().unwrap();
    assert!(!legacy.contains("property_sources"));

    let decoded = EffectAsset::from_ron(&legacy).unwrap();
    let initialize = decoded.emitters[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_core::MODULE_INITIALIZE)
        .unwrap();
    assert_eq!(
        initialize.property_source("lifetime"),
        Some(PropertySource::RandomRange)
    );
    let appearance = decoded.emitters[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .unwrap();
    assert_eq!(
        appearance.property_source("size"),
        Some(PropertySource::Curve(
            PropertyEvaluationDomain::ParticleLife
        ))
    );
    assert!(
        !decoded
            .to_pretty_ron()
            .unwrap()
            .contains("property_sources")
    );

    let mut overridden = decoded;
    let appearance = overridden.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .unwrap();
    appearance
        .property_sources
        .insert("size".into(), PropertySource::Constant);
    let encoded = overridden.to_pretty_ron().unwrap();
    assert!(encoded.contains("property_sources"));
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    let appearance = decoded.emitters[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == aestra_core::MODULE_APPEARANCE)
        .unwrap();
    assert_eq!(
        appearance.property_source("size"),
        Some(PropertySource::Constant)
    );
}

#[test]
fn alternate_property_source_values_round_trip_without_changing_the_format_version() {
    let mut effect = EffectAsset::new("Source values", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    let emission = effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap();
    let source = PropertySource::Curve(PropertyEvaluationDomain::EmitterTime);
    let curve = Curve::new(vec![CurveKey::new(0.0, 2.0), CurveKey::new(1.0, 20.0)]);
    emission
        .property_sources
        .insert("spawn_rate".into(), source);
    emission.property_source_values.insert(
        "spawn_rate".into(),
        vec![PropertySourceValue::new(
            source,
            Value::Curve(curve.clone()),
        )],
    );

    let encoded = effect.to_pretty_ron().unwrap();
    assert!(encoded.contains("property_source_values"));
    assert!(encoded.contains("version: 3"));
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    let emission = decoded.emitters[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap();

    assert_eq!(
        emission.parameter_value("spawn_rate"),
        Some(Value::Scalar(24.0))
    );
    assert_eq!(
        emission.active_parameter_value("spawn_rate"),
        Some(Value::Curve(curve))
    );
}

#[test]
fn vector_property_source_values_round_trip_with_independent_curve_ids() {
    let mut effect = EffectAsset::new("Vector source values", 2.0);
    effect.emitters.push(Emitter::basic_sprite("Emitter", 2.0));
    let motion = effect.emitters[0]
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    let source = PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
    let curves = Vec3Curve::constant([2.0, -4.0, 6.0]);
    assert_ne!(curves.curves[0].id, curves.curves[1].id);
    assert_ne!(curves.curves[1].id, curves.curves[2].id);
    motion.property_sources.insert("gravity".into(), source);
    motion.property_source_values.insert(
        "gravity".into(),
        vec![
            PropertySourceValue::new(
                PropertySource::RandomRange,
                Value::Vec3Range(Vec3Range::new([-3.0; 3], [3.0; 3])),
            ),
            PropertySourceValue::new(source, Value::Vec3Curve(curves.clone())),
        ],
    );

    assert!(effect.validation_report().is_valid());
    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    let motion = decoded.emitters[0]
        .modules
        .iter()
        .find(|module| module.module_type.0 == MODULE_MOTION)
        .unwrap();
    assert_eq!(
        motion.active_parameter_value("gravity"),
        Some(Value::Vec3Curve(curves))
    );
    assert!(decoded.validation_report().is_valid());
}

#[test]
fn timeline_markers_round_trip_and_validate_the_effect_range() {
    let mut effect = EffectAsset::new("Marked", 2.0);
    let marker = EffectMarker::new("Impact", 1.25);
    let id = marker.id;
    effect.markers.push(marker);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    assert_eq!(decoded.markers[0].id, id);
    assert_eq!(decoded.markers[0].name, "Impact");
    assert_eq!(decoded.markers[0].time, 1.25);

    effect.markers[0].time = 2.5;
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == DiagnosticCode::InvalidValue
                    && diagnostic.path == "effect.markers[0].time"
            })
    );
}

#[test]
fn choreography_events_round_trip_and_validate_timing_payload_and_marker_resolution() {
    let mut effect = EffectAsset::new("Events", 2.0);
    let marker = EffectMarker::new("Impact", 0.75);
    let marker_id = marker.id;
    effect.markers.push(marker);
    let mut event = ChoreographyEvent::new(
        "Shake",
        1.0,
        ChoreographyEventPayload::CameraShake { intensity: 0.8 },
    );
    let event_id = event.id;
    event.time_reference = Some(MarkerTimeReference::new(marker_id, 0.25));
    effect.choreography_events.push(event);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    assert_eq!(decoded.choreography_events[0].id, event_id);
    assert_eq!(decoded.choreography_events[0].time, 1.0);
    assert_eq!(
        decoded.choreography_events[0].time_reference,
        Some(MarkerTimeReference::new(marker_id, 0.25))
    );

    effect.choreography_events[0].time = 1.25;
    effect.choreography_events[0].payload =
        ChoreographyEventPayload::CameraShake { intensity: -1.0 };
    let report = effect.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidTiming
            && diagnostic.path.ends_with("time_reference")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidValue
            && diagnostic.path.ends_with("payload.intensity")
    }));
}

#[test]
fn marker_relative_starts_round_trip_and_reject_missing_or_stale_resolution() {
    let mut effect = EffectAsset::new("Relative", 2.0);
    let marker = EffectMarker::new("Impact", 0.75);
    let marker_id = marker.id;
    effect.markers.push(marker);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter.start_time = 1.0;
    emitter.start_reference = Some(MarkerTimeReference::new(marker_id, 0.25));
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    assert_eq!(decoded, effect);
    assert!(decoded.validation_report().is_valid());

    effect.emitters[0].start_time = 1.1;
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == DiagnosticCode::InvalidTiming
                    && diagnostic.path == "effect.emitters[0].start_reference"
            })
    );

    effect.emitters[0].start_time = 1.0;
    effect.emitters[0].start_reference =
        Some(MarkerTimeReference::new(aestra_core::MarkerId::new(), 0.25));
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == DiagnosticCode::InvalidReference
                    && diagnostic.path == "effect.emitters[0].start_reference.marker"
            })
    );
}

#[test]
fn emitter_display_color_is_optional_validated_authoring_metadata() {
    let mut effect = EffectAsset::new("Colored Timeline", 1.5);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.5);
    emitter.display_color = Some([0.25, 0.5, 0.75, 1.0]);
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    assert!(encoded.contains("display_color"));
    assert_eq!(EffectAsset::from_ron(&encoded).unwrap(), effect);

    effect.emitters[0].display_color = Some([1.2, 0.5, 0.75, 1.0]);
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == DiagnosticCode::InvalidValue
                    && diagnostic.path.ends_with("emitters[0].display_color")
            })
    );
}

#[test]
fn flipbooks_validate_texture_frames_and_round_trip() {
    let mut effect = EffectAsset::new("Flipbook", 1.0);
    let texture = AssetDefinition::texture("Atlas", "textures/atlas.png");
    let flipbook = FlipbookDefinition::grid("Burst", texture.id, 2, 2, 12.0);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter.renderers[0] = RendererInstance::flipbook(effect.materials[0].id, flipbook.id);
    effect.assets.push(texture);
    effect.flipbooks.push(flipbook);
    effect.emitters.push(emitter);

    assert!(effect.validation_report().is_valid());
    let encoded = effect.to_pretty_ron().unwrap();
    assert_eq!(EffectAsset::from_ron(&encoded).unwrap(), effect);

    effect.flipbooks[0].frames.clear();
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == DiagnosticCode::InvalidValue
                    && diagnostic.path.ends_with("flipbooks[0].frames")
            })
    );
}

#[test]
fn duplicated_emitters_receive_fresh_owned_ids() {
    let original = Emitter::basic_sprite("Original", 1.0);
    let original_module_ids = original
        .modules
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let original_renderer_ids = original
        .renderers
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>();
    let mut duplicate = original.clone();

    duplicate.regenerate_ids();

    assert_ne!(duplicate.id, original.id);
    assert!(
        duplicate
            .modules
            .iter()
            .zip(original_module_ids)
            .all(|(module, original_id)| module.id != original_id)
    );
    assert!(
        duplicate
            .renderers
            .iter()
            .zip(original_renderer_ids)
            .all(|(renderer, original_id)| renderer.id != original_id)
    );
}

#[test]
fn validation_reports_multiple_semantic_failures() {
    let mut effect = EffectAsset::new("Invalid", 0.0);
    let mut emitter = Emitter::basic_sprite("Broken", 1.0);
    emitter.id = EmitterId::from_u128(0);
    emitter.max_particles = 0;
    emitter.renderers.clear();
    *emitter.lifetime_mut() = ScalarRange::new(2.0, 1.0);
    effect.emitters.push(emitter);

    let report = effect.validation_report();

    assert!(!report.is_valid());
    for code in [
        DiagnosticCode::InvalidDuration,
        DiagnosticCode::NilId,
        DiagnosticCode::InvalidTiming,
        DiagnosticCode::InvalidCapacity,
        DiagnosticCode::InvalidValue,
        DiagnosticCode::MissingRenderer,
    ] {
        assert!(
            report.diagnostics.iter().any(|item| item.code == code),
            "missing diagnostic {code:?}"
        );
    }
}

#[test]
fn one_emitter_supports_multiple_renderers() {
    let mut effect = EffectAsset::new("Multi Renderer", 1.0);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .renderers
        .push(RendererInstance::sprite(effect.materials[0].id));
    effect.emitters.push(emitter);

    effect.validate().unwrap();
    assert_eq!(effect.emitters[0].renderers.len(), 2);
}

#[test]
fn texture_assets_are_stable_validated_renderer_references() {
    let mut effect = EffectAsset::new("Textured", 1.0);
    let texture = AssetDefinition::texture("Spark", "textures/spark.png");
    let texture_id = texture.id;
    let emitter = Emitter::basic_sprite("Emitter", 1.0);
    let MaterialProperties::Sprite {
        texture: material_texture,
        ..
    } = &mut effect.materials[0].properties;
    *material_texture = Some(texture_id);
    effect.assets.push(texture);
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();
    assert_eq!(decoded.assets[0].id, texture_id);
    assert!(encoded.contains("textures/spark.png"));

    effect.assets.clear();
    let report = effect.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidReference
            && diagnostic.path.ends_with("properties.texture")
    }));
}

#[test]
fn module_parameter_bindings_survive_round_trip() {
    let mut effect = EffectAsset::new("Bound", 1.0);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Spawn Rate".into(),
        default: Value::Scalar(24.0),
        exposed: true,
    };
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter.id);
    effect.parameters.push(parameter);
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();

    assert_eq!(decoded, effect);
    assert!(encoded.contains("bindings"));
}

#[test]
fn saving_replaces_an_existing_effect_without_leaving_temporary_files() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("effect.aestra.ron");
    let mut original = EffectAsset::new("Original", 1.0);
    original
        .emitters
        .push(Emitter::basic_sprite("Emitter", 1.0));
    original.save_ron(&path).unwrap();

    let mut replacement = original.clone();
    replacement.name = "Replacement".into();
    replacement.save_ron(&path).unwrap();

    assert_eq!(EffectAsset::load_ron(&path).unwrap(), replacement);
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn binding_types_are_validated_in_the_semantic_model() {
    let mut effect = EffectAsset::new("Bad binding", 1.0);
    let parameter = EffectParameter {
        id: ParameterId::new(),
        name: "Wrong Type".into(),
        default: Value::Vec2([1.0, 2.0]),
        exposed: true,
    };
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_EMISSION)
        .unwrap()
        .bindings
        .insert("spawn_rate".into(), parameter.id);
    effect.parameters.push(parameter);
    effect.emitters.push(emitter);

    let report = effect.validation_report();
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == DiagnosticCode::ParameterTypeMismatch)
    );
}

#[test]
fn volumetric_shape_dimensions_are_validated() {
    let mut effect = EffectAsset::new("Invalid volume", 1.0);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter
        .modules
        .iter_mut()
        .find(|module| module.module_type.0 == MODULE_SHAPE)
        .unwrap()
        .parameters = ModuleParameters::Shape {
        shape: EmitterShape::Box {
            half_extents: [2.0, 0.0, 4.0],
        },
    };
    effect.emitters.push(emitter);

    let report = effect.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidValue && diagnostic.path.ends_with("shape")
    }));
}

#[test]
fn emitter_transforms_round_trip_and_reject_degenerate_values() {
    let mut effect = EffectAsset::new("Transformed", 1.0);
    let mut emitter = Emitter::basic_sprite("Emitter", 1.0);
    emitter.transform = EmitterTransform {
        translation: [3.0, -2.0, 7.0],
        rotation: [0.0, 0.0, 0.70710677, 0.70710677],
        scale: [2.0, 1.0, 0.5],
    };
    effect.emitters.push(emitter);

    let encoded = effect.to_pretty_ron().unwrap();
    assert_eq!(EffectAsset::from_ron(&encoded).unwrap(), effect);

    effect.emitters[0].transform.scale[1] = 0.0;
    let report = effect.validation_report();
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidValue && diagnostic.path.ends_with("transform")
    }));
}

#[test]
fn reusable_effect_clips_round_trip_without_bumping_the_v3_format() {
    let child = EffectId::from_u128(0xC11D);
    let mut effect = EffectAsset::new("Composition", 3.0);
    let mut clip = EffectClip::new(child, 0.5, 1.5);
    clip.source_offset = 0.25;
    clip.transform.translation = [3.0, 4.0, 5.0];
    clip.transform.scale = [2.0, 2.0, 2.0];
    clip.seed = EffectClipSeed::Fixed(42);
    let parameter = ParameterId::new();
    clip.parameter_overrides
        .insert(parameter, Value::Scalar(48.0));
    let clip_id = clip.id;
    effect.effect_clips.push(clip);
    effect.choreography_order = vec![aestra_core::ChoreographyTrackId::EffectClip(clip_id)];

    let encoded = effect.to_pretty_ron().unwrap();
    let decoded = EffectAsset::from_ron(&encoded).unwrap();

    assert_eq!(decoded, effect);
    assert_eq!(decoded.format_version, 3);
    assert!(encoded.contains("effect_clips"));
    assert!(encoded.contains("parameter_overrides"));
    assert_eq!(
        decoded.effect_clips[0].parameter_overrides[&parameter],
        Value::Scalar(48.0)
    );
    assert_eq!(
        decoded.effect_clips[0].transform.translation,
        [3.0, 4.0, 5.0]
    );
    assert_eq!(decoded.choreography_order, effect.choreography_order);
}

#[test]
fn effect_clip_instance_transform_defaults_and_is_validated() {
    let mut effect = EffectAsset::new("Composition", 2.0);
    let clip = EffectClip::new(EffectId::from_u128(0xC11D), 0.0, 1.0);
    assert_eq!(clip.transform, EmitterTransform::default());
    effect.effect_clips.push(clip);
    effect.effect_clips[0].transform.scale[1] = 0.0;
    assert!(
        effect
            .validation_report()
            .diagnostics
            .iter()
            .any(|diagnostic| {
                diagnostic.code == DiagnosticCode::InvalidValue
                    && diagnostic.path.ends_with("effect_clips[0].transform")
            })
    );
}

#[test]
fn existing_v3_sources_without_effect_clips_remain_compatible() {
    let effect = EffectAsset::new("Legacy v3", 1.0);
    let encoded = effect.to_pretty_ron().unwrap();

    assert!(!encoded.contains("effect_clips"));
    assert!(
        EffectAsset::from_ron(&encoded)
            .unwrap()
            .effect_clips
            .is_empty()
    );
}

#[test]
fn effect_clip_timing_and_direct_self_references_are_validated() {
    let mut effect = EffectAsset::new("Invalid composition", 1.0);
    let mut clip = EffectClip::new(effect.id, 0.75, 0.5);
    clip.source_offset = -1.0;
    effect.effect_clips.push(clip);

    let report = effect.validation_report();

    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::ReferenceCycle
            && diagnostic.path.ends_with("effect_clips[0].source")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidTiming
            && diagnostic.path.ends_with("effect_clips[0].source_offset")
    }));
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == DiagnosticCode::InvalidTiming
            && diagnostic.path.ends_with("effect_clips[0]")
    }));
}

#[test]
fn effect_clip_time_mapping_and_seed_derivation_are_deterministic() {
    let clip = EffectClip::new(EffectId::from_u128(2), 1.0, 2.0);

    assert_eq!(clip.map_time(0.5, 4.0, false), None);
    assert_eq!(clip.map_time(1.5, 4.0, false), Some(0.5));
    assert_eq!(clip.map_time(3.0, 1.5, true), Some(0.5));
    assert_eq!(
        EffectClipSeed::Inherit.resolve(9, clip.id),
        EffectClipSeed::Inherit.resolve(9, clip.id)
    );
    assert_eq!(EffectClipSeed::Fixed(77).resolve(9, clip.id), 77);
}
