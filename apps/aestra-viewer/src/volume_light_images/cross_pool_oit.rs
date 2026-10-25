//! Experimental native OIT oracle; production smoke/mixed-draw acceptance remains separate.
use super::*;
use aestra_bevy::{ExperimentalOitSettings, ExperimentalOitStatistics, ExperimentalOitStatus};

#[path = "cross_pool_saturation.rs"]
mod saturation;

#[path = "cross_pool_depth.rs"]
mod depth;

#[path = "cross_pool_hdr.rs"]
mod hdr;

#[path = "cross_pool_lighting.rs"]
mod lighting;

fn enable(app: &mut App) -> Entity {
    let mut cameras = app.world_mut().query_filtered::<Entity, With<Camera3d>>();
    let camera = cameras.iter(app.world()).next().unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert((Msaa::Off, ExperimentalOitSettings::default()));
    camera
}

#[test]
#[ignore = "native opt-in fragment-list OIT correctness and bounded allocation probe; run alone"]
fn fragment_list_probe_composes_interleaved_and_moving_pools() {
    let root = root("fragment-list");
    let (project, seed, z) = interleaved();
    let moving = sheet_with(true, 1, EmitterShape::Point, 2.0);
    let mut cases = Vec::new();
    for reversed in [false, true] {
        for opposite in [false, true] {
            let (mut app, red, blue, target) = pair(
                project.clone(),
                seed,
                reversed,
                opposite,
                false,
                INTERLEAVED_HOST_Z,
            );
            let camera = enable(&mut app);
            forward(&mut app, red, blue, 30);
            let name = format!(
                "interleaved-spawn-{}-view-{}",
                u8::from(reversed),
                u8::from(opposite)
            );
            let mut c = observe(
                &mut app,
                &target,
                (red, blue),
                &root,
                &name,
                source_over(&z, opposite),
                2,
            );
            let status = app.world().get::<ExperimentalOitStatus>(camera).unwrap();
            assert_eq!(status.rejection, None, "{status:?}");
            let allocation = app
                .world()
                .resource::<ExperimentalOitStatistics>()
                .snapshot();
            c["node_capacity"] = json!(allocation.node_capacity);
            c["head_capacity"] = json!(allocation.head_capacity);
            c["storage_bytes"] = json!(allocation.storage_bytes);
            assert_eq!(status.estimated_storage_bytes, allocation.storage_bytes);
            if !reversed && !opposite {
                c["frozen_gpu_costs"] = costs(&mut app);
            }
            // Budget rejection must remove OIT and restore the measured incorrect baseline.
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitSettings {
                    max_storage_bytes: 1,
                    ..default()
                });
            let fallback = capture(&mut app, &target);
            assert!(
                app.world()
                    .get::<ExperimentalOitStatus>(camera)
                    .unwrap()
                    .rejection
                    .is_some()
            );
            assert!(error(&fallback, source_over(&z, opposite)) > 0.1);
            let released_bytes = app
                .world()
                .resource::<ExperimentalOitStatistics>()
                .snapshot()
                .storage_bytes;
            assert!(released_bytes <= 16, "OIT buffers were not released");
            c["budget_fallback_error"] = json!(error(&fallback, source_over(&z, opposite)));
            c["released_storage_bytes"] = json!(released_bytes);
            if !reversed && !opposite {
                c["frozen_no_oit_costs"] = costs(&mut app);
            }
            fallback
                .save(root.join(format!("{name}-budget-rejected.png")))
                .unwrap();
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitSettings::default());
            let restored = capture(&mut app, &target);
            assert!(error(&restored, source_over(&z, opposite)) < 0.01);
            // Change MSAA while OIT is active: PreUpdate must remove its settings
            // before Bevy's Update-time OIT/MSAA assertion, never override the host.
            app.world_mut().entity_mut(camera).insert(Msaa::Sample4);
            let msaa = capture(&mut app, &target);
            assert_eq!(*app.world().get::<Msaa>(camera).unwrap(), Msaa::Sample4);
            assert!(
                app.world()
                    .get::<ExperimentalOitStatus>(camera)
                    .unwrap()
                    .rejection
                    .is_some()
            );
            assert!(error(&msaa, source_over(&z, opposite)) > 0.1);
            msaa.save(root.join(format!("{name}-msaa-rejected.png")))
                .unwrap();
            app.world_mut().entity_mut(camera).insert(Msaa::Off);
            let restored = capture(&mut app, &target);
            assert!(error(&restored, source_over(&z, opposite)) < 0.01);
            app.world_mut()
                .entity_mut(camera)
                .remove::<ExperimentalOitSettings>();
            let removed = capture(&mut app, &target);
            assert!(app.world().get::<ExperimentalOitStatus>(camera).is_none());
            assert!(
                app.world()
                    .get::<bevy::core_pipeline::oit::OrderIndependentTransparencySettings>(camera)
                    .is_none()
            );
            assert!(error(&removed, source_over(&z, opposite)) > 0.1);
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitSettings::default());
            let restored = capture(&mut app, &target);
            let original = image::open(root.join(format!("{name}-both.png")))
                .unwrap()
                .to_rgba8();
            c["reenabled_max_byte_error"] = json!(delta(&original, &restored).1);
            assert!(delta(&original, &restored).1 <= 1);
            restored
                .save(root.join(format!("{name}-reenabled.png")))
                .unwrap();
            c["msaa_rejection_and_removal_passed"] = json!(true);
            cases.push(c);

            let (mut app, red, blue, target) =
                pair(moving.clone(), 1, reversed, opposite, false, -2.0);
            enable(&mut app);
            for frame in [30, 90, 150] {
                forward(&mut app, red, blue, frame);
                let z: Vec<_> = depths(&moving, frame as f32 / 60.0, 1)
                    .into_iter()
                    .map(|z| z - 2.0)
                    .collect();
                let name = format!(
                    "moving-spawn-{}-view-{}-frame-{frame}",
                    u8::from(reversed),
                    u8::from(opposite)
                );
                cases.push(observe(
                    &mut app,
                    &target,
                    (red, blue),
                    &root,
                    &name,
                    source_over(&z, opposite),
                    1,
                ));
            }
        }
    }
    let passed = cases.len() == 16
        && cases
            .iter()
            .all(|c| calibrated(c) && c["source_over_max_linear_error"].as_f64().unwrap() < 0.01);
    let report = json!({"probe_passed":passed,"cases":cases,"scope":"Experimental bounded OIT, unlit LDR alpha sprite probes only. Not overflow, HDR/lit, mixed-draw or production-performance acceptance."});
    write_report(&root, &report);
    assert!(passed, "{report}");
}
