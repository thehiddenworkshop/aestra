//! Host input events (event system E2): a host sends an effect its declared inputs; each is checked
//! against the declared payload, stamped with the tick it takes effect at, and recorded so a backward
//! seek replays exactly what was sent. Input routes (E3) turn them into bursts of particles.

use aestra_compiler::{EffectCompiler, ExtensionRegistry};
use aestra_core::{
    EffectAsset, Emitter, EventDefinition, EventField, EventFieldType, EventValue, InputSpawnRoute,
};
use aestra_runtime::{
    CompiledEventRoute, CompiledInputSpawn, EffectInstance, EmissionCutoffs, EventInputError,
    INPUT_KILL, INPUT_RESTART, INPUT_STOP_EMITTING, neutral_payload, trace_tick,
};
use std::sync::Arc;

fn instance() -> EffectInstance {
    let mut effect = EffectAsset::new("Mine", 4.0);
    effect.playback_mode = aestra_core::EffectPlaybackMode::Once;
    effect.emitters.push(Emitter::basic_sprite("Sparks", 4.0));
    let mut power = EventField::new("power", EventFieldType::Float);
    power.required = false;
    effect.event_inputs = vec![
        EventDefinition::new("Detonate")
            .with_field(EventField::new("position", EventFieldType::Vec3))
            .with_field(power),
    ];
    let compiled = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap();
    EffectInstance::new(Arc::new(compiled))
}

fn position() -> (String, EventValue) {
    ("position".to_string(), EventValue::Vec3([1.0, 2.0, 3.0]))
}

#[test]
fn inputs_are_checked_stamped_and_recorded() {
    let mut instance = instance();
    assert_eq!(instance.send_event("Detonate", vec![position()]), Ok(1));
    instance.set_playback_time(1.0);
    let later = instance
        .send_event(
            "Detonate",
            vec![position(), ("power".into(), EventValue::Float(2.0))],
        )
        .unwrap();
    assert_eq!(later, trace_tick(1.0) + 1);
    assert_eq!(instance.received_events().len(), 2);
    assert_eq!(instance.events_in_ticks(0..10).count(), 1);

    // Refused events leave the record unchanged.
    let detonate = |field: &str| ("Detonate".to_string(), field.to_string());
    for (payload, error) in [
        (vec![], {
            let (input, field) = detonate("position");
            EventInputError::MissingField { input, field }
        }),
        (vec![("position".into(), EventValue::Float(1.0))], {
            let (input, field) = detonate("position");
            EventInputError::InvalidField { input, field }
        }),
        (vec![position(), position()], {
            let (input, field) = detonate("position");
            EventInputError::InvalidField { input, field }
        }),
        (
            vec![position(), ("radius".into(), EventValue::Float(1.0))],
            {
                let (input, field) = detonate("radius");
                EventInputError::UnknownField { input, field }
            },
        ),
    ] {
        assert_eq!(instance.send_event("Detonate", payload), Err(error));
    }
    assert_eq!(
        instance.send_event("Launch", vec![]),
        Err(EventInputError::UnknownInput("Launch".into()))
    );
    assert_eq!(instance.received_events().len(), 2);

    // Sending after a backward seek replaces the recorded future and changes the input identity.
    let identity = instance.host_input_epoch();
    instance.seek(0.5);
    instance.send_event("Detonate", vec![position()]).unwrap();
    assert_eq!(instance.received_events().len(), 2);
    assert_eq!(instance.received_events()[1].tick, trace_tick(0.5) + 1);
    assert_ne!(instance.host_input_epoch(), identity);

    // A recorded history is adopted as is by another instance; a new play forgets it.
    let recorded = instance.received_events().to_vec();
    let mut replay = self::instance();
    replay.set_received_events(recorded.clone());
    assert_eq!(replay.received_events(), recorded.as_slice());
    assert_eq!(replay.host_input_epoch(), instance.host_input_epoch());
    replay.clear_received_events();
    assert!(replay.received_events().is_empty());
}

#[test]
fn restart_is_built_in_and_not_recorded() {
    let mut instance = instance();
    instance.set_playback_time(1.5);
    assert_eq!(instance.send_event(INPUT_RESTART, vec![]), Ok(0));
    assert_eq!(instance.time(), 0.0);
    assert!(instance.received_events().is_empty());
}

#[test]
fn a_neutral_payload_fills_every_field() {
    let instance = instance();
    let payload = neutral_payload(instance.effect(), "Detonate").unwrap();
    assert_eq!(
        payload,
        vec![
            ("position".to_string(), EventValue::Vec3([0.0; 3])),
            ("power".to_string(), EventValue::Float(0.0)),
        ]
    );
    assert!(neutral_payload(instance.effect(), "Launch").is_none());
}

fn live(instance: &EffectInstance) -> Vec<aestra_runtime::ParticleSample> {
    let mut samples = Vec::new();
    instance.evaluate(&mut samples);
    samples
}

#[test]
fn stop_emitting_and_kill_cut_the_emission_exactly() {
    let mut instance = instance();
    let mut uncut = instance.clone();
    instance.set_playback_time(1.0);
    // Built-in inputs take no payload.
    assert_eq!(
        instance.send_event(
            INPUT_STOP_EMITTING,
            vec![("power".into(), EventValue::Float(1.0))]
        ),
        Err(EventInputError::UnknownField {
            input: INPUT_STOP_EMITTING.into(),
            field: "power".into(),
        })
    );
    let stop = instance.send_event(INPUT_STOP_EMITTING, vec![]).unwrap();
    assert_eq!(
        instance.emission_cutoffs(),
        EmissionCutoffs {
            stop_tick: Some(stop),
            kill_tick: None,
        }
    );

    // Before the stop nothing changes, so a backward seek is exact; after it, only what was born
    // before remains, ageing out.
    for time in [0.5, 0.9] {
        instance.seek(time);
        uncut.seek(time);
        assert_eq!(live(&instance), live(&uncut));
    }
    instance.seek(1.5);
    uncut.seek(1.5);
    assert!(live(&instance).len() < live(&uncut).len());
    assert!(!live(&instance).is_empty());

    let kill = instance.send_event(INPUT_KILL, vec![]).unwrap();
    assert_eq!(instance.emission_cutoffs().stop_tick, Some(stop));
    assert_eq!(instance.emission_cutoffs().kill_tick, Some(kill));
    instance.seek(kill as f32 / 60.0 + 0.05);
    assert!(live(&instance).is_empty(), "nothing survives a kill");
    instance.seek(1.4);
    assert!(
        !live(&instance).is_empty(),
        "the past before the kill is intact"
    );

    // Forgetting the inputs restores the authored emission.
    instance.clear_received_events();
    assert_eq!(instance.emission_cutoffs(), EmissionCutoffs::NONE);
    uncut.seek(1.4);
    assert_eq!(live(&instance), live(&uncut));
}

/// A mine whose `Detonate` input bursts its shrapnel (event system E3), and a second route of the
/// same input into its smoke, at the origin.
fn mine() -> (EffectAsset, EffectInstance) {
    let mut effect = EffectAsset::new("Mine", 4.0);
    let shrapnel = Emitter::basic_sprite("Shrapnel", 4.0);
    let smoke = Emitter::basic_sprite("Smoke", 4.0);
    let mut detonate = EventDefinition::new("Detonate")
        .with_field(EventField::new("position", EventFieldType::Vec3))
        .with_field(EventField::new("power", EventFieldType::Float));
    detonate.fields[1].required = false;
    let mut burst = InputSpawnRoute::new(detonate.id, shrapnel.id);
    burst.count = 48;
    burst.position = Some(detonate.fields[0].id);
    let puff = InputSpawnRoute::new(detonate.id, smoke.id);
    effect.emitters = vec![Emitter::basic_sprite("Sparks", 4.0), shrapnel, smoke];
    effect.event_inputs = vec![detonate];
    effect.input_spawns = vec![burst, puff];
    let compiled = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&effect)
        .unwrap();
    (effect, EffectInstance::new(Arc::new(compiled)))
}

#[test]
fn input_routes_compile_to_sub_emitters_and_bursts() {
    let (effect, mut instance) = mine();
    let compiled = instance.effect().clone();
    assert_eq!(
        compiled.event_routes,
        [
            CompiledEventRoute::InputSpawn(CompiledInputSpawn {
                input: "Detonate".into(),
                target: 1,
                count: 48,
                position: Some("position".into()),
            }),
            CompiledEventRoute::InputSpawn(CompiledInputSpawn {
                input: "Detonate".into(),
                target: 2,
                count: 1,
                position: None,
            }),
        ]
    );
    // A route's target spawns only from its routes, and simulates persistent particles.
    assert!(!compiled.is_event_target(0));
    assert!(compiled.is_event_target(1) && compiled.is_event_target(2));
    assert_eq!(
        compiled.emitters[1].simulation_class,
        aestra_runtime::SimulationClass::Stateful
    );
    assert_eq!(
        compiled.emitters[0].simulation_class,
        aestra_runtime::SimulationClass::Analytic
    );

    // Two detonations in one tick, one more later: a burst per route and tick, events in the order
    // received, at the position field — the smoke at the origin.
    let at = |position| vec![("position".to_string(), EventValue::Vec3(position))];
    let first = instance
        .send_event("Detonate", at([1.0, 2.0, 3.0]))
        .unwrap();
    instance
        .send_event("Detonate", at([4.0, 5.0, 6.0]))
        .unwrap();
    instance.set_playback_time(1.0);
    let second = instance
        .send_event("Detonate", at([7.0, 8.0, 9.0]))
        .unwrap();
    let bursts = instance.input_spawn_bursts();
    let summary: Vec<_> = bursts
        .iter()
        .map(|burst| {
            (
                burst.tick,
                burst.route,
                burst.target,
                burst.count,
                burst
                    .events
                    .iter()
                    .map(|event| (event.ordinal, event.position))
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                first,
                0,
                1,
                48,
                vec![(0, [1.0, 2.0, 3.0]), (1, [4.0, 5.0, 6.0])]
            ),
            (first, 1, 2, 1, vec![(0, [0.0; 3]), (1, [0.0; 3])]),
            (second, 0, 1, 48, vec![(0, [7.0, 8.0, 9.0])]),
            (second, 1, 2, 1, vec![(0, [0.0; 3])]),
        ]
    );
    assert_eq!(bursts[0].records().count(), 96);
    assert!(
        bursts[0]
            .events
            .iter()
            .all(|event| event.velocity == [0.0; 3])
    );

    // A route to a disabled emitter is dropped: its input spawns nothing there.
    let mut disabled = effect.clone();
    disabled.emitters[2].enabled = false;
    let compiled = EffectCompiler::with_extensions(ExtensionRegistry::builtin())
        .compile(&disabled)
        .unwrap();
    assert_eq!(compiled.event_routes.len(), 1);
}
