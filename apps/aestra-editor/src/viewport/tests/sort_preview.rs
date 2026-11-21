use super::*;
use std::sync::Arc;

fn spawn_effect(
    app: &mut App,
    effect: &aestra_runtime::CompiledEffect,
    name: &str,
    position: Vec3,
    layer: usize,
) -> Entity {
    let effect = Arc::new(aestra_runtime::CompiledEffect {
        name: name.into(),
        ..effect.clone()
    });
    app.world_mut()
        .spawn((
            PreviewPresentedEffect,
            PresentedEffect::new(effect),
            GlobalTransform::from_translation(position),
            InheritedVisibility::VISIBLE,
            RenderLayers::layer(layer),
        ))
        .id()
}

#[test]
fn readout_reorders_with_y_preview_and_uses_the_active_scene_camera_layers() {
    let mut app = App::new();
    app.insert_resource(Localizer::new("en-US").unwrap())
        .insert_resource(PreviewCameraController {
            xy: true,
            ..default()
        })
        .insert_resource(PreviewDisplayState {
            show_sort_order: true,
            ..default()
        })
        .add_systems(Update, (sync_preview_sort_policy, update_rows).chain());
    let normal = app
        .world_mut()
        .spawn((Camera::default(), PreviewXyCamera, RenderLayers::layer(0)))
        .id();
    let pixel = app
        .world_mut()
        .spawn((
            Camera {
                is_active: false,
                ..default()
            },
            PixelSceneCamera,
            RenderLayers::layer(3),
        ))
        .id();
    let row = app.world_mut().spawn((SortRows, Text::default())).id();
    let session = crate::test_support::session_with_timing_slack();
    let effect = session.preview().unwrap().effect();
    let upper = spawn_effect(&mut app, effect, "Upper", Vec3::new(0.0, 6.0, 3.0), 0);
    let lower = spawn_effect(&mut app, effect, "Lower", Vec3::new(0.0, -6.0, 2.0), 0);
    spawn_effect(&mut app, effect, "Pixel only", Vec3::ZERO, 3);
    let hidden = spawn_effect(&mut app, effect, "Hidden", Vec3::ZERO, 0);
    app.world_mut()
        .entity_mut(hidden)
        .insert(InheritedVisibility::HIDDEN);
    app.world_mut()
        .entity_mut(upper)
        .insert(AestraSort2d::Custom(5.0));
    let lines = |app: &App| {
        let text = &app.world().get::<Text>(row).unwrap().0;
        text.lines()
            .map(|line| line.replace(['\u{2068}', '\u{2069}'], ""))
            .collect::<Vec<_>>()
    };

    app.update();
    let world_keys = lines(&app);
    assert_eq!(world_keys.len(), 2);
    assert!(world_keys[0].contains("5.00 · Custom · Layers 0 · Upper"));
    assert!(world_keys[1].contains("2.00 · World Z · Layers 0 · Lower"));

    app.world_mut()
        .resource_mut::<PreviewDisplayState>()
        .preview_y_sort = true;
    app.update();
    let y_keys = lines(&app);
    assert!(y_keys[0].contains("8.00 · World Y · Layers 0 · Lower"));
    assert!(y_keys[1].contains("-3.00 · World Y · Layers 0 · Upper"));

    app.world_mut().get_mut::<Camera>(normal).unwrap().is_active = false;
    app.world_mut().get_mut::<Camera>(pixel).unwrap().is_active = true;
    app.update();
    let pixel_keys = lines(&app);
    assert_eq!(pixel_keys.len(), 1);
    assert!(pixel_keys[0].contains("Layers 3 · Pixel only"));

    app.world_mut().resource_mut::<PreviewCameraController>().xy = false;
    app.update();
    assert_eq!(
        app.world().get::<AestraSort2d>(upper),
        Some(&AestraSort2d::Custom(5.0))
    );
    assert!(app.world().get::<AestraSort2d>(lower).is_none());
}
