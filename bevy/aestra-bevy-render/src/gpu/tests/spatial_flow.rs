// Production fixed-tick dispatch, matched by spawn identity against the CPU reference.
#[test]
#[ignore="native fixed-tick spatial flow conformance; run alone on a GPU"]
fn spatial_flow_native_ticks_match_cpu_leave_deposits_and_allocate_no_replay_history() {
    use aestra_core::spatial_flow::{SpatialFlow,FlowMode,FlowSpace};
    for (mode,octaves,space) in [(FlowMode::Noise,1,FlowSpace::Effect),(FlowMode::Curl,1,FlowSpace::Effect),(FlowMode::Curl,3,FlowSpace::World)] {
        let mut scene=scene(false).expect("native GPU required");scene.domains.clear();scene.dispatches.truncate(1);
        let flow=SpatialFlow{mode,octaves,space,seed:if space==FlowSpace::World {u32::MAX-17} else {17},scale:0.4,strength:2.0,..Default::default()};
        let pose=[[0.0,-2.0,0.0,10.0],[-3.0,0.0,0.0,-5.0],[0.0,0.0,0.5,1.0]];
        let d=&mut scene.dispatches[0];
        d.field_follow=None;d.spawn_rate=60.0;d.burst_count=0;d.speed=(0.0,0.0);d.lifetime=(4.0,4.0);d.shape_kind=0;
        d.spread=0.0;d.gravity=[0.0,0.1,0.0];d.drag=0.3;d.turbulence=0.0;d.spatial_flow=Some(flow);d.world_from_effect=pose;
        let config=aestra_runtime::StatefulConfig{gravity:d.gravity,spawn_per_tick:1,speed:d.speed,lifetime:d.lifetime,direction:d.direction,velocity_distribution:aestra_core::VelocityDistribution::LegacyCone,spread:0.0,drag:d.drag,shape:aestra_runtime::SpawnShape::Point,placement:aestra_runtime::SpawnPlacement::IDENTITY,turbulence:0.0,colliders:[aestra_core::Collider::NONE;aestra_runtime::MAX_COLLIDERS],collider_count:0,capacity:CAPACITY,homing:None};
        let mut cpu=aestra_runtime::StatefulSimulation::new(config,d.seed);
        cpu.set_spatial_flow(Some(flow)).unwrap();cpu.set_flow_world_from_effect(pose).unwrap();
        let mut state=StatefulPersistentState::allocate(&scene.device,CAPACITY,STRIDE,d.fingerprint(),false);
        state.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);scene.states=vec![state];
        for tick in 0..120 {
            let placement=aestra_runtime::SpawnPlacement{translation:[tick as f32*0.05,0.0,0.0],..aestra_runtime::SpawnPlacement::IDENTITY};
            scene.dispatches[0].placement=placement;cpu.set_placement(placement);cpu.advance_tick();
            advance_independent_spawn_scene(&mut scene,tick+1);
        }
        assert!(scene.states[0].checkpoints.is_empty());
        let bytes=read_back(&scene.device,&scene.queue,&scene.states[0].state);
        let mut actual=std::collections::BTreeMap::new();
        for words in bytes.as_chunks::<{STRIDE as usize*4}>().0 {
            let f=|word:usize|f32::from_le_bytes(words[word*4..word*4+4].try_into().unwrap());
            if f(7)>0.0 && f(6)<f(7) {actual.insert(u32::from_le_bytes(words[32..36].try_into().unwrap()) as u64,[f(0),f(1),f(2)]);}
        }
        assert_eq!(actual.len(),cpu.live_count());
        let mut maximum=0.0f32;
        for (id,expected) in cpu.alive_particles() {
            for (a,b) in actual[&id].into_iter().zip(expected) {maximum=maximum.max((a-b).abs());assert!((a-b).abs()<0.003,"{mode:?}/{space:?} id={id}: {a} != {b}");}
        }
        let before=bytes;advance_independent_spawn_scene(&mut scene,120);
        assert_eq!(read_back(&scene.device,&scene.queue,&scene.states[0].state),before,"pause must leave flow state unchanged");
        println!("{mode:?}/{space:?} octaves={octaves}: 120 births, max trajectory error={maximum}, no replay checkpoints");
    }
}
