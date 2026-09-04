use aestra_compiler::EffectCompiler;
use aestra_core::{
    ChoreographyEvent, ChoreographyEventId, ChoreographyEventPayload, ColorKey, Curve, CurveKey,
    EffectAsset, EffectParameter, EffectPlaybackMode, Emitter, EmitterRegion, EmitterShape,
    Gradient, ModuleInstance, ModuleParameters, ParameterId, PropertyEvaluationDomain,
    PropertySource, PropertySourceValue, ScalarRange, Value, Vec3Curve, Vec3Range,
};
use aestra_gpu::{
    GpuEffectArtifact, GpuGlobals, GpuParticle, WORKGROUP_SIZE, fold_seed, indirect_draw_commands,
    shader::GpuShaderPackage,
};
use aestra_runtime::{CompiledParameterOverride, EffectInstance, ParticleSample, RuntimeValue};
use encase::{ShaderType, StorageBuffer, internal::WriteInto};
use std::{
    borrow::Cow,
    sync::{Arc, mpsc},
    time::Duration,
};
use wgpu::util::DeviceExt;

const REQUIRED_GPU_ENV: &str = "AESTRA_REQUIRE_GPU_CONFORMANCE";
// Shader/pipeline warm-up on a busy software or self-hosted CI adapter can exceed 30 seconds even
// though the submission is healthy. Keep the wait bounded below the workflow timeout, but allow
// enough time for the first native submission to compile and complete.
const GPU_SUBMISSION_TIMEOUT: Duration = Duration::from_secs(120);
const GPU_MAP_CALLBACK_TIMEOUT: Duration = Duration::from_secs(5);
const TEST_SEED: u64 = 0x1234_5678_9abc_def0;
const ONCE_SAMPLE_TIMES: [f32; 4] = [0.05, 0.55, 1.1, 1.65];
const RESTART_SAMPLE_TIMES: [f32; 4] = [1.95, 2.05, 2.55, 4.1];
const CONTINUOUS_SAMPLE_TIMES: [f32; 6] = [1.9, 2.1, 2.55, 4.15, 4.55, 131_072.55];
const SOURCE_SAMPLE_TIMES: [f32; 4] = [0.35, 0.9, 1.4, 1.85];
const CONTINUOUS_SOURCE_SAMPLE_TIMES: [f32; 5] = [2.4, 2.9, 4.4, 4.9, 131_072.9];
const PARAMETER_SAMPLE_TIMES: [f32; 3] = [0.4, 1.0, 1.7];

const EVENT_STEPS: [EventStep; 5] = [
    EventStep::new(0.25, 0.25, &["Begin"]),
    EventStep::new(0.25, 0.5, &["Half A", "Half B"]),
    EventStep::new(0.75, 1.25, &["Accent"]),
    EventStep::new(0.75, 2.0, &["End", "Begin"]),
    EventStep::new(0.5, 2.5, &["Half A", "Half B"]),
];

#[derive(Clone, Copy)]
struct EventStep {
    delta: f32,
    continuous_time: f32,
    expected: &'static [&'static str],
}

impl EventStep {
    const fn new(delta: f32, continuous_time: f32, expected: &'static [&'static str]) -> Self {
        Self {
            delta,
            continuous_time,
            expected,
        }
    }
}

#[test]
fn choreography_event_timing_is_deterministic_across_playback_modes_and_boundaries() {
    let once = event_conformance_effect(EffectPlaybackMode::Once);
    assert_eq!(
        once.choreography_events
            .iter()
            .map(|event| event.name.as_str())
            .collect::<Vec<_>>(),
        ["Begin", "Half A", "Half B", "Accent", "End"]
    );
    assert_event_steps(
        once,
        &[
            EventStep::new(0.25, 0.25, &["Begin"]),
            EventStep::new(0.25, 0.5, &["Half A", "Half B"]),
            EventStep::new(0.75, 1.25, &["Accent"]),
            EventStep::new(0.75, 2.0, &["End"]),
            EventStep::new(1.0, 2.0, &[]),
        ],
    );

    for playback_mode in [
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        assert_event_steps(event_conformance_effect(playback_mode), &EVENT_STEPS);
        assert_multi_loop_event_step(playback_mode);
    }

    assert_seek_pause_and_restart_event_semantics();
}

#[test]
fn deterministic_gpu_particles_match_the_cpu_reference_across_playback_sources_and_parameters() {
    let once = conformance_effect(EffectPlaybackMode::Once, false);
    let artifact = GpuEffectArtifact::from_instance(&EffectInstance::new(once.clone())).unwrap();
    let shaders = GpuShaderPackage::for_artifact(&artifact).unwrap();
    let require_gpu = std::env::var_os(REQUIRED_GPU_ENV).is_some();
    let harness = match GpuHarness::new(&shaders.simulation.wgsl) {
        Ok(Some(harness)) => harness,
        Ok(None) if !require_gpu => {
            eprintln!(
                "skipping CPU/GPU conformance: no compatible compute adapter; set \
                 {REQUIRED_GPU_ENV}=1 to require one"
            );
            return;
        }
        Ok(None) => panic!("{REQUIRED_GPU_ENV}=1 but no compatible compute adapter was found"),
        Err(error) => panic!("failed to create GPU conformance harness: {error}"),
    };

    assert_effect_matches_at_times(&harness, once, &ONCE_SAMPLE_TIMES);
    assert_effect_matches_at_times(
        &harness,
        conformance_effect(EffectPlaybackMode::LoopRestart, false),
        &RESTART_SAMPLE_TIMES,
    );
    assert_effect_matches_at_times(
        &harness,
        conformance_effect(EffectPlaybackMode::LoopContinuous, true),
        &CONTINUOUS_SAMPLE_TIMES,
    );
    assert_effect_matches_at_times(
        &harness,
        source_conformance_effect(EffectPlaybackMode::Once, SourceFixture::Curves),
        &SOURCE_SAMPLE_TIMES,
    );
    assert_effect_matches_at_times(
        &harness,
        source_conformance_effect(EffectPlaybackMode::Once, SourceFixture::RandomRanges),
        &SOURCE_SAMPLE_TIMES,
    );
    assert_effect_matches_at_times(
        &harness,
        source_conformance_effect(
            EffectPlaybackMode::LoopContinuous,
            SourceFixture::RandomRanges,
        ),
        &CONTINUOUS_SOURCE_SAMPLE_TIMES,
    );
    assert_parameter_overrides_match_without_recompiling(&harness);
    assert_pruned_attributes_match_cpu(&harness);
    assert_event_aware_playback_matches(&harness, EffectPlaybackMode::Once);
    assert_event_aware_playback_matches(&harness, EffectPlaybackMode::LoopRestart);
    assert_event_aware_playback_matches(&harness, EffectPlaybackMode::LoopContinuous);
}

fn assert_event_steps(effect: Arc<aestra_runtime::CompiledEffect>, steps: &[EventStep]) {
    let playback_mode = effect.playback_mode;
    let mut instance = EffectInstance::with_seed(effect, TEST_SEED);
    let mut dispatched = Vec::new();
    for step in steps {
        instance.advance_with_choreography_events(step.delta, &mut dispatched);
        assert_eq!(
            event_names(&dispatched),
            step.expected,
            "event order diverged for {playback_mode:?} after advancing by {:.3}s",
            step.delta
        );
        let expected_time = match playback_mode {
            EffectPlaybackMode::LoopRestart => {
                step.continuous_time.rem_euclid(instance.effect().duration)
            }
            EffectPlaybackMode::Once => step.continuous_time.min(instance.effect().duration),
            EffectPlaybackMode::LoopContinuous => step.continuous_time,
        };
        assert_time_close(playback_mode, expected_time, instance.time());
    }
}

fn assert_multi_loop_event_step(playback_mode: EffectPlaybackMode) {
    let effect = event_conformance_effect(playback_mode);
    let mut instance = EffectInstance::with_seed(effect, TEST_SEED);
    let mut dispatched = Vec::new();
    instance.advance_with_choreography_events(4.5, &mut dispatched);
    assert_eq!(
        event_names(&dispatched),
        [
            "Begin", "Half A", "Half B", "Accent", "End", "Begin", "Half A", "Half B", "Accent",
            "End", "Begin", "Half A", "Half B",
        ],
        "a large step must dispatch every crossed event for {playback_mode:?}"
    );
    let expected_time = if playback_mode.is_continuous() {
        4.5
    } else {
        0.5
    };
    assert_time_close(playback_mode, expected_time, instance.time());
}

fn assert_seek_pause_and_restart_event_semantics() {
    let effect = event_conformance_effect(EffectPlaybackMode::Once);
    let mut instance = EffectInstance::with_seed(effect, TEST_SEED);
    let mut dispatched = Vec::new();

    instance.advance_with_choreography_events(0.0, &mut dispatched);
    assert!(dispatched.is_empty());
    assert_time_close(EffectPlaybackMode::Once, 0.0, instance.time());

    instance.seek(0.5);
    instance.advance_with_choreography_events(0.75, &mut dispatched);
    assert_eq!(event_names(&dispatched), ["Accent"]);
    assert_time_close(EffectPlaybackMode::Once, 1.25, instance.time());

    instance.advance_with_choreography_events(0.0, &mut dispatched);
    assert!(dispatched.is_empty());
    assert_time_close(EffectPlaybackMode::Once, 1.25, instance.time());

    instance.restart();
    instance.advance_with_choreography_events(0.01, &mut dispatched);
    assert_eq!(event_names(&dispatched), ["Begin"]);
    assert_time_close(EffectPlaybackMode::Once, 0.01, instance.time());
}

fn event_names(events: &[aestra_runtime::DispatchedChoreographyEvent]) -> Vec<&str> {
    events.iter().map(|event| event.name.as_str()).collect()
}

fn assert_time_close(playback_mode: EffectPlaybackMode, expected: f32, actual: f32) {
    assert!(
        (expected - actual).abs() <= f32::EPSILON * 8.0,
        "playback time diverged for {playback_mode:?}: expected {expected:.7}, got {actual:.7}"
    );
}

fn assert_event_aware_playback_matches(harness: &GpuHarness, playback_mode: EffectPlaybackMode) {
    let effect = event_conformance_effect(playback_mode);
    let mut instance = EffectInstance::with_seed(effect.clone(), TEST_SEED);
    let artifact = GpuEffectArtifact::from_instance(&instance).unwrap();
    let steps: &[EventStep] = if playback_mode == EffectPlaybackMode::Once {
        &[
            EventStep::new(0.25, 0.25, &["Begin"]),
            EventStep::new(0.25, 0.5, &["Half A", "Half B"]),
            EventStep::new(0.75, 1.25, &["Accent"]),
            EventStep::new(0.75, 2.0, &["End"]),
        ]
    } else {
        &EVENT_STEPS
    };
    let mut dispatched = Vec::new();
    let mut elapsed = 0.0;
    for step in steps {
        elapsed += step.delta;
        instance.advance_with_choreography_events(step.delta, &mut dispatched);
        assert_eq!(event_names(&dispatched), step.expected);

        let mut cpu = Vec::new();
        instance.evaluate(&mut cpu);
        let simulation_time = instance.time();
        let gpu = harness
            .simulate(
                &artifact,
                GpuGlobals {
                    time: simulation_time,
                    total_slots: artifact.total_slots,
                    seed: fold_seed(TEST_SEED),
                    emitter_count: artifact.emitters.len() as u32,
                    duration: effect.duration,
                    continuous: u32::from(playback_mode.is_continuous()),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| {
                panic!(
                    "GPU simulation failed for event-aware {playback_mode:?} playback at elapsed \
                     {elapsed:.3}s (simulation {simulation_time:.3}s): {error}"
                )
            });
        assert_particle_samples_match(playback_mode, elapsed, simulation_time, &cpu, &gpu);
    }
}

fn assert_effect_matches_at_times(
    harness: &GpuHarness,
    effect: Arc<aestra_runtime::CompiledEffect>,
    times: &[f32],
) {
    let template = EffectInstance::with_seed(effect.clone(), TEST_SEED);
    assert_instance_matches_at_times(harness, &template, times);
}

fn assert_pruned_attributes_match_cpu(harness: &GpuHarness) {
    use aestra_gpu::particle_attributes::GpuParticleAttributes as A;
    for mode in [
        EffectPlaybackMode::Once,
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        let effect = conformance_effect(mode, true);
        let mut instance = EffectInstance::with_seed(effect.clone(), TEST_SEED);
        let mut artifact = GpuEffectArtifact::from_instance(&instance).unwrap();
        instance.advance(if mode == EffectPlaybackMode::Once {
            0.9
        } else {
            2.9
        });
        let mut reference = Vec::new();
        instance.evaluate(&mut reference);
        assert!(!reference.is_empty());
        for omitted in [
            0,
            A::COLOR,
            A::OPACITY,
            A::COLOR | A::OPACITY,
            A::NORMALIZED_AGE,
            56,
            A::ALL.0,
            0,
        ] {
            for emitter in &mut artifact.emitters {
                emitter.omitted_attributes = omitted;
            }
            let mut expected = reference.clone();
            for particle in &mut expected {
                if omitted & A::POSITION != 0 {
                    particle.position = [0.0; 3];
                }
                if omitted & A::SIZE != 0 {
                    particle.size = 0.0;
                }
                if omitted & A::ROTATION != 0 {
                    particle.rotation = 0.0;
                }
                if omitted & A::COLOR != 0 {
                    particle.color[..3].fill(1.0);
                }
                if omitted & A::OPACITY != 0 {
                    particle.color[3] = 1.0;
                }
                if omitted & A::NORMALIZED_AGE != 0 {
                    particle.normalized_age = 0.0;
                }
            }
            let gpu = harness
                .simulate(
                    &artifact,
                    GpuGlobals {
                        time: instance.time(),
                        total_slots: artifact.total_slots,
                        seed: fold_seed(TEST_SEED),
                        emitter_count: artifact.emitters.len() as u32,
                        duration: effect.duration,
                        continuous: u32::from(mode.is_continuous()),
                        ..Default::default()
                    },
                )
                .unwrap();
            assert_particle_samples_match(mode, instance.time(), instance.time(), &expected, &gpu);
        }
    }
}

fn assert_instance_matches_at_times(
    harness: &GpuHarness,
    template: &EffectInstance,
    times: &[f32],
) {
    let effect = template.effect().clone();
    let artifact = GpuEffectArtifact::from_instance(template).unwrap();
    for &elapsed in times {
        let mut instance = template.clone();
        instance.advance(elapsed);
        let mut cpu = Vec::new();
        instance.evaluate(&mut cpu);
        let simulation_time = instance.time();
        let gpu = harness
            .simulate(
                &artifact,
                GpuGlobals {
                    time: simulation_time,
                    total_slots: artifact.total_slots,
                    seed: fold_seed(TEST_SEED),
                    emitter_count: artifact.emitters.len() as u32,
                    duration: effect.duration,
                    continuous: u32::from(effect.playback_mode.is_continuous()),
                    ..Default::default()
                },
            )
            .unwrap_or_else(|error| {
                panic!(
                    "GPU simulation failed for {:?} at elapsed {elapsed:.3}s (simulation \
                     {simulation_time:.3}s): {error}",
                    effect.playback_mode
                )
            });
        assert_particle_samples_match(effect.playback_mode, elapsed, simulation_time, &cpu, &gpu);
    }
}

fn assert_parameter_overrides_match_without_recompiling(harness: &GpuHarness) {
    let (effect, overrides) = parameter_conformance_effect();
    let default_instance = EffectInstance::with_seed(effect.clone(), TEST_SEED);
    assert_instance_matches_at_times(harness, &default_instance, &PARAMETER_SAMPLE_TIMES);
    let default_artifact = GpuEffectArtifact::from_instance(&default_instance).unwrap();
    let default_samples = evaluated_samples(&default_instance, 1.0);

    for (index, (parameter, value)) in overrides.iter().enumerate() {
        let mut individual = EffectInstance::with_seed(effect.clone(), TEST_SEED);
        individual.set_parameter(*parameter, value.clone()).unwrap();
        assert!(Arc::ptr_eq(default_instance.effect(), individual.effect()));
        assert_instance_matches_at_times(harness, &individual, &PARAMETER_SAMPLE_TIMES);
        assert_ne!(default_samples, evaluated_samples(&individual, 1.0));
        assert_parameter_artifact_changed(
            index,
            &default_artifact,
            &GpuEffectArtifact::from_instance(&individual).unwrap(),
        );
    }

    let mut overridden_instance = EffectInstance::with_seed(effect.clone(), TEST_SEED);
    for (parameter, value) in &overrides {
        overridden_instance
            .set_parameter(*parameter, value.clone())
            .unwrap();
    }
    assert!(Arc::ptr_eq(
        default_instance.effect(),
        overridden_instance.effect()
    ));
    assert_eq!(
        overridden_instance.overridden_parameters().count(),
        overrides.len()
    );
    assert_instance_matches_at_times(harness, &overridden_instance, &PARAMETER_SAMPLE_TIMES);

    let mut clip_override_instance = EffectInstance::with_seed(effect.clone(), TEST_SEED);
    let compiled_overrides = overrides
        .iter()
        .map(|(parameter, value)| CompiledParameterOverride {
            source: *parameter,
            slot: effect.parameter_slots[parameter],
            value: RuntimeValue::compile(value).unwrap(),
        })
        .collect::<Vec<_>>();
    clip_override_instance.apply_compiled_parameter_overrides(&compiled_overrides);
    assert_eq!(
        clip_override_instance.parameter_values(),
        overridden_instance.parameter_values()
    );
    assert_instance_matches_at_times(harness, &clip_override_instance, &PARAMETER_SAMPLE_TIMES);
}

fn evaluated_samples(instance: &EffectInstance, elapsed: f32) -> Vec<ParticleSample> {
    let mut instance = instance.clone();
    instance.advance(elapsed);
    let mut samples = Vec::new();
    instance.evaluate(&mut samples);
    samples
}

fn assert_parameter_artifact_changed(
    parameter_index: usize,
    default: &GpuEffectArtifact,
    overridden: &GpuEffectArtifact,
) {
    let default = &default.emitters[0];
    let overridden = &overridden.emitters[0];
    match parameter_index {
        0 => assert_ne!(default.spawn_rate, overridden.spawn_rate),
        1 => assert_ne!(default.lifetime, overridden.lifetime),
        2 => assert_ne!(default.gravity, overridden.gravity),
        3 => assert_ne!(default.size.keys[1], overridden.size.keys[1]),
        4 => assert_ne!(default.color.keys[0].color, overridden.color.keys[0].color),
        _ => panic!("unexpected conformance parameter {parameter_index}"),
    }
}

fn conformance_effect(
    playback_mode: EffectPlaybackMode,
    use_emitter_region: bool,
) -> Arc<aestra_runtime::CompiledEffect> {
    compile_effect(conformance_asset(playback_mode, use_emitter_region))
}

fn event_conformance_effect(
    playback_mode: EffectPlaybackMode,
) -> Arc<aestra_runtime::CompiledEffect> {
    let mut effect = conformance_asset(playback_mode, false);
    effect.name = "CPU GPU event timing".into();
    effect.choreography_events = vec![
        conformance_event(1, "Begin", 0.0),
        // Deliberately author equal-time events in reverse semantic-ID order. Compilation must
        // produce a stable order independent of source-vector insertion order.
        conformance_event(3, "Half B", 0.5),
        conformance_event(2, "Half A", 0.5),
        conformance_event(4, "Accent", 1.25),
        conformance_event(5, "End", effect.duration),
    ];
    compile_effect(effect)
}

fn conformance_event(id: u128, name: &str, time: f32) -> ChoreographyEvent {
    let mut event = ChoreographyEvent::new(
        name,
        time,
        ChoreographyEventPayload::GameplayNotify {
            topic: format!("conformance.{name}"),
        },
    );
    event.id = ChoreographyEventId::from_u128(id);
    event
}

fn conformance_asset(playback_mode: EffectPlaybackMode, use_emitter_region: bool) -> EffectAsset {
    let mut effect = EffectAsset::new("CPU GPU conformance", 2.0);
    effect.playback_mode = playback_mode;
    let mut emitter = Emitter::basic_sprite("Deterministic fixture", effect.duration);
    emitter.max_particles = 32;
    emitter.transform.translation = [1.25, -0.75, 2.5];
    emitter.transform.rotation = [
        0.0,
        0.0,
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ];
    emitter.transform.scale = [1.25, 0.8, 1.5];
    if use_emitter_region {
        emitter.regions = vec![EmitterRegion::new(0.35, 0.15, 1.25)];
    }

    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => {
                *spawn_rate = 6.0;
                *burst_count = 4;
            }
            ModuleParameters::Shape { shape } => {
                *shape = EmitterShape::Box {
                    half_extents: [0.5, 0.75, 0.25],
                };
            }
            ModuleParameters::Initialize {
                lifetime,
                speed,
                direction,
                spread_degrees,
                angular_velocity,
            } => {
                *lifetime = ScalarRange::new(0.8, 1.6);
                *speed = ScalarRange::new(2.0, 5.0);
                *direction = [0.25, 1.0, -0.2];
                *spread_degrees = 35.0;
                *angular_velocity = ScalarRange::new(-1.5, 1.25);
            }
            ModuleParameters::Motion {
                gravity,
                drag,
                turbulence,
            } => {
                *gravity = [0.5, -3.25, 1.0];
                *drag = 0.45;
                *turbulence = 0.3;
            }
            ModuleParameters::Appearance {
                size,
                opacity,
                color,
            } => {
                *size = Curve::new(vec![
                    CurveKey::new(0.0, 0.5),
                    CurveKey::new(0.4, 2.0),
                    CurveKey::new(1.0, 0.25),
                ]);
                *opacity = Curve::new(vec![
                    CurveKey::new(0.0, 0.2),
                    CurveKey::new(0.25, 1.0),
                    CurveKey::new(1.0, 0.0),
                ]);
                *color = Gradient::new(vec![
                    ColorKey::new(0.0, [0.2, 0.4, 1.0, 1.0]),
                    ColorKey::new(0.55, [1.0, 0.35, 0.1, 0.8]),
                    ColorKey::new(1.0, [0.1, 0.05, 0.2, 0.0]),
                ]);
            }
            ModuleParameters::Custom(_) => {}
        }
    }
    effect.emitters.push(emitter);
    effect
}

#[derive(Clone, Copy)]
enum SourceFixture {
    Curves,
    RandomRanges,
}

fn source_conformance_effect(
    playback_mode: EffectPlaybackMode,
    fixture: SourceFixture,
) -> Arc<aestra_runtime::CompiledEffect> {
    let mut effect = conformance_asset(playback_mode, false);
    let emitter = &mut effect.emitters[0];
    emitter.max_particles = 48;

    for module in &mut emitter.modules {
        match &mut module.parameters {
            ModuleParameters::Emission {
                spawn_rate,
                burst_count,
            } => {
                *spawn_rate = 5.0;
                *burst_count = 2;
                match fixture {
                    SourceFixture::Curves => set_source(
                        module,
                        "spawn_rate",
                        PropertySource::Curve(PropertyEvaluationDomain::EmitterTime),
                        Value::Curve(Curve::new(vec![
                            CurveKey::new(0.0, 2.0),
                            CurveKey::new(0.45, 11.0),
                            CurveKey::new(1.0, 4.0),
                        ])),
                    ),
                    SourceFixture::RandomRanges => set_source(
                        module,
                        "spawn_rate",
                        PropertySource::RandomRange,
                        Value::Range(ScalarRange::new(3.0, 9.0)),
                    ),
                }
            }
            ModuleParameters::Initialize { lifetime, .. } => {
                *lifetime = ScalarRange::new(3.5, 3.5);
            }
            ModuleParameters::Motion { .. } => match fixture {
                SourceFixture::Curves => {
                    let particle_curve =
                        PropertySource::Curve(PropertyEvaluationDomain::ParticleLife);
                    set_source(
                        module,
                        "drag",
                        particle_curve,
                        Value::Curve(Curve::new(vec![
                            CurveKey::new(0.0, 0.1),
                            CurveKey::new(0.5, 1.1),
                            CurveKey::new(1.0, 0.35),
                        ])),
                    );
                    set_source(
                        module,
                        "turbulence",
                        particle_curve,
                        Value::Curve(Curve::new(vec![
                            CurveKey::new(0.0, 0.0),
                            CurveKey::new(0.4, 0.8),
                            CurveKey::new(1.0, 0.2),
                        ])),
                    );
                    set_source(
                        module,
                        "gravity",
                        particle_curve,
                        Value::Vec3Curve(Vec3Curve {
                            curves: [
                                Curve::new(vec![CurveKey::new(0.0, -1.0), CurveKey::new(1.0, 2.0)]),
                                Curve::new(vec![
                                    CurveKey::new(0.0, -5.0),
                                    CurveKey::new(0.6, 1.0),
                                    CurveKey::new(1.0, -2.0),
                                ]),
                                Curve::new(vec![CurveKey::new(0.0, 0.5), CurveKey::new(1.0, 3.0)]),
                            ],
                        }),
                    );
                }
                SourceFixture::RandomRanges => {
                    set_source(
                        module,
                        "drag",
                        PropertySource::RandomRange,
                        Value::Range(ScalarRange::new(0.05, 1.25)),
                    );
                    set_source(
                        module,
                        "turbulence",
                        PropertySource::RandomRange,
                        Value::Range(ScalarRange::new(0.1, 0.9)),
                    );
                    set_source(
                        module,
                        "gravity",
                        PropertySource::RandomRange,
                        Value::Vec3Range(Vec3Range::new([-2.0, -6.0, -1.0], [3.0, 1.0, 4.0])),
                    );
                }
            },
            _ => {}
        }
    }

    compile_effect(effect)
}

fn parameter_conformance_effect() -> (
    Arc<aestra_runtime::CompiledEffect>,
    Vec<(ParameterId, Value)>,
) {
    let mut effect = conformance_asset(EffectPlaybackMode::Once, false);
    effect.name = "CPU GPU runtime parameters".into();
    effect.emitters[0].max_particles = 48;

    let definitions = [
        ("Spawn Rate", Value::Scalar(5.0), Value::Scalar(9.0)),
        (
            "Lifetime",
            Value::Range(ScalarRange::new(1.1, 1.8)),
            Value::Range(ScalarRange::new(2.4, 3.1)),
        ),
        (
            "Gravity",
            Value::Vec3([0.0, -2.0, 0.0]),
            Value::Vec3([2.0, -7.0, 1.5]),
        ),
        (
            "Size",
            Value::Curve(Curve::new(vec![
                CurveKey::new(0.0, 0.5),
                CurveKey::new(0.5, 1.5),
                CurveKey::new(1.0, 0.25),
            ])),
            Value::Curve(Curve::new(vec![
                CurveKey::new(0.0, 2.0),
                CurveKey::new(0.35, 4.5),
                CurveKey::new(1.0, 1.0),
            ])),
        ),
        (
            "Color",
            Value::Gradient(Gradient::new(vec![
                ColorKey::new(0.0, [0.1, 0.4, 1.0, 1.0]),
                ColorKey::new(1.0, [0.8, 0.1, 0.2, 0.0]),
            ])),
            Value::Gradient(Gradient::new(vec![
                ColorKey::new(0.0, [1.0, 0.8, 0.1, 0.9]),
                ColorKey::new(0.45, [0.8, 0.1, 1.0, 0.7]),
                ColorKey::new(1.0, [0.05, 0.2, 0.1, 0.0]),
            ])),
        ),
    ];
    let mut parameter_ids = Vec::with_capacity(definitions.len());
    let mut overrides = Vec::with_capacity(definitions.len());
    for (name, default, overridden) in definitions {
        let parameter = EffectParameter {
            id: ParameterId::new(),
            name: name.into(),
            default,
            exposed: true,
        };
        overrides.push((parameter.id, overridden));
        parameter_ids.push(parameter.id);
        effect.parameters.push(parameter);
    }

    for module in &mut effect.emitters[0].modules {
        match &module.parameters {
            ModuleParameters::Emission { .. } => {
                module
                    .bindings
                    .insert("spawn_rate".into(), parameter_ids[0]);
            }
            ModuleParameters::Initialize { .. } => {
                module.bindings.insert("lifetime".into(), parameter_ids[1]);
            }
            ModuleParameters::Motion { .. } => {
                module.bindings.insert("gravity".into(), parameter_ids[2]);
            }
            ModuleParameters::Appearance { .. } => {
                module.bindings.insert("size".into(), parameter_ids[3]);
                module.bindings.insert("color".into(), parameter_ids[4]);
            }
            _ => {}
        }
    }

    let compiled = compile_effect(effect);
    assert_eq!(compiled.parameters.len(), overrides.len());
    assert_eq!(
        compiled.optimizations.runtime_parameter_reads,
        overrides.len()
    );
    (compiled, overrides)
}

fn set_source(module: &mut ModuleInstance, property: &str, source: PropertySource, value: Value) {
    module.property_sources.insert(property.into(), source);
    module.property_source_values.insert(
        property.into(),
        vec![PropertySourceValue::new(source, value)],
    );
}

fn compile_effect(effect: EffectAsset) -> Arc<aestra_runtime::CompiledEffect> {
    Arc::new(EffectCompiler::default().compile(&effect).unwrap())
}

fn assert_particle_samples_match(
    playback_mode: EffectPlaybackMode,
    elapsed: f32,
    simulation_time: f32,
    cpu: &[ParticleSample],
    gpu: &[ParticleSample],
) {
    let mut cpu = cpu.to_vec();
    let mut gpu = gpu.to_vec();
    let order = |sample: &ParticleSample| (sample.emitter_index, sample.particle_index);
    cpu.sort_by_key(order);
    gpu.sort_by_key(order);

    assert_eq!(
        cpu.len(),
        gpu.len(),
        "alive particle count diverged for {playback_mode:?} at elapsed {elapsed:.3}s \
         (simulation {simulation_time:.3}s)\nCPU: {cpu:#?}\nGPU: {gpu:#?}"
    );
    for (expected, actual) in cpu.iter().zip(&gpu) {
        let identity = order(expected);
        assert_eq!(
            identity,
            order(actual),
            "particle identity diverged for {playback_mode:?} at elapsed {elapsed:.3}s \
             (simulation {simulation_time:.3}s)"
        );
        for axis in 0..3 {
            assert_close(
                playback_mode,
                elapsed,
                simulation_time,
                identity,
                &format!("position[{axis}]"),
                expected.position[axis],
                actual.position[axis],
                0.003,
                0.0005,
            );
        }
        assert_close(
            playback_mode,
            elapsed,
            simulation_time,
            identity,
            "size",
            expected.size,
            actual.size,
            0.002,
            0.0005,
        );
        assert_close(
            playback_mode,
            elapsed,
            simulation_time,
            identity,
            "rotation",
            expected.rotation,
            actual.rotation,
            0.001,
            0.0005,
        );
        assert_close(
            playback_mode,
            elapsed,
            simulation_time,
            identity,
            "normalized_age",
            expected.normalized_age,
            actual.normalized_age,
            0.0002,
            0.0002,
        );
        for channel in 0..4 {
            assert_close(
                playback_mode,
                elapsed,
                simulation_time,
                identity,
                &format!("color[{channel}]"),
                expected.color[channel],
                actual.color[channel],
                0.001,
                0.0005,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn assert_close(
    playback_mode: EffectPlaybackMode,
    elapsed: f32,
    simulation_time: f32,
    particle: (usize, u32),
    field: &str,
    expected: f32,
    actual: f32,
    absolute: f32,
    relative: f32,
) {
    let tolerance = absolute + relative * expected.abs().max(actual.abs());
    assert!(
        (expected - actual).abs() <= tolerance,
        "{field} diverged for {playback_mode:?} at elapsed {elapsed:.3}s (simulation \
         {simulation_time:.3}s) for particle {particle:?}: CPU={expected:.7}, GPU={actual:.7}, \
         tolerance={tolerance:.7}"
    );
}

struct GpuHarness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    reset_pipeline: wgpu::ComputePipeline,
    simulate_pipeline: wgpu::ComputePipeline,
}

impl GpuHarness {
    fn new(wgsl: &str) -> Result<Option<Self>, String> {
        let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        instance_descriptor.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(instance_descriptor);
        let adapter =
            match pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: None,
            })) {
                Ok(adapter) => adapter,
                Err(_) => return Ok(None),
            };
        if !adapter_supports_conformance(&adapter) {
            return Ok(None);
        }
        let limits = adapter.limits();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Aestra CPU GPU conformance device"),
            required_limits: limits,
            ..Default::default()
        }))
        .map_err(|error| error.to_string())?;
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Aestra CPU GPU conformance bindings"),
            entries: &(0..7)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage {
                            read_only: matches!(binding, 0 | 6),
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                })
                .collect::<Vec<_>>(),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Aestra CPU GPU conformance pipeline layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Aestra validated simulation WGSL"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(wgsl)),
        });
        let pipeline = |entry_point| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let reset_pipeline = pipeline("reset");
        let simulate_pipeline = pipeline("simulate");
        Ok(Some(Self {
            device,
            queue,
            bind_group_layout,
            reset_pipeline,
            simulate_pipeline,
        }))
    }

    fn simulate(
        &self,
        artifact: &GpuEffectArtifact,
        globals: GpuGlobals,
    ) -> Result<Vec<ParticleSample>, String> {
        let emitters = self.read_only_buffer("emitters", &encode(&artifact.emitters)?);
        let particles_bytes = encode(&artifact.particles)?;
        let particles = self.read_write_buffer("particles", &particles_bytes, true);
        let indices = vec![0_u32; artifact.total_slots as usize];
        let alive = self.read_write_buffer("alive", &encode(&indices)?, false);
        let dead = self.read_write_buffer("dead", &encode(&indices)?, false);
        let counters = self.read_write_buffer("counters", &encode(&vec![0_u32; 2])?, false);
        let indirect = self.read_write_buffer(
            "indirect",
            &encode(&indirect_draw_commands(&artifact.emitters))?,
            false,
        );
        let globals = self.read_only_buffer("globals", &encode(&globals)?);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Aestra CPU GPU conformance bind group"),
            layout: &self.bind_group_layout,
            entries: &[
                binding(0, &emitters),
                binding(1, &particles),
                binding(2, &alive),
                binding(3, &dead),
                binding(4, &counters),
                binding(5, &indirect),
                binding(6, &globals),
            ],
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Aestra CPU GPU conformance readback"),
            size: particles_bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Aestra CPU GPU conformance commands"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Aestra CPU GPU conformance simulation"),
                ..Default::default()
            });
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_pipeline(&self.reset_pipeline);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(&self.simulate_pipeline);
            pass.dispatch_workgroups(artifact.total_slots.div_ceil(WORKGROUP_SIZE), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&particles, 0, &staging, 0, particles_bytes.len() as u64);
        let submission = self.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (sender, receiver) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: Some(GPU_SUBMISSION_TIMEOUT),
            })
            .map_err(|error| {
                format!(
                    "GPU submission did not complete within {:.0}s: {error}",
                    GPU_SUBMISSION_TIMEOUT.as_secs_f32()
                )
            })?;
        receiver
            .recv_timeout(GPU_MAP_CALLBACK_TIMEOUT)
            .map_err(|error| {
                format!(
                    "GPU readback callback was not delivered within {:.0}s after submission: \
                     {error}",
                    GPU_MAP_CALLBACK_TIMEOUT.as_secs_f32()
                )
            })?
            .map_err(|error| error.to_string())?;
        let bytes = slice.get_mapped_range().to_vec();
        staging.unmap();
        let particles: Vec<GpuParticle> = StorageBuffer::new(bytes)
            .create()
            .map_err(|error| error.to_string())?;
        Ok(particles
            .into_iter()
            .filter(|particle| particle.alive != 0)
            .map(|particle| ParticleSample {
                emitter_index: particle.emitter_index as usize,
                particle_index: particle.particle_index,
                position: particle.position.to_array(),
                size: particle.size,
                rotation: particle.rotation,
                color: particle.color.to_array(),
                normalized_age: particle.normalized_age,
            })
            .collect())
    }

    fn read_only_buffer(&self, label: &str, contents: &[u8]) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
    }

    fn read_write_buffer(&self, label: &str, contents: &[u8], copy_src: bool) -> wgpu::Buffer {
        let mut usage = wgpu::BufferUsages::STORAGE;
        if copy_src {
            usage |= wgpu::BufferUsages::COPY_SRC;
        }
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage,
            })
    }
}

fn adapter_supports_conformance(adapter: &wgpu::Adapter) -> bool {
    let limits = adapter.limits();
    adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        && limits.max_compute_invocations_per_workgroup >= WORKGROUP_SIZE
        && limits.max_compute_workgroup_size_x >= WORKGROUP_SIZE
        && limits.max_storage_buffers_per_shader_stage >= 7
        && limits.max_bindings_per_bind_group >= 7
}

fn binding(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

fn encode<T>(value: &T) -> Result<Vec<u8>, String>
where
    T: ShaderType + WriteInto,
{
    let mut bytes = Vec::new();
    StorageBuffer::new(&mut bytes)
        .write(value)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}
