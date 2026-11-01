// Real production admission/launch paths: source-local layouts, no owner storage.
#[test]
#[ignore="native particle sampling births; run alone on a GPU"]
fn particle_sampling_native_event_cohorts_and_direct_births_match_cpu() {
    use aestra_core::particle_sampling::{ParticleSampling,SamplingMode};
    use aestra_runtime::particle_sampling::{SamplingCohort,direction,position};
    for count in [1,3,7,129] {
        let (mut scene,_)=chained_event_scene().expect("native GPU required");
        let settings=ParticleSampling{positions:SamplingMode::Legacy,directions:SamplingMode::Stratified,seed:u32::MAX,jitter:0.15,..Default::default()};
        for (i,d) in scene.dispatches.iter_mut().enumerate() {
            d.spawn_rate=0.0;d.burst_count=if i==0 {2}else{0};d.burst_tick=0;
            d.lifetime=(4.0,4.0);d.gravity=[0.0;3];d.speed=if i==1 {(10.0,12.0)}else{(0.0,0.0)};
            d.turbulence=0.0;d.drag=0.0;d.field_follow=None;d.colliders.clear();d.shape_kind=0;
            d.event_mask=if i==0 {1}else{0};
            d.velocity_distribution=aestra_core::VelocityDistribution::Sphere as u32;
            if i==1 {d.particle_sampling=Some(settings);d.capacity=if count==129 {200}else{19};}
        }
        scene.states=scene.dispatches.iter().map(|d|{
            let mut s=StatefulPersistentState::allocate(&scene.device,d.capacity,STRIDE,d.fingerprint(),d.event_mask!=0);
            s.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);s
        }).collect();
        let links=[aestra_runtime::CompiledEventLink{source:0,trigger:aestra_core::EventTrigger::OnSpawn,target:1,count,inherit:0.0}];
        advance_chained_event_scene(&mut scene,&links,1);
        let snapshot=chained_event_snapshot(&scene);
        let d=&scene.dispatches[1];
        assert_eq!(snapshot.spawn_counts[1],(2*count).min(d.capacity));
        let mut max_error=0f32;
        for record in &snapshot.particles[1] {
            let ordinal=record[8];let index=ordinal%count;
            let c=SamplingCohort{index,count,first:ordinal-index};
            let expected=direction(settings,d.seed as u32 ^ (d.seed >> 32) as u32,c,aestra_core::VelocityDistribution::Sphere,d.direction,d.spread);
            let speed=d.speed.0+(d.speed.1-d.speed.0)*aestra_runtime::StatefulSimulation::spawn_uniform(d.seed,ordinal as u64,0);
            for axis in 0..3 {let error=(f32::from_bits(record[axis+3])-expected[axis]*speed).abs();max_error=max_error.max(error);assert!(error<0.0001);}
        }
        assert!(scene.states.iter().all(|s|s.checkpoints.is_empty()));
        advance_chained_event_scene(&mut scene,&links,1);assert_eq!(chained_event_snapshot(&scene),snapshot,"pause consumes no births");
        println!("event fan-out={count}: accepted={}, max velocity error={max_error}, zero replay checkpoints",snapshot.spawn_counts[1]);
    }

    // Host input lists exercise the CPU upload side of the same versioned metadata.
    let mut host=scene(false).expect("native GPU required");
    let settings=ParticleSampling{positions:SamplingMode::Legacy,directions:SamplingMode::Stratified,seed:17,jitter:0.12,..Default::default()};
    for d in &mut host.dispatches {d.spawn_rate=0.0;d.burst_count=0;d.event_mask=0;d.field_follow=None;d.gravity=[0.0;3];d.drag=0.0;d.turbulence=0.0;d.lifetime=(4.0,4.0);d.speed=(10.0,10.0);d.velocity_distribution=aestra_core::VelocityDistribution::Sphere as u32;d.particle_sampling=Some(settings);}
    host.dispatches[1].capacity=19;
    host.states=host.dispatches.iter().map(|d|{let mut state=StatefulPersistentState::allocate(&host.device,d.capacity,STRIDE,d.fingerprint(),false);state.set_history_policy(PlaybackHistoryPolicy::PlaybackOnly);state}).collect();
    let events=vec![aestra_runtime::ParticleEvent{ordinal:0,position:[1.0,2.0,3.0],velocity:[0.0;3]},aestra_runtime::ParticleEvent{ordinal:1,position:[-1.0,2.0,3.0],velocity:[0.0;3]}];
    let bursts=[aestra_runtime::InputSpawnBurst{tick:1,route:0,target:1,count:7,events:events.clone()},aestra_runtime::InputSpawnBurst{tick:2,route:1,target:1,count:3,events}];
    let routes=RouteWiring{bursts:&bursts,..Default::default()};
    advance_event_scene(&mut host,&[],&routes,3);
    let snapshot=chained_event_snapshot(&host);assert_eq!(snapshot.spawn_counts[1],19);
    let d=&host.dispatches[1];
    for record in &snapshot.particles[1] {
        let ordinal=record[8];let (index,count,first)=if ordinal<14 {(ordinal%7,7,ordinal-ordinal%7)}else if ordinal<17 {(ordinal-14,3,14)}else{(ordinal-17,3,17)};
        let expected=direction(settings,d.seed as u32 ^ (d.seed>>32) as u32,SamplingCohort{index,count,first},aestra_core::VelocityDistribution::Sphere,d.direction,d.spread);
        for axis in 0..3 {assert!((f32::from_bits(record[axis+3])-expected[axis]*10.0).abs()<0.0001);}
    }
    assert!(host.states.iter().all(|s|s.checkpoints.is_empty()));
    println!("host-input cohorts: count 7 then 3, accepted 14 then 5, requested layouts preserved");
    let mut scene=scene(false).expect("native GPU required");
    let settings=ParticleSampling{positions:SamplingMode::Stratified,directions:SamplingMode::Stratified,sequence_count:129,seed:17,jitter:0.12};
    for (i,d) in scene.dispatches.iter_mut().enumerate() {
        d.spawn_rate=0.0;d.burst_count=if i==0 {129}else{0};d.burst_tick=0;d.lifetime=(4.0,4.0);
        d.speed=(10.0,10.0);d.velocity_distribution=aestra_core::VelocityDistribution::Sphere as u32;
        d.particle_sampling=Some(settings);d.gravity=[0.0;3];d.turbulence=0.0;d.drag=0.0;d.field_follow=None;d.shape_kind=1;d.shape_radius=4.0;
    }
    advance_independent_spawn_scene(&mut scene,1);
    let snapshot=chained_event_snapshot(&scene);assert_eq!(snapshot.spawn_counts[0],129);
    let d=&scene.dispatches[0];let seed=d.seed as u32 ^ (d.seed>>32) as u32;
    for record in &snapshot.particles[0] {
        let c=SamplingCohort::sequence(record[8],129);
        let p=position(settings,seed,c,aestra_core::EmitterShape::Sphere{radius:4.0});
        let v=direction(settings,seed,c,aestra_core::VelocityDistribution::Sphere,d.direction,d.spread);
        for axis in 0..3 {assert!((f32::from_bits(record[axis])-p[axis]).abs()<0.0001);assert!((f32::from_bits(record[axis+3])-v[axis]*10.0).abs()<0.0001);}
    }
    println!("direct sphere pattern: 129 accepted births, CPU-matched positions and velocities");
}
