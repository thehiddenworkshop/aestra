//! Default-prepass and deliberately degraded capacity controls for shared mixed lists.
use super::*;

pub(super) fn qualify(
    app: &mut App,
    camera: Entity,
    players: [Entity; 2],
    target: &Handle<Image>,
    baseline: &RgbaImage,
    kind: MixedKind,
    root: &std::path::Path,
) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    let mesh = app
        .world_mut()
        .resource_mut::<Assets<Mesh>>()
        .add(Rectangle::new(16.0, 16.0));
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial {
            base_color: Color::linear_rgb(0.0, 0.0, 0.25),
            unlit: true,
            cull_mode: None,
            alpha_mode: AlphaMode::Opaque,
            ..default()
        });
    // The canonical camera is at -16, green at -3 and red at 0.
    let wall = app
        .world_mut()
        .spawn((
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_xyz(0.0, 0.0, -8.0),
            Visibility::Visible,
        ))
        .id();
    assert!(
        app.world()
            .get::<bevy::core_pipeline::prepass::DepthPrepass>(camera)
            .is_some()
    );
    let covered = capture(app, target);
    let sample = app
        .world()
        .resource::<ExperimentalOitDiagnostics>()
        .snapshot()
        .unwrap();
    assert_eq!(sample.camera, camera);
    assert_eq!(sample.attempted_fragments, 0);
    assert_eq!(sample.dropped_fragments, 0);
    assert_eq!(sample.over_sort_pixels, 0);
    assert_eq!(sample.invalid_links, 0);
    for entity in players {
        app.world_mut()
            .entity_mut(entity)
            .insert(Visibility::Hidden);
    }
    let wall_only = capture(app, target);
    let wall_color = center(&wall_only);
    assert!(wall_color[0] < 0.001 && wall_color[1] < 0.001);
    assert!((wall_color[2] - 0.25).abs() < 0.01);
    assert_eq!(delta(&covered, &wall_only).1, 0);
    for entity in players {
        app.world_mut()
            .entity_mut(entity)
            .insert(Visibility::Visible);
    }
    app.world_mut().entity_mut(wall).insert(Visibility::Hidden);
    let restored = capture(app, target);
    healthy(app);
    assert_eq!(delta(baseline, &restored).1, 0);
    for (suffix, image) in [
        ("covered", &covered),
        ("wall-only", &wall_only),
        ("restored", &restored),
    ] {
        image
            .save(root.join(format!("{}-depth-{suffix}.png", kind.name())))
            .unwrap();
    }
    results.push(
        json!({"path":kind.name(),"control":"opaque-default-prepass",
        "attempted_fragments":sample.attempted_fragments,"covered_max_byte_error":0,
        "restore_max_byte_error":0,"wall_linear_rgb":wall_color}),
    );

    if matches!(kind, MixedKind::SemanticAdditiveSprite) {
        // Over-sort is still diagnostic even when the two-fragment special case
        // is exact. The adapter must preserve premultiplied additive emission;
        // larger interleaved lists have a separate residual-order oracle.
        for (stage, average, sorted_max) in [
            ("global-exhausted", 0.001, 8),
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
            let image = capture(app, target);
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
                .unwrap();
            assert_eq!(sample.camera, camera);
            assert_eq!(sample.sorted_fragment_max_count, sorted_max);
            assert_eq!(sample.invalid_links, 0);
            assert!(sample.attempted_fragments > 0);
            let error = image_difference(baseline, &image);
            match stage {
                "global-exhausted" => {
                    assert!(sample.dropped_fragments > 0);
                    assert!(error > 0.01);
                }
                "local-approximation" => {
                    assert_eq!(sample.dropped_fragments, 0);
                    assert!(sample.over_sort_pixels > 0);
                    assert_eq!(sample.capped_max_depth, 2);
                    assert!(error < 0.01, "two-fragment overflow must preserve emission");
                }
                _ => {
                    healthy(app);
                    assert_eq!(delta(baseline, &image).1, 0);
                }
            }
            image
                .save(root.join(format!("{}-capacity-{stage}.png", kind.name())))
                .unwrap();
            results.push(json!({"path":kind.name(),"control":stage,
                "average":average,"sorted_max":sorted_max,"node_capacity":sample.node_capacity,
                "attempted_fragments":sample.attempted_fragments,"dropped_fragments":sample.dropped_fragments,
                "over_sort_pixels":sample.over_sort_pixels,"invalid_links":sample.invalid_links,
                "capped_max_depth":sample.capped_max_depth,"whole_image_linear_error":error,
                "expected_degradation":stage=="global-exhausted",
                "exact_two_fragment_control":stage=="local-approximation"}));
        }
    }
    results
}
