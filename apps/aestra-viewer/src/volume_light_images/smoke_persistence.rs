//! F8.1A bounded saved sprite-smoke persistence; screenshot readback is test-only.
use super::*;

fn persistence_project(tier: &str) -> Arc<aestra_bevy::CompiledEffectProject> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
    let effect = EffectAsset::from_ron(
        &fs::read_to_string(root.join("effects/fireworks_smoke_persistence.aestra.ron")).unwrap(),
    )
    .unwrap();
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

#[test]
#[ignore = "native F8.1A accumulating/persistent sprite-smoke image gate; run alone"]
fn saved_smoke_persists_drifts_receives_later_breaks_and_drains() {
    let root = std::env::var_os("AESTRA_SMOKE_PERSISTENCE_REPORTS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/fireworks-f8/smoke-persistence")
        });
    for tier in ["high", "medium", "low"] {
        let directory = root.join(tier);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("report.json"), br#"{"accepted":false}"#).unwrap();
        let (mut app, owner, target) =
            headless_project(tier, true, true, persistence_project(tier));
        let empty = capture(&mut app, &target);
        let mut images = vec![("empty", empty.clone())];
        for (name, frame) in [
            ("accumulating", 120),
            ("emission-ended", 246),
            ("persistent-tail", 420),
        ] {
            advance(&mut app, owner, frame);
            images.push((name, capture(&mut app, &target)));
        }
        let tail = images.last().unwrap().1.clone();
        let tail_visibility = delta(&empty, &tail);
        let repeat = capture(&mut app, &target);
        let frozen_repeat = delta(&tail, &repeat);
        images.push(("tail-repeat", repeat));
        let artifact = app
            .world()
            .get::<EffectPlayer>(owner)
            .unwrap()
            .effect()
            .clone();
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
        let density_off = capture(&mut app, &target);
        let density_off_delta = delta(&empty, &density_off);
        images.push(("density-off", density_off));
        app.world_mut()
            .get_mut::<PresentedEffect>(owner)
            .unwrap()
            .unbind_material(material);
        let density_restored = capture(&mut app, &target);
        let density_restore_delta = delta(&tail, &density_restored);
        images.push(("density-restored", density_restored));
        let cap = policy(tier).particle.max_lights;
        // enabled=false retires one-shot requests by design; a reversible mute keeps them alive.
        let maximum_lumens = app.world().resource::<TransientLightSettings>().max_lumens;
        let mut illumination = Vec::new();
        for (name, frame) in [("late-red", 492), ("late-blue", 612)] {
            advance(&mut app, owner, frame);
            let both = capture(&mut app, &target);
            assert_native_transport(&app);
            app.world_mut()
                .resource_mut::<ParticleLightGpuSettings>()
                .max_lights = 0;
            let representative = capture(&mut app, &target);
            app.world_mut()
                .resource_mut::<TransientLightSettings>()
                .max_lumens = 0.0;
            let off = capture(&mut app, &target);
            app.world_mut()
                .resource_mut::<ParticleLightGpuSettings>()
                .max_lights = cap;
            let selected = capture(&mut app, &target);
            app.world_mut()
                .resource_mut::<TransientLightSettings>()
                .max_lumens = maximum_lumens;
            let restored = capture(&mut app, &target);
            illumination.push(serde_json::json!({"phase":name,"representative":delta(&off,&representative),"selected":delta(&off,&selected),"both":delta(&off,&both),"restored":delta(&both,&restored)}));
            // Static labels avoid changing earlier gates' files.
            for (suffix, image) in [
                ("both", both),
                ("representative", representative),
                ("selected", selected),
                ("off", off),
                ("restored", restored),
            ] {
                image
                    .save(directory.join(format!("{name}-{suffix}.png")))
                    .unwrap();
            }
        }
        advance(&mut app, owner, 900);
        images.push(("fading", capture(&mut app, &target)));
        advance(&mut app, owner, 1080);
        let drained = capture(&mut app, &target);
        let drained_delta = delta(&empty, &drained);
        images.push(("drained", drained));
        app.world_mut().despawn(owner);
        let retired = capture(&mut app, &target);
        let retired_delta = delta(&empty, &retired);
        images.push(("retired", retired));
        let sort = app
            .world()
            .resource::<aestra_bevy::gpu::GpuAlphaSortStatistics>()
            .snapshot();
        assert_eq!((sort.pairs, sort.owned_buffer_bytes), (0, 0));
        let accepted = tail_visibility.0 >= 100
            && frozen_repeat.1 <= 1
            && density_off_delta.1 <= 1
            && density_restore_delta.1 <= 1
            && drained_delta.1 <= 1
            && retired_delta.1 <= 1
            && illumination.iter().all(|r| {
                r["representative"][0].as_u64().unwrap() >= 100
                    && r["selected"][0].as_u64().unwrap() >= 100
                    && r["restored"][1].as_u64().unwrap() <= 1
            });
        for (name, image) in &images {
            image.save(directory.join(format!("{name}.png"))).unwrap();
        }
        let report = serde_json::json!({"slice":"F8.1A","accepted":accepted,"tier":tier,"tail_visibility":tail_visibility,"frozen_repeat":frozen_repeat,"density_off":density_off_delta,"density_restored":density_restore_delta,"illumination":illumination,"drained":drained_delta,"retired":retired_delta,"runtime_selected_position_readback":false,"smoke_capacity":96,"retired_sort_bytes":sort.owned_buffer_bytes,"limitations":"bounded 88-birth procedural sprite prototype; same smoke density at all tiers; fixed acceleration approximation, not physical wind/fluid; no cross-draw/full-show/finale/art acceptance"});
        fs::write(
            directory.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
        assert!(accepted, "persistence image gate failed: {report}");
    }
}
