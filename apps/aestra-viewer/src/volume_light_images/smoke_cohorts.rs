//! F8.1B: genuine GPU death-link cohorts in one shared alpha draw; test-only screenshots.
use super::*;
use std::path::Path;

#[derive(Resource, Default)]
struct BirthCues(Vec<aestra_bevy::AestraOutputEvent>);

fn collect_birth_cues(
    mut outputs: MessageReader<aestra_bevy::AestraOutputEvent>,
    mut cues: ResMut<BirthCues>,
) {
    cues.0.extend(
        outputs
            .read()
            .filter(|cue| matches!(cue.event.kind.as_str(), "red_break" | "blue_break"))
            .cloned(),
    );
}

fn cohort_project(tier: &str, smoke_links: u8) -> Arc<aestra_bevy::CompiledEffectProject> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let mut effect = EffectAsset::from_ron(
        &fs::read_to_string(root.join("effects/fireworks_smoke_cohorts.aestra.ron")).unwrap(),
    )
    .unwrap();
    let smoke = effect.emitters[0].id;
    let red = effect.emitters[3].id;
    let blue = effect.emitters[4].id;
    // Remove only smoke links: actual star outputs/lights and emitter order/seed stay unchanged.
    effect.events.retain(|link| {
        link.target != smoke
            || (link.source == red && smoke_links & 1 != 0)
            || (link.source == blue && smoke_links & 2 != 0)
    });
    let resolved = aestra_project::ProjectAssetIndex::scan(root)
        .resolve_effect_project(&effect)
        .unwrap();
    Arc::new(
        EffectCompiler::default()
            .with_tier(aestra_bevy::QualityTier::preset(tier).unwrap())
            .compile_resolved_project(&resolved)
            .unwrap(),
    )
}

fn light_controls(
    app: &mut App,
    target: &Handle<Image>,
    tier: &str,
    directory: &Path,
    label: &str,
) -> serde_json::Value {
    let both = capture(app, target);
    assert_native_transport(app);
    let stats = app.world().resource::<TransientLightStatistics>();
    let representative_admission = serde_json::json!({"accepted":stats.accepted,"active":stats.active,"expired":stats.expired,"invalid":stats.invalid,"binding_invalid":stats.binding_invalid,"budget_dropped":stats.budget_dropped});
    let cap = policy(tier).particle.max_lights;
    let lumens = app.world().resource::<TransientLightSettings>().max_lumens;
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_lights = 0;
    let representative = capture(app, target);
    app.world_mut()
        .resource_mut::<TransientLightSettings>()
        .max_lumens = 0.0;
    let off = capture(app, target);
    app.world_mut()
        .resource_mut::<ParticleLightGpuSettings>()
        .max_lights = cap;
    let selected = capture(app, target);
    app.world_mut()
        .resource_mut::<TransientLightSettings>()
        .max_lumens = lumens;
    let restored = capture(app, target);
    let result = serde_json::json!({"phase":label,"representative":delta(&off,&representative),"selected":delta(&off,&selected),"restored":delta(&both,&restored),"representative_admission":representative_admission});
    for (suffix, image) in [
        ("both", both),
        ("representative", representative),
        ("off", off),
        ("selected", selected),
        ("restored", restored),
    ] {
        image
            .save(directory.join(format!("{label}-{suffix}.png")))
            .unwrap();
    }
    result
}

fn differing_pixels(a: &RgbaImage, b: &RgbaImage) -> usize {
    a.pixels()
        .zip(b.pixels())
        .filter(|(a, b)| (0..3).any(|c| a[c].abs_diff(b[c]) > 3))
        .count()
}

#[test]
#[ignore = "native F8.1B death-linked smoke cohort/lighting gate; run alone"]
fn saved_shell_deaths_birth_overlapping_smoke_that_receives_later_break_light() {
    let root = std::env::var_os("AESTRA_SMOKE_COHORT_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/fireworks-f8/smoke-cohorts")
        });
    for tier in ["high", "medium", "low"] {
        let directory = root.join(tier);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("report.json"), br#"{"accepted":false}"#).unwrap();
        let (mut app, owner, target) = headless_project(tier, true, true, cohort_project(tier, 3));
        app.init_resource::<BirthCues>()
            .add_systems(Update, collect_birth_cues);
        let empty = capture(&mut app, &target);
        advance(&mut app, owner, 110);
        let before = capture(&mut app, &target);
        let before_delta = delta(&empty, &before);
        advance(&mut app, owner, 144);
        let red = light_controls(&mut app, &target, tier, &directory, "first-break");
        advance(&mut app, owner, 210);
        let first = capture(&mut app, &target);
        let first_visibility = delta(&empty, &first);
        advance(&mut app, owner, 264);
        let blue = light_controls(&mut app, &target, tier, &directory, "second-break");
        advance(&mut app, owner, 420);
        let tail = capture(&mut app, &target);
        let tail_visibility = delta(&empty, &tail);
        let links = app
            .world()
            .get::<aestra_bevy::gpu::GpuEventLinkStatistics>(owner)
            .unwrap();
        let births_pass = links.readback_samples > 0
            && links.source_overflow == 0
            && links.links.len() == 4
            && links.links.iter().all(|link| {
                link.captured_demand == 32
                    && link.accepted == 32
                    && link.expansion_omitted == 0
                    && link.destination_rejected == 0
            });
        let birth_admission = serde_json::json!({"source_overflow":links.source_overflow,"links":links.links.iter().map(|link| serde_json::json!({"captured_demand":link.captured_demand,"accepted":link.accepted,"expansion_omitted":link.expansion_omitted,"destination_rejected":link.destination_rejected})).collect::<Vec<_>>()});
        let repeat = capture(&mut app, &target);
        let repeat_delta = delta(&tail, &repeat);
        let artifact = app
            .world()
            .get::<EffectPlayer>(owner)
            .unwrap()
            .effect()
            .clone();
        let cues = &app.world().resource::<BirthCues>().0;
        assert_eq!(
            cues.len(),
            2,
            "one FirstPerTick cue for each accepted child cohort"
        );
        let epoch = app
            .world()
            .get::<EffectPlayer>(owner)
            .unwrap()
            .instance()
            .history_epoch();
        for (cue, source) in cues.iter().zip([1, 2]) {
            assert_eq!(cue.effect, owner);
            assert!(cue.clip_path.is_empty());
            assert_eq!(cue.playback_epoch, Some(epoch));
            assert_eq!(cue.event.origin, aestra_bevy::EventOrigin::Emitter(source));
            assert_eq!(cue.event.magnitude, 32.0);
            let spatial = cue.particle.as_ref().expect("native child source context");
            assert_eq!(spatial.source_effect, artifact.source);
            assert!((spatial.root_time_seconds - cue.event.tick as f32 / 60.0).abs() < 1e-5);
            let local: [f32; 3] = cue.event.value.as_slice().try_into().unwrap();
            let world = spatial.world_position.expect("real child world position");
            for axis in 0..3 {
                assert!((world[axis] - local[axis] * 0.1).abs() < 1e-5);
            }
        }
        let material = artifact.emitters[0].renderers[0].material;
        {
            let mut presented = app.world_mut().get_mut::<PresentedEffect>(owner).unwrap();
            let mut binding = presented
                .material_binding_for_emitter(material, artifact.emitters[0].source)
                .unwrap()
                .clone();
            let density = binding
                .program()
                .reflection
                .parameters
                .iter()
                .find(|p| p.name == "Density")
                .unwrap()
                .id;
            binding
                .set_value(density, aestra_bevy::material::MaterialValue::Float(0.0))
                .unwrap();
            presented.bind_material(material, binding);
        }
        let off = capture(&mut app, &target);
        let density_off = delta(&empty, &off);
        app.world_mut()
            .get_mut::<PresentedEffect>(owner)
            .unwrap()
            .unbind_material(material);
        let restored = capture(&mut app, &target);
        let density_restored = delta(&tail, &restored);
        advance(&mut app, owner, 900);
        let fading = capture(&mut app, &target);
        advance(&mut app, owner, 1080);
        let drained = capture(&mut app, &target);
        let drained_delta = delta(&empty, &drained);
        app.world_mut().despawn(owner);
        let retired = capture(&mut app, &target);
        let retired_delta = delta(&empty, &retired);
        let sort = app
            .world()
            .resource::<aestra_bevy::gpu::GpuAlphaSortStatistics>()
            .snapshot();
        assert_eq!((sort.pairs, sort.owned_buffer_bytes), (0, 0));
        for (name, image) in [
            ("empty", &empty),
            ("before-break", &before),
            ("first-cohort", &first),
            ("persistent-cohorts", &tail),
            ("tail-repeat", &repeat),
            ("density-off", &off),
            ("density-restored", &restored),
            ("fading", &fading),
            ("drained", &drained),
            ("retired", &retired),
        ] {
            image.save(directory.join(format!("{name}.png"))).unwrap();
        }
        drop(app);
        let mut cohort_controls = Vec::new();
        let mut older_light = serde_json::Value::Null;
        for (mask, name) in [
            (1, "older-only"),
            (2, "younger-only"),
            (0, "no-smoke-links"),
        ] {
            let (mut control, owner, target) =
                headless_project(tier, true, true, cohort_project(tier, mask));
            if mask == 1 {
                advance(&mut control, owner, 264);
                older_light = light_controls(
                    &mut control,
                    &target,
                    tier,
                    &directory,
                    "older-at-second-break",
                );
            }
            advance(&mut control, owner, 420);
            let image = capture(&mut control, &target);
            let visibility = delta(&empty, &image);
            let difference = differing_pixels(&tail, &image);
            image.save(directory.join(format!("{name}.png"))).unwrap();
            cohort_controls.push(serde_json::json!({"name":name,"visibility":visibility,"difference_from_both_pixels":difference}));
        }
        let lights_pass = [&red, &blue, &older_light].iter().all(|r| {
            r["representative"][0].as_u64().unwrap() >= 100
                && r["selected"][0].as_u64().unwrap() >= 100
                && r["restored"][1].as_u64().unwrap() <= 1
                && r["representative_admission"]["active"] == 1
                && ["expired", "invalid", "binding_invalid", "budget_dropped"]
                    .iter()
                    .all(|key| r["representative_admission"][key] == 0)
        });
        let cohorts_pass = cohort_controls[..2].iter().all(|r| {
            r["visibility"][0].as_u64().unwrap() >= 100
                && r["difference_from_both_pixels"].as_u64().unwrap() >= 100
        }) && cohort_controls[2]["visibility"][1].as_u64().unwrap() <= 1;
        let accepted = lights_pass
            && births_pass
            && cohorts_pass
            && first_visibility.0 >= 100
            && tail_visibility.0 >= 100
            && [
                before_delta,
                repeat_delta,
                density_off,
                density_restored,
                drained_delta,
                retired_delta,
            ]
            .iter()
            .all(|d| d.1 <= 1);
        let report = serde_json::json!({"slice":"F8.1B","tier":tier,"accepted":accepted,"before_break":before_delta,"first_visibility":first_visibility,"tail_visibility":tail_visibility,"repeat":repeat_delta,"density_off":density_off,"density_restored":density_restored,"drained":drained_delta,"retired":retired_delta,"retired_sort_bytes":sort.owned_buffer_bytes,"lights":[red,blue,older_light],"birth_admission":birth_admission,"cohort_controls":cohort_controls,"smoke_capacity":96,"authored_birth_demand_bound":64,"runtime_selected_position_readback":false,"limitations":"bounded two real ballistic shells; one shared smoke draw; async counters prove accepted link births, images prove overlap/light controls, not instantaneous GPU alive-count, cross-draw sorting, full-show cost or AAA art acceptance"});
        fs::write(
            directory.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
        assert!(accepted, "shell-born cohort gate failed: {report}");
    }
}
