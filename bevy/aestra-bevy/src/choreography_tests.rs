use super::*;
use bevy::prelude::{On, ResMut};

fn notified(name: &str, duration: f32, times: &[f32]) -> EffectAsset {
    let mut effect = EffectAsset::new(name, duration);
    effect.playback_mode = EffectPlaybackMode::Once;
    effect.choreography_events = times
        .iter()
        .map(|&time| {
            ChoreographyEvent::new(
                format!("{name}:{time}"),
                time,
                ChoreographyEventPayload::GameplayNotify { topic: name.into() },
            )
        })
        .collect();
    effect
}

fn compile(root: &EffectAsset, children: &[EffectAsset]) -> Arc<CompiledEffectProject> {
    let compiler = EffectCompiler::default();
    Arc::new(CompiledEffectProject {
        root: Arc::new(compiler.compile(root).unwrap()),
        dependencies: children
            .iter()
            .map(|child| {
                let effect = Arc::new(compiler.compile(child).unwrap());
                (effect.source, effect)
            })
            .collect(),
    })
}

#[test]
fn nested_windows_cover_offsets_short_clips_and_expiry() {
    let leaf = notified("Leaf", 2.0, &[0.0, 0.25, 0.5, 0.75, 1.0]);
    let mut carrier = notified("Carrier", 2.0, &[0.0, 0.5]);
    let mut inner = EffectClip::new(leaf.id, 0.5, 0.5);
    inner.source_offset = 0.25;
    let inner_id = inner.id;
    carrier.effect_clips.push(inner);
    let mut root = notified("Root", 3.0, &[0.0]);
    let mut outer = EffectClip::new(carrier.id, 1.0, 1.0);
    outer.source_offset = 0.25;
    let outer_id = outer.id;
    root.effect_clips.push(outer);
    let project = compile(&root, &[carrier, leaf.clone()]);
    // The complete inner clip is gone by the end of this single update.
    let events = project.choreography_events_between(0.0, 2.5, true);
    let leaves: Vec<_> = events
        .iter()
        .filter(|event| event.effect == leaf.id)
        .collect();
    assert_eq!(
        leaves
            .iter()
            .map(|event| event.root_time)
            .collect::<Vec<_>>(),
        [1.25, 1.5, 1.75]
    );
    assert_eq!(
        leaves
            .iter()
            .map(|event| event.event.time)
            .collect::<Vec<_>>(),
        [0.25, 0.5, 0.75]
    );
    assert!(
        leaves
            .iter()
            .all(|event| event.path == [outer_id, inner_id])
    );
    assert!(!events.iter().any(|event| event.event.name == "Carrier:0"));
    assert!(
        project
            .choreography_events_between(2.5, 3.0, false)
            .is_empty()
    );
    assert!(
        project
            .choreography_events_between(2.5, 1.0, false)
            .is_empty()
    );
    assert!(
        project
            .choreography_events_between(0.0, f32::NAN, true)
            .is_empty()
    );
}

#[test]
fn project_events_are_partition_invariant_across_all_loop_modes() {
    for root_mode in [
        EffectPlaybackMode::Once,
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        for child_mode in [
            EffectPlaybackMode::Once,
            EffectPlaybackMode::LoopRestart,
            EffectPlaybackMode::LoopContinuous,
        ] {
            let mut leaf = notified("Leaf", 0.5, &[0.0, 0.25, 0.5]);
            leaf.playback_mode = child_mode;
            let mut root = notified("Root", 2.0, &[0.0, 1.0, 2.0]);
            root.playback_mode = root_mode;
            root.effect_clips.push(EffectClip::new(leaf.id, 0.25, 1.5));
            let project = compile(&root, &[leaf]);
            let coarse = project.choreography_events_between(0.0, 6.0, true);
            let fine: Vec<_> = (0..48)
                .flat_map(|i| {
                    project.choreography_events_between(
                        i as f32 / 8.0,
                        (i + 1) as f32 / 8.0,
                        i == 0,
                    )
                })
                .collect();
            assert_eq!(coarse, fine, "{root_mode:?}/{child_mode:?}");
            let names: Vec<_> = project
                .choreography_events_between(0.0, 0.75, true)
                .into_iter()
                .map(|e| e.event.name)
                .collect();
            if child_mode.is_looping() {
                assert_eq!(
                    names,
                    ["Root:0", "Leaf:0", "Leaf:0.25", "Leaf:0.5", "Leaf:0"]
                );
            } else {
                assert_eq!(names, ["Root:0", "Leaf:0", "Leaf:0.25", "Leaf:0.5"]);
            }
        }
    }
}

#[test]
fn clip_entry_at_loop_offset_does_not_replay_preroll_or_duplicate_boundary() {
    let mut leaf = notified("Leaf", 1.0, &[0.0, 0.5, 1.0]);
    leaf.playback_mode = EffectPlaybackMode::LoopContinuous;
    let mut root = notified("Root", 3.0, &[]);
    let mut clip = EffectClip::new(leaf.id, 0.5, 1.0);
    clip.source_offset = 2.0;
    root.effect_clips.push(clip);
    let project = compile(&root, &[leaf]);
    let names: Vec<_> = project
        .choreography_events_between(0.0, 2.0, true)
        .into_iter()
        .map(|e| e.event.name)
        .collect();
    assert_eq!(names, ["Leaf:0", "Leaf:0.5", "Leaf:1", "Leaf:0"]);
    // Seeking directly to entry suppresses the exact entry notification too.
    let names: Vec<_> = project
        .choreography_events_between(0.5, 0.75, false)
        .into_iter()
        .map(|e| e.event.name)
        .collect();
    assert!(names.is_empty());
}

fn player_fixture() -> Arc<CompiledEffectProject> {
    let mut leaf = notified("Leaf", 1.0, &[0.0, 0.25, 0.5, 1.0]);
    leaf.playback_mode = EffectPlaybackMode::LoopRestart;
    let mut root = notified("Root", 2.0, &[0.0, 0.5, 2.0]);
    root.playback_mode = EffectPlaybackMode::LoopContinuous;
    root.effect_clips.push(EffectClip::new(leaf.id, 0.0, 2.0));
    compile(&root, &[leaf])
}

#[test]
fn restart_notifications_use_the_same_rounded_frame_boundary_as_presentation() {
    let leaf = notified("Leaf", 0.31, &[0.0, 0.31]);
    let mut root = notified("Root", 0.31, &[]);
    root.playback_mode = EffectPlaybackMode::LoopRestart;
    root.effect_clips.push(EffectClip::new(leaf.id, 0.0, 0.31));
    let project = compile(&root, &[leaf]);
    let mut player = EffectPlayer::from_project(project.clone());
    let mut fine = Vec::new();
    for tick in 1..=57 {
        player.advance_clock(1.0 / 60.0);
        let events: Vec<_> = player.drain_project_choreography_events().collect();
        if tick % 19 == 0 {
            assert_eq!(player.frame(), 0);
            assert_eq!(
                events.iter().map(|e| e.event.time).collect::<Vec<_>>(),
                [0.31, 0.0]
            );
        } else if tick != 1 {
            assert!(events.is_empty());
        }
        fine.extend(events);
    }
    let mut coarse = EffectPlayer::from_project(project);
    coarse.advance_clock(0.951);
    assert_eq!(
        fine,
        coarse
            .drain_project_choreography_events()
            .collect::<Vec<_>>()
    );
}

#[test]
fn player_seek_step_external_clock_and_restart_have_explicit_event_policy() {
    let mut player = EffectPlayer::from_project(player_fixture());
    player.advance_clock(0.25);
    assert!(!player.project_choreography_events.is_empty());
    player.seek_simulation_time(0.5);
    assert!(player.project_choreography_events.is_empty());
    assert!(player.choreography_events.is_empty());
    player.advance_clock(0.25);
    assert!(player.project_choreography_events.is_empty());
    player.seek_simulation_time(0.0);
    player.advance_clock(0.125);
    assert!(player.project_choreography_events.is_empty());
    player.advance_clock(0.125);
    assert_eq!(player.project_choreography_events.len(), 1);
    player.step_back();
    assert!(player.project_choreography_events.is_empty());
    player.set_playback_time(1.0);
    assert!(player.project_choreography_events.is_empty());
    player.restart();
    player.advance_clock(0.0);
    assert!(player.project_choreography_events.is_empty());
    player.advance_clock(0.125);
    assert_eq!(player.project_choreography_events.len(), 2);
    assert!(
        player
            .project_choreography_events
            .iter()
            .all(|e| e.event.time == 0.0)
    );
    assert_eq!(player.drain_choreography_events().count(), 1);
    assert_eq!(player.drain_project_choreography_events().count(), 2);
}

#[derive(Resource, Default)]
struct Received(Vec<AestraChoreographyEvent>);

#[test]
fn single_effect_notifications_keep_empty_paths_and_silent_replay_seeks() {
    for seek in [
        SimulationSeekMode::StatelessDirect,
        SimulationSeekMode::CheckpointRestore,
        SimulationSeekMode::RestartReplay,
    ] {
        let mut effect = EffectCompiler::default()
            .compile(&notified("Root", 2.0, &[0.0, 0.5]))
            .unwrap();
        effect.seek_mode = seek;
        let mut player = EffectPlayer::from_compiled(Arc::new(effect));
        player.speed = 0.0;
        player.advance_clock(1.0);
        assert!(player.drain_choreography_events().next().is_none());
        player.speed = 1.0;
        player.advance_clock(0.001);
        assert!(player.drain_choreography_events().next().is_none());
        player.advance_clock(0.1);
        assert_eq!(player.drain_choreography_events().count(), 1);
        player.seek(0.5);
        player.seek(0.0);
        player.advance_clock(0.1);
        assert!(player.drain_choreography_events().next().is_none());
    }
    let mut app = App::new();
    app.init_resource::<Received>()
        .add_observer(
            |event: On<AestraChoreographyEvent>, mut received: ResMut<Received>| {
                received.0.push(event.event().clone());
            },
        )
        .add_systems(Update, tick_and_dispatch);
    let effect = notified("Single", 2.0, &[0.0]);
    let root = app.world_mut().spawn(EffectPlayer::new(&effect)).id();
    app.update();
    let received = &app.world().resource::<Received>().0;
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].player, root);
    assert_eq!(received[0].effect, effect.id);
    assert!(received[0].clip_path.is_empty());
}

#[test]
fn player_events_match_across_tick_sizes_speed_and_seek_modes() {
    for mode in [
        EffectPlaybackMode::Once,
        EffectPlaybackMode::LoopRestart,
        EffectPlaybackMode::LoopContinuous,
    ] {
        for seek in [
            SimulationSeekMode::StatelessDirect,
            SimulationSeekMode::CheckpointRestore,
            SimulationSeekMode::RestartReplay,
        ] {
            let mut project = (*player_fixture()).clone();
            Arc::make_mut(&mut project.root).playback_mode = mode;
            Arc::make_mut(&mut project.root).seek_mode = seek;
            let project = Arc::new(project);
            let mut coarse = EffectPlayer::from_project(project.clone());
            coarse.advance_clock(6.0);
            let expected: Vec<_> = coarse.drain_project_choreography_events().collect();
            let mut fine = EffectPlayer::from_project(project);
            fine.speed = 2.0;
            let mut actual = Vec::new();
            for _ in 0..180 {
                fine.advance_clock(1.0 / 60.0);
                actual.extend(fine.drain_project_choreography_events());
            }
            assert_eq!(actual, expected, "{mode:?}/{seek:?}");
            fine.seek_simulation_time(0.0);
            fine.advance_clock(1.0 / 60.0);
            assert!(fine.drain_project_choreography_events().next().is_none());
        }
    }
}

fn tick_and_dispatch(mut commands: Commands, mut players: Query<(Entity, &mut EffectPlayer)>) {
    for (entity, mut player) in &mut players {
        if player.playing {
            player.advance_clock(0.25);
        }
        dispatch_choreography_events(&mut commands, entity, &mut player);
    }
}

#[test]
fn observers_identify_roots_and_repeated_sources_without_presentation_entities() {
    let mut project = (*player_fixture()).clone();
    let root = Arc::make_mut(&mut project.root);
    let mut duplicate = root.effect_clips[0].clone();
    duplicate.source_clip = EffectClipId::new();
    root.effect_clips.push(duplicate);
    let project = Arc::new(project);
    let mut app = App::new();
    app.init_resource::<Received>()
        .add_observer(
            |event: On<AestraChoreographyEvent>, mut received: ResMut<Received>| {
                received.0.push(event.event().clone());
            },
        )
        .add_systems(Update, tick_and_dispatch);
    let first = app
        .world_mut()
        .spawn(EffectPlayer::from_project(project.clone()))
        .id();
    let second = app
        .world_mut()
        .spawn(EffectPlayer::from_project(project.clone()))
        .id();
    app.update();
    let received = &app.world().resource::<Received>().0;
    for entity in [first, second] {
        let events: Vec<_> = received
            .iter()
            .filter(|event| event.player == entity)
            .collect();
        assert_eq!(events.len(), 5);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.clip_path.is_empty())
                .count(),
            1
        );
        for clip in &project.root.effect_clips {
            let child: Vec<_> = events
                .iter()
                .filter(|event| event.clip_path == [clip.source_clip])
                .collect();
            assert_eq!(child.len(), 2);
            assert!(child.iter().all(|event| event.effect == clip.source.id));
        }
    }
    app.world_mut().resource_mut::<Received>().0.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(first)
        .unwrap()
        .playing = false;
    app.world_mut().entity_mut(second).despawn();
    app.update();
    assert!(app.world().resource::<Received>().0.is_empty());
    app.world_mut()
        .get_mut::<EffectPlayer>(first)
        .unwrap()
        .playing = true;
    app.update();
    let received = &app.world().resource::<Received>().0;
    assert_eq!(received.len(), 3);
    assert!(
        received
            .iter()
            .all(|event| event.player == first && event.event.time == 0.5)
    );
    // Queue drained: paused updates never resend notifications.
    app.world_mut().resource_mut::<Received>().0.clear();
    app.world_mut()
        .get_mut::<EffectPlayer>(first)
        .unwrap()
        .playing = false;
    app.update();
    assert!(app.world().resource::<Received>().0.is_empty());
}
