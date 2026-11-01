use super::*;

#[test]
fn water_is_opt_in_reuses_assets_and_follows_editor_seek_pause_and_restart() {
    let mut app = App::new();
    app.init_resource::<EditorSettings>()
        .insert_resource(crate::test_support::session_with_timing_slack())
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<WaterMaterial>>()
        .add_systems(Update, (sync_water, sync_reflections).chain());
    let camera = app
        .world_mut()
        .spawn(super::super::navigation::PreviewRenderCamera)
        .id();
    app.update();
    assert_eq!(app.world().resource::<Assets<WaterMaterial>>().len(), 0);
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .water_enabled = true;
    app.world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(1.0);
    app.update();
    let (entity, material) = {
        let world = app.world_mut();
        let (e, m) = world
            .query_filtered::<(Entity, &MeshMaterial3d<WaterMaterial>), With<PreviewWater>>()
            .single(world)
            .unwrap();
        (e, m.0.clone())
    };
    assert_eq!(
        app.world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .unwrap()
            .extension
            .time(),
        1.0
    );
    app.update();
    assert_eq!(app.world().resource::<Assets<WaterMaterial>>().len(), 1);
    app.world_mut()
        .resource_mut::<EditorSession>()
        .seek_time(0.0);
    app.update();
    assert_eq!(
        app.world()
            .resource::<Assets<WaterMaterial>>()
            .get(&material)
            .unwrap()
            .extension
            .time(),
        0.0
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .water_enabled = false;
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(entity),
        Some(&Visibility::Hidden)
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .water_enabled = true;
    app.update();
    assert_eq!(
        app.world().get::<Visibility>(entity),
        Some(&Visibility::Inherited)
    );
    assert_eq!(app.world().resource::<Assets<WaterMaterial>>().len(), 1);
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 1);
    use aestra_bevy_render::preview::planar::PlanarReflection;
    assert!(app.world().get::<PlanarReflection>(camera).is_none());
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .water_reflections = true;
    app.update();
    assert_eq!(
        app.world()
            .get::<PlanarReflection>(camera)
            .unwrap()
            .receiver,
        entity
    );
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .water_enabled = false;
    app.update();
    assert!(app.world().get::<PlanarReflection>(camera).is_none());
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .preview
        .water_enabled = true;
    app.update();
    assert_eq!(
        app.world()
            .get::<PlanarReflection>(camera)
            .unwrap()
            .receiver,
        entity
    );
}
