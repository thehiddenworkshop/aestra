//! F8.1C3 authored-show controls; public hosting stays in aestra-bevy/examples/fireworks.
use super::*;
use aestra_bevy::gpu::{GpuAlphaSortStatistics, GpuParticleStatistics};
use serde_json::json;

fn density(app: &mut App, restore: bool) {
    let mut query = app.world_mut().query::<&mut PresentedEffect>();
    for mut presented in query.iter_mut(app.world_mut()) {
        let effect = presented.instance.effect().clone();
        for material in &effect.material_instances {
            if ![
                "a3574a00-0000-4000-8000-000000f81200",
                "a3574a00-0000-4000-8000-000000f81700",
            ]
            .contains(&material.program.id().to_string().as_str())
            {
                continue;
            }
            if restore {
                presented.unbind_material(material.id);
                continue;
            }
            let emitter = effect
                .emitters
                .iter()
                .find(|e| e.renderers.iter().any(|r| r.material == material.id))
                .unwrap();
            let mut binding = presented
                .material_binding_for_emitter(material.id, emitter.source)
                .unwrap()
                .clone();
            let parameter = binding
                .program()
                .reflection
                .parameters
                .iter()
                .find(|p| p.name == "Density")
                .unwrap()
                .id;
            binding
                .set_value(parameter, aestra_bevy::material::MaterialValue::Float(0.0))
                .unwrap();
            presented.bind_material(material.id, binding);
        }
    }
}

#[test]
#[ignore = "native F8.1C3 authored persistent-smoke show controls; run alone"]
fn authored_persistent_smoke_show_lights_overlaps_and_drains() {
    qualify_show("AESTRA_PERSISTENT_SHOW_IMAGES", false);
}

#[test]
#[ignore = "native wispy art draft lifecycle/light/density controls; run alone"]
fn authored_wispy_smoke_show_lights_overlaps_and_drains() {
    qualify_show("AESTRA_WISPY_SHOW_IMAGES", true);
}

fn qualify_show(output_env: &str, wispy: bool) {
    let root =
        PathBuf::from(std::env::var_os(output_env).expect("fresh absolute output directory"));
    assert!(root.is_absolute());
    for tier in ["high", "medium", "low"] {
        let directory = root.join(tier);
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("report.json"), b"{\"accepted\":false}").unwrap();
        let compiled = if wispy {
            let assets = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/test");
            let effect = EffectAsset::from_ron(
                &fs::read_to_string(assets.join("effects/fireworks_show_wispy_smoke.aestra.ron"))
                    .unwrap(),
            )
            .unwrap();
            let resolved = aestra_project::ProjectAssetIndex::scan(assets)
                .resolve_effect_project(&effect)
                .unwrap();
            Arc::new(
                EffectCompiler::default()
                    .with_tier(aestra_bevy::QualityTier::preset(tier).unwrap())
                    .compile_resolved_project(&resolved)
                    .unwrap(),
            )
        } else {
            live_costs::fixture(tier, live_costs::Case::PersistentShow { draw: true })
        };
        let (mut app, owner, target) = headless_project(tier, true, true, compiled);
        *app.world_mut().get_mut::<Transform>(owner).unwrap() = Transform::IDENTITY;
        let mut cameras = app
            .world_mut()
            .query_filtered::<&mut Transform, With<Camera3d>>();
        for mut camera in cameras.iter_mut(app.world_mut()) {
            *camera = crate::fireworks_show::camera(crate::FireworksCamera::Audience);
        }
        let mut policy = LightingQualityPolicy::preset(tier).unwrap();
        policy.particle.enabled = false;
        policy.particle.max_lights = 0;
        policy.apply(app.world_mut()).unwrap();
        let empty = capture(&mut app, &target);
        let mut lights = Vec::new();
        let mut path_admission = Vec::new();
        // At 9.3s three simultaneous authored launches have broken while older smoke persists.
        // Sample inside each authored pulse, not at its async birth/delivery boundary.
        for (phase, frame) in [("overlap-break", 558), ("late-break", 1104)] {
            advance(&mut app, owner, frame);
            let both = capture(&mut app, &target);
            let cap = app.world().resource::<TransientLightSettings>().max_lumens;
            app.world_mut()
                .resource_mut::<TransientLightSettings>()
                .max_lumens = 0.0;
            let off = capture(&mut app, &target);
            app.world_mut()
                .resource_mut::<TransientLightSettings>()
                .max_lumens = cap;
            let restored = capture(&mut app, &target);
            let difference = delta(&off, &both);
            let repeat = delta(&both, &restored);
            let active = app.world().resource::<TransientLightStatistics>().active;
            for (suffix, image) in [("lit", both), ("off", off), ("restored", restored)] {
                image
                    .save(directory.join(format!("{phase}-{suffix}.png")))
                    .unwrap();
            }
            lights.push(json!({"phase":phase,"light_difference":difference,"restored":repeat,"active":active}));
            if wispy {
                let mut query = app
                    .world_mut()
                    .query::<(&PresentedEffect, &aestra_bevy::gpu::GpuEventLinkStatistics)>();
                let mut admitted = 0;
                for (presented, stats) in query.iter(app.world()) {
                    let Some((index, _)) = presented
                        .instance
                        .effect()
                        .event_links
                        .iter()
                        .enumerate()
                        .find(|(_, link)| link.trigger.distance_settings().is_some())
                    else {
                        continue;
                    };
                    assert!(stats.readback_samples > 0);
                    assert_eq!(stats.source_overflow, 0);
                    let link = &stats.links[index];
                    assert_eq!(
                        link.accepted, 43,
                        "one deposited puff for each full unit of rocket travel"
                    );
                    assert_eq!(link.captured_demand, link.accepted);
                    assert_eq!((link.expansion_omitted, link.destination_rejected), (0, 0));
                    path_admission.push(json!({"phase":phase,"effect":presented.instance.effect().name,"accepted":link.accepted,"source_overflow":stats.source_overflow}));
                    admitted += 1;
                }
                assert!(
                    admitted >= 7,
                    "observe overlapping authored rocket wakes, not an empty query"
                );
            }
        }
        advance(&mut app, owner, 1800); // 30s: normal show already ended at 26s.
        let tail = capture(&mut app, &target);
        let frozen = capture(&mut app, &target);
        let visibility = delta(&empty, &tail);
        let repeat = delta(&tail, &frozen);
        let mut stats = app
            .world_mut()
            .query::<(&PresentedEffect, &GpuParticleStatistics)>();
        let tail_counts: Vec<_> = stats
            .iter(app.world())
            .filter_map(|(p, s)| {
                s.observation(&p.instance)
                    .map(|(t, c)| json!({"time":t,"counts":c}))
            })
            .collect();
        density(&mut app, false);
        let hidden = capture(&mut app, &target);
        let density_off = delta(&empty, &hidden);
        density(&mut app, true);
        let restored = capture(&mut app, &target);
        let density_restore = delta(&tail, &restored);
        advance(&mut app, owner, 1980); // 33s: smoke naturally dead, last clip still retained to 35s.
        let drained = capture(&mut app, &target);
        let natural = delta(&empty, &drained);
        let mut stats = app
            .world_mut()
            .query::<(&PresentedEffect, &GpuParticleStatistics)>();
        let final_counts: Vec<_> = stats
            .iter(app.world())
            .filter_map(|(p, s)| s.observation(&p.instance).map(|(_, c)| c.to_vec()))
            .collect();
        // Do not accept an empty observation set after accidental early clip retirement.
        // The four late clips still own their effects at 33s, before their 34/35s ends.
        let zero = final_counts.len() == 4
            && final_counts.iter().all(|counts| !counts.is_empty())
            && final_counts.iter().flatten().all(|count| *count == 0);
        advance(&mut app, owner, 2220);
        let ended = capture(&mut app, &target);
        let sort = app.world().resource::<GpuAlphaSortStatistics>().snapshot();
        let retired = (sort.pairs, sort.owned_buffer_bytes);
        for (name, image) in [
            ("empty", empty),
            ("tail", tail),
            ("frozen", frozen),
            ("density-off", hidden),
            ("density-restored", restored),
            ("natural-drain", drained),
            ("ended", ended),
        ] {
            image.save(directory.join(format!("{name}.png"))).unwrap();
        }
        assert_native_transport(&app);
        let accepted = visibility.0 >= 100
            && repeat.1 <= 1
            && density_off.1 <= 1
            && density_restore.1 <= 1
            && natural.1 <= 1
            && zero
            && retired == (0, 0)
            && lights.iter().all(|r| {
                r["light_difference"][0].as_u64().unwrap() >= 100
                    && r["restored"][1].as_u64().unwrap() <= 1
            });
        let report = json!({"slice":if wispy {"F8.1C4B distance smoke"} else {"F8.1C3"},"tier":tier,"accepted":accepted,"light_sample_root_times":[9.3,18.4],"tail_visibility":visibility,"frozen_repeat":repeat,"density_off":density_off,"density_restored":density_restore,"natural_drained_pixels":natural,"natural_zero":zero,"tail_populations":tail_counts,"final_populations":final_counts,"retired_sort":retired,"illumination":lights,"scope":"saved 13-clip representative-only show candidate, LDR public audience camera; no selected-star output invention, cross-draw/AAA art or whole-game cost approval"});
        fs::write(
            directory.join("report.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        if wispy {
            fs::write(
                directory.join("distance-admission.json"),
                serde_json::to_vec_pretty(&path_admission).unwrap(),
            )
            .unwrap();
        }
        println!("{report}");
        assert!(accepted, "authored smoke show gate failed: {report}");
    }
}
