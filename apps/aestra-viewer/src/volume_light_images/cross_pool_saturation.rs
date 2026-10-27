//! Expected OIT saturation degradation, not production compositor acceptance.
use super::*;
use aestra_bevy::ExperimentalOitDiagnostics;

#[test]
#[ignore = "native OIT global-list and exact-sort saturation diagnostic; run alone"]
fn fragment_list_saturation_reports_runtime_loss_separately_from_budget_rejection() {
    let root = root("fragment-list-saturation");
    let (project, seed, z) = interleaved();
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
            assert!(
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .snapshot()
                    .is_none()
            );
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .set_enabled(true);
            forward(&mut app, red, blue, 30);
            let mut healthy_attempts = None;
            for (stage, average, sorted_max) in [
                ("healthy", 4.0, 8),
                ("global-exhausted", 0.01, 8),
                ("local-approximation", 4.0, 1),
                ("restored", 4.0, 8),
            ] {
                app.world_mut()
                    .entity_mut(camera)
                    .insert(ExperimentalOitSettings {
                        fragments_per_pixel_average: average,
                        sorted_fragment_max_count: sorted_max,
                        ..default()
                    });
                let image = capture(&mut app, &target);
                assert_eq!(
                    app.world()
                        .get::<ExperimentalOitStatus>(camera)
                        .unwrap()
                        .rejection,
                    None
                );
                let sample = app
                    .world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .snapshot()
                    .expect("GPU aggregate must complete; missing is not healthy");
                assert_eq!(sample.camera, camera);
                assert_eq!(sample.target_size, UVec2::new(480, 360));
                assert_eq!(sample.sorted_fragment_max_count, sorted_max);
                assert_eq!(sample.invalid_links, 0);
                assert!(sample.attempted_fragments > 0);
                assert!(sample.capped_max_depth <= sorted_max + 1);
                let source_error = error(&image, source_over(&z, opposite));
                match stage {
                    "global-exhausted" => {
                        assert!(sample.dropped_fragments > 0);
                        assert!(
                            source_error > 0.01,
                            "global loss must not pass the blend oracle"
                        );
                    }
                    "local-approximation" => {
                        assert_eq!(sample.dropped_fragments, 0);
                        assert!(sample.over_sort_pixels > 0);
                        assert_eq!(sample.capped_max_depth, 2);
                        assert!(
                            source_error < 0.01,
                            "overflow link sorting must preserve order"
                        );
                    }
                    _ => {
                        assert_eq!(sample.dropped_fragments, 0);
                        assert_eq!(sample.over_sort_pixels, 0);
                        assert_eq!(sample.capped_max_depth, 3);
                        assert!(source_error < 0.01);
                    }
                }
                if let Some(attempts) = healthy_attempts {
                    assert_eq!(
                        sample.attempted_fragments, attempts,
                        "capacity cannot change attempted coverage"
                    );
                } else {
                    healthy_attempts = Some(sample.attempted_fragments);
                }
                let name = format!(
                    "spawn-{}-view-{}-{stage}",
                    u8::from(reversed),
                    u8::from(opposite)
                );
                image.save(root.join(format!("{name}.png"))).unwrap();
                let adapter = app
                    .world()
                    .resource::<bevy::render::renderer::RenderAdapterInfo>();
                cases.push(json!({"name":name,"average":average,"sorted_max":sorted_max,
                    "adapter":{"name":adapter.name,"backend":format!("{:?}",adapter.backend)},
                    "source_over_max_linear_error":source_error,
                    "sequence":sample.sequence,"attempted_fragments":sample.attempted_fragments,
                    "node_capacity":sample.node_capacity,"dropped_fragments":sample.dropped_fragments,
                    "over_sort_pixels":sample.over_sort_pixels,"invalid_links":sample.invalid_links,
                    "capped_max_depth":sample.capped_max_depth}));
            }
            // Rejection invalidates prior GPU success; it must not invent a zero-drop sample.
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitSettings {
                    max_storage_bytes: 1,
                    ..default()
                });
            let _ = capture(&mut app, &target);
            assert!(
                app.world()
                    .get::<ExperimentalOitStatus>(camera)
                    .unwrap()
                    .rejection
                    .is_some()
            );
            assert!(
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .snapshot()
                    .is_none()
            );
            app.world_mut()
                .entity_mut(camera)
                .insert(ExperimentalOitSettings::default());
            let _ = capture(&mut app, &target);
            assert_eq!(
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .snapshot()
                    .unwrap()
                    .dropped_fragments,
                0
            );
            app.world()
                .resource::<ExperimentalOitDiagnostics>()
                .set_enabled(false);
            assert!(
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .snapshot()
                    .is_none()
            );
            let _ = capture(&mut app, &target);
            assert!(
                app.world()
                    .resource::<ExperimentalOitDiagnostics>()
                    .snapshot()
                    .is_none()
            );
            assert_native_transport(&app);
        }
    }
    write_report(
        &root,
        &json!({"probe_passed":true,"cases":cases,
        "scope":"Expected global-drop failure and positive local-overflow link-sort control for one full-target camera, frozen unlit LDR sprites. Historical local-approximation stage name retained. Not production smoke acceptance."}),
    );
}
