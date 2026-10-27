// Drives the production lockstep encoder with a persistent forward backlog.
#[test]
fn first_trail_observation_at_nonzero_clock_keeps_the_launch_cue() {
    let Some(mut fixture) = sized_event_trail_scene(false, 16) else {
        assert!(
            std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
            "native GPU required"
        );
        return;
    };
    let source = &mut fixture.scene.dispatches[0];
    source.spawn_rate = 0.0;
    source.burst_count = 1;
    source.burst_tick = 0;
    source.event_mask |= 1;
    let ring = aestra_gpu::PARTICLE_OUTPUT_RING_WORDS;
    let counters = fixture
        .scene
        .device
        .create_buffer_with_data(&BufferInitDescriptor {
            label: Some("initial launch output"),
            contents: &vec![0; (32 + ring) as usize * 4],
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        });
    fixture.scene.render[3] = counters.clone();
    fixture.buffers[4] = counters;
    fixture.effect.history_epoch = 5;
    fixture.effect.particle_outputs = vec![(
        aestra_runtime::CompiledParticleOutput {
            output: "launch".into(),
            source: 0,
            trigger: aestra_core::EventTrigger::OnSpawn,
            aggregation: aestra_core::EventAggregation::FirstPerTick,
        },
        32,
    )];
    assert!(fixture.history.tick.is_none());
    fixture.frame(1, 4); // first activation has no paused render-world tick-zero frame
    let bytes = read_back(
        &fixture.scene.device,
        &fixture.scene.queue,
        &fixture.buffers[4],
    );
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_le_bytes(*v))
        .collect();
    let at = (32 + aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS) as usize;
    let record = aestra_gpu::read_particle_output_slot(
        &words[at..at + aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize],
    );
    assert_eq!(
        record.map(|r| (r.tick, r.epoch, r.count)),
        Some((1, 5, 1)),
        "initializing trail history is not replay of a launch already delivered"
    );
}

type OutputCatchupScene = (
    Scene,
    Vec<aestra_runtime::CompiledEventLink>,
    Vec<(aestra_runtime::CompiledParticleOutput, u32)>,
);
fn output_catchup_scene() -> Option<OutputCatchupScene> {
    let (mut scene, mut links) = chained_event_scene()?;
    scene.dispatches[0].spawn_rate = 0.0;
    scene.dispatches[0].burst_count = 1;
    scene.dispatches[0].burst_tick = 0;
    scene.dispatches[0].lifetime = (0.05, 0.05);
    for dispatch in &mut scene.dispatches[1..] {
        dispatch.event_mask = 1;
        dispatch.speed = (0.0, 0.0);
        dispatch.lifetime = (10.0, 10.0);
        dispatch.colliders.clear();
    }
    scene.states = scene
        .dispatches
        .iter()
        .map(|dispatch| {
            StatefulPersistentState::allocate(
                &scene.device,
                dispatch.capacity,
                STRIDE,
                dispatch.fingerprint(),
                true,
            )
        })
        .collect();
    let ring = aestra_gpu::PARTICLE_OUTPUT_RING_WORDS;
    scene.render[3] = scene.device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("paced output regression"),
        contents: &vec![0; (32 + 2 * ring) as usize * 4],
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
    });
    links[0].count = 12;
    let routes = [
        (0, aestra_core::EventTrigger::OnDeath),
        (1, aestra_core::EventTrigger::OnSpawn),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (source, trigger))| {
        (
            aestra_runtime::CompiledParticleOutput {
                output: format!("source_{source}"),
                source,
                trigger,
                aggregation: aestra_core::EventAggregation::FirstPerTick,
            },
            32 + index as u32 * ring,
        )
    })
    .collect();
    Some((scene, links[..1].to_vec(), routes))
}

fn output_catchup_frame(
    scene: &mut Scene,
    links: &[aestra_runtime::CompiledEventLink],
    wiring: &RouteWiring<'_>,
    target: u32,
    budget: u32,
) -> u32 {
    let mut encoder = scene.device.create_command_encoder(&Default::default());
    let [particles, alive, indirect, counters] = &scene.render;
    let (ticks, _) = run_coupled_stateful(
        &scene.device,
        &mut encoder,
        (
            &scene.pipelines[0],
            &scene.pipelines[1],
            &scene.pipelines[2],
            Some(&scene.pipelines[3]),
        ),
        &scene.layout,
        &mut scene.states,
        &scene.dispatches,
        Coupling {
            domains: &mut [],
            inputs: StageInputs::default(),
            follower: &scene.follower,
            spawner: &scene.spawner,
            gatherer: &scene.gatherer,
        },
        links,
        wiring,
        &StatefulRenderBuffers {
            uploads: None,
            particles,
            alive,
            indirect,
            counters,
            world: &scene.no_world,
            physics: &scene.no_world,
        },
        (target as f32 + 0.5) * STATEFUL_TICK_DT,
        budget,
        None,
    );
    scene.queue.submit([encoder.finish()]);
    ticks
}

fn output_catchup_drain(
    scene: &Scene,
    wiring: &RouteWiring<'_>,
    delivery: &mut particle_outputs::Delivery,
) -> Vec<(usize, u64, u32)> {
    let bytes = read_back(&scene.device, &scene.queue, &scene.render[3]);
    let words: Vec<_> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|v| u32::from_le_bytes(*v))
        .collect();
    let mut records = Vec::new();
    for (index, (_, base)) in wiring.outputs.iter().enumerate() {
        for slot in 0..aestra_gpu::PARTICLE_OUTPUT_RING_TICKS {
            let at = (base + slot * aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS) as usize;
            if let Some(record) = aestra_gpu::read_particle_output_slot(
                &words[at..at + aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize],
            ) {
                records.push((index, *base, record));
            }
        }
    }
    records.sort_by_key(|(_, _, r)| r.tick);
    records
        .into_iter()
        .filter_map(|(index, ring, record)| {
            (delivery.accept(
                ring,
                &record,
                wiring.output_epoch,
                wiring.output_suppress_through,
            ) && record.count > 0)
                .then_some((index, record.tick, record.count))
        })
        .collect()
}

#[test]
fn paced_forward_backlog_exports_parent_deaths_and_child_births_without_replaying_history() {
    let Some((mut scene, links, outputs)) = output_catchup_scene() else {
        assert!(
            std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
            "native GPU required"
        );
        return;
    };
    let wiring = RouteWiring {
        outputs: &outputs,
        output_epoch: 7,
        ..Default::default()
    };
    let mut delivery = particle_outputs::Delivery::default();
    let mut heard = Vec::new();
    while scene.states[0].last_tick < 90 {
        assert!(output_catchup_frame(&mut scene, &links, &wiring, 90, 4) <= 4);
        heard.extend(output_catchup_drain(&scene, &wiring, &mut delivery));
    }
    assert_eq!(
        heard,
        [(0, 4, 1), (1, 4, 12)],
        "a long forward backlog is not replay"
    );
    let expected = chained_event_snapshot(&scene);
    output_catchup_frame(&mut scene, &links, &wiring, 90, 4);
    assert!(output_catchup_drain(&scene, &wiring, &mut delivery).is_empty());
    // A real seek has an explicit epoch/reconstruction boundary, even while its
    // forward reconstruction still has more than ten ticks left to catch up.
    for state in &mut scene.states {
        state.reset_to_zero(&scene.device);
    }
    let replay = RouteWiring {
        output_epoch: 8,
        output_suppress_through: 90,
        ..wiring
    };
    while scene.states[0].last_tick < 90 {
        output_catchup_frame(&mut scene, &links, &replay, 90, 4);
        assert!(output_catchup_drain(&scene, &replay, &mut delivery).is_empty());
    }
    assert_eq!(chained_event_snapshot(&scene), expected);
    let later_burst = [aestra_runtime::InputSpawnBurst {
        tick: 91,
        route: 0,
        target: 1,
        count: 1,
        events: vec![aestra_runtime::ParticleEvent {
            ordinal: 0,
            position: [2.0, 3.0, 4.0],
            velocity: [0.0; 3],
        }],
    }];
    let continued = RouteWiring {
        bursts: &later_burst,
        ..replay
    };
    output_catchup_frame(&mut scene, &links, &continued, 94, 4);
    assert_eq!(
        output_catchup_drain(&scene, &continued, &mut delivery),
        [(1, 92, 1)],
        "genuine ticks beyond a seek boundary resume live output"
    );
    for state in &mut scene.states {
        state.reset_to_zero(&scene.device);
    }
    let restart = RouteWiring {
        output_epoch: 9,
        ..wiring
    };
    let mut fresh = Vec::new();
    while scene.states[0].last_tick < 90 {
        output_catchup_frame(&mut scene, &links, &restart, 90, 4);
        fresh.extend(output_catchup_drain(&scene, &restart, &mut delivery));
    }
    assert_eq!(fresh, heard, "restart permits fresh output once");
    assert_eq!(chained_event_snapshot(&scene), expected);
}
