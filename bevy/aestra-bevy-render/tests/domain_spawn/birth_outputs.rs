//! Native accepted-birth export, bounded deterministic capture and no link recursion.
use super::*;
use aestra_bevy_render::execution::{EventEmissionList, EventGatherPipeline, ParticleOutputSlot};
use aestra_core::{EventAggregation, EventTrigger};
use aestra_runtime::{CompiledEventLink, CompiledParticleOutput};

fn captured_spawn(
    gpu: &Gpu,
    pipeline: &DomainSpawnPipeline,
    target: &Emitter,
    list: &wgpu::Buffer,
    capacity: u32,
    events: &wgpu::Buffer,
) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    pipeline.encode(
        &gpu.device,
        &mut encoder,
        SpawnState {
            state: &target.state,
            free_list: &target.free_list,
            free_count: &target.free_count,
            spawn_counter: &target.spawn_counter,
            params: &target.params,
            output_births: Some(events),
        },
        list,
        &CompiledDomainSpawn {
            stage: 0,
            emission: EmissionLayout {
                resource: ResourceTypeId::new("org.example.test::resource/emission"),
                capacity,
            },
            inherit: 0.5,
        },
    );
    gpu.queue.submit([encoder.finish()]);
}

#[test]
fn accepted_external_births_merge_into_host_outputs_without_changing_particles_or_links() {
    let Some(gpu) = gpu() else { return };
    let pipeline = DomainSpawnPipeline::new(&gpu.device);
    let gather = EventGatherPipeline::new(&gpu.device);
    let target = emitter(&gpu, 10, 5);
    let mut words =
        vec![0; aestra_gpu::particle_event_words(aestra_runtime::PARTICLE_EVENT_CAPACITY)];
    // An ordinary spawn already recorded this tick: child capture must append, not overwrite it.
    words[0] = 1;
    words[4] = 1;
    words[5] = 4;
    words[6] = (-1.0f32).to_bits();
    let events = storage(&gpu, &words);
    let list = emission(&gpu, 40, 64);
    captured_spawn(&gpu, &pipeline, &target, &list, 64, &events);
    let captured = read(&gpu, &events);
    assert_eq!(
        &captured[..2],
        &[11, 0],
        "ten accepted births, not forty requested"
    );
    assert_eq!(&captured[4..12], &words[4..12], "original record preserved");
    for i in 0..10 {
        let record = &captured[12 + i * 8..20 + i * 8];
        assert_eq!(&record[..2], &[8, 5 + i as u32]);
        assert_eq!(
            record[2..5]
                .iter()
                .map(|word| f32::from_bits(*word))
                .collect::<Vec<_>>(),
            [i as f32, 2.0 * i as f32, -(i as f32)]
        );
        assert_eq!(
            record[5..8]
                .iter()
                .map(|word| f32::from_bits(*word))
                .collect::<Vec<_>>(),
            [0.0, 2.0, 5.0]
        );
    }
    let control = emitter(&gpu, 10, 5);
    spawn(&gpu, &pipeline, &control, &list, 64);
    assert_eq!(read(&gpu, &target.state), read(&gpu, &control.state));
    assert_eq!(read(&gpu, &target.spawn_counter), [15]);
    captured_spawn(&gpu, &pipeline, &target, &list, 64, &events);
    assert_eq!(
        read(&gpu, &events),
        captured,
        "capacity rejection exports no cue"
    );

    for (index, aggregation) in [
        EventAggregation::FirstPerTick,
        EventAggregation::EachEvent { limit: 3 },
    ]
    .into_iter()
    .enumerate()
    {
        let rings = storage(
            &gpu,
            &vec![0; aestra_gpu::PARTICLE_OUTPUT_RING_WORDS as usize],
        );
        let route = CompiledParticleOutput {
            output: "child_birth".into(),
            source: 0,
            trigger: EventTrigger::OnSpawn,
            aggregation,
        };
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        gather.encode_output(
            &gpu.device,
            &mut encoder,
            &events,
            ParticleOutputSlot {
                counters: &rings,
                ring: 0,
                tick: 7,
                epoch: 2,
            },
            &route,
        );
        gpu.queue.submit([encoder.finish()]);
        let words = read(&gpu, &rings);
        let start = 7 * aestra_gpu::PARTICLE_OUTPUT_SLOT_WORDS as usize;
        let record = aestra_gpu::read_particle_output_slot(&words[start..]).unwrap();
        assert_eq!((record.tick, record.epoch, record.count), (7, 2, 11));
        assert_eq!(
            record
                .first
                .iter()
                .map(|record| record.0)
                .collect::<Vec<_>>(),
            if index == 0 { vec![4] } else { vec![4, 5, 6] }
        );
        let raised = route.raise(record.count, &record.first, record.tick);
        assert_eq!(raised.len(), if index == 0 { 1 } else { 3 });
        assert!(
            raised
                .iter()
                .all(|cue| cue.tick == 7 && cue.magnitude == 11.0)
        );
    }
    // Existing OnSpawn link sees only the ordinary birth, never output-only child records.
    let link = CompiledEventLink {
        source: 0,
        target: 1,
        trigger: EventTrigger::OnSpawn,
        count: 1,
        inherit: 0.0,
    };
    let output = storage(&gpu, &vec![0; aestra_gpu::particle_event_words(64)]);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    gather.encode(
        &gpu.device,
        &mut encoder,
        &events,
        EventEmissionList {
            buffer: &output,
            capacity: 64,
        },
        &link,
    );
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(read(&gpu, &output)[0], 1, "no same-tick cascade introduced");
}

#[test]
fn overflowing_external_birth_capture_keeps_a_deterministic_prefix_without_losing_particles() {
    let Some(gpu) = gpu() else { return };
    let pipeline = DomainSpawnPipeline::new(&gpu.device);
    let count = aestra_runtime::PARTICLE_EVENT_CAPACITY + 76;
    let list = emission(&gpu, count, count);
    let run = || {
        let target = emitter_capacity(&gpu, count, count, 5);
        let events = storage(
            &gpu,
            &vec![0; aestra_gpu::particle_event_words(aestra_runtime::PARTICLE_EVENT_CAPACITY)],
        );
        captured_spawn(&gpu, &pipeline, &target, &list, count, &events);
        assert_eq!(read(&gpu, &target.spawn_counter), [5 + count]);
        assert_eq!(read(&gpu, &target.free_count), [0]);
        let captured = read(&gpu, &events);
        assert_eq!(&captured[..2], &[count, 76]);
        for (i, record) in captured[4..].as_chunks::<8>().0.iter().enumerate() {
            assert_eq!(&record[..2], &[8, 5 + i as u32]);
            assert_eq!(record[2], (i as f32).to_bits());
        }
        (read(&gpu, &target.state), captured)
    };
    let first = run();
    assert_eq!(
        first,
        run(),
        "overflow keeps the same lowest accepted birth ordinals"
    );
    assert_eq!(
        first
            .0
            .as_chunks::<10>()
            .0
            .iter()
            .filter(|state| f32::from_bits(state[7]) > 0.0)
            .count(),
        count as usize
    );
}
