// Included in coupled_tests so this regression drives the production lockstep encoder.
struct EventTrailScene {
    scene: Scene,
    effect: GpuEffectBuffers,
    buffers: [Buffer; 8],
    render_globals: Buffer,
    group: BindGroup,
    trail_pipelines: [ComputePipeline; 3],
    paged_pipelines: [ComputePipeline; 9],
    history: stateful_trails::History,
}

fn event_trail_scene(mixed: bool) -> Option<EventTrailScene> {
    sized_event_trail_scene(mixed, 800)
}

fn sized_event_trail_scene(mixed: bool, stars: u32) -> Option<EventTrailScene> {
    use encase::{ShaderType, StorageBuffer, internal::WriteInto};
    fn encode<T: ShaderType + WriteInto>(value: &T) -> Vec<u8> {
        let mut bytes = Vec::new();
        StorageBuffer::new(&mut bytes).write(value).unwrap();
        bytes
    }
    let mut scene = scene(false)?;
    scene.domains.clear();
    let template = scene.dispatches[0].clone();
    let emitter_count = if mixed { 3 } else { 2 };
    // Large cohorts use coincident real source deaths, not an unsupported fan-out.
    let sources = if stars == 800 { 1 } else { stars / 512 };
    scene.dispatches = vec![
        StatefulDispatch {
            capacity: sources,
            slot_offset: 0,
            emitter_index: 0,
            emitter_count,
            spawn_rate: 60.0 * sources as f32,
            speed: (4.0, 4.0),
            lifetime: (0.1, 0.1),
            gravity: [0.0; 3],
            shape_kind: 0,
            field_follow: None,
            event_mask: 2,
            cutoffs: aestra_runtime::EmissionCutoffs {
                stop_tick: Some(1),
                kill_tick: None,
            },
            ..template.clone()
        },
        StatefulDispatch {
            capacity: stars,
            slot_offset: sources,
            emitter_index: 1,
            emitter_count,
            spawn_rate: 0.0,
            speed: (5.0, 6.0),
            lifetime: (0.5, 0.5),
            gravity: [0.0, -2.0, 0.0],
            shape_kind: 0,
            field_follow: None,
            event_mask: 0,
            ..template
        },
    ];
    scene.states = scene
        .dispatches
        .iter()
        .map(|dispatch| {
            StatefulPersistentState::allocate(
                &scene.device,
                dispatch.capacity,
                STRIDE,
                dispatch.fingerprint(),
                dispatch.event_mask != 0,
            )
        })
        .collect();
    let slots = sources + stars + u32::from(mixed);
    let points = 32;
    let owners = if stars == 800 { 1024 } else { stars * 2 };
    let records = slots + 1 + points * owners;
    let mut emitters = vec![
        GpuEmitter {
            max_particles: sources,
            stateful: 1,
            ..Default::default()
        },
        GpuEmitter {
            slot_offset: sources,
            max_particles: stars,
            stateful: 1,
            trail_offset: slots,
            trail_points: points,
            trail_capacity: owners,
            trail_interval: STATEFUL_TICK_DT,
            trail_lifetime: 0.25,
            ..Default::default()
        },
    ];
    if mixed {
        let mut one = GpuCurve::default();
        one.keys[0] = Vec2::new(0.0, 1.0);
        one.count = 1;
        emitters.push(GpuEmitter {
            slot_offset: sources + stars,
            max_particles: 1,
            burst_count: 1,
            duration: 10.0,
            source_duration: 10.0,
            lifetime: Vec2::splat(10.0),
            size: one,
            opacity: one,
            scale: Vec3::ONE,
            max_scale: 1.0,
            rotation: Vec4::new(0.0, 0.0, 0.0, 1.0),
            ..Default::default()
        });
    }
    let globals = GpuGlobals {
        seed: 7,
        emitter_count,
        total_slots: slots,
        world_from_effect: Mat4::IDENTITY,
        ..Default::default()
    };
    let trail_plan = aestra_gpu::TrailScratchPlan::configure(&mut emitters, records).unwrap();
    let buffer = |bytes: &[u8]| {
        scene.device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("event-to-trail conformance"),
            contents: bytes,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        })
    };
    let buffers = [
        buffer(&encode(&emitters)),
        buffer(&vec![0; records as usize * 48]),
        buffer(&vec![0; slots as usize * 4]),
        buffer(&vec![0; slots as usize * 4]),
        buffer(&vec![0; 256]),
        buffer(&encode(&indirect_draw_commands_with_statistics(&emitters))),
        buffer(&encode(&globals)),
        buffer(&vec![0; trail_plan.aux_words as usize * 4]),
    ];
    scene.render = [
        buffers[1].clone(),
        buffers[2].clone(),
        buffers[5].clone(),
        buffers[4].clone(),
    ];
    let layout = scene.device.create_bind_group_layout(
        "event-to-trail",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                storage_buffer_read_only::<Vec<GpuEmitter>>(false),
                storage_buffer::<Vec<GpuParticle>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer::<Vec<u32>>(false),
                storage_buffer_read_only::<GpuGlobals>(false),
                storage_buffer::<Vec<u32>>(false),
            ),
        ),
    );
    let group = scene.device.create_bind_group(
        "event-to-trail",
        &layout,
        &BindGroupEntries::sequential((
            buffers[0].as_entire_buffer_binding(),
            buffers[1].as_entire_buffer_binding(),
            buffers[2].as_entire_buffer_binding(),
            buffers[3].as_entire_buffer_binding(),
            buffers[4].as_entire_buffer_binding(),
            buffers[5].as_entire_buffer_binding(),
            buffers[6].as_entire_buffer_binding(),
            buffers[7].as_entire_buffer_binding(),
        )),
    );
    let module = scene
        .device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("event-to-trail"),
            source: wgpu::ShaderSource::Wgsl(
                aestra_gpu::shader::compile_wesl(
                    "package::event_trails",
                    SIMULATION_WESL,
                    &[
                        "reset",
                        "simulate",
                        "update_trails",
                        "sort_trail_page",
                        "merge_trail_pages",
                        "present_trail_heads",
                        "reserve_trail_owners",
                        "scan_trail_births",
                        "scan_trail_birth_pages",
                        "update_trail_owners",
                        "bound_trail_page",
                        "finish_trail_pages",
                    ],
                )
                .unwrap()
                .wgsl
                .into(),
            ),
        });
    let pipeline_layout = scene
        .device
        .create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
    let pipeline = |entry: &str| {
        scene
            .device
            .create_compute_pipeline(&RawComputePipelineDescriptor {
                label: None,
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
    };
    let trail_pipelines = [
        pipeline("reset"),
        pipeline("simulate"),
        pipeline("update_trails"),
    ];
    let paged_pipelines = aestra_gpu::PAGED_TRAIL_ENTRY_POINTS.map(pipeline);
    let render_globals = buffer(&vec![0; 256]);
    let mut context = trail_checkpoints::TrailContext::default();
    for (index, value) in Mat4::IDENTITY.to_cols_array().iter().enumerate() {
        context.key[6 + index] = value.to_bits();
    }
    let effect = GpuEffectBuffers {
        emitters: default(),
        renderers: default(),
        particles: default(),
        alive: default(),
        dead: default(),
        counters: default(),
        indirect: default(),
        globals: default(),
        aux: default(),
        render_globals: default(),
        workgroups: slots.div_ceil(64),
        has_ribbons: false,
        has_trails: true,
        ribbon_workgroups: 0,
        trail_workgroups: emitter_count,
        trail_plan,
        total_slots: slots,
        simulation_time: 0.0,
        seek_quality: SeekQuality::Exact,
        history_epoch: 0,
        statistics_token: 0,
        checkpoint_context: Arc::new(context),
        trail_roots: vec![(1, slots)],
        simulation_state: default(),
        stateful_dispatch: scene.dispatches.clone(),
        stateful_only: !mixed,
        event_links: vec![aestra_runtime::CompiledEventLink {
            source: 0,
            target: 1,
            trigger: aestra_core::EventTrigger::OnDeath,
            count: stars / sources,
            inherit: 0.0,
        }],
        routed: false,
        particle_outputs: Vec::new(),
        host_events: default(),
        physics: aestra_gpu::pack_physics_scene(&Default::default()).into(),
    };
    Some(EventTrailScene {
        scene,
        effect,
        buffers,
        render_globals,
        group,
        trail_pipelines,
        paged_pipelines,
        history: default(),
    })
}

impl EventTrailScene {
    fn frame(&mut self, target: u32, budget: u32) {
        let scene = &mut self.scene;
        self.history.sync(&self.effect, &scene.states);
        let paged = self.effect.trail_plan.paged().then(|| {
            paged_trails::Dispatch::new(
                &scene.device,
                self.effect.trail_plan,
                self.paged_pipelines.each_ref(),
                self.effect.trail_workgroups,
            )
        });
        let mut observer = stateful_trails::Observer {
            history: &mut self.history,
            effect: &self.effect,
            group: &self.group,
            reset: &self.trail_pipelines[0],
            simulate: &self.trail_pipelines[1],
            update: &self.trail_pipelines[2],
            paged: paged.as_ref(),
            ribbons: None,
            globals: &self.buffers[6],
            render_globals: &self.render_globals,
            buffers: [
                &self.buffers[1],
                &self.buffers[2],
                &self.buffers[3],
                &self.buffers[4],
                &self.buffers[5],
                &self.buffers[7],
            ],
            memory_budget: trail_checkpoints::MEMORY_LIMIT,
        };
        let mut encoder = scene.device.create_command_encoder(&Default::default());
        let [particles, alive, indirect, counters] = &scene.render;
        run_coupled_stateful(
            &scene.device,
            &mut encoder,
            (
                &scene.pipelines[0],
                &scene.pipelines[1],
                &scene.pipelines[2],
                None,
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
            &self.effect.event_links,
            &RouteWiring::default(),
            &StatefulRenderBuffers {
                particles,
                alive,
                indirect,
                counters,
                world: &scene.no_world,
                physics: &scene.no_world,
            },
            (target as f32 + 0.5) * STATEFUL_TICK_DT,
            budget,
            Some(&mut observer),
        );
        scene.queue.submit([encoder.finish()]);
        scene
            .device
            .wgpu_device()
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: Some(std::time::Duration::from_secs(60)),
            })
            .unwrap();
    }

    fn trail_state(&self) -> (Vec<u8>, Vec<u8>) {
        let root = self.effect.total_slots as usize;
        let records = self.buffers[1].size() as usize / 48;
        (
            read_back(&self.scene.device, &self.scene.queue, &self.buffers[1])[root * 48..]
                .to_vec(),
            read_back(&self.scene.device, &self.scene.queue, &self.buffers[7])
                [root * 12..records * 12]
                .to_vec(),
        )
    }

    fn usage(&self) -> [u32; 4] {
        let bytes = read_back(&self.scene.device, &self.scene.queue, &self.buffers[4]);
        [0, 1, 2, 5].map(|index| {
            u32::from_le_bytes(bytes[(8 + index) * 4..(9 + index) * 4].try_into().unwrap())
        })
    }
}

#[test]
fn paged_event_born_trails_observe_large_live_cohorts_and_retire_without_loss() {
    for stars in [2048, 8192] {
        let Some(mut test) = sized_event_trail_scene(true, stars) else {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "native GPU required"
            );
            return;
        };
        for tick in 0..=30 {
            test.frame(tick, 4);
        }
        assert_eq!(test.usage(), [stars, 0, 0, 0]);
        let forward = test.trail_state();
        let mut owners = 0;
        for (index, record) in forward.0.as_chunks::<48>().0.iter().enumerate() {
            if index > 0
                && (index - 1) % 32 == 0
                && u32::from_le_bytes(record[40..44].try_into().unwrap()) & 0xffff != 0
            {
                owners += 1;
                let base = index * 12;
                assert!(u32::from_le_bytes(forward.1[base + 4..base + 8].try_into().unwrap()) > 2);
            }
        }
        assert_eq!(owners, stars);
        test.frame(30, 4);
        assert!(
            test.trail_state() == forward,
            "paused paged histories changed"
        );
        while test.history.tick != Some(40) {
            test.frame(40, 4);
        }
        assert_eq!(test.usage(), [stars, stars, 0, 0]);
        while test.history.tick != Some(60) {
            test.frame(60, 4);
        }
        assert_eq!(test.usage(), [0, 0, 0, 0]);
        test.effect.history_epoch = 1;
        test.scene
            .queue
            .write_buffer(&test.buffers[6], 24, &1u32.to_le_bytes());
        test.frame(0, 4);
        while test.history.tick != Some(30) {
            test.frame(30, 4);
        }
        let mut restarted = test.trail_state();
        restarted.1[..4].copy_from_slice(&forward.1[..4]);
        assert!(
            restarted == forward,
            "paged allocation changed across batched restart"
        );
    }
}

#[test]
fn event_born_800_star_trails_observe_ticks_retire_and_restore_together() {
    for mixed in [false, true] {
        let Some(mut test) = event_trail_scene(mixed) else {
            assert!(
                std::env::var_os("AESTRA_REQUIRE_GPU_CONFORMANCE").is_none(),
                "native GPU required"
            );
            return;
        };
        for tick in 0..=30 {
            test.frame(tick, 4);
        }
        let forward = test.trail_state();
        assert_eq!(test.usage(), [800, 0, 0, 0]);
        let live = read_back(&test.scene.device, &test.scene.queue, &test.buffers[4]);
        assert_eq!(
            u32::from_le_bytes(live[..4].try_into().unwrap()),
            800 + u32::from(mixed)
        );
        // Every owner has a non-degenerate trail, including births from the event spawner.
        let count = forward
            .0
            .as_chunks::<48>()
            .0
            .iter()
            .enumerate()
            .filter(|(index, record)| {
                *index > 0
                    && (*index - 1) % 32 == 0
                    && u32::from_le_bytes(record[40..44].try_into().unwrap()) & 0xffff != 0
            })
            .count();
        assert_eq!(count, 800);
        for owner in 0..800 {
            let base = (1 + owner * 32) * 12;
            assert!(u32::from_le_bytes(forward.1[base + 4..base + 8].try_into().unwrap()) > 2);
        }
        test.frame(30, 4);
        assert_eq!(test.trail_state(), forward, "pause must not append history");
        // One bounded frame cannot process the entire seek. Draw the processed time, not target.
        test.frame(70, 4);
        assert_eq!(test.history.tick, Some(34));
        let render = read_back(&test.scene.device, &test.scene.queue, &test.render_globals);
        assert_eq!(
            f32::from_le_bytes(render[64..68].try_into().unwrap()),
            34.0 * STATEFUL_TICK_DT
        );
        test.frame(40, 8);
        assert_eq!(
            test.usage(),
            [800, 800, 0, 0],
            "dead stars leave unexpired retired tails"
        );
        // Change discontinuity epoch exactly as a host seek does.
        test.effect.history_epoch = 1;
        test.scene
            .queue
            .write_buffer(&test.buffers[6], 24, &1u32.to_le_bytes());
        test.frame(25, 8);
        assert_eq!(test.history.tick, Some(25));
        test.frame(30, 8);
        let mut restored = test.trail_state();
        // Only the restored header's epoch differs; all head/ring/sample identities match.
        restored.1[..4].copy_from_slice(&forward.1[..4]);
        assert_eq!(
            restored, forward,
            "joint checkpoint restore must reproduce live trails"
        );
        while test.history.tick != Some(60) {
            test.frame(60, 8);
        }
        assert_eq!(
            test.usage(),
            [0, 0, 0, 0],
            "retired histories eventually expire"
        );
        // A fresh restart followed by slow/catch-up frames must observe the same
        // intermediate trajectory as one tick per frame, not only the final heads.
        test.effect.history_epoch = 2;
        test.scene
            .queue
            .write_buffer(&test.buffers[6], 24, &2u32.to_le_bytes());
        test.frame(0, 4);
        while test.history.tick != Some(30) {
            let previous = test.history.tick.unwrap();
            test.frame(30, 4);
            assert!(test.history.tick.unwrap() - previous <= 4);
        }
        let mut batched = test.trail_state();
        batched.1[..4].copy_from_slice(&forward.1[..4]);
        assert!(
            batched == forward,
            "batched playback must observe every event-born trajectory"
        );
    }
}
