// Native behavioral coverage for both production stateful scheduling paths.
#[test]
fn sparse_scheduled_births_skip_empty_dispatches_but_keep_carry_drain_and_replay() {
    let Some(mut coupled) = scene(false) else {
        assert!(std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none());
        return;
    };
    let mut independent = scene(false).unwrap();
    for test in [&mut coupled, &mut independent] {
        for (i, dispatch) in test.dispatches.iter_mut().enumerate() {
            dispatch.spawn_rate = if i == 0 { 7.5 } else { 0.0 };
            // Cross a workgroup boundary while staying below the 256-slot pool.
            dispatch.burst_count = if i == 0 { 129 } else { 0 };
            dispatch.burst_tick = 10;
            dispatch.lifetime = (0.5, 0.5);
            dispatch.cutoffs.stop_tick = Some(80);
        }
    }
    let mut expected = Vec::new();
    for target in [12, 80, 240] {
        super::encoding_profile::begin();
        advance_chained_event_scene(&mut coupled, &[], target);
        let operations = super::encoding_profile::finish();
        let spawn = operations.iter().find(|op| op.name == "stateful_spawn").unwrap();
        assert!(spawn.dispatches < spawn.calls, "empty ticks must not dispatch spawn");
        advance_independent_spawn_scene(&mut independent, target);
        let snapshot = chained_event_snapshot(&coupled);
        assert_eq!(snapshot, chained_event_snapshot(&independent));
        assert_eq!(snapshot.spawn_counts, if target == 12 { vec![130, 0] } else { vec![139, 0] });
        if target == 240 {
            assert!(snapshot.particles.iter().all(Vec::is_empty), "integration must drain even when no spawn is dispatched");
        }
        expected.push((target, snapshot));
    }
    // Pausing cannot consume fractional carry or accidentally run an old burst.
    super::encoding_profile::begin();
    advance_chained_event_scene(&mut coupled, &[], 240);
    assert!(!super::encoding_profile::finish().iter().any(|op| op.name == "stateful_spawn"));
    for (target, snapshot) in expected {
        advance_chained_event_scene(&mut coupled, &[], target);
        advance_independent_spawn_scene(&mut independent, target);
        assert_eq!(chained_event_snapshot(&coupled), snapshot, "coupled replay at {target}");
        assert_eq!(chained_event_snapshot(&independent), snapshot, "independent replay at {target}");
    }
}

fn advance_independent_spawn_scene(scene: &mut Scene, target: u32) {
    loop {
        let mut encoder = scene.device.create_command_encoder(&Default::default());
        let [particles, alive, indirect, counters] = &scene.render;
        encoder.clear_buffer(counters, 0, Some(4));
        for (dispatch, state) in scene.dispatches.iter().zip(&mut scene.states) {
            dispatch_stateful_effect(
                &scene.device, &mut encoder, &scene.pipelines[0], &scene.pipelines[1],
                &scene.pipelines[2], Some(&scene.pipelines[3]), &scene.layout, state, dispatch,
                &StatefulRenderBuffers { uploads: None, particles, alive, indirect, counters,
                    world: &scene.no_world, physics: &scene.no_world },
                (target as f32 + 0.5) * STATEFUL_TICK_DT, SeekQuality::Exact,
            );
        }
        scene.queue.submit([encoder.finish()]);
        if scene.states.iter().all(|s| s.last_tick == target) { break; }
    }
}
